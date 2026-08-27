//! Apple Silicon Metal Compute Shader Pipeline for LOD Downsampling.
//!
//! Utilizes Apple Unified Memory Architecture (zero PCIe copies) and SIMD hardware
//! execution units for GPU-accelerated spatial box filtering and multi-level Octree generation.

#[cfg(all(target_os = "macos", feature = "metal"))]
use metal::*;
#[cfg(all(target_os = "macos", feature = "metal"))]
use std::sync::OnceLock;

use crate::heightmap::ColumnVoxelPoint;
use crate::octree::LodSection;

#[cfg(all(target_os = "macos", feature = "metal"))]
static METAL_STATE: OnceLock<Option<MetalEngine>> = OnceLock::new();

#[cfg(all(target_os = "macos", feature = "metal"))]
struct MetalEngine {
    device: Device,
    command_queue: CommandQueue,
    pipeline_state: ComputePipelineState,
}

#[cfg(all(target_os = "macos", feature = "metal"))]
const MSL_SOURCE: &str = r#"
#include <metal_stdlib>
using namespace metal;

struct GpuPoint {
    short y_min;
    short y_max;
    uint color;
    uint flags;
    uint count;
};

kernel void downsample_quadrant_2x2(
    device const GpuPoint* in_child_cols   [[buffer(0)]],
    device GpuPoint*       out_parent_cols [[buffer(1)]],
    constant uint&         child_quadrant  [[buffer(2)]],
    uint2                  gid             [[thread_position_in_grid]]
) {
    if (gid.x >= 32 || gid.y >= 32) return;

    uint px = gid.x;
    uint pz = gid.y;

    uint c_x0 = px * 2;
    uint c_z0 = pz * 2;

    uint idx00 = c_x0 * 64 + c_z0;
    uint idx01 = c_x0 * 64 + (c_z0 + 1);
    uint idx10 = (c_x0 + 1) * 64 + c_z0;
    uint idx11 = (c_x0 + 1) * 64 + (c_z0 + 1);

    GpuPoint p00 = in_child_cols[idx00];
    GpuPoint p01 = in_child_cols[idx01];
    GpuPoint p10 = in_child_cols[idx10];
    GpuPoint p11 = in_child_cols[idx11];

    short min_y = 32767;
    short max_y = -32768;
    uint sum_a = 0, sum_r = 0, sum_g = 0, sum_b = 0;
    uint count = 0;
    uint flags = 0;

    GpuPoint samples[4] = {p00, p01, p10, p11};
    for (int i = 0; i < 4; ++i) {
        if (samples[i].count > 0) {
            min_y = min(min_y, samples[i].y_min);
            max_y = max(max_y, samples[i].y_max);
            uint c = samples[i].color;
            uint a = (c >> 24) & 0xFF;
            if (a > 0) {
                sum_a += a;
                sum_r += (c >> 16) & 0xFF;
                sum_g += (c >> 8) & 0xFF;
                sum_b += c & 0xFF;
                count++;
            }
            if (flags == 0) flags = samples[i].flags;
        }
    }

    uint out_color = 0;
    if (count > 0) {
        uint avg_a = sum_a / count;
        uint avg_r = sum_r / count;
        uint avg_g = sum_g / count;
        uint avg_b = sum_b / count;
        out_color = (avg_a << 24) | (avg_r << 16) | (avg_g << 8) | avg_b;
    }

    uint rel_qx = child_quadrant & 1;
    uint rel_qz = (child_quadrant >> 1) & 1;
    uint out_x = rel_qx * 32 + px;
    uint out_z = rel_qz * 32 + pz;
    uint out_idx = out_x * 64 + out_z;

    GpuPoint res;
    res.y_min = (count > 0) ? min_y : 0;
    res.y_max = (count > 0) ? max_y : 0;
    res.color = out_color;
    res.flags = flags;
    res.count = (count > 0) ? 1 : 0;

    out_parent_cols[out_idx] = res;
}
"#;

#[cfg(all(target_os = "macos", feature = "metal"))]
impl MetalEngine {
    fn new() -> Option<Self> {
        let device = Device::system_default()?;
        let command_queue = device.new_command_queue();

        let options = CompileOptions::new();
        let library = device.new_library_with_source(MSL_SOURCE, &options).ok()?;
        let kernel = library.get_function("downsample_quadrant_2x2", None).ok()?;
        let pipeline_state = device
            .new_compute_pipeline_state_with_function(&kernel)
            .ok()?;

        Some(Self {
            device,
            command_queue,
            pipeline_state,
        })
    }
}

/// Checks whether Apple Silicon Metal GPU acceleration is available on this system.
pub fn is_metal_gpu_available() -> bool {
    #[cfg(all(target_os = "macos", feature = "metal"))]
    {
        METAL_STATE.get_or_init(MetalEngine::new).is_some()
    }
    #[cfg(not(all(target_os = "macos", feature = "metal")))]
    {
        false
    }
}

/// GPU-accelerated downsampler running on Apple Silicon GPU using unified zero-copy memory.
#[cfg(all(target_os = "macos", feature = "metal"))]
pub fn metal_downsample_quadrant(
    parent_detail_level: u8,
    parent_pos_x: i32,
    parent_pos_z: i32,
    children: &[&LodSection],
) -> LodSection {
    let engine = match METAL_STATE.get_or_init(MetalEngine::new) {
        Some(e) => e,
        None => {
            return LodSection::downsample_from_children(
                parent_detail_level,
                parent_pos_x,
                parent_pos_z,
                children,
            )
        }
    };

    let mut parent = LodSection::new_empty(parent_detail_level, parent_pos_x, parent_pos_z);

    for child in children {
        if child.min_y < parent.min_y || parent.min_y == 0 {
            parent.min_y = child.min_y;
        }
        if child.max_y > parent.max_y {
            parent.max_y = child.max_y;
        }

        let rel_child_x = (child.pos_x & 1).rem_euclid(2) as u32;
        let rel_child_z = (child.pos_z & 1).rem_euclid(2) as u32;
        let child_quadrant = rel_child_x | (rel_child_z << 1);

        // Convert child top points on heap to prevent thread stack overflow
        let mut child_gpu_cols = vec![GpuPointFlat::default(); 4096];
        for (i, col) in child.columns.iter().enumerate() {
            if let Some(pt) = col.last() {
                child_gpu_cols[i] = GpuPointFlat {
                    y_min: pt.y_min,
                    y_max: pt.y_max,
                    color: pt.color,
                    flags: pt.flags as u32,
                    count: 1,
                };
            }
        }

        let child_buf = engine.device.new_buffer_with_data(
            child_gpu_cols.as_ptr() as *const _,
            (4096 * std::mem::size_of::<GpuPointFlat>()) as u64,
            MTLResourceOptions::StorageModeShared,
        );

        let parent_buf = engine.device.new_buffer(
            (4096 * std::mem::size_of::<GpuPointFlat>()) as u64,
            MTLResourceOptions::StorageModeShared,
        );

        let quad_buf = engine.device.new_buffer_with_data(
            &child_quadrant as *const u32 as *const _,
            4,
            MTLResourceOptions::StorageModeShared,
        );

        let cmd_buffer = engine.command_queue.new_command_buffer();
        let encoder = cmd_buffer.new_compute_command_encoder();
        encoder.set_compute_pipeline_state(&engine.pipeline_state);
        encoder.set_buffer(0, Some(&child_buf), 0);
        encoder.set_buffer(1, Some(&parent_buf), 0);
        encoder.set_buffer(2, Some(&quad_buf), 0);

        let threadgroup_size = MTLSize {
            width: 16,
            height: 16,
            depth: 1,
        };
        let grid_size = MTLSize {
            width: 32,
            height: 32,
            depth: 1,
        };

        encoder.dispatch_threads(grid_size, threadgroup_size);
        encoder.end_encoding();
        cmd_buffer.commit();
        cmd_buffer.wait_until_completed();

        // Read results directly from unified memory zero-copy buffer
        let out_ptr = parent_buf.contents() as *const GpuPointFlat;
        unsafe {
            for pz in 0..32 {
                for px in 0..32 {
                    let out_x = (rel_child_x as usize) * 32 + px;
                    let out_z = (rel_child_z as usize) * 32 + pz;
                    let out_idx = out_x * 64 + out_z;
                    let res = *out_ptr.add(out_idx);

                    if res.count > 0 {
                        parent.columns[out_idx] = vec![ColumnVoxelPoint {
                            y_min: res.y_min,
                            y_max: res.y_max,
                            color: res.color,
                            flags: res.flags as u16,
                        }];
                    }
                }
            }
        }
    }

    parent
}

#[repr(C, align(16))]
#[derive(Debug, Clone, Copy, Default)]
struct GpuPointFlat {
    y_min: i16,
    y_max: i16,
    color: u32,
    flags: u32,
    count: u32,
}
