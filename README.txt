================================================================================
                            MINECRAFT-LOD-ENGINE
               High-Performance Headless Minecraft LOD Pre-Baker
================================================================================

Author:        deadmau5v
Language:      Rust (1.75+)
Target Formats: Distant Horizons SQLite v2; Voxy Native RocksDB
License:       MIT OR Apache-2.0


OVERVIEW
--------
mca2lod is a high-throughput, standalone headless Level of Detail (LOD)
pre-baking engine for Minecraft worlds. It directly processes Anvil region
files (.mca) or .zip archives and writes either Distant Horizons compatible
SQLite databases or Voxy native hierarchical RocksDB section storage.


FEATURES
--------
* Pure Native Binary: Zero JVM dependency, minimal memory overhead.
* Multi-Version Compatibility: Supports 1.18+, 1.13-1.17, and legacy Anvil.
* Hardware SIMD Acceleration: AVX2 (x86_64) and NEON (ARM64) color downsampling.
* Measured DH Throughput: Approximately 10,900 chunks/s on the reference world.
* Direct Zip Ingestion: Reads uncompressed world directories or .zip archives.
* Dual Storage Backends: Distant Horizons SQLite and Voxy native RocksDB.
* Voxy 3D Hierarchy: Complete 32x32x32 section levels 0 through 4.
* Deterministic Identity Mapping: Preserves block-state properties, biomes, and light.
* High-Performance Storage: Bulk SQLite transactions and batched RocksDB writes.


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
  [ Canonical Block-State / Biome / Light Extraction ]
                     │
            ┌────────┴────────┐
            ▼                 ▼
  [ DH Column RLE ]   [ Voxy 32x32x32 Sections ]
            │                 │
  [ DH Octree 0..10 ] [ Voxy Hierarchy 0..4 ]
            │                 │
            ▼                 ▼
  [ SQLite v2 ]       [ Zstd + Native RocksDB ]


BUILDING FROM SOURCE
--------------------
Prerequisites:
  * Rust Toolchain 1.75.0 or later
  * C/C++ build toolchain, CMake, and libclang (bundled RocksDB build)

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

    --format <FORMAT>
            Output backend: dh or voxy. [default: dh]

    -o, --output <PATH>
            Destination DH SQLite file or Voxy native RocksDB storage directory.
            DH default: DistantHorizons.sqlite
            Voxy implicit default: voxy-storage/

    --cx <CX>
            Center X coordinate in blocks. [default: 0]

    --cz <CZ>
            Center Z coordinate in blocks. [default: 0]

    -r, --radius <RADIUS>
            Baking radius in chunks (1 chunk = 16 blocks). [default: 64]

    -j, -t, --threads <NUM>
            Worker thread count (defaults to logical CPU core count).

    -l, --detail-levels <LEVELS>
            Maximum DH hierarchy level (0..=10). [default: 4]
            Voxy always emits its required complete hierarchy, levels 0..=4.

    --zstd-level <LEVEL>
            Zstandard level for DH blobs or Voxy sections (1..=22). [default: 3]
            Voxy's upstream default is level 1.

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


DEPLOYING NATIVE VOXY STORAGE
-----------------------------
Generate a complete native Voxy database:

    $ mca2lod --format voxy -m /path/to/world \
        -o /path/to/new/storage --radius 512 --zstd-level 1

The Voxy output is a RocksDB directory, not a single file. Generate one
storage directory per Minecraft dimension; the tool rejects inputs containing
multiple region/ directories. It contains the world_sections and id_mappings
column families expected by Voxy's default
Serializer -> CompressionAdaptor(ZSTD) -> RocksDB configuration.

Fabric VoxyServer deployment:

1. Install VoxyServer, Voxy, and Sodium in the dedicated server's mods folder.
2. Start and stop the server once so it creates:
       <world>/voxyserver/<world_id>/storage/
3. Keep a backup and remove the generated empty storage directory.
4. Move the mca2lod output directory into that exact storage path.
5. Start the server and verify the VoxyServer presence index completes.

Never write into a RocksDB directory while Minecraft or VoxyServer is running.
The writer rejects non-empty output directories to prevent incompatible mapping
IDs from being merged accidentally. It builds in a same-filesystem staging
directory and atomically publishes the completed RocksDB database.

Paper / Purpur / Folia deployment:

Native Voxy RocksDB files are not consumed by Bukkit plugins. For these servers,
use LOD Server Support (LSS), available as a Paper plugin. Install LSS on the
server and install both LSS and Voxy on each client. LSS reads Anvil data,
optionally maintains its own SQLite LOD store, and streams Voxy-compatible voxel
columns. See VOXY_SERVER_SUPPORT.txt for compatibility and tuning guidance.

Rendering remains client-side on the player's GPU. "Server support" means that
the server voxelizes, caches, validates, and transmits terrain data; it does not
run Voxy's OpenGL renderer on the headless server.


EXAMPLES
--------
1. Bake standard world directory:
   $ mca2lod -m /path/to/world -o DistantHorizons.sqlite

2. Bake from zip world archive with 128-chunk radius (2048 blocks):
   $ mca2lod -m world.zip --cx 0 --cz 0 -r 128 -j 16

3. Generate native Voxy storage for a Fabric VoxyServer instance:
   $ mca2lod --format voxy -m /srv/minecraft/world \
       -o /srv/minecraft/voxy-staging/storage --radius 512 --zstd-level 1

4. Quiet execution for scripts and background daemon tasks:
   $ mca2lod -q -m /srv/minecraft/world -o /srv/minecraft/world/data/DistantHorizons.sqlite


LICENSE
-------
Licensed under either of:
  * Apache License, Version 2.0 (http://www.apache.org/licenses/LICENSE-2.0)
  * MIT license (http://opensource.org/licenses/MIT)
at your option.
================================================================================
