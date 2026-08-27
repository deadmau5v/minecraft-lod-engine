pub mod heightmap;
pub mod octree;
pub mod palette_lut;
pub mod simd_blend;

pub use heightmap::{ChunkVoxelGrid, ColumnData, ColumnVoxelPoint};
pub use octree::LodSection;
pub use palette_lut::{BlockStateMaterial, GlobalPaletteLut};
pub use simd_blend::{blend_8_colors, blend_8_colors_scalar};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simd_blend_accuracy() {
        let colors = [
            0xFF102030, 0xFF203040, 0xFF304050, 0xFF405060, 0xFF506070, 0xFF607080, 0xFF708090,
            0xFF8090A0,
        ];
        let scalar_res = blend_8_colors_scalar(&colors);
        let simd_res = blend_8_colors(&colors);
        assert_eq!(scalar_res, simd_res);
    }

    #[test]
    fn test_palette_lut() {
        let lut = GlobalPaletteLut::get_global();
        let mat = lut.get_material_by_name("minecraft:stone");
        assert_eq!(mat.base_color, 0xFF7D7D7D);
        assert_ne!(mat.opacity, 0);

        let air = lut.get_material_by_name("minecraft:air");
        assert_eq!(air.base_color, 0);
    }
}
