//! Global block state material and color lookup table (LUT).
//!
//! Provides ultra-fast O(1) RGB color, opacity, light emission, and block flag lookups
//! for Minecraft block state registry keys.

use ahash::AHashMap;
use std::sync::OnceLock;

/// Material attributes for a Minecraft block state.
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockStateMaterial {
    /// 32-bit ARGB color (0xAARRGGBB).
    pub base_color: u32,
    /// Bitmask flags: opaque, liquid, foliage, grass, water, air.
    pub flags: u16,
    /// Light emission level (0..=15).
    pub light_emission: u8,
    /// Light absorption / opacity level (0..=15).
    pub opacity: u8,
}

pub const FLAG_OPAQUE: u16 = 1 << 0;
pub const FLAG_LIQUID: u16 = 1 << 1;
pub const FLAG_FOLIAGE: u16 = 1 << 2;
pub const FLAG_GRASS: u16 = 1 << 3;
pub const FLAG_WATER: u16 = 1 << 4;
pub const FLAG_AIR: u16 = 1 << 5;

static GLOBAL_LUT: OnceLock<GlobalPaletteLut> = OnceLock::new();

/// Global palette lookup table mapping block identifiers to materials.
pub struct GlobalPaletteLut {
    materials: Vec<BlockStateMaterial>,
    name_to_id: AHashMap<String, u32>,
}

impl GlobalPaletteLut {
    /// Retrieves singleton static reference to the global palette LUT.
    pub fn get_global() -> &'static Self {
        GLOBAL_LUT.get_or_init(Self::init_default)
    }

    fn init_default() -> Self {
        let mut lut = Self {
            materials: Vec::with_capacity(1024),
            name_to_id: AHashMap::new(),
        };

        // 0: Air & Non-solid
        lut.register("minecraft:air", 0x00000000, FLAG_AIR, 0, 0);
        lut.register("minecraft:cave_air", 0x00000000, FLAG_AIR, 0, 0);
        lut.register("minecraft:void_air", 0x00000000, FLAG_AIR, 0, 0);
        lut.register("minecraft:structure_void", 0x00000000, FLAG_AIR, 0, 0);
        lut.register("minecraft:barrier", 0x00000000, FLAG_AIR, 0, 0);
        lut.register("minecraft:light", 0x00000000, FLAG_AIR, 15, 0);

        // Stone and geological formations
        lut.register("minecraft:stone", 0xFF7D7D7D, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:granite", 0xFF956755, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:polished_granite", 0xFF956755, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:diorite", 0xFFBEBEBE, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:polished_diorite", 0xFFBEBEBE, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:andesite", 0xFF888888, FLAG_OPAQUE, 0, 15);
        lut.register(
            "minecraft:polished_andesite",
            0xFF888888,
            FLAG_OPAQUE,
            0,
            15,
        );
        lut.register("minecraft:deepslate", 0xFF4F4F54, FLAG_OPAQUE, 0, 15);
        lut.register(
            "minecraft:cobbled_deepslate",
            0xFF4D4D52,
            FLAG_OPAQUE,
            0,
            15,
        );
        lut.register(
            "minecraft:polished_deepslate",
            0xFF48484E,
            FLAG_OPAQUE,
            0,
            15,
        );
        lut.register("minecraft:deepslate_bricks", 0xFF46464B, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:deepslate_tiles", 0xFF36363B, FLAG_OPAQUE, 0, 15);
        lut.register(
            "minecraft:reinforced_deepslate",
            0xFF4D4F4F,
            FLAG_OPAQUE,
            0,
            15,
        );
        lut.register("minecraft:tuff", 0xFF6C6D64, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:calcite", 0xFFDFE0DF, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:dripstone_block", 0xFF866555, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:pointed_dripstone", 0xFF866555, 0, 0, 1);
        lut.register("minecraft:bedrock", 0xFF535353, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:cobblestone", 0xFF808080, FLAG_OPAQUE, 0, 15);
        lut.register(
            "minecraft:mossy_cobblestone",
            0xFF738268,
            FLAG_OPAQUE,
            0,
            15,
        );
        lut.register("minecraft:stone_bricks", 0xFF7A7A7A, FLAG_OPAQUE, 0, 15);
        lut.register(
            "minecraft:mossy_stone_bricks",
            0xFF738268,
            FLAG_OPAQUE,
            0,
            15,
        );
        lut.register(
            "minecraft:cracked_stone_bricks",
            0xFF757575,
            FLAG_OPAQUE,
            0,
            15,
        );
        lut.register(
            "minecraft:chiseled_stone_bricks",
            0xFF777777,
            FLAG_OPAQUE,
            0,
            15,
        );
        lut.register("minecraft:smooth_stone", 0xFF9E9E9E, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:obsidian", 0xFF14121E, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:crying_obsidian", 0xFF2A153E, FLAG_OPAQUE, 10, 15);

        // Dirt, Grass, Foliage & Soils
        lut.register(
            "minecraft:grass_block",
            0xFF5B8C32,
            FLAG_OPAQUE | FLAG_GRASS,
            0,
            15,
        );
        lut.register("minecraft:dirt", 0xFF866043, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:coarse_dirt", 0xFF77553B, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:podzol", 0xFF5B3F1E, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:rooted_dirt", 0xFF90674C, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:mud", 0xFF3C393E, FLAG_OPAQUE, 0, 15);
        lut.register(
            "minecraft:muddy_mangrove_roots",
            0xFF473F35,
            FLAG_OPAQUE,
            0,
            15,
        );
        lut.register("minecraft:packed_mud", 0xFF8D6B51, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:mud_bricks", 0xFF89674F, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:farmland", 0xFF553823, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:dirt_path", 0xFF97753B, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:moss_block", 0xFF596E2D, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:moss_carpet", 0xFF596E2D, FLAG_FOLIAGE, 0, 1);
        lut.register("minecraft:mycelium", 0xFF6F6265, FLAG_OPAQUE, 0, 15);

        // Sands and Gravel
        lut.register("minecraft:sand", 0xFFDBCFA3, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:red_sand", 0xFFBE6721, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:sandstone", 0xFFD8CB9B, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:cut_sandstone", 0xFFD8CB9B, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:smooth_sandstone", 0xFFD8CB9B, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:red_sandstone", 0xFFBA631C, FLAG_OPAQUE, 0, 15);
        lut.register(
            "minecraft:cut_red_sandstone",
            0xFFBA631C,
            FLAG_OPAQUE,
            0,
            15,
        );
        lut.register(
            "minecraft:smooth_red_sandstone",
            0xFFBA631C,
            FLAG_OPAQUE,
            0,
            15,
        );
        lut.register("minecraft:gravel", 0xFF837F7E, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:clay", 0xFFA0A7B4, FLAG_OPAQUE, 0, 15);

        // Water and Liquids
        lut.register(
            "minecraft:water",
            0xCC3F76E4,
            FLAG_LIQUID | FLAG_WATER,
            0,
            2,
        );
        lut.register(
            "minecraft:bubble_column",
            0xCC3F76E4,
            FLAG_LIQUID | FLAG_WATER,
            0,
            2,
        );
        lut.register(
            "minecraft:lava",
            0xFFD45A12,
            FLAG_LIQUID | FLAG_OPAQUE,
            15,
            15,
        );

        // Woods / Planks / Logs / Stems
        lut.register("minecraft:oak_planks", 0xFFA2824E, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:spruce_planks", 0xFF684E32, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:birch_planks", 0xFFC2B176, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:jungle_planks", 0xFFA07351, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:acacia_planks", 0xFFA85A32, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:dark_oak_planks", 0xFF422A15, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:mangrove_planks", 0xFF763631, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:cherry_planks", 0xFFE5B2B0, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:bamboo_planks", 0xFFC09B42, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:bamboo_mosaic", 0xFFBFA044, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:bamboo_block", 0xFF7F9536, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:crimson_planks", 0xFF653046, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:warped_planks", 0xFF2B6863, FLAG_OPAQUE, 0, 15);

        lut.register("minecraft:oak_log", 0xFF6B5330, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:spruce_log", 0xFF3B2713, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:birch_log", 0xFFD7D7D7, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:jungle_log", 0xFF554419, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:acacia_log", 0xFF676157, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:dark_oak_log", 0xFF362818, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:mangrove_log", 0xFF542421, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:cherry_log", 0xFF362326, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:crimson_stem", 0xFF5C182A, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:warped_stem", 0xFF2B6863, FLAG_OPAQUE, 0, 15);

        // Stripped Logs
        lut.register("minecraft:stripped_oak_log", 0xFFA2824E, FLAG_OPAQUE, 0, 15);
        lut.register(
            "minecraft:stripped_spruce_log",
            0xFF684E32,
            FLAG_OPAQUE,
            0,
            15,
        );
        lut.register(
            "minecraft:stripped_birch_log",
            0xFFC5B88D,
            FLAG_OPAQUE,
            0,
            15,
        );
        lut.register(
            "minecraft:stripped_jungle_log",
            0xFFA07351,
            FLAG_OPAQUE,
            0,
            15,
        );
        lut.register(
            "minecraft:stripped_acacia_log",
            0xFFA85A32,
            FLAG_OPAQUE,
            0,
            15,
        );
        lut.register(
            "minecraft:stripped_dark_oak_log",
            0xFF422A15,
            FLAG_OPAQUE,
            0,
            15,
        );
        lut.register(
            "minecraft:stripped_mangrove_log",
            0xFF763631,
            FLAG_OPAQUE,
            0,
            15,
        );
        lut.register(
            "minecraft:stripped_cherry_log",
            0xFFE5B2B0,
            FLAG_OPAQUE,
            0,
            15,
        );

        // Foliage & Leaves
        lut.register("minecraft:oak_leaves", 0xFF3C7C22, FLAG_FOLIAGE, 0, 3);
        lut.register("minecraft:spruce_leaves", 0xFF3B5B3E, FLAG_FOLIAGE, 0, 3);
        lut.register("minecraft:birch_leaves", 0xFF608436, FLAG_FOLIAGE, 0, 3);
        lut.register("minecraft:jungle_leaves", 0xFF307416, FLAG_FOLIAGE, 0, 3);
        lut.register("minecraft:acacia_leaves", 0xFF4E771F, FLAG_FOLIAGE, 0, 3);
        lut.register("minecraft:dark_oak_leaves", 0xFF2C5611, FLAG_FOLIAGE, 0, 3);
        lut.register("minecraft:mangrove_leaves", 0xFF517623, FLAG_FOLIAGE, 0, 3);
        lut.register("minecraft:cherry_leaves", 0xFFF1A8C0, FLAG_FOLIAGE, 0, 3);
        lut.register("minecraft:azalea_leaves", 0xFF586F30, FLAG_FOLIAGE, 0, 3);
        lut.register(
            "minecraft:flowering_azalea_leaves",
            0xFF6B6E3A,
            FLAG_FOLIAGE,
            0,
            3,
        );
        lut.register(
            "minecraft:nether_wart_block",
            0xFF730000,
            FLAG_OPAQUE,
            0,
            15,
        );
        lut.register(
            "minecraft:warped_wart_block",
            0xFF147470,
            FLAG_OPAQUE,
            0,
            15,
        );
        lut.register("minecraft:shroomlight", 0xFFF39239, FLAG_OPAQUE, 15, 15);

        // Ores & Minerals
        lut.register("minecraft:coal_ore", 0xFF464646, FLAG_OPAQUE, 0, 15);
        lut.register(
            "minecraft:deepslate_coal_ore",
            0xFF353539,
            FLAG_OPAQUE,
            0,
            15,
        );
        lut.register("minecraft:iron_ore", 0xFF8A7F75, FLAG_OPAQUE, 0, 15);
        lut.register(
            "minecraft:deepslate_iron_ore",
            0xFF58534C,
            FLAG_OPAQUE,
            0,
            15,
        );
        lut.register("minecraft:copper_ore", 0xFF7F786C, FLAG_OPAQUE, 0, 15);
        lut.register(
            "minecraft:deepslate_copper_ore",
            0xFF55524B,
            FLAG_OPAQUE,
            0,
            15,
        );
        lut.register("minecraft:gold_ore", 0xFF9E8D62, FLAG_OPAQUE, 0, 15);
        lut.register(
            "minecraft:deepslate_gold_ore",
            0xFF6A6045,
            FLAG_OPAQUE,
            0,
            15,
        );
        lut.register("minecraft:redstone_ore", 0xFF965B5B, FLAG_OPAQUE, 0, 15);
        lut.register(
            "minecraft:deepslate_redstone_ore",
            0xFF684141,
            FLAG_OPAQUE,
            0,
            15,
        );
        lut.register("minecraft:lapis_ore", 0xFF5B6984, FLAG_OPAQUE, 0, 15);
        lut.register(
            "minecraft:deepslate_lapis_ore",
            0xFF404A5D,
            FLAG_OPAQUE,
            0,
            15,
        );
        lut.register("minecraft:diamond_ore", 0xFF5C8B8E, FLAG_OPAQUE, 0, 15);
        lut.register(
            "minecraft:deepslate_diamond_ore",
            0xFF416365,
            FLAG_OPAQUE,
            0,
            15,
        );
        lut.register("minecraft:emerald_ore", 0xFF598565, FLAG_OPAQUE, 0, 15);
        lut.register(
            "minecraft:deepslate_emerald_ore",
            0xFF3E5C46,
            FLAG_OPAQUE,
            0,
            15,
        );
        lut.register(
            "minecraft:nether_quartz_ore",
            0xFF773B3B,
            FLAG_OPAQUE,
            0,
            15,
        );
        lut.register("minecraft:nether_gold_ore", 0xFF783E2D, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:ancient_debris", 0xFF59453C, FLAG_OPAQUE, 0, 15);

        // Snow / Ice
        lut.register("minecraft:snow", 0xFFFAFAFA, 0, 0, 1);
        lut.register("minecraft:snow_block", 0xFFFAFAFA, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:ice", 0xCC91B5FC, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:packed_ice", 0xFF8DB2FB, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:blue_ice", 0xFF74A7FD, FLAG_OPAQUE, 0, 15);

        // Nether & End
        lut.register("minecraft:netherrack", 0xFF632828, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:soul_sand", 0xFF4F3C31, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:soul_soil", 0xFF48372D, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:glowstone", 0xFFFFBC5E, FLAG_OPAQUE, 15, 15);
        lut.register("minecraft:basalt", 0xFF4E4B54, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:polished_basalt", 0xFF55525B, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:smooth_basalt", 0xFF46454A, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:blackstone", 0xFF27222A, FLAG_OPAQUE, 0, 15);
        lut.register(
            "minecraft:polished_blackstone",
            0xFF312C36,
            FLAG_OPAQUE,
            0,
            15,
        );
        lut.register(
            "minecraft:polished_blackstone_bricks",
            0xFF2B2630,
            FLAG_OPAQUE,
            0,
            15,
        );
        lut.register("minecraft:nether_bricks", 0xFF2C151B, FLAG_OPAQUE, 0, 15);
        lut.register(
            "minecraft:red_nether_bricks",
            0xFF450709,
            FLAG_OPAQUE,
            0,
            15,
        );
        lut.register("minecraft:end_stone", 0xFFD8DE9D, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:end_stone_bricks", 0xFFD7DE9E, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:purpur_block", 0xFFA97DA9, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:purpur_pillar", 0xFFAB80AB, FLAG_OPAQUE, 0, 15);

        // Ocean & Prismarine
        lut.register("minecraft:prismarine", 0xFF5F9E9E, FLAG_OPAQUE, 0, 15);
        lut.register(
            "minecraft:prismarine_bricks",
            0xFF63AB9E,
            FLAG_OPAQUE,
            0,
            15,
        );
        lut.register("minecraft:dark_prismarine", 0xFF35594C, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:sea_lantern", 0xFFB3D3CC, FLAG_OPAQUE, 15, 15);

        // Amethyst & Sculk
        lut.register("minecraft:amethyst_block", 0xFF8460B8, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:budding_amethyst", 0xFF825EB5, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:sculk", 0xFF0D1D24, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:sculk_catalyst", 0xFF0F2027, FLAG_OPAQUE, 6, 15);
        lut.register("minecraft:sculk_shrieker", 0xFF14242A, FLAG_OPAQUE, 0, 15);

        // Terracotta & Stained Blocks
        lut.register("minecraft:terracotta", 0xFF975E44, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:white_terracotta", 0xFFD1B2A1, FLAG_OPAQUE, 0, 15);
        lut.register(
            "minecraft:orange_terracotta",
            0xFFA05325,
            FLAG_OPAQUE,
            0,
            15,
        );
        lut.register(
            "minecraft:magenta_terracotta",
            0xFF95576C,
            FLAG_OPAQUE,
            0,
            15,
        );
        lut.register(
            "minecraft:light_blue_terracotta",
            0xFF706C8A,
            FLAG_OPAQUE,
            0,
            15,
        );
        lut.register(
            "minecraft:yellow_terracotta",
            0xFFB98424,
            FLAG_OPAQUE,
            0,
            15,
        );
        lut.register("minecraft:lime_terracotta", 0xFF677535, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:pink_terracotta", 0xFFA04D4E, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:gray_terracotta", 0xFF392923, FLAG_OPAQUE, 0, 15);
        lut.register(
            "minecraft:light_gray_terracotta",
            0xFF876B62,
            FLAG_OPAQUE,
            0,
            15,
        );
        lut.register("minecraft:cyan_terracotta", 0xFF575B5B, FLAG_OPAQUE, 0, 15);
        lut.register(
            "minecraft:purple_terracotta",
            0xFF7A4455,
            FLAG_OPAQUE,
            0,
            15,
        );
        lut.register("minecraft:blue_terracotta", 0xFF4A3B5B, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:brown_terracotta", 0xFF4D3223, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:green_terracotta", 0xFF4C522A, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:red_terracotta", 0xFF8E3C2E, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:black_terracotta", 0xFF251610, FLAG_OPAQUE, 0, 15);

        // Glass & Transparent
        lut.register("minecraft:glass", 0x22FFFFFF, 0, 0, 1);
        lut.register("minecraft:tinted_glass", 0xAA2B2633, FLAG_OPAQUE, 0, 15);

        // Minerals & Construction Blocks
        lut.register("minecraft:white_wool", 0xFFE9ECEC, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:gold_block", 0xFFF8D237, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:iron_block", 0xFFD8D8D8, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:copper_block", 0xFFC06B4F, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:diamond_block", 0xFF5DECF5, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:netherite_block", 0xFF423D3F, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:emerald_block", 0xFF28C757, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:lapis_block", 0xFF18449C, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:redstone_block", 0xFF9E1405, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:coal_block", 0xFF121212, FLAG_OPAQUE, 0, 15);
        lut.register("minecraft:bricks", 0xFF966053, FLAG_OPAQUE, 0, 15);

        lut
    }

    fn register(&mut self, name: &str, color: u32, flags: u16, light: u8, opacity: u8) -> u32 {
        let id = self.materials.len() as u32;
        let clean_name = name.strip_prefix("minecraft:").unwrap_or(name);
        self.materials.push(BlockStateMaterial {
            base_color: color,
            flags,
            light_emission: light,
            opacity,
        });
        self.name_to_id.insert(name.to_string(), id);
        self.name_to_id.insert(clean_name.to_string(), id);
        id
    }

    /// Queries material attributes for a given block namespace key.
    #[inline(always)]
    pub fn get_material_by_name(&self, name: &str) -> BlockStateMaterial {
        let clean = name.strip_prefix("minecraft:").unwrap_or(name);
        if let Some(&id) = self
            .name_to_id
            .get(name)
            .or_else(|| self.name_to_id.get(clean))
        {
            self.materials[id as usize]
        } else {
            // Intelligent heuristic fallback for unknown or modded blocks
            if clean.contains("leaves") {
                BlockStateMaterial {
                    base_color: 0xFF3C7C22,
                    flags: FLAG_FOLIAGE,
                    light_emission: 0,
                    opacity: 3,
                }
            } else if clean.contains("wood")
                || clean.contains("log")
                || clean.contains("plank")
                || clean.contains("stem")
            {
                BlockStateMaterial {
                    base_color: 0xFFA2824E,
                    flags: FLAG_OPAQUE,
                    light_emission: 0,
                    opacity: 15,
                }
            } else if clean.contains("water") {
                BlockStateMaterial {
                    base_color: 0xCC3F76E4,
                    flags: FLAG_LIQUID | FLAG_WATER,
                    light_emission: 0,
                    opacity: 2,
                }
            } else if clean.contains("lava") || clean.contains("fire") {
                BlockStateMaterial {
                    base_color: 0xFFD45A12,
                    flags: FLAG_LIQUID | FLAG_OPAQUE,
                    light_emission: 15,
                    opacity: 15,
                }
            } else if clean.contains("sand") {
                BlockStateMaterial {
                    base_color: 0xFFDBCFA3,
                    flags: FLAG_OPAQUE,
                    light_emission: 0,
                    opacity: 15,
                }
            } else if clean.contains("grass") || clean.contains("dirt") || clean.contains("soil") {
                BlockStateMaterial {
                    base_color: 0xFF5B8C32,
                    flags: FLAG_OPAQUE | FLAG_GRASS,
                    light_emission: 0,
                    opacity: 15,
                }
            } else if clean.contains("glass") || clean.contains("air") || clean.contains("void") {
                BlockStateMaterial {
                    base_color: 0x00000000,
                    flags: FLAG_AIR,
                    light_emission: 0,
                    opacity: 0,
                }
            } else {
                BlockStateMaterial {
                    base_color: 0xFF7D7D7D,
                    flags: FLAG_OPAQUE,
                    light_emission: 0,
                    opacity: 15,
                }
            }
        }
    }
}
