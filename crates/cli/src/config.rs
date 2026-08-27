//! Command-line interface definitions and configuration parser for `mca2lod`.

use clap::Parser;
use std::path::PathBuf;

/// Command-line configuration structure for the `mca2lod` pipeline.
#[derive(Parser, Debug, Clone)]
#[command(
    name = "mca2lod",
    author = "deadmau5v",
    version = "1.0.0",
    about = "Ultra-fast headless Minecraft Anvil MCA to Distant Horizons LOD generator",
    long_about = "mca2lod is a production-grade, highly parallel headless Level of Detail (LOD)\n\
                  baking pipeline for Minecraft worlds. It parses multi-version Anvil MCA region\n\
                  files, extracts voxel heightmaps, constructs multi-level downsampled octrees,\n\
                  and serializes directly into Distant Horizons SQLite database format with\n\
                  maximum I/O throughput."
)]
pub struct CliConfig {
    /// Path to Minecraft world save directory or .zip archive
    #[arg(
        short = 'm',
        long = "map",
        value_name = "PATH",
        help = "Path to Minecraft world folder or .zip world archive"
    )]
    pub map: PathBuf,

    /// Destination Distant Horizons SQLite database path
    #[arg(
        short = 'o',
        long = "output",
        value_name = "FILE",
        default_value = "DistantHorizons.sqlite",
        help = "Target Distant Horizons SQLite database output path"
    )]
    pub output: PathBuf,

    /// World center X coordinate in block units
    #[arg(
        long = "cx",
        default_value_t = 0,
        help = "Center X coordinate in blocks (default: 0)"
    )]
    pub cx: i32,

    /// World center Z coordinate in block units
    #[arg(
        long = "cz",
        default_value_t = 0,
        help = "Center Z coordinate in blocks (default: 0)"
    )]
    pub cz: i32,

    /// Generation bounding radius in chunk units
    #[arg(
        short = 'r',
        long = "radius",
        default_value_t = 64,
        help = "Baking radius from center in chunk units (16 blocks = 1 chunk)"
    )]
    pub radius: i32,

    /// Number of parallel worker execution threads
    #[arg(
        short = 'j',
        short_alias = 't',
        long = "threads",
        value_name = "NUM",
        help = "Concurrency worker thread count (defaults to logical CPU core count)"
    )]
    pub threads: Option<usize>,

    /// Maximum LOD octree detail level (0 to 10)
    #[arg(
        short = 'l',
        long = "detail-levels",
        default_value_t = 4,
        value_parser = clap::value_parser!(u8).range(0..=10),
        help = "Maximum hierarchical LOD detail levels to compute (default: 4)"
    )]
    pub detail_levels: u8,

    /// Zstandard compression level for FullData blobs (1-22)
    #[arg(
        long = "zstd-level",
        default_value_t = 3,
        value_parser = clap::value_parser!(i32).range(1..=22),
        help = "Zstandard compression level for SQLite blob payloads (default: 3)"
    )]
    pub zstd_level: i32,

    /// Enable verbose execution trace and per-stage telemetry
    #[arg(
        short = 'v',
        long = "verbose",
        help = "Enable detailed diagnostics and timing information"
    )]
    pub verbose: bool,

    /// Suppress progress indicators and non-essential output
    #[arg(
        short = 'q',
        long = "quiet",
        help = "Operate quietly, suppressing progress bars and non-critical messages"
    )]
    pub quiet: bool,
}
