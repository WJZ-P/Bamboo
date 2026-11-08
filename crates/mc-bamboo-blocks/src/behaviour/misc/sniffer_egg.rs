//! `SnifferEggBlock`。孵化出嗅探兽不在范围里，只排孵化的计划刻。

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::tags::block_tags;

use crate::{behaviour::Behaviour, state::BlockState, world::World};

/// 普通情况下孵化要多少刻。
const HATCH_TIME: i32 = 24_000;
/// 底下是加速孵化的方块时要多少刻。
const BOOSTED_HATCH_TIME: i32 = 12_000;
/// 孵化分三个阶段，每个阶段排一次计划刻。
const HATCH_STAGES: i32 = 3;
/// 每次计划刻在阶段时长上多加的随机刻数的上界（不含）。
const RANDOM_EXTRA_DELAY: i32 = 300;

/// `SnifferEggBlock`。
pub(super) struct SnifferEgg;

/// `hatchBoost`：底下的方块能加速孵化。
fn hatch_boost(world: &World, pos: BlockPos) -> bool {
    world
        .block_state(pos.below())
        .is_tagged(block_tags::SNIFFER_EGG_HATCH_BOOST)
}

impl Behaviour for SnifferEgg {
    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, _old: BlockState) {
        // 孵化本身不在范围里，但这次计划刻要照官方从世界随机源取数，不然后面的随机数都会错位。
        let hatch_time = if hatch_boost(world, pos) {
            BOOSTED_HATCH_TIME
        } else {
            HATCH_TIME
        };
        let delay = hatch_time / HATCH_STAGES + world.random().next_int(RANDOM_EXTRA_DELAY);
        world.schedule_tick(pos, state.block(), delay);
    }
}
