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


DEPLOYING TO DISTANT HORIZONS (DH 文件放置与拷贝说明)
---------------------------------------------------
mca2lod 生成的 `DistantHorizons.sqlite` 可直接拷贝至以下对应路径：

1. 单人游戏客户端 (Singleplayer Client):
   将生成的文件重命名为 `DistantHorizons.sqlite` 并放入世界存档的 `data` 目录：
   `.minecraft/saves/<WorldName>/data/DistantHorizons.sqlite`

2. Fabric / Forge / NeoForge 模组服务端 (Modded Server):
   将文件放入服务端的对应维度数据目录：
   * 主世界 (Overworld) : <ServerRoot>/world/data/DistantHorizons.sqlite
   * 下界 (Nether)       : <ServerRoot>/world/DIM-1/data/DistantHorizons.sqlite
   * 末地 (The End)      : <ServerRoot>/world/DIM1/data/DistantHorizons.sqlite

3. Paper / Spigot / Folia 插件服务端 (DHS Server Plugin):
   将文件放入 DHS 插件数据目录：
   <ServerRoot>/plugins/DistantHorizons/data/<WorldName>/DistantHorizons.sqlite
   或世界数据目录：
   <ServerRoot>/world/data/DistantHorizons.sqlite

4. 联机客户端本地直连缓存 (Client Multiplayer Cache):
   若服务器未安装远景传输插件，客户端可直接把预生成的数据库拷贝到本地联机缓存：
   `.minecraft/Distant_Horizons_server_data/<Server_IP_or_Hash>/DistantHorizons.sqlite`

拷贝完成后启动客户端或服务端，Distant Horizons 即可即时加载完整远景 LOD，
无需在游戏内消耗服务器 CPU/TPS 跑图生成。


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
