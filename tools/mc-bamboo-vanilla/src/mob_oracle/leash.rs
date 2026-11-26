//! 拴绳逐刻对拍：官方的 `Leashable.tickLeash` 和 Bamboo 的 `leash::tick_leash` 逐位比。
//!
//! 每个场景一头牛、一个玩家，可选一个栅栏 + 拴绳结。玩家逐刻走远时弹性拉扯、
//! 超距断绳；也有剪绳 / 解回 / 拆栅栏的场景。每刻比牛的位置、速度、四个朝向、
//! 随机源、绳的状态（是否拴着、牵引者种类、角动量）和掉落物数量。
//!
//! Tick-by-tick leash replay: compare vanilla `Leashable.tickLeash` with Bamboo's
//! `leash::tick_leash` bit for bit. One cow, one player, optionally a fence knot.
//! Covers elastic pull, snap, shear, untie and knot break.

use mc_bamboo_blocks::entity::{
    KindPublic,
    mob::{
        MobType,
        leash::{self, LeashData, LeashDelayed},
    },
};
use mc_bamboo_data::items;
use mc_bamboo_protocol::item::ItemStack;

use super::*;

/// 动作发生刻（各场景一致）。
const ACTION_TICK: usize = 10;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct LeashCase {
    name: String,
    first_id: i32,
    player_id: i32,
    cow_seed: i64,
    /// 牛的起点 `[x,y,z]`（f64 位）。
    cow: [i64; 3],
    /// 玩家起点 `[x,y,z]`（f64 位）。
    player_start: [i64; 3],
    /// 方块 `[x,y,z,stateId]`（地面 + 可选栅栏）。
    blocks: Vec<[i32; 4]>,
    /// 拴绳结的栅栏格（无则 null）。
    #[serde(default)]
    knot_pos: Option<[i32; 3]>,
    /// 起步牵引者：0 无 / 1 玩家 / 2 拴绳结。
    #[serde(default)]
    start_leash: i32,
    /// 每刻玩家 `[x,y,z,dx,dy,dz]`（f64 位）；第 0 刻不用。
    player_motion: Vec<[i64; 6]>,
    frames: Vec<LeashFrame>,
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(rename_all = "camelCase")]
struct LeashFrame {
    /// 牛 `[x,y,z,dx,dy,dz]`（f64 位）。
    cow: [i64; 6],
    /// 牛 `[yRot,xRot,yHeadRot,yBodyRot]`（f32 位）。
    rot: [i32; 4],
    ground: bool,
    seed: i64,
    /// `[leashed, mayLeashed, holderKind, angularMomentumBits]`。
    /// holderKind：0 无 / 1 玩家 / 2 拴绳结。
    leash: [i64; 4],
    /// 这一刻世界里的拴绳掉落物数量。
    items: i32,
    /// 拴绳结是否还在（没有结的场景恒 false）。
    #[serde(default)]
    knot_alive: bool,
    /// 正在跑的目标类名（空格分隔），开 AI 的场景逐刻比。
    #[serde(default)]
    goals: String,
    /// 导航路径 `[nextNodeIndex, x,y,z, ...]`，开 AI 的场景逐刻比。
    #[serde(default)]
    path: Option<Vec<i32>>,
}

fn v3(bits: &[i64]) -> Vec3 {
    Vec3::new(
        f64::from_bits(bits[0] as u64),
        f64::from_bits(bits[1] as u64),
        f64::from_bits(bits[2] as u64),
    )
}

fn player_view(uid: i32, position: Vec3) -> PlayerView {
    PlayerView {
        uid,
        uuid: Some(((1_u128 << 64) | 2).to_be_bytes()),
        position,
        delta: Vec3::default(),
        width: 0.6,
        height: 1.8,
        eye_height: 1.62,
        health: 20.0,
        look_direction: Vec3::view_vector(0.0, 0.0),
        gaze_disguise: false,
        alive: true,
        spectator: false,
        creative: false,
        game_mode: 0,
        mining_fatigue: None,
        sneaking: false,
        sprinting: false,
        swimming: false,
        flying: false,
        invisible: false,
        explosion_knockback_resistance: 0.0,
        armor_cover: 0.0,
        main_hand: 0,
        off_hand: 0,
        team: None,
        yaw: 0.0,
        pitch: 0.0,
        combat: Default::default(),
        effects: Default::default(),
        shoulder_left: None,
        shoulder_right: None,
    }
}

impl LeashCase {
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
                bright_outside: true,
                monsters_burn: true,
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
            containers: Default::default(),
            container_open_counts: Default::default(),
            explosion_break: 0,
            shoulder: Default::default(),
            poi: mc_bamboo_world::poi::PoiStore::new(),
            entities: Entities::with_next_id(self.first_id),
            players: vec![player_view(self.player_id, v3(&self.player_start))],
            game: None,
            inhabited: Default::default(),
            mob_teams: Default::default(),
            ignore_bed_monsters: false,
            short_hunger: false,
            wrong_was_hurt: false,
        }
    }

    /// 建牛（id = firstId），可选建拴绳结，设起步牵引状态。返回拴绳结 uid（-1 无）。
    fn setup(&self, world: &mut World<'_>, break_test: Option<&str>) -> i32 {
        let cow_pos = v3(&self.cow);
        let cow_id = world.spawn_entity(Entity::by_mob_type(MobType::Cow, cow_pos));
        world.with_entity_mut(cow_id, &mut |e| {
            e.random = JavaRandom::new(self.cow_seed);
            e.y_rot = 0.0;
            e.old_position = cow_pos;
            if let Some(mob) = e.kind.mob_mut() {
                mob.uuid = 1_u128.to_be_bytes();
                mob.x_rot = 0.0;
                mob.y_rot_o = 0.0;
                mob.x_rot_o = 0.0;
                mob.y_head_rot = 0.0;
                mob.y_body_rot = 0.0;
                // `leash_ai_*` 开 AI，逐刻比牛跟着牵引者走的导航 / 目标 / 随机源；
                // 其余场景沿用 NoAI 只比拴绳物理。和 Java 的 `setNoAi(!ai)` 一致。
                mob.no_ai = !self.name.starts_with("leash_ai_");
                if self.start_leash == 1 && break_test != Some("leash_attach") {
                    mob.leash = Some(LeashData::new(
                        self.player_id,
                        LeashDelayed::Entity(((1_u128 << 64) | 2).to_be_bytes()),
                    ));
                }
            }
        });
        let mut knot_uid = -1;
        if let Some(p) = self.knot_pos {
            let kp = BlockPos::new(p[0], p[1], p[2]);
            knot_uid = world.spawn_entity(Entity::leash_knot(kp));
            if self.start_leash == 2 && break_test != Some("leash_attach") {
                world.with_entity_mut(cow_id, &mut |e| {
                    if let Some(mob) = e.kind.mob_mut() {
                        mob.leash = Some(LeashData::new(knot_uid, LeashDelayed::Knot(kp)));
                    }
                });
            }
        }
        knot_uid
    }
}

impl Report {
    pub(super) fn replay_leash(&mut self, case: &LeashCase) {
        self.cases += 1;
        let break_test = self.break_test.clone();
        // 第 4 处改坏确认：命中 `leash_nav` 时让 `closeRangeLeashBehaviour` 不调导航，
        // 开 AI 的牛就不自己跟着牵引者走，逐刻对拍应报不一致。跑完重置，避免串场。
        mc_bamboo_blocks::entity::mob::leash::set_oracle_skip_leash_nav(
            break_test.as_deref() == Some("leash_nav"),
        );
        let mut level = case.level();
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        let knot_uid = case.setup(
            &mut World::new(&mut level, &mut ticks),
            break_test.as_deref(),
        );
        // 起步状态由 setup 直接写字段，清掉同步/生成事件，避免干扰。
        let _ = level.entities.take_mob_events();
        let _ = level.entities.take_spawn_snapshots();

        for (tick, expected) in case.frames.iter().enumerate() {
            if tick != 0 {
                let m = &case.player_motion[tick];
                // 改坏确认：玩家不走远 / 玩家速度清零。
                match break_test.as_deref() {
                    Some("leash_motion") => {
                        level.players[0].position = v3(&case.player_start);
                        level.players[0].delta = Vec3::default();
                    }
                    Some("leash_holder") => {
                        level.players[0].position = v3(&case.player_start);
                        level.players[0].delta = v3(&m[3..6]);
                    }
                    _ => {
                        level.players[0].position = v3(&m[0..3]);
                        level.players[0].delta = v3(&m[3..6]);
                    }
                }
                // 拆栅栏场景：动作刻移除栅栏方块。
                if case.name == "leash_fence_break"
                    && tick == ACTION_TICK
                    && let Some(p) = case.knot_pos
                {
                    level.blocks.remove(&BlockPos::new(p[0], p[1], p[2]));
                }
                ticks.set_time(1000 + tick as i64);
                {
                    let mut world = World::new(&mut level, &mut ticks);
                    if tick == ACTION_TICK {
                        match case.name.as_str() {
                            "leash_player_shear" => {
                                world.with_entity_mut(case.first_id, &mut |e| {
                                    if let Some(mob) = e.kind.mob_mut() {
                                        leash::drop_leash(mob);
                                    }
                                });
                                // 掉落物只比数量，不比坐标，用牛的起点即可。
                                world.spawn_at_location(
                                    v3(&case.cow),
                                    ItemStack::new(items::LEAD, 1),
                                );
                            }
                            "leash_fence_untie" => {
                                // 官方 `setLeashedTo(player)` 内部对旧 holder（结）调
                                // `notifyLeasheeRemoved`：结上没有别的生物了就消失。
                                let mut old = None;
                                world.with_entity_mut(case.first_id, &mut |e| {
                                    if let Some(mob) = e.kind.mob_mut() {
                                        old = leash::set_leashed_to(
                                            mob,
                                            case.player_id,
                                            LeashDelayed::Entity(
                                                ((1_u128 << 64) | 2).to_be_bytes(),
                                            ),
                                        );
                                    }
                                });
                                if let Some(old) = old {
                                    mc_bamboo_blocks::entity::leash_knot::notify_leashee_removed(
                                        &mut world, old,
                                    );
                                }
                            }
                            "leash_fence_break" if knot_uid >= 0 => {
                                // 和官方一致：结先消失，牛这一刻 tickLeash 看到牵引者
                                // 已 removed → dropLeash。
                                world.with_entity_mut(knot_uid, &mut |e| e.discard());
                            }
                            _ => {}
                        }
                    }
                    world.tick_entities();
                    world.drain_effects();
                }
            }
            let _ = level.entities.take_mob_events();

            let label = format!("{} tick {tick}", case.name);
            let Some(cow) = level.entities.get(case.first_id) else {
                self.mismatch(format!("{label}: cow missing in bamboo"));
                continue;
            };
            let Some(mob) = cow.kind.mob() else {
                self.mismatch(format!("{label}: cow has no mob data"));
                continue;
            };
            let ours = LeashFrame {
                cow: [
                    cow.position.x.to_bits() as i64,
                    cow.position.y.to_bits() as i64,
                    cow.position.z.to_bits() as i64,
                    cow.delta.x.to_bits() as i64,
                    cow.delta.y.to_bits() as i64,
                    cow.delta.z.to_bits() as i64,
                ],
                rot: [
                    cow.y_rot.to_bits() as i32,
                    mob.x_rot.to_bits() as i32,
                    mob.y_head_rot.to_bits() as i32,
                    mob.y_body_rot.to_bits() as i32,
                ],
                ground: cow.on_ground,
                seed: cow.random.state(),
                leash: match &mob.leash {
                    None => [0, 0, 0, 0],
                    Some(data) => [
                        i64::from(data.holder.is_some()),
                        1,
                        if data.holder.is_some() {
                            match data.attachment {
                                LeashDelayed::Entity(_) => 1,
                                LeashDelayed::Knot(_) => 2,
                            }
                        } else {
                            0
                        },
                        data.angular_momentum.to_bits() as i64,
                    ],
                },
                items: level
                    .entities
                    .ids()
                    .iter()
                    .filter(|&&id| {
                        level
                            .entities
                            .public_at(id)
                            .is_some_and(|p| matches!(p.kind, KindPublic::Item { .. }))
                    })
                    .count() as i32,
                knot_alive: knot_uid >= 0
                    && level
                        .entities
                        .public_at(knot_uid)
                        .is_some_and(|p| !p.removed),
                goals: mob.goals.running_names_of(mob).join(" "),
                path: super::shown_path(&level, &cow, mob),
            };
            if &ours != expected {
                self.mismatch(format!(
                    "{label}: official {expected:?}\n         bamboo  {ours:?}"
                ));
            }
            self.steps += 1;
        }
        // 诊断（`MOB_ORACLE_DUMP`）：官方侧牛的位置跨度、有导航路径 / 运行目标的刻数，
        // 用来确认开 AI 的场景真的测到了「牛跟着走 / 导航 / 目标」，而不是两边都静止。
        if std::env::var_os("MOB_ORACLE_DUMP").is_some() {
            let xs: Vec<f64> = case
                .frames
                .iter()
                .map(|f| f64::from_bits(f.cow[0] as u64))
                .collect();
            let (min, max) = xs
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &x| {
                    (a.min(x), b.max(x))
                });
            let paths = case.frames.iter().filter(|f| f.path.is_some()).count();
            let goals = case.frames.iter().filter(|f| !f.goals.is_empty()).count();
            eprintln!(
                "{}: official x {:.3}..{:.3} (span {:.3}), path {} ticks, goals {} ticks",
                case.name,
                min,
                max,
                max - min,
                paths,
                goals
            );
        }
        println!(
            "{}: {} ticks, knot {knot_uid}",
            case.name,
            case.frames.len()
        );
    }
}
