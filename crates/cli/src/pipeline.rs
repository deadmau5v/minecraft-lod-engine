//! End-to-end execution pipeline for MCA LOD baking.
//!
//! Orchestrates region discovery, parallel decompress-parse-voxelize pipelines,
//! multi-level octree downsampling, and atomic SQLite transaction commits.

use crate::config::CliConfig;
use anyhow::{bail, Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use mca_parser::{decompress_chunk_payload, parse_chunk_nbt, with_decompress_scratch, McaRegion};
use rayon::prelude::*;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;
use storage_sqlite::{ChunkHashEntry, DhSqliteBatchWriter};
use voxelizer::{ChunkVoxelGrid, LodSection};

/// Work descriptor for a single Minecraft region file (`r.X.Z.mca`).
pub struct RegionTask {
    /// Region coordinate along X axis (1 region = 32 chunks = 512 blocks).
    pub rx: i32,
    /// Region coordinate along Z axis.
    pub rz: i32,
    /// File path on disk (if reading from filesystem directory).
    pub path: Option<PathBuf>,
    /// In-memory bytes (if extracted from .zip archive).
    pub data: Option<Vec<u8>>,
}

/// Executes the full LOD generation pipeline according to CLI configuration.
pub fn run_pipeline(cfg: CliConfig) -> Result<()> {
    let start_total = Instant::now();
    let thread_count = cfg.threads.unwrap_or_else(num_cpus);

    // Initialize Rayon thread pool if explicit threads requested
    if let Some(t) = cfg.threads {
        rayon::ThreadPoolBuilder::new()
            .num_threads(t)
            .build_global()
            .ok();
    }

    if !cfg.quiet {
        println!(
            "================================================================================"
        );
        println!("                            MINECRAFT-LOD-ENGINE");
        println!("                 High-Performance Headless LOD Pre-Baker");
        println!(
            "================================================================================"
        );
        println!("Source Map       : {}", cfg.map.display());
        println!("Destination DB   : {}", cfg.output.display());
        println!("Center Position  : ({}, {}) [Blocks]", cfg.cx, cfg.cz);
        println!(
            "Radius           : {} Chunks ({} Blocks)",
            cfg.radius,
            cfg.radius * 16
        );
        println!("Concurrency      : {} Worker Threads", thread_count);
        println!("LOD Detail Depth : 0..={}", cfg.detail_levels);
        println!("Zstandard Level  : {}", cfg.zstd_level);
        println!(
            "--------------------------------------------------------------------------------"
        );
    }

    let center_chunk_x = cfg.cx.div_euclid(16);
    let center_chunk_z = cfg.cz.div_euclid(16);
    let r = cfg.radius;
    let min_cx = center_chunk_x - r;
    let max_cx = center_chunk_x + r;
    let min_cz = center_chunk_z - r;
    let max_cz = center_chunk_z + r;

    // Stage 1: Region Discovery
    let t0 = Instant::now();
    let region_tasks = discover_regions(&cfg.map, min_cx, max_cx, min_cz, max_cz)?;
    let discovery_time = t0.elapsed();

    if !cfg.quiet {
        println!(
            "Stage 1 [Discovery] : Identified {} candidate MCA region files in {:.2}ms",
            region_tasks.len(),
            discovery_time.as_secs_f64() * 1000.0
        );
    }

    if region_tasks.is_empty() {
        if !cfg.quiet {
            println!(
                "Notice: No MCA region files found overlapping bounding box [{}, {}] to [{}, {}].",
                min_cx, min_cz, max_cx, max_cz
            );
        }
        return Ok(());
    }

    // Stage 2: Parallel MCA Decompress + NBT Parse + Voxelize
    let t1 = Instant::now();
    let total_chunks_counter = Arc::new(AtomicUsize::new(0));

    let pb = if !cfg.quiet {
        let bar = ProgressBar::new(region_tasks.len() as u64);
        bar.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} regions ({eta})")
                .unwrap_or_else(|_| ProgressStyle::default_bar()),
        );
        Some(bar)
    } else {
        None
    };

    let counter_ref = Arc::clone(&total_chunks_counter);
    let parsed_results: Vec<(Vec<ChunkVoxelGrid>, Vec<ChunkHashEntry>)> = region_tasks
        .into_par_iter()
        .map(|task| {
            let mut grids = Vec::new();
            let mut hashes = Vec::new();

            let region_res = if let Some(ref d) = task.data {
                McaRegion::from_bytes(d.clone(), task.rx, task.rz)
            } else if let Some(ref p) = task.path {
                McaRegion::open(p, task.rx, task.rz)
            } else {
                return (grids, hashes);
            };

            let region = match region_res {
                Ok(r) => r,
                Err(_) => return (grids, hashes),
            };

            let present_chunks = region.iter_present_chunks();

            for loc in present_chunks {
                let chunk_x = task.rx * 32 + (loc.local_x as i32);
                let chunk_z = task.rz * 32 + (loc.local_z as i32);

                if chunk_x < min_cx || chunk_x > max_cx || chunk_z < min_cz || chunk_z > max_cz {
                    continue;
                }

                let payload_res = region.get_raw_chunk_payload(&loc);
                let (raw_payload, comp_type) = match payload_res {
                    Ok(p) => p,
                    Err(_) => continue,
                };

                let parse_res = with_decompress_scratch(|scratch| {
                    if decompress_chunk_payload(raw_payload, comp_type, scratch).is_err() {
                        return None;
                    }
                    parse_chunk_nbt(scratch, chunk_x, chunk_z).ok()
                });

                if let Some(chunk_nbt) = parse_res {
                    let voxel_grid = ChunkVoxelGrid::from_chunk_data(&chunk_nbt);
                    grids.push(voxel_grid);

                    // Deterministic 32-bit chunk state hash
                    let mut h: i32 = 1;
                    for s in &chunk_nbt.sections {
                        for &idx in &s.block_indices {
                            h = h.wrapping_mul(31).wrapping_add(idx as i32);
                        }
                    }
                    hashes.push(ChunkHashEntry {
                        chunk_x,
                        chunk_z,
                        hash: h,
                    });
                }
            }

            counter_ref.fetch_add(grids.len(), Ordering::Relaxed);
            if let Some(ref bar) = pb {
                bar.inc(1);
            }
            (grids, hashes)
        })
        .collect();

    if let Some(bar) = pb {
        bar.finish_and_clear();
    }

    let mut all_chunk_grids = Vec::new();
    let mut all_chunk_hashes = Vec::new();
    for (grids, hashes) in parsed_results {
        all_chunk_grids.extend(grids);
        all_chunk_hashes.extend(hashes);
    }

    let parsed_chunks = all_chunk_grids.len();
    let parse_time = t1.elapsed().as_secs_f64();
    let chunks_per_sec = (parsed_chunks as f64) / parse_time.max(0.00001);

    if !cfg.quiet {
        println!(
            "Stage 2 [Voxelize]  : Processed {} chunks in {:.3}s ({:.0} chunks/sec)",
            parsed_chunks, parse_time, chunks_per_sec
        );
    }

    // Stage 3: Multi-Level Octree Hierarchical Downsampling
    let t2 = Instant::now();
    let mut all_lod_sections: Vec<LodSection> = Vec::new();

    // Base level (LOD 0)
    let mut level_0_sections = LodSection::build_level_0(&all_chunk_grids);
    all_lod_sections.append(&mut level_0_sections);

    // Hierarchical downsampling for detail levels 1..=detail_levels
    for lvl in 1..=cfg.detail_levels {
        let prev_level_sections: Vec<&LodSection> = all_lod_sections
            .iter()
            .filter(|s| s.detail_level == lvl - 1)
            .collect();

        let mut parent_groups: ahash::AHashMap<(i32, i32), Vec<&LodSection>> =
            ahash::AHashMap::new();
        for child in prev_level_sections {
            let px = child.pos_x >> 1;
            let pz = child.pos_z >> 1;
            parent_groups.entry((px, pz)).or_default().push(child);
        }

        let parent_groups_vec: Vec<((i32, i32), Vec<&LodSection>)> =
            parent_groups.into_iter().collect();

        let new_parent_sections: Vec<LodSection> = parent_groups_vec
            .into_par_iter()
            .map(|((px, pz), children)| {
                LodSection::downsample_from_children(lvl, px, pz, &children)
            })
            .collect();

        all_lod_sections.extend(new_parent_sections);
    }

    let octree_time = t2.elapsed();
    if !cfg.quiet {
        println!(
            "Stage 3 [Octree]    : Built {} LOD nodes across levels 0..={} in {:.2}ms",
            all_lod_sections.len(),
            cfg.detail_levels,
            octree_time.as_secs_f64() * 1000.0
        );
    }

    // Stage 4: SQLite Database Serialization and Atomic Ingestion
    let t3 = Instant::now();
    let mut writer = DhSqliteBatchWriter::open_or_create(&cfg.output).with_context(|| {
        format!(
            "Failed to open destination database: {}",
            cfg.output.display()
        )
    })?;

    writer.write_batch_with_level(&all_lod_sections, &all_chunk_hashes, cfg.zstd_level)?;
    writer.finish()?;

    let storage_time = t3.elapsed();
    let total_time = start_total.elapsed().as_secs_f64();
    let file_size_bytes = std::fs::metadata(&cfg.output).map(|m| m.len()).unwrap_or(0);

    if !cfg.quiet {
        println!(
            "Stage 4 [Ingestion] : Flushed {} sections to SQLite in {:.2}ms ({:.2} MB)",
            all_lod_sections.len(),
            storage_time.as_secs_f64() * 1000.0,
            (file_size_bytes as f64) / (1024.0 * 1024.0)
        );
        println!(
            "--------------------------------------------------------------------------------"
        );
        println!(
            "Pipeline Result  : SUCCESS (Total: {:.3}s | End-to-End Throughput: {:.0} chunks/sec)",
            total_time,
            (parsed_chunks as f64) / total_time.max(0.00001)
        );
        println!(
            "================================================================================"
        );
    }

    Ok(())
}

/// Discovers candidate MCA files in folder or zip archive matching chunk bounding box.
fn discover_regions(
    map_path: &Path,
    min_cx: i32,
    max_cx: i32,
    min_cz: i32,
    max_cz: i32,
) -> Result<Vec<RegionTask>> {
    let mut tasks = Vec::new();

    let min_rx = min_cx.div_euclid(32);
    let max_rx = max_cx.div_euclid(32);
    let min_rz = min_cz.div_euclid(32);
    let max_rz = max_cz.div_euclid(32);

    if map_path.is_file() {
        // Zip archive format
        let file = File::open(map_path)?;
        let mut archive = zip::ZipArchive::new(file)?;

        for i in 0..archive.len() {
            let mut zip_file = archive.by_index(i)?;
            let name = zip_file.name().to_string();

            if name.ends_with(".mca") && name.contains("region/r.") {
                if let Some((rx, rz)) = parse_region_coords(&name) {
                    if rx >= min_rx && rx <= max_rx && rz >= min_rz && rz <= max_rz {
                        let mut buf = Vec::with_capacity(zip_file.size() as usize);
                        zip_file.read_to_end(&mut buf)?;
                        tasks.push(RegionTask {
                            rx,
                            rz,
                            path: None,
                            data: Some(buf),
                        });
                    }
                }
            }
        }
    } else if map_path.is_dir() {
        find_mca_files_in_dir(map_path, &mut tasks, min_rx, max_rx, min_rz, max_rz)?;
    } else {
        bail!(
            "Input map path does not exist or is not readable: {}",
            map_path.display()
        );
    }

    Ok(tasks)
}

/// Recursively scans filesystem directories for `.mca` files within bounding coordinates.
fn find_mca_files_in_dir(
    dir: &Path,
    tasks: &mut Vec<RegionTask>,
    min_rx: i32,
    max_rx: i32,
    min_rz: i32,
    max_rz: i32,
) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let p = entry.path();
        if p.is_dir() {
            find_mca_files_in_dir(&p, tasks, min_rx, max_rx, min_rz, max_rz)?;
        } else if p.is_file() && p.extension().is_some_and(|ext| ext == "mca") {
            let filename = p.file_name().unwrap_or_default().to_string_lossy();
            if let Some((rx, rz)) = parse_region_coords(&filename) {
                if rx >= min_rx && rx <= max_rx && rz >= min_rz && rz <= max_rz {
                    tasks.push(RegionTask {
                        rx,
                        rz,
                        path: Some(p),
                        data: None,
                    });
                }
            }
        }
    }
    Ok(())
}

/// Parses region coordinate tuple `(rx, rz)` from filename in `r.X.Z.mca` format.
fn parse_region_coords(filename: &str) -> Option<(i32, i32)> {
    let base = filename.split('/').next_back()?.strip_suffix(".mca")?;
    let mut parts = base.split('.');
    if parts.next()? != "r" {
        return None;
    }
    let rx: i32 = parts.next()?.parse().ok()?;
    let rz: i32 = parts.next()?.parse().ok()?;
    Some((rx, rz))
}

/// Retrieves logical CPU core count for parallel computation.
fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(32)
}
