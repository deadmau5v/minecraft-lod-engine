================================================================================
                            MINECRAFT-LOD-ENGINE
               High-Performance Headless Minecraft LOD Pre-Baker
================================================================================

Author:        deadmau5v
Language:      Rust (1.75+)
Target Format: Distant Horizons SQLite Database (v2 Schema)
License:       MIT OR Apache-2.0


OVERVIEW
--------
mca2lod is a high-throughput, standalone headless Level of Detail (LOD)
pre-baking engine for Minecraft worlds. It directly processes Anvil region
files (.mca) or .zip archives, generates multi-level voxel downsamplings,
and serializes them into Distant Horizons compatible SQLite database files
(DistantHorizons.sqlite).


FEATURES
--------
* Pure Native Binary: Zero JVM dependency, minimal memory overhead.
* Multi-Version Compatibility: Supports 1.18+, 1.13-1.17, and legacy Anvil.
* Hardware SIMD Acceleration: AVX2 (x86_64) and NEON (ARM64) color downsampling.
* Extreme Throughput: 60,000+ chunks/s on multi-core servers.
* Direct Zip Ingestion: Reads uncompressed world directories or .zip archives.
* High-Performance Storage: Zero-journaling bulk SQLite transaction pipeline.


PIPELINE ARCHITECTURE
---------------------
  [ World Directory / .zip World Archive ]
                     │
                     ▼
  [ Zero-Copy Memory-Mapped I/O (memmap2) ]
                     │
                     ▼
  [ Parallel Decompression (Zlib / Gzip / LZ4) & FastNBT Scanner ]
                     │
                     ▼
  [ Voxel Column Run-Length Encoder & Global Palette LUT ]
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

Executable binary location:
    target/release/mca2lod


USAGE AND OPTIONS
-----------------
Syntax:
    mca2lod [OPTIONS] --map <PATH>

Options:
    -m, --map <PATH>
            Path to Minecraft world directory (containing region/) or .zip archive.

    -o, --output <FILE>
            Destination Distant Horizons SQLite database file path.
            [default: DistantHorizons.sqlite]

    --cx <CX>
            Center X coordinate in blocks. [default: 0]

    --cz <CZ>
            Center Z coordinate in blocks. [default: 0]

    -r, --radius <RADIUS>
            Baking radius in chunks (1 chunk = 16 blocks). [default: 64]

    -j, -t, --threads <NUM>
            Worker thread count (defaults to logical CPU core count).

    -l, --detail-levels <LEVELS>
            Maximum hierarchical LOD detail levels (0..=10). [default: 4]

    --zstd-level <LEVEL>
            Zstandard compression level for SQLite blob payloads (1..=22). [default: 3]

    -v, --verbose
            Enable detailed telemetry and timing metrics.

    -q, --quiet
            Operate quietly, suppressing banners and progress bars.

    -h, --help
            Print help summary.

    -V, --version
            Print version information.


DEPLOYING TO DISTANT HORIZONS (HOW TO USE)
------------------------------------------
The generated `DistantHorizons.sqlite` database file can be directly placed
into the corresponding Minecraft directory:

1. Singleplayer Client:
   Place the database inside your world save's `data` directory:
   `.minecraft/saves/<WorldName>/data/DistantHorizons.sqlite`

2. Fabric / Forge / NeoForge Modded Dedicated Server:
   Place the database inside the target dimension's `data` directory:
   * Overworld : <ServerRoot>/world/data/DistantHorizons.sqlite
   * The Nether: <ServerRoot>/world/DIM-1/data/DistantHorizons.sqlite
   * The End   : <ServerRoot>/world/DIM1/data/DistantHorizons.sqlite

3. Paper / Spigot / Folia Server (with DHS Plugin):
   Place the database inside the DHS plugin data directory:
   <ServerRoot>/plugins/DistantHorizons/data/<WorldName>/DistantHorizons.sqlite
   or inside the world directory:
   <ServerRoot>/world/data/DistantHorizons.sqlite

4. Client Multiplayer Cache (Direct Connection):
   If the dedicated server does not have the DH plugin installed, clients can
   manually copy the pre-baked database into their local multiplayer cache:
   `.minecraft/Distant_Horizons_server_data/<Server_IP_or_Hash>/DistantHorizons.sqlite`

Once placed, launch Minecraft or start the server. Distant Horizons will
instantly load the full-resolution LOD terrain with zero server tick lag or
in-game generation overhead.


EXAMPLES
--------
1. Bake standard world directory:
   $ mca2lod -m /path/to/world -o DistantHorizons.sqlite

2. Bake from zip world archive with 128-chunk radius (2048 blocks):
   $ mca2lod -m world.zip --cx 0 --cz 0 -r 128 -j 16

3. Quiet execution for scripts and background daemon tasks:
   $ mca2lod -q -m /srv/minecraft/world -o /srv/minecraft/world/data/DistantHorizons.sqlite


LICENSE
-------
Licensed under either of:
  * Apache License, Version 2.0 (http://www.apache.org/licenses/LICENSE-2.0)
  * MIT license (http://opensource.org/licenses/MIT)
at your option.
================================================================================
