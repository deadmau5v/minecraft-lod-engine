//! Command-line interface definitions and configuration parser for `mca2lod`.

use clap::{Parser, ValueEnum};
use std::path::{Path, PathBuf};

/// Output storage backend selected for the generated LOD data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    /// Distant Horizons v2 SQLite database.
    #[value(alias = "distant-horizons")]
    Dh,
    /// Native Voxy RocksDB section storage (hierarchy levels 0 through 4).
    Voxy,
}

/// Command-line configuration structure for the `mca2lod` pipeline.
#[derive(Parser, Debug, Clone)]
#[command(
    name = "mca2lod",
    author = "deadmau5v",
    version = "1.0.0",
    about = "Ultra-fast headless Minecraft Anvil MCA to DH or Voxy LOD generator",
    long_about = "mca2lod is a production-grade, highly parallel headless Level of Detail (LOD)\n\
                  baking pipeline for Minecraft worlds. It parses multi-version Anvil MCA region\n\
                  files and writes either Distant Horizons v2 SQLite data or Voxy's native\n\
                  hierarchical RocksDB section storage with maximum I/O throughput."
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

    /// Output storage format.
    #[arg(
        long = "format",
        value_enum,
        default_value_t = OutputFormat::Dh,
        help = "Output backend: dh (SQLite) or voxy (native RocksDB storage)"
    )]
    pub format: OutputFormat,

    /// Destination database file or storage directory.
    #[arg(
        short = 'o',
        long = "output",
        value_name = "PATH",
        default_value = "DistantHorizons.sqlite",
        help = "DH SQLite file or Voxy storage directory"
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
        help = "Maximum DH hierarchy level; Voxy always writes required levels 0..=4"
    )]
    pub detail_levels: u8,

    /// Zstandard compression level for DH blobs or Voxy sections (1-22)
    #[arg(
        long = "zstd-level",
        default_value_t = 3,
        value_parser = clap::value_parser!(i32).range(1..=22),
        help = "Zstandard level for DH blobs or Voxy sections (default: 3; Voxy recommends 1)"
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

impl CliConfig {
    pub fn resolved_output(&self) -> PathBuf {
        if self.format == OutputFormat::Voxy && self.output == Path::new("DistantHorizons.sqlite") {
            PathBuf::from("voxy-storage")
        } else {
            self.output.clone()
        }
    }
}
