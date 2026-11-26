//! `CarvedPumpkinBlock`：雕刻南瓜和南瓜灯，还有放下时拼成傀儡的 `BlockPattern`。
//!
//! 图案里的方块会被拿走，铜傀儡在铜块的位置留下一个箱子。`WitherSkullBlock.checkSpawn`
//! 复用 `CarvedPumpkinBlock.clearPatternBlocks` / `updatePatternBlocks`，所以凋灵的
//! 图案检测和清方块也放在这里：它额外生成一只带无敌期的凋灵。
//!
//! The wither-summon pattern check also lives here because vanilla `checkSpawn`
//! reuses `CarvedPumpkinBlock.clearPatternBlocks` / `updatePatternBlocks`; it
//! additionally spawns a wither in its invulnerability phase.

use mc_bamboo_core::{Axis, BlockPos, Direction, Vec3};
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
    /// 凋灵图案里的凋灵骷髅头（地面或墙上）。
    ///
    /// A wither skeleton skull (floor or wall) in the wither pattern.
    WitherSkull,
    /// `#wither_summon_base_blocks`（灵魂沙 / 灵魂土）。
    ///
    /// `#wither_summon_base_blocks` (soul sand / soul soil).
    WitherBase,
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
            Self::WitherSkull => {
                state.is(blocks::WITHER_SKELETON_SKULL)
                    || state.is(blocks::WITHER_SKELETON_WALL_SKULL)
            }
            Self::WitherBase => state.is_tagged(block_tags::WITHER_SUMMON_BASE_BLOCKS),
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
/// `getOrCreateWitherFull`：`"^^^", "###", "~#~"`。第一行在最上面。
///
/// `getOrCreateWitherFull`: `"^^^", "###", "~#~"`. The first row is the top.
const WITHER_FULL: &[&[Cell]] = &[
    &[Cell::WitherSkull, Cell::WitherSkull, Cell::WitherSkull],
    &[Cell::WitherBase, Cell::WitherBase, Cell::WitherBase],
    &[Cell::Air, Cell::WitherBase, Cell::Air],
];

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

/// `getOrCreateWitherFull().find`，同时记下匹配时的 `forwards`：生成位置和身体朝向
/// 都取自它（`forwards` 在 X 轴上是 0 度，否则 90 度）。
///
/// `getOrCreateWitherFull().find`, also returning the matched `forwards`: the spawn
/// position and body rotation come from it (0 degrees on the X axis, else 90).
fn find_wither(world: &World, origin: BlockPos) -> Option<(PatternMatch, Direction)> {
    let reach = WITHER_FULL.len().max(WITHER_FULL[0].len()) as i32;
    for z in 0..reach {
        for y in 0..reach {
            for x in 0..reach {
                if let Some(found) = find_wither_at(world, origin.offset(x, y, z)) {
                    return Some(found);
                }
            }
        }
    }
    None
}

fn find_wither_at(world: &World, origin: BlockPos) -> Option<(PatternMatch, Direction)> {
    for forwards in Direction::ALL {
        for up in Direction::ALL {
            if up.axis() == forwards.axis() {
                continue;
            }
            if let Some(cells) = match_at(world, WITHER_FULL, origin, forwards, up) {
                return Some((cells, forwards));
            }
        }
    }
    None
}

/// `EntityType.create` + `snapTo` + `addFreshEntity`：官方 `spawnGolemInWorld` 的中间段。
/// 南瓜图案里 `getBlock(...)` 指定的格子给出脚底位置，朝向固定为 0。
///
/// The middle of vanilla `spawnGolemInWorld`: create the mob, snap it to the
/// pattern cell with zero rotation, then add it to the world.
fn spawn_golem(
    world: &mut World,
    kind: crate::entity::mob::MobType,
    foot: BlockPos,
    player_created: bool,
    weather: Option<crate::entity::mob::copper_golem::WeatherState>,
) {
    use crate::entity::{Entity, mob::iron_golem};
    let position = Vec3::new(
        f64::from(foot.x) + 0.5,
        f64::from(foot.y) + 0.05,
        f64::from(foot.z) + 0.5,
    );
    let mut entity = world.prepare_entity(Entity::by_mob_type(kind, position));
    entity.with_mob(|me, mob| {
        me.y_rot = 0.0;
        me.old_position = position;
        mob.x_rot = 0.0;
        mob.y_rot_o = 0.0;
        mob.x_rot_o = 0.0;
        mob.y_head_rot = 0.0;
        mob.y_body_rot = 0.0;
        mob.y_head_rot_o = 0.0;
        mob.y_body_rot_o = 0.0;
        if player_created {
            iron_golem::mark_player_created(mob);
        }
        if let Some(weather) = weather {
            // `CopperGolem.spawn(WeatherState)`：按图案里铜块的氧化等级设初始状态。
            crate::entity::mob::copper_golem::spawn(mob, weather);
        }
    });
    world.add_prepared_entity(entity);
    // `CarvedPumpkinBlock.spawnGolemInWorld` 在实体落地后给包围盒膨胀 5 格内的
    // 所有 ServerPlayer 触发 SUMMONED_ENTITY。
    world.push_effect(Effect::SummonedEntity {
        entity: kind.name(),
        position,
        radius: 5.0,
    });
}

/// `CarvedPumpkinBlock.clearPatternBlocks`：每格清成空气，并按顺序发 2001 破坏事件。
fn clear_pattern_blocks(world: &mut World, cells: &PatternMatch) {
    for &(pos, state) in cells {
        world.set_block(pos, BlockState::AIR, UPDATE_CLIENTS);
        world.push_effect(Effect::Destroyed {
            pos,
            state,
            except_initiator: false,
        });
    }
}

/// `CarvedPumpkinBlock.updatePatternBlocks`：清完之后每格更新邻居。
fn update_pattern_blocks(world: &mut World, cells: &PatternMatch) {
    for &(pos, _) in cells {
        world.update_neighbors_at(pos, BlockState::AIR.block());
    }
}

/// `getWeatherStateFromPattern`：铜块的氧化等级（打蜡的按去蜡对应块取），
/// 认不出的铜块回退到 `UNAFFECTED`。
///
/// `getWeatherStateFromPattern`: the copper block's oxidation stage (waxed blocks
/// use their unwaxed counterpart); unknown copper falls back to `UNAFFECTED`.
fn copper_weather(copper: BlockState) -> crate::entity::mob::copper_golem::WeatherState {
    use crate::entity::mob::copper_golem::WeatherState as W;
    use mc_bamboo_data::blocks as b;
    if copper.is(b::EXPOSED_COPPER) || copper.is(b::WAXED_EXPOSED_COPPER) {
        W::Exposed
    } else if copper.is(b::WEATHERED_COPPER) || copper.is(b::WAXED_WEATHERED_COPPER) {
        W::Weathered
    } else if copper.is(b::OXIDIZED_COPPER) || copper.is(b::WAXED_OXIDIZED_COPPER) {
        W::Oxidized
    } else {
        W::Unaffected
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

/// 对拍夹具用：直接跑一次官方 `CarvedPumpkinBlock.onPlace` 的搭建判定。
/// 官方夹具调的是 protected `onPlace`，这里走同一条 `trySpawnGolem`。
///
/// Oracle hook: run the vanilla `CarvedPumpkinBlock.onPlace` spawn check once.
pub fn place_for_oracle(world: &mut World, top: BlockPos) {
    try_spawn_golem(world, top);
}

/// `WitherSkullBlock.checkSpawn(level, pos, skull)`：图案成立时清方块、生成带无敌期的
/// 凋灵、给半径 50 格内的玩家触发 `SUMMONED_ENTITY`。和平时不召唤。
///
/// `WitherSkullBlock.checkSpawn(level, pos, skull)`: clear the pattern, spawn a
/// wither in its invulnerability phase and credit `SUMMONED_ENTITY` to players
/// within 50 blocks. Nothing happens on peaceful difficulty.
pub(crate) fn check_spawn_wither(world: &mut World, pos: BlockPos) {
    let state = world.block_state(pos);
    if !state.is(blocks::WITHER_SKELETON_SKULL) && !state.is(blocks::WITHER_SKELETON_WALL_SKULL) {
        return;
    }
    // `pos.getY() >= level.getMinY()` 且难度不是和平。
    // `pos.getY() >= level.getMinY()` and a non-peaceful difficulty.
    if !wither_spawn_gate(world.difficulty_id(), pos.y, world.min_y()) {
        return;
    }
    let Some((cells, forwards)) = find_wither(world, pos) else {
        return;
    };
    clear_pattern_blocks(world, &cells);
    // `BlockPatternMatch.getBlock(1, 2, 0)`：图案底行中间那块灵魂沙。
    // `BlockPatternMatch.getBlock(1, 2, 0)`: the middle soul block of the bottom row.
    let foot = cells[5].0;
    let angle = if forwards.axis() == Axis::X {
        0.0
    } else {
        90.0
    };
    let position = Vec3::new(
        f64::from(foot.x) + 0.5,
        f64::from(foot.y) + 0.55,
        f64::from(foot.z) + 0.5,
    );
    spawn_wither(world, position, angle);
    world.push_effect(Effect::SummonedEntity {
        entity: "minecraft:wither",
        position,
        radius: 50.0,
    });
    update_pattern_blocks(world, &cells);
}

/// 难度不是和平、且放置高度不低于世界底部时才能召唤。
///
/// Summoning is allowed only on a non-peaceful difficulty and at or above the
/// world's minimum Y.
fn wither_spawn_gate(difficulty_id: i32, y: i32, min_y: i32) -> bool {
    y >= min_y && difficulty_id != 0
}

/// `EntityType.create` + `snapTo` + `makeInvulnerable` + `addFreshEntity`。
fn spawn_wither(world: &mut World, position: Vec3, angle: f32) {
    use crate::entity::{Entity, mob::MobType};
    let mut entity = world.prepare_entity(Entity::by_mob_type(MobType::Wither, position));
    entity.with_mob(|me, mob| {
        me.y_rot = angle;
        me.old_position = position;
        mob.x_rot = 0.0;
        mob.y_rot_o = angle;
        mob.x_rot_o = 0.0;
        mob.y_head_rot = angle;
        mob.y_body_rot = angle;
        mob.y_head_rot_o = angle;
        mob.y_body_rot_o = angle;
        // `makeInvulnerable()`：血量设成最大血量的三分之一（300 / 3）。
        // `makeInvulnerable()`: health becomes a third of the maximum (300 / 3).
        mob.health = mob
            .attributes
            .value(mc_bamboo_data::registries::attribute::MAX_HEALTH) as f32
            / 3.0;
        mob.wither.make_invulnerable();
    });
    world.add_prepared_entity(entity);
}

/// `trySpawnGolem`。
fn try_spawn_golem(world: &mut World, top: BlockPos) {
    use crate::entity::mob::MobType;
    // 官方依次试雪、铁、铜三种完整图案。`getBlock(x, y, 0)` 的 y 是图案里从上往下数。
    // Vanilla tries the snow, iron and copper full patterns in order; `y` in
    // `getBlock(x, y, 0)` counts down from the top of the pattern.
    if let Some(cells) = find(world, SNOW_GOLEM, top) {
        let foot = cells[2].0;
        clear_pattern_blocks(world, &cells);
        spawn_golem(world, MobType::SnowGolem, foot, false, None);
        update_pattern_blocks(world, &cells);
        return;
    }
    if let Some(cells) = find(world, IRON_GOLEM, top) {
        let foot = cells[5].0;
        clear_pattern_blocks(world, &cells);
        spawn_golem(world, MobType::IronGolem, foot, true, None);
        update_pattern_blocks(world, &cells);
        return;
    }
    if let Some(cells) = find(world, COPPER_GOLEM, top) {
        let weather = copper_weather(cells[1].1);
        let foot = cells[0].0;
        clear_pattern_blocks(world, &cells);
        // 官方 `spawnGolemInWorld` 在南瓜格生成实体，随后铜块换成箱子、再 `spawn`。
        spawn_golem(world, MobType::CopperGolem, foot, false, Some(weather));
        replace_copper_block_with_chest(world, &cells);
        update_pattern_blocks(world, &cells);
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
            .filter(|effect| {
                !matches!(
                    effect,
                    Effect::Sound { .. }
                        | Effect::SummonedEntity { .. }
                        | Effect::BredAnimals { .. }
                        | Effect::CrafterCrafted { .. }
                        | Effect::KilledByArrow { .. }
                )
            })
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

    /// 图案真的生成一只铜傀儡，初始氧化等级取铜块（打蜡的也认）。
    #[test]
    fn a_copper_golem_entity_takes_the_copper_oxidation() {
        use crate::entity::mob::{MobType, copper_golem};
        for (block, weather) in [
            (blocks::COPPER_BLOCK, copper_golem::WeatherState::Unaffected),
            (blocks::EXPOSED_COPPER, copper_golem::WeatherState::Exposed),
            (
                blocks::WEATHERED_COPPER,
                copper_golem::WeatherState::Weathered,
            ),
            (
                blocks::OXIDIZED_COPPER,
                copper_golem::WeatherState::Oxidized,
            ),
            (
                blocks::WAXED_WEATHERED_COPPER,
                copper_golem::WeatherState::Weathered,
            ),
        ] {
            let mut level = TestLevel::new();
            level.set(TOP.below(), BlockState(block));
            place_pumpkin(&mut level);
            let golems: Vec<_> = level
                .entities()
                .iter()
                .filter(|entity| {
                    entity
                        .kind
                        .mob()
                        .is_some_and(|mob| mob.mob_type == MobType::CopperGolem)
                })
                .collect();
            assert_eq!(golems.len(), 1, "block {block}");
            let mob = golems[0].kind.mob().unwrap();
            assert_eq!(mob.copper_golem.weather_state, weather, "block {block}");
        }
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

    const WITHER_TOP: BlockPos = BlockPos::new(0, 68, 0);

    /// 摆好凋灵图案，只留下 `WITHER_TOP` 那一格由放置动作补上。
    fn wither_scaffold(level: &mut TestLevel) {
        let skull = BlockState(blocks::WITHER_SKELETON_SKULL);
        let soul = BlockState(blocks::SOUL_SAND);
        let body = WITHER_TOP.below();
        level.set(WITHER_TOP.west(), skull);
        level.set(WITHER_TOP.east(), skull);
        level.set(body.west(), soul);
        level.set(body, soul);
        level.set(body.east(), soul);
        level.set(body.below(), soul);
    }

    fn place_wither_skull(level: &mut TestLevel) -> Vec<Effect> {
        let actor = actor_holding(items::WITHER_SKELETON_SKULL);
        use_item(level, &actor, top_hit(WITHER_TOP.below())).effects
    }

    fn count_withers(level: &TestLevel) -> usize {
        use crate::entity::mob::MobType;
        level
            .entities()
            .iter()
            .filter(|entity| {
                entity
                    .kind
                    .mob()
                    .is_some_and(|mob| mob.mob_type == MobType::Wither)
            })
            .count()
    }

    /// 玩家放下最后一个凋灵头：清 9 格、2001 事件、生成带无敌期的凋灵、进度触发。
    #[test]
    fn placing_the_last_wither_skull_spawns_an_invulnerable_wither() {
        use crate::entity::mob::MobType;
        let mut level = TestLevel::new();
        wither_scaffold(&mut level);
        let effects = place_wither_skull(&mut level);
        assert_eq!(count_withers(&level), 1);
        let withers: Vec<_> = level
            .entities()
            .iter()
            .filter(|entity| {
                entity
                    .kind
                    .mob()
                    .is_some_and(|mob| mob.mob_type == MobType::Wither)
            })
            .collect();
        let mob = withers[0].kind.mob().unwrap();
        assert_eq!(mob.wither.invulnerable_ticks, 220);
        assert_eq!(mob.health, 100.0);
        assert_eq!(mob.wither.boss_progress, 0.0);
        let body = WITHER_TOP.below();
        for pos in [
            WITHER_TOP.west(),
            WITHER_TOP,
            WITHER_TOP.east(),
            body.west(),
            body,
            body.east(),
            body.below().west(),
            body.below(),
            body.below().east(),
        ] {
            assert!(level.get(pos).is_air(), "{pos:?}");
        }
        assert!(effects.iter().any(|effect| matches!(
            effect,
            Effect::SummonedEntity { radius, .. } if *radius == 50.0
        )));
        assert_eq!(
            effects
                .iter()
                .filter(|effect| matches!(effect, Effect::Destroyed { .. }))
                .count(),
            9
        );
    }

    /// 图案缺一块灵魂沙时不召唤，方块也不清。
    #[test]
    fn an_incomplete_wither_pattern_does_not_spawn() {
        let mut level = TestLevel::new();
        wither_scaffold(&mut level);
        level.set(WITHER_TOP.below().east(), BlockState::AIR);
        place_wither_skull(&mut level);
        assert_eq!(count_withers(&level), 0);
        assert!(level.get(WITHER_TOP.below()).is(blocks::SOUL_SAND));
    }

    /// 和平难度或低于世界底部时不允许召唤。
    #[test]
    fn peaceful_difficulty_and_low_placement_block_the_wither() {
        assert!(!wither_spawn_gate(0, 66, -64));
        assert!(wither_spawn_gate(2, 66, -64));
        assert!(!wither_spawn_gate(2, -65, -64));
    }
}
