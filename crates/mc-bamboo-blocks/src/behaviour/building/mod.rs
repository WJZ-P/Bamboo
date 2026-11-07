//! 建筑方块：柱子、台阶、楼梯、栅栏、栅栏门、墙、玻璃板、铁栏杆、门、活板门、床、锁链、
//! 地毯、带釉陶瓦、巨型蘑菇、雪草。
//!
//! 一个文件放一组官方类，文件头写着里面有哪些类。这里只有分派和几个类共用的小函数。

mod bed;
mod carpet;
mod door;
mod fence;
mod fence_gate;
mod glazed_terracotta;
mod huge_mushroom;
mod iron_bars;
mod pillar;
mod slab;
mod snowy;
mod stairs;
mod trapdoor;
mod wall;

use mc_bamboo_core::Direction;
use mc_bamboo_data::BlockClass;

use self::{
    bed::Bed,
    carpet::{Carpet, MossyCarpet},
    door::Door,
    fence::Fence,
    fence_gate::FenceGate,
    glazed_terracotta::GlazedTerracotta,
    huge_mushroom::HugeMushroom,
    iron_bars::IronBars,
    pillar::{Chain, RotatedPillar},
    slab::Slab,
    snowy::Snowy,
    stairs::Stair,
    trapdoor::TrapDoor,
    wall::Wall,
};
use super::Behaviour;
use crate::{
    context::PlaceContext,
    state::{BlockState, SupportType},
};

/// 这个家族里类到行为的分派。新版本多出来、还没移植的类是 `None`。
pub(super) fn of(class: BlockClass) -> Option<&'static dyn Behaviour> {
    use BlockClass as C;

    match class {
        C::BedBlock | C::StrawBedBlock => Some(&Bed),
        C::CarpetBlock | C::WoolCarpetBlock => Some(&Carpet),
        C::ChainBlock | C::WeatheringCopperChainBlock => Some(&Chain),
        C::DoorBlock | C::WeatheringCopperDoorBlock => Some(&Door),
        C::FenceBlock => Some(&Fence),
        C::FenceGateBlock => Some(&FenceGate),
        C::GlazedTerracottaBlock => Some(&GlazedTerracotta),
        C::GrassBlock | C::MyceliumBlock | C::SnowyBlock => Some(&Snowy),
        C::HugeMushroomBlock => Some(&HugeMushroom),
        C::IronBarsBlock | C::StainedGlassPaneBlock | C::WeatheringCopperBarsBlock => {
            Some(&IronBars)
        }
        C::MossyCarpetBlock => Some(&MossyCarpet),
        C::HayBlock | C::InfestedRotatedPillarBlock | C::RotatedPillarBlock => Some(&RotatedPillar),
        C::SlabBlock | C::WeatheringCopperSlabBlock => Some(&Slab),
        C::StairBlock | C::WeatheringCopperStairBlock => Some(&Stair),
        C::TrapDoorBlock | C::WeatheringCopperTrapDoorBlock => Some(&TrapDoor),
        C::WallBlock => Some(&Wall),
        _ => None,
    }
}

/// 点击点在要放的这一格的上半部分。
fn clicked_upper_half(ctx: &PlaceContext) -> bool {
    ctx.click_location().y - f64::from(ctx.clicked_pos().y) > 0.5
}

/// `isFaceSturdy(level, pos, face)`：这个状态的这个面是完整的。
///
/// 形状更新拿到的邻居可能比世界里的新，也可能比世界里的旧，所以不用
/// [`World::is_face_sturdy`](crate::World::is_face_sturdy) 重新读世界。
/// 形状跟着渲染偏移走的方块都不是整面，查表的结果在任何位置上都对。
fn has_full_face(state: BlockState, face: Direction) -> bool {
    state.is_face_sturdy(face, SupportType::Full)
}

#[cfg(test)]
mod test_support {
    use mc_bamboo_core::{BlockPos, Direction, Vec3};
    use mc_bamboo_data::blocks;

    use crate::{BlockHit, BlockState, testing::TestLevel};

    pub(super) const FLOOR: BlockPos = BlockPos::new(0, 64, 0);

    /// 点在 `pos` 这一格 `face` 面的 `(fx, fy, fz)` 处，这是点击点在方块内的相对坐标。
    pub(super) fn hit(pos: BlockPos, face: Direction, fx: f64, fy: f64, fz: f64) -> BlockHit {
        BlockHit {
            pos,
            face,
            location: Vec3::new(
                f64::from(pos.x) + fx,
                f64::from(pos.y) + fy,
                f64::from(pos.z) + fz,
            ),
            inside: false,
        }
    }

    pub(super) fn top_of(pos: BlockPos) -> BlockHit {
        hit(pos, Direction::Up, 0.5, 1.0, 0.5)
    }

    pub(super) fn side_of(pos: BlockPos, face: Direction, fy: f64) -> BlockHit {
        let (dx, _, dz) = face.normal();
        hit(
            pos,
            face,
            0.5 + 0.5 * f64::from(dx),
            fy,
            0.5 + 0.5 * f64::from(dz),
        )
    }

    pub(super) fn level_with_floor() -> TestLevel {
        let mut level = TestLevel::new();
        level.set(FLOOR, BlockState(blocks::STONE));
        level
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::BlockPos;
    use mc_bamboo_data::blocks;

    use super::*;
    use crate::shape;

    /// [`has_full_face`] 不看位置的依据。
    #[test]
    fn shapes_that_follow_the_render_offset_never_have_a_full_face() {
        let samples = [BlockPos::new(0, 0, 0), BlockPos::new(5, 0, -7)];
        for block in blocks::BLOCKS {
            if !block.offset.is_some_and(|offset| offset.moves_shape) {
                continue;
            }
            for id in block.first_state..block.first_state + state_count(block) {
                let state = BlockState(id);
                for face in Direction::ALL {
                    assert!(!has_full_face(state, face), "{}", block.name);
                    for pos in samples {
                        let boxes = shape::support_boxes(state, pos);
                        assert!(!shape::is_face_full(&boxes, face), "{}", block.name);
                    }
                }
            }
        }
    }

    fn state_count(block: &mc_bamboo_data::Block) -> u32 {
        block
            .properties
            .iter()
            .map(|property| property.values.len() as u32)
            .product()
    }
}
