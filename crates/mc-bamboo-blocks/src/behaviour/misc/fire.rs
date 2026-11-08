//! `FireBlock`、`SoulFireBlock` 和父类 `BaseFireBlock`。
//!
//! 火的蔓延和熄灭都在 `FireBlock.tick` 里：火放下时排一个 30 到 39 刻的计划刻，每次到期再排下一个。
//! 火没有方块物品，打火石和火焰弹用 [`can_be_placed_at`] 和 [`state_at`] 生火，见 `crate::item_use`。

use mc_bamboo_core::{Axis, BlockPos, Direction};
use mc_bamboo_data::{BlockAncestor, block_maps::FIRE_ODDS, blocks, tags::block_tags};

use super::portal::{PortalShape, is_frame};
use crate::{
    behaviour::Behaviour,
    state::{AGE_15, BlockState, EAST, NORTH, Property, SOUTH, SupportType, UP, WATERLOGGED, WEST},
    world::{UPDATE_NONE, World},
};

/// `FireBlock.MAX_AGE`。
const MAX_AGE: i32 = 15;
/// 火的计划刻最短的延迟，再随机加 0 到 9。
const FIRE_TICK_DELAY: i32 = 30;
const FIRE_TICK_DELAY_SPREAD: i32 = 10;
/// `checkBurnOut` 的六个方向和机会的分母，按官方的顺序：东、西、下、上、北、南。
const BURN_OUT_CHANCES: [(Direction, i32); 6] = [
    (Direction::East, 300),
    (Direction::West, 300),
    (Direction::Down, 250),
    (Direction::Up, 250),
    (Direction::North, 300),
    (Direction::South, 300),
];

/// `FireBlock`。
pub(super) struct Fire;

/// `SoulFireBlock`。
pub(super) struct SoulFire;

/// `PROPERTY_BY_DIRECTION`：火没有朝下的面。
const fn side_property(side: Direction) -> Option<Property<bool>> {
    match side {
        Direction::North => Some(NORTH),
        Direction::East => Some(EAST),
        Direction::South => Some(SOUTH),
        Direction::West => Some(WEST),
        Direction::Up => Some(UP),
        Direction::Down => None,
    }
}

/// `getIgniteOdds(state)`。
fn ignite_odds(state: BlockState) -> i32 {
    if state.try_get(WATERLOGGED) == Some(true) {
        return 0;
    }
    let block = state.block_id() as i32;
    FIRE_ODDS
        .iter()
        .find(|&&(id, _, _)| id == block)
        .map_or(0, |&(_, ignite, _)| ignite)
}

/// `FireBlock.canBurn`。
fn can_burn(state: BlockState) -> bool {
    ignite_odds(state) > 0
}

/// `isValidFireLocation`：旁边有能烧的方块。
fn is_valid_location(world: &World, pos: BlockPos) -> bool {
    Direction::ALL
        .into_iter()
        .any(|side| can_burn(world.block_state(pos.relative(side))))
}

/// `FireBlock.getStateForPlacement(level, pos)`：悬空的火贴在能烧的方块上。
fn fire_state(world: &World, pos: BlockPos) -> BlockState {
    let below_pos = pos.below();
    let default = BlockState(blocks::FIRE);
    if can_burn(world.block_state(below_pos))
        || world.is_face_sturdy(below_pos, Direction::Up, SupportType::Full)
    {
        return default;
    }
    Direction::ALL.into_iter().fold(default, |state, side| {
        side_property(side).map_or(state, |property| {
            state.with(property, can_burn(world.block_state(pos.relative(side))))
        })
    })
}

/// `SoulFireBlock.canSurviveOnBlock`。
fn is_soul_fire_base(state: BlockState) -> bool {
    state.is_tagged(block_tags::SOUL_FIRE_BASE_BLOCKS)
}

/// `BaseFireBlock.getState`。
pub(crate) fn state_at(world: &World, pos: BlockPos) -> BlockState {
    if is_soul_fire_base(world.block_state(pos.below())) {
        BlockState(blocks::SOUL_FIRE)
    } else {
        fire_state(world, pos)
    }
}

/// `BaseFireBlock.canBePlacedAt`：空气里放得住火，或者这里能点亮一个空的传送门框架。
pub(crate) fn can_be_placed_at(world: &World, pos: BlockPos, forward: Direction) -> bool {
    world.block_state(pos).is_air()
        && (state_at(world, pos).can_survive(world, pos) || is_portal(world, pos, forward))
}

/// `BaseFireBlock.isPortal`：旁边有框架方块时，按玩家朝向的左手轴优先找空框架。
/// 官方只在朝向是竖直的时候取随机的轴，玩家的水平朝向永远是水平的。
fn is_portal(world: &World, pos: BlockPos, forward: Direction) -> bool {
    let beside_frame = Direction::ALL
        .into_iter()
        .any(|side| is_frame(world.block_state(pos.relative(side))));
    beside_frame
        && PortalShape::find_empty(world, pos, forward.counter_clockwise().axis()).is_some()
}

/// `getStateWithAge`。
fn state_with_age(world: &World, pos: BlockPos, age: i32) -> BlockState {
    let state = state_at(world, pos);
    if state.is(blocks::FIRE) {
        state.with(AGE_15, age)
    } else {
        state
    }
}

/// `BaseFireBlock.onPlace`：点燃传送门框架，或者放不住时灭掉。
fn on_place(state: BlockState, world: &mut World, pos: BlockPos, old: BlockState) {
    if old.is_same_block(state) {
        return;
    }
    if let Some(shape) = PortalShape::find_empty(world, pos, Axis::X) {
        shape.create_portal_blocks(world);
    } else if !state.can_survive(world, pos) {
        world.remove_block(pos);
    }
}

/// `getFireTickDelay`：30 到 39 刻。
fn fire_tick_delay(world: &World) -> i32 {
    FIRE_TICK_DELAY + world.random().next_int(FIRE_TICK_DELAY_SPREAD)
}

/// `FireBlock.getBurnOdds`：含水的方块不会烧掉。
fn burn_odds(state: BlockState) -> i32 {
    if state.try_get(WATERLOGGED) == Some(true) {
        return 0;
    }
    let block = state.block_id() as i32;
    FIRE_ODDS
        .iter()
        .find(|&&(id, _, _)| id == block)
        .map_or(0, |&(_, _, burn)| burn)
}

/// `getIgniteOdds(level, pos)`：空气格旁边最容易点着的方块的点着概率。
fn ignite_odds_at(world: &World, pos: BlockPos) -> i32 {
    if !world.is_empty_block(pos) {
        return 0;
    }
    Direction::ALL
        .into_iter()
        .map(|side| ignite_odds(world.block_state(pos.relative(side))))
        .fold(0, i32::max)
}

/// `checkBurnOut`：旁边的方块按 `burnOdds` 烧掉，有时烧成火。
fn check_burn_out(world: &mut World, pos: BlockPos, chance: i32, age: i32) {
    let odds = burn_odds(world.block_state(pos));
    if world.random().next_int(chance) >= odds {
        return;
    }
    let old = world.block_state(pos);
    if world.random().next_int(age + 10) < 5 && !world.is_raining_at(pos) {
        let new_age = (age + world.random().next_int(5) / 4).min(MAX_AGE);
        let fire = state_with_age(world, pos, new_age);
        world.set_block_and_update(pos, fire);
    } else {
        world.remove_block(pos);
    }
    if old.is_a(BlockAncestor::TntBlock) {
        // `TntBlock.prime`：点燃的 TNT 实体在构造时从世界随机源取一个数定方向。这个世界不生成实体，
        // 数照取，后面的随机数才和官方对得上。
        world.random().next_double();
    }
}

/// 周围 3×6×3（往上多四层）的空气格按概率点着，越往上越难。
fn spread(world: &mut World, pos: BlockPos, age: i32) {
    for dx in -1..=1 {
        for dz in -1..=1 {
            for dy in -1..=4 {
                if (dx, dy, dz) == (0, 0, 0) {
                    continue;
                }
                let rate = if dy > 1 { 100 + (dy - 1) * 100 } else { 100 };
                spread_to(world, pos.offset(dx, dy, dz), rate, age);
            }
        }
    }
}

fn spread_to(world: &mut World, target: BlockPos, rate: i32, age: i32) {
    let ignite = ignite_odds_at(world, target);
    if ignite <= 0 {
        return;
    }
    let odds = (ignite + 40 + world.difficulty_id() * 7) / (age + 30);
    // 下雨时官方还要看那一格附近在不在下雨，这个世界不下雨，这一项总是真。
    if odds > 0 && world.random().next_int(rate) <= odds && !world.is_raining() {
        let spread_age = (age + world.random().next_int(5) / 4).min(MAX_AGE);
        let fire = state_with_age(world, target, spread_age);
        world.set_block_and_update(target, fire);
    }
}

impl Fire {
    /// `FireBlock.tick` 里火还在的部分：慢慢变老、烧掉旁边的方块、往外蔓延。
    /// 返回 `false` 表示火灭了，官方在这里返回。下雨灭火的分支走不到，这个世界不下雨。
    fn burn(state: BlockState, world: &mut World, pos: BlockPos, infiniburn: bool) -> bool {
        let age = state.get(AGE_15);
        let new_age = (age + world.random().next_int(3) / 2).min(MAX_AGE);
        if age != new_age {
            world.set_block(pos, state.with(AGE_15, new_age), UPDATE_NONE);
        }
        if infiniburn {
            return true;
        }
        if !is_valid_location(world, pos) {
            if !world.is_face_sturdy(pos.below(), Direction::Up, SupportType::Full) || age > 3 {
                world.remove_block(pos);
            }
            return false;
        }
        let burnt_out = age == MAX_AGE
            && world.random().next_int(4) == 0
            && !can_burn(world.block_state(pos.below()));
        if burnt_out {
            world.remove_block(pos);
        }
        !burnt_out
    }
}

impl Behaviour for Fire {
    fn can_survive(&self, _state: BlockState, world: &World, pos: BlockPos) -> bool {
        world.is_face_sturdy(pos.below(), Direction::Up, SupportType::Full)
            || is_valid_location(world, pos)
    }

    /// `FireBlock.tick`：先排下一次，再烧。平原没有 `increased_fire_burnout`。
    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        world.schedule_tick(pos, state.block(), fire_tick_delay(world));
        if !world.can_spread_fire_around(pos) {
            return;
        }
        if !state.can_survive(world, pos) {
            world.remove_block(pos);
        }
        let infiniburn = world
            .block_state(pos.below())
            .is_tagged(block_tags::INFINIBURN_OVERWORLD);
        if !Self::burn(state, world, pos, infiniburn) {
            return;
        }
        let age = state.get(AGE_15);
        for (side, chance) in BURN_OUT_CHANCES {
            check_burn_out(world, pos.relative(side), chance, age);
        }
        spread(world, pos, age);
    }

    fn update_shape(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor: BlockState,
    ) -> BlockState {
        if state.can_survive(world, pos) {
            state_with_age(world, pos, state.get(AGE_15))
        } else {
            BlockState::AIR
        }
    }

    /// `BaseFireBlock.onPlace` 之后不管火还在不在都排一个刻，换 `age` 时也一样。
    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, old: BlockState) {
        on_place(state, world, pos, old);
        world.schedule_tick(pos, state.block(), fire_tick_delay(world));
    }

    fn spawn_destroy_by_entity_particles(
        &self,
        _state: BlockState,
        _world: &mut World,
        _pos: BlockPos,
        _by_player: bool,
    ) {
    }
}

impl Behaviour for SoulFire {
    fn can_survive(&self, _state: BlockState, world: &World, pos: BlockPos) -> bool {
        is_soul_fire_base(world.block_state(pos.below()))
    }

    fn update_shape(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor: BlockState,
    ) -> BlockState {
        if state.can_survive(world, pos) {
            state.default_state()
        } else {
            BlockState::AIR
        }
    }

    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, old: BlockState) {
        on_place(state, world, pos, old);
    }

    fn spawn_destroy_by_entity_particles(
        &self,
        _state: BlockState,
        _world: &mut World,
        _pos: BlockPos,
        _by_player: bool,
    ) {
    }
}
