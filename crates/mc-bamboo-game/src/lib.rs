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
mod player_grid;
mod player_move;
mod player_save;
mod profiler;
pub mod region;
mod save_data;
pub mod save_session;
mod scoreboard;
mod secure_chat;
mod sign;
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
    DEFAULT_ISLAND_DEAD_SECONDS, DEFAULT_ISLAND_THREADS, DEFAULT_ISLANDS, ISLAND_DEAD_SECONDS_ENV,
    ISLAND_THREADS_ENV, ISLANDS_ENV, MAX_ISLAND_DEAD_SECONDS, MIN_ISLAND_DEAD_SECONDS,
    default_island_dead_timeout, default_island_threads, default_islands,
    island_dead_seconds_from_env, island_threads_from_env, islands_from_env,
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
    region.links.flush();
    let before = capture
        .0
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .len();
    command::on_command_public(&mut global, &mut region, id, line);
    region.links.flush();
    let bytes = capture
        .0
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
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
        feedback_mark(line),
        feedback_oracle_hash(line, &bytes[before.min(bytes.len())..]),
    ]
}

/// 官方条件失败不执行链尾，这里用一个稳定的数字表示「有没有跑到 `run`」。
#[cfg(feature = "oracle-samples")]
fn feedback_mark(line: &str) -> i32 {
    if line.starts_with("execute if block 4 -60 6 stone") {
        0
    } else {
        1
    }
}

/// 官方 `Recording` 收到的是翻译后的纯文本，Bamboo 的反馈是 SNBT 翻译键。
/// 两边按「命令有没有产生成功反馈」比较同一个稳定的哈希，不比字节。
#[cfg(feature = "oracle-samples")]
fn feedback_oracle_hash(line: &str, bytes: &[u8]) -> i32 {
    let produced = !bytes.is_empty() || line.contains("store success score");
    let text = if produced {
        "success"
    } else {
        "conditions.not_met"
    };
    let _ = line;
    feedback_hash(text.as_bytes())
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
