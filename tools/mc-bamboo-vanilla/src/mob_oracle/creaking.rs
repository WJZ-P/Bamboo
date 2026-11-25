//! 嘎枝 / 嘎枝之心的逐刻对拍：昼夜状态、绑定、被看着冻结、受伤与拆解。
//!
//! Java 一侧按官方 `serverTick` / `Creaking.aiStep` 把状态写成布尔帧；这里用
//! Bamboo 的同一份方块实体、实体和玩家快照逐刻重放并逐字段比较。

use mc_bamboo_blocks::{
    BlockEntity,
    behaviour::misc::creaking_heart_entity::CreakingHeartEntity,
    entity::ctx::PlayerView,
    entity::mob::{DamageSource, MobType, nbt},
};
use mc_bamboo_data::{blocks, registries::entity_type};
use mc_bamboo_nbt::Tag;

use super::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CreakingCase {
    name: String,
    first_id: i32,
    blocks: Vec<[i32; 4]>,
    frames: Vec<CreakingFrame>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct CreakingFrame {
    heart_state: String,
    bound: bool,
    creaking_present: bool,
    can_move: bool,
    active: bool,
    tearing_down: bool,
    resin: i32,
}

impl CreakingCase {
    fn level(&self) -> MobLevel {
        MobLevel {
            blocks: self
                .blocks
                .iter()
                .map(|&[x, y, z, state]| (BlockPos::new(x, y, z), BlockState(state as u32)))
                .collect(),
            random: LevelRandom::new(1),
            loot_sequences: mc_bamboo_loot::RandomSequences::new(0),
            looting: Vec::new(),
            rules: mc_bamboo_blocks::BlockGameRules::vanilla(),
            facts: ZombieFacts {
                difficulty: 2,
                clock: 12600,
                ..ZombieFacts::default()
            },
            natural_spawn: None,
            biome: None,
            redstone: mc_bamboo_blocks::RedstoneMemory::default(),
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
            poi: mc_bamboo_world::poi::PoiStore::new(),
            entities: Entities::with_next_id(self.first_id),
            players: Vec::new(),
            game: None,
            inhabited: Default::default(),
            mob_teams: Default::default(),
            ignore_bed_monsters: false,
            short_hunger: false,
            wrong_was_hurt: false,
        }
    }
}

/// 和 Java 场景同一个玩家：站在 (4.5, 61, 0.5)，看着负 X 方向的嘎枝。
fn gaze_player() -> PlayerView {
    PlayerView {
        uuid: Some(((1_u128 << 64) | 2).to_be_bytes()),
        uid: 1025,
        yaw: 90.0,
        pitch: 0.0,
        position: Vec3::new(4.5, 61.0, 0.5),
        delta: Vec3::default(),
        eye_height: 1.62,
        health: 20.0,
        look_direction: Vec3::view_vector(90.0, 0.0),
        gaze_disguise: false,
        alive: true,
        spectator: false,
        game_mode: 0,
        mining_fatigue: None,
        creative: false,
        sneaking: false,
        sprinting: false,
        swimming: false,
        invisible: false,
        armor_cover: 0.0,
        main_hand: 0,
        off_hand: 0,
        team: None,
        combat: Default::default(),
        effects: Default::default(),
    }
}

fn player_looking(player: &mut PlayerView, watching: bool) {
    player.look_direction = if watching {
        Vec3::view_vector(90.0, 0.0)
    } else {
        Vec3::view_vector(-90.0, 0.0)
    };
    player.yaw = if watching { 90.0 } else { -90.0 };
}

fn resin_clumps(world: &World) -> i32 {
    let mut count = 0;
    for x in -6..=6 {
        for y in 58..=66 {
            for z in -6..=6 {
                if world
                    .block_state(BlockPos::new(x, y, z))
                    .is(blocks::RESIN_CLUMP)
                {
                    count += 1;
                }
            }
        }
    }
    count
}

/// 三处改坏确认：冻结、绑定和树脂数量各改一个被逐刻比较的字段。
fn broken(break_test: Option<&str>, frame: &mut CreakingFrame) {
    match break_test {
        Some("creaking_freeze") => frame.can_move = !frame.can_move,
        Some("creaking_bound") => frame.bound = !frame.bound,
        Some("creaking_resin") => frame.resin += 1,
        _ => {}
    }
}

pub(super) fn replay_creaking(report: &mut Report, case: &CreakingCase) {
    report.cases += 1;
    let mut level = case.level();
    let mut ticks = BlockTicks::default();
    ticks.set_time(1000);
    let home = BlockPos::new(0, 64, 0);
    let mut heart = CreakingHeartEntity::new(home);
    let player = gaze_player();
    // `creaking_watched / creaking_unwatched / creaking_hurt / creaking_heart_broken`
    // 预先绑定一只嘎枝；`creaking_night` 让心在夜里自己刷出一只。
    let pre_bound = !case.name.starts_with("creaking_night");
    let id = case.first_id;
    if pre_bound {
        let mut entity = Entity::by_mob_type(MobType::Creaking, Vec3::new(3.5, 61.0, 0.5));
        entity.id = case.first_id;
        entity.random = JavaRandom::new(0x5EED_5EED);
        entity.y_rot = 0.0;
        entity.with_mob(|_, mob| {
            mob.y_head_rot = 0.0;
            mob.y_body_rot = 0.0;
            mob.uuid = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
            mc_bamboo_blocks::entity::mob::creaking::bind(mob, home);
        });
        heart.set_creaking_info([0, 0, 0, 1], false);
        level.entities.insert_row_preserving(entity.split());
        let _ = level.entities.take_spawn_snapshots();
    }
    level.block_entities.insert(Box::new(heart));
    level.players.push(player);

    let break_test = report.break_test.clone();
    for (tick, expected) in case.frames.iter().enumerate() {
        if tick != 0 {
            ticks.set_time(1000 + tick as i64);
            let watching = case.name == "creaking_watched";
            player_looking(&mut level.players[0], watching);
            if case.name == "creaking_night" {
                // 夜里刷出的嘎枝落点随机，让玩家抬头避免正好对视。
                level.players[0].look_direction = Vec3::view_vector(0.0, -90.0);
                level.players[0].yaw = 0.0;
            }
            let mut world = World::new(&mut level, &mut ticks);
            // `ServerLevel.tick` 顺序：先实体，后方块实体。
            world.tick_entities();
            let _ =
                world.with_block_entity::<CreakingHeartEntity, _>(home, |be, world| be.tick(world));
            if case.name == "creaking_hurt" && tick == 1 {
                let mut source = DamageSource::player(1025, Vec3::new(4.5, 61.0, 0.5), false);
                source.attacker_uuid = Some(((1_u128 << 64) | 2).to_be_bytes());
                let _ = world.hurt_mob(id, source, 3.0);
            }
            if case.name == "creaking_heart_broken" && tick == 3 {
                let _ = world.with_block_entity::<CreakingHeartEntity, _>(home, |be, world| {
                    be.remove_protector(world)
                });
                world.set_block_and_update(home, BlockState::AIR);
            }
        }
        let world = World::new(&mut level, &mut ticks);
        let state = world.block_state(home);
        let heart_state = state
            .try_get(mc_bamboo_blocks::state::CREAKING_HEART_STATE)
            .map(|value| value.name().to_owned())
            .unwrap_or_else(|| "air".to_owned());
        let bound = world
            .block_entity::<CreakingHeartEntity>(home)
            .is_some_and(|be| be.creaking_uuid().is_some());
        let resin = resin_clumps(&world);
        drop(world);
        let mut ours = level.entities.get(id).map(|entity| {
            let mob = entity.kind.mob().expect("creaking is a mob");
            CreakingFrame {
                heart_state: heart_state.clone(),
                bound,
                creaking_present: !entity.removed,
                can_move: mob.creaking.can_move,
                active: mob.creaking.active,
                tearing_down: mob.creaking.tearing_down,
                resin,
            }
        });
        if let Some(frame) = &mut ours {
            broken(break_test.as_deref(), frame);
        } else {
            ours = Some(CreakingFrame {
                heart_state,
                bound,
                creaking_present: false,
                can_move: false,
                active: false,
                tearing_down: false,
                resin,
            });
        }
        if ours.as_ref() != Some(expected) {
            report.mismatch(format!(
                "{} tick {tick}: official {expected:?}, bamboo {:?}",
                case.name,
                ours.unwrap()
            ));
        }
        report.steps += 1;
    }

    // 存档往返：home_pos 和心的 creaking UUID 都不许丢（拆解场景里嘎枝已移除，跳过）。
    if pre_bound && case.name != "creaking_heart_broken" {
        let mut copy = Entity::by_mob_type(MobType::Creaking, Vec3::default());
        let row = level.entities.get(id).expect("bound creaking exists");
        let written = nbt::write_mob(&row.clone().split()).expect("write creaking");
        nbt::load_into(&mut copy, &written).expect("load creaking");
        let home_saved = copy.kind.mob().and_then(|mob| mob.creaking.home_pos);
        if home_saved != Some(home) {
            report.mismatch(format!("{} home_pos: {home_saved:?}", case.name));
        }
        let heart_tag = Tag::Compound(
            level
                .block_entities
                .get_dyn(home)
                .map(|be| be.write_additional().unwrap_or_default())
                .unwrap_or_default(),
        );
        if mc_bamboo_blocks::archive::tag::int_array(&heart_tag, "creaking").len() != 4 {
            report.mismatch(format!("{} heart save lost creaking UUID", case.name));
        }
    }
    let _ = entity_type::CREAKING;
}
