//! Zero-copy memory-mapped Minecraft Anvil MCA Region (`.mca`) reader.
//!
//! Conforms to Minecraft Anvil file format specifications:
//! - 8KB Header (4KB chunk location table + 4KB chunk timestamp table)
//! - Variable-length chunk data sectors (4KB sector aligned).

use anyhow::{bail, Result};
use memmap2::Mmap;
use std::fs::File;
use std::path::Path;

/// Backing storage buffer for MCA region data.
pub enum RegionSource {
    /// Zero-copy OS virtual memory mapping.
    Mmap(Mmap),
    /// In-memory heap allocated buffer (e.g. extracted from .zip archive).
    Buffer(Vec<u8>),
}

impl std::ops::Deref for RegionSource {
    type Target = [u8];
    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        match self {
            RegionSource::Mmap(m) => m,
            RegionSource::Buffer(b) => b,
        }
    }
}

/// Parsed Minecraft MCA Region file handle.
pub struct McaRegion {
    source: RegionSource,
    /// Absolute Region coordinate along X axis.
    pub region_x: i32,
    /// Absolute Region coordinate along Z axis.
    pub region_z: i32,
}

/// Metadata descriptor for a chunk present inside an MCA file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkLocation {
    /// Local chunk X coordinate (0..31).
    pub local_x: usize,
    /// Local chunk Z coordinate (0..31).
    pub local_z: usize,
    /// Byte offset where chunk sector starts.
    pub sector_offset: usize,
    /// Number of 4096-byte sectors allocated.
    pub sector_count: usize,
    /// Last modified epoch timestamp in seconds.
    pub timestamp: u32,
}

impl McaRegion {
    /// Opens an `.mca` file from disk using memory-mapped I/O.
    pub fn open<P: AsRef<Path>>(path: P, region_x: i32, region_z: i32) -> Result<Self> {
        let file = File::open(path)?;
        let mmap = unsafe { Mmap::map(&file)? };
        if mmap.len() < 8192 {
            bail!("Invalid MCA file: header smaller than 8KB");
        }
        Ok(Self {
            source: RegionSource::Mmap(mmap),
            region_x,
            region_z,
        })
    }

    /// Creates an MCA Region reader from an in-memory byte buffer.
    pub fn from_bytes(bytes: Vec<u8>, region_x: i32, region_z: i32) -> Result<Self> {
        if bytes.len() < 8192 {
            bail!("Invalid MCA file: buffer smaller than 8KB");
        }
        Ok(Self {
            source: RegionSource::Buffer(bytes),
            region_x,
            region_z,
        })
    }

    /// Looks up chunk header metadata by local chunk coordinates (0..31, 0..31).
    #[inline(always)]
    pub fn get_chunk_location(&self, local_x: usize, local_z: usize) -> Option<ChunkLocation> {
        let index = (local_x & 31) + (local_z & 31) * 32;
        let data = &self.source;
        let loc_bytes = &data[index * 4..(index + 1) * 4];
        let offset = ((loc_bytes[0] as usize) << 16)
            | ((loc_bytes[1] as usize) << 8)
            | (loc_bytes[2] as usize);
        let sector_count = loc_bytes[3] as usize;

        if offset == 0 || sector_count == 0 {
            return None;
        }

        let ts_bytes = &data[4096 + index * 4..4096 + (index + 1) * 4];
        let timestamp = u32::from_be_bytes([ts_bytes[0], ts_bytes[1], ts_bytes[2], ts_bytes[3]]);

        Some(ChunkLocation {
            local_x,
            local_z,
            sector_offset: offset * 4096,
            sector_count,
            timestamp,
        })
    }

    /// Retrieves raw compressed chunk payload slice and compression scheme byte.
    #[inline(always)]
    pub fn get_raw_chunk_payload(&self, loc: &ChunkLocation) -> Result<(&[u8], u8)> {
        let data = &self.source;
        let start = loc.sector_offset;
        if start + 5 > data.len() {
            bail!("Corrupt MCA: sector offset exceeds file size");
        }
        let len = u32::from_be_bytes(data[start..start + 4].try_into()?) as usize;
        if len == 0 {
            bail!("Invalid MCA chunk payload length 0");
        }
        let compression_type = data[start + 4];
        let payload_end = start + 4 + len;
        if payload_end > data.len() {
            bail!("Corrupt MCA: payload exceeds file size");
        }
        Ok((&data[start + 5..payload_end], compression_type))
    }

    /// Collects all non-empty chunks present in this region.
    pub fn iter_present_chunks(&self) -> Vec<ChunkLocation> {
        let mut chunks = Vec::with_capacity(1024);
        for lz in 0..32 {
            for lx in 0..32 {
                if let Some(loc) = self.get_chunk_location(lx, lz) {
                    chunks.push(loc);
                }
            }
        }
        chunks
    }
}
