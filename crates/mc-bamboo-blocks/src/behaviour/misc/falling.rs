//! `ConcretePowderBlock`、`DragonEggBlock`。
//!
//! 它们的父类 `FallingBlock` 排的计划刻只用来下落，下落不在范围里，这里都不排。
//! 没有别的覆盖的子类 `ColoredFallingBlock`、`SandBlock` 不用写结构体。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::Block;

use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{default_state, field_block},
    },
    context::{Actor, PlaceContext, UseContext},
    interact::InteractionResult,
    state::{BlockState, SupportType},
    world::{UPDATE_CLIENTS, World},
};

/// `ConcretePowderBlock`。
pub(super) struct ConcretePowder;

/// `canSolidify`。
fn can_solidify(state: BlockState) -> bool {
    state.fluid().is_water()
}

/// `touchesLiquid`：旁边或者上面有水，而且水没有被完整的面挡着。
/// 下面的水只有这一格自己也有水时才算，这样粉末能落在水底的方块上。
fn touches_liquid(world: &World, pos: BlockPos) -> bool {
    let wet_here = can_solidify(world.block_state(pos));
    Direction::ALL
        .into_iter()
        .filter(|&side| side != Direction::Down || wet_here)
        .any(|side| {
            let neighbor = world.block_state(pos.relative(side));
            can_solidify(neighbor) && !neighbor.is_face_sturdy(side.opposite(), SupportType::Full)
        })
}

fn concrete_of(state: BlockState) -> BlockState {
    default_state(field_block(state.block(), "concrete"))
}

impl Behaviour for ConcretePowder {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let pos = ctx.clicked_pos();
        let default = default_state(block);
        let solid = can_solidify(world.block_state(pos)) || touches_liquid(world, pos);
        Some(if solid { concrete_of(default) } else { default })
    }

    fn update_shape(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        direction: Direction,
        neighbor_pos: BlockPos,
        neighbor: BlockState,
    ) -> BlockState {
        if touches_liquid(world, pos) {
            return concrete_of(state);
        }
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }
}

/// `DragonEggBlock`：右键时传送到附近随机一个下面有东西的空格。
pub(super) struct DragonEgg;

/// 传送最多试这么多次。
const TELEPORT_ATTEMPTS: i32 = 1000;

/// `BlockUtil.packDifferenceInPosition`：横向半径 16、纵向 8，位移加上半径后各取一字节。
fn packed_difference(from: BlockPos, to: BlockPos) -> i32 {
    let x = (to.x - from.x + 16) & 0xFF;
    let y = (to.y - from.y + 8) & 0xFF;
    let z = (to.z - from.z + 16) & 0xFF;
    x << 16 | y << 8 | z
}

impl DragonEgg {
    /// `teleport`：找到位置后先报粒子，再挪方块。
    fn teleport(state: BlockState, world: &mut World, pos: BlockPos) {
        for _ in 0..TELEPORT_ATTEMPTS {
            let random = world.random();
            let dx = random.next_int(16) - random.next_int(16);
            let dy = random.next_int(8) - random.next_int(8);
            let dz = random.next_int(16) - random.next_int(16);
            let target = pos.offset(dx, dy, dz);
            if world.block_state(target).is_air()
                && !world.block_state(target.below()).is_air()
                && world.is_within_world_border(target)
                && world.is_inside_build_height(target.y)
            {
                world.level_event(2015, pos, packed_difference(pos, target));
                world.set_block(target, state, UPDATE_CLIENTS);
                world.remove_block(pos);
                return;
            }
        }
    }
}

impl Behaviour for DragonEgg {
    /// `DragonEggBlock.attack`：拿手打一下和右键一样，传送到附近。
    fn attack(&self, state: BlockState, world: &mut World, pos: BlockPos, _player: &Actor) {
        Self::teleport(state, world, pos);
    }

    fn use_without_item(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        Self::teleport(state, world, ctx.clicked_pos());
        InteractionResult::Success
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::{blocks, items};

    use super::*;
    use crate::testing::{TestLevel, actor_holding, top_hit, use_item};

    /// 只有一格能落脚时，龙蛋迟早传送到那里。
    #[test]
    fn a_dragon_egg_teleports_onto_a_block() {
        let pos = BlockPos::new(0, 64, 0);
        let landing = BlockPos::new(3, 64, -2);
        let mut level = TestLevel::new();
        level.set(pos, BlockState(blocks::DRAGON_EGG));
        level.set(pos.below(), BlockState(blocks::STONE));
        level.set(landing.below(), BlockState(blocks::STONE));
        let outcome = use_item(&mut level, &actor_holding(items::AIR), top_hit(pos));
        assert_eq!(outcome.result, InteractionResult::Success);
        let egg = BlockState(blocks::DRAGON_EGG);
        let moved = level.get(landing) == egg && level.get(pos).is_air();
        let stayed = level.get(pos) == egg;
        assert!(moved || stayed);
    }
}
