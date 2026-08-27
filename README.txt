================================================================================
                            MINECRAFT-LOD-ENGINE
               High-Performance Headless Minecraft LOD Pre-Baker
================================================================================

Author:        deadmau5v
Language:      Rust (1.75+)
Target Format: Distant Horizons SQLite Database
License:       MIT OR Apache-2.0


OVERVIEW
--------
mca2lod is a fast, standalone headless Level of Detail (LOD) pre-rendering
utility for Minecraft worlds. It parses Anvil region files (.mca), extracts
voxel heightmaps, generates multi-level octree downsamplings, and writes directly
into Distant Horizons SQLite databases.


FEATURES
--------
* Pure Rust Native: Zero JVM dependency, low memory footprint.
* Multi-Version Support: Compatible with Minecraft 1.18+, 1.13-1.17, and legacy Anvil.
* Hardware SIMD Acceleration: AVX2 (x86_64) and NEON (ARM64) color downsampling.
* High Throughput: 60,000+ chunks/s on multi-core servers.
* Direct Zip Ingestion: Process uncompressed world folders or .zip archives directly.
* Atomic Bulk Writes: Optimized SQLite transaction pipeline with memory journal.


PIPELINE ARCHITECTURE
---------------------
  [ World Folder / .zip Archive ]
                │
                ▼
  [ Zero-Copy Memory Map / Fast Read ]
                │
                ▼
  [ Parallel Decompress (Zlib / Gzip / LZ4) & NBT Parse ]
                │
                ▼
  [ Voxel Column Run-Length Encoder & Palette LUT ]
                │
                ▼
  [ Multi-Level Octree Downsampling (SIMD AVX2 / NEON) ]
                │
                ▼
  [ Zstandard Compression & Atomic SQLite Commit (DistantHorizons.sqlite) ]


BUILDING FROM SOURCE
--------------------
Prerequisites:
  * Rust Toolchain 1.75.0 or later

Compile release binary:
    $ cargo build --release

The compiled executable will be located at:
    target/release/mca2lod


USAGE
-----
Syntax:
    mca2lod [OPTIONS] --map <PATH>

Options:
    -m, --map <PATH>
            Path to Minecraft world folder or .zip world archive.

    -o, --output <FILE>
            Destination Distant Horizons SQLite database file.
            [default: DistantHorizons.sqlite]

    --cx <CX>
            Center X coordinate in blocks. [default: 0]

    --cz <CZ>
            Center Z coordinate in blocks. [default: 0]

    -r, --radius <RADIUS>
            Baking radius in chunks (1 chunk = 16 blocks). [default: 64]

    -j, -t, --threads <NUM>
            Worker thread count (defaults to logical CPU cores).

    -l, --detail-levels <LEVELS>
            Maximum LOD detail levels (0..=10). [default: 4]

    --zstd-level <LEVEL>
            Zstandard compression level for SQLite blobs (1..=22). [default: 3]

    -v, --verbose
            Enable detailed diagnostics and timing traces.

    -q, --quiet
            Operate quietly, suppressing progress indicators.

    -h, --help
            Print help summary.

    -V, --version
            Print version information.


EXAMPLES
--------
1. Bake a standard world save with 64-chunk radius:
   $ mca2lod -m /path/to/world -o DistantHorizons.sqlite

2. Bake from a zip archive centered at (-256, 512) with 128-chunk radius:
   $ mca2lod -m world_backup.zip --cx -256 --cz 512 -r 128

3. Fast headless execution using 16 threads:
   $ mca2lod -m /path/to/world -r 128 -j 16 -q


LICENSE
-------
Licensed under either of:
  * Apache License, Version 2.0 (http://www.apache.org/licenses/LICENSE-2.0)
  * MIT license (http://opensource.org/licenses/MIT)
at your option.
================================================================================
