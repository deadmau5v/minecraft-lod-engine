//! Native writer for Voxy's default serialized RocksDB storage.
//!
//! Voxy stores terrain hierarchy levels 0 through 4 in 32x32x32 world sections. Each section contains
//! packed mapping IDs, a local 16-bit lookup table, and a Zstandard-compressed
//! binary payload. Block-state and biome mappings are stored as compressed NBT.

use ahash::AHashMap;
use anyhow::{bail, Context, Result};
use flate2::write::GzEncoder;
use flate2::Compression;
use mca_parser::{BlockStateIdentity, SectionData};
use rocksdb::{
    ColumnFamilyDescriptor, DBCompressionType, FlushOptions, Options, WriteBatch, DB,
    DEFAULT_COLUMN_FAMILY_NAME,
};
use serde::Serialize;
use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use voxelizer::palette_lut::FLAG_FOLIAGE;
use voxelizer::GlobalPaletteLut;

const WORLD_SECTIONS_CF: &str = "world_sections";
const ID_MAPPINGS_CF: &str = "id_mappings";
const SECTION_EDGE: usize = 32;
const SECTION_VOLUME: usize = SECTION_EDGE * SECTION_EDGE * SECTION_EDGE;
const BLOCK_STATE_MAPPING_TYPE: u32 = 1;
const BIOME_MAPPING_TYPE: u32 = 2;
const MAX_BLOCK_ID: u32 = (1 << 20) - 1;
const MAX_BIOME_ID: u32 = (1 << 9) - 1;

/// Deterministic global mapping table used by every encoded Voxy section.
#[derive(Debug, Clone)]
pub struct VoxyMappings {
    block_states: Vec<BlockStateIdentity>,
    biomes: Vec<String>,
    block_ids: AHashMap<BlockStateIdentity, u32>,
    biome_ids: AHashMap<String, u32>,
    block_opacity: Vec<u8>,
}

impl VoxyMappings {
    /// Builds contiguous mapping IDs from sorted unique identities.
    ///
    /// Block ID 0 is reserved by Voxy for all air variants and is never persisted.
    pub fn build(
        block_states: BTreeSet<BlockStateIdentity>,
        mut biomes: BTreeSet<String>,
    ) -> Result<Self> {
        biomes.insert("minecraft:plains".to_string());

        let block_states: Vec<_> = block_states
            .into_iter()
            .filter(|state| !state.is_air())
            .collect();
        let biomes: Vec<_> = biomes.into_iter().collect();

        if block_states.len() as u32 > MAX_BLOCK_ID {
            bail!(
                "Voxy block-state mapping limit exceeded: {} > {}",
                block_states.len(),
                MAX_BLOCK_ID
            );
        }
        if biomes.len() as u32 > MAX_BIOME_ID + 1 {
            bail!(
                "Voxy biome mapping limit exceeded: {} > {}",
                biomes.len(),
                MAX_BIOME_ID + 1
            );
        }

        let block_ids = block_states
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, state)| (state, index as u32 + 1))
            .collect();
        let biome_ids = biomes
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, biome)| (biome, index as u32))
            .collect();
        let material_lut = GlobalPaletteLut::get_global();
        let mut block_opacity = Vec::with_capacity(block_states.len() + 1);
        block_opacity.push(0);
        block_opacity.extend(block_states.iter().map(|state| {
            let material = material_lut.get_material_by_name(&state.name);
            if material.flags & FLAG_FOLIAGE != 0 {
                15
            } else {
                material.opacity.min(15)
            }
        }));

        Ok(Self {
            block_states,
            biomes,
            block_ids,
            biome_ids,
            block_opacity,
        })
    }

    pub fn block_state_count(&self) -> usize {
        self.block_states.len()
    }

    pub fn biome_count(&self) -> usize {
        self.biomes.len()
    }

    #[inline]
    fn block_id(&self, state: &BlockStateIdentity) -> u32 {
        if state.is_air() {
            0
        } else {
            self.block_ids.get(state).copied().unwrap_or(0)
        }
    }

    #[inline]
    fn biome_id(&self, biome: &str) -> u32 {
        self.biome_ids
            .get(biome)
            .copied()
            .or_else(|| self.biome_ids.get("minecraft:plains").copied())
            .unwrap_or(0)
    }

    #[inline]
    fn opacity(&self, mapping: u64) -> u8 {
        let block_id = ((mapping >> 27) & u64::from(MAX_BLOCK_ID)) as usize;
        self.block_opacity.get(block_id).copied().unwrap_or(15)
    }
}

/// Mutable level-0 Voxy world section assembled from up to eight chunk sections.
pub struct VoxySectionBuilder {
    x: i32,
    y: i32,
    z: i32,
    data: Box<[u64; SECTION_VOLUME]>,
    non_air_blocks: usize,
}

impl VoxySectionBuilder {
    pub fn new(x: i32, y: i32, z: i32) -> Self {
        Self {
            x,
            y,
            z,
            data: Box::new([0; SECTION_VOLUME]),
            non_air_blocks: 0,
        }
    }

    /// Inserts one vanilla 16x16x16 chunk section into its 32x32x32 Voxy parent.
    pub fn ingest_section(
        &mut self,
        chunk_x: i32,
        chunk_z: i32,
        section: &SectionData,
        mappings: &VoxyMappings,
    ) {
        let offset_x = chunk_x.rem_euclid(2) as usize * 16;
        let offset_y = (section.y as i32).rem_euclid(2) as usize * 16;
        let offset_z = chunk_z.rem_euclid(2) as usize * 16;

        for y in 0..16usize {
            for z in 0..16usize {
                for x in 0..16usize {
                    let source_index = y * 256 + z * 16 + x;
                    let palette_index = section.block_indices[source_index] as usize;
                    let Some(state) = section.palette.get(palette_index) else {
                        continue;
                    };

                    let block_id = mappings.block_id(state);
                    let light = section.packed_light(source_index);
                    let mapping_id = if block_id == 0 {
                        u64::from(light) << 56
                    } else {
                        let biome_index = (y >> 2) * 16 + (z >> 2) * 4 + (x >> 2);
                        let biome_name = section
                            .biomes
                            .get(section.biome_indices[biome_index] as usize)
                            .map(String::as_str)
                            .unwrap_or("minecraft:plains");
                        compose_mapping_id(light, block_id, mappings.biome_id(biome_name))
                    };

                    let destination_index =
                        (offset_y + y) * 1024 + (offset_z + z) * 32 + offset_x + x;
                    let previous = self.data[destination_index];
                    if !is_air_mapping(previous) {
                        self.non_air_blocks -= 1;
                    }
                    self.data[destination_index] = mapping_id;
                    if block_id != 0 {
                        self.non_air_blocks += 1;
                    }
                }
            }
        }
    }

    pub fn finish(self) -> Option<VoxySection> {
        if self.non_air_blocks == 0 {
            return None;
        }
        Some(VoxySection {
            level: 0,
            x: self.x,
            y: self.y,
            z: self.z,
            data: self.data,
            non_empty_children: 0xff,
        })
    }
}

/// Uncompressed Voxy world section retained only while constructing its parents.
pub struct VoxySection {
    level: u8,
    x: i32,
    y: i32,
    z: i32,
    data: Box<[u64; SECTION_VOLUME]>,
    non_empty_children: u8,
}

impl VoxySection {
    pub fn level(&self) -> u8 {
        self.level
    }

    pub fn x(&self) -> i32 {
        self.x
    }

    pub fn y(&self) -> i32 {
        self.y
    }

    pub fn z(&self) -> i32 {
        self.z
    }

    pub fn encode(&self, zstd_level: i32) -> Result<EncodedVoxySection> {
        let key = world_section_id(self.level, self.x, self.y, self.z)?;
        let raw = serialize_section(key, self.non_empty_children, self.data.as_ref())?;
        let compressed = zstd::bulk::compress(&raw, zstd_level)
            .context("failed to Zstandard-compress Voxy section")?;
        Ok(EncodedVoxySection { key, compressed })
    }
}

/// Downsamples eight possible child octants into one Voxy parent section.
pub fn downsample_parent(
    level: u8,
    x: i32,
    y: i32,
    z: i32,
    children: &[&VoxySection],
    mappings: &VoxyMappings,
) -> Result<Option<VoxySection>> {
    if level == 0 || level > 4 {
        bail!("Voxy hierarchy level must be in 1..=4: {level}");
    }

    let mut child_lookup = AHashMap::with_capacity(children.len());
    let mut child_mask = 0u8;
    for child in children {
        if child.level + 1 != level {
            bail!("Voxy hierarchy contains a child at the wrong level");
        }
        let rx = child.x - x * 2;
        let ry = child.y - y * 2;
        let rz = child.z - z * 2;
        if !(0..=1).contains(&rx) || !(0..=1).contains(&ry) || !(0..=1).contains(&rz) {
            bail!("Voxy child section is outside its requested parent");
        }
        let index = rx as u8 | ((rz as u8) << 1) | ((ry as u8) << 2);
        if child.non_empty_children != 0 {
            child_mask |= 1 << index;
        }
        child_lookup.insert((child.x, child.y, child.z), *child);
    }
    if child_mask == 0 {
        return Ok(None);
    }

    let mut data = Box::new([0u64; SECTION_VOLUME]);
    let mut non_air_blocks = 0usize;
    for output_y in 0..SECTION_EDGE {
        let child_y = y * 2 + i32::from(output_y >= 16);
        let source_y = (output_y & 15) * 2;
        for output_z in 0..SECTION_EDGE {
            let child_z = z * 2 + i32::from(output_z >= 16);
            let source_z = (output_z & 15) * 2;
            for output_x in 0..SECTION_EDGE {
                let child_x = x * 2 + i32::from(output_x >= 16);
                let source_x = (output_x & 15) * 2;
                let Some(child) = child_lookup.get(&(child_x, child_y, child_z)) else {
                    continue;
                };

                let mut samples = [0u64; 8];
                for dy in 0..2usize {
                    for dz in 0..2usize {
                        for dx in 0..2usize {
                            let sample_index = dx | (dz << 1) | (dy << 2);
                            let source_index =
                                (source_y + dy) * 1024 + (source_z + dz) * 32 + source_x + dx;
                            samples[sample_index] = child.data[source_index];
                        }
                    }
                }
                let value = mip_voxels(&samples, mappings);
                let output_index = output_y * 1024 + output_z * 32 + output_x;
                data[output_index] = value;
                non_air_blocks += usize::from(!is_air_mapping(value));
            }
        }
    }

    if non_air_blocks == 0 {
        return Ok(None);
    }
    Ok(Some(VoxySection {
        level,
        x,
        y,
        z,
        data,
        non_empty_children: child_mask,
    }))
}

fn mip_voxels(samples: &[u64; 8], mappings: &VoxyMappings) -> u64 {
    let mut best_score = -1i16;
    let mut best_value = 0u64;
    for (index, &sample) in samples.iter().enumerate() {
        if is_air_mapping(sample) {
            continue;
        }
        let score = i16::from(mappings.opacity(sample)) * 16 + index as i16;
        if score > best_score {
            best_score = score;
            best_value = sample;
        }
    }
    if best_score >= 0 {
        return best_value;
    }

    let block_light_sum: u16 = samples
        .iter()
        .map(|sample| ((sample >> 60) & 0x0f) as u16)
        .sum();
    let sky_light_sum: u16 = samples
        .iter()
        .map(|sample| ((sample >> 56) & 0x0f) as u16)
        .sum();
    let block_light = (block_light_sum / 8) as u8;
    let sky_light = sky_light_sum.div_ceil(8) as u8;
    u64::from((block_light << 4) | sky_light) << 56
}

/// Ready-to-write Voxy section payload.
#[derive(Debug)]
pub struct EncodedVoxySection {
    key: u64,
    compressed: Vec<u8>,
}

/// Creates a new Voxy RocksDB storage directory and writes mappings and sections.
pub struct VoxyStorageWriter {
    path: PathBuf,
    db: DB,
    batch: WriteBatch,
    pending_sections: usize,
    written_sections: usize,
}

impl VoxyStorageWriter {
    /// Creates an empty database. Existing non-empty directories are rejected to
    /// prevent mapping-ID corruption when replacing a live Voxy database.
    pub fn create(path: &Path, mappings: &VoxyMappings) -> Result<Self> {
        ensure_empty_output(path)?;
        fs::create_dir_all(path).with_context(|| {
            format!("failed to create Voxy output directory {}", path.display())
        })?;

        let mut db_options = Options::default();
        db_options.create_if_missing(true);
        db_options.create_missing_column_families(true);
        db_options.set_max_total_wal_size(128 * 1024 * 1024);
        db_options.increase_parallelism(2);

        let mut default_cf = Options::default();
        default_cf.set_compression_type(DBCompressionType::Zstd);

        let mut section_cf = Options::default();
        section_cf.set_compression_type(DBCompressionType::None);
        section_cf.optimize_for_point_lookup(128);

        let mut mapping_cf = Options::default();
        mapping_cf.set_compression_type(DBCompressionType::Zstd);

        let descriptors = vec![
            ColumnFamilyDescriptor::new(DEFAULT_COLUMN_FAMILY_NAME, default_cf),
            ColumnFamilyDescriptor::new(WORLD_SECTIONS_CF, section_cf),
            ColumnFamilyDescriptor::new(ID_MAPPINGS_CF, mapping_cf),
        ];
        let db = DB::open_cf_descriptors(&db_options, path, descriptors)
            .with_context(|| format!("failed to create Voxy RocksDB at {}", path.display()))?;

        let mut writer = Self {
            path: path.to_path_buf(),
            db,
            batch: WriteBatch::default(),
            pending_sections: 0,
            written_sections: 0,
        };
        writer.write_mappings(mappings)?;
        writer.flush_batch()?;
        Ok(writer)
    }

    pub fn write_section(&mut self, section: EncodedVoxySection) -> Result<()> {
        let cf = self
            .db
            .cf_handle(WORLD_SECTIONS_CF)
            .context("Voxy world_sections column family is missing")?;
        self.batch
            .put_cf(cf, section.key.to_be_bytes(), section.compressed);
        self.pending_sections += 1;
        self.written_sections += 1;
        if self.pending_sections >= 256 {
            self.flush_batch()?;
        }
        Ok(())
    }

    pub fn written_sections(&self) -> usize {
        self.written_sections
    }

    pub fn finish(mut self) -> Result<()> {
        self.flush_batch()?;
        let mut options = FlushOptions::default();
        options.set_wait(true);
        self.db
            .flush_opt(&options)
            .with_context(|| format!("failed to flush Voxy database {}", self.path.display()))?;
        Ok(())
    }

    fn write_mappings(&mut self, mappings: &VoxyMappings) -> Result<()> {
        let cf = self
            .db
            .cf_handle(ID_MAPPINGS_CF)
            .context("Voxy id_mappings column family is missing")?;

        for (index, state) in mappings.block_states.iter().enumerate() {
            let id = index as u32 + 1;
            let key = ((BLOCK_STATE_MAPPING_TYPE << 30) | id).to_be_bytes();
            self.batch
                .put_cf(cf, key, encode_block_state_mapping(id, state)?);
        }
        for (id, biome) in mappings.biomes.iter().enumerate() {
            let id = id as u32;
            let key = ((BIOME_MAPPING_TYPE << 30) | id).to_be_bytes();
            self.batch.put_cf(cf, key, encode_biome_mapping(id, biome)?);
        }
        Ok(())
    }

    fn flush_batch(&mut self) -> Result<()> {
        if self.batch.is_empty() {
            return Ok(());
        }
        let batch = std::mem::take(&mut self.batch);
        self.db
            .write(batch)
            .with_context(|| format!("failed to write Voxy database {}", self.path.display()))?;
        self.pending_sections = 0;
        Ok(())
    }
}

#[inline]
pub fn compose_mapping_id(light: u8, block_id: u32, biome_id: u32) -> u64 {
    (u64::from(light) << 56) | (u64::from(biome_id) << 47) | (u64::from(block_id) << 27)
}

#[inline]
fn is_air_mapping(mapping: u64) -> bool {
    ((mapping >> 27) & u64::from(MAX_BLOCK_ID)) == 0
}

/// Packs Voxy's format-version-1 section key.
pub fn world_section_id(level: u8, x: i32, y: i32, z: i32) -> Result<u64> {
    if level > 15 {
        bail!("Voxy detail level must fit four bits: {level}");
    }
    if !(-128..=127).contains(&y) {
        bail!("Voxy section Y must fit signed eight bits: {y}");
    }
    const MIN_24: i32 = -(1 << 23);
    const MAX_24: i32 = (1 << 23) - 1;
    if !(MIN_24..=MAX_24).contains(&x) || !(MIN_24..=MAX_24).contains(&z) {
        bail!("Voxy section X/Z must fit signed 24 bits: ({x}, {z})");
    }

    Ok((u64::from(level) << 60)
        | (((y as u32 & 0xff) as u64) << 52)
        | (((z as u32 & 0x00ff_ffff) as u64) << 28)
        | (((x as u32 & 0x00ff_ffff) as u64) << 4))
}

fn serialize_section(key: u64, non_empty_children: u8, data: &[u64]) -> Result<Vec<u8>> {
    let mut lut = Vec::<u64>::new();
    let mut lut_ids = AHashMap::<u64, u16>::new();
    let mut indices = Vec::<u16>::with_capacity(data.len());

    for &mapping in data {
        let index = if let Some(&index) = lut_ids.get(&mapping) {
            index
        } else {
            let index =
                u16::try_from(lut.len()).context("Voxy section LUT exceeded 65535 entries")?;
            lut.push(mapping);
            lut_ids.insert(mapping, index);
            index
        };
        indices.push(index);
    }

    let metadata = (lut.len() as u64) | (u64::from(non_empty_children) << 16);
    let mut output = Vec::with_capacity(16 + data.len() * 2 + lut.len() * 8);
    output.extend_from_slice(&key.to_le_bytes());
    output.extend_from_slice(&metadata.to_le_bytes());
    for index in indices {
        output.extend_from_slice(&index.to_le_bytes());
    }
    for mapping in lut {
        output.extend_from_slice(&mapping.to_le_bytes());
    }
    Ok(output)
}

#[derive(Serialize)]
struct BlockStateMapping<'a> {
    id: i32,
    block_state: SerializedBlockState<'a>,
}

#[derive(Serialize)]
struct SerializedBlockState<'a> {
    #[serde(rename = "Name")]
    name: &'a str,
    #[serde(
        rename = "Properties",
        skip_serializing_if = "std::collections::BTreeMap::is_empty"
    )]
    properties: &'a std::collections::BTreeMap<String, String>,
}

#[derive(Serialize)]
struct BiomeMapping<'a> {
    id: i32,
    biome_id: &'a str,
}

fn encode_block_state_mapping(id: u32, state: &BlockStateIdentity) -> Result<Vec<u8>> {
    let root = BlockStateMapping {
        id: id as i32,
        block_state: SerializedBlockState {
            name: &state.name,
            properties: &state.properties,
        },
    };
    encode_compressed_nbt(&root)
}

fn encode_biome_mapping(id: u32, biome: &str) -> Result<Vec<u8>> {
    encode_compressed_nbt(&BiomeMapping {
        id: id as i32,
        biome_id: biome,
    })
}

fn encode_compressed_nbt<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let nbt = fastnbt::to_bytes(value).context("failed to serialize Voxy mapping NBT")?;
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(&nbt)
        .context("failed to compress Voxy mapping NBT")?;
    encoder
        .finish()
        .context("failed to finish Voxy mapping NBT")
}

fn ensure_empty_output(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    if !path.is_dir() {
        bail!(
            "Voxy output path exists and is not a directory: {}",
            path.display()
        );
    }
    if fs::read_dir(path)
        .with_context(|| format!("failed to inspect Voxy output directory {}", path.display()))?
        .next()
        .is_some()
    {
        bail!(
            "Voxy output directory is not empty: {}. Stop the server and choose a new storage directory.",
            path.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::read::GzDecoder;
    use std::collections::BTreeMap;
    use std::io::Read;

    #[test]
    fn section_key_matches_voxy_bit_layout() {
        let key = world_section_id(0, -1, -2, 3).unwrap();
        assert_eq!((key >> 60) & 0xf, 0);
        assert_eq!(((key << 36) as i64 >> 40) as i32, -1);
        assert_eq!(((key << 4) as i64 >> 56) as i32, -2);
        assert_eq!(((key << 12) as i64 >> 40) as i32, 3);
    }

    #[test]
    fn section_serialization_uses_little_endian_header_and_lut() {
        let key = world_section_id(0, 1, 2, 3).unwrap();
        let mut data = vec![0; SECTION_VOLUME];
        data[1] = compose_mapping_id(0xf0, 7, 2);
        let encoded = serialize_section(key, 0xff, &data).unwrap();
        assert_eq!(u64::from_le_bytes(encoded[0..8].try_into().unwrap()), key);
        let metadata = u64::from_le_bytes(encoded[8..16].try_into().unwrap());
        assert_eq!(metadata & 0xffff, 2);
        assert_eq!((metadata >> 16) & 0xff, 0xff);
    }

    #[test]
    fn mapping_nbt_round_trips_through_gzip() {
        let state = BlockStateIdentity {
            name: "minecraft:oak_log".to_string(),
            properties: BTreeMap::from([("axis".to_string(), "y".to_string())]),
        };
        let compressed = encode_block_state_mapping(1, &state).unwrap();
        let mut decoder = GzDecoder::new(compressed.as_slice());
        let mut raw = Vec::new();
        decoder.read_to_end(&mut raw).unwrap();
        let decoded: fastnbt::Value = fastnbt::from_bytes(&raw).unwrap();
        assert!(matches!(decoded, fastnbt::Value::Compound(_)));
    }

    #[test]
    fn hierarchy_downsampling_preserves_child_octant_and_mask() {
        let stone = BlockStateIdentity::simple("minecraft:stone".to_string());
        let mappings = VoxyMappings::build(
            BTreeSet::from([stone.clone()]),
            BTreeSet::from(["minecraft:plains".to_string()]),
        )
        .unwrap();
        let stone_mapping = compose_mapping_id(0, mappings.block_id(&stone), 0);
        let child = VoxySection {
            level: 0,
            x: 0,
            y: 0,
            z: 0,
            data: Box::new([stone_mapping; SECTION_VOLUME]),
            non_empty_children: 0xff,
        };

        let parent = downsample_parent(1, 0, 0, 0, &[&child], &mappings)
            .unwrap()
            .unwrap();
        assert_eq!(parent.non_empty_children, 0b0000_0001);
        assert_eq!(parent.data[0], stone_mapping);
        assert_eq!(parent.data[15 * 1024 + 15 * 32 + 15], stone_mapping);
        assert_eq!(parent.data[16], 0);
    }

    #[test]
    fn writer_creates_expected_column_families_and_payloads() {
        let temp = tempfile::tempdir().unwrap();
        let stone = BlockStateIdentity::simple("minecraft:stone".to_string());
        let mappings = VoxyMappings::build(
            BTreeSet::from([stone.clone()]),
            BTreeSet::from(["minecraft:plains".to_string()]),
        )
        .unwrap();
        let key = world_section_id(0, 0, 0, 0).unwrap();
        let section = VoxySection {
            level: 0,
            x: 0,
            y: 0,
            z: 0,
            data: Box::new([compose_mapping_id(0, mappings.block_id(&stone), 0); SECTION_VOLUME]),
            non_empty_children: 0xff,
        };
        let mut writer = VoxyStorageWriter::create(temp.path(), &mappings).unwrap();
        writer.write_section(section.encode(1).unwrap()).unwrap();
        writer.finish().unwrap();

        let families = DB::list_cf(&Options::default(), temp.path()).unwrap();
        assert!(families.iter().any(|name| name == WORLD_SECTIONS_CF));
        assert!(families.iter().any(|name| name == ID_MAPPINGS_CF));

        let db = DB::open_cf(
            &Options::default(),
            temp.path(),
            [WORLD_SECTIONS_CF, ID_MAPPINGS_CF],
        )
        .unwrap();
        let sections_cf = db.cf_handle(WORLD_SECTIONS_CF).unwrap();
        let compressed = db.get_cf(sections_cf, key.to_be_bytes()).unwrap().unwrap();
        let raw = zstd::stream::decode_all(compressed.as_slice()).unwrap();
        assert_eq!(u64::from_le_bytes(raw[0..8].try_into().unwrap()), key);

        let mappings_cf = db.cf_handle(ID_MAPPINGS_CF).unwrap();
        let mapping_key = ((BLOCK_STATE_MAPPING_TYPE << 30) | 1).to_be_bytes();
        assert!(db.get_cf(mappings_cf, mapping_key).unwrap().is_some());
    }
}
