//! End-to-end execution pipeline for MCA LOD baking.
//!
//! Orchestrates region discovery, parallel decompress-parse-voxelize pipelines,
//! zero-copy Level 0 LOD assembly, multi-level octree downsampling (with Metal GPU/SIMD),
//! and atomic SQLite transaction commits.

use crate::config::{CliConfig, OutputFormat};
use anyhow::{bail, Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use mca_parser::{decompress_chunk_payload, parse_chunk_nbt, with_decompress_scratch, McaRegion};
use rayon::prelude::*;
use std::collections::BTreeSet;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;
use storage_sqlite::{ChunkHashEntry, DhSqliteBatchWriter};
use storage_voxy::{
    downsample_parent, EncodedVoxySection, VoxyMappings, VoxySection, VoxySectionBuilder,
    VoxyStorageWriter,
};
use voxelizer::{ChunkVoxelGrid, LodSection};

#[cfg(all(target_os = "macos", feature = "metal"))]
use voxelizer::{is_metal_gpu_available, metal_downsample_quadrant};

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
    /// Dimension-specific region directory identity used to prevent cross-dimension merges.
    pub region_root: String,
}

/// Executes the selected LOD generation pipeline according to CLI configuration.
pub fn run_pipeline(cfg: CliConfig) -> Result<()> {
    match cfg.format {
        OutputFormat::Dh => run_dh_pipeline(cfg),
        OutputFormat::Voxy => run_voxy_pipeline(cfg),
    }
}

fn run_dh_pipeline(cfg: CliConfig) -> Result<()> {
    let start_total = Instant::now();
    let output = cfg.resolved_output();
    let thread_count = cfg.threads.unwrap_or_else(num_cpus);

    // Initialize Rayon thread pool if explicit threads requested
    if let Some(t) = cfg.threads {
        rayon::ThreadPoolBuilder::new()
            .num_threads(t)
            .build_global()
            .ok();
    }

    #[cfg(all(target_os = "macos", feature = "metal"))]
    let gpu_accelerated = is_metal_gpu_available();
    #[cfg(not(all(target_os = "macos", feature = "metal")))]
    let gpu_accelerated = false;

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
        println!("Destination DB   : {}", output.display());
        println!("Center Position  : ({}, {}) [Blocks]", cfg.cx, cfg.cz);
        println!(
            "Radius           : {} Chunks ({} Blocks)",
            cfg.radius,
            cfg.radius * 16
        );
        println!("Concurrency      : {} Worker Threads", thread_count);
        println!(
            "Hardware Accel   : {}",
            if gpu_accelerated {
                "Apple Metal MPS (Zero-Copy Unified GPU)"
            } else {
                "Host CPU SIMD (AVX2 / NEON)"
            }
        );
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

    // Stage 2: Parallel MCA Decompress + NBT Parse + Zero-Copy Region-Local LOD 0 Assembly
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
    let parsed_results: Vec<(Vec<LodSection>, Vec<ChunkHashEntry>)> = region_tasks
        .into_par_iter()
        .map(|task| {
            let mut local_sections: Vec<Option<LodSection>> = (0..64).map(|_| None).collect();
            let mut hashes = Vec::new();
            let mut chunk_count = 0;

            let region_res = if let Some(ref d) = task.data {
                McaRegion::from_bytes(d.clone(), task.rx, task.rz)
            } else if let Some(ref p) = task.path {
                McaRegion::open(p, task.rx, task.rz)
            } else {
                return (Vec::new(), hashes);
            };

            let region = match region_res {
                Ok(r) => r,
                Err(_) => return (Vec::new(), hashes),
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
                    let mut voxel_grid = ChunkVoxelGrid::from_chunk_data(&chunk_nbt);
                    chunk_count += 1;

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

                    // Fast Zero-Copy Placement into 8x8 LOD 0 Section
                    let local_sec_x = loc.local_x >> 2;
                    let local_sec_z = loc.local_z >> 2;
                    let sec_idx = local_sec_x + local_sec_z * 8;
                    let abs_sec_x = task.rx * 8 + (local_sec_x as i32);
                    let abs_sec_z = task.rz * 8 + (local_sec_z as i32);

                    let sec = local_sections[sec_idx]
                        .get_or_insert_with(|| LodSection::new_empty(0, abs_sec_x, abs_sec_z));

                    if voxel_grid.min_y < sec.min_y || sec.min_y == 0 {
                        sec.min_y = voxel_grid.min_y;
                    }
                    if voxel_grid.max_y > sec.max_y {
                        sec.max_y = voxel_grid.max_y;
                    }

                    let chunk_rel_x = loc.local_x & 3;
                    let chunk_rel_z = loc.local_z & 3;

                    for cz in 0..16 {
                        for cx in 0..16 {
                            let col_idx = cx + cz * 16;
                            let block_x = chunk_rel_x * 16 + cx;
                            let block_z = chunk_rel_z * 16 + cz;
                            let grid_idx = block_x * 64 + block_z;

                            sec.columns[grid_idx] =
                                std::mem::take(&mut voxel_grid.columns[col_idx].points);
                        }
                    }
                }
            }

            counter_ref.fetch_add(chunk_count, Ordering::Relaxed);
            if let Some(ref bar) = pb {
                bar.inc(1);
            }

            let valid_sections: Vec<LodSection> = local_sections.into_iter().flatten().collect();
            (valid_sections, hashes)
        })
        .collect();

    if let Some(bar) = pb {
        bar.finish_and_clear();
    }

    let mut all_lod_sections = Vec::new();
    let mut all_chunk_hashes = Vec::new();
    for (secs, hashes) in parsed_results {
        all_lod_sections.extend(secs);
        all_chunk_hashes.extend(hashes);
    }

    let parsed_chunks = total_chunks_counter.load(Ordering::Relaxed);
    let parse_time = t1.elapsed().as_secs_f64();
    let chunks_per_sec = (parsed_chunks as f64) / parse_time.max(0.00001);

    if !cfg.quiet {
        println!(
            "Stage 2 [Voxelize]  : Processed {} chunks ({} Level-0 LOD nodes) in {:.3}s ({:.0} chunks/sec)",
            parsed_chunks,
            all_lod_sections.len(),
            parse_time,
            chunks_per_sec
        );
    }

    // Stage 3: Multi-Level Octree Hierarchical Downsampling
    let t2 = Instant::now();

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
                #[cfg(all(target_os = "macos", feature = "metal"))]
                {
                    if gpu_accelerated {
                        metal_downsample_quadrant(lvl, px, pz, &children)
                    } else {
                        LodSection::downsample_from_children(lvl, px, pz, &children)
                    }
                }
                #[cfg(not(all(target_os = "macos", feature = "metal")))]
                {
                    LodSection::downsample_from_children(lvl, px, pz, &children)
                }
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
    let mut writer = DhSqliteBatchWriter::open_or_create(&output)
        .with_context(|| format!("Failed to open destination database: {}", output.display()))?;

    writer.write_batch_with_level(&all_lod_sections, &all_chunk_hashes, cfg.zstd_level)?;
    writer.finish()?;

    let storage_time = t3.elapsed();
    let total_time = start_total.elapsed().as_secs_f64();
    let file_size_bytes = std::fs::metadata(&output).map(|m| m.len()).unwrap_or(0);

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

fn run_voxy_pipeline(cfg: CliConfig) -> Result<()> {
    let start_total = Instant::now();
    let output = cfg.resolved_output();
    let staging_output = prepare_voxy_staging_path(&output)?;
    let thread_count = cfg.threads.unwrap_or_else(num_cpus);
    if let Some(threads) = cfg.threads {
        let _ = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build_global();
    }

    let center_chunk_x = cfg.cx.div_euclid(16);
    let center_chunk_z = cfg.cz.div_euclid(16);
    let min_cx = center_chunk_x - cfg.radius;
    let max_cx = center_chunk_x + cfg.radius;
    let min_cz = center_chunk_z - cfg.radius;
    let max_cz = center_chunk_z + cfg.radius;

    if !cfg.quiet {
        println!(
            "================================================================================"
        );
        println!("                            MINECRAFT-LOD-ENGINE");
        println!("                     Native Voxy Server Pre-Baker");
        println!(
            "================================================================================"
        );
        println!("Source Map       : {}", cfg.map.display());
        println!("Destination Store: {}", output.display());
        println!("Output Format    : Voxy RocksDB (Hierarchy 0..=4)");
        println!("Concurrency      : {} Worker Threads", thread_count);
        println!("Zstandard Level  : {}", cfg.zstd_level);
        println!(
            "--------------------------------------------------------------------------------"
        );
    }

    // Pass 1 establishes deterministic, contiguous mapping IDs before any section is encoded.
    let palette_start = Instant::now();
    let palette_tasks = discover_regions(&cfg.map, min_cx, max_cx, min_cz, max_cz)?;
    validate_single_voxy_dimension(&palette_tasks)?;
    if palette_tasks.is_empty() {
        if !cfg.quiet {
            println!("Notice: No MCA region files overlap the requested bounds.");
        }
        return Ok(());
    }
    let scans: Vec<Result<PaletteScan>> = palette_tasks
        .into_par_iter()
        .map(|task| scan_voxy_palette(task, min_cx, max_cx, min_cz, max_cz))
        .collect();
    let mut block_states = BTreeSet::new();
    let mut biomes = BTreeSet::new();
    let mut palette_chunks = 0usize;
    for scan in scans {
        let scan = scan?;
        block_states.extend(scan.block_states);
        biomes.extend(scan.biomes);
        palette_chunks += scan.chunks;
    }
    let mappings = Arc::new(VoxyMappings::build(block_states, biomes)?);
    if !cfg.quiet {
        println!(
            "Pass 1 [Mappings]  : Scanned {} chunks; {} block states and {} biomes in {:.3}s",
            palette_chunks,
            mappings.block_state_count(),
            mappings.biome_count(),
            palette_start.elapsed().as_secs_f64()
        );
    }

    // Pass 2 builds complete Voxy hierarchy levels 0..=4. A bounded channel keeps
    // RocksDB writes serialized while region voxelization remains parallel.
    let encode_start = Instant::now();
    let encode_tasks = discover_regions(&cfg.map, min_cx, max_cx, min_cz, max_cz)?;
    let writer = match VoxyStorageWriter::create(&staging_output, &mappings) {
        Ok(writer) => writer,
        Err(error) => {
            let _ = std::fs::remove_dir_all(&staging_output);
            return Err(error);
        }
    };
    let (sender, receiver) = std::sync::mpsc::sync_channel::<Vec<EncodedVoxySection>>(8);
    let writer_thread = std::thread::spawn(move || -> Result<usize> {
        let mut writer = writer;
        while let Ok(batch) = receiver.recv() {
            for section in batch {
                writer.write_section(section)?;
            }
        }
        let count = writer.written_sections();
        writer.finish()?;
        Ok(count)
    });

    let parsed_chunks = Arc::new(AtomicUsize::new(0));
    let producer_result: Result<()> = encode_tasks.into_par_iter().try_for_each(|task| {
        process_voxy_region(
            task,
            &mappings,
            &sender,
            &parsed_chunks,
            min_cx,
            max_cx,
            min_cz,
            max_cz,
            cfg.zstd_level,
        )
    });
    drop(sender);
    let writer_result = match writer_thread.join() {
        Ok(result) => result,
        Err(_) => Err(anyhow::anyhow!("Voxy RocksDB writer thread panicked")),
    };
    let written_sections = match producer_result.and(writer_result) {
        Ok(count) => count,
        Err(error) => {
            let _ = std::fs::remove_dir_all(&staging_output);
            return Err(error);
        }
    };
    commit_voxy_staging(&staging_output, &output)?;

    let parsed_chunks = parsed_chunks.load(Ordering::Relaxed);
    let total_time = start_total.elapsed().as_secs_f64();
    if !cfg.quiet {
        println!(
            "Pass 2 [Hierarchy] : Encoded {} chunks into {} Voxy sections (levels 0..=4) in {:.3}s",
            parsed_chunks,
            written_sections,
            encode_start.elapsed().as_secs_f64()
        );
        println!(
            "--------------------------------------------------------------------------------"
        );
        println!(
            "Pipeline Result  : SUCCESS (Total: {:.3}s | {:.0} chunks/sec)",
            total_time,
            parsed_chunks as f64 / total_time.max(0.00001)
        );
        println!(
            "================================================================================"
        );
    }
    Ok(())
}

fn prepare_voxy_staging_path(output: &Path) -> Result<PathBuf> {
    if output.exists() {
        if !output.is_dir() {
            bail!(
                "Voxy output exists and is not a directory: {}",
                output.display()
            );
        }
        if std::fs::read_dir(output)?.next().is_some() {
            bail!("Voxy output directory is not empty: {}", output.display());
        }
    }

    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = output
        .file_name()
        .and_then(|name| name.to_str())
        .context("Voxy output path must have a valid UTF-8 directory name")?;
    let staging = parent.join(format!(".{name}.mca2lod-staging-{}", std::process::id()));
    if staging.exists() {
        bail!(
            "Voxy staging directory already exists from another or interrupted run: {}",
            staging.display()
        );
    }
    Ok(staging)
}

fn commit_voxy_staging(staging: &Path, output: &Path) -> Result<()> {
    if output.exists() {
        if !output.is_dir() || std::fs::read_dir(output)?.next().is_some() {
            bail!(
                "Voxy output changed while generation was running; completed staging data remains at {}",
                staging.display()
            );
        }
        std::fs::remove_dir(output)?;
    }
    std::fs::rename(staging, output).with_context(|| {
        format!(
            "failed to atomically publish Voxy storage {} to {}",
            staging.display(),
            output.display()
        )
    })
}

fn validate_single_voxy_dimension(tasks: &[RegionTask]) -> Result<()> {
    let region_directories: BTreeSet<_> =
        tasks.iter().map(|task| task.region_root.as_str()).collect();
    if region_directories.len() > 1 {
        let directories = region_directories
            .iter()
            .copied()
            .collect::<Vec<_>>()
            .join(", ");
        bail!(
            "Voxy storage is dimension-specific, but multiple region directories were found: {directories}. Pass one dimension directory at a time."
        );
    }
    Ok(())
}

struct PaletteScan {
    block_states: BTreeSet<mca_parser::BlockStateIdentity>,
    biomes: BTreeSet<String>,
    chunks: usize,
}

fn scan_voxy_palette(
    task: RegionTask,
    min_cx: i32,
    max_cx: i32,
    min_cz: i32,
    max_cz: i32,
) -> Result<PaletteScan> {
    let region = open_region_task(task)?;
    let mut block_states = BTreeSet::new();
    let mut biomes = BTreeSet::new();
    let mut chunks = 0usize;

    for location in region.iter_present_chunks() {
        let chunk_x = region.region_x * 32 + location.local_x as i32;
        let chunk_z = region.region_z * 32 + location.local_z as i32;
        if chunk_x < min_cx || chunk_x > max_cx || chunk_z < min_cz || chunk_z > max_cz {
            continue;
        }
        let Some(chunk) = parse_region_chunk(&region, &location, chunk_x, chunk_z) else {
            continue;
        };
        chunks += 1;
        for section in chunk.sections {
            block_states.extend(section.palette);
            biomes.extend(section.biomes);
        }
    }
    Ok(PaletteScan {
        block_states,
        biomes,
        chunks,
    })
}

#[allow(clippy::too_many_arguments)]
fn process_voxy_region(
    task: RegionTask,
    mappings: &VoxyMappings,
    sender: &std::sync::mpsc::SyncSender<Vec<EncodedVoxySection>>,
    parsed_chunks: &AtomicUsize,
    min_cx: i32,
    max_cx: i32,
    min_cz: i32,
    max_cz: i32,
    zstd_level: i32,
) -> Result<()> {
    let region = open_region_task(task)?;
    let mut batch = Vec::with_capacity(64);
    {
        let mut emit = |section: &VoxySection| -> Result<()> {
            batch.push(section.encode(zstd_level)?);
            if batch.len() >= 64 {
                let ready = std::mem::replace(&mut batch, Vec::with_capacity(64));
                sender.send(ready).map_err(|_| {
                    anyhow::anyhow!("Voxy writer stopped before encoding completed")
                })?;
            }
            Ok(())
        };

        build_voxy_tile(
            &region,
            4,
            0,
            0,
            mappings,
            parsed_chunks,
            min_cx,
            max_cx,
            min_cz,
            max_cz,
            &mut emit,
        )?;
    }
    if !batch.is_empty() {
        sender
            .send(batch)
            .map_err(|_| anyhow::anyhow!("Voxy writer stopped before encoding completed"))?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn build_voxy_tile<F>(
    region: &McaRegion,
    level: u8,
    local_chunk_x: usize,
    local_chunk_z: usize,
    mappings: &VoxyMappings,
    parsed_chunks: &AtomicUsize,
    min_cx: i32,
    max_cx: i32,
    min_cz: i32,
    max_cz: i32,
    emit: &mut F,
) -> Result<Vec<VoxySection>>
where
    F: FnMut(&VoxySection) -> Result<()>,
{
    if level == 0 {
        let mut builders = ahash::AHashMap::<i32, VoxySectionBuilder>::new();
        for dz in 0..2usize {
            for dx in 0..2usize {
                let lx = local_chunk_x + dx;
                let lz = local_chunk_z + dz;
                let chunk_x = region.region_x * 32 + lx as i32;
                let chunk_z = region.region_z * 32 + lz as i32;
                if chunk_x < min_cx || chunk_x > max_cx || chunk_z < min_cz || chunk_z > max_cz {
                    continue;
                }
                let Some(location) = region.get_chunk_location(lx, lz) else {
                    continue;
                };
                let Some(chunk) = parse_region_chunk(region, &location, chunk_x, chunk_z) else {
                    continue;
                };
                parsed_chunks.fetch_add(1, Ordering::Relaxed);
                for section in &chunk.sections {
                    if section.is_empty_air || section.palette.is_empty() {
                        continue;
                    }
                    let section_y = (section.y as i32).div_euclid(2);
                    builders
                        .entry(section_y)
                        .or_insert_with(|| {
                            VoxySectionBuilder::new(
                                chunk_x.div_euclid(2),
                                section_y,
                                chunk_z.div_euclid(2),
                            )
                        })
                        .ingest_section(chunk_x, chunk_z, section, mappings);
                }
            }
        }

        let mut sections: Vec<_> = builders
            .into_values()
            .filter_map(VoxySectionBuilder::finish)
            .collect();
        sections.sort_unstable_by_key(VoxySection::y);
        for section in &sections {
            emit(section)?;
        }
        return Ok(sections);
    }

    let child_span_chunks = 1usize << level;
    let mut children = Vec::new();
    for dz in 0..2usize {
        for dx in 0..2usize {
            children.extend(build_voxy_tile(
                region,
                level - 1,
                local_chunk_x + dx * child_span_chunks,
                local_chunk_z + dz * child_span_chunks,
                mappings,
                parsed_chunks,
                min_cx,
                max_cx,
                min_cz,
                max_cz,
                emit,
            )?);
        }
    }

    let mut groups = ahash::AHashMap::<(i32, i32, i32), Vec<usize>>::new();
    for (index, child) in children.iter().enumerate() {
        groups
            .entry((
                child.x().div_euclid(2),
                child.y().div_euclid(2),
                child.z().div_euclid(2),
            ))
            .or_default()
            .push(index);
    }

    let mut parents = Vec::with_capacity(groups.len());
    for ((x, y, z), indices) in groups {
        let child_refs: Vec<_> = indices.iter().map(|&index| &children[index]).collect();
        if let Some(parent) = downsample_parent(level, x, y, z, &child_refs, mappings)? {
            emit(&parent)?;
            parents.push(parent);
        }
    }
    parents.sort_unstable_by_key(VoxySection::y);
    Ok(parents)
}

fn open_region_task(task: RegionTask) -> Result<McaRegion> {
    match (task.data, task.path) {
        (Some(data), _) => McaRegion::from_bytes(data, task.rx, task.rz)
            .context("failed to open in-memory MCA region"),
        (None, Some(path)) => McaRegion::open(&path, task.rx, task.rz)
            .with_context(|| format!("failed to open MCA region {}", path.display())),
        (None, None) => bail!("region task has neither a path nor in-memory data"),
    }
}

fn parse_region_chunk(
    region: &McaRegion,
    location: &mca_parser::ChunkLocation,
    chunk_x: i32,
    chunk_z: i32,
) -> Option<mca_parser::ChunkData> {
    let (payload, compression) = region.get_raw_chunk_payload(location).ok()?;
    with_decompress_scratch(|scratch| {
        decompress_chunk_payload(payload, compression, scratch).ok()?;
        parse_chunk_nbt(scratch, chunk_x, chunk_z).ok()
    })
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
                            region_root: name
                                .rsplit_once('/')
                                .map(|(parent, _)| parent.to_string())
                                .unwrap_or_else(|| "region".to_string()),
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
        } else if p.is_file()
            && p.extension().is_some_and(|ext| ext == "mca")
            && p.parent()
                .and_then(Path::file_name)
                .is_some_and(|name| name == "region")
        {
            let filename = p.file_name().unwrap_or_default().to_string_lossy();
            if let Some((rx, rz)) = parse_region_coords(&filename) {
                if rx >= min_rx && rx <= max_rx && rz >= min_rz && rz <= max_rz {
                    let region_root = p
                        .parent()
                        .map(|parent| parent.display().to_string())
                        .unwrap_or_else(|| "region".to_string());
                    tasks.push(RegionTask {
                        rx,
                        rz,
                        path: Some(p),
                        data: None,
                        region_root,
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
