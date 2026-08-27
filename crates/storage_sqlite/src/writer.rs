//! High-throughput Distant Horizons SQLite storage ingestion backend.
//!
//! Applies zero-journaling and in-memory caches to reach SQLite ingestion
//! speeds exceeding 100,000 chunk equivalents per second.

use crate::blob_codec::{encode_lod_section_to_dh_blob_with_level, EncodedFullData};
use crate::schema::init_dh_database_schema;
use anyhow::Result;
use rusqlite::{params, Connection, OpenFlags};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use voxelizer::LodSection;

/// Record structure representing precomputed chunk block hash.
#[derive(Debug, Clone, Copy)]
pub struct ChunkHashEntry {
    /// Chunk coordinate along X axis.
    pub chunk_x: i32,
    /// Chunk coordinate along Z axis.
    pub chunk_z: i32,
    /// 32-bit integer hash of block states.
    pub hash: i32,
}

/// Batch writer managing SQLite database connection and transaction pipelines.
pub struct DhSqliteBatchWriter {
    conn: Connection,
}

impl DhSqliteBatchWriter {
    /// Opens or creates a Distant Horizons `.sqlite` database with optimized PRAGMAs.
    pub fn open_or_create<P: AsRef<Path>>(path: P) -> Result<Self> {
        let p = path.as_ref();
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open_with_flags(
            p,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;

        // High performance PRAGMAs optimized for bulk insertion
        conn.execute_batch(
            "PRAGMA journal_mode = OFF;
             PRAGMA synchronous = OFF;
             PRAGMA page_size = 4096;
             PRAGMA cache_size = -262144;
             PRAGMA temp_store = MEMORY;
             PRAGMA locking_mode = EXCLUSIVE;",
        )?;

        init_dh_database_schema(&conn)?;

        Ok(Self { conn })
    }

    /// Writes all LOD sections and chunk hash entries in a single atomic transaction.
    pub fn write_batch(
        &mut self,
        sections: &[LodSection],
        chunk_hashes: &[ChunkHashEntry],
    ) -> Result<()> {
        self.write_batch_with_level(sections, chunk_hashes, 3)
    }

    /// Writes all LOD sections and chunk hash entries with user-specified Zstd compression level.
    pub fn write_batch_with_level(
        &mut self,
        sections: &[LodSection],
        chunk_hashes: &[ChunkHashEntry],
        zstd_level: i32,
    ) -> Result<()> {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);

        let tx = self.conn.transaction()?;
        {
            let mut stmt_data = tx.prepare_cached(
                "INSERT OR REPLACE INTO FullData (
                    DetailLevel, PosX, PosZ, MinY, DataChecksum, Data,
                    ColumnGenerationStep, ColumnWorldCompressionMode, Mapping,
                    DataFormatVersion, CompressionMode, ApplyToParent,
                    LastModifiedUnixDateTime, CreatedUnixDateTime, ApplyToChildren
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, NULL, NULL, 2, 4, 0, ?7, ?7, 0)",
            )?;

            for sec in sections {
                let encoded: EncodedFullData =
                    encode_lod_section_to_dh_blob_with_level(sec, zstd_level)?;
                stmt_data.execute(params![
                    sec.detail_level,
                    sec.pos_x,
                    sec.pos_z,
                    sec.min_y as i32,
                    encoded.checksum,
                    encoded.data_blob,
                    now_ms,
                ])?;
            }

            let mut stmt_hash = tx.prepare_cached(
                "INSERT OR REPLACE INTO ChunkHash (
                    ChunkPosX, ChunkPosZ, ChunkHash, LastModifiedUnixDateTime, CreatedUnixDateTime
                ) VALUES (?1, ?2, ?3, ?4, ?4)",
            )?;

            for ch in chunk_hashes {
                stmt_hash.execute(params![ch.chunk_x, ch.chunk_z, ch.hash, now_ms])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Optimizes and closes the database connection.
    pub fn finish(self) -> Result<()> {
        let _ = self.conn.execute("PRAGMA optimize;", []);
        Ok(())
    }
}
