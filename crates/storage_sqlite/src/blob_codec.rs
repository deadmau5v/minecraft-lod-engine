//! Distant Horizons binary blob codec.
//!
//! Encodes hierarchical LOD section voxel columns into Zstandard-compressed
//! byte payloads with proprietary 31-bit Adler-variant rolling checksums.

use anyhow::Result;
use byteorder::{BigEndian, WriteBytesExt};
use voxelizer::LodSection;

/// Encoded LOD payload data containing computed checksum and compressed blob.
#[derive(Debug, Clone)]
pub struct EncodedFullData {
    /// Rolling 31-bit checksum expected by Distant Horizons renderer.
    pub checksum: i32,
    /// Zstandard compressed payload byte array.
    pub data_blob: Vec<u8>,
}

/// Encodes a single integer as a variable-length byte sequence (LEB128 format).
#[inline(always)]
fn write_varint(buf: &mut Vec<u8>, mut value: u32) {
    while value >= 0x80 {
        buf.push((value as u8 & 0x7F) | 0x80);
        value >>= 7;
    }
    buf.push(value as u8);
}

/// Serializes an in-memory `LodSection` into a Distant Horizons compatible compressed blob
/// using default Zstd compression level 3.
pub fn encode_lod_section_to_dh_blob(section: &LodSection) -> Result<EncodedFullData> {
    encode_lod_section_to_dh_blob_with_level(section, 3)
}

/// Serializes an in-memory `LodSection` into a Distant Horizons compatible compressed blob
/// with a user-specified Zstandard compression level (1-22).
pub fn encode_lod_section_to_dh_blob_with_level(
    section: &LodSection,
    zstd_level: i32,
) -> Result<EncodedFullData> {
    let mut raw_bytes = Vec::with_capacity(64 * 1024);

    // 1. Column size table (4096 varints for 64x64 grid)
    for col in &section.columns {
        write_varint(&mut raw_bytes, col.len() as u32);
    }

    // 2. Continuous voxel run points
    for col in &section.columns {
        for pt in col {
            raw_bytes.write_i16::<BigEndian>(pt.y_min)?;
            raw_bytes.write_i16::<BigEndian>(pt.y_max)?;
            raw_bytes.write_u32::<BigEndian>(pt.color)?;
            raw_bytes.write_u16::<BigEndian>(pt.flags)?;
        }
    }

    // 3. Compute deterministic DH checksum
    let mut checksum: i32 = 1;
    for &b in &raw_bytes {
        checksum = checksum.wrapping_mul(31).wrapping_add(b as i8 as i32);
    }

    // 4. Compress payload with Zstandard
    let compressed = zstd::encode_all(&raw_bytes[..], zstd_level)?;

    Ok(EncodedFullData {
        checksum,
        data_blob: compressed,
    })
}
