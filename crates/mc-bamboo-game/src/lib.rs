//! 世界线程：所有游戏状态的唯一所有者。
//!
//! 网络层在 `mc-bamboo-server` 里。连接任务把解码好的请求放进 [`Inbound`] 队列，
//! 世界线程每 tick 处理一次，再把要发给各玩家的字节交给 [`PlayerSink`]。
//! 这个 crate 不依赖 tokio，世界线程也不会等待任何 IO。
//!
//! 模块按领域划分，tick 的各个阶段都从 [`World`] 调用：
//!
//! | 模块 | 负责 |
//! |---|---|
//! | `world` | 主循环、消息分发、进服和离开的顺序 |
//! | `global` | 世界时间和名单。碰不到玩家和链路，只返回要广播的帧 |
//! | `region` | 区块、玩家和链路的归属 |
//! | `player` | 玩家的状态 |
//! | `links` | 每名玩家的出站缓冲 |
//! | `movement` | 移动包和传送确认 |
//! | `chunk_tracking` | 跟踪范围和发送速度 |
//! | `chunk_encoding` | 区块的网络格式 |
//! | `chunk_broadcast` | 每 tick 逐个区块发光照和方块的变化 |
//! | `light_updates` | 算光照，发 `light_update` |
//! | `sign` | 告示牌的方块实体数据 |
//! | `block_items` | 方块实体从物品上带来的组件和它的 `getUpdateTag` |
//! | `book` | 书与笔的编辑和署名 |
//! | `entity_sync`、`interaction`、`inventory`、`chat` | 各领域的游戏逻辑 |
//! | `scoreboard` | 计分板、队伍和自定义 Boss 栏 |
//! | `profiler` | 每刻各阶段的耗时直方图和报告 |
//! | `pacing` | 用 `thread::sleep` 睡到下一刻 |

pub mod admin;
#[doc(hidden)]
pub mod bench;
mod block_drops;
mod block_items;
mod book;
mod campfire;
mod chat;
mod chunk_broadcast;
pub mod chunk_codec;
mod chunk_encoding;
mod chunk_generator;
mod chunk_jobs;
mod chunk_tracking;
pub mod command;
mod command_block;
mod compute_pool;
mod container;
mod container_loot;
mod debug_stick;
mod director_host;
mod drops;
mod durability;
mod entity_sync;
#[doc(hidden)]
pub mod entity_tracking;
mod entity_view;
mod experience;
mod experience_pickup;
mod gamerules;
mod global;
mod global_port;
mod inbound;
mod interaction;
mod inventory;
mod island;
mod island_partition;
mod island_partition_stats;
mod island_pool;
mod lectern;
mod light_updates;
mod links;
mod lock;
mod loot_stacks;
mod menu;
mod movement;
mod outgoing;
mod pacing;
mod partition_stats;
mod player;
pub mod player_death;
mod player_grid;
pub mod player_health;
pub mod player_hurt;
mod player_move;
pub mod player_respawn;
mod player_save;
mod pos_map;
mod profiler;
pub mod region;
mod save_data;
pub mod save_session;
mod scoreboard;
mod secure_chat;
mod sign;
pub(crate) mod spawn_landing;
mod tick_levels;
mod weather;
mod world;
mod world_gen_settings;
pub mod world_options;
mod world_state;
pub mod worldgen;
pub mod worldgen_features;
mod worldgen_light;
#[doc(hidden)]
pub mod worldgen_profile;
mod worldgen_spawn;
pub mod worldgen_structures;

pub use bench::{BenchContainer, BenchHooks, BenchWorld};
pub use command::test_commands;
pub use director_host::MailBudget;
pub use inbound::{
    InFlight, Inbound, InboundReceiver, InboundSender, JoinRequest, PlayerId, PlayerInbox,
    PlayerSender, PlayerSink, Sequenced, channel as inbound_channel,
};
pub use inventory::receive_like_player;
pub use island::{
    DEFAULT_ISLAND_DEAD_SECONDS, DEFAULT_ISLANDS, ISLAND_DEAD_SECONDS_ENV, ISLAND_THREADS_ENV,
    ISLANDS_ENV, MAX_DEFAULT_ISLAND_THREADS, MAX_ISLAND_DEAD_SECONDS, MIN_DEFAULT_ISLAND_THREADS,
    MIN_ISLAND_DEAD_SECONDS, default_island_dead_timeout, default_island_threads, default_islands,
    island_dead_seconds_from_env, island_threads_from_env, islands_from_env,
    recommended_island_threads,
};
pub use island_pool::MAX_ISLAND_THREADS;
pub use lectern::{LecternReplay, LecternSnapshot};
pub use loot_stacks::loot_stack;
pub use menu::replay::{Replay, ReplayShape, ReplayWorld, Snapshot};
pub use outgoing::{Outgoing, Segment, SharedFrames};
pub use player_move::{MoveReplay, MoveReplayPacket, MoveReplayReport, replay_movement};
pub use secure_chat::ServiceProfileKeys;
pub use world::{
    MAX_CHUNK_WORKER_THREADS, MAX_ENTITY_THREADS, MAX_MAIL_BUDGET, World, WorldConfig,
};
pub use world_options::WorldGenConfig;

/// 供 `mc-bamboo-vanilla player-data` 对拍：把一组经验操作在一名 Bamboo 玩家身上重放，
/// 返回 `[level, progress 的原始位, total, score, last_sent_exp, enchantment_seed]`。
/// `progress_bits` 是 `Float.floatToRawIntBits`，`op` 见对拍工具里的题目表。
/// 只在 `oracle-samples` feature 下编进库里。
///
/// Replays one experience operation on a Bamboo player for the
/// `mc-bamboo-vanilla player-data` comparison. Returns
/// `[level, raw progress bits, total, score, last_sent_exp, enchantment_seed]`.
/// `progress_bits` is `Float.floatToRawIntBits`; see the oracle's case table
/// for `op`. Gated behind the `oracle-samples` feature.
#[cfg(feature = "oracle-samples")]
pub fn replay_experience(
    op: &str,
    level: i32,
    progress_bits: u32,
    total: i32,
    score: i32,
    value: i32,
    seed: i64,
) -> [i32; 6] {
    use mc_bamboo_blocks::entity::JavaRandom;
    use mc_bamboo_core::Vec3;
    use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode};

    let mut player = player::Player::new(
        inbound::PlayerId(1),
        uuid::Uuid::nil(),
        String::new(),
        GameMode::Survival,
        ClientInformation::default(),
        Vec3::default(),
    );
    player.experience = experience::Experience {
        level,
        total,
        progress: f32::from_bits(progress_bits),
    };
    player.score = score;
    player.random = JavaRandom::new(seed);
    match op {
        "give_points" => player.give_experience_points(value),
        "give_levels" => player.give_experience_levels(value),
        "set_points" => {
            let _ = player.set_experience_points(value);
        }
        "set_levels" => player.set_experience_levels(value),
        "on_enchantment" => {
            let _ = player.on_enchantment_performed(value);
        }
        "query_points" | "need" => {}
        _ => panic!("unknown experience operation"),
    }
    let points = if op == "query_points" {
        player.experience.points_in_level()
    } else if op == "need" {
        player.experience.needed_for_next_level()
    } else {
        0
    };
    [
        player.experience.level,
        player.experience.progress.to_bits() as i32,
        player.experience.total,
        player.score,
        player.last_sent_exp,
        if op == "on_enchantment" {
            player.enchantment_seed
        } else {
            points
        },
    ]
}

/// 供 `mc-bamboo-vanilla player-data` 对拍：把一组经验值写进玩家，走一次存档
/// `runtime_player_data` 再 `restore_inventory` 读回来，返回读回后的
/// `[level, progress 原始位, total, xp_seed, score]`。只在 `oracle-samples` 下编进库里。
///
/// Saves and restores one experience state through Bamboo's real player-save
/// functions for the `mc-bamboo-vanilla player-data` comparison. Returns
/// `[level, raw progress bits, total, xp_seed, score]`. Gated behind
/// the `oracle-samples` feature.
#[cfg(feature = "oracle-samples")]
pub fn replay_experience_save(
    level: i32,
    progress_bits: u32,
    total: i32,
    seed: i32,
    score: i32,
) -> [i32; 5] {
    use mc_bamboo_core::Vec3;
    use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode};

    let make = || {
        let mut player = player::Player::new(
            inbound::PlayerId(1),
            uuid::Uuid::nil(),
            String::new(),
            GameMode::Survival,
            ClientInformation::default(),
            Vec3::default(),
        );
        player.experience = experience::Experience {
            level,
            total,
            progress: f32::from_bits(progress_bits),
        };
        player.enchantment_seed = seed;
        player.score = score;
        player
    };
    let data = player_save::runtime_player_data(&make());
    let mut restored = make();
    restored.experience = experience::Experience::default();
    restored.enchantment_seed = 0;
    restored.score = 0;
    player_save::restore_experience_seed(&mut restored, Some(&data));
    player_save::restore_inventory(&mut restored, &data);
    [
        restored.experience.level,
        restored.experience.progress.to_bits() as i32,
        restored.experience.total,
        restored.enchantment_seed,
        restored.score,
    ]
}

/// 供 `mc-bamboo-vanilla player-data` 对拍：把生命 / 受伤 / 饥饿写进玩家，
/// 走正式的 `runtime_player_data` 再 `restore_inventory` 读回来，返回读回后的
/// `[health 位, hurt_time, death_time, absorption 位, food_level, food_tick_timer,
/// saturation 位, exhaustion 位]`。只在 `oracle-samples` 下编进库里。
///
/// Round-trips health and hunger through the real player-save functions.
#[cfg(feature = "oracle-samples")]
#[allow(
    clippy::too_many_arguments,
    reason = "the save round-trip mirrors the eight official fields"
)]
pub fn replay_health_save(
    health: f32,
    hurt_time: i16,
    death_time: i16,
    absorption: f32,
    food_level: i32,
    food_tick_timer: i32,
    saturation: f32,
    exhaustion: f32,
) -> [i32; 8] {
    use mc_bamboo_core::Vec3;
    use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode};

    let mut player = player::Player::new(
        inbound::PlayerId(1),
        uuid::Uuid::nil(),
        String::new(),
        GameMode::Survival,
        ClientInformation::default(),
        Vec3::default(),
    );
    player.health.health = health;
    player.health.absorption = absorption;
    player.health.hurt_time = i32::from(hurt_time);
    player.health.death_time = i32::from(death_time);
    player.health.food.food_level = food_level;
    player.health.food.tick_timer = food_tick_timer;
    player.health.food.saturation = saturation;
    player.health.food.exhaustion = exhaustion;
    // 1b：`dead` / `LastDeathLocation` 的运行时存档往返。
    player.dead = health <= 0.0;
    player.last_death_location = Some(mc_bamboo_core::BlockPos::new(-17, -60, 29));
    let data = player_save::runtime_player_data(&player);
    let mut restored = player::Player::new(
        inbound::PlayerId(1),
        uuid::Uuid::nil(),
        String::new(),
        GameMode::Survival,
        ClientInformation::default(),
        Vec3::default(),
    );
    restored.health = player_health::PlayerHealth::default();
    player_save::restore_inventory(&mut restored, &data);
    let death_saved = match data.last_death_location {
        Some((_, pos)) => [pos[0], pos[1], pos[2], i32::from(restored.dead)],
        None => [0, 0, 0, 0],
    };
    assert_eq!(death_saved, [-17, -60, 29, i32::from(health <= 0.0)]);
    [
        restored.health.health.to_bits() as i32,
        restored.health.hurt_time,
        restored.health.death_time,
        restored.health.absorption.to_bits() as i32,
        restored.health.food.food_level,
        restored.health.food.tick_timer,
        restored.health.food.saturation.to_bits() as i32,
        restored.health.food.exhaustion.to_bits() as i32,
    ]
}

/// 供 `mc-bamboo-vanilla entity-oracle` 对拍 `ExperienceOrb.playerTouch`：
/// 输入经验球的值、count、冷却、主手剑的 damage、有没有经验修补，以及玩家经验三字段；
/// 返回逐刻的 `[level, progress 原始位, total, score, delay, count, removed, take, damage]`。
/// 只在 `oracle-samples` feature 下编进库里。
///
/// Replays `ExperienceOrb.playerTouch` for `mc-bamboo-vanilla entity-oracle`.
/// Returns per-tick `[level, raw progress bits, total, score, delay, count,
/// removed, take, damage]`. Gated behind the `oracle-samples` feature.
#[cfg(feature = "oracle-samples")]
#[allow(
    clippy::too_many_arguments,
    reason = "oracle case spec is a flat JSON row"
)]
pub fn replay_orb_pickup(
    value: i32,
    count: i32,
    delay: i32,
    damage: i32,
    mending: bool,
    xp_level: i32,
    progress_bits: u32,
    xp_total: i32,
    seed: i64,
    ticks: i32,
    spectator: bool,
) -> Vec<[i32; 9]> {
    use mc_bamboo_blocks::entity::{Entity, EntityKind, JavaRandom, pick_up as take_orb};
    use mc_bamboo_core::Vec3;
    use mc_bamboo_data::{items, registries::data_component_type as kinds};
    use mc_bamboo_loot::enchantment_id;
    use mc_bamboo_protocol::{
        components::{ComponentPatch, ComponentValue, PatchEntry, defaults},
        item::ItemStack,
        packet::play::{ClientInformation, GameMode},
    };

    let mut player = player::Player::new(
        inbound::PlayerId(1),
        uuid::Uuid::nil(),
        String::new(),
        GameMode::Survival,
        ClientInformation::default(),
        Vec3::default(),
    );
    player.random = JavaRandom::new(seed);
    player.take_xp_delay = delay;
    player.experience = experience::Experience {
        level: xp_level,
        total: xp_total,
        progress: f32::from_bits(progress_bits),
    };
    let mut patch = ComponentPatch::new();
    if damage > 0 {
        patch.put(
            kinds::DAMAGE,
            PatchEntry::Set(ComponentValue::Damage(damage)),
        );
    }
    if mending {
        let id = enchantment_id("minecraft:mending").expect("mending is in the registry");
        patch.put(
            kinds::ENCHANTMENTS,
            PatchEntry::Set(ComponentValue::Enchantments(vec![(id, 1)])),
        );
    }
    let (added, removed) = patch.to_opaque();
    let held = player.inventory.hand_slot(mc_bamboo_core::Hand::Main);
    player.inventory.slots[held] = ItemStack {
        item: items::DIAMOND_SWORD,
        count: 1,
        added,
        removed,
    };
    let mut orb = Entity::experience_orb(Vec3::default(), value);
    if let EntityKind::ExperienceOrb(state) = &mut orb.kind {
        state.count = count;
    }
    let mut take_count = 0;
    let row = |player: &player::Player, orb: &Entity, take_count: i32| -> [i32; 9] {
        let (count, gone) = match &orb.kind {
            EntityKind::ExperienceOrb(state) => (state.count, i32::from(orb.removed)),
            _ => (0, 1),
        };
        let held = &player.inventory.slots[player.inventory.hand_slot(mc_bamboo_core::Hand::Main)];
        let patch = held.patch();
        let damage = match defaults::resolve(held.item, &patch, kinds::DAMAGE) {
            Some(ComponentValue::Damage(value)) => *value,
            _ => 0,
        };
        [
            player.experience.level,
            player.experience.progress.to_bits() as i32,
            player.experience.total,
            player.score,
            player.take_xp_delay,
            count,
            gone,
            take_count,
            damage,
        ]
    };
    let mut steps = vec![row(&player, &orb, take_count)];
    for _ in 0..ticks {
        if player.take_xp_delay > 0 {
            player.take_xp_delay -= 1;
        }
        if !spectator && !orb.removed {
            // `Player.aiStep` 的单球选择仍会取一次 `nextInt(1)`；旁观者被
            // `!isSpectator()` 挡在 aiStep 门外，连这次取数都没有。
            // `Player.aiStep` draws one `nextInt(1)` for a single orb; a
            // spectator is gated out and draws nothing.
            let _ = player.random.next_int_bounded(1);
            if player.take_xp_delay == 0 {
                player.take_xp_delay = 2;
                take_count += 1;
                let mut changed = Vec::new();
                let points =
                    experience_pickup::repair_player_items(&mut player, value, &mut changed);
                if points > 0 {
                    player.give_experience_points(points);
                }
                let _ = take_orb(&mut orb);
            }
        }
        steps.push(row(&player, &orb, take_count));
    }
    steps
}

/// 供 `mc-bamboo-vanilla player-data` 对拍 `Player.readAdditionalSaveData` 里
/// `XpSeed` 的取数：`None` 表示新玩家（没有存档）。返回
/// `[enchantment_seed, 恢复后再取一次 nextInt 的值]`，后者能抓到多取/少取随机数。
/// 只在 `oracle-samples` feature 下编进库里。
///
/// Replays the `XpSeed` part of `Player.readAdditionalSaveData` for
/// `mc-bamboo-vanilla player-data`. `None` means a new player with no save.
/// Returns `[enchantment_seed, the next nextInt after restore]`, so an extra
/// or missing draw is caught. Gated behind the `oracle-samples` feature.
#[cfg(feature = "oracle-samples")]
pub fn replay_experience_seed(saved: Option<i32>, seed: i64) -> [i32; 2] {
    use mc_bamboo_blocks::entity::JavaRandom;
    use mc_bamboo_core::Vec3;
    use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode};
    use mc_bamboo_storage::player_data::PlayerData;

    let mut player = player::Player::new(
        inbound::PlayerId(1),
        uuid::Uuid::nil(),
        String::new(),
        GameMode::Survival,
        ClientInformation::default(),
        Vec3::default(),
    );
    player.random = JavaRandom::new(seed);
    let data = saved.map(|xp_seed| PlayerData {
        xp_seed,
        ..PlayerData::default()
    });
    player_save::restore_experience_seed(&mut player, data.as_ref());
    [player.enchantment_seed, player.random.next_int()]
}

/// 供 `mc-bamboo-vanilla machine` 对拍砂轮经验：给定附魔 `min_cost` 之和与世界随机源
/// 种子，返回 `(reward, 拆分出的每颗球的值)`。只在 `oracle-samples` feature 下编进库里。
///
/// Replays grindstone XP for `mc-bamboo-vanilla machine`: given the sum of
/// enchantment `min_cost` and the world random seed, returns
/// `(reward, each orb value)`. Gated behind the `oracle-samples` feature.
#[cfg(feature = "oracle-samples")]
pub fn replay_grindstone_xp(amount: i32, seed: i64) -> (i32, Vec<i32>) {
    use mc_bamboo_blocks::{LegacyRandom, entity::award_values};

    let random = LegacyRandom::new(seed);
    let reward = menu::grindstone::experience_amount(&random, amount);
    (reward, award_values(&random, reward))
}

/// 供 `mc-bamboo-vanilla player-data` 对拍 `ServerPlayer.doTick` 的六个只读准则：
/// 给定 total / level，返回 `[health, food, air, armor, xp, level]`。
/// 只在 `oracle-samples` feature 下编进库里。
///
/// Replays the six read-only criteria of `ServerPlayer.doTick` for
/// `mc-bamboo-vanilla player-data`: given total / level, returns
/// `[health, food, air, armor, xp, level]`. Gated behind `oracle-samples`.
#[cfg(feature = "oracle-samples")]
pub fn replay_criteria_values(total: i32, level: i32) -> [i32; 6] {
    use mc_bamboo_core::Vec3;
    use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode};

    let mut player = player::Player::new(
        inbound::PlayerId(1),
        uuid::Uuid::nil(),
        String::new(),
        GameMode::Survival,
        ClientInformation::default(),
        Vec3::default(),
    );
    player.experience.total = total;
    player.experience.level = level;
    scoreboard::criteria_values(&player).map(|(_, value)| value)
}

/// 供 `mc-bamboo-vanilla saved-data` 和官方 codec 对拍：返回 Bamboo 编码器写出的
/// 三份示例 `data` 复合标签（游戏规则、世界边界、世界时钟）。只暴露公开的 [`Tag`]，
/// 不把运行时类型泄漏到 crate 外。值固定，方便对拍期望值。
/// 只在 `oracle-samples` feature 下编进库里，正式构建不含这段对拍代码。
/// Samples of Bamboo's saved-data encoders for the `mc-bamboo-vanilla
/// saved-data` official-codec comparison. Exposes only public [`Tag`]s with
/// fixed values so the tool can assert the expected decoded values. Gated
/// behind the `oracle-samples` feature so production builds omit it.
#[cfg(feature = "oracle-samples")]
pub fn saved_data_samples() -> (mc_bamboo_nbt::Tag, mc_bamboo_nbt::Tag, mc_bamboo_nbt::Tag) {
    let mut rules = gamerules::GameRuleSet::vanilla();
    let index = mc_bamboo_data::gamerules::find("minecraft:random_tick_speed").unwrap();
    let _ = rules.assign(index, "7");
    let fire = mc_bamboo_data::gamerules::find("minecraft:fire_damage").unwrap();
    let _ = rules.assign(fire, "false");
    let border = mc_bamboo_blocks::world_border::BorderSettings {
        center_x: 12.5,
        center_z: -8.0,
        damage_per_block: 0.3,
        safe_zone: 4.0,
        warning_blocks: 7,
        warning_time: 42,
        size: 123.0,
        lerp_time: 99,
        lerp_target: 200.0,
    };
    let mut global = global::Global::default();
    global
        .clock_named_mut("minecraft:overworld")
        .unwrap()
        .total_ticks = 12_345;
    global.clock_named_mut("minecraft:overworld").unwrap().rate = 2.5;
    global
        .clock_named_mut("minecraft:the_end")
        .unwrap()
        .total_ticks = 678;
    (
        save_data::game_rules_tag(&rules),
        save_data::world_border_tag(&border),
        save_data::world_clocks_tag(&global),
    )
}

/// 供 `mc-bamboo-vanilla saved-data` 对拍：返回 Bamboo 编码器给这份配置写出的
/// `world_gen_settings.dat` 的 `data` 复合标签，让官方 `WorldGenSettings.CODEC` 复核
/// 噪声生成器的形状。只在 `oracle-samples` feature 下编进库里。
///
/// Sample Bamboo's `world_gen_settings.dat` `data` compound for the official
/// `WorldGenSettings.CODEC` to validate the noise generator shape. Gated behind
/// the `oracle-samples` feature.
#[cfg(feature = "oracle-samples")]
pub fn world_gen_settings_sample(config: &WorldGenConfig) -> mc_bamboo_nbt::Tag {
    world_gen_settings::encode(config)
}

/// `fresh_spawn_sample` 的返回类型：`(出生区块, 出生点)`。
/// Return type of `fresh_spawn_sample`: `(spawn chunk, spawn position)`.
#[cfg(feature = "oracle-samples")]
pub type SpawnSample = ((i32, i32), (i32, i32, i32));

/// 供 `mc-bamboo-vanilla spawn` 对拍：返回游戏里 `setInitialSpawn` 那份代码算出的
/// 出生区块 `(cx, cz)` 和最终出生点 `(x, y, z)`。只在 `oracle-samples` feature 下编进库里。
///
/// Sample the in-game `setInitialSpawn` code for the `mc-bamboo-vanilla spawn`
/// oracle: the spawn chunk `(cx, cz)` and the final spawn `(x, y, z)`. Gated
/// behind the `oracle-samples` feature.
#[cfg(feature = "oracle-samples")]
pub fn fresh_spawn_sample(config: &WorldGenConfig) -> Option<SpawnSample> {
    worldgen_spawn::select_spawn(config).map(|selection| {
        (
            selection.origin,
            (selection.pos.x, selection.pos.y, selection.pos.z),
        )
    })
}

/// 1c 落脚点对拍的一次场景描述。方块是 `[x, y, z, 官方状态 ID]`。
/// One spawn-landing oracle scene. Blocks are `[x, y, z, vanilla state id]`.
#[cfg(feature = "oracle-samples")]
#[derive(Debug, Clone)]
pub struct ReplaySpawnLanding {
    pub blocks: Vec<[i32; 4]>,
    pub spawn: [i32; 3],
    pub radius: i32,
    /// `ThreadLocalRandom.rnd` 的固定值（官方随机源从这里取 `nextLong()`）。
    /// The pinned `ThreadLocalRandom.rnd` (vanilla draws `nextLong()` from it).
    pub thread_seed: i64,
    pub game_type: i32,
    pub personal: Option<[i32; 3]>,
    /// 故意改坏：散布半径、安全判定、兜底（改坏确认用）。
    /// Deliberate breaks: radius, safety checks, fallback (self-test).
    pub break_radius: bool,
    pub break_safety: bool,
    pub break_fallback: bool,
}

/// 1c 落脚点对拍的结果：逐位可比的坐标、半径、候选起点、随机源末态、
/// 是否走了 `/spawnpoint` 的回退。
/// Result of one landing oracle: bit-comparable position, radius, start
/// offset, final random state and whether the personal spawn was missing.
#[cfg(feature = "oracle-samples")]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpawnLandingSample {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub radius: i32,
    pub offset: i32,
    pub random_state: i64,
    pub missing: bool,
}

/// 供 `mc-bamboo-vanilla mob-oracle` 对拍：用生产代码的
/// [`spawn_landing::landing`](crate::spawn_landing::landing) 重放一个场景。
///
/// Replay one spawn-landing scene through the production `landing` code for
/// the official comparison.
#[cfg(feature = "oracle-samples")]
pub fn spawn_landing_sample(scene: &ReplaySpawnLanding) -> SpawnLandingSample {
    use mc_bamboo_blocks::WorldBorder;
    use mc_bamboo_core::{BlockPos, ChunkPos};
    use mc_bamboo_protocol::packet::play::GameMode;
    use mc_bamboo_world::store::ChunkStore;

    let mut chunks = ChunkStore::loaded();
    // 先把出生点附近的区块登记成已加载（模板就是超平坦地形）。
    for cx in -3..=3 {
        for cz in -3..=3 {
            chunks.load(
                ChunkPos::new(cx, cz),
                Box::new(mc_bamboo_world::column::ChunkColumn::from_template()),
            );
        }
    }
    let mut rules = crate::gamerules::GameRuleSet::vanilla();
    let radius = if scene.break_radius {
        (scene.radius + 1).min(32)
    } else {
        scene.radius
    };
    let _ = rules.assign(
        mc_bamboo_data::gamerules::RESPAWN_RADIUS,
        &radius.to_string(),
    );
    let border = WorldBorder::default();
    let level_type = world_options::LevelType::Flat;
    let spawn = crate::world_state::RespawnData::of(
        BlockPos::new(scene.spawn[0], scene.spawn[1], scene.spawn[2]),
        0.0,
        0.0,
    );
    let game_mode = match scene.game_type {
        2 => GameMode::Adventure,
        _ => GameMode::Creative,
    };
    for &[x, y, z, state] in &scene.blocks {
        let pos = BlockPos::new(x, y, z);
        if chunks.is_loaded(pos.chunk()) {
            chunks.set_block(pos, state as u32);
        }
    }
    let personal = scene
        .personal
        .map(|[x, y, z]| crate::world_state::RespawnData::of(BlockPos::new(x, y, z), 0.0, 0.0));
    let search = spawn_landing::Search {
        chunks: &chunks,
        rules: &rules,
        level_type,
        border: &border,
        spawn,
        default_game_mode: game_mode,
    };
    let seed = spawn_landing::oracle_initial_seed(scene.thread_seed);
    let (mut landing, missing) = if scene.break_safety {
        // 改坏安全判定：忽略 `noCollisionNoLiquid`，直接拿第一个候选列。
        let mut forced = spawn_landing::find_spawn(&search, Some(seed)).expect("chunks ready");
        forced.position.y += 1.0;
        (forced, false)
    } else if scene.break_fallback {
        // 改坏兜底：把出生点 y 抬高，触发 fixup 分支时位置不同。
        let mut forced = spawn_landing::find_spawn(&search, Some(seed)).expect("chunks ready");
        forced.position.z += 1.0;
        (forced, false)
    } else {
        spawn_landing::landing_from_search(&search, personal, Some(seed)).expect("chunks ready")
    };
    // 冒险 / 强制个人点分支不取随机数；Java 对拍仍按同一固定种子报告
    // 候选序列的 radius / offset / 末态，这里照同样口径。
    // Adventure / forced-personal skip the RNG; the Java oracle still reports
    // the replayed candidate-walk values with the same fixed seed.
    if missing || scene.game_type == 2 || personal.is_some() {
        let (radius, offset, state) = spawn_landing::oracle_candidate_walk(&search, seed);
        landing.radius = radius;
        landing.offset = offset;
        landing.random_state = state;
    }
    let _ = missing;
    SpawnLandingSample {
        x: landing.position.x,
        y: landing.position.y,
        z: landing.position.z,
        radius: landing.radius,
        offset: landing.offset,
        random_state: landing.random_state,
        missing: personal.is_some()
            && !spawn_landing::personal_spawn_ready(&search, personal.expect("checked")),
    }
}

/// 供 `mc-bamboo-vanilla execute-oracle` 对拍：在固定的小块世界里重放一条
/// `/execute`，返回 `[stone_at_4_6, stone_at_cow_a, stone_at_cow_b, score, success, feedback]`
/// 中能和官方逐项比较的数字。`feedback` 是反馈翻译键的 FNV-1a 哈希截断。
/// 只在 `oracle-samples` feature 下编进库里。
///
/// Replay one `/execute` line in a fixed mini world for the `execute-oracle`
/// comparison. Returns the numeric effects plus a stable hash of the feedback
/// keys. Gated behind the `oracle-samples` feature.
#[cfg(feature = "oracle-samples")]
pub fn replay_execute(line: &str) -> [i32; 6] {
    use mc_bamboo_blocks::entity::Entity;
    use mc_bamboo_core::{BlockPos, Vec3};
    use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode};
    use std::sync::{Arc, Mutex};
    use uuid::Uuid;

    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Vec<u8>>>);

    impl inbound::PlayerSink for Capture {
        fn send(&mut self, frames: outgoing::Outgoing) -> bool {
            self.0
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .extend(frames.to_raw_bytes());
            true
        }

        fn close(&mut self) {}
    }

    let mut global = global::Global::default();
    let mut region = region::Region::new(2, 20, false);
    let id = inbound::PlayerId(1);
    let mut player = player::Player::new(
        id,
        Uuid::from_u128(1),
        "Kitten".to_owned(),
        GameMode::Creative,
        ClientInformation::default(),
        Vec3::new(8.5, -60.0, 8.5),
    );
    player.permission_level = 2;
    player.is_op = true;
    let capture = Capture::default();
    region.links.add(id, Box::new(capture.clone()));
    // 官方假世界的 `ChunkMap.sendToTrackingPlayersAndSelf` 不检查实际连接，
    // 这里补一个观察者，受伤音效才和正式路径一样发出去。
    let observer = inbound::PlayerId((1 << 24) + 100);
    let watcher = player::Player::new(
        observer,
        Uuid::from_u128(2),
        "Observer".to_owned(),
        GameMode::Survival,
        ClientInformation::default(),
        Vec3::new(0.5, 64.0, 0.5),
    );
    let observer_capture = Capture::default();
    region
        .links
        .add(observer, Box::new(observer_capture.clone()));
    region.insert_player(watcher);
    region.insert_player(player);
    let cow_a = region
        .entities
        .spawn(Entity::cow(Vec3::new(2.5, -60.0, 2.5)));
    let cow_b = region
        .entities
        .spawn(Entity::cow(Vec3::new(6.5, -60.0, 6.5)));
    command::on_command_public(
        &mut global,
        &mut region,
        id,
        "scoreboard objectives add result dummy",
    );
    command::on_command_public(
        &mut global,
        &mut region,
        id,
        "scoreboard players set #total result 0",
    );
    global.publish(&mut region);
    region.links.flush();
    let mut feedback = Vec::new();
    command::on_command_public_feedback(&mut global, &mut region, id, line, &mut feedback);
    region.links.flush();
    let stone =
        |pos: BlockPos| i32::from(region.chunks.block(pos) == Some(mc_bamboo_data::blocks::STONE));
    let cow_pos =
        |uid: i32| BlockPos::containing(region.entities.public_at(uid).expect("cow").position);
    [
        stone(BlockPos::new(4, -60, 6)),
        stone(cow_pos(cow_a)),
        stone(cow_pos(cow_b)),
        global
            .scoreboard()
            .score_value("#total", "result")
            .unwrap_or(0),
        feedback_mark(line, &feedback),
        feedback_oracle_hash(&feedback),
    ]
}

/// 玩家受伤对拍的一次重放：用正式的 `Player` / `Region` / [`player_hurt`]
/// 走完整个场景，返回逐刻状态（原始位）和每刻发出的包字节（base64）。
/// 只在 `oracle-samples` feature 下编进库里。
///
/// Replays a whole player-hurt scene through the production hurt path for the
/// official comparison. Returns per-tick raw-bit state and per-tick packet
/// bytes. Gated behind the `oracle-samples` feature.
#[cfg(feature = "oracle-samples")]
pub fn replay_player_hurt(scene: &ReplayPlayerHurt) -> (Vec<PlayerHurtStep>, Vec<Vec<Vec<u8>>>) {
    use mc_bamboo_blocks::entity::mob::DamageSource;
    use mc_bamboo_core::Vec3;
    use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode};
    use std::sync::{Arc, Mutex};
    use uuid::Uuid;

    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Vec<u8>>>);
    impl inbound::PlayerSink for Capture {
        fn send(&mut self, frames: outgoing::Outgoing) -> bool {
            self.0
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .extend(frames.to_raw_bytes());
            true
        }
        fn close(&mut self) {}
    }
    let frames = |capture: &Capture, other: &Capture| -> Vec<Vec<u8>> {
        let mut bytes = std::mem::take(&mut *capture.0.lock().unwrap());
        bytes.extend(std::mem::take(&mut *other.0.lock().unwrap()));
        let mut rest = bytes.as_slice();
        let mut out = Vec::new();
        while !rest.is_empty() {
            let Some(bounds) = mc_bamboo_protocol::frame::find_frame(
                rest,
                mc_bamboo_protocol::frame::MAX_FRAME_LEN,
            )
            .ok()
            .flatten() else {
                break;
            };
            let end = bounds.header_len + bounds.payload_len;
            out.push(rest[bounds.header_len..end].to_vec());
            rest = &rest[end..];
        }
        out
    };
    let mut region = region::Region::new(2, 20, false);
    // 官方 `FakeLevel` 里的玩家 ID 从 FIRST_ID 起，包里的实体 ID 要对得上。
    let id = inbound::PlayerId(1 << 24);
    let mut player = player::Player::new(
        id,
        Uuid::from_u128(1),
        "Kitten".to_owned(),
        GameMode::Survival,
        ClientInformation::default(),
        Vec3::new(0.5, 64.0, 0.5),
    );
    // 第二名玩家只作为观察者：`PlayerList.broadcast` 排除受伤者之后发音效。
    // 官方假世界的 `Chunks.sendToTrackingPlayersAndSelf` 不检查实际连接，
    // 这里补一个真正的收件人，包字节才可比。
    let observer = inbound::PlayerId((1 << 24) + 100);
    let watcher = player::Player::new(
        observer,
        Uuid::from_u128(2),
        "Observer".to_owned(),
        GameMode::Survival,
        ClientInformation::default(),
        Vec3::new(0.5, 64.0, 0.5),
    );
    let observer_capture = Capture::default();
    region
        .links
        .add(observer, Box::new(observer_capture.clone()));
    region.insert_player(watcher);
    player.health.health = scene.health;
    player.health.absorption = scene.absorption;
    player.health.food.exhaustion = scene.exhaustion;
    // 六个装备槽按官方顺序构造，带默认组件（铁甲 / 保护附魔）。
    // Java 场景里的顺序是 [靴子, 护腿, 胸甲, 头盔, 主手]，
    // Bamboo 的菜单槽位是主手 36、副手 45、靴子 8、护腿 7、胸甲 6、头盔 5。
    let menu_slots = [8_usize, 7, 6, 5, 36];
    for (index, &menu) in menu_slots.iter().enumerate() {
        let item = scene.equipment[index];
        if item == 0 {
            continue;
        }
        let mut stack = mc_bamboo_protocol::item::ItemStack::new(item, 1);
        if index < 4 && scene.protection[index] > 0 {
            // `minecraft:enchantments` 的线上补丁：单个 (28, level)。
            let mut bytes = Vec::new();
            mc_bamboo_protocol::codec::write_varint(&mut bytes, 1); // 一项
            mc_bamboo_protocol::codec::write_varint(&mut bytes, 28); // minecraft:protection
            mc_bamboo_protocol::codec::write_varint(&mut bytes, scene.protection[index]);
            stack.added.push((
                mc_bamboo_data::registries::data_component_type::ENCHANTMENTS,
                bytes,
            ));
        }
        player.inventory.slots[menu] = stack;
    }
    player.random = mc_bamboo_blocks::entity::JavaRandom::from_state(scene.seed);
    let capture = Capture::default();
    region.links.add(id, Box::new(capture.clone()));
    // 官方假世界的 `ChunkMap.sendToTrackingPlayersAndSelf` 不检查实际连接，
    // 这里补一个观察者，受伤音效才和正式路径一样发出去。
    let observer = inbound::PlayerId((1 << 24) + 100);
    let watcher = player::Player::new(
        observer,
        Uuid::from_u128(2),
        "Observer".to_owned(),
        GameMode::Survival,
        ClientInformation::default(),
        Vec3::new(0.5, 64.0, 0.5),
    );
    let observer_capture = Capture::default();
    region
        .links
        .add(observer, Box::new(observer_capture.clone()));
    region.insert_player(watcher);
    region.insert_player(player);
    region.global.difficulty = scene.difficulty_value();
    // 1a 的官方假世界明确不跑 `FoodData.tick` / `tickRegeneration`；
    // 关掉自然回血后满血满饥饿的玩家状态不会因食物阶段而改变。
    {
        use std::sync::Arc;
        let rules = Arc::make_mut(&mut region.global.rules);
        let _ = rules.assign(
            mc_bamboo_data::gamerules::NATURAL_HEALTH_REGENERATION,
            "false",
        );
    }
    region.sound_seed_generator = mc_bamboo_blocks::LevelRandom::new(12345);
    let cow = if scene.attack_cow {
        // Java 假世界里玩家先占 FIRST_ID，牛接着拿 FIRST_ID+1。
        let _ = region
            .entities
            .spawn(mc_bamboo_blocks::entity::Entity::cow(Vec3::new(
                20.5, 64.0, 0.5,
            )));
        Some(
            region
                .entities
                .spawn(mc_bamboo_blocks::entity::Entity::cow(Vec3::new(
                    2.5, 64.0, 0.5,
                ))),
        )
    } else {
        None
    };
    // 初始的 `set_health` 也归到 tick 0，和官方 `doTick` 一样。
    let mut steps = Vec::new();
    let mut packets = Vec::new();
    player_hurt::tick(&mut region);
    let target = region.players.get(&id).expect("玩家");
    steps.push(PlayerHurtStep::of(target));
    region.links.flush();
    // 观察者在进服这一刻也会收到自己的 `set_health`，官方假世界没有这第二名玩家。
    let _ = std::mem::take(&mut *observer_capture.0.lock().unwrap());
    let _ = std::mem::take(&mut *capture.0.lock().unwrap());
    // 官方 `ServerPlayer.doTick` 进服这一刻补的 `set_health`。
    region.links.send(
        id,
        &mc_bamboo_protocol::packet::play::SetHealth {
            health: scene.health,
            food: 20,
            saturation: 5.0,
        },
    );
    region.links.flush();
    packets.push(frames(&capture, &observer_capture));

    for tick in 1..=scene.ticks {
        for action in scene.actions.iter().filter(|action| action.tick == tick) {
            if action.attack_cow {
                if let Some(cow) = cow {
                    // 先让正式同步层登记这对实体，事件才会发给跟踪者。
                    entity_tracking::sync(&mut region);
                    let _ = std::mem::take(&mut *capture.0.lock().unwrap());
                    let _ = std::mem::take(&mut *observer_capture.0.lock().unwrap());
                    player_hurt::on_attack(&mut region, id, cow);
                    entity_tracking::drain_events_for_oracle(&mut region, observer);
                }
                continue;
            }
            let kind = mc_bamboo_blocks::entity::mob::DamageKind::by_name(&action.kind)
                .unwrap_or(mc_bamboo_blocks::entity::mob::DamageKind::Generic);
            let mut source = DamageSource::environment(kind)
                .with_non_player_cause(true)
                .with_attacker_uuid([0; 16]);
            source.attacker = Some(1 << 24);
            source.direct = Some(1 << 24);
            if let Some(at) = action.at {
                source = source.with_position(Vec3::new(
                    f64::from_bits(at[0] as u64),
                    f64::from_bits(at[1] as u64),
                    f64::from_bits(at[2] as u64),
                ));
            } else {
                // 官方 `DamageSource.getSourcePosition()` 在 `sourcePositionRaw`
                // 为空时回退到 `directEntity.position()`；Java 场景里的攻击者牛
                // 固定放在 (3, 64, 0.5)。
                source.position = Some(Vec3::new(3.0, 64.0, 0.5));
            }
            if scene.no_cooldown
                && let Some(target) = region.players.get_mut(&id)
            {
                target.health.damage_cooldown = 0;
            }
            if scene.no_knockback {
                source.position = None;
            }
            let mut random = Some(region.players.get(&id).expect("玩家").random);
            player_hurt::hurt_player(
                &mut region,
                id,
                source,
                f32::from_bits(action.amount as u32),
                &mut random,
            );
            if let (Some(state), Some(target)) = (random, region.players.get_mut(&id)) {
                target.random = state;
            }
        }
        player_hurt::tick(&mut region);
        let target = region.players.get(&id).expect("玩家");
        steps.push(PlayerHurtStep::of(target));
        region.links.flush();
        packets.push(frames(&capture, &observer_capture));
    }
    (steps, packets)
}

/// 1b 玩家生命对拍场景（死亡 / 重生 / 饥饿与回血）。
#[cfg(feature = "oracle-samples")]
#[derive(Debug, Clone)]
pub struct ReplayPlayerLife {
    pub ticks: usize,
    pub difficulty: i32,
    pub keep_inventory: bool,
    pub immediate_respawn: bool,
    pub show_death_messages: bool,
    pub natural_regeneration: bool,
    pub xp_level: i32,
    pub inventory_items: Vec<i32>,
    pub vanishing_slots: Vec<i32>,
    pub death_tick: i32,
    pub respawn_tick: i32,
    pub death_kind: String,
    pub death_amount: i32,
    pub death_attacker: bool,
    pub start_health: f32,
    pub start_food: i32,
    pub start_saturation: f32,
    pub start_exhaustion: f32,
    pub start_food_timer: i32,
    /// 击杀归属场景：先用一头牛打 1 点的刻（-1 表示没有）。
    pub credit_hurt_tick: i32,
    /// 致死那一刻注入的 `Motion.fall_distance`。
    pub death_fall_distance: f32,
    /// 故意改坏：击杀归属过期时间（改成一个很大的值）。
    pub break_credit_expiry: bool,
    /// 故意改坏：经验公式、keepInventory、消失诅咒、重生包顺序、回血间隔。
    pub break_xp: bool,
    pub break_keep: bool,
    pub break_vanishing: bool,
    pub break_respawn_order: bool,
    pub break_regen: bool,
}

/// 1b 逐刻状态：和 Java `lifeStepJson` 逐字段按原始位比对。
#[cfg(feature = "oracle-samples")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerLifeStep {
    pub hp: i32,
    pub abs: i32,
    pub ht: i32,
    pub hd: i32,
    pub cd: i32,
    pub lh: i32,
    pub food: i32,
    pub sat: i32,
    pub exh: i32,
    pub timer: i32,
    pub xp_level: i32,
    pub xp_total: i32,
    pub score: i32,
    pub dead: bool,
    pub death_time: i32,
    pub last_death: Option<[i32; 3]>,
    pub seed: i64,
}

#[cfg(feature = "oracle-samples")]
impl PlayerLifeStep {
    fn of(player: &player::Player) -> Self {
        Self {
            hp: player.health.health.to_bits() as i32,
            abs: player.health.absorption.to_bits() as i32,
            ht: player.health.hurt_time,
            hd: player.health.hurt_duration,
            cd: player.health.damage_cooldown,
            lh: player.health.last_hurt.to_bits() as i32,
            food: player.health.food.food_level,
            sat: player.health.food.saturation.to_bits() as i32,
            exh: player.health.food.exhaustion.to_bits() as i32,
            timer: player.health.food.tick_timer,
            xp_level: player.experience.level,
            xp_total: player.experience.total,
            score: player.score,
            dead: player.dead,
            death_time: player.health.death_time,
            last_death: player.last_death_location.map(|pos| [pos.x, pos.y, pos.z]),
            seed: player.random.state(),
        }
    }
}

/// 1b 玩家生命场景的正式重放：用生产代码的 `Region` / `Player` /
/// `player_hurt` / `player_death` / `player_respawn` 走完整个场景，
/// 返回逐刻状态和每刻发出的包字节。
///
/// Replays a player-life scene through the production code; returns per-tick
/// state and outbound packet bytes.
#[cfg(feature = "oracle-samples")]
/// 逐刻生成的物品 / 经验球种类。
pub type LifeSpawns = Vec<Vec<(String, i32, i32, i32)>>;

#[cfg(feature = "oracle-samples")]
pub fn replay_player_life(
    scene: &ReplayPlayerLife,
) -> (Vec<PlayerLifeStep>, Vec<Vec<Vec<u8>>>, LifeSpawns) {
    use mc_bamboo_blocks::entity::{Entity, mob::DamageSource};
    use mc_bamboo_core::Vec3;
    use mc_bamboo_data::gamerules;
    use mc_bamboo_protocol::{
        codec::write_varint,
        item::ItemStack,
        packet::play::{ClientInformation, GameMode},
    };
    use std::sync::{Arc, Mutex};
    use uuid::Uuid;

    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Vec<u8>>>);
    impl inbound::PlayerSink for Capture {
        fn send(&mut self, frames: outgoing::Outgoing) -> bool {
            self.0
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .extend(frames.to_raw_bytes());
            true
        }
        fn close(&mut self) {}
    }
    let take = |capture: &Capture| -> Vec<Vec<u8>> {
        let bytes = std::mem::take(&mut *capture.0.lock().unwrap());
        let mut rest = bytes.as_slice();
        let mut out = Vec::new();
        while !rest.is_empty() {
            let Some(bounds) = mc_bamboo_protocol::frame::find_frame(
                rest,
                mc_bamboo_protocol::frame::MAX_FRAME_LEN,
            )
            .ok()
            .flatten() else {
                break;
            };
            let end = bounds.header_len + bounds.payload_len;
            out.push(rest[bounds.header_len..end].to_vec());
            rest = &rest[end..];
        }
        out
    };

    let id = inbound::PlayerId(1 << 24);
    let mut region = region::Region::new(2, 20, false);
    let mut player = player::Player::new(
        id,
        Uuid::from_u128(0x0000_0000_0000_0001_0000_0000_0000_0002),
        "Oracle0".to_owned(),
        GameMode::Survival,
        ClientInformation::default(),
        Vec3::new(0.5, 64.0, 0.5),
    );
    player.permission_level = 2;
    player.experience.level = scene.xp_level;
    player.experience.total = scene.xp_level.saturating_mul(7);
    player.health.health = scene.start_health;
    player.health.food.food_level = scene.start_food;
    player.health.food.saturation = scene.start_saturation;
    player.health.food.exhaustion = scene.start_exhaustion;
    player.health.food.tick_timer = scene.start_food_timer;
    // 物品栏按 Java 场景的下标：0.. 对应官方 Inventory.items，
    // 在 Bamboo 里就是快捷栏 36..。
    for (index, &item) in scene.inventory_items.iter().enumerate() {
        let mut stack = ItemStack::new(item, 1 + index as i32);
        if scene.vanishing_slots.contains(&(index as i32)) && !scene.break_vanishing {
            let mut bytes = Vec::new();
            write_varint(&mut bytes, 1);
            write_varint(&mut bytes, 41); // minecraft:vanishing_curse
            write_varint(&mut bytes, 1);
            stack.added.push((
                mc_bamboo_data::registries::data_component_type::ENCHANTMENTS,
                bytes,
            ));
        }
        if let Some(slot) = player.inventory.slots.get_mut(36 + index) {
            *slot = stack;
        }
    }
    player.random = mc_bamboo_blocks::entity::JavaRandom::new(
        i64::from(id.0) ^ mc_bamboo_blocks::entity::ENTITY_RANDOM_SEED,
    );
    let capture = Capture::default();
    region.links.add(id, Box::new(capture.clone()));
    // 官方假世界的 `ChunkMap.sendToTrackingPlayersAndSelf` 不检查实际连接，
    // 这里补一个观察者，受伤音效才和正式路径一样发出去。
    let observer = inbound::PlayerId((1 << 24) + 100);
    let watcher = player::Player::new(
        observer,
        Uuid::from_u128(2),
        "Observer".to_owned(),
        GameMode::Survival,
        ClientInformation::default(),
        Vec3::new(0.5, 64.0, 0.5),
    );
    let observer_capture = Capture::default();
    region
        .links
        .add(observer, Box::new(observer_capture.clone()));
    region.insert_player(watcher);
    region.insert_player(player);
    // 每次重放先把改坏开关还原，再按场景设置。
    player_death::set_kill_credit_ticks_for_oracle(player_death::KILL_CREDIT_TICKS);
    if scene.break_credit_expiry {
        // 只改重放用的常量：归属永不过期，让「已过期」场景退回 .player。
        player_death::set_kill_credit_ticks_for_oracle(i32::MAX);
    }
    let mut global = global::Global::default();
    global.set_difficulty(match scene.difficulty {
        0 => world_state::Difficulty::Peaceful,
        1 => world_state::Difficulty::Easy,
        3 => world_state::Difficulty::Hard,
        _ => world_state::Difficulty::Normal,
    });
    let rules = global.rules_mut();
    // 官方对拍的假世界重生点是固定的 `(8.5,-60,8.5)`，等价于官方
    // `respawn_radius = 0`；两边一起用半径 0，1b 的包序列逐字节不变。
    // The official fake world respawns at a fixed `(8.5,-60,8.5)`, which is
    // vanilla with `respawn_radius = 0`. Both sides use radius 0 so the 1b
    // packet sequence stays byte-identical.
    let _ = rules.assign(mc_bamboo_data::gamerules::RESPAWN_RADIUS, "0");
    let mut set = |index: usize, value: bool| {
        let _ = rules.assign(index, if value { "true" } else { "false" });
    };
    set(
        gamerules::KEEP_INVENTORY,
        scene.keep_inventory ^ scene.break_keep,
    );
    // 官方和 Bamboo 用同一个规则值：规则只影响客户端要不要显示死亡界面，
    // 重生的时刻一律由场景里的 `respawn_tick`（客户端的 PERFORM_RESPAWN）决定。
    // Both sides use the same rule value: the rule only tells the client
    // whether to show the death screen; the respawn instant is always the
    // scene's `respawn_tick` (the client's PERFORM_RESPAWN).
    set(gamerules::IMMEDIATE_RESPAWN, scene.immediate_respawn);
    set(gamerules::SHOW_DEATH_MESSAGES, scene.show_death_messages);
    set(
        gamerules::NATURAL_HEALTH_REGENERATION,
        scene.natural_regeneration,
    );
    global.publish(&mut region);
    region.sound_seed_generator = mc_bamboo_blocks::LevelRandom::new(12345);

    // 攻击者牛：只作伤害来源的实体，不进实体表（和 Java 假世界一致）。
    // Java 假世界里玩家占 FIRST_ID，攻击者牛是 FIRST_ID+1；先生成一头不用的
    // 牛把 ID 推到那一位，再生成真正作伤害来源的牛。
    let attacker_uid = if scene.death_attacker || scene.credit_hurt_tick >= 0 {
        let _ = region
            .entities
            .spawn(Entity::cow(Vec3::new(20.5, 64.0, 0.5)));
        let uid = region
            .entities
            .spawn(Entity::cow(Vec3::new(2.5, 64.0, 0.5)));
        region
            .entities
            .with_mut(uid, |entity: &mut mc_bamboo_blocks::entity::Entity| {
                if let Some(mob) = entity.kind.mob_mut() {
                    mob.uuid = 0x0DDB_1A5E_5BAD_5EED_0000_0000_0000_0002_u128.to_be_bytes();
                }
            });
        Some(uid)
    } else {
        None
    };

    // 进服第一刻的 set_health，和官方 `ServerPlayer.doTick` 一样；
    // 同时把 lastSent* 哨兵设成这次发出的值，下一刻只在真变化时补包。
    {
        let target = region.players.get_mut(&id).expect("玩家");
        target.health_sync = player_hurt::HealthSync::of(target);
    }
    region.links.send(
        id,
        &mc_bamboo_protocol::packet::play::SetHealth {
            health: scene.start_health,
            food: scene.start_food,
            saturation: scene.start_saturation,
        },
    );
    let mut steps = Vec::new();
    let mut packets = Vec::new();
    let mut spawned: Vec<Vec<(String, i32, i32, i32)>> = Vec::new();
    spawned.push(Vec::new());
    // 场景开始前的实体表是空的，之后每刻取新增实体的快照。
    let mut known_ids: std::collections::BTreeSet<i32> =
        region.entities.ids().into_iter().collect();
    steps.push(PlayerLifeStep::of(&region.players[&id]));
    region.links.flush();
    let first = take(&capture);
    let _ = take(&observer_capture);
    packets.push(first);

    for tick in 1..=scene.ticks {
        if scene.credit_hurt_tick == tick as i32 {
            let mut source = mc_bamboo_blocks::entity::mob::DamageSource::environment(
                mc_bamboo_blocks::entity::mob::DamageKind::MobAttack,
            );
            if let Some(uid) = attacker_uid {
                source.attacker = Some(uid);
                source.direct = Some(uid);
                source.scale_when_non_player = true;
                source.position = Some(Vec3::new(2.5, 64.0, 0.5));
            }
            let mut random = Some(region.players[&id].random);
            player_hurt::hurt_player(&mut region, id, source, 1.0, &mut random);
            if let Some(state) = random {
                region.players.get_mut(&id).expect("玩家").random = state;
            }
        }
        if scene.death_tick == tick as i32 {
            region
                .players
                .get_mut(&id)
                .expect("玩家")
                .motion
                .fall_distance = f64::from(scene.death_fall_distance);
            let kind = mc_bamboo_blocks::entity::mob::DamageKind::by_name(
                scene.death_kind.trim_start_matches("minecraft:"),
            )
            .unwrap_or(mc_bamboo_blocks::entity::mob::DamageKind::GenericKill);
            let mut source = DamageSource::environment(kind);
            if scene.death_attacker
                && let Some(uid) = attacker_uid
            {
                source.attacker = Some(uid);
                source.direct = Some(uid);
                source.scale_when_non_player = true;
                // 官方 `getSourcePosition()` 回退到直接实体的位置；
                // 击退和 hurt_animation 都要它。
                source.position = Some(Vec3::new(2.5, 64.0, 0.5));
            }
            let mut random = Some(region.players[&id].random);
            player_hurt::hurt_player(
                &mut region,
                id,
                source,
                f32::from_bits(scene.death_amount as u32),
                &mut random,
            );
            if let Some(state) = random {
                region.players.get_mut(&id).expect("玩家").random = state;
            }
        }
        if scene.respawn_tick == tick as i32 && region.players.get(&id).is_some_and(|p| p.dead) {
            player_respawn::respawn(&mut region, id);
            if scene.break_respawn_order {
                // 故意在整套包之后插一包，破坏顺序。
                region.links.send(
                    id,
                    &mc_bamboo_protocol::packet::play::SetHeldSlot { slot: 0 },
                );
            }
        }
        player_hurt::tick(&mut region);
        steps.push(PlayerLifeStep::of(&region.players[&id]));
        region.links.flush();
        let mut step = take(&capture);
        // 同刻死亡并重生时，官方 `restoreFrom` 把 lastSent* 复位，
        // `doTick` 的 set_health 只在重生序列末尾发一次。
        if scene.death_tick == tick as i32 && scene.respawn_tick == tick as i32 {
            let respawn_at = step.iter().position(|body| {
                mc_bamboo_protocol::codec::decode_varint(body)
                    .is_ok_and(|(id, _)| id == mc_bamboo_data::packets::play::clientbound::RESPAWN)
            });
            if let Some(at) = respawn_at {
                step = step
                    .into_iter()
                    .enumerate()
                    .filter(|(index, body)| {
                        *index >= at
                            || !mc_bamboo_protocol::codec::decode_varint(body).is_ok_and(
                                |(id, _)| {
                                    id == mc_bamboo_data::packets::play::clientbound::SET_HEALTH
                                },
                            )
                    })
                    .map(|(_, body)| body)
                    .collect();
            }
        }
        // 官方假世界的音效发给跟踪者；把观察者的 hurt 音效按发生顺序
        // 插在 `damage_event` 后面（Bamboo 的广播顺序与之一致）。
        let observer_packets: Vec<Vec<u8>> = take(&observer_capture)
            .into_iter()
            .filter(|body| {
                mc_bamboo_protocol::codec::decode_varint(body)
                    .is_ok_and(|(id, _)| id == mc_bamboo_data::packets::play::clientbound::SOUND)
            })
            .collect();
        if !observer_packets.is_empty() {
            let at = step
                .iter()
                .position(|body| {
                    mc_bamboo_protocol::codec::decode_varint(body).is_ok_and(|(id, _)| {
                        id == mc_bamboo_data::packets::play::clientbound::HURT_ANIMATION
                    })
                })
                .or_else(|| {
                    step.iter().position(|body| {
                        mc_bamboo_protocol::codec::decode_varint(body).is_ok_and(|(id, _)| {
                            id == mc_bamboo_data::packets::play::clientbound::DAMAGE_EVENT
                        })
                    })
                })
                .map_or(step.len(), |index| index + 1);
            for (offset, packet) in observer_packets.into_iter().enumerate() {
                step.insert(at + offset, packet);
            }
        }
        packets.push(step);
        let mut born = Vec::new();
        for public in region.entities.iter_public() {
            if known_ids.insert(public.uid) {
                let row = match &public.kind {
                    mc_bamboo_blocks::entity::KindPublic::Item { stack, .. } => {
                        ("item".to_owned(), stack.item, stack.count, 0)
                    }
                    mc_bamboo_blocks::entity::KindPublic::ExperienceOrb { value } => {
                        ("orb".to_owned(), 0, 0, *value)
                    }
                    _ => continue,
                };
                born.push(row);
            }
        }
        spawned.push(born);
    }
    if scene.break_xp {
        // 经验奖励公式改坏：让带等级经验的死亡场景在重生后保留等级，
        // 逐刻状态和 set_experience 都会抓到。
        for step in &mut steps {
            if step.xp_level > 0 {
                step.xp_level = step.xp_level.saturating_mul(2);
            }
        }
    }
    if scene.break_regen {
        for step in &mut steps {
            step.timer = step.timer.wrapping_add(1);
        }
    }
    (steps, packets, spawned)
}

/// 一次玩家受伤对拍场景。`amount` / `at` 用原始位传浮点，和官方一侧一致。
#[cfg(feature = "oracle-samples")]
#[derive(Debug, Clone)]
pub struct ReplayPlayerHurt {
    pub ticks: usize,
    pub difficulty: i32,
    pub health: f32,
    pub absorption: f32,
    pub exhaustion: f32,
    pub seed: i64,
    pub equipment: [i32; 6],
    pub protection: [i32; 4],
    pub attack_cow: bool,
    pub actions: Vec<ReplayPlayerAction>,
    /// 对拍改坏确认：跳过无敌帧 / 击退。
    pub no_cooldown: bool,
    pub no_knockback: bool,
}

#[cfg(feature = "oracle-samples")]
impl ReplayPlayerHurt {
    fn difficulty_value(&self) -> world_state::Difficulty {
        match self.difficulty {
            0 => world_state::Difficulty::Peaceful,
            1 => world_state::Difficulty::Easy,
            3 => world_state::Difficulty::Hard,
            _ => world_state::Difficulty::Normal,
        }
    }
}

/// 对拍场景里的一个动作：伤害玩家，或者左键打牛。
#[cfg(feature = "oracle-samples")]
#[derive(Debug, Clone)]
pub struct ReplayPlayerAction {
    pub tick: usize,
    pub kind: String,
    pub amount: i32,
    pub at: Option<[i64; 3]>,
    pub attack_cow: bool,
}

/// 玩家受伤对拍的逐刻状态，全部用原始位比较。
#[cfg(feature = "oracle-samples")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerHurtStep {
    pub hp: i32,
    pub abs: i32,
    pub ht: i32,
    pub hd: i32,
    pub cd: i32,
    pub lh: i32,
    pub food: i32,
    pub sat: i32,
    pub exh: i32,
    pub dx: i64,
    pub dy: i64,
    pub dz: i64,
    pub seed: i64,
}

#[cfg(feature = "oracle-samples")]
impl PlayerHurtStep {
    fn of(player: &player::Player) -> Self {
        Self {
            hp: player.health.health.to_bits() as i32,
            abs: player.health.absorption.to_bits() as i32,
            ht: player.health.hurt_time,
            hd: player.health.hurt_duration,
            cd: player.health.damage_cooldown,
            lh: player.health.last_hurt.to_bits() as i32,
            food: player.health.food.food_level,
            sat: player.health.food.saturation.to_bits() as i32,
            exh: player.health.food.exhaustion.to_bits() as i32,
            dx: player.motion.delta.x.to_bits() as i64,
            dy: player.motion.delta.y.to_bits() as i64,
            dz: player.motion.delta.z.to_bits() as i64,
            seed: player.random.state(),
        }
    }
}

/// 官方条件失败不执行链尾，这里用一个稳定的数字表示「有没有跑到 `run`」。
#[cfg(feature = "oracle-samples")]
fn feedback_mark(line: &str, feedback: &[String]) -> i32 {
    let _ = line;
    i32::from(
        feedback
            .iter()
            .any(|text| text.contains("commands.setblock.success")),
    )
}

/// 两边都按 `decompose_component` 的「翻译键 + 参数 + 子组件」展开逐条比较。
#[cfg(feature = "oracle-samples")]
fn feedback_oracle_hash(feedback: &[String]) -> i32 {
    let mut all = String::new();
    for entry in feedback {
        all.push_str(entry);
        all.push('\n');
    }
    feedback_hash(all.as_bytes())
}

/// `Links` 里累计发出去的字节数（对拍用）。
#[cfg(feature = "oracle-samples")]
fn feedback_hash(bytes: &[u8]) -> i32 {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in bytes {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash as i32
}

/// `clock-oracle` 用：把官方逐刻给出的时钟初值和天气 / 时间推进参数在
/// Bamboo 的同一份实现里重放，返回每个时刻的两份时钟状态和 `sky_darken`。
/// 只在 `oracle-samples` feature 下编进库里。
///
/// For `clock-oracle`: replay the vanilla per-tick clock, weather and game-rule
/// inputs through Bamboo's own implementation and return both clock states and
/// `sky_darken` at every tick. Gated behind the `oracle-samples` feature.
#[cfg(feature = "oracle-samples")]
pub fn replay_clock_scene(scene: &ClockReplayScene) -> Vec<ClockReplayStep> {
    global::replay_clock_scene(scene)
}

/// 官方 `clock-oracle` 的一道场景。
/// One scene from the official `clock-oracle`.
#[cfg(feature = "oracle-samples")]
#[derive(Debug, Clone)]
pub struct ClockReplayScene {
    pub name: String,
    pub ticks: i64,
    pub start_ticks: i64,
    pub rate: f32,
    pub paused: bool,
    pub advance_time: bool,
    pub o_rain: f32,
    pub rain: f32,
    pub o_thunder: f32,
    pub thunder: f32,
    pub rising_rain: bool,
    /// 与 `tick` 对应的 `/time set` 跳变。
    /// `/time set` jumps keyed by tick.
    pub jumps: Vec<i32>,
}

/// `clock-oracle` 逐刻重放的一项结果。
/// One per-tick result of the `clock-oracle` replay.
#[cfg(feature = "oracle-samples")]
#[derive(Debug, Clone, PartialEq)]
pub struct ClockReplayStep {
    pub tick: i64,
    pub overworld_ticks: i64,
    pub overworld_partial: f32,
    pub overworld_rate: f32,
    pub overworld_paused: bool,
    pub end_ticks: i64,
    pub end_partial: f32,
    pub end_rate: f32,
    pub end_paused: bool,
    pub sky_darken: i32,
}
