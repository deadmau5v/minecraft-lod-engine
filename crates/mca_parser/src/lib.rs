//! High-performance Minecraft Anvil Region (`.mca`) parser.
//!
//! Provides zero-copy memory-mapped MCA reading, SIMD-accelerated bit-array unpacking,
//! thread-local decompression pooling, and multi-version NBT deserialization.

pub mod bit_array;
pub mod decompress;
pub mod nbt;
pub mod region;

pub use bit_array::BitArrayUnpacker;
pub use decompress::{decompress_chunk_payload, with_decompress_scratch};
pub use nbt::{parse_chunk_nbt, BlockStateIdentity, ChunkData, SectionData};
pub use region::{ChunkLocation, McaRegion, RegionSource};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bit_array_unpack() {
        let mut out = [0u16; 4096];
        let packed = vec![0x12345678_9ABCDEF0i64; 256];
        BitArrayUnpacker::unpack_4096(4, &packed, &mut out);
        assert_eq!(out[0], 0);
        assert_eq!(out[1], 0xF);
        assert_eq!(out[2], 0xE);
    }

    #[test]
    fn test_bit_array_unpack_64() {
        let mut out = [0u16; 64];
        let packed = vec![0x12345678_9ABCDEF0i64; 4];
        BitArrayUnpacker::unpack_64(4, &packed, &mut out);
        assert_eq!(out[0], 0);
        assert_eq!(out[1], 0xF);
    }

    #[test]
    fn test_decompress_zlib() {
        use flate2::write::ZlibEncoder;
        use flate2::Compression;
        use std::io::Write;

        let original = b"Hello Minecraft LOD World!";
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(original).unwrap();
        let compressed = encoder.finish().unwrap();

        let mut decompressed = Vec::new();
        decompress_chunk_payload(&compressed, 2, &mut decompressed).unwrap();
        assert_eq!(&decompressed, original);
    }

    #[test]
    fn test_packed_section_light() {
        let section = SectionData {
            y: 0,
            is_empty_air: false,
            palette: Vec::new(),
            block_indices: [0; 4096],
            biomes: Vec::new(),
            biome_indices: [0; 64],
            sky_light: vec![0x21],
            block_light: vec![0x43],
        };
        assert_eq!(section.packed_light(0), 0x31);
        assert_eq!(section.packed_light(1), 0x42);
        assert_eq!(section.packed_light(2), 0x00);
    }

    #[test]
    fn test_mca_region_buffer_parser() {
        // Construct a synthetic 8KB+ header
        let mut synthetic_mca = vec![0u8; 8192 + 4096];
        // Set chunk at (0, 0): offset = 2 (sectors), length = 1
        synthetic_mca[0] = 0;
        synthetic_mca[1] = 0;
        synthetic_mca[2] = 2;
        synthetic_mca[3] = 1;

        // Set payload at offset 2 * 4096 = 8192
        // Length field in MCA is (1 byte compression_type + data_length)
        let payload_len: u32 = 11;
        synthetic_mca[8192..8196].copy_from_slice(&payload_len.to_be_bytes());
        synthetic_mca[8196] = 3; // uncompressed
        synthetic_mca[8197..8207].copy_from_slice(b"0123456789");

        let region = McaRegion::from_bytes(synthetic_mca, 0, 0).unwrap();
        let loc = region.get_chunk_location(0, 0).expect("Chunk should exist");
        assert_eq!(loc.sector_offset, 8192);

        let (payload, comp) = region.get_raw_chunk_payload(&loc).unwrap();
        assert_eq!(comp, 3);
        assert_eq!(payload, b"0123456789");
    }
}
