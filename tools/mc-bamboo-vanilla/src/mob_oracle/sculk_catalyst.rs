//! 幽匿催发体对拍：生物死在催发体旁边，经验转成电荷，`SculkSpreader` 逐刻
//! 蔓延生成幽匿块 / 幽匿脉络；另有一条存档往返对拍。
//!
//! Java 一侧用真实的 `SculkCatalystBlockEntity` + `SculkSpreader` 驱动，死亡事件走
//! 官方 `ServerLevel.gameEvent` → `CatalystListener.handleGameEvent`。这里用
//! `world.game_event` 走 Bamboo 的完整分发，两者一致。

use mc_bamboo_blocks::{
    behaviour::misc::sculk_catalyst::{
        SculkCatalystEntity, additional_from_tag, create_block_entity,
    },
    game_event::{GameEvent, dispatch::EventContext},
};
use mc_bamboo_core::{BlockPos, Direction, Vec3};

use super::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SculkCatalystCase {
    name: String,
    start_time: i64,
    blocks: Vec<[i32; 4]>,
    catalyst: [i32; 3],
    /// 死亡生物的格位置（事件源），经验由生物类型推导（僵尸 5）。
    die: [i32; 3],
    frames: Vec<SculkCatalystFrame>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SculkCatalystFrame {
    /// 每只电荷游标 `[x, y, z, charge, updateDelay, decayDelay, facings]`，
    /// `facings` 是官方 `MultifaceBlock.pack` 的位编码（0 表示 null）。
    cursors: Vec<[i32; 7]>,
    /// 这一刻全部非空气方块，按 `(x, y, z)` 排序，`[x, y, z, 状态号]`。
    blocks: Vec<[i32; 4]>,
    level_seed: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SculkCatalystSaveCase {
    filled: bool,
    nbt: String,
    reloaded: String,
}

/// `Direction` 到 `MultifaceBlock.pack` 的位编码，与 `sculk_catalyst::pack_faces` 一致。
fn pack_faces(faces: Option<&[Direction]>) -> i32 {
    faces.map_or(0, |faces| {
        faces.iter().fold(0, |bits, &face| match face {
            Direction::Down => bits | 1,
            Direction::Up => bits | 2,
            Direction::North => bits | 4,
            Direction::South => bits | 8,
            Direction::West => bits | 16,
            Direction::East => bits | 32,
        })
    })
}

fn mob_level(blocks: &[[i32; 4]]) -> MobLevel {
    MobLevel {
        blocks: blocks
            .iter()
            .map(|&[x, y, z, state]| (BlockPos::new(x, y, z), BlockState(state as u32)))
            .collect(),
        random: LevelRandom::new(1),
        loot_sequences: mc_bamboo_loot::RandomSequences::new(0),
        looting: Vec::new(),
        rules: mc_bamboo_blocks::BlockGameRules::vanilla(),
        facts: ZombieFacts {
            difficulty: 2,
            ..ZombieFacts::default()
        },
        natural_spawn: None,
        biome: None,
        redstone: mc_bamboo_blocks::RedstoneMemory::default(),
        block_entities: BlockEntities::default(),
        block_events: BlockEvents::default(),
        game_event_listeners: Default::default(),
        warden_warnings: Default::default(),
        darkness_ticks: 0,
        poi: mc_bamboo_world::poi::PoiStore::new(),
        entities: EntityStore::with_next_id(1),
        players: Vec::new(),
        game: None,
        inhabited: HashMap::new(),
        mob_teams: HashMap::new(),
        wrong_was_hurt: false,
        explosion_break: 0,
        shoulder: Default::default(),
        containers: Default::default(),
        container_open_counts: Default::default(),
        short_hunger: false,
        ignore_bed_monsters: false,
    }
}

/// 读全部电荷游标，转成 `[x, y, z, charge, updateDelay, decayDelay, facings]`。
fn dump_cursors(world: &World<'_>, pos: BlockPos) -> Vec<[i32; 7]> {
    world
        .block_entity::<SculkCatalystEntity>(pos)
        .map(|entity| {
            entity
                .spreader
                .cursors
                .iter()
                .map(|cursor| {
                    [
                        cursor.pos.x,
                        cursor.pos.y,
                        cursor.pos.z,
                        cursor.charge,
                        cursor.update_delay,
                        cursor.decay_delay,
                        pack_faces(cursor.facings.as_deref()),
                    ]
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 全部非空气方块，按 `(x, y, z)` 排序，`[x, y, z, 状态号]`。
fn dump_blocks(level: &MobLevel) -> Vec<[i32; 4]> {
    let mut out: Vec<[i32; 4]> = level
        .blocks
        .iter()
        .filter(|(_, state)| !state.is_air())
        .map(|(&pos, &state)| [pos.x, pos.y, pos.z, state.0 as i32])
        .collect();
    out.sort_by_key(|entry| (entry[0], entry[1], entry[2]));
    out
}

impl Report {
    pub(super) fn replay_sculk_catalyst(&mut self, case: &SculkCatalystCase) {
        use mc_bamboo_blocks::entity::{Entity, mob::MobType};

        self.cases += 1;
        let broken = self.break_test.as_deref();
        let break_charge = broken == Some("sculk_catalyst_charge");
        let break_block = broken == Some("sculk_catalyst_block");

        let catalyst = BlockPos::new(case.catalyst[0], case.catalyst[1], case.catalyst[2]);
        let die = Vec3::new(
            f64::from(case.die[0]) + 0.5,
            f64::from(case.die[1]),
            f64::from(case.die[2]) + 0.5,
        );
        let mut level = mob_level(&case.blocks);
        let mut ticks = BlockTicks::default();
        let zombie = {
            let mut world = World::new(&mut level, &mut ticks);
            world.set_block_entity(create_block_entity(catalyst));
            let entity = Entity::by_mob_type(
                MobType::Zombie,
                Vec3::new(
                    f64::from(case.die[0]) + 0.5,
                    f64::from(case.die[1]),
                    f64::from(case.die[2]) + 0.5,
                ),
            );
            world.spawn_entity(entity)
        };

        for (tick, expected) in case.frames.iter().enumerate() {
            if tick > 0 {
                let now = case.start_time + tick as i64;
                ticks.set_time(now);
                {
                    let mut world = World::new(&mut level, &mut ticks);
                    if tick == 1 {
                        world.game_event(
                            GameEvent::EntityDie,
                            die,
                            EventContext {
                                source_entity: Some(zombie),
                                ..Default::default()
                            },
                        );
                    }
                }
                let _ = mc_bamboo_blocks::run_block_entity_tick(&mut level, &mut ticks);
                let _ = mc_bamboo_blocks::run_due_ticks(&mut level, &mut ticks, now);
            }

            let world = World::new(&mut level, &mut ticks);
            let mut cursors = dump_cursors(&world, catalyst);
            let mut blocks = dump_blocks(&level);
            let level_seed = level.random.source().seed();
            if break_charge && tick == 2 && !cursors.is_empty() {
                cursors[0][3] = cursors[0][3].saturating_add(1);
            }
            if break_block {
                blocks.retain(|entry| entry[0] != catalyst.x || entry[2] != catalyst.z);
            }
            if cursors != expected.cursors {
                self.mismatch(format!(
                    "{} tick {tick} cursors: official {:?}, bamboo {cursors:?}",
                    case.name, expected.cursors
                ));
            }
            if blocks != expected.blocks {
                self.mismatch(format!(
                    "{} tick {tick} blocks: official {:?}, bamboo {blocks:?}",
                    case.name, expected.blocks
                ));
            }
            if level_seed != expected.level_seed {
                self.mismatch(format!(
                    "{} tick {tick} level seed: official {}, bamboo {level_seed}",
                    case.name, expected.level_seed
                ));
            }
            self.steps += 1;
            if self.mismatches.len() >= self.show {
                return;
            }
        }
        println!("{}: {} ticks", case.name, case.frames.len());
    }

    pub(super) fn replay_sculk_catalyst_save(&mut self, case: &SculkCatalystSaveCase) {
        self.cases += 1;
        let label = format!("sculk catalyst save filled {}", case.filled);
        let official = match mc_bamboo_nbt::parse_snbt(&case.nbt) {
            Ok(tag) => tag,
            Err(error) => {
                self.mismatch(format!("{label}: SNBT does not parse: {error:?}"));
                return;
            }
        };
        let reloaded = match mc_bamboo_nbt::parse_snbt(&case.reloaded) {
            Ok(tag) => tag,
            Err(error) => {
                self.mismatch(format!("{label}: reloaded SNBT does not parse: {error:?}"));
                return;
            }
        };
        // `SculkSpreader.load` → `save` 走同一条 codec，比较 `cursors` 键。
        let ours = additional_from_tag(&official);
        let expected =
            super::small_mob::save_field(&reloaded, "cursors").map(super::small_mob::canonical_tag);
        let mut actual = ours
            .iter()
            .find(|(name, _)| name == "cursors")
            .map(|(_, value)| super::small_mob::canonical_tag(value));
        if self.break_test.as_deref() == Some("sculk_catalyst_save") && case.filled {
            actual = None;
        }
        if expected != actual {
            self.mismatch(format!(
                "{label} cursors: official {expected:?}, bamboo {actual:?}"
            ));
        }
    }
}
