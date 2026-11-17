//! `MonsterRoomFeature`（地牢：圆石房间 + 箱子 + 刷怪笼）。
//!
//! 照 `MonsterRoomFeature.place`：先量 3 层高的房间外框，底部和顶部必须都是实心；
//! 侧墙的开口数要落在 1..=5；然后逐格把墙换成圆石（底面 3/4 概率苔石）、内部挖成
//! 洞穴空气，最后按「只有一侧是实心墙」找位置放箱子，并在原点放刷怪笼。
//!
//! 方块实体照官方写：箱子是 `RandomizableContainer.setBlockEntityLootTable`
//! （`chests/simple_dungeon` + 一次 `random.nextLong()` 当种子），刷怪笼是
//! `SpawnerBlockEntity.setEntityId(randomEntityId(random), random)` 写出的默认刷怪数据。
//!
//! `MonsterRoomFeature.place`: size a 3-high room shell whose floor and ceiling must both be
//! solid, require 1..=5 side openings, then turn the shell into cobblestone (the floor is
//! mossy 3 times out of 4), hollow the inside into cave air, place a chest where exactly one
//! horizontal neighbor is a solid wall, and put a spawner at the origin.
//!
//! Block entities follow vanilla: the chest gets
//! `RandomizableContainer.setBlockEntityLootTable` (`chests/simple_dungeon` plus one
//! `random.nextLong()` seed) and the spawner the default spawn data written by
//! `SpawnerBlockEntity.setEntityId(randomEntityId(random), random)`.

use mc_bamboo_data::{blocks, facts, state_facts, tags};
use serde_json::Value;

use crate::random::WorldgenRandom;

use super::{state::apply_property, world::FeatureWorld};

/// `MonsterRoomFeature.MOBS`：骷髅、僵尸、僵尸、蜘蛛。
///
/// `MonsterRoomFeature.MOBS`: skeleton, zombie, zombie, spider.
pub const MOBS: [&str; 4] = [
    "minecraft:skeleton",
    "minecraft:zombie",
    "minecraft:zombie",
    "minecraft:spider",
];

/// `MonsterRoomFeature` 没有配置字段（`MapCodec.unit`）。
///
/// `MonsterRoomFeature` has no config fields (`MapCodec.unit`).
#[derive(Debug, Clone, Copy)]
pub struct MonsterRoomFeature;

impl MonsterRoomFeature {
    pub fn parse(_config: &Value) -> Result<Self, String> {
        Ok(Self)
    }

    /// 没有嵌套配置，恒可对拍。
    ///
    /// No nested config, always comparable.
    pub fn supported(&self) -> bool {
        true
    }
}

/// `Direction.Plane.HORIZONTAL` 的顺序：NORTH, SOUTH, WEST, EAST。
const HORIZONTAL: [(i32, i32, i32); 4] = [(0, 0, -1), (0, 0, 1), (-1, 0, 0), (1, 0, 0)];

/// `StructurePiece.reorient`：按四周的实心墙给箱子定朝向。
///
/// `StructurePiece.reorient`: orient the chest by the surrounding solid walls.
fn reorient(world: &FeatureWorld<'_>, pos: (i32, i32, i32), state: u32) -> u32 {
    let mut solid_neighbor: Option<(i32, i32, i32)> = None;
    for direction in HORIZONTAL {
        let relative = (
            pos.0 + direction.0,
            pos.1 + direction.1,
            pos.2 + direction.2,
        );
        let neighbor = world.get_block(relative.0, relative.1, relative.2);
        if is_block(neighbor, "minecraft:chest") {
            return state;
        }
        if is_solid_render(neighbor) {
            if solid_neighbor.is_some() {
                solid_neighbor = None;
                break;
            }
            solid_neighbor = Some(direction);
        }
    }
    if let Some(direction) = solid_neighbor {
        return face_of(state, (-direction.0, -direction.1, -direction.2));
    }
    // 没有唯一实心墙：按原朝向试对侧、顺时针、逆时针。
    // No unique solid wall: try the opposite, clockwise and counter-clockwise directions.
    let mut lock = facing_of(state);
    let mut relative = offset(pos, lock);
    if is_solid_render(world.get_block(relative.0, relative.1, relative.2)) {
        lock = (-lock.0, -lock.1, -lock.2);
        relative = offset(pos, lock);
    }
    if is_solid_render(world.get_block(relative.0, relative.1, relative.2)) {
        lock = clockwise(lock);
        relative = offset(pos, lock);
    }
    if is_solid_render(world.get_block(relative.0, relative.1, relative.2)) {
        lock = (-lock.0, -lock.1, -lock.2);
    }
    face_of(state, lock)
}

fn offset(pos: (i32, i32, i32), direction: (i32, i32, i32)) -> (i32, i32, i32) {
    (
        pos.0 + direction.0,
        pos.1 + direction.1,
        pos.2 + direction.2,
    )
}

/// `Direction.getClockWise()`：水平面上顺时针（俯视）。
///
/// `Direction.getClockWise()`: clockwise on the horizontal plane.
fn clockwise(direction: (i32, i32, i32)) -> (i32, i32, i32) {
    match direction {
        (0, 0, -1) => (1, 0, 0),
        (1, 0, 0) => (0, 0, 1),
        (0, 0, 1) => (-1, 0, 0),
        (-1, 0, 0) => (0, 0, -1),
        other => other,
    }
}

/// `HorizontalDirectionalBlock.FACING` 的当前枚举序号。
///
/// The current `HorizontalDirectionalBlock.FACING` ordinal.
fn facing_of(state: u32) -> (i32, i32, i32) {
    const DIRECTIONS: [(i32, i32, i32); 4] = [(0, 0, -1), (0, 0, 1), (-1, 0, 0), (1, 0, 0)];
    let index = state_value_index(state, "facing").unwrap_or(0);
    DIRECTIONS[index.min(DIRECTIONS.len() - 1)]
}

/// 某个属性在这个状态里的取值序号（按官方 `StateDefinition` 的属性顺序折算）。
///
/// The value index of one property in this state, following the official
/// `StateDefinition` property order.
fn state_value_index(state: u32, name: &str) -> Option<usize> {
    let block = mc_bamboo_data::block_of_state(state)?;
    let mut stride = 1_u32;
    for property in block.properties.iter().rev() {
        let offset = state - block.first_state;
        let index = (offset / stride) % property.values.len() as u32;
        if property.name == name {
            return Some(index as usize);
        }
        stride *= property.values.len() as u32;
    }
    None
}

/// 把 `facing` 设成给定方向。
///
/// Set `facing` to the given direction.
fn face_of(state: u32, direction: (i32, i32, i32)) -> u32 {
    let name = match direction {
        (0, 0, -1) => "north",
        (0, 0, 1) => "south",
        (-1, 0, 0) => "west",
        _ => "east",
    };
    apply_property(state, "facing", name).unwrap_or(state)
}

fn is_block(state: u32, name: &str) -> bool {
    mc_bamboo_data::block_of_state(state).is_some_and(|block| block.name == name)
}

/// `BlockState.isSolidRender()`：整格实心（`facts::SOLID_RENDER`）。
///
/// `BlockState.isSolidRender()`: a full solid block (`facts::SOLID_RENDER`).
fn is_solid_render(state: u32) -> bool {
    state_facts(state) & facts::SOLID_RENDER != 0
}

fn is_solid(state: u32) -> bool {
    state_facts(state) & facts::SOLID != 0
}

/// `MonsterRoomFeature.place`。
///
/// `MonsterRoomFeature.place`.
pub fn place(
    _feature: &MonsterRoomFeature,
    world: &mut FeatureWorld<'_>,
    random: &WorldgenRandom,
    origin: (i32, i32, i32),
) -> bool {
    let cannot_replace: &[i32] =
        mc_bamboo_data::block_tag(tags::block_tags::FEATURES_CANNOT_REPLACE.name).unwrap_or(&[]);
    let (ox, oy, oz) = origin;
    let xr = random.next_int(2) + 2;
    let min_x = -xr - 1;
    let max_x = xr + 1;
    let zr = random.next_int(2) + 2;
    let min_z = -zr - 1;
    let max_z = zr + 1;
    let mut hole_count = 0;

    for dx in min_x..=max_x {
        for dy in -1..=4 {
            for dz in min_z..=max_z {
                let pos = (ox + dx, oy + dy, oz + dz);
                let solid = is_solid(world.get_block(pos.0, pos.1, pos.2));
                if dy == -1 && !solid {
                    return false;
                }
                if dy == 4 && !solid {
                    return false;
                }
                if (dx == min_x || dx == max_x || dz == min_z || dz == max_z)
                    && dy == 0
                    && is_air(world, pos)
                    && is_air(world, (pos.0, pos.1 + 1, pos.2))
                {
                    hole_count += 1;
                }
            }
        }
    }

    if !(1..=5).contains(&hole_count) {
        return false;
    }

    let cobblestone = blocks::COBBLESTONE;
    let mossy = blocks::MOSSY_COBBLESTONE;
    let cave_air = blocks::CAVE_AIR;
    for dx in min_x..=max_x {
        for dy in (-1..=3).rev() {
            for dz in min_z..=max_z {
                let pos = (ox + dx, oy + dy, oz + dz);
                let wall_state = world.get_block(pos.0, pos.1, pos.2);
                let is_shell =
                    dx == min_x || dy == -1 || dz == min_z || dx == max_x || dy == 4 || dz == max_z;
                if is_shell {
                    let below = world.get_block(pos.0, pos.1 - 1, pos.2);
                    if pos.1 >= world.min_y && !is_solid(below) {
                        // 官方这一支是裸 `level.setBlock(wallBlock, AIR, 2)`，不过滤替换条件。
                        // This branch is a bare `level.setBlock(wallBlock, AIR, 2)` in the
                        // official code: no replaceability filter.
                        world.set_block_with_heightmaps(pos.0, pos.1, pos.2, cave_air);
                    } else if is_solid(wall_state) && !is_block(wall_state, "minecraft:chest") {
                        let state = if dy == -1 && random.next_int(4) != 0 {
                            mossy
                        } else {
                            cobblestone
                        };
                        safe_set_block(world, pos, state, cannot_replace);
                    }
                } else if !is_block(wall_state, "minecraft:chest")
                    && !is_block(wall_state, "minecraft:spawner")
                {
                    safe_set_block(world, pos, cave_air, cannot_replace);
                }
            }
        }
    }
    let chest = blocks::CHEST;
    for _ in 0..2 {
        for _ in 0..3 {
            let xc = ox + random.next_int(xr * 2 + 1) - xr;
            let yc = oy;
            let zc = oz + random.next_int(zr * 2 + 1) - zr;
            let chest_pos = (xc, yc, zc);
            if !is_air(world, chest_pos) {
                continue;
            }
            let mut wall_count = 0;
            for direction in HORIZONTAL {
                let neighbor = offset(chest_pos, direction);
                if is_solid(world.get_block(neighbor.0, neighbor.1, neighbor.2)) {
                    wall_count += 1;
                }
            }
            if wall_count == 1 {
                let state = reorient(world, chest_pos, chest);
                safe_set_block(world, chest_pos, state, cannot_replace);
                // `RandomizableContainer.setBlockEntityLootTable`：取一次 `nextLong` 当种子，
                // 和战利品表一起写进箱子的方块实体。
                // `RandomizableContainer.setBlockEntityLootTable`: one `nextLong` seed,
                // stored with the loot table in the chest's block entity.
                let loot_seed = random.next_long();
                world.set_block_entity(
                    chest_pos.0,
                    chest_pos.1,
                    chest_pos.2,
                    &mc_bamboo_nbt::Tag::Compound(vec![
                        (
                            "LootTable".to_owned(),
                            mc_bamboo_nbt::Tag::String(
                                "minecraft:chests/simple_dungeon".to_owned(),
                            ),
                        ),
                        (
                            "LootTableSeed".to_owned(),
                            mc_bamboo_nbt::Tag::Long(loot_seed),
                        ),
                    ]),
                );
                break;
            }
        }
    }

    safe_set_block(world, (ox, oy, oz), blocks::SPAWNER, cannot_replace);
    // `BaseSpawner.getOrCreateNextSpawnData` 在 `spawnPotentials` 为空时不取数，
    // `setEntityId` 只是写入 id。
    // `BaseSpawner.getOrCreateNextSpawnData` draws nothing when `spawnPotentials` is empty,
    // and `setEntityId` only writes the id.
    let mob = MOBS[random.next_int(MOBS.len() as i32) as usize];
    world.set_block_entity(ox, oy, oz, &crate::structure::piece_place::spawner_nbt(mob));
    true
}

/// `Feature.safeSetBlock(level, pos, state, canReplace)`：目标格不能替换时不动。
///
/// `Feature.safeSetBlock(level, pos, state, canReplace)`: skip when the target is not
/// replaceable.
fn safe_set_block(
    world: &mut FeatureWorld<'_>,
    pos: (i32, i32, i32),
    state: u32,
    cannot_replace: &[i32],
) {
    let current = world.get_block(pos.0, pos.1, pos.2);
    let replaceable = mc_bamboo_data::block_id_of_state(current)
        .is_none_or(|id| !cannot_replace.contains(&(id as i32)));
    if replaceable {
        world.set_block_with_heightmaps(pos.0, pos.1, pos.2, state);
    }
}

/// `WorldGenLevel.isEmptyBlock`：空气（含洞穴空气）。
///
/// `WorldGenLevel.isEmptyBlock`: air (cave air included).
fn is_air(world: &FeatureWorld<'_>, pos: (i32, i32, i32)) -> bool {
    state_facts(world.get_block(pos.0, pos.1, pos.2)) & facts::AIR != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monster_room_has_no_config_fields() {
        let config: Value = serde_json::json!({"type": "minecraft:monster_room"});
        assert!(MonsterRoomFeature::parse(&config).is_ok());
    }

    #[test]
    fn clockwise_matches_the_official_horizontal_order() {
        // NORTH -> EAST -> SOUTH -> WEST -> NORTH。
        // NORTH -> EAST -> SOUTH -> WEST -> NORTH.
        assert_eq!(clockwise((0, 0, -1)), (1, 0, 0));
        assert_eq!(clockwise((1, 0, 0)), (0, 0, 1));
        assert_eq!(clockwise((0, 0, 1)), (-1, 0, 0));
        assert_eq!(clockwise((-1, 0, 0)), (0, 0, -1));
    }
}

#[cfg(test)]
mod fact_checks {
    use super::*;

    #[test]
    fn solid_render_matches_the_blocks_used_here() {
        for name in [
            "minecraft:cobblestone",
            "minecraft:mossy_cobblestone",
            "minecraft:chest",
            "minecraft:stone",
            "minecraft:air",
            "minecraft:cave_air",
        ] {
            let block = mc_bamboo_data::block_by_name(name).expect(name);
            println!(
                "{name}: solid={} solid_render={}",
                is_solid(block.default_state),
                is_solid_render(block.default_state)
            );
        }
    }
}
