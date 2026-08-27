use anyhow::Result;
use rusqlite::Connection;

pub const SCHEMA_SCRIPTS: &[&str] = &[
    "sqlScripts/0010-sqlite-createInitialDataTables.sql",
    "sqlScripts/0020-sqlite-createFullDataSourceV2Tables.sql",
    "sqlScripts/0030-sqlite-changeTableJournaling.sql",
    "sqlScripts/0031-sqlite-useSqliteWalJournaling.sql",
    "sqlScripts/0040-sqlite-removeRenderCache.sql",
    "sqlScripts/0050-sqlite-addApplyToParentIndex.sql",
    "sqlScripts/0060-sqlite-createChunkHashTable.sql",
    "sqlScripts/0070-sqlite-createBeaconBeamTable.sql",
    "sqlScripts/0080-sqlite-addApplyToChildrenColumn.sql",
    "sqlScripts/0090-sqlite-addAdjacentFullDataColumns.sql",
    "sqlScripts/0100-sqlite-deleteLowDetailDataForRegen.sql",
];

pub fn init_dh_database_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS Schema (
            SchemaVersionId INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
            ScriptName TEXT NOT NULL UNIQUE,
            AppliedDateTime DATETIME NOT NULL default CURRENT_TIMESTAMP
        );

        CREATE TABLE IF NOT EXISTS Legacy_FullData_V1 (
            DhSectionPos TEXT NOT NULL PRIMARY KEY,
            DataDetailLevel TINYINT NULL,
            Checksum INT NULL,
            DataVersion BIGINT NULL,
            WorldGenStep NVARCHAR(32) NULL,
            DataType NVARCHAR(48) NULL,
            BinaryDataFormatVersion TINYINT NULL,
            Data BLOB NULL,
            CreatedDateTime DATETIME NOT NULL default CURRENT_TIMESTAMP,
            LastModifiedDateTime DATETIME NOT NULL default CURRENT_TIMESTAMP,
            MigrationFailed BIT NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS FullData (
            DetailLevel TINYINT NOT NULL,
            PosX INT NOT NULL,
            PosZ INT NOT NULL,
            MinY INT NOT NULL,
            DataChecksum INT NOT NULL,
            Data BLOB NULL,
            ColumnGenerationStep BLOB NULL,
            ColumnWorldCompressionMode BLOB NULL,
            Mapping BLOB NULL,
            DataFormatVersion TINYINT NULL,
            CompressionMode TINYINT NULL,
            ApplyToParent BIT NULL,
            LastModifiedUnixDateTime BIGINT NOT NULL,
            CreatedUnixDateTime BIGINT NOT NULL,
            ApplyToChildren BIT NULL,
            NorthAdjData BLOB NULL,
            SouthAdjData BLOB NULL,
            EastAdjData BLOB NULL,
            WestAdjData BLOB NULL,
            PRIMARY KEY (DetailLevel, PosX, PosZ)
        );

        CREATE INDEX IF NOT EXISTS FullDataUpdatedIndex on FullData (ApplyToParent) where ApplyToParent = 1;
        CREATE INDEX IF NOT EXISTS FullDataApplyToChildrenIndex on FullData (ApplyToChildren) where ApplyToChildren = 1;

        CREATE TABLE IF NOT EXISTS ChunkHash (
            ChunkPosX INT NOT NULL,
            ChunkPosZ INT NOT NULL,
            ChunkHash INT NOT NULL,
            LastModifiedUnixDateTime BIGINT NOT NULL,
            CreatedUnixDateTime BIGINT NOT NULL,
            PRIMARY KEY (ChunkPosX, ChunkPosZ)
        );

        CREATE TABLE IF NOT EXISTS BeaconBeam (
            BlockPosX INT NOT NULL,
            BlockPosY INT NOT NULL,
            BlockPosZ INT NOT NULL,
            ColorR INT NOT NULL,
            ColorG INT NOT NULL,
            ColorB INT NOT NULL,
            LastModifiedUnixDateTime BIGINT NOT NULL,
            CreatedUnixDateTime BIGINT NOT NULL,
            PRIMARY KEY (BlockPosX, BlockPosY, BlockPosZ)
        );"
    )?;

    // Insert schema migration records
    let mut stmt = conn.prepare_cached(
        "INSERT OR IGNORE INTO Schema (SchemaVersionId, ScriptName) VALUES (?1, ?2)",
    )?;
    for (idx, script) in SCHEMA_SCRIPTS.iter().enumerate() {
        stmt.execute(rusqlite::params![idx + 1, script])?;
    }

    Ok(())
}
