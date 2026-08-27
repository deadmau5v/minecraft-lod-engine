//! Packed bit array decoders for Minecraft BlockState and Biome palettes.
//!
//! Implements both modern (1.16+ word-aligned non-spanning) and legacy (1.13-1.15 spanning)
//! bit packing specifications with minimal branching.

/// Bit-array unpacker utility.
pub struct BitArrayUnpacker;

impl BitArrayUnpacker {
    /// Unpacks 16x16x16 (4096 blocks) packed bit array into 4096 palette indices.
    #[inline(always)]
    pub fn unpack_4096(bits_per_block: usize, packed: &[i64], out_indices: &mut [u16; 4096]) {
        if bits_per_block == 0 || packed.is_empty() {
            out_indices.fill(0);
            return;
        }

        let mask = (1u64 << bits_per_block) - 1;
        let blocks_per_long = 64 / bits_per_block;

        // In 1.16+, entries do not cross long boundaries:
        let expected_longs_1_16 = 4096_usize.div_ceil(blocks_per_long);

        if packed.len() >= expected_longs_1_16 {
            let mut block_idx = 0;
            for &raw_long in packed {
                let mut val = raw_long as u64;
                for _ in 0..blocks_per_long {
                    if block_idx >= 4096 {
                        return;
                    }
                    out_indices[block_idx] = (val & mask) as u16;
                    val >>= bits_per_block;
                    block_idx += 1;
                }
            }
        } else {
            // 1.13 - 1.15 Spanning BitArray unpacking
            let mut bit_idx: usize = 0;
            for out_slot in out_indices.iter_mut() {
                let start_long = bit_idx / 64;
                let start_offset = bit_idx % 64;
                let end_long = (bit_idx + bits_per_block - 1) / 64;

                let val = if start_long == end_long {
                    if start_long < packed.len() {
                        ((packed[start_long] as u64) >> start_offset) & mask
                    } else {
                        0
                    }
                } else {
                    let mut combined = 0u64;
                    if start_long < packed.len() {
                        combined = (packed[start_long] as u64) >> start_offset;
                    }
                    if end_long < packed.len() {
                        let shift = 64 - start_offset;
                        combined |= (packed[end_long] as u64) << shift;
                    }
                    combined & mask
                };

                *out_slot = (val & mask) as u16;
                bit_idx += bits_per_block;
            }
        }
    }

    /// Unpacks 4x4x4 (64 biomes) packed bit array into 64 biome palette indices.
    #[inline(always)]
    pub fn unpack_64(bits_per_biome: usize, packed: &[i64], out_indices: &mut [u16; 64]) {
        if bits_per_biome == 0 || packed.is_empty() {
            out_indices.fill(0);
            return;
        }

        let mask = (1u64 << bits_per_biome) - 1;
        let biomes_per_long = 64 / bits_per_biome;
        let mut idx = 0;

        for &raw_long in packed {
            let mut val = raw_long as u64;
            for _ in 0..biomes_per_long {
                if idx >= 64 {
                    return;
                }
                out_indices[idx] = (val & mask) as u16;
                val >>= bits_per_biome;
                idx += 1;
            }
        }
    }
}
