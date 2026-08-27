use crate::bit_array::BitArrayUnpacker;
use ahash::AHashMap;
use anyhow::Result;
use fastnbt::{ByteArray, LongArray};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct ChunkData {
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub sections: Vec<SectionData>,
}

#[derive(Debug, Clone)]
pub struct SectionData {
    pub y: i8,
    pub is_empty_air: bool,
    pub palette: Vec<String>,
    pub block_indices: [u16; 4096],
    pub biomes: Vec<String>,
    pub biome_indices: [u16; 64],
}

#[allow(dead_code)]
#[derive(Deserialize, Debug, Clone)]
struct ModernChunkNbt {
    #[serde(rename = "DataVersion")]
    data_version: Option<i32>,
    #[serde(rename = "xPos")]
    x_pos: Option<i32>,
    #[serde(rename = "zPos")]
    z_pos: Option<i32>,
    #[serde(rename = "sections")]
    sections: Option<Vec<ModernSectionNbt>>,
    #[serde(rename = "Level")]
    level: Option<LegacyLevelWrapper>,
}

#[derive(Deserialize, Debug, Clone)]
struct ModernSectionNbt {
    #[serde(rename = "Y")]
    y: i8,
    #[serde(rename = "block_states")]
    block_states: Option<ModernBlockStatesNbt>,
    #[serde(rename = "biomes")]
    biomes: Option<ModernBiomesNbt>,
    #[serde(rename = "Palette")]
    legacy_palette: Option<Vec<ModernBlockStateEntry>>,
    #[serde(rename = "BlockStates")]
    legacy_block_states: Option<LongArray>,
    #[serde(rename = "Blocks")]
    legacy_blocks: Option<ByteArray>,
    #[serde(rename = "Data")]
    legacy_data: Option<ByteArray>,
    #[serde(rename = "Add")]
    legacy_add: Option<ByteArray>,
}

#[derive(Deserialize, Debug, Clone)]
struct ModernBlockStatesNbt {
    #[serde(rename = "palette")]
    palette: Vec<ModernBlockStateEntry>,
    #[serde(rename = "data")]
    data: Option<LongArray>,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug, Clone)]
struct ModernBlockStateEntry {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Properties")]
    properties: Option<HashMap<String, String>>,
}

#[derive(Deserialize, Debug, Clone)]
struct ModernBiomesNbt {
    #[serde(rename = "palette")]
    palette: Vec<String>,
    #[serde(rename = "data")]
    data: Option<LongArray>,
}

#[derive(Deserialize, Debug, Clone)]
struct LegacyLevelWrapper {
    #[serde(rename = "xPos")]
    x_pos: Option<i32>,
    #[serde(rename = "zPos")]
    z_pos: Option<i32>,
    #[serde(rename = "Sections")]
    sections: Option<Vec<ModernSectionNbt>>,
}

#[inline(always)]
fn is_air_name(name: &str) -> bool {
    let clean = name.strip_prefix("minecraft:").unwrap_or(name);
    clean == "air" || clean == "cave_air" || clean == "void_air"
}

pub fn parse_chunk_nbt(
    raw_nbt: &[u8],
    fallback_chunk_x: i32,
    fallback_chunk_z: i32,
) -> Result<ChunkData> {
    let mut parsed: ModernChunkNbt = fastnbt::from_bytes(raw_nbt)?;

    let chunk_x = parsed
        .x_pos
        .or_else(|| parsed.level.as_ref().and_then(|l| l.x_pos))
        .unwrap_or(fallback_chunk_x);
    let chunk_z = parsed
        .z_pos
        .or_else(|| parsed.level.as_ref().and_then(|l| l.z_pos))
        .unwrap_or(fallback_chunk_z);

    let raw_sections = parsed
        .sections
        .take()
        .or_else(|| parsed.level.as_mut().and_then(|l| l.sections.take()))
        .unwrap_or_default();

    let mut sections = Vec::with_capacity(raw_sections.len());

    for s in raw_sections {
        let y = s.y;
        let mut section = SectionData {
            y,
            is_empty_air: false,
            palette: Vec::new(),
            block_indices: [0; 4096],
            biomes: Vec::new(),
            biome_indices: [0; 64],
        };

        // 1. Modern 1.18+ block_states
        if let Some(bs) = s.block_states {
            if bs.palette.is_empty() || (bs.palette.len() == 1 && is_air_name(&bs.palette[0].name)) {
                section.is_empty_air = true;
            } else {
                section.palette = bs.palette.into_iter().map(|p| p.name).collect();
                if let Some(ref data) = bs.data {
                    let p_len = section.palette.len();
                    let bits = if p_len <= 1 {
                        0
                    } else {
                        let mut b = 4;
                        while (1 << b) < p_len {
                            b += 1;
                        }
                        b
                    };
                    BitArrayUnpacker::unpack_4096(bits, data, &mut section.block_indices);
                } else {
                    section.block_indices.fill(0);
                }
            }
        }
        // 2. 1.13 - 1.17 Palette + BlockStates
        else if let Some(p) = s.legacy_palette {
            if p.is_empty() || (p.len() == 1 && is_air_name(&p[0].name)) {
                section.is_empty_air = true;
            } else {
                section.palette = p.into_iter().map(|e| e.name).collect();
                if let Some(ref data) = s.legacy_block_states {
                    let p_len = section.palette.len();
                    let bits = if p_len <= 1 {
                        0
                    } else {
                        let mut b = 4;
                        while (1 << b) < p_len {
                            b += 1;
                        }
                        b
                    };
                    BitArrayUnpacker::unpack_4096(bits, data, &mut section.block_indices);
                } else {
                    section.block_indices.fill(0);
                }
            }
        }
        // 3. 1.2 - 1.12 Legacy Anvil format (Blocks + Data + Add)
        else if let Some(ref blocks) = s.legacy_blocks {
            let data_bytes = s.legacy_data.as_deref().unwrap_or(&[]);
            let add_bytes = s.legacy_add.as_deref().unwrap_or(&[]);

            let mut legacy_palette_map: AHashMap<String, u16> = AHashMap::new();
            let mut unique_palette: Vec<String> = Vec::new();

            for i in 0..4096 {
                let block_id_low = if i < blocks.len() {
                    blocks[i] as u16
                } else {
                    0
                };
                let block_id_high = if !add_bytes.is_empty() && (i / 2) < add_bytes.len() {
                    let add_byte = add_bytes[i / 2] as u16;
                    if i % 2 == 0 {
                        add_byte & 0x0F
                    } else {
                        (add_byte >> 4) & 0x0F
                    }
                } else {
                    0
                };
                let full_block_id = (block_id_high << 8) | block_id_low;

                let meta = if !data_bytes.is_empty() && (i / 2) < data_bytes.len() {
                    let d = data_bytes[i / 2];
                    if i % 2 == 0 {
                        d & 0x0F
                    } else {
                        (d >> 4) & 0x0F
                    }
                } else {
                    0
                };

                let name = legacy_id_to_name(full_block_id, meta as u8);
                let palette_idx = if let Some(&idx) = legacy_palette_map.get(&name) {
                    idx
                } else {
                    let idx = unique_palette.len() as u16;
                    legacy_palette_map.insert(name.clone(), idx);
                    unique_palette.push(name);
                    idx
                };

                section.block_indices[i] = palette_idx;
            }

            if unique_palette.is_empty()
                || (unique_palette.len() == 1 && is_air_name(&unique_palette[0]))
            {
                section.is_empty_air = true;
            }
            section.palette = unique_palette;
        } else {
            section.is_empty_air = true;
        }

        // Biomes handling (only if not empty air)
        if !section.is_empty_air {
            if let Some(bm) = s.biomes {
                section.biomes = bm.palette;
                if section.biomes.is_empty() {
                    section.biomes.push("minecraft:plains".to_string());
                }
                if let Some(ref data) = bm.data {
                    let p_len = section.biomes.len();
                    let bits = if p_len <= 1 {
                        0
                    } else {
                        let mut b = 1;
                        while (1 << b) < p_len {
                            b += 1;
                        }
                        b
                    };
                    BitArrayUnpacker::unpack_64(bits, data, &mut section.biome_indices);
                }
            }
        }

        sections.push(section);
    }

    Ok(ChunkData {
        chunk_x,
        chunk_z,
        sections,
    })
}

fn legacy_id_to_name(id: u16, meta: u8) -> String {
    match id {
        0 => "minecraft:air".to_string(),
        1 => match meta {
            1 => "minecraft:granite".to_string(),
            2 => "minecraft:polished_granite".to_string(),
            3 => "minecraft:diorite".to_string(),
            4 => "minecraft:polished_diorite".to_string(),
            5 => "minecraft:andesite".to_string(),
            6 => "minecraft:polished_andesite".to_string(),
            _ => "minecraft:stone".to_string(),
        },
        2 => "minecraft:grass_block".to_string(),
        3 => match meta {
            1 => "minecraft:coarse_dirt".to_string(),
            2 => "minecraft:podzol".to_string(),
            _ => "minecraft:dirt".to_string(),
        },
        4 => "minecraft:cobblestone".to_string(),
        5 => match meta {
            1 => "minecraft:spruce_planks".to_string(),
            2 => "minecraft:birch_planks".to_string(),
            3 => "minecraft:jungle_planks".to_string(),
            4 => "minecraft:acacia_planks".to_string(),
            5 => "minecraft:dark_oak_planks".to_string(),
            _ => "minecraft:oak_planks".to_string(),
        },
        7 => "minecraft:bedrock".to_string(),
        8 | 9 => "minecraft:water".to_string(),
        10 | 11 => "minecraft:lava".to_string(),
        12 => match meta {
            1 => "minecraft:red_sand".to_string(),
            _ => "minecraft:sand".to_string(),
        },
        13 => "minecraft:gravel".to_string(),
        14 => "minecraft:gold_ore".to_string(),
        15 => "minecraft:iron_ore".to_string(),
        16 => "minecraft:coal_ore".to_string(),
        17 => match meta & 3 {
            1 => "minecraft:spruce_log".to_string(),
            2 => "minecraft:birch_log".to_string(),
            3 => "minecraft:jungle_log".to_string(),
            _ => "minecraft:oak_log".to_string(),
        },
        18 => match meta & 3 {
            1 => "minecraft:spruce_leaves".to_string(),
            2 => "minecraft:birch_leaves".to_string(),
            3 => "minecraft:jungle_leaves".to_string(),
            _ => "minecraft:oak_leaves".to_string(),
        },
        20 => "minecraft:glass".to_string(),
        21 => "minecraft:lapis_ore".to_string(),
        22 => "minecraft:lapis_block".to_string(),
        24 => "minecraft:sandstone".to_string(),
        35 => "minecraft:white_wool".to_string(),
        41 => "minecraft:gold_block".to_string(),
        42 => "minecraft:iron_block".to_string(),
        43 | 44 => "minecraft:stone_slab".to_string(),
        45 => "minecraft:bricks".to_string(),
        46 => "minecraft:tnt".to_string(),
        47 => "minecraft:bookshelf".to_string(),
        48 => "minecraft:mossy_cobblestone".to_string(),
        49 => "minecraft:obsidian".to_string(),
        50 => "minecraft:torch".to_string(),
        51 => "minecraft:fire".to_string(),
        56 => "minecraft:diamond_ore".to_string(),
        57 => "minecraft:diamond_block".to_string(),
        _ => "minecraft:stone".to_string(),
    }
}
