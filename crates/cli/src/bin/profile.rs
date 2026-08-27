//! Microsecond profiler for measuring fine-grained execution latency across all pipeline stages.

use mca_parser::{decompress_chunk_payload, parse_chunk_nbt, with_decompress_scratch, McaRegion};
use rayon::prelude::*;
use std::path::Path;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;
use storage_sqlite::{
    encode_lod_section_to_dh_blob_with_level, ChunkHashEntry, DhSqliteBatchWriter,
};
use voxelizer::{is_metal_gpu_available, metal_downsample_quadrant, ChunkVoxelGrid, LodSection};

fn main() -> anyhow::Result<()> {
    let region_dir = Path::new("sample_world/dimensions/minecraft/overworld/region");
    if !region_dir.exists() {
        eprintln!("sample_world not found at {}", region_dir.display());
        std::process::exit(1);
    }

    let gpu_ok = is_metal_gpu_available();

    println!("================================================================================");
    println!("             MINECRAFT-LOD-ENGINE FINE-GRAINED MICRO-PROFILER");
    println!(
        " Hardware Acceleration: {}",
        if gpu_ok {
            "Apple Metal MPS (Unified Memory GPU)"
        } else {
            "Host CPU SIMD"
        }
    );
    println!("================================================================================");

    let t_total = Instant::now();

    // 1. Stage 1 Discovery
    let t_disc = Instant::now();
    let mut region_files = Vec::new();
    for entry in std::fs::read_dir(region_dir)? {
        let p = entry?.path();
        if p.extension().is_some_and(|ext| ext == "mca") {
            let fname = p.file_name().unwrap().to_string_lossy();
            if let Some((rx, rz)) = parse_region_coords(&fname) {
                region_files.push((rx, rz, p));
            }
        }
    }
    let disc_elapsed = t_disc.elapsed();
    println!(
        "Stage 1 [Discovery]: Found {} regions in {:.3}ms",
        region_files.len(),
        disc_elapsed.as_secs_f64() * 1000.0
    );

    // Micro timers for Stage 2 (in nanoseconds)
    let time_mmap_ns = Arc::new(AtomicU64::new(0));
    let time_decompress_ns = Arc::new(AtomicU64::new(0));
    let time_nbt_parse_ns = Arc::new(AtomicU64::new(0));
    let time_voxelize_ns = Arc::new(AtomicU64::new(0));
    let time_hash_ns = Arc::new(AtomicU64::new(0));
    let total_chunks = Arc::new(AtomicUsize::new(0));

    let t2 = Instant::now();
    let parsed_results: Vec<(Vec<LodSection>, Vec<ChunkHashEntry>)> = region_files
        .into_par_iter()
        .map(|(rx, rz, p)| {
            let mut local_sections: Vec<Option<LodSection>> = (0..64).map(|_| None).collect();
            let mut hashes = Vec::new();
            let mut chunk_count = 0;

            let t_m0 = Instant::now();
            let region = match McaRegion::open(&p, rx, rz) {
                Ok(r) => r,
                Err(_) => return (Vec::new(), hashes),
            };
            time_mmap_ns.fetch_add(t_m0.elapsed().as_nanos() as u64, Ordering::Relaxed);

            let chunks = region.iter_present_chunks();

            for loc in chunks {
                let chunk_x = rx * 32 + (loc.local_x as i32);
                let chunk_z = rz * 32 + (loc.local_z as i32);

                let (raw_payload, comp_type) = match region.get_raw_chunk_payload(&loc) {
                    Ok(p) => p,
                    Err(_) => continue,
                };

                let mut nbt_buf = Vec::new();
                let t_dec = Instant::now();
                let dec_ok = with_decompress_scratch(|scratch| {
                    if decompress_chunk_payload(raw_payload, comp_type, scratch).is_ok() {
                        nbt_buf.extend_from_slice(scratch);
                        true
                    } else {
                        false
                    }
                });
                time_decompress_ns.fetch_add(t_dec.elapsed().as_nanos() as u64, Ordering::Relaxed);

                if !dec_ok {
                    continue;
                }

                let t_nbt = Instant::now();
                let chunk_data = parse_chunk_nbt(&nbt_buf, chunk_x, chunk_z);
                time_nbt_parse_ns.fetch_add(t_nbt.elapsed().as_nanos() as u64, Ordering::Relaxed);

                if let Ok(chunk_nbt) = chunk_data {
                    let t_vox = Instant::now();
                    let mut voxel_grid = ChunkVoxelGrid::from_chunk_data(&chunk_nbt);
                    time_voxelize_ns
                        .fetch_add(t_vox.elapsed().as_nanos() as u64, Ordering::Relaxed);
                    chunk_count += 1;

                    let t_h = Instant::now();
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
                    time_hash_ns.fetch_add(t_h.elapsed().as_nanos() as u64, Ordering::Relaxed);

                    // Zero-copy local assembly
                    let local_sec_x = loc.local_x >> 2;
                    let local_sec_z = loc.local_z >> 2;
                    let sec_idx = local_sec_x + local_sec_z * 8;
                    let abs_sec_x = rx * 8 + (local_sec_x as i32);
                    let abs_sec_z = rz * 8 + (local_sec_z as i32);

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

            total_chunks.fetch_add(chunk_count, Ordering::Relaxed);
            let valid_sections: Vec<LodSection> = local_sections.into_iter().flatten().collect();
            (valid_sections, hashes)
        })
        .collect();

    let stage2_wall = t2.elapsed();
    let num_chunks = total_chunks.load(Ordering::Relaxed);

    let mut all_lod_sections = Vec::new();
    let mut all_hashes = Vec::new();
    for (secs, h) in parsed_results {
        all_lod_sections.extend(secs);
        all_hashes.extend(h);
    }

    println!("\nStage 2 [Parallel MCA Parsing, Voxelization & Zero-Copy Level 0 Assembly] (Wall Time: {:.3}s | {:.0} chunks/s):",
             stage2_wall.as_secs_f64(), (num_chunks as f64) / stage2_wall.as_secs_f64());
    println!(
        "  ├─ Memory-Map Overhead (CPU Time) : {:>7.2} ms",
        (time_mmap_ns.load(Ordering::Relaxed) as f64) / 1_000_000.0
    );
    println!(
        "  ├─ Chunk Decompression (CPU Time) : {:>7.2} ms",
        (time_decompress_ns.load(Ordering::Relaxed) as f64) / 1_000_000.0
    );
    println!(
        "  ├─ FastNBT Parsing     (CPU Time) : {:>7.2} ms",
        (time_nbt_parse_ns.load(Ordering::Relaxed) as f64) / 1_000_000.0
    );
    println!(
        "  ├─ Voxel Column RLE    (CPU Time) : {:>7.2} ms",
        (time_voxelize_ns.load(Ordering::Relaxed) as f64) / 1_000_000.0
    );
    println!(
        "  ├─ Block Hashing       (CPU Time) : {:>7.2} ms",
        (time_hash_ns.load(Ordering::Relaxed) as f64) / 1_000_000.0
    );
    println!(
        "  └─ Level 0 LOD Nodes (Zero-Copy)  : {:>7} sections (Built in parallel in Stage 2!)",
        all_lod_sections.len()
    );

    // Stage 3 Octree Downsampling Breakdown
    println!("\nStage 3 [Multi-Level Octree Hierarchical Downsampling]:");
    let t3 = Instant::now();

    for lvl in 1..=10 {
        let t_lvl = Instant::now();
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
                if gpu_ok {
                    metal_downsample_quadrant(lvl, px, pz, &children)
                } else {
                    LodSection::downsample_from_children(lvl, px, pz, &children)
                }
            })
            .collect();

        let lvl_elapsed = t_lvl.elapsed();
        println!(
            "  ├─ Level {:<2} (Downsample from L{})   : {:>6} sections in {:>7.2} ms",
            lvl,
            lvl - 1,
            new_parent_sections.len(),
            lvl_elapsed.as_secs_f64() * 1000.0
        );
        all_lod_sections.extend(new_parent_sections);
    }
    let stage3_wall = t3.elapsed();
    println!(
        "  └─ Total Stage 3 Wall Time         : {:.3} s (Total LOD Nodes: {})",
        stage3_wall.as_secs_f64(),
        all_lod_sections.len()
    );

    // Stage 4 Storage Breakdown
    println!("\nStage 4 [SQLite Serialization & Atomic Commit]:");
    let t_enc = Instant::now();
    let encoded: Vec<_> = all_lod_sections
        .par_iter()
        .map(|sec| encode_lod_section_to_dh_blob_with_level(sec, 3))
        .collect::<Result<Vec<_>, _>>()?;
    let enc_elapsed = t_enc.elapsed();
    println!(
        "  ├─ Parallel Zstd & VarInt Encode   : {:>7.2} ms ({:.2} MB)",
        enc_elapsed.as_secs_f64() * 1000.0,
        (encoded.iter().map(|e| e.data_blob.len()).sum::<usize>() as f64) / (1024.0 * 1024.0)
    );

    let t_db = Instant::now();
    let temp_db = std::env::temp_dir().join(format!("profile_dh_{}.sqlite", std::process::id()));
    let _ = std::fs::remove_file(&temp_db);
    let mut writer = DhSqliteBatchWriter::open_or_create(&temp_db)?;
    writer.write_batch_with_level(&all_lod_sections, &all_hashes, 3)?;
    writer.finish()?;
    let db_elapsed = t_db.elapsed();
    let _ = std::fs::remove_file(&temp_db);
    println!(
        "  └─ SQLite Transaction Disk Flush   : {:>7.2} ms",
        db_elapsed.as_secs_f64() * 1000.0
    );

    println!("\n================================================================================");
    println!(
        "PROFILING SUMMARY (Total Chunks: {}, Total Time: {:.3}s | {:.0} chunks/sec)",
        num_chunks,
        t_total.elapsed().as_secs_f64(),
        (num_chunks as f64) / t_total.elapsed().as_secs_f64()
    );
    println!("================================================================================");

    Ok(())
}

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
