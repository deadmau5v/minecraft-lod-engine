//! High-throughput SQLite storage module for Distant Horizons LOD databases.
//!
//! Provides zero-copy serialization, LEB128 varint encoding, Zstandard compression,
//! and bulk SQLite batch insertion conforming to the Distant Horizons v2 format.

pub mod blob_codec;
pub mod schema;
pub mod writer;

pub use blob_codec::{
    encode_lod_section_to_dh_blob, encode_lod_section_to_dh_blob_with_level, EncodedFullData,
};
pub use schema::init_dh_database_schema;
pub use writer::{ChunkHashEntry, DhSqliteBatchWriter};

#[cfg(test)]
mod tests {
    use super::*;
    use voxelizer::LodSection;

    #[test]
    fn test_sqlite_schema_and_write() {
        let temp_db = std::env::temp_dir().join(format!("test_dh_{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&temp_db);

        let mut writer = DhSqliteBatchWriter::open_or_create(&temp_db).unwrap();
        let sec = LodSection::new_empty(0, 0, 0);
        let hash = ChunkHashEntry {
            chunk_x: 0,
            chunk_z: 0,
            hash: 12345,
        };

        writer.write_batch(&[sec], &[hash]).unwrap();
        writer.finish().unwrap();

        assert!(temp_db.exists());
        let _ = std::fs::remove_file(&temp_db);
    }
}
