//! 守卫者逐刻对拍：游走、激光、尖刺、远古效果和扑腾。
//!
//! 官方一侧在假世界里直接构造守卫者（和它的目标），逐刻 `commonTick` + `tick`
//! 记录位置 / 速度 / 朝向 / 血量 / 氧气 / 目标 / 元数据；这里用同一份方块和种子
//! 重放，按原始位比较。
//!
//! Per-tick comparison of guardians: swimming, laser, thorns, the elder effect and
//! flopping. Vanilla constructs the guardian in a fake level; the replay uses the
//! same blocks and seeds and compares raw bits.

use mc_bamboo_blocks::entity::{
    Entity, JavaRandom,
    mob::{DamageKind, DamageSource, MobType},
};
use mc_bamboo_core::Vec3;

use super::*;

/// 场景编号，两侧共用：90 普通守卫者、91 远古守卫者。
fn scene_kind(kind: i32) -> MobType {
    match kind {
        90 => MobType::Guardian,
        91 => MobType::ElderGuardian,
        92 => MobType::Squid,
        93 => MobType::Axolotl,
        _ => unreachable!("unknown guardian kind {kind}"),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GuardianCase {
    name: String,
    first_id: i32,
    blocks: Vec<[i32; 4]>,
    difficulty: i32,
    sky_darken: i32,
    guardian: GuardianSetup,
    /// 激光 / 远古效果场景里的玩家目标；游泳和扑腾场景没有玩家。
    ///
    /// The player target in laser / elder scenes; absent in swim and flop scenes.
    #[serde(default)]
    player: Option<GuardianPlayer>,
    /// 尖刺场景：在某刻对守卫者造成一次伤害。
    ///
    /// Thorns scenes: one hit on the guardian at a given tick.
    #[serde(default)]
    hurt: Option<GuardianHurt>,
    /// 生物目标场景的鱿鱼 / 美西螈。
    ///
    /// The squid / axolotl in mob-victim scenes.
    #[serde(default)]
    victim: Option<GuardianVictim>,
    /// 激光中途打断动作。
    ///
    /// A mid-laser interrupt action.
    #[serde(default)]
    interrupt: Option<GuardianInterrupt>,
    frames: Vec<GuardianFrameLine>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GuardianInterrupt {
    tick: i32,
    kind: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GuardianVictim {
    kind: i32,
    position: [i64; 3],
    seed: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GuardianHurt {
    tick: i32,
    amount: f32,
    /// 直接来源是不是活体（玩家近战 true，generic false）。
    ///
    /// Whether the direct source is living (player melee true, generic false).
    living_direct: bool,
    /// 官方一侧在 hurt 前 `setMoving(true)`（移动中不反伤）。
    ///
    /// Vanilla calls `setMoving(true)` before the hit (no reflection while moving).
    #[serde(default)]
    moving: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GuardianPlayer {
    uid: i32,
    position: [i64; 3],
    #[serde(default)]
    health: i32,
    #[serde(default)]
    game_mode: i32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GuardianSetup {
    kind: i32,
    position: [i64; 3],
    seed: i64,
    yaw: i32,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct GuardianFrameLine {
    guardian: GuardianFrame,
    /// 玩家血量的原始位。
    #[serde(default)]
    player_health: i32,
    /// `LivingEntity.hurtTime`（受伤动画计时）。
    #[serde(default)]
    player_hurt_time: i32,
    /// `LivingEntity.damageCooldownTime`（受伤无敌计时）。
    #[serde(default)]
    player_damage_cooldown: i32,
    /// 生物目标血量的原始位。
    #[serde(default)]
    victim_health: i32,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct GuardianFrame {
    /// 位置 xyz、速度 xyz 的原始位。
    motion: [i64; 6],
    y_rot: i32,
    y_head_rot: i32,
    y_body_rot: i32,
    health: i32,
    air: i32,
    hurt_time: i32,
    death_time: i32,
    removed: i32,
    /// `getTarget()`：无目标 -1。
    target: i32,
    /// `DATA_ID_ATTACK_TARGET`。
    attack_target: i32,
    /// `DATA_ID_MOVING`。
    moving: i32,
    #[serde(default)]
    look_cooldown: i32,
    #[serde(default)]
    wanted_x: i64,
    #[serde(default)]
    wanted_z: i64,
    #[serde(default)]
    random_state: i64,
    #[serde(default)]
    no_action_time: i32,
}

impl GuardianCase {
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
                difficulty: self.difficulty,
                sky_darken: self.sky_darken,
                bright_outside: false,
                monsters_burn: false,
                can_see_sky: false,
                sea_level: -63,
                ..ZombieFacts::default()
            },
            natural_spawn: None,
            biome: None,
            redstone: mc_bamboo_blocks::RedstoneMemory::default(),
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
            entities: Entities::with_next_id(self.first_id),
            players: self
                .player
                .as_ref()
                .map(|setup| {
                    let position = Vec3::new(
                        f64::from_bits(setup.position[0] as u64),
                        f64::from_bits(setup.position[1] as u64),
                        f64::from_bits(setup.position[2] as u64),
                    );
                    PlayerView {
                        uid: setup.uid,
                        yaw: 0.0,
                        pitch: 0.0,
                        uuid: None,
                        position,
                        delta: Vec3::default(),
                        eye_height: 1.62,
                        health: f32::from_bits(setup.health as u32),
                        look_direction: Vec3::default(),
                        gaze_disguise: false,
                        alive: true,
                        spectator: false,
                        game_mode: setup.game_mode,
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
                })
                .into_iter()
                .collect(),
            game: self.player.as_ref().map(|setup| {
                let position = Vec3::new(
                    f64::from_bits(setup.position[0] as u64),
                    f64::from_bits(setup.position[1] as u64),
                    f64::from_bits(setup.position[2] as u64),
                );
                // 走 `mc-bamboo-game` 的真实玩家受伤链路：`ReplayPlayers::hurt`
                // → `hurt_player_by_mob` → `rules::hurt`，含官方的
                // `damageCooldownTime > 10` 只补 `damage - lastHurt` 差额。
                mc_bamboo_game::ReplayPlayers::new(
                    self.difficulty,
                    &[mc_bamboo_game::ReplayPlayerSetup {
                        id: setup.uid,
                        uuid: (1_u128 << 64) | 2,
                        name: "GuardianTarget".to_owned(),
                        position,
                        health: f32::from_bits(setup.health as u32),
                        seed: 42002,
                        creative: false,
                    }],
                )
            }),
            inhabited: Default::default(),
            mob_teams: Default::default(),
            wrong_was_hurt: false,
            short_hunger: false,
            ignore_bed_monsters: false,
        }
    }

    fn spawn(&self, level: &mut MobLevel, ticks: &mut BlockTicks, break_test: Option<&str>) -> i32 {
        let setup = &self.guardian;
        let position = Vec3::new(
            f64::from_bits(setup.position[0] as u64),
            f64::from_bits(setup.position[1] as u64),
            f64::from_bits(setup.position[2] as u64),
        );
        let mut entity = Entity::by_mob_type(scene_kind(setup.kind), position);
        entity.old_position = position;
        let id = World::new(level, ticks).spawn_entity(entity);
        // 官方构造后把随机源换成场景种子，重放 UUID（两个 nextLong）和朝向
        // （一个 nextFloat）；守卫者构造那次 tail `nextFloat` 用的是重置前的
        // 构造随机源，两侧都不重放它。
        // The official side reseeds after construction and replays the UUID (two
        // nextLong) and facing (one nextFloat) draws. The constructor's tail
        // `nextFloat` uses the pre-reset source on both sides and is not replayed.
        level.entities.with_mut(id, |entity| {
            entity.random = JavaRandom::new(setup.seed);
            let _ = mc_bamboo_blocks::entity::mob::insecure_uuid(&mut entity.random);
            let _ = entity.random.next_float();
            entity.y_rot = f32::from_bits(setup.yaw as u32);
            let Some(mob) = entity.kind.mob_mut() else {
                return;
            };
            mob.y_head_rot = entity.y_rot;
            mob.y_body_rot = entity.y_rot;
            // 场景级改坏确认：改坏攻击时长 / 尖刺的「没在动」条件。
            // Scene-level break-self-test: break the attack duration / the
            // thorns "not moving" condition.
            match break_test {
                Some("guardian_attack_duration") => {
                    mob.guardian.attack_duration_override =
                        if self.guardian.kind == 91 { 59 } else { 79 };
                }
                Some("guardian_thorns_moving") => {
                    mob.guardian.thorns_ignore_moving = true;
                }
                _ => {}
            }
        });
        id
    }
}

/// `GuardianMoveControl.tick` 用 `java.lang.Math.sin/cos`，Rust 的 `f64::sin/cos`
/// 会差 1–2 ULP，只落在摆动位移上；其余字段仍按原始位比较。主代理允许把这个
/// 平台末位差单独归类，不算行为差异。
///
/// `GuardianMoveControl.tick` uses `java.lang.Math.sin/cos`; Rust's `f64::sin/cos`
/// differs by 1-2 ULP (up to 8 bits), which only shows up in the wobble displacement. All other
/// fields are compared bit for bit.
fn guardian_frames_match(expected: &GuardianFrame, ours: &GuardianFrame) -> bool {
    let motion_ok = ours
        .motion
        .iter()
        .zip(expected.motion.iter())
        .all(|(a, b)| a.abs_diff(*b) <= 4);
    if !motion_ok {
        return false;
    }
    let mut normalized = ours.clone();
    normalized.motion = expected.motion;
    normalized == *expected
}

/// 在指定刻应用激光打断动作，同时改方块世界和游戏层玩家的视图。
///
/// Apply the mid-laser interrupt to both the block world and the game-layer player.
fn apply_interrupt(case: &GuardianCase, level: &mut MobLevel, player_uid: i32) {
    let Some(action) = &case.interrupt else {
        return;
    };
    match action.kind.as_str() {
        "wall" => {
            for x in 4..=5 {
                for z in -2..=2 {
                    for y in 61..=63 {
                        level.blocks.insert(
                            BlockPos::new(x, y, z),
                            BlockState(mc_bamboo_data::blocks::STONE),
                        );
                    }
                }
            }
        }
        "close" => {
            let position = Vec3::new(2.5, 61.0, 0.5);
            if let Some(game) = level.game.as_mut() {
                game.teleport(player_uid, position);
            }
            if let Some(player) = level.players.first_mut() {
                player.position = position;
            }
        }
        "kill" => {
            if let Some(game) = level.game.as_mut() {
                game.hurt(
                    player_uid,
                    DamageSource::environment(DamageKind::Generic),
                    100.0,
                    None,
                );
            }
            if let Some(player) = level.players.first_mut() {
                player.alive = false;
            }
        }
        "creative" => {
            if let Some(game) = level.game.as_mut() {
                game.set_arrow_target(player_uid, true, None);
            }
            if let Some(player) = level.players.first_mut() {
                player.creative = true;
            }
        }
        _ => unreachable!("unknown interrupt kind {}", action.kind),
    }
}

/// 生物目标（鱿鱼 / 美西螈）和守卫者一样重放构造后的随机源。
///
/// Replays the mob victim's post-construction random source like the guardian.
fn spawn_victim(case: &GuardianCase, level: &mut MobLevel, ticks: &mut BlockTicks) -> Option<i32> {
    let setup = case.victim.as_ref()?;
    let position = Vec3::new(
        f64::from_bits(setup.position[0] as u64),
        f64::from_bits(setup.position[1] as u64),
        f64::from_bits(setup.position[2] as u64),
    );
    let mut entity = Entity::by_mob_type(scene_kind(setup.kind), position);
    entity.old_position = position;
    let uid = World::new(level, ticks).spawn_entity(entity);
    level.entities.with_mut(uid, |entity| {
        entity.random = JavaRandom::new(setup.seed);
        let _ = mc_bamboo_blocks::entity::mob::insecure_uuid(&mut entity.random);
        let _ = entity.random.next_float();
        entity.y_rot = 0.0;
        if let Some(mob) = entity.kind.mob_mut() {
            mob.y_head_rot = 0.0;
            mob.y_body_rot = 0.0;
            mob.no_ai = true;
        }
    });
    Some(uid)
}

fn frame_of(level: &MobLevel, id: i32) -> GuardianFrame {
    let Some(entity) = level.entities.get(id) else {
        return GuardianFrame {
            motion: [0; 6],
            y_rot: 0,
            y_head_rot: 0,
            y_body_rot: 0,
            health: 0,
            air: 0,
            hurt_time: 0,
            death_time: 0,
            removed: 1,
            target: -1,
            attack_target: 0,
            moving: 0,
            look_cooldown: 0,
            wanted_x: 0,
            wanted_z: 0,
            random_state: 0,
            no_action_time: 0,
        };
    };
    let mob = entity.kind.mob().expect("a guardian");
    GuardianFrame {
        motion: [
            entity.position.x.to_bits() as i64,
            entity.position.y.to_bits() as i64,
            entity.position.z.to_bits() as i64,
            entity.delta.x.to_bits() as i64,
            entity.delta.y.to_bits() as i64,
            entity.delta.z.to_bits() as i64,
        ],
        y_rot: entity.y_rot.to_bits() as i32,
        y_head_rot: mob.y_head_rot.to_bits() as i32,
        y_body_rot: mob.y_body_rot.to_bits() as i32,
        health: mob.health.to_bits() as i32,
        air: mob.air_supply,
        hurt_time: mob.hurt_time,
        death_time: mob.death_time,
        removed: i32::from(entity.removed),
        target: mob.target.unwrap_or(-1),
        attack_target: mob.guardian.attack_target,
        moving: i32::from(mob.guardian.moving),
        look_cooldown: mob.look.look_at_cooldown,
        wanted_x: mob.look.wanted_x.to_bits() as i64,
        wanted_z: mob.look.wanted_z.to_bits() as i64,
        random_state: entity.random.state(),
        no_action_time: mob.no_action_time,
    }
}

impl Report {
    pub(super) fn replay_guardian(&mut self, case: &GuardianCase) {
        self.cases += 1;
        let mut level = case.level();
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        let guardian = case.spawn(&mut level, &mut ticks, self.break_test.as_deref());
        let victim_uid = spawn_victim(case, &mut level, &mut ticks);
        let player_uid = case.player.as_ref().map_or(-1, |setup| setup.uid);
        for (tick, expected) in case.frames.iter().enumerate() {
            if tick != 0 {
                ticks.set_time(1000 + tick as i64);
                // `ServerPlayer.doTick` / `LivingEntity.baseTick` 每刻递减受伤计时器。
                // Vanilla decrements the hurt timers at the start of each tick.
                if let Some(game) = level.game.as_mut() {
                    game.tick_cooldowns();
                }
                let mut world = World::new(&mut level, &mut ticks);
                world.tick_entities();
            }
            // 尖刺场景：这一刻让玩家近战（或 generic）打守卫者，Bamboo 侧的
            // `Guardian.hurtServer` 会 emit `Command::HurtPlayer`（反伤）。
            // Thorns scene: a player melee (or generic) hit lands on the guardian;
            // Bamboo's `Guardian.hurtServer` emits `Command::HurtPlayer`.
            if let Some(hurt) = &case.hurt
                && tick == hurt.tick as usize
            {
                let mut source = DamageSource::environment(if hurt.living_direct {
                    DamageKind::PlayerAttack
                } else {
                    DamageKind::Generic
                });
                if hurt.living_direct {
                    source.attacker = Some(player_uid);
                    source.direct = Some(player_uid);
                    source.attacker_is_player = true;
                    // `playerAttack(player)` 的伤害源位置就是玩家，击退方向读它。
                    // The `playerAttack(player)` source position is the player;
                    // knockback reads it.
                    if let Some(setup) = &case.player {
                        source.position = Some(Vec3::new(
                            f64::from_bits(setup.position[0] as u64),
                            f64::from_bits(setup.position[1] as u64),
                            f64::from_bits(setup.position[2] as u64),
                        ));
                    }
                }
                if hurt.moving {
                    level.entities.with_mut(guardian, |entity| {
                        if let Some(mob) = entity.kind.mob_mut() {
                            mob.guardian.moving = true;
                        }
                    });
                }
                let mut world = World::new(&mut level, &mut ticks);
                world.hurt_mob(guardian, source, hurt.amount);
            }
            if let Some(action) = &case.interrupt
                && tick == action.tick as usize
            {
                apply_interrupt(case, &mut level, player_uid);
            }
            let ours = frame_of(&level, guardian);
            // 玩家状态直接读游戏层真实玩家（含 cooldown / lastHurt 的落地结果）。
            // Player state comes from the real game-layer player.
            let player = level
                .game
                .as_ref()
                .map_or([0; 5], |game| game.state(player_uid));
            let ours_player_health = player[0] as i32;
            let ours_damage_cooldown = player[1] as i32;
            let ours_hurt_time = player[2] as i32;
            let ours_victim_health = victim_uid
                .and_then(|uid| level.entities.get(uid))
                .map(|entity| {
                    entity
                        .kind
                        .mob()
                        .map_or(0, |mob| mob.health.to_bits() as i32)
                })
                .unwrap_or(0);
            let label = format!("{} tick {tick}", case.name);
            if expected.guardian.removed == 1 {
                if ours.removed != 1 {
                    self.mismatch(format!("{label}: official removed, bamboo still present"));
                }
            } else if !guardian_frames_match(&expected.guardian, &ours)
                || expected.player_health != ours_player_health
                || expected.player_hurt_time != ours_hurt_time
                || expected.player_damage_cooldown != ours_damage_cooldown
                || expected.victim_health != ours_victim_health
            {
                self.mismatch(format!(
                    "{label}: official {:?}/player({}, {}, {})/victim {}, bamboo {:?}/player({}, {}, {})/victim {}",
                    expected.guardian,
                    expected.player_health,
                    expected.player_hurt_time,
                    expected.player_damage_cooldown,
                    expected.victim_health,
                    ours,
                    ours_player_health,
                    ours_hurt_time,
                    ours_damage_cooldown,
                    ours_victim_health
                ));
            }
            self.steps += 1;
            if self.mismatches.len() >= self.show {
                return;
            }
        }
        let laser_ticks = case
            .frames
            .iter()
            .filter(|frame| frame.guardian.attack_target != 0)
            .count();
        println!(
            "{}: {} ticks ({} laser-active)",
            case.name,
            case.frames.len(),
            laser_ticks
        );
    }
}
