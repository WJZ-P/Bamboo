//! 幽匿感测体对拍：逐刻比相位、红石输出强度和比较器频率。
//!
//! Java 一侧用真实的 `SculkSensorBlockEntity` + `VibrationSystem` 驱动，事件走
//! 官方的 `ServerLevel.gameEvent` → `GameEventDispatcher.post`（区段路由 + 按距离
//! 排序后逐个投递）。这里用 `world.game_event` 走 Bamboo 的完整分发，两者一致。

use mc_bamboo_blocks::{
    behaviour::misc::sculk_entity::{SculkSensorEntity, create_block_entity},
    game_event::{GameEvent, dispatch::EventContext},
    state::{POWER, SCULK_SENSOR_PHASE, SculkSensorPhase},
};

use super::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SculkSensorCase {
    name: String,
    start_time: i64,
    blocks: Vec<[i32; 4]>,
    sensor: [i32; 3],
    calibrated: bool,
    /// `[刻, 事件 id, x, y, z]`：这一刻在 `(x, y, z)` 发一个游戏事件。
    events: [i32; 5],
    /// 冷却场景的第二发事件；没有时是 `null`。
    event2: Option<[i32; 5]>,
    /// 每刻 `[相位, 红石强度, 频率]`，相位 0 待命 / 1 激活 / 2 冷却。
    frames: Vec<[i32; 3]>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SculkSensorOrderCase {
    name: String,
    start_time: i64,
    blocks: Vec<[i32; 4]>,
    sensors: Vec<[i32; 3]>,
    events: [i32; 5],
    /// 每刻每只感测体一个 `[相位, 红石强度, 频率]`。
    frames: Vec<Vec<[i32; 3]>>,
}

/// 在 `world` 里按 `[刻, 事件 id, x, y, z]` 发一个游戏事件（格心）。
fn fire(world: &mut World<'_>, event: &[i32; 5]) {
    let kind = GameEvent::from_id(event[1] as u8).unwrap_or(GameEvent::Step);
    let source = mc_bamboo_core::Vec3::new(
        f64::from(event[2]) + 0.5,
        f64::from(event[3]) + 0.5,
        f64::from(event[4]) + 0.5,
    );
    world.game_event(kind, source, EventContext::default());
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
        facts: ZombieFacts::default(),
        natural_spawn: None,
        biome: None,
        redstone: mc_bamboo_blocks::RedstoneMemory::default(),
        block_entities: BlockEntities::default(),
        block_events: BlockEvents::default(),
        game_event_listeners: Default::default(),
        poi: mc_bamboo_world::poi::PoiStore::new(),
        entities: EntityStore::with_next_id(1),
        players: Vec::new(),
        game: None,
        inhabited: HashMap::new(),
        mob_teams: HashMap::new(),
        wrong_was_hurt: false,
        short_hunger: false,
        ignore_bed_monsters: false,
    }
}

/// 读一只感测体的 `[相位, 红石强度, 频率]`。
fn sensor_state(world: &World<'_>, pos: BlockPos) -> [i32; 3] {
    let state = world.block_state(pos);
    let phase = match state.get(SCULK_SENSOR_PHASE) {
        SculkSensorPhase::Inactive => 0,
        SculkSensorPhase::Active => 1,
        SculkSensorPhase::Cooldown => 2,
    };
    let power = state.get(POWER);
    let freq = world
        .block_entity::<SculkSensorEntity>(pos)
        .map(|entity| entity.last_frequency())
        .unwrap_or(0);
    [phase, power, freq]
}

impl Report {
    pub(super) fn replay_sculk_sensor(&mut self, case: &SculkSensorCase) {
        self.cases += 1;
        let broken = self.break_test.as_deref();
        let break_phase = broken == Some("sculk_sensor_phase");
        let break_power = broken == Some("sculk_sensor_power");

        let sensor = BlockPos::new(case.sensor[0], case.sensor[1], case.sensor[2]);
        let mut level = mob_level(&case.blocks);
        let mut ticks = BlockTicks::default();
        {
            let mut world = World::new(&mut level, &mut ticks);
            world.set_block_entity(create_block_entity(sensor, case.calibrated));
        }

        for (tick, expected) in case.frames.iter().enumerate() {
            if tick > 0 {
                let now = case.start_time + tick as i64;
                ticks.set_time(now);
                {
                    let mut world = World::new(&mut level, &mut ticks);
                    if case.events[0] == tick as i32 {
                        fire(&mut world, &case.events);
                    }
                    if let Some(second) = &case.event2
                        && second[0] == tick as i32
                    {
                        fire(&mut world, second);
                    }
                }
                let _ = mc_bamboo_blocks::run_block_entity_tick(&mut level, &mut ticks);
                let _ = mc_bamboo_blocks::run_due_ticks(&mut level, &mut ticks, now);
            }

            let world = World::new(&mut level, &mut ticks);
            let mut state = sensor_state(&world, sensor);
            if break_phase && tick == 4 {
                state[0] = 0;
            }
            if break_power {
                state[1] = state[1].saturating_add(1);
            }
            if state != *expected {
                self.mismatch(format!(
                    "{} tick {tick}: official {expected:?}, bamboo {state:?}",
                    case.name
                ));
            }
        }
    }

    pub(super) fn replay_sculk_sensor_order(&mut self, case: &SculkSensorOrderCase) {
        self.cases += 1;
        let sensors: Vec<BlockPos> = case
            .sensors
            .iter()
            .map(|&[x, y, z]| BlockPos::new(x, y, z))
            .collect();
        let mut level = mob_level(&case.blocks);
        let mut ticks = BlockTicks::default();
        {
            let mut world = World::new(&mut level, &mut ticks);
            for &pos in &sensors {
                world.set_block_entity(create_block_entity(pos, false));
            }
        }

        for (tick, expected) in case.frames.iter().enumerate() {
            if tick > 0 {
                let now = case.start_time + tick as i64;
                ticks.set_time(now);
                {
                    let mut world = World::new(&mut level, &mut ticks);
                    if case.events[0] == tick as i32 {
                        fire(&mut world, &case.events);
                    }
                }
                let _ = mc_bamboo_blocks::run_block_entity_tick(&mut level, &mut ticks);
                let _ = mc_bamboo_blocks::run_due_ticks(&mut level, &mut ticks, now);
            }

            let world = World::new(&mut level, &mut ticks);
            let states: Vec<[i32; 3]> = sensors
                .iter()
                .map(|&pos| sensor_state(&world, pos))
                .collect();
            if states != *expected {
                self.mismatch(format!(
                    "{} tick {tick}: official {expected:?}, bamboo {states:?}",
                    case.name
                ));
            }
        }
    }
}
