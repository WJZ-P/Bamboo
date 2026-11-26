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
    /// 玩家生命（`float` 原始位）。嘎枝从 3 格外的绑定点走进近战范围才会掉血，所以
    /// 生命下降本身就同时证明了「走过去」和「攻击」。玩家没 tick，首次命中后进入
    /// 永久无敌，生命只在命中那一刻从 20 掉到 17。
    player_health: i32,
    /// 挥击动画剩余刻数（`attackAnimationRemainingTicks`）。每次近战置 15 并逐刻递减，
    /// 直接反映 40 刻攻击冷却：把冷却改坏成 1 会每刻挥击、动画常驻 15。
    attack_animation: i32,
    /// 嘎枝逐刻位置（`double` 原始位）。修好 FIGHT 优先级后两条链同一刻走同一段，
    /// 逐刻严格相等；改坏时序会让整段位移错开一刻。
    x: i64,
    z: i64,
}

impl CreakingCase {
    fn level(&self) -> MobLevel {
        MobLevel {
            raid: None,
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
            game_event_listeners: Default::default(),
            warden_warnings: Default::default(),
            darkness_ticks: 0,
            poi: mc_bamboo_world::poi::PoiStore::new(),
            containers: Default::default(),
            container_open_counts: Default::default(),
            shoulder: Default::default(),
            entities: Entities::with_next_id(self.first_id),
            players: Vec::new(),
            game: None,
            inhabited: Default::default(),
            mob_teams: Default::default(),
            ignore_bed_monsters: false,
            short_hunger: false,
            wrong_was_hurt: false,
            explosion_break: 0,
        }
    }
}

/// 和 Java 场景同一个玩家：站在 (4.5, 61, 0.5)，看着负 X 方向的嘎枝；追人场景
/// 站在更远的 (6.5, 61, 0.5)，让嘎枝从 3 格外走进近战范围再动手。
fn gaze_player(name: &str) -> PlayerView {
    let x = if name == "creaking_chase" { 6.5 } else { 4.5 };
    PlayerView {
        uuid: Some(((1_u128 << 64) | 2).to_be_bytes()),
        uid: 1025,
        yaw: 90.0,
        pitch: 0.0,
        position: Vec3::new(x, 61.0, 0.5),
        delta: Vec3::default(),
        width: 0.6,
        height: 1.8,
        shoulder_left: None,
        shoulder_right: None,
        eye_height: 1.62,
        health: 20.0,
        look_direction: Vec3::view_vector(90.0, 0.0),
        gaze_disguise: false,
        alive: true,
        spectator: false,
        game_mode: 0,
        mining_fatigue: None,
        creative: false,
        flying: false,
        explosion_knockback_resistance: 0.0,
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

/// 改坏确认：冻结、绑定、树脂、近战命中（生命）、挥击冷却（动画）、激活态和位移各改
/// 一个被逐刻比较的字段。分别对应 `checkCanMove` / 心绑定 / 树脂蔓延 / `MeleeAttack` 的
/// 命中与 40 刻冷却 / `StartAttacking` / `MoveToTargetSink` 的逐刻位置。
fn broken(break_test: Option<&str>, frame: &mut CreakingFrame) {
    match break_test {
        Some("creaking_freeze") => frame.can_move = !frame.can_move,
        Some("creaking_bound") => frame.bound = !frame.bound,
        Some("creaking_resin") => frame.resin += 1,
        Some("creaking_attack") => frame.player_health = frame.player_health.wrapping_add(1),
        Some("creaking_cooldown") => {
            frame.attack_animation = frame.attack_animation.wrapping_add(1)
        }
        Some("creaking_active") => frame.active = !frame.active,
        Some("creaking_position") => frame.x = frame.x.wrapping_add(1),
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
    let player = gaze_player(&case.name);
    // `creaking_watched / creaking_unwatched / creaking_hurt / creaking_heart_broken`
    // 预先绑定一只嘎枝；`creaking_night` 让心在夜里自己刷出一只。
    let pre_bound = !case.name.starts_with("creaking_night");
    let id = case.first_id;
    if pre_bound {
        let mut entity = Entity::by_mob_type(MobType::Creaking, Vec3::new(3.5, 61.0, 0.5));
        entity.id = case.first_id;
        entity.random = JavaRandom::new(0x5EED_5EED);
        entity.y_rot = 0.0;
        entity.with_mob(|me, mob| {
            mob.y_head_rot = 0.0;
            mob.y_body_rot = 0.0;
            mob.uuid = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
            mc_bamboo_blocks::entity::mob::creaking::bind(mob, home);
            // `Brain.Provider.makeBrain` 的两个 20 刻传感器各取一次 `nextInt(20)`；
            // 夹具把实体随机源覆写成场景种子后重放这两次取数、把延迟定成 1、并按
            // 推进后的状态重设闸门洗牌。Bamboo 构造时用 `ENTITY_RANDOM_SEED` 取过一轮，
            // 这里按场景初值覆盖回去，逐刻取数链和官方同相位。
            if let Some(brain) = mob.brain.as_mut() {
                brain.randomly_delay_start(&mut me.random);
                brain.seed_shuffles(me.random.state());
                for sensor in &mut brain.sensors {
                    sensor.time_to_tick = 1;
                }
            }
        });
        heart.set_creaking_info([0, 0, 0, 1], false);
        level.entities.insert_row_preserving(entity.split());
        let _ = level.entities.take_spawn_snapshots();
    }
    level.block_entities.insert(Box::new(heart));
    level.players.push(player);

    let break_test = report.break_test.clone();
    // 夜里刷出的嘎枝只在出现那一刻对齐一次场景种子。
    let mut spawned_initialized = false;
    // 官方假场景不 tick 玩家，所以首次命中后 `damageCooldownTime` 不减，之后所有近战
    // 命中都被无敌挡下——生命只从 20 掉一次到 17。这里 drain `Effect::HurtPlayer`
    // （`game: None` 时 `hurt_player_now` 把伤害记成延后效果），命中一次扣一次 3 点。
    let mut player_hurt = false;
    for (tick, expected) in case.frames.iter().enumerate() {
        if tick != 0 {
            ticks.set_time(1000 + tick as i64);
            // `creaking_watched` 全程看；`creaking_chase` 前几刻看（激活）再转头追，
            // 末段再转回来看，验证「玩家转头看它时停住」。
            let watching = match case.name.as_str() {
                "creaking_watched" => true,
                "creaking_chase" => !(5..45).contains(&tick),
                _ => false,
            };
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
            if !pre_bound && !spawned_initialized {
                // 夜里心刷出的那只由 `prepare_entity` 按实体种子源建；心把它交给
                // 世界的那一刻（只一次）按夹具覆盖成和预绑定场景一样的初值。
                // `with_entity_mut` 返回这一刻实体是否已经出现。
                spawned_initialized = world.with_entity_mut(id, &mut |entity| {
                    entity.with_mob(|me, mob| {
                        me.random = JavaRandom::new(0x5EED_5EED);
                        if let Some(brain) = mob.brain.as_mut() {
                            brain.randomly_delay_start(&mut me.random);
                            brain.seed_shuffles(me.random.state());
                            for sensor in &mut brain.sensors {
                                sensor.time_to_tick = 1;
                            }
                        }
                    });
                });
            }
            if case.name == "creaking_hurt" && tick == 5 {
                let mut source = DamageSource::player(1025, Vec3::new(4.5, 61.0, 0.5), false);
                source.attacker_uuid = Some(((1_u128 << 64) | 2).to_be_bytes());
                let _ = world.hurt_mob(id, source, 3.0);
            }
            if case.name == "creaking_heart_broken" && tick == 3 {
                let _ = world.with_block_entity::<CreakingHeartEntity, _>(home, |be, world| {
                    be.remove_protector_by_player(world)
                });
                world.set_block_and_update(home, BlockState::AIR);
            }
            if case.name == "creaking_heart_removed" && tick == 3 {
                let _ = world.with_block_entity::<CreakingHeartEntity, _>(home, |be, world| {
                    be.remove_protector(world)
                });
                world.set_block_and_update(home, BlockState::AIR);
            }
            for effect in world.drain_effects() {
                if let mc_bamboo_blocks::Effect::HurtPlayer { target, amount, .. } = effect
                    && target == 1025
                    && !player_hurt
                {
                    level.players[0].health = (level.players[0].health - amount).max(0.0);
                    player_hurt = true;
                }
            }
        }
        let player_health = level.players[0].health.to_bits() as i32;
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
        // 位置取官方同一套原始位；嘎枝已移除的帧两边都写 0。
        let mut ours = level.entities.get(id).map(|entity| {
            let mob = entity.kind.mob().expect("creaking is a mob");
            let (x, z) = if entity.removed {
                (0, 0)
            } else {
                (
                    entity.position.x.to_bits() as i64,
                    entity.position.z.to_bits() as i64,
                )
            };
            CreakingFrame {
                heart_state: heart_state.clone(),
                bound,
                creaking_present: !entity.removed,
                can_move: mob.creaking.can_move,
                active: mob.creaking.active,
                tearing_down: mob.creaking.tearing_down,
                resin,
                player_health,
                attack_animation: mob.creaking.attack_animation,
                x,
                z,
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
                player_health,
                attack_animation: 0,
                x: 0,
                z: 0,
            });
        }
        // 位置、生命、动画和所有离散字段一律逐刻严格相等，没有任何放宽窗口。
        // 优先级修好后两条链同一刻起步，落刀、挥击冷却、位移都在同一刻对齐。
        if ours.as_ref() != Some(expected) {
            report.mismatch(format!(
                "{} tick {tick}: official {expected:?}, bamboo {:?}",
                case.name,
                ours.unwrap()
            ));
        }
        report.steps += 1;
    }

    // 存档往返：home_pos 和心的 creaking UUID 都不许丢（拆解 / 移走场景里嘎枝已移除，跳过）。
    if pre_bound && case.name != "creaking_heart_broken" && case.name != "creaking_heart_removed" {
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
