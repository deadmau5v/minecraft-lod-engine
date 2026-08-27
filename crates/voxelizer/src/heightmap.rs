//! Voxel column extraction and run-length vertical compression.
//!
//! Converts dense 3D chunk voxel grids (16x16xY) into run-length encoded
//! vertical columns storing continuous height segments with consolidated colors.

use crate::palette_lut::{GlobalPaletteLut, FLAG_AIR};
use mca_parser::ChunkData;

/// Vertical column voxel run segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnVoxelPoint {
    /// Bottom Y coordinate of continuous segment.
    pub y_min: i16,
    /// Top Y coordinate of continuous segment.
    pub y_max: i16,
    /// 32-bit ARGB packed color.
    pub color: u32,
    /// Canonical Minecraft block state name.
    pub block_name: String,
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

        // Initialize 256 columns
        let mut columns: Vec<ColumnData> = (0..256)
            .map(|_| ColumnData { points: Vec::new() })
            .collect();

        // Sort sections by Y ascending
        let mut sorted_sections = chunk.sections.clone();
        sorted_sections.sort_by_key(|s| s.y);

        for section in &sorted_sections {
            let sec_y_base = (section.y as i16) * 16;
            let palette_materials: Vec<_> = section
                .palette
                .iter()
                .map(|name| {
                    let mat = lut.get_material_by_name(name);
                    (name.clone(), mat)
                })
                .collect();

            for z in 0..16 {
                for x in 0..16 {
                    let col_idx = x + z * 16;
                    let mut cur_run_name: Option<String> = None;
                    let mut cur_run_color: u32 = 0;
                    let mut cur_run_flags: u16 = 0;
                    let mut cur_run_min_y: i16 = 0;
                    let mut cur_run_max_y: i16 = 0;

                    for y_rel in 0..16 {
                        let block_idx = (y_rel * 256) + (z * 16) + x;
                        let pal_idx = section.block_indices[block_idx] as usize;
                        let (name, mat) = if pal_idx < palette_materials.len() {
                            &palette_materials[pal_idx]
                        } else if !palette_materials.is_empty() {
                            &palette_materials[0]
                        } else {
                            continue;
                        };

                        if (mat.flags & FLAG_AIR) != 0 || mat.base_color == 0 {
                            // Air block finishes any current run
                            if let Some(run_name) = cur_run_name.take() {
                                columns[col_idx].points.push(ColumnVoxelPoint {
                                    y_min: cur_run_min_y,
                                    y_max: cur_run_max_y,
                                    color: cur_run_color,
                                    block_name: run_name,
                                    flags: cur_run_flags,
                                });
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

                        match cur_run_name.as_ref() {
                            Some(existing_name) if existing_name == name => {
                                // Extend current run
                                cur_run_max_y = abs_y;
                            }
                            _ => {
                                // Flush previous run
                                if let Some(run_name) = cur_run_name.take() {
                                    columns[col_idx].points.push(ColumnVoxelPoint {
                                        y_min: cur_run_min_y,
                                        y_max: cur_run_max_y,
                                        color: cur_run_color,
                                        block_name: run_name,
                                        flags: cur_run_flags,
                                    });
                                }
                                // Start new run
                                cur_run_name = Some(name.clone());
                                cur_run_color = mat.base_color;
                                cur_run_flags = mat.flags;
                                cur_run_min_y = abs_y;
                                cur_run_max_y = abs_y;
                            }
                        }
                    }

                    // Flush end of section run
                    if let Some(run_name) = cur_run_name.take() {
                        columns[col_idx].points.push(ColumnVoxelPoint {
                            y_min: cur_run_min_y,
                            y_max: cur_run_max_y,
                            color: cur_run_color,
                            block_name: run_name,
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
                    if last.block_name == pt.block_name && last.y_max + 1 == pt.y_min {
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
