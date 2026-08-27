//! Voxel column extraction and run-length vertical compression.
//!
//! Converts dense 3D chunk voxel grids (16x16xY) into run-length encoded
//! vertical columns storing continuous height segments with consolidated colors.

use crate::palette_lut::{GlobalPaletteLut, FLAG_AIR};
use mca_parser::ChunkData;

/// Vertical column voxel run segment (12 bytes, Copy, zero heap allocations).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColumnVoxelPoint {
    /// Bottom Y coordinate of continuous segment.
    pub y_min: i16,
    /// Top Y coordinate of continuous segment.
    pub y_max: i16,
    /// 32-bit ARGB packed color.
    pub color: u32,
    /// Material property bitflags.
    pub flags: u16,
}

/// A single vertical X/Z column containing sorted run segments.
#[derive(Debug, Clone)]
pub struct ColumnData {
    /// Continuous vertical voxel runs from bottom to top.
    pub points: Vec<ColumnVoxelPoint>,
}

/// 16x16 column voxel representation of a single chunk.
#[derive(Debug, Clone)]
pub struct ChunkVoxelGrid {
    /// Chunk coordinate X.
    pub chunk_x: i32,
    /// Chunk coordinate Z.
    pub chunk_z: i32,
    /// Minimum non-air block Y coordinate.
    pub min_y: i16,
    /// Maximum non-air block Y coordinate.
    pub max_y: i16,
    /// Flat array of 256 columns (x + z * 16).
    pub columns: [ColumnData; 256],
}

impl ChunkVoxelGrid {
    /// Constructs a `ChunkVoxelGrid` by parsing and compressing 3D section voxel data.
    pub fn from_chunk_data(chunk: &ChunkData) -> Self {
        let lut = GlobalPaletteLut::get_global();
        let mut min_chunk_y: i16 = 320;
        let mut max_chunk_y: i16 = -64;

        // Initialize 256 columns with small pre-allocated capacity
        let mut columns: Vec<ColumnData> = (0..256)
            .map(|_| ColumnData {
                points: Vec::with_capacity(8),
            })
            .collect();

        // Filter and sort non-empty sections by Y ascending
        let mut active_sections: Vec<&mca_parser::SectionData> =
            chunk.sections.iter().filter(|s| !s.is_empty_air).collect();
        active_sections.sort_by_key(|s| s.y);

        for section in active_sections {
            let sec_y_base = (section.y as i16) * 16;
            let palette_materials: Vec<_> = section
                .palette
                .iter()
                .map(|name| lut.get_material_by_name(name))
                .collect();

            // Fast-path: single uniform solid material across whole 16x16x16 section
            if palette_materials.len() == 1 {
                let mat = &palette_materials[0];
                if (mat.flags & FLAG_AIR) == 0 && mat.base_color != 0 {
                    let sec_y_end = sec_y_base + 15;
                    if sec_y_base < min_chunk_y {
                        min_chunk_y = sec_y_base;
                    }
                    if sec_y_end > max_chunk_y {
                        max_chunk_y = sec_y_end;
                    }
                    let pt = ColumnVoxelPoint {
                        y_min: sec_y_base,
                        y_max: sec_y_end,
                        color: mat.base_color,
                        flags: mat.flags,
                    };
                    for col in columns.iter_mut() {
                        col.points.push(pt);
                    }
                    continue;
                }
            }

            for z in 0..16 {
                for x in 0..16 {
                    let col_idx = x + z * 16;
                    let mut has_run = false;
                    let mut cur_run_color: u32 = 0;
                    let mut cur_run_flags: u16 = 0;
                    let mut cur_run_min_y: i16 = 0;
                    let mut cur_run_max_y: i16 = 0;

                    for y_rel in 0..16 {
                        let block_idx = (y_rel * 256) + (z * 16) + x;
                        let pal_idx = section.block_indices[block_idx] as usize;
                        let mat = if pal_idx < palette_materials.len() {
                            &palette_materials[pal_idx]
                        } else if !palette_materials.is_empty() {
                            &palette_materials[0]
                        } else {
                            continue;
                        };

                        if (mat.flags & FLAG_AIR) != 0 || mat.base_color == 0 {
                            // Air block finishes any current run
                            if has_run {
                                columns[col_idx].points.push(ColumnVoxelPoint {
                                    y_min: cur_run_min_y,
                                    y_max: cur_run_max_y,
                                    color: cur_run_color,
                                    flags: cur_run_flags,
                                });
                                has_run = false;
                            }
                            continue;
                        }

                        let abs_y = sec_y_base + y_rel as i16;
                        if abs_y < min_chunk_y {
                            min_chunk_y = abs_y;
                        }
                        if abs_y > max_chunk_y {
                            max_chunk_y = abs_y;
                        }

                        if has_run && cur_run_color == mat.base_color && cur_run_flags == mat.flags
                        {
                            // Extend current run
                            cur_run_max_y = abs_y;
                        } else {
                            // Flush previous run
                            if has_run {
                                columns[col_idx].points.push(ColumnVoxelPoint {
                                    y_min: cur_run_min_y,
                                    y_max: cur_run_max_y,
                                    color: cur_run_color,
                                    flags: cur_run_flags,
                                });
                            }
                            // Start new run
                            has_run = true;
                            cur_run_color = mat.base_color;
                            cur_run_flags = mat.flags;
                            cur_run_min_y = abs_y;
                            cur_run_max_y = abs_y;
                        }
                    }

                    // Flush end of section run
                    if has_run {
                        columns[col_idx].points.push(ColumnVoxelPoint {
                            y_min: cur_run_min_y,
                            y_max: cur_run_max_y,
                            color: cur_run_color,
                            flags: cur_run_flags,
                        });
                    }
                }
            }
        }

        // Merge adjacent runs across sections in each column
        let mut fixed_columns: [ColumnData; 256] =
            std::array::from_fn(|_| ColumnData { points: Vec::new() });
        for (i, col) in columns.into_iter().enumerate() {
            let mut merged: Vec<ColumnVoxelPoint> = Vec::with_capacity(col.points.len());
            for pt in col.points {
                if let Some(last) = merged.last_mut() {
                    if last.color == pt.color
                        && last.flags == pt.flags
                        && last.y_max + 1 == pt.y_min
                    {
                        last.y_max = pt.y_max;
                        continue;
                    }
                }
                merged.push(pt);
            }
            fixed_columns[i] = ColumnData { points: merged };
        }

        if min_chunk_y > max_chunk_y {
            min_chunk_y = 0;
            max_chunk_y = 0;
        }

        ChunkVoxelGrid {
            chunk_x: chunk.chunk_x,
            chunk_z: chunk.chunk_z,
            min_y: min_chunk_y,
            max_y: max_chunk_y,
            columns: fixed_columns,
        }
    }
}
