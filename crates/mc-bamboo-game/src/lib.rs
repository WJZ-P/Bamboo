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
pub mod advancement_rewards;
pub mod advancement_triggers;
pub mod advancements;
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
mod ender_pearl;
mod entity_sync;
#[doc(hidden)]
pub mod entity_tracking;
mod entity_view;
mod experience;
mod experience_pickup;
mod gamerules;
pub mod generation_post_process;
mod global;
mod global_port;
mod inbound;
mod interaction;
mod inventory;
mod island;
mod island_partition;
mod island_partition_stats;
mod island_pool;
mod item_cooldowns;
mod item_use_stats;
mod lectern;
mod light_updates;
mod links;
mod lock;
mod loot_stacks;
mod menu;
mod merchant;
mod movement;
mod natural_spawn;
mod outgoing;
mod pacing;
mod partition_stats;
mod patrol_spawn;
mod phantom_spawn;
pub mod pig_riding;
mod player;
mod player_attack;
mod player_attributes;
mod player_auto_spin;
mod player_combat_memory;
pub mod player_consume;
pub mod player_death;
pub mod player_effects;
pub mod player_egg;
pub mod player_enchantments;
pub mod player_environment;
pub mod player_exhaustion;
mod player_grid;
pub mod player_health;
pub mod player_hurt;
mod player_move;
pub mod player_respawn;
mod player_save;
mod player_shoulder;
pub mod player_weapon;
pub mod player_wind_charge;
pub mod poi_query;
mod pos_map;
mod profiler;
pub mod region;
#[cfg(feature = "oracle-samples")]
mod replay_players;
mod riding;
#[cfg(feature = "oracle-samples")]
pub use drops::replay_arrow_pickup;
#[cfg(feature = "oracle-samples")]
pub use replay_players::{ReplayPlayerSetup, ReplayPlayers};
#[cfg(feature = "oracle-samples")]
#[doc(hidden)]
pub mod resume_replay;
mod save_data;
pub mod save_session;
mod scoreboard;
mod secure_chat;
mod sign;
mod sleep;
pub(crate) mod spawn_landing;
/// 结构刷怪覆盖（`ChunkGenerator.getMobsAt`）；对拍工具也用它。
///
/// The structure spawn override (`ChunkGenerator.getMobsAt`), also used by the oracle.
pub mod structure_spawn;
mod tick_levels;
mod weather;
mod world;
mod world_gen_settings;
pub mod world_options;
mod world_state;
pub mod worldgen;
pub mod worldgen_cache;
pub mod worldgen_compact;
pub mod worldgen_features;
pub mod worldgen_light;
mod worldgen_mobs;
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
pub use natural_spawn::spawning_chunk_candidates;
pub use outgoing::{Outgoing, Segment, SharedFrames};
pub use player_move::{MoveReplay, MoveReplayPacket, MoveReplayReport, replay_movement};
pub use secure_chat::ServiceProfileKeys;
pub use world::{
    MAX_CHUNK_WORKER_THREADS, MAX_ENTITY_COLLISIONS, MAX_ENTITY_THREADS, MAX_MAIL_BUDGET,
    MAX_MOB_DISTANCE, MAX_MOB_WAKEUPS_SCALE, MAX_PATHFINDING_BUDGET, MAX_VILLAGER_BRAIN_INTERVAL,
    World, WorldConfig,
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

/// 玩家属性运行时存盘往返对拍：修改基础值、挂三种永久修饰符和临时修饰符，
/// 走正式的 `runtime_player_data` -> gzip -> `restore_inventory`，返回读回后的
/// `(max_health_value, health_bits, has_perm_modifiers, had_transient_on_disk)`。
///
/// Runtime player-attribute save round-trip for the player-data oracle.
#[doc(hidden)]
pub fn replay_attributes_save() -> (u64, u32, bool, bool) {
    use mc_bamboo_blocks::entity::mob::{
        attributes::{Modifier, Operation},
        effects::EffectInstance,
    };
    use mc_bamboo_core::Vec3;
    use mc_bamboo_data::registries::{attribute, mob_effect};
    use mc_bamboo_protocol::packet::play::{ClientInformation, ENTITY_FLAG_SPRINTING, GameMode};
    use mc_bamboo_storage::player_data::PlayerData;

    let mut player = player::Player::new(
        inbound::PlayerId(1),
        uuid::Uuid::nil(),
        String::new(),
        GameMode::Creative,
        ClientInformation::default(),
        Vec3::default(),
    );
    if let Some(inst) = player.attributes.instance(attribute::MAX_HEALTH) {
        inst.base = 24.0;
        inst.modifiers.push(Modifier {
            id: "minecraft:oracle_add".to_owned(),
            amount: 6.0,
            operation: Operation::AddValue,
            permanent: true,
        });
    }
    if let Some(inst) = player.attributes.instance(attribute::ATTACK_DAMAGE) {
        inst.modifiers.push(Modifier {
            id: "minecraft:oracle_base".to_owned(),
            amount: 0.5,
            operation: Operation::AddMultipliedBase,
            permanent: true,
        });
        inst.modifiers.push(Modifier {
            id: "minecraft:oracle_total".to_owned(),
            amount: 0.25,
            operation: Operation::AddMultipliedTotal,
            permanent: true,
        });
    }
    player
        .effects
        .add(EffectInstance::with_amplifier(mob_effect::SPEED, 200, 0));
    player.sync.shared_flags |= ENTITY_FLAG_SPRINTING;
    player_attributes::rebuild_player(&mut player, false);
    player.health.health = 28.5;

    let data = player_save::runtime_player_data(&player);
    let gzip = data.to_gzip().expect("player attributes gzip");
    let loaded = PlayerData::from_gzip(&gzip).expect("player attributes decode");
    let serialized = format!("{:?}", loaded.attributes);
    let had_transient_on_disk = serialized.contains("minecraft:sprinting")
        || serialized.contains("minecraft:effect.speed")
        || serialized.contains("minecraft:creative_mode_block_range");

    let mut restored = player::Player::new(
        inbound::PlayerId(1),
        uuid::Uuid::nil(),
        String::new(),
        GameMode::Survival,
        ClientInformation::default(),
        Vec3::default(),
    );
    player_save::restore_inventory(&mut restored, &loaded);
    let has_perm = restored
        .attributes
        .get(attribute::MAX_HEALTH)
        .and_then(|a| a.modifier("minecraft:oracle_add"))
        .is_some_and(|m| m.permanent && m.amount == 6.0)
        && restored
            .attributes
            .get(attribute::ATTACK_DAMAGE)
            .and_then(|a| a.modifier("minecraft:oracle_base"))
            .is_some_and(|m| m.permanent && m.amount == 0.5)
        && restored
            .attributes
            .get(attribute::ATTACK_DAMAGE)
            .and_then(|a| a.modifier("minecraft:oracle_total"))
            .is_some_and(|m| m.permanent && m.amount == 0.25);
    (
        restored.attributes.value(attribute::MAX_HEALTH).to_bits(),
        restored.health.health.to_bits(),
        has_perm,
        had_transient_on_disk,
    )
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
pub fn replay_execute(line: &str) -> [i32; 7] {
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
    // 场景脚本里的 ` ;; ` 前面是准备命令（效果先给上再测 clear），
    // 它的反馈不计入比较，和 Java 的 `emit` 一致。
    let (setup, measured) = match line.split_once(" ;; ") {
        Some((setup, measured)) => (setup, measured),
        None => ("", line),
    };
    if !setup.is_empty() {
        command::on_command_public(&mut global, &mut region, id, setup);
        region.links.flush();
    }
    let mut feedback = Vec::new();
    command::on_command_public_feedback(&mut global, &mut region, id, measured, &mut feedback);
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
        feedback_mark(measured, &feedback),
        feedback_oracle_hash(&feedback),
        replay_effect_state(&region, id, &[cow_a, cow_b]),
    ]
}

/// 玩家 + 两头牛的 `activeEffects`，和 Java `ExecuteOracle.effectState` 的
/// 文本格式逐字节一致（按 tracked 的固定顺序，不用实体 ID）。
#[cfg(feature = "oracle-samples")]
fn replay_effect_state(region: &region::Region, id: inbound::PlayerId, cows: &[i32]) -> i32 {
    use mc_bamboo_blocks::entity::mob::effects::EffectMap;
    let mut all = String::new();
    let append = |all: &mut String, index: usize, effects: &EffectMap| {
        all.push_str(&format!("{index}:"));
        for instance in effects.entries() {
            all.push_str(&format!(
                "{},{},{},{},{},{};",
                instance.effect,
                instance.duration,
                instance.amplifier,
                i32::from(instance.ambient),
                i32::from(instance.visible),
                i32::from(instance.show_icon),
            ));
        }
        all.push('\n');
    };
    if let Some(player) = region.players.get(&id) {
        append(&mut all, 0, &player.effects);
    }
    for (index, &uid) in cows.iter().enumerate() {
        if let Some(entity) = region.entities.get(uid)
            && let Some(mob) = entity.kind.mob()
        {
            append(&mut all, index + 1, &mob.effects);
        }
    }
    feedback_hash(all.as_bytes())
}

/// 玩家受伤对拍的一次重放：用正式的 `Player` / `Region` / [`player_hurt`]
/// 走完整个场景，返回逐刻状态（原始位）和每刻发出的包字节（base64）。
/// 只在 `oracle-samples` feature 下编进库里。
///
/// Replays a whole player-hurt scene through the production hurt path for the
/// official comparison. Returns per-tick raw-bit state and per-tick packet
/// bytes. Gated behind the `oracle-samples` feature.
/// `replay_player_hurt` 的三组逐刻结果。
///
/// The three per-tick result groups of `replay_player_hurt`.
#[cfg(feature = "oracle-samples")]
pub struct ReplayPlayerHurtOutput {
    pub steps: Vec<PlayerHurtStep>,
    pub packets: Vec<Vec<Vec<u8>>>,
    /// 观察者一个人收到的包，按发生顺序。官方的效果包收集器也是一条单收件人
    /// 的流，攻击场景用它逐刻比较音效 / 粒子 / 动画。
    ///
    /// Packets received by the observer alone, in order. The official effect
    /// collector is a single-recipient stream too; attack scenes compare sounds
    /// / particles / animations against it.
    pub observer_packets: Vec<Vec<Vec<u8>>>,
    /// 攻击者自己收到的包，按发生顺序。
    ///
    /// Packets the attacker received, in order.
    pub attacker_packets: Vec<Vec<Vec<u8>>>,
    pub cow_states: Vec<Vec<CowAttackState>>,
}

#[cfg(feature = "oracle-samples")]
pub fn replay_player_hurt(scene: &ReplayPlayerHurt) -> ReplayPlayerHurtOutput {
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
    let frames = |capture: &Capture| -> Vec<Vec<u8>> {
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
    player.attack_strength_ticker = scene.attack_charge;
    player.motion.fall_distance = scene.attack_fall;
    player.on_ground = scene.attack_ground;
    player.last_main_hand_item = player.inventory.held(mc_bamboo_core::Hand::Main).clone();
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
    if !scene.attack_enchants.is_empty() {
        let slot = player.inventory.hand_slot(mc_bamboo_core::Hand::Main);
        player.inventory.slots[slot] = enchanted_weapon(scene.equipment[4], &scene.attack_enchants);
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
    if scene.attack_sprint {
        entity_sync::set_sprinting(&mut region, id, true);
    }
    player_attributes::rebuild(&mut region);
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
    // 第一头 dummy 只用来对齐官方假世界的实体 ID；`real_cows` 是 Java
    // `stepJson(cows)` 里真正逐刻比较的那几头。
    //
    // The first dummy cow only aligns entity ids with the official fake world;
    // `real_cows` are the ones Java reports per tick.
    let (cow, real_cows) = if scene.attack_cow {
        let _ = region
            .entities
            .spawn(mc_bamboo_blocks::entity::Entity::cow(Vec3::new(
                20.5, 64.0, 0.5,
            )));
        let mut real =
            vec![
                region
                    .entities
                    .spawn(mc_bamboo_blocks::entity::Entity::cow(Vec3::new(
                        2.5, 64.0, 0.5,
                    ))),
            ];
        for &(x, y, z) in &scene.extra_cow_positions {
            real.push(
                region
                    .entities
                    .spawn(mc_bamboo_blocks::entity::Entity::cow(Vec3::new(x, y, z))),
            );
        }
        // 官方场景给每头牛设了固定种子；Bamboo 生成时用的是自己的随机源，
        // 这里照官方种子重设，逐刻才能比较受害者随机源的状态。
        //
        // The official scene seeds every cow; Bamboo's spawn uses its own
        // random, so reset each cow to the official seed to compare the victim
        // random state per tick.
        for (index, &seed) in scene.cow_seeds.iter().enumerate() {
            if let Some(&uid) = real.get(index) {
                region.entities.with_mut(uid, |entity| {
                    entity.random = mc_bamboo_blocks::entity::JavaRandom::new(seed);
                });
            }
        }
        (real.first().copied(), real)
    } else {
        (None, Vec::new())
    };
    // 初始的 `set_health` 也归到 tick 0，和官方 `doTick` 一样。
    let mut steps = Vec::new();
    let mut packets = Vec::new();
    let mut observer_packets = Vec::new();
    let mut attacker_packets = Vec::new();
    player_hurt::tick(&mut region);
    // 官方 `causeFoodExhaustion` 当场写 FoodData；Bamboo 的攻击走 pending，
    // 对拍里在取快照前 flush，才和官方逐刻的 exhaustion 一致。
    //
    // Vanilla writes FoodData immediately; Bamboo's attack uses pending. Flush
    // before the snapshot so the per-tick exhaustion matches in the oracle.
    crate::player_exhaustion::flush(&mut region);
    let mut cow_steps = vec![cow_states(&region, &real_cows)];
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
    let attacker_only = frames(&capture);
    let observer_only = frames(&observer_capture);
    let mut merged = attacker_only.clone();
    merged.extend(observer_only.iter().cloned());
    packets.push(merged);
    attacker_packets.push(attacker_only);
    observer_packets.push(observer_only);

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
        crate::player_exhaustion::flush(&mut region);
        cow_steps.push(cow_states(&region, &real_cows));
        let target = region.players.get(&id).expect("玩家");
        steps.push(PlayerHurtStep::of(target));
        region.links.flush();
        let attacker_only = frames(&capture);
        let observer_only = frames(&observer_capture);
        let mut merged = attacker_only.clone();
        merged.extend(observer_only.iter().cloned());
        packets.push(merged);
        attacker_packets.push(attacker_only);
        observer_packets.push(observer_only);
    }
    ReplayPlayerHurtOutput {
        steps,
        packets,
        observer_packets,
        attacker_packets,
        cow_states: cow_steps,
    }
}

/// 给对拍武器挂上官方导出的附魔 ID / 等级。
///
/// Attach generated enchantment ids / levels to the oracle weapon.
#[cfg(feature = "oracle-samples")]
fn enchanted_weapon(item: i32, enchants: &[(String, i32)]) -> mc_bamboo_protocol::item::ItemStack {
    use mc_bamboo_protocol::components::{ComponentPatch, ComponentValue, PatchEntry};
    let mut stack = mc_bamboo_protocol::item::ItemStack::new(item, 1);
    let levels: Vec<(i32, i32)> = enchants
        .iter()
        .filter_map(|(name, level)| {
            mc_bamboo_data::registries::SYNCED_REGISTRIES
                .iter()
                .find(|registry| registry.name == "minecraft:enchantment")
                .and_then(|registry| registry.entries.iter().position(|entry| *entry == name))
                .map(|id| (id as i32, *level))
        })
        .collect();
    let mut patch = ComponentPatch::from_opaque(&stack).unwrap_or_default();
    patch.put(
        mc_bamboo_data::registries::data_component_type::ENCHANTMENTS,
        PatchEntry::Set(ComponentValue::Enchantments(levels)),
    );
    let (added, removed) = patch.to_opaque();
    stack.added = added;
    stack.removed = removed;
    stack
}

/// 逐刻取每头牛的状态，供攻击场景对拍。
///
/// Snapshot each cow's state per tick for the attack scenes.
#[cfg(feature = "oracle-samples")]
fn cow_states(region: &region::Region, cows: &[i32]) -> Vec<CowAttackState> {
    cows.iter()
        .map(|&uid| match region.entities.get(uid) {
            Some(entity) => CowAttackState {
                hp: entity
                    .kind
                    .mob()
                    .map_or(0, |mob| mob.health.to_bits() as i32),
                dx: entity.delta.x.to_bits() as i64,
                dy: entity.delta.y.to_bits() as i64,
                dz: entity.delta.z.to_bits() as i64,
                fire: entity.remaining_fire_ticks,
                hurt_time: entity.kind.mob().map_or(0, |mob| mob.hurt_time),
                removed: entity.removed,
                seed: entity.random.state(),
            },
            None => CowAttackState {
                removed: true,
                ..Default::default()
            },
        })
        .collect()
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
    /// 攻击场景：玩家 `attackStrengthTicker` 的初值、疾跑 / 下落状态，
    /// 主手武器上的附魔，以及横扫用的额外牛的位置。
    ///
    /// Attack scenes: the initial `attackStrengthTicker`, sprint / fall state,
    /// main-hand enchantments, and extra cows for sweeping.
    pub attack_charge: i32,
    pub attack_sprint: bool,
    pub attack_fall: f64,
    pub attack_ground: bool,
    pub attack_enchants: Vec<(String, i32)>,
    pub extra_cow_positions: Vec<(f64, f64, f64)>,
    /// 每头参战牛的随机源初始状态，按位置顺序；官方场景给每头牛设了同一种子。
    ///
    /// Initial random state of each scene cow, in position order; the official
    /// scene seeds every cow.
    pub cow_seeds: Vec<i64>,
}

/// 攻击场景里每头牛的逐刻状态，全部用原始位比较。
///
/// Per-tick state of each cow in an attack scene, all compared as raw bits.
#[cfg(feature = "oracle-samples")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CowAttackState {
    pub hp: i32,
    pub dx: i64,
    pub dy: i64,
    pub dz: i64,
    pub fire: i32,
    pub hurt_time: i32,
    pub removed: bool,
    /// 受害者自己的随机源状态：受伤音高的两次取数、节肢杀手的缓慢全走它。
    ///
    /// The victim's own random state: the two hurt-pitch draws and Bane of
    /// Arthropods' Slowness all read it.
    pub seed: i64,
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

/// `sleep-oracle` 用：从产品代码里取出睡眠判定和跳早数学的答案。
/// 只在 `oracle-samples` feature 下编进库里。
///
/// For `sleep-oracle`: read the sleep decisions and morning-skip maths from
/// the production code. Gated behind the `oracle-samples` feature.
#[cfg(feature = "oracle-samples")]
pub fn replay_sleep_facts(scene: &SleepReplayFacts) -> SleepReplayAnswer {
    let status = sleep::SleepStatus::of_counts(scene.active, scene.sleeping);
    // 和正式游戏同一份 `sky` 公式：主世界时间线 + 雨 / 雷暴层。
    let sky_darken =
        mc_bamboo_blocks::sky::sky_darken(scene.clock_ticks, scene.rain, scene.thunder);
    SleepReplayAnswer {
        sky_darken,
        dark_outside: sky_darken >= 4,
        needed: status.sleepers_needed(scene.percentage),
        enough: status.enough_sleeping(scene.percentage),
        can_sleep_through_night: scene.percentage <= 100,
        wake_up_ticks: sleep::wake_up_ticks(scene.total_ticks),
        occurs_at: scene.total_ticks.rem_euclid(24_000) == 0,
    }
}

/// `sleep-oracle` 的一道题。
/// One case of the `sleep-oracle`.
#[cfg(feature = "oracle-samples")]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SleepReplayFacts {
    pub clock_ticks: i64,
    pub rain: f32,
    pub thunder: f32,
    pub percentage: i32,
    pub active: i32,
    pub sleeping: i32,
    pub total_ticks: i64,
}

/// `sleep-oracle` 的一道题的产品答案。
/// The production answer to one `sleep-oracle` case.
#[cfg(feature = "oracle-samples")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SleepReplayAnswer {
    pub sky_darken: i32,
    pub dark_outside: bool,
    pub needed: i32,
    pub enough: bool,
    pub can_sleep_through_night: bool,
    pub wake_up_ticks: i64,
    pub occurs_at: bool,
}

/// S2 玩家效果对拍场景：`actions` 每项为
/// `(刻, 0=add / 1=clear-one / 2=clear-all, 效果 ID, 时长, 等级)`。
///
/// An S2 player-effect scene; each action is
/// `(tick, 0=add / 1=clear-one / 2=clear-all, effect id, duration, amplifier)`.
#[cfg(feature = "oracle-samples")]
#[derive(Debug, Clone)]
pub struct ReplayPlayerEffects {
    pub ticks: usize,
    pub start_health: f32,
    pub start_absorption: f32,
    pub actions: Vec<[i32; 5]>,
    /// 故意改坏：时长少减 1 / 抗性倍率 / 丢失隐藏效果。
    pub break_duration: bool,
    pub break_flags: bool,
    pub break_clear: bool,
}

/// S2 逐刻状态：效果表全字段 + 血量 / 吸收 / 饥饿 / 冷却 / 随机源。
#[cfg(feature = "oracle-samples")]
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerEffectStep {
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
    pub seed: i64,
    pub effects: Vec<[i32; 6]>,
}

/// S2 玩家效果场景的正式重放：走 `player_effects` 的 give / clear / tick
/// 和 `player_hurt` 的受伤入口，返回逐刻状态和每刻的包字节。
///
/// Replays a player-effect scene through the production effect path; returns
/// per-tick state and packet bytes.
#[cfg(feature = "oracle-samples")]
pub fn replay_player_effects(
    scene: &ReplayPlayerEffects,
) -> (Vec<PlayerEffectStep>, Vec<Vec<Vec<u8>>>) {
    use mc_bamboo_blocks::entity::mob::effects::EffectInstance;
    use mc_bamboo_core::Vec3;
    use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode, SetHealth};
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
    let mut region = region::Region::new(2, 20, false);
    let id = inbound::PlayerId(1 << 24);
    let mut player = player::Player::new(
        id,
        Uuid::from_u128(1),
        "Oracle0".to_owned(),
        GameMode::Survival,
        ClientInformation::default(),
        Vec3::new(0.5, 64.0, 0.5),
    );
    player.health.health = scene.start_health;
    player.health.absorption = scene.start_absorption;
    player.random =
        mc_bamboo_blocks::entity::JavaRandom::new(mc_bamboo_blocks::entity::ENTITY_RANDOM_SEED);
    let capture = Capture::default();
    region.links.add(id, Box::new(capture.clone()));
    // 官方假世界的 `ChunkMap.sendToTrackingPlayersAndSelf` 不检查实际连接，
    // 但 `Player.playSound` 要一个范围内的收件人才能抓到音效包。
    // Vanilla's fake world collects broadcast sounds; add one observer.
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
    player_attributes::rebuild(&mut region);
    region.sound_seed_generator = mc_bamboo_blocks::LevelRandom::new(12345);
    // 1a 的 `player_hurt::tick` 等价物：关掉自然回血，场景只跑效果链。
    {
        use std::sync::Arc;
        let rules = Arc::make_mut(&mut region.global.rules);
        let _ = rules.assign(
            mc_bamboo_data::gamerules::NATURAL_HEALTH_REGENERATION,
            "false",
        );
    }
    let step_of = |player: &player::Player| -> PlayerEffectStep {
        PlayerEffectStep {
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
            seed: player.random.state(),
            effects: player
                .effects
                .entries()
                .iter()
                .map(|effect| {
                    [
                        effect.effect,
                        effect.duration,
                        effect.amplifier,
                        i32::from(effect.ambient),
                        i32::from(effect.visible),
                        i32::from(effect.show_icon),
                    ]
                })
                .collect(),
        }
    };
    // 官方 `ServerPlayer.doTick` 进服第一刻补的 set_health 不进对拍：
    // Java 侧在 tick 0 之前把它丢掉了。
    region.links.send(
        id,
        &SetHealth {
            health: scene.start_health,
            food: 20,
            saturation: 5.0,
        },
    );
    region.links.flush();
    let _ = take(&capture);
    if let Some(player) = region.players.get_mut(&id) {
        player.health_sync = player_hurt::HealthSync::of(player);
    }
    let mut steps = vec![step_of(&region.players[&id])];
    let mut packets: Vec<Vec<Vec<u8>>> = vec![Vec::new()];
    let mut last_sent = region.players[&id].health.health;
    for tick in 1..=scene.ticks {
        // 这一刻开始时 `health_sync` 记的上次发包值：官方 `doTick` 用它判断
        // 要不要补 set_health。`hurt_player` 中途发包会把它更新成新血量。
        let sync_before = region.players.get(&id).map(|p| p.health_sync);
        for &[at, kind, effect, duration, amplifier] in &scene.actions {
            if at != tick as i32 {
                continue;
            }
            match kind {
                0 => {
                    let mut instance = EffectInstance::with_amplifier(effect, duration, amplifier);
                    if scene.break_duration {
                        instance.duration = instance.duration.saturating_add(1);
                    }
                    if scene.break_flags {
                        instance.visible = false;
                    }
                    player_effects::give(&mut region, id, instance);
                }
                1 => {
                    if !scene.break_clear {
                        player_effects::clear(&mut region, id, Some(effect));
                    }
                }
                2 if !scene.break_clear => {
                    player_effects::clear(&mut region, id, None);
                }
                _ => {}
            }
        }
        // Java 侧显式推进 tickCount、受伤冷却，然后调 `tickEffects`；
        // `player_effects::tick` 里的 entity_ticks 取同一份相对计数。
        if let Some(player) = region.players.get_mut(&id) {
            player.entity_ticks = player.entity_ticks.wrapping_add(1);
            player.health.tick_cooldowns();
        }
        player_effects::tick(&mut region);
        // `ServerPlayer.doTick`：血量变了补一包 `set_health`。
        // `hurt_player` 内部已经发过时 `health_sync` 会在这一刻更新，
        // 用「开始时 vs 现在」判断，避免重复发包。
        // `ServerPlayer.doTick` sends set_health when health changed;
        // `hurt_player` already sent it if `health_sync` moved this tick.
        let player = &region.players[&id];
        let hp = player.health.health;
        let already_sent = sync_before.is_some_and(|before| before != player.health_sync);
        let changed = hp.to_bits() != last_sent.to_bits();
        if changed && !already_sent {
            region.links.send(
                id,
                &SetHealth {
                    health: hp,
                    food: player.health.food.food_level,
                    saturation: player.health.food.saturation,
                },
            );
            region.players.get_mut(&id).expect("player").health_sync = player_hurt::HealthSync {
                health: hp,
                food: player.health.food.food_level,
                saturation_zero: player.health.food.saturation == 0.0,
            };
        }
        if changed || already_sent {
            last_sent = hp;
        }
        steps.push(step_of(&region.players[&id]));
        region.links.flush();
        let mut step = take(&capture);
        // 官方 `Player.playSound` 的音效经 `level.playSound` 广播给跟踪者；
        // 假世界用 `chunkSource.events` 收进同一刻的字节流。这里把观察者
        // 收到的音效按发生顺序插在 `damage_event` 之后。
        let mut sounds = take(&observer_capture);
        if !sounds.is_empty() {
            let at = step
                .iter()
                .position(|body| {
                    mc_bamboo_protocol::codec::decode_varint(body).is_ok_and(|(packet, _)| {
                        packet == mc_bamboo_data::packets::play::clientbound::DAMAGE_EVENT
                    })
                })
                .map_or(step.len(), |index| index + 1);
            for (offset, sound) in sounds.drain(..).enumerate() {
                step.insert(at + offset, sound);
            }
        }
        packets.push(step);
    }
    (steps, packets)
}

/// S3 环境伤害对拍场景：`block` 是 `MobOracle.envBlock` 的种类，
/// `fire` / `air` / `frozen` / `fall` 是起始状态，`y` 是玩家纵坐标。
///
/// An S3 environment-damage scene; `block` is the `envBlock` kind.
#[cfg(feature = "oracle-samples")]
#[derive(Debug, Clone)]
pub struct ReplayPlayerEnv {
    pub ticks: usize,
    pub start_health: f32,
    pub start_absorption: f32,
    pub block: i32,
    pub fire: i32,
    pub air: i32,
    pub frozen: i32,
    pub fall: i32,
    pub y: f64,
    pub difficulty: i32,
    /// 故意改坏：着火不扣血 / 氧气回满速度 / 摔落伤害公式。
    pub break_fire: bool,
    pub break_air: bool,
    pub break_fall: bool,
}

/// S3 逐刻状态：环境字段 + 1a 的受伤状态。
#[cfg(feature = "oracle-samples")]
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerEnvStep {
    pub hp: i32,
    pub abs: i32,
    pub ht: i32,
    pub hd: i32,
    pub cd: i32,
    pub lh: i32,
    pub food: i32,
    pub sat: i32,
    pub exh: i32,
    pub fire: i32,
    pub air: i32,
    pub frozen: i32,
    pub fall: i64,
    pub seed: i64,
}

/// S3 环境伤害场景的正式重放：用生产代码的 `Region` / `Player` /
/// `player_environment::tick_environment` 走完整个场景。
///
/// Replays an environment-damage scene through the production path.
#[cfg(feature = "oracle-samples")]
pub fn replay_player_env(scene: &ReplayPlayerEnv) -> (Vec<PlayerEnvStep>, Vec<Vec<Vec<u8>>>) {
    use mc_bamboo_core::{BlockPos, Vec3};
    use mc_bamboo_data::blocks;
    use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode, SetHealth};
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
    let state = match scene.block {
        1 => blocks::FIRE,
        2 => blocks::LAVA,
        3 => blocks::WATER,
        4 => blocks::CACTUS,
        5 => blocks::SWEET_BERRY_BUSH,
        6 => blocks::MAGMA_BLOCK,
        7 => blocks::POWDER_SNOW,
        8 => blocks::STONE,
        9 => blocks::BUBBLE_COLUMN,
        _ => blocks::AIR,
    };
    let mut region = region::Region::new(2, 20, false);
    region.global.difficulty = match scene.difficulty {
        0 => world_state::Difficulty::Peaceful,
        1 => world_state::Difficulty::Easy,
        3 => world_state::Difficulty::Hard,
        _ => world_state::Difficulty::Normal,
    };
    {
        use std::sync::Arc;
        let rules = Arc::make_mut(&mut region.global.rules);
        let _ = rules.assign(
            mc_bamboo_data::gamerules::NATURAL_HEALTH_REGENERATION,
            "false",
        );
    }
    if scene.block != 0 {
        for y in 63..=65 {
            region.chunks.set_block(BlockPos::new(0, y, 0), state);
        }
        if scene.block == 6 {
            region.chunks.set_block(BlockPos::new(0, 63, 0), state);
        }
    }
    if scene.fall > 0 {
        region
            .chunks
            .set_block(BlockPos::new(0, 62, 0), blocks::STONE);
    }
    let id = inbound::PlayerId(1 << 24);
    let mut player = player::Player::new(
        id,
        Uuid::from_u128(1),
        "Oracle0".to_owned(),
        GameMode::Survival,
        ClientInformation::default(),
        Vec3::new(0.5, scene.y, 0.5),
    );
    player.health.health = scene.start_health;
    player.health.absorption = scene.start_absorption;
    player.environment.remaining_fire_ticks = scene.fire;
    player.environment.air_supply = scene.air;
    player.environment.ticks_frozen = scene.frozen;
    player.motion.fall_distance = f64::from(scene.fall);
    player.on_ground = scene.fall > 0;
    player.random =
        mc_bamboo_blocks::entity::JavaRandom::new(mc_bamboo_blocks::entity::ENTITY_RANDOM_SEED);
    let capture = Capture::default();
    region.links.add(id, Box::new(capture.clone()));
    let observer = inbound::PlayerId((1 << 24) + 100);
    let watcher = player::Player::new(
        observer,
        Uuid::from_u128(2),
        "Observer".to_owned(),
        GameMode::Survival,
        ClientInformation::default(),
        Vec3::new(0.5, scene.y, 0.5),
    );
    let observer_capture = Capture::default();
    region
        .links
        .add(observer, Box::new(observer_capture.clone()));
    region.insert_player(watcher);
    region.insert_player(player);
    player_attributes::rebuild(&mut region);
    region.sound_seed_generator = mc_bamboo_blocks::LevelRandom::new(12345);
    // 摔落伤害在 `finish_tick` 里由 `pending_land` 触发。
    if scene.fall > 0 {
        region
            .players
            .get_mut(&id)
            .expect("player")
            .motion
            .pending_land = true;
    }
    let step_of = |player: &player::Player| -> PlayerEnvStep {
        PlayerEnvStep {
            hp: player.health.health.to_bits() as i32,
            abs: player.health.absorption.to_bits() as i32,
            ht: player.health.hurt_time,
            hd: player.health.hurt_duration,
            cd: player.health.damage_cooldown,
            lh: player.health.last_hurt.to_bits() as i32,
            food: player.health.food.food_level,
            sat: player.health.food.saturation.to_bits() as i32,
            exh: player.health.food.exhaustion.to_bits() as i32,
            fire: player.environment.remaining_fire_ticks,
            air: player.environment.air_supply,
            frozen: player.environment.ticks_frozen,
            fall: player.motion.fall_distance.to_bits() as i64,
            seed: player.random.state(),
        }
    };
    region.links.send(
        id,
        &SetHealth {
            health: scene.start_health,
            food: 20,
            saturation: 5.0,
        },
    );
    region.links.flush();
    let _ = take(&capture);
    if let Some(player) = region.players.get_mut(&id) {
        player.health_sync = player_hurt::HealthSync::of(player);
    }
    let mut steps = vec![step_of(&region.players[&id])];
    let mut packets: Vec<Vec<Vec<u8>>> = vec![Vec::new()];
    let mut last_sent = scene.start_health;
    for tick in 1..=scene.ticks {
        let sync_before = region.players.get(&id).map(|p| p.health_sync);
        if scene.fall > 0 && tick == 1 {
            // 第一刻 `on_land` 结算摔落伤害（官方 `causeFallDamage`）。
            if scene.break_fall {
                let amount =
                    player_environment::fall_damage(&region.players[&id], f64::from(scene.fall))
                        as f32
                        + 1.0;
                player_hurt::hurt_player(
                    &mut region,
                    id,
                    mc_bamboo_blocks::entity::mob::DamageSource::environment(
                        mc_bamboo_blocks::entity::mob::DamageKind::Fall,
                    ),
                    amount,
                    &mut None,
                );
                region
                    .players
                    .get_mut(&id)
                    .expect("player")
                    .motion
                    .fall_distance = 0.0;
            } else {
                player_environment::on_land(&mut region, id);
            }
            if let Some(p) = region.players.get_mut(&id) {
                p.motion.pending_land = false;
            }
        }
        if let Some(player) = region.players.get_mut(&id) {
            player.entity_ticks = player.entity_ticks.wrapping_add(1);
            if scene.break_fire {
                player.environment.remaining_fire_ticks = 0;
            }
            if scene.break_air {
                player.environment.air_supply = player_environment::MAX_AIR;
            }
        }
        player_environment::tick_environment_players(&mut region, &[id]);
        // `LivingEntity.baseTick` 在 `onFire` / `isInWall` 伤害之后递减 hurtTime。
        if let Some(p) = region.players.get_mut(&id) {
            p.health.hurt_time = (p.health.hurt_time - 1).max(0);
        }
        let p = &region.players[&id];
        let hp = p.health.health;
        let already_sent = sync_before.is_some_and(|before| before != p.health_sync);
        let changed = hp.to_bits() != last_sent.to_bits();
        if changed && !already_sent {
            region.links.send(
                id,
                &SetHealth {
                    health: hp,
                    food: p.health.food.food_level,
                    saturation: p.health.food.saturation,
                },
            );
            region.players.get_mut(&id).expect("player").health_sync = player_hurt::HealthSync {
                health: hp,
                food: p.health.food.food_level,
                saturation_zero: p.health.food.saturation == 0.0,
            };
        }
        if changed || already_sent {
            last_sent = hp;
        }
        steps.push(step_of(&region.players[&id]));
        region.links.flush();
        let mut step = take(&capture);
        let mut sounds = take(&observer_capture);
        if !sounds.is_empty() {
            let at = step
                .iter()
                .position(|body| {
                    mc_bamboo_protocol::codec::decode_varint(body).is_ok_and(|(packet, _)| {
                        packet == mc_bamboo_data::packets::play::clientbound::DAMAGE_EVENT
                    })
                })
                .map_or(step.len(), |index| index + 1);
            for (offset, sound) in sounds.drain(..).enumerate() {
                step.insert(at + offset, sound);
            }
        }
        packets.push(step);
    }
    (steps, packets)
}

/// S4 吃东西对拍场景。
#[cfg(feature = "oracle-samples")]
#[derive(Debug, Clone)]
pub struct ReplayPlayerConsume {
    pub ticks: usize,
    pub item: i32,
    pub start_food: i32,
    pub break_duration: bool,
    pub break_food: bool,
    pub break_remainder: bool,
}

/// S4 逐刻状态。
#[cfg(feature = "oracle-samples")]
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerConsumeStep {
    pub hp: i32,
    pub food: i32,
    pub sat: i32,
    pub exh: i32,
    pub item: i32,
    pub count: i32,
    pub using: i32,
    pub effects: Vec<[i32; 3]>,
}

/// S4 吃东西场景的正式重放。
#[cfg(feature = "oracle-samples")]
pub fn replay_player_consume(
    scene: &ReplayPlayerConsume,
) -> (Vec<PlayerConsumeStep>, Vec<Vec<Vec<u8>>>) {
    use mc_bamboo_core::{Hand, Vec3};
    use mc_bamboo_protocol::{
        item::ItemStack,
        packet::play::{ClientInformation, GameMode, SetHealth},
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
    let mut region = region::Region::new(2, 20, false);
    let id = inbound::PlayerId(1 << 24);
    let mut player = player::Player::new(
        id,
        Uuid::from_u128(1),
        "Oracle0".to_owned(),
        GameMode::Survival,
        ClientInformation::default(),
        Vec3::new(0.5, 64.0, 0.5),
    );
    player.health.food.food_level = scene.start_food;
    player.random =
        mc_bamboo_blocks::entity::JavaRandom::new(mc_bamboo_blocks::entity::ENTITY_RANDOM_SEED);
    let slot = player.inventory.hand_slot(Hand::Main);
    player.inventory.slots[slot] = ItemStack::new(scene.item, 1);
    let capture = Capture::default();
    region.links.add(id, Box::new(capture.clone()));
    region.insert_player(player);
    region.links.send(
        id,
        &SetHealth {
            health: 20.0,
            food: scene.start_food,
            saturation: 5.0,
        },
    );
    region.links.flush();
    let _ = take(&capture);
    if let Some(p) = region.players.get_mut(&id) {
        p.health_sync = player_hurt::HealthSync::of(p);
    }
    let step_of = |p: &player::Player| -> PlayerConsumeStep {
        let held = p.inventory.held(Hand::Main);
        PlayerConsumeStep {
            hp: p.health.health.to_bits() as i32,
            food: p.health.food.food_level,
            sat: p.health.food.saturation.to_bits() as i32,
            exh: p.health.food.exhaustion.to_bits() as i32,
            item: held.item,
            count: held.count,
            using: p.use_item.as_ref().map_or(0, |u| u.remaining),
            effects: p
                .effects
                .entries()
                .iter()
                .map(|e| [e.effect, e.duration, e.amplifier])
                .collect(),
        }
    };
    let mut steps = vec![step_of(&region.players[&id])];
    let mut packets: Vec<Vec<Vec<u8>>> = vec![Vec::new()];
    // `startUsingItem` 在 tick 1 开始（官方 `handleUseItem` 之后）。
    player_consume::start(&mut region, id, Hand::Main);
    if scene.break_remainder {
        // 改坏确认：把 `use_remainder` 换成一个错误的物品 ID。
        region.players.get_mut(&id).expect("player").inventory.slots[slot].count = 2;
    }
    if scene.break_duration
        && let Some(p) = region.players.get_mut(&id)
        && let Some(using) = p.use_item.as_mut()
    {
        using.remaining += 1;
    }
    let mut last_sent = 20.0_f32;
    for _ in 1..=scene.ticks {
        let sync_before = region.players.get(&id).map(|p| p.health_sync);
        // 假世界的 `playConsume` 不跑 `LivingEntity.baseTick`，所以这里也
        // 不推进效果表；只跑吃东西的剩余刻数。
        player_consume::tick(&mut region);
        if scene.break_food {
            region
                .players
                .get_mut(&id)
                .expect("player")
                .health
                .food
                .food_level = 0;
        }
        let p = &region.players[&id];
        let hp = p.health.health;
        let already = sync_before.is_some_and(|before| before != p.health_sync);
        let changed = hp.to_bits() != last_sent.to_bits();
        if changed && !already {
            region.links.send(
                id,
                &SetHealth {
                    health: hp,
                    food: p.health.food.food_level,
                    saturation: p.health.food.saturation,
                },
            );
            region.players.get_mut(&id).expect("player").health_sync = player_hurt::HealthSync {
                health: hp,
                food: p.health.food.food_level,
                saturation_zero: p.health.food.saturation == 0.0,
            };
        }
        if changed || already {
            last_sent = hp;
        }
        steps.push(step_of(&region.players[&id]));
        region.links.flush();
        // 假世界没有物品栏容器桩，`container_set_slot` 两边都不比。
        let filtered: Vec<Vec<u8>> = take(&capture)
            .into_iter()
            .filter(|body| {
                mc_bamboo_protocol::codec::decode_varint(body).is_ok_and(|(packet, _)| {
                    packet != mc_bamboo_data::packets::play::clientbound::CONTAINER_SET_SLOT
                })
            })
            .collect();
        packets.push(filtered);
    }
    (steps, packets)
}

/// 一次射箭重放的场景：武器、弹药、拉弓刻数、附魔和视角。
///
/// One shot replay scene: weapon, ammo, draw ticks, enchantments and view angles.
#[cfg(feature = "oracle-samples")]
#[derive(Debug, Clone)]
pub struct ReplayShot {
    pub weapon: i32,
    pub ammo_item: i32,
    pub ammo_count: i32,
    pub hold_ticks: i32,
    pub yaw: i32,
    pub pitch: i32,
    pub movement: Option<mc_bamboo_core::Vec3>,
    pub enchantments: Vec<(String, i32)>,
    pub potion: Option<i32>,
    pub creative: bool,
}

/// 一支箭的初始状态，按原始位记录。
///
/// One arrow's initial state, as raw bits.
#[cfg(feature = "oracle-samples")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayShotArrow {
    /// 位置 x/y/z、速度 x/y/z 的 f64 原始位。
    pub motion: [i64; 6],
    pub yaw: i32,
    pub pitch: i32,
    pub critical: bool,
    pub pierce: i32,
    pub fire: i32,
    pub pickup: u8,
    /// `pickup` 物品堆的物品 ID 和数量。
    pub item: i32,
    pub count: i32,
}

/// 一次射箭重放的结果。
///
/// One shot replay result.
#[cfg(feature = "oracle-samples")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayShotResult {
    pub arrows: Vec<ReplayShotArrow>,
    pub ammo: i32,
    pub ammo_item: i32,
    pub weapon_damage: i32,
    pub player_seed: i64,
    pub level_seed: i64,
}

/// 玩家射箭的正式重放，用和游戏里同一套 `player_weapon` 入口。
///
/// The production replay of a player shot, through the same `player_weapon`
/// entry points the game uses.
#[cfg(feature = "oracle-samples")]
pub fn replay_shot(scene: &ReplayShot) -> ReplayShotResult {
    use mc_bamboo_core::{Hand, Vec3};
    use mc_bamboo_protocol::{
        components::{ComponentPatch, ComponentValue, PatchEntry},
        item::ItemStack,
        packet::play::{ClientInformation, GameMode},
    };
    use uuid::Uuid;

    let mut region = region::Region::new(2, 20, false);
    region.random = mc_bamboo_blocks::LevelRandom::new(1);
    let id = inbound::PlayerId(1 << 24);
    let game_mode = if scene.creative {
        GameMode::Creative
    } else {
        GameMode::Survival
    };
    let mut player = player::Player::new(
        id,
        Uuid::from_u128(1),
        "Oracle0".to_owned(),
        game_mode,
        ClientInformation::default(),
        Vec3::new(0.5, 64.0, 0.5),
    );
    player.yaw = f32::from_bits(scene.yaw as u32);
    player.head_yaw = player.yaw;
    player.pitch = f32::from_bits(scene.pitch as u32);
    player.motion.known_movement = scene.movement.unwrap_or_default();
    player.random =
        mc_bamboo_blocks::entity::JavaRandom::new(mc_bamboo_blocks::entity::ENTITY_RANDOM_SEED);
    let main = player.inventory.hand_slot(Hand::Main);
    player.inventory.slots[main] = ItemStack::new(scene.weapon, 1);
    player.inventory.slots[main + 1] = ItemStack::new(scene.ammo_item, scene.ammo_count);
    let mut ammo_patch = ComponentPatch::new();
    if let Some(potion) = scene.potion {
        ammo_patch.put(
            mc_bamboo_data::registries::data_component_type::POTION_CONTENTS,
            PatchEntry::Set(ComponentValue::PotionContents {
                potion: Some(potion),
                custom_color: None,
                custom_effects: Vec::new(),
                custom_name: None,
            }),
        );
    }
    let (added, removed) = ammo_patch.to_opaque();
    player.inventory.slots[main + 1].added = added;
    player.inventory.slots[main + 1].removed = removed;
    let mut weapon_patch = ComponentPatch::new();
    if !scene.enchantments.is_empty() {
        let levels: Vec<(i32, i32)> = scene
            .enchantments
            .iter()
            .filter_map(|(name, level)| {
                mc_bamboo_data::registries::SYNCED_REGISTRIES
                    .iter()
                    .find(|registry| registry.name == "minecraft:enchantment")
                    .and_then(|registry| registry.entries.iter().position(|entry| entry == name))
                    .map(|id| (id as i32, *level))
            })
            .collect();
        weapon_patch.put(
            mc_bamboo_data::registries::data_component_type::ENCHANTMENTS,
            PatchEntry::Set(ComponentValue::Enchantments(levels)),
        );
    }
    let (added, removed) = weapon_patch.to_opaque();
    player.inventory.slots[main].added = added;
    player.inventory.slots[main].removed = removed;
    region.insert_player(player);

    player_weapon::start(&mut region, id, Hand::Main);
    // 弩要多跑一刻：官方 `onUseTick` 在递减之前调用，装满发生在第 hold 刻。
    //
    // A crossbow needs one more tick: `onUseTick` runs before the decrement, so
    // loading happens on the hold-th tick.
    let ticks = scene.hold_ticks + i32::from(scene.weapon == mc_bamboo_data::items::CROSSBOW);
    for _ in 0..ticks {
        player_weapon::tick(&mut region);
    }
    if scene.weapon == mc_bamboo_data::items::CROSSBOW {
        player_weapon::release(&mut region, id);
        player_weapon::start(&mut region, id, Hand::Main);
    } else {
        player_weapon::release(&mut region, id);
    }

    let arrows = region
        .entities
        .ids()
        .into_iter()
        .filter_map(|uid| {
            let entity = region.entities.public_at(uid)?;
            let mc_bamboo_blocks::entity::KindPublic::Arrow(public) = entity.kind else {
                return None;
            };
            let state = region.entities.get(uid)?;
            let mc_bamboo_blocks::entity::EntityKind::Arrow(arrow) = &state.kind else {
                return None;
            };
            Some(ReplayShotArrow {
                motion: [
                    entity.position.x.to_bits() as i64,
                    entity.position.y.to_bits() as i64,
                    entity.position.z.to_bits() as i64,
                    entity.delta.x.to_bits() as i64,
                    entity.delta.y.to_bits() as i64,
                    entity.delta.z.to_bits() as i64,
                ],
                yaw: entity.yaw.to_bits() as i32,
                pitch: public.pitch.to_bits() as i32,
                critical: public.critical,
                pierce: i32::from(public.pierce),
                fire: state.remaining_fire_ticks,
                pickup: match public.pickup {
                    mc_bamboo_blocks::entity::arrow::Pickup::Disallowed => 0,
                    mc_bamboo_blocks::entity::arrow::Pickup::Allowed => 1,
                    mc_bamboo_blocks::entity::arrow::Pickup::CreativeOnly => 2,
                },
                item: arrow.pickup_item.item,
                count: arrow.pickup_item.count,
            })
        })
        .collect();
    let held = region.players[&id].inventory.held(Hand::Main).clone();
    let damage = match mc_bamboo_protocol::components::defaults::resolve(
        held.item,
        &held.patch(),
        mc_bamboo_data::registries::data_component_type::DAMAGE,
    ) {
        Some(ComponentValue::Damage(value)) => *value,
        _ => 0,
    };
    ReplayShotResult {
        arrows,
        ammo: region.players[&id].inventory.slots[main + 1].count,
        ammo_item: region.players[&id].inventory.slots[main + 1].item,
        weapon_damage: damage,
        player_seed: region.players[&id].random.state(),
        level_seed: region.random.source().seed(),
    }
}

/// 复放一条 `/advancement` 命令，返回「改动了多少判据 / 反馈键 / 完成的进度集合」，
/// 给 `command-oracle` 的官方假世界执行结果对拍用。
///
/// Replay one `/advancement` command against a fresh `PlayerAdvancements` state.
/// Returns the number of changed criteria, the feedback key and the sorted
/// completed advancement ids. Gated behind the `oracle-samples` feature.
#[cfg(feature = "oracle-samples")]
pub fn replay_advancement_command(line: &str) -> (i32, String, String) {
    use mc_bamboo_advancements::AdvancementTree;

    let tree = AdvancementTree::global();
    let mut state = advancements::PlayerAdvancements::default();
    let mut setup = None;
    let command = match line.split_once(" ;; ") {
        Some((first, second)) => {
            setup = Some(first);
            second
        }
        None => line,
    };
    let apply = |state: &mut advancements::PlayerAdvancements, command: &str| -> (i32, String) {
        let words: Vec<&str> = command.split_whitespace().collect();
        if words.len() < 4 || words[0] != "advancement" {
            return (0, "command.failed".to_owned());
        }
        let grant = words[1] == "grant";
        let action = if grant { "grant" } else { "revoke" };
        let mode_word = words[3];
        let everything = mode_word == "everything";
        let target_id = if everything {
            None
        } else {
            words.get(4).copied()
        };
        let criterion = if everything {
            None
        } else {
            words.get(5).copied()
        };
        let index = target_id.and_then(|id| tree.index(id)).unwrap_or(0);
        if let Some(id) = target_id
            && tree.index(id).is_none()
        {
            return (0, "advancement.advancementNotFound".to_owned());
        }
        // 官方 `AdvancementCommands$Mode` 的 parents / children。
        let (parents, children) = match mode_word {
            "only" => (false, false),
            "from" => (false, true),
            "until" => (true, false),
            "through" => (true, true),
            "everything" => (true, true),
            _ => (false, false),
        };
        let mut indices = Vec::new();
        if target_id.is_none() {
            indices.extend(0..tree.nodes.len());
        } else {
            if parents {
                let mut current = Some(index);
                while let Some(node) = current {
                    indices.push(node);
                    current = tree.nodes[node].parent;
                }
                indices.reverse();
                indices.pop();
            }
            indices.push(index);
            if children {
                let mut stack = tree.nodes[index].children.clone();
                while let Some(node) = stack.pop() {
                    indices.push(node);
                    stack.extend(tree.nodes[node].children.iter().copied());
                }
            }
        }
        let mut count = 0i32;
        let mut bad_criterion = false;
        for &node in &indices {
            let names: Vec<&str> = tree.nodes[node]
                .advancement
                .criteria
                .iter()
                .map(|(name, _)| name.as_str())
                .filter(|name| criterion.is_none_or(|only| only == *name))
                .collect();
            if criterion.is_some() && names.is_empty() && node == index {
                bad_criterion = true;
            }
            for name in names {
                let changed = if grant {
                    state.award(tree, node, name)
                } else {
                    state.revoke(tree, node, name)
                };
                if changed {
                    count += 1;
                }
            }
        }
        if bad_criterion {
            return (0, "commands.advancement.criterionNotFound".to_owned());
        }
        let amount = if criterion.is_some() {
            "criterion"
        } else if indices.len() == 1 {
            "one"
        } else {
            "many"
        };
        let key = if count == 0 {
            format!("commands.advancement.{action}.{amount}.to.one.failure")
        } else {
            format!("commands.advancement.{action}.{amount}.to.one.success")
        };
        (count, key)
    };
    if let Some(setup) = setup {
        apply(&mut state, setup);
    }
    let (count, key) = apply(&mut state, command);
    let mut done: Vec<&str> = state
        .progress
        .iter()
        .filter(|(_, progress)| progress.is_done())
        .map(|(index, _)| tree.nodes[*index].id)
        .collect();
    done.sort_unstable();
    (count, key, done.join(","))
}

/// 玩家属性对拍里的自定义修饰符：`(attribute_id, mod_id, amount, op, slot_group)`。
///
/// One custom item attribute modifier entry for the player-attributes oracle.
#[cfg(feature = "oracle-samples")]
pub type ReplayCustomModifier = (i32, String, f64, i32, i32);

/// 玩家属性对拍里单条已生效修饰符：`(mod_id, amount 原始位, op)`。
///
/// One resolved attribute modifier for the player-attributes oracle.
#[cfg(feature = "oracle-samples")]
pub type ReplayAttrModifier = (String, u64, i32);

/// 玩家属性对拍里单条属性结果：`(attribute_id, value 原始位, 修饰符列表)`。
///
/// One resolved attribute snapshot for the player-attributes oracle.
#[cfg(feature = "oracle-samples")]
pub type ReplayAttrEntry = (i32, u64, Vec<ReplayAttrModifier>);

/// 玩家属性对拍里单个装备槽的输入描述。
///
/// One equipment slot input for the player-attributes oracle.
#[cfg(feature = "oracle-samples")]
#[derive(Debug, Clone)]
pub struct ReplayAttrSlot {
    /// 0=MainHand, 1=OffHand, 2=Feet, 3=Legs, 4=Chest, 5=Head。
    pub slot: u8,
    pub item: i32,
    pub enchantments: Vec<(i32, i32)>,
    /// `Some` 时覆盖默认 `attribute_modifiers` 组件。
    pub custom_modifiers: Option<Vec<ReplayCustomModifier>>,
}

/// 玩家属性对拍的场景描述。
///
/// One randomized scenario for the player-attributes oracle.
#[cfg(feature = "oracle-samples")]
#[derive(Debug, Clone)]
pub struct ReplayAttrScene {
    /// 0=Survival, 1=Creative, 2=Adventure, 3=Spectator。
    pub game_mode: u8,
    pub sprinting: bool,
    pub crouching: bool,
    pub ticks_frozen: i32,
    pub on_solid_for_frost: bool,
    pub slots: Vec<ReplayAttrSlot>,
    pub effects: Vec<(i32, i32)>,
    /// 0=正常，1=故意改坏效率附魔，2=故意把 AddMultipliedTotal 改成 AddMultipliedBase。
    pub break_mode: u8,
}

/// 玩家属性对拍的输出：每个属性的 `(id, value 原始位, 排序后的 (mod_id, amount 原始位, op))`，
/// 以及供挖掘速度对拍的 `mining_inputs`。
///
/// Output of one player-attributes replay: each attribute's `(id, raw value bits,
/// sorted modifiers)` plus resolved `mining_inputs`.
#[cfg(feature = "oracle-samples")]
#[derive(Debug, Clone)]
pub struct ReplayAttrResult {
    pub attributes: Vec<ReplayAttrEntry>,
    pub mining_efficiency: f32,
    pub block_break_speed: f32,
    pub submerged_mining_speed: f32,
    pub has_dig_speed: bool,
    pub dig_speed_amplification: i32,
    pub mining_fatigue: Option<i32>,
}

/// 用生产代码的 `Player` + `player_attributes::rebuild_player` 重放一次属性场景，
/// 供 `mc-bamboo-vanilla player-attributes` 对拍。
///
/// Replay one attribute scenario through production `Player` and
/// `player_attributes::rebuild_player` for `mc-bamboo-vanilla player-attributes`.
#[cfg(feature = "oracle-samples")]
pub fn replay_player_attributes(scene: &ReplayAttrScene) -> ReplayAttrResult {
    use mc_bamboo_blocks::entity::mob::{attributes::Operation, effects::EffectInstance};
    use mc_bamboo_core::Vec3;
    use mc_bamboo_data::registries::{attribute, data_component_type as kinds};
    use mc_bamboo_protocol::{
        components::{
            ComponentPatch, ComponentValue, PatchEntry,
            value::{AttributeDisplay, AttributeEntry, AttributeModifier},
        },
        item::ItemStack,
        packet::play::{ClientInformation, ENTITY_FLAG_SPRINTING, GameMode},
    };
    use uuid::Uuid;

    let game_mode = match scene.game_mode {
        1 => GameMode::Creative,
        2 => GameMode::Adventure,
        3 => GameMode::Spectator,
        _ => GameMode::Survival,
    };
    let mut player = player::Player::new(
        inbound::PlayerId(42),
        Uuid::from_u128(42),
        "AttrOracle".to_owned(),
        game_mode,
        ClientInformation::default(),
        Vec3::default(),
    );
    if scene.sprinting {
        player.sync.shared_flags |= ENTITY_FLAG_SPRINTING;
    }
    player.sync.shift_key_down = scene.crouching;
    player.environment.ticks_frozen = scene.ticks_frozen;

    // 0=MainHand(36), 1=OffHand(45), 2=Feet(8), 3=Legs(7), 4=Chest(6), 5=Head(5)。
    let menu_slot = [36_usize, 45, 8, 7, 6, 5];
    for slot in &scene.slots {
        let Some(&index) = menu_slot.get(slot.slot as usize) else {
            continue;
        };
        let mut patch = ComponentPatch::new();
        if !slot.enchantments.is_empty() {
            patch.put(
                kinds::ENCHANTMENTS,
                PatchEntry::Set(ComponentValue::Enchantments(slot.enchantments.clone())),
            );
        }
        if let Some(custom) = &slot.custom_modifiers {
            let entries: Vec<AttributeEntry> = custom
                .iter()
                .map(|(attr_id, mod_id, amount, op, group)| AttributeEntry {
                    attribute: *attr_id,
                    modifier: AttributeModifier {
                        id: mod_id.clone(),
                        amount: *amount,
                        operation: *op,
                    },
                    slot: *group,
                    display: AttributeDisplay::Default,
                })
                .collect();
            patch.put(
                kinds::ATTRIBUTE_MODIFIERS,
                PatchEntry::Set(ComponentValue::AttributeModifiers(entries)),
            );
        }
        let (added, removed) = patch.to_opaque();
        player.inventory.slots[index] = ItemStack {
            item: slot.item,
            count: 1,
            added,
            removed,
        };
    }
    for &(effect, amplifier) in &scene.effects {
        player
            .effects
            .add(EffectInstance::with_amplifier(effect, 200, amplifier));
    }
    player_attributes::rebuild_player(&mut player, scene.on_solid_for_frost);

    if scene.break_mode == 1
        && let Some(inst) = player.attributes.instance(attribute::MINING_EFFICIENCY)
    {
        inst.base += 1.0;
    } else if scene.break_mode == 2 {
        for entry in &mut player.attributes.entries {
            for modifier in &mut entry.modifiers {
                if modifier.operation == Operation::AddMultipliedTotal {
                    modifier.operation = Operation::AddMultipliedBase;
                }
            }
        }
    }

    let mut attributes: Vec<ReplayAttrEntry> = player
        .attributes
        .entries
        .iter()
        .map(|entry| {
            let mut mods: Vec<ReplayAttrModifier> = entry
                .modifiers
                .iter()
                .map(|m| {
                    let op = match m.operation {
                        Operation::AddValue => 0,
                        Operation::AddMultipliedBase => 1,
                        Operation::AddMultipliedTotal => 2,
                    };
                    (m.id.clone(), m.amount.to_bits(), op)
                })
                .collect();
            mods.sort_by(|a, b| a.0.cmp(&b.0));
            (entry.id, entry.value().to_bits(), mods)
        })
        .collect();
    attributes.sort_by_key(|row| row.0);

    let mining = player_attributes::mining_inputs(&player);
    ReplayAttrResult {
        attributes,
        mining_efficiency: mining.mining_efficiency,
        block_break_speed: mining.block_break_speed,
        submerged_mining_speed: mining.submerged_mining_speed,
        has_dig_speed: mining.has_dig_speed,
        dig_speed_amplification: mining.dig_speed_amplification,
        mining_fatigue: mining.mining_fatigue,
    }
}

/// 供 `mc-bamboo-vanilla player-attributes` 对拍 `UpdateAttributes` 线上包体：
/// 返回 `(进服全量包体, 速度 II 脏同步包体, 效率V镐+创造+疾跑脏同步包体)`（去掉长度和包 ID）。
/// `omit_dirty = true` 用于改坏确认（模拟漏发脏属性包）。
///
/// Replay `UpdateAttributes` wire payloads (body after packet id) for:
/// `(full join snapshot, Speed II dirty sync, Efficiency V + Creative + Sprinting dirty sync)`.
#[cfg(feature = "oracle-samples")]
pub fn replay_speed_sync_packets(omit_dirty: bool) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    use mc_bamboo_blocks::entity::mob::effects::EffectInstance;
    use mc_bamboo_core::Vec3;
    use mc_bamboo_data::{
        items,
        packets::play::clientbound,
        registries::{data_component_type as kinds, mob_effect},
    };
    use mc_bamboo_protocol::{
        codec::decode_varint,
        components::{ComponentPatch, ComponentValue, PatchEntry},
        frame::{MAX_FRAME_LEN, find_frame},
        item::ItemStack,
        packet::play::{ClientInformation, ENTITY_FLAG_SPRINTING, GameMode},
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

    let extract_update_body = |capture: &Capture| -> Vec<u8> {
        let bytes = std::mem::take(&mut *capture.0.lock().unwrap());
        let mut rest = bytes.as_slice();
        while !rest.is_empty() {
            let Some(bounds) = find_frame(rest, MAX_FRAME_LEN).ok().flatten() else {
                break;
            };
            let payload = &rest[bounds.header_len..bounds.header_len + bounds.payload_len];
            rest = &rest[bounds.header_len + bounds.payload_len..];
            if let Ok((packet_id, consumed)) = decode_varint(payload)
                && packet_id == clientbound::UPDATE_ATTRIBUTES
            {
                return payload[consumed..].to_vec();
            }
        }
        Vec::new()
    };

    let mut region = region::Region::new(2, 20, false);
    let id = inbound::PlayerId(42);
    let player = player::Player::new(
        id,
        Uuid::from_u128(42),
        "Alice".to_owned(),
        GameMode::Creative,
        ClientInformation::default(),
        Vec3::new(0.5, 64.0, 0.5),
    );
    let capture = Capture::default();
    region.links.add(id, Box::new(capture.clone()));
    region.insert_player(player);

    // 1. 创造模式进服初始同步（两项创造触及距离）。
    player_attributes::send_full_to_self(&mut region, id);
    region.links.flush();
    let full_body = extract_update_body(&capture);

    // 2. 速度 II 药水脏同步。
    region
        .players
        .get_mut(&id)
        .expect("player")
        .effects
        .add(EffectInstance::with_amplifier(mob_effect::SPEED, 200, 1));
    player_attributes::rebuild(&mut region);
    if !omit_dirty {
        player_attributes::sync_dirty(&mut region);
    }
    region.links.flush();
    let speed_body = extract_update_body(&capture);

    // 3. 主手效率 V 钻石镐 + 切回生存模式 + 疾跑脏同步。
    let efficiency_id = mc_bamboo_data::registries::SYNCED_REGISTRIES
        .iter()
        .find(|r| r.name == "minecraft:enchantment")
        .and_then(|r| r.entries.iter().position(|e| *e == "minecraft:efficiency"))
        .expect("efficiency enchantment") as i32;
    let mut patch = ComponentPatch::new();
    patch.put(
        kinds::ENCHANTMENTS,
        PatchEntry::Set(ComponentValue::Enchantments(vec![(efficiency_id, 5)])),
    );
    let (added, removed) = patch.to_opaque();
    let pickaxe = ItemStack {
        item: items::DIAMOND_PICKAXE,
        count: 1,
        added,
        removed,
    };
    {
        let p = region.players.get_mut(&id).expect("player");
        p.inventory.slots[36] = pickaxe;
        p.game_mode = GameMode::Survival;
        p.sync.shared_flags |= ENTITY_FLAG_SPRINTING;
    }
    player_attributes::rebuild(&mut region);
    if !omit_dirty {
        player_attributes::sync_dirty(&mut region);
    }
    region.links.flush();
    let complex_body = extract_update_body(&capture);

    (full_body, speed_body, complex_body)
}

/// 供 `mc-bamboo-vanilla player-attributes` 对拍灵魂疾行激活时的
/// `UpdateAttributes` 脏同步包体：玩家穿灵魂疾行 III 靴子站在灵魂沙上，
/// 按官方 `location_changed` 属性效果挂两个临时修饰符，再发脏同步包。
/// `omit_soul_speed = true` 用于改坏确认（漏掉 location 效果）。
///
/// Replay the dirty `UpdateAttributes` payload for an active soul-speed III
/// enchantment; `omit_soul_speed` is a deliberate break check.
#[cfg(feature = "oracle-samples")]
pub fn replay_soul_speed_sync_packet(omit_soul_speed: bool) -> Vec<u8> {
    use mc_bamboo_blocks::entity::mob::enchantment_effects::EffectId;
    use mc_bamboo_core::Vec3;
    use mc_bamboo_data::{
        items, packets::play::clientbound, registries::data_component_type as kinds,
    };
    use mc_bamboo_protocol::{
        codec::decode_varint,
        components::{ComponentPatch, ComponentValue, PatchEntry},
        frame::{MAX_FRAME_LEN, find_frame},
        item::ItemStack,
        packet::play::ClientInformation,
    };
    use std::sync::{Arc, Mutex};

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

    let capture = Capture::default();
    let mut region = region::Region::new(2, 20, false);
    let id = inbound::PlayerId(42);
    let player = player::Player::new(
        id,
        uuid::Uuid::from_u128(42),
        "Soul".to_owned(),
        mc_bamboo_protocol::packet::play::GameMode::Survival,
        ClientInformation::default(),
        Vec3::new(0.5, 64.0, 0.5),
    );
    region.links.add(id, Box::new(capture.clone()));
    region.insert_player(player);
    let soul_speed_id = mc_bamboo_data::registries::SYNCED_REGISTRIES
        .iter()
        .find(|r| r.name == "minecraft:enchantment")
        .and_then(|r| r.entries.iter().position(|e| *e == "minecraft:soul_speed"))
        .expect("soul_speed exists") as i32;
    let mut patch = ComponentPatch::new();
    patch.put(
        kinds::ENCHANTMENTS,
        PatchEntry::Set(ComponentValue::Enchantments(vec![(soul_speed_id, 3)])),
    );
    let (added, removed) = patch.to_opaque();
    region.players.get_mut(&id).expect("player").inventory.slots[8] = ItemStack {
        item: items::DIAMOND_BOOTS,
        count: 1,
        added,
        removed,
    };
    player_attributes::send_full_to_self(&mut region, id);
    region.links.flush();
    // 丢掉进服全量包，只留这一刻的脏包。
    let _ = std::mem::take(&mut *capture.0.lock().unwrap());
    if !omit_soul_speed {
        // 官方 location_changed 的 attribute 效果在玩家属性表里加两个临时修饰符。
        let player = region.players.get_mut(&id).expect("player");
        let mut active = player.location_effects;
        active.set(EffectId::SoulSpeedAttributes, true);
        player.location_effects = active;
        player.soul_speed_level = 3;
    }
    player_attributes::rebuild(&mut region);
    player_attributes::sync_dirty(&mut region);
    region.links.flush();
    let bytes = std::mem::take(&mut *capture.0.lock().unwrap());
    let mut rest = bytes.as_slice();
    while let Some(bounds) = find_frame(rest, MAX_FRAME_LEN).ok().flatten() {
        let payload = &rest[bounds.header_len..bounds.header_len + bounds.payload_len];
        rest = &rest[bounds.header_len + bounds.payload_len..];
        if let Ok((packet_id, consumed)) = decode_varint(payload)
            && packet_id == clientbound::UPDATE_ATTRIBUTES
        {
            return payload[consumed..].to_vec();
        }
    }
    Vec::new()
}
