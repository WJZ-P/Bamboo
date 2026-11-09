//! `CarvedPumpkinBlock`：雕刻南瓜和南瓜灯，还有放下时拼成傀儡的 `BlockPattern`。
//!
//! 这个世界看不到实体，傀儡不会出现，但是方块的变化和官方一样：
//! 图案里的方块被拿走，铜傀儡在铜块的位置留下一个箱子。

use mc_bamboo_core::{BlockPos, Direction, Vec3};
use mc_bamboo_data::{Block, blocks, sounds, sounds::sound_event, tags::block_tags};

use super::chest::chest_from_copper_block;
use crate::{
    behaviour::{Behaviour, common::facing_the_player},
    context::PlaceContext,
    level::Effect,
    state::{BlockState, HORIZONTAL_FACING},
    world::{UPDATE_CLIENTS, World},
};

/// `CarvedPumpkinBlock`。
pub(super) struct CarvedPumpkin;

/// 图案里的一格。
#[derive(Clone, Copy)]
enum Cell {
    /// 图案里的空格，`BlockPatternBuilder` 里 `' '` 匹配任何方块。
    ///
    /// A space in the pattern. `' '` in `BlockPatternBuilder` matches any block.
    Any,
    Pumpkin,
    Snow,
    Iron,
    Copper,
    Air,
}

impl Cell {
    fn matches(self, state: BlockState) -> bool {
        match self {
            Self::Any => true,
            Self::Pumpkin => state.is(blocks::CARVED_PUMPKIN) || state.is(blocks::JACK_O_LANTERN),
            Self::Snow => state.is(blocks::SNOW_BLOCK),
            Self::Iron => state.is(blocks::IRON_BLOCK),
            Self::Copper => state.is_tagged(block_tags::COPPER),
            Self::Air => state.is_air(),
        }
    }
}

/// `getOrCreateSnowGolemFull`，一行是图案的一层，第一行在最上面。
const SNOW_GOLEM: &[&[Cell]] = &[&[Cell::Pumpkin], &[Cell::Snow], &[Cell::Snow]];
/// `getOrCreateIronGolemFull`。
const IRON_GOLEM: &[&[Cell]] = &[
    &[Cell::Air, Cell::Pumpkin, Cell::Air],
    &[Cell::Iron, Cell::Iron, Cell::Iron],
    &[Cell::Air, Cell::Iron, Cell::Air],
];
/// `getOrCreateCopperGolemFull`。
const COPPER_GOLEM: &[&[Cell]] = &[&[Cell::Pumpkin], &[Cell::Copper]];
/// `getOrCreateSnowGolemBase`。最上面一格是空格，空气也算。
///
/// `getOrCreateSnowGolemBase`. The top cell is a space, so air matches too.
const SNOW_BASE: &[&[Cell]] = &[&[Cell::Any], &[Cell::Snow], &[Cell::Snow]];
/// `getOrCreateIronGolemBase`。
const IRON_BASE: &[&[Cell]] = &[
    &[Cell::Air, Cell::Any, Cell::Air],
    &[Cell::Iron, Cell::Iron, Cell::Iron],
    &[Cell::Air, Cell::Iron, Cell::Air],
];
/// `getOrCreateCopperGolemBase`。
const COPPER_BASE: &[&[Cell]] = &[&[Cell::Any], &[Cell::Copper]];

/// `CarvedPumpkinBlock.canSpawnGolem`。
///
/// `CarvedPumpkinBlock.canSpawnGolem`.
pub(crate) fn can_spawn_golem(world: &World, pos: BlockPos) -> bool {
    find(world, SNOW_BASE, pos).is_some()
        || find(world, IRON_BASE, pos).is_some()
        || find(world, COPPER_BASE, pos).is_some()
}

/// `BlockPattern.translateAndRotate`，图案只有一层厚，`forwards` 方向不用走。
fn translate(
    origin: BlockPos,
    forwards: Direction,
    up: Direction,
    right: i32,
    down: i32,
) -> BlockPos {
    let (fx, fy, fz) = forwards.normal();
    let (ux, uy, uz) = up.normal();
    let (rx, ry, rz) = (fy * uz - fz * uy, fz * ux - fx * uz, fx * uy - fy * ux);
    origin.offset(
        ux * -down + rx * right,
        uy * -down + ry * right,
        uz * -down + rz * right,
    )
}

/// `BlockPatternMatch`：这个图案每一格的位置和匹配时的状态，先列后行，和官方遍历的顺序相同。
type PatternMatch = Vec<(BlockPos, BlockState)>;

/// `BlockPattern.matches`。
fn match_at(
    world: &World,
    pattern: &[&[Cell]],
    origin: BlockPos,
    forwards: Direction,
    up: Direction,
) -> Option<PatternMatch> {
    let mut cells = Vec::new();
    for right in 0..pattern[0].len() {
        for (down, row) in pattern.iter().enumerate() {
            let pos = translate(origin, forwards, up, right as i32, down as i32);
            let state = world.block_state(pos);
            if !row[right].matches(state) {
                return None;
            }
            cells.push((pos, state));
        }
    }
    Some(cells)
}

/// `BlockPattern.find`。
fn find(world: &World, pattern: &[&[Cell]], origin: BlockPos) -> Option<PatternMatch> {
    let reach = pattern.len().max(pattern[0].len()) as i32;
    for z in 0..reach {
        for y in 0..reach {
            for x in 0..reach {
                let found = find_at(world, pattern, origin.offset(x, y, z));
                if found.is_some() {
                    return found;
                }
            }
        }
    }
    None
}

/// 这个原点上每一种朝向都试一遍。
fn find_at(world: &World, pattern: &[&[Cell]], origin: BlockPos) -> Option<PatternMatch> {
    for forwards in Direction::ALL {
        for up in Direction::ALL {
            if up.axis() == forwards.axis() {
                continue;
            }
            let found = match_at(world, pattern, origin, forwards, up);
            if found.is_some() {
                return found;
            }
        }
    }
    None
}

/// `spawnGolemInWorld`：`clearPatternBlocks` 和 `updatePatternBlocks`，中间官方在生成实体。
fn take_pattern_blocks(world: &mut World, cells: &PatternMatch) {
    for &(pos, state) in cells {
        world.set_block(pos, BlockState::AIR, UPDATE_CLIENTS);
        world.push_effect(Effect::Destroyed {
            pos,
            state,
            except_initiator: false,
        });
    }
    for &(pos, _) in cells {
        world.update_neighbors_at(pos, BlockState::AIR.block());
    }
}

/// `replaceCopperBlockWithChest`：图案从上往下，第一格是南瓜，第二格是铜块。
fn replace_copper_block_with_chest(world: &mut World, cells: &PatternMatch) {
    let (_, pumpkin) = cells[0];
    let (copper_pos, copper) = cells[1];
    let facing = pumpkin.get(HORIZONTAL_FACING);
    let chest = chest_from_copper_block(copper, facing, world, copper_pos);
    world.set_block(copper_pos, chest, UPDATE_CLIENTS);
}

/// `trySpawnGolem`。
fn try_spawn_golem(world: &mut World, top: BlockPos) {
    for pattern in [SNOW_GOLEM, IRON_GOLEM] {
        if let Some(cells) = find(world, pattern, top) {
            take_pattern_blocks(world, &cells);
            return;
        }
    }
    if let Some(cells) = find(world, COPPER_GOLEM, top) {
        take_pattern_blocks(world, &cells);
        replace_copper_block_with_chest(world, &cells);
        // 官方生成铜傀儡后 `CopperGolem.spawn` 会报一声，位置是傀儡脚底往上 0.05 格。
        let (pumpkin_pos, _) = cells[0];
        world.play_sound(
            Vec3::new(
                f64::from(pumpkin_pos.x) + 0.5,
                f64::from(pumpkin_pos.y) + 0.05,
                f64::from(pumpkin_pos.z) + 0.5,
            ),
            sound_event("minecraft:entity.copper_golem.spawn"),
            sounds::NEUTRAL,
            1.0,
            1.0,
            false,
        );
    }
}

impl Behaviour for CarvedPumpkin {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        _world: &World,
    ) -> Option<BlockState> {
        Some(facing_the_player(block, ctx))
    }

    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, old: BlockState) {
        if !old.is_same_block(state) {
            try_spawn_golem(world, pos);
        }
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::items;

    use super::*;
    use crate::{
        state::{CHEST_TYPE, ChestType},
        testing::{TestLevel, actor_holding, top_hit, use_item},
    };

    const TOP: BlockPos = BlockPos::new(0, 66, 0);

    /// 在 `TOP` 下面一格的顶面放南瓜：`TOP` 本身是图案里南瓜的位置。
    fn place_pumpkin(level: &mut TestLevel) -> Vec<Effect> {
        let actor = actor_holding(items::CARVED_PUMPKIN);
        use_item(level, &actor, top_hit(TOP.below())).effects
    }

    /// 傀儡取走方块的数量，不算南瓜自己的放置音效。
    fn destroyed_count(effects: Vec<Effect>) -> usize {
        effects
            .iter()
            .filter(|effect| !matches!(effect, Effect::Sound { .. }))
            .count()
    }

    #[test]
    fn a_snow_golem_takes_its_blocks() {
        let mut level = TestLevel::new();
        let snow = BlockState(blocks::SNOW_BLOCK);
        level.set(TOP.below(), snow);
        level.set(TOP.below().below(), snow);
        let effects = place_pumpkin(&mut level);
        for pos in [TOP, TOP.below(), TOP.below().below()] {
            assert!(level.get(pos).is_air(), "{pos:?}");
        }
        // 另有南瓜自己的放置音效，和傀儡取方块无关。
        assert_eq!(destroyed_count(effects), 3);
    }

    /// 图案里的空气也算一格，官方照样发破坏事件。
    #[test]
    fn an_iron_golem_needs_air_around_its_legs() {
        let mut level = TestLevel::new();
        let iron = BlockState(blocks::IRON_BLOCK);
        let body = TOP.below();
        for pos in [body, body.west(), body.east(), body.below()] {
            level.set(pos, iron);
        }
        let effects = place_pumpkin(&mut level);
        for pos in [TOP, body, body.west(), body.east(), body.below()] {
            assert!(level.get(pos).is_air(), "{pos:?}");
        }
        assert_eq!(destroyed_count(effects), 9);
    }

    #[test]
    fn a_blocked_leg_keeps_the_iron_golem_from_forming() {
        let mut level = TestLevel::new();
        let iron = BlockState(blocks::IRON_BLOCK);
        let body = TOP.below();
        for pos in [body, body.west(), body.east(), body.below()] {
            level.set(pos, iron);
        }
        level.set(body.below().west(), BlockState(blocks::STONE));
        level.set(body.below().east(), BlockState(blocks::STONE));
        let effects = place_pumpkin(&mut level);
        assert!(level.get(TOP).is(blocks::CARVED_PUMPKIN));
        assert!(level.get(body).is(blocks::IRON_BLOCK));
        assert_eq!(destroyed_count(effects), 0);
    }

    #[test]
    fn a_copper_golem_leaves_a_chest_facing_like_the_pumpkin() {
        let mut level = TestLevel::new();
        level.set(TOP.below(), BlockState(blocks::EXPOSED_COPPER));
        place_pumpkin(&mut level);
        let chest = level.get(TOP.below());
        assert!(level.get(TOP).is_air());
        assert!(chest.is(blocks::EXPOSED_COPPER_CHEST));
        // 玩家朝南，南瓜朝北对着他。
        assert_eq!(chest.get(HORIZONTAL_FACING), Direction::North);
        assert_eq!(chest.get(CHEST_TYPE), ChestType::Single);
    }

    /// 铜傀儡实体由官方在方块动作里生成，顺手报一声。
    #[test]
    fn a_copper_golem_announces_itself() {
        let mut level = TestLevel::new();
        level.set(TOP.below(), BlockState(blocks::COPPER_BLOCK));
        let effects = place_pumpkin(&mut level);
        let spawn = effects.iter().find(|effect| {
            matches!(
                effect,
                Effect::Sound { sound, .. }
                    if *sound == i32::from(sound_event("minecraft:entity.copper_golem.spawn"))
            )
        });
        assert_eq!(
            spawn,
            Some(&Effect::Sound {
                sound: i32::from(sound_event("minecraft:entity.copper_golem.spawn")),
                source: sounds::NEUTRAL,
                x: 0.5,
                y: 66.05,
                z: 0.5,
                volume: 1.0,
                pitch: 1.0,
                except_initiator: false,
                seed: None,
            })
        );
    }

    /// 新箱子旁边有朝向相同的单个铜箱子时要合成大箱子，取两半里氧化最少的一种。
    #[test]
    fn a_copper_golem_chest_joins_the_chest_beside_it() {
        let mut level = TestLevel::new();
        level.set(TOP.below(), BlockState(blocks::OXIDIZED_COPPER));
        // 朝北的箱子，右手边在东，所以西边那一格的箱子把这边当成 `Left`。
        level.set(TOP.below().east(), BlockState(blocks::COPPER_CHEST));
        place_pumpkin(&mut level);
        let chest = level.get(TOP.below());
        assert!(chest.is(blocks::COPPER_CHEST));
        assert_eq!(chest.get(CHEST_TYPE), ChestType::Left);
        assert_eq!(
            level.get(TOP.below().east()).get(CHEST_TYPE),
            ChestType::Right
        );
    }
}
