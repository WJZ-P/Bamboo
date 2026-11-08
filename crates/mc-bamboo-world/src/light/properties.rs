//! 光照引擎看方块的那几样性质，`LightEngine` 和 `ChunkSkyLightSources` 里的静态方法。
//!
//! 数据都来自官方服务端生成的表，这里只是把它们按官方的规则组合起来。

use mc_bamboo_core::Direction;
use mc_bamboo_data::{
    face_shape_occludes, facts, light_dampening, light_emission, light_occlusion_faces, state_facts,
};

/// `getLightEmission`。
pub(crate) fn emission(state: u32) -> u8 {
    light_emission(state)
}

/// `LightEngine.getOpacity`：光每走一格至少减 1。
pub(crate) fn opacity(state: u32) -> u8 {
    light_dampening(state).max(1)
}

/// `LightEngine.isEmptyShape`：不遮挡，或者不按形状遮挡光。
pub(crate) fn is_empty_shape(state: u32) -> bool {
    let facts = state_facts(state);
    facts & facts::CAN_OCCLUDE == 0 || facts & facts::USE_SHAPE_FOR_LIGHT_OCCLUSION == 0
}

/// `LightEngine.getOcclusionShape(state, direction)` 的编号。
fn occlusion_face(state: u32, direction: Direction) -> u16 {
    light_occlusion_faces(state)[direction.id() as usize]
}

/// `LightEngine.shapeOccludes`：光从 `from` 朝 `direction` 走进 `to` 时被两边的形状挡住。
pub(crate) fn shape_occludes(from: u32, to: u32, direction: Direction) -> bool {
    face_shape_occludes(
        occlusion_face(from, direction),
        occlusion_face(to, direction.opposite()),
    )
}

/// `ChunkSkyLightSources.isEdgeOccluded`：天空光能不能从 `top` 直直地照进下面的 `bottom`。
pub(crate) fn is_edge_occluded(top: u32, bottom: u32) -> bool {
    light_dampening(bottom) != 0
        || face_shape_occludes(
            occlusion_face(top, Direction::Down),
            occlusion_face(bottom, Direction::Up),
        )
}

/// `LightEngine.hasDifferentLightProperties`：换掉方块以后光照要不要重算。
pub fn has_different_light_properties(old: u32, new: u32) -> bool {
    let uses_shape = |state| state_facts(state) & facts::USE_SHAPE_FOR_LIGHT_OCCLUSION != 0;
    old != new
        && (light_dampening(new) != light_dampening(old)
            || light_emission(new) != light_emission(old)
            || uses_shape(new)
            || uses_shape(old))
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::blocks;

    use super::*;

    /// 默认状态是不含水的下半砖。type（top、bottom、double）在前，waterlogged 在后。
    const BOTTOM_SLAB: u32 = blocks::OAK_SLAB;
    const TOP_SLAB: u32 = blocks::OAK_SLAB - 2;

    #[test]
    fn torches_glow_and_stone_blocks() {
        assert_eq!(emission(blocks::TORCH), 14);
        assert_eq!(emission(blocks::STONE), 0);
        assert_eq!(opacity(blocks::AIR), 1);
        assert_eq!(opacity(blocks::STONE), 15);
        assert_eq!(opacity(blocks::GLASS), 1);
    }

    #[test]
    fn slabs_occlude_by_their_shape() {
        assert!(!is_empty_shape(BOTTOM_SLAB));
        assert!(is_empty_shape(blocks::GLASS));
        assert!(is_empty_shape(blocks::STONE));

        // 下半砖的底面是满的，顶面是空的。
        assert!(shape_occludes(blocks::AIR, BOTTOM_SLAB, Direction::Up));
        assert!(!shape_occludes(blocks::AIR, BOTTOM_SLAB, Direction::Down));
        assert!(shape_occludes(BOTTOM_SLAB, blocks::AIR, Direction::Down));
        assert!(!shape_occludes(BOTTOM_SLAB, BOTTOM_SLAB, Direction::North));
        // 上半砖在下半砖上面一格，两块中间空着半格，光能穿过去；反过来就贴在一起了。
        assert!(!shape_occludes(TOP_SLAB, BOTTOM_SLAB, Direction::Down));
        assert!(shape_occludes(BOTTOM_SLAB, TOP_SLAB, Direction::Down));
    }

    #[test]
    fn sky_light_stops_on_top_of_slabs_and_dampening_blocks() {
        assert!(is_edge_occluded(blocks::AIR, blocks::STONE));
        assert!(is_edge_occluded(blocks::AIR, blocks::OAK_LEAVES));
        assert!(!is_edge_occluded(blocks::AIR, blocks::GLASS));
        assert!(!is_edge_occluded(blocks::AIR, BOTTOM_SLAB));
        assert!(is_edge_occluded(BOTTOM_SLAB, blocks::AIR));
    }

    #[test]
    fn shaped_blocks_always_need_a_light_check() {
        assert!(!has_different_light_properties(
            blocks::STONE,
            blocks::STONE
        ));
        assert!(!has_different_light_properties(blocks::STONE, blocks::DIRT));
        assert!(has_different_light_properties(blocks::AIR, blocks::STONE));
        assert!(has_different_light_properties(blocks::AIR, blocks::TORCH));
        assert!(has_different_light_properties(BOTTOM_SLAB, TOP_SLAB));
    }
}
