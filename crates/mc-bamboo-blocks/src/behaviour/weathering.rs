//! `ChangeOverTimeBlock` 和 `WeatheringCopper`：铜方块随着时间氧化。
//!
//! 会氧化的铜分散在四个家族里（台阶、门、灯笼、箱子……），它们的随机刻都调用 [`change_over_time`]。

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::{BlockAncestor, block_maps::WEATHERING};

use super::common::{block_by_id, field_name};
use crate::{state::BlockState, world::World};

/// `eachBlockOncePerDayChance`：每次随机刻有这么大的机会考虑氧化。
const CHANGE_CHANCE: f32 = 0.056_888_89;
/// `SCAN_DISTANCE`：数周围的铜时看的曼哈顿距离。
const SCAN_DISTANCE: i32 = 4;
/// 没氧化过的铜氧化得慢一些。
const UNAFFECTED_CHANCE_MODIFIER: f32 = 0.75;
/// `WeatherState` 的四个阶段，下标就是官方的 `ordinal`。
const AGES: [&str; 4] = ["unaffected", "exposed", "weathered", "oxidized"];

/// `getAge().ordinal()`。
fn age(state: BlockState) -> usize {
    let name = field_name(state.block(), "getAge()");
    AGES.iter()
        .position(|&age| age == name)
        .unwrap_or_else(|| panic!("unknown weather state {name}"))
}

/// `WeatheringCopper.getNext(state)`：下一个阶段的方块，属性照抄。
fn next(state: BlockState) -> Option<BlockState> {
    let id = i32::try_from(state.block_id()).expect("方块 ID 放得进 int");
    WEATHERING
        .iter()
        .find(|&&(from, _)| from == id)
        .map(|&(_, to)| state.copied_onto(block_by_id(to)))
}

/// `changeOverTime`。
pub fn change_over_time(state: BlockState, world: &mut World, pos: BlockPos) {
    if world.random().next_float() < CHANGE_CHANCE
        && let Some(weathered) = next_state(state, world, pos)
    {
        world.set_block_and_update(pos, weathered);
    }
}

/// `getNextState`：周围有更新的铜时不变；否则周围更老的铜越多，越可能变成下一个阶段。
fn next_state(state: BlockState, world: &World, pos: BlockPos) -> Option<BlockState> {
    let (older, same) = count_neighbours(age(state), world, pos)?;
    let chance = (older + 1) as f32 / (older + same + 1) as f32;
    let modifier = if age(state) == 0 {
        UNAFFECTED_CHANCE_MODIFIER
    } else {
        1.0
    };
    let actual = chance * chance * modifier;
    if world.random().next_float() < actual {
        next(state)
    } else {
        None
    }
}

/// 曼哈顿距离 4 以内（不算自己）的会氧化的铜：比自己老的和一样老的有几个。有更新的铜时是 `None`。
fn count_neighbours(own: usize, world: &World, pos: BlockPos) -> Option<(i32, i32)> {
    let (mut older, mut same) = (0, 0);
    for dx in -SCAN_DISTANCE..=SCAN_DISTANCE {
        for dy in -SCAN_DISTANCE..=SCAN_DISTANCE {
            for dz in -SCAN_DISTANCE..=SCAN_DISTANCE {
                let near = dx.abs() + dy.abs() + dz.abs() <= SCAN_DISTANCE;
                if !near || (dx, dy, dz) == (0, 0, 0) {
                    continue;
                }
                let neighbour = world.block_state(pos.offset(dx, dy, dz));
                if !neighbour.is_a(BlockAncestor::WeatheringCopper) {
                    continue;
                }
                match age(neighbour).cmp(&own) {
                    std::cmp::Ordering::Less => return None,
                    std::cmp::Ordering::Greater => older += 1,
                    std::cmp::Ordering::Equal => same += 1,
                }
            }
        }
    }
    Some((older, same))
}
