//! 玩家重生：原地重置 `Player`，发官方 `PlayerList.respawn` 的整套包。
//!
//! 官方重建 `ServerPlayer` 再 `restoreFrom(old, keepEverything)`；Bamboo
//! **只换状态、不换对象**，保留 `EntitySync.seen_by`、菜单、`player_grid`、
//! `links` 和岛里按玩家分的各张表。[`reset_after_respawn`] 是「重置哪些 /
//! 保留哪些」的唯一入口，对应官方 `ServerPlayer.<init>` + `restoreFrom`：
//! 以后若要改成重建对象，只改这一处和包序列。
//!
//! Vanilla rebuilds `ServerPlayer` and calls `restoreFrom`; Bamboo resets the
//! state in place, keeping the entity pairings, menus, grid, links and all
//! per-player island tables. [`reset_after_respawn`] is the single place that
//! decides what is reset and what is kept, mirroring the official constructor
//! plus `restoreFrom`.

use mc_bamboo_data::{gamerules, registries::dimension_type};
use mc_bamboo_protocol::{
    item::ItemStack,
    packet::play::{
        ChangeDifficulty, CommonPlayerSpawnInfo, ContainerSetContent, GameEvent, GameMode,
        InitializeBorder, PlayerPosition, Respawn, SetDefaultSpawnPosition, SetEntityData,
        SetEquipment, SetExperience, SetHeldSlot, SetTime, UpdateAttributes,
    },
};

use crate::{
    inbound::PlayerId,
    player::Player,
    player_health::{FoodData, PlayerHealth},
    region::{CREATIVE_ATTRIBUTES, DIMENSION, Region},
    world_state::RespawnData,
};

/// `keepInventory` 打开或死者是旁观者时，`restoreFrom` 会搬物品栏 / 经验 /
/// Score；否则这些字段由新 `ServerPlayer` 的构造函数给默认值。`enchantmentSeed`
/// 和 `enderChestInventory` 两种情况下都原样搬。
///
/// `restoreFrom` transfers inventory / XP / score only with `keepInventory`
/// or when the dead player is a spectator. The enchantment seed and ender chest
/// are always kept.
fn transfer_inventory_xp_and_score(player: &mut Player, keep: bool) {
    if keep {
        return;
    }
    player.inventory = crate::inventory::Inventory::default();
    player.experience = crate::experience::Experience::default();
    player.score = 0;
}

/// 官方 `ServerPlayer.<init>` + `restoreFrom(old, keepEverything=false)` 的等价物。
///
/// **重置**：生命 / 吸收 / 三个受伤倒计时 / 饥饿四字段（构造默认值）、
/// `dead` / `deathTime` / `combat_entries` / 击杀归属 / `in_combat` / `awaiting_respawn`、
/// `lastSent*` 三个同步哨兵、经验 / Score（非 keepInventory）、物品栏（非 keepInventory）、
/// 菜单光标 / 采矿 / 移动速度 / 传送确认、`takeXpDelay`、`recorded_criteria`、
/// `entity_ticks` 归零（新实体从 0 开始）。
///
/// **保留**：`id` / `uuid` / `name` / `game_mode` / 权限、客户端设置、位置与朝向、
/// `EntitySync`（含 `seen_by`、姿态元数据）、菜单表、`player_grid` 登记、`links`、
/// `inbox`、`random`、`enchantment_seed`、`ender_chest`、`respawn` 配置、
/// `last_death_location`（官方 restoreFrom 搬它）、`chat`、`spam`、
/// `last_action_millis` / `peer_ips`。`last_death_location` 同时写进重生包。
///
/// 所有新增字段都不是绝对时刻；`entity_ticks` 是相对 tick 数，整行随
/// `IslandPart::Players` 搬，岛拆合不需要时钟平移。
///
/// Equivalent of the official constructor + `restoreFrom(old, false)`.
/// Reset: health / absorption / three hurt countdowns / four food fields,
/// death flags, the three last-sent sentinels, XP / score / inventory (unless
/// keepInventory), cursor / mining / motion / teleport state, XP pickup delay,
/// scoreboard criteria and the relative entity tick counter.
/// Kept: identity, game mode, permissions, client settings, position and
/// rotation, `EntitySync` (including `seen_by`), menus, grid, links, inbox,
/// random, enchantment seed, ender chest, respawn config, last death location,
/// chat and throttlers. No absolute instants are stored.
pub(crate) fn reset_after_respawn(region: &mut Region, id: PlayerId, keep_inventory: bool) {
    let spectator = region
        .players
        .get(&id)
        .is_some_and(|p| p.game_mode == GameMode::Spectator);
    let keep = keep_inventory || spectator;
    let Some(player) = region.players.get_mut(&id) else {
        return;
    };
    player.health = PlayerHealth {
        food: FoodData::default(),
        ..PlayerHealth::default()
    };
    // 官方重建 `ServerPlayer` 再 `restoreFrom`：效果表是空的，
    // 重生序列里 `sendActivePlayerEffects` 因此不发效果包。
    // Vanilla rebuilds `ServerPlayer` before `restoreFrom`, so effects are empty.
    player.effects = mc_bamboo_blocks::entity::mob::effects::EffectMap::new();
    player.dead = false;
    player.awaiting_respawn = false;
    player.in_combat = false;
    player.combat_entries.clear();
    player.combat_idle_ticks = 0;
    player.hurt_by_player = None;
    player.hurt_by_mob = None;
    player.health_sync = crate::player_hurt::HealthSync::INITIAL;
    player.last_sent_exp = -1;
    player.take_xp_delay = 0;
    player.recorded_criteria = crate::scoreboard::UNRECORDED_CRITERIA;
    player.entity_ticks = 0;
    player.inventory_cursor = crate::menu::station::InventoryCursor::new();
    player.mining = crate::interaction::mining::Mining::default();
    player.prediction = crate::interaction::BlockPrediction::default();
    let position = player.position();
    player.motion = crate::player_move::Motion::new(position);
    player.teleport = crate::movement::Teleport::default();
    player.sync.metadata_dirty = true;
    transfer_inventory_xp_and_score(player, keep);
}

/// 官方 `PlayerList.respawn` 的等价物。把玩家放到重生点、发整套包。
/// 重生点可能在另一个岛：位置经 `place_player` / `move_player` 更新
/// `player_grid`，主控的 `relocate_players` 按玩家所在区块整行搬。
///
/// Equivalent of `PlayerList.respawn`: find the spawn, reset state in place
/// and send the whole packet sequence. A spawn on another island goes through
/// the existing `place_player` path and island relocation.
pub(crate) fn respawn(region: &mut Region, id: PlayerId) {
    let Some(player) = region.players.get(&id) else {
        return;
    };
    if !player.dead {
        return;
    }
    let keep_inventory = region.global.rules.bool(gamerules::KEEP_INVENTORY);
    let personal = player.respawn;
    let last_death = player.last_death_location;
    let game_mode = player.game_mode;
    let permission_level = player.permission_level;
    let hashed_seed = region.global.hashed_seed();
    let difficulty = region.global.difficulty as u8;
    let difficulty_locked = region.global.difficulty_locked;
    let border = region.global.world_border.clone();
    let weather = region.global.weather;
    let time = region.global.time;
    let clock = region.global.clock;
    let world_spawn = region.global.spawn;
    let (is_flat, sea_level) = crate::spawn_landing::search_from(region).level_flags();
    // 官方 `findRespawnAndUseSpawnBlock`：强制个人点的两格不能重生就回退到
    // 世界出生点 + `PlayerSpawnFinder.findSpawn`，并发 NO_RESPAWN_BLOCK_AVAILABLE。
    // 进服和重生的精确落点都由 `spawn_landing::landing` 算（同一个函数）。
    // Vanilla `findRespawnAndUseSpawnBlock`: a blocked forced personal spawn
    // falls back to worldSpawn + `PlayerSpawnFinder.findSpawn` and sends
    // NO_RESPAWN_BLOCK_AVAILABLE. Join and respawn share `landing`.
    let missing = crate::spawn_landing::personal_spawn_missing(region, personal);
    if missing && let Some(player) = region.players.get_mut(&id) {
        player.respawn = None;
    }
    let (position, spawn, ready) =
        match crate::spawn_landing::landing(region, personal, None, false) {
            Ok((landing, _)) => (
                landing.position,
                RespawnData::of(
                    mc_bamboo_core::BlockPos::new(
                        landing.position.x.floor() as i32,
                        landing.position.y.floor() as i32,
                        landing.position.z.floor() as i32,
                    ),
                    landing.yaw,
                    landing.pitch,
                ),
                true,
            ),
            Err(crate::spawn_landing::NotReady) => (
                crate::region::temporary_position(region),
                region.global.spawn,
                false,
            ),
        };
    reset_after_respawn(region, id, keep_inventory);
    let experience = region.players.get(&id).expect("玩家刚才还在").experience;

    // 官方 `PlayerList.respawn` 的 `connection.send` 顺序：
    // 1) 可选的 NO_RESPAWN_BLOCK_AVAILABLE（Bamboo 的出生点总是可用）
    // 2) Respawn(CommonPlayerSpawnInfo, dataToKeep=0)
    // 3) teleport → player_position
    // 4) set_default_spawn_position
    // 5) change_difficulty
    // 6) set_experience
    // 7) sendActivePlayerEffects：重生后效果表已清空，正常不发包。
    // 8) sendLevelInfo：initialize_border / set_time /
    //    set_default_spawn_position / 雨三个 game_event / game_event 13
    // 9) sendPlayerPermissionLevel（entity_event 权限 + 命令树）
    // 10) addRespawnedPlayer 的配对包、initInventoryMenu 的整套物品栏 /
    //     装备 / 属性 / 手持槽 / 能力包
    // 官方下一刻 `doTick` 再补 set_health / set_experience，这里不提前发。
    let info = CommonPlayerSpawnInfo {
        dimension_type: dimension_type::OVERWORLD,
        dimension: DIMENSION,
        hashed_seed,
        game_mode,
        previous_game_mode: None,
        is_debug: false,
        is_flat,
        last_death: last_death.map(|pos| (DIMENSION, pos)),
        portal_cooldown: 0,
        sea_level,
    };
    // `TeleportTransition.missingRespawnBlock` 的那一包在 Respawn 之前。
    // The `TeleportTransition.missingRespawnBlock` packet precedes Respawn.
    if missing {
        region.links.send(
            id,
            &GameEvent {
                event: GameEvent::NO_RESPAWN_BLOCK_AVAILABLE,
                param: 0.0,
            },
        );
    }
    // `ServerPlayer.restoreFrom` 里 `onUpdateAbilities` 在 Respawn 包之前发。
    region
        .links
        .send(id, &crate::region::abilities_for(game_mode));
    region.links.send(
        id,
        &Respawn {
            info,
            data_to_keep: 0,
        },
    );
    // `ServerGamePacketListenerImpl.teleport`：绝对坐标、绝对朝向、速度 0。
    // 搜索覆盖的区块没就绪时这一包等 `finish_pending` 补发。
    // `ServerGamePacketListenerImpl.teleport`: absolute position / rotation,
    // zero velocity. When the covered chunks are not ready `finish_pending`
    // sends this packet later.
    if ready {
        region.links.send(
            id,
            &PlayerPosition {
                teleport_id: 1,
                x: position.x,
                y: position.y,
                z: position.z,
                yaw: spawn.yaw,
                pitch: spawn.pitch,
                relative: 0,
            },
        );
    }
    let spawn_packet = SetDefaultSpawnPosition {
        dimension: DIMENSION,
        pos: world_spawn.pos,
        yaw: world_spawn.yaw,
        pitch: world_spawn.pitch,
    };
    region.links.send(id, &spawn_packet);
    region.links.send(
        id,
        &ChangeDifficulty {
            difficulty,
            locked: difficulty_locked,
        },
    );
    region.links.send(
        id,
        &SetExperience {
            experience_progress: experience.progress,
            experience_level: experience.level,
            total_experience: experience.total,
        },
    );
    // `PlayerList.sendActivePlayerEffects`（第 7 步）。重生后为空，
    // 但保留调用点：改成功效保留规则时这里会按官方补发。
    crate::player_effects::send_active(region, id);

    // `PlayerList.sendLevelInfo`。
    region.links.send(
        id,
        &InitializeBorder {
            center_x: border.center_x(),
            center_z: border.center_z(),
            old_size: border.size(),
            new_size: border.lerp_target(),
            lerp_time: border.lerp_time(),
            absolute_max_size: border.absolute_max_size(),
            warning_blocks: border.warning_blocks(),
            warning_time: border.warning_time(),
        },
    );
    region.links.send(
        id,
        &SetTime {
            game_time: time.0,
            clock: mc_bamboo_data::registries::world_clock::OVERWORLD,
            clock_ticks: clock.total_ticks,
            clock_partial: clock.partial_tick,
            clock_rate: clock.network_rate(),
        },
    );
    region.links.send(id, &spawn_packet);
    if weather.is_raining() {
        region.links.send(
            id,
            &GameEvent {
                event: GameEvent::START_RAINING,
                param: 0.0,
            },
        );
        region.links.send(
            id,
            &GameEvent {
                event: GameEvent::RAIN_LEVEL_CHANGE,
                param: weather.rain_level,
            },
        );
        region.links.send(
            id,
            &GameEvent {
                event: GameEvent::THUNDER_LEVEL_CHANGE,
                param: weather.rain_level * weather.thunder_level,
            },
        );
    }
    region.links.send(
        id,
        &GameEvent {
            event: GameEvent::LEVEL_CHUNKS_LOAD_START,
            param: 0.0,
        },
    );
    crate::command::send_session(&mut region.links, id, permission_level, id.0);
    send_pairing_packets(region, id);

    // 位置和朝向走和 `/tp` 相同的移动路径，更新 player_grid；
    // `player_position` 已在上面发过，这里不再发第二包。跨岛搬运由主控
    // 按玩家所在区块完成，不能直接改坐标。
    crate::command::place_player_at_respawn(region, id, position, spawn.yaw, spawn.pitch);
    if !ready {
        region
            .players
            .get_mut(&id)
            .expect("玩家刚才还在")
            .spawn_pending = Some(crate::spawn_landing::PendingLanding::respawn(personal));
    }
}

/// 官方重建玩家后 `ChunkMap.addEntity` 的 `sendPairingData` 和
/// `initInventoryMenu`：元数据、属性、装备、整套物品栏、手持槽、能力。
///
/// Pairing data of the newly added player plus `initInventoryMenu`.
fn send_pairing_packets(region: &mut Region, id: PlayerId) {
    let Some(player) = region.players.get(&id) else {
        return;
    };
    // 官方 `ServerEntity.sendPairingData` 从 `entityData.packDirty` 起，
    // 新玩家全部元数据都算脏：Health(20)、主手、模型层。
    let items = [
        mc_bamboo_protocol::packet::play::EntityData::Health(20.0),
        mc_bamboo_protocol::packet::play::EntityData::MainHand(player.client.main_hand),
        mc_bamboo_protocol::packet::play::EntityData::ModelCustomisation(
            player.client.model_customisation,
        ),
    ];
    region.links.send(
        id,
        &SetEntityData {
            entity_id: id.0,
            items: &items,
        },
    );
    if player.game_mode == GameMode::Creative {
        region.links.send(
            id,
            &UpdateAttributes {
                entity_id: id.0,
                attributes: &CREATIVE_ATTRIBUTES,
            },
        );
    }
    let empty = ItemStack::empty();
    let stacks = player.equipment_stacks(&empty);
    let slots = [
        mc_bamboo_protocol::packet::play::EquipmentSlot::MainHand,
        mc_bamboo_protocol::packet::play::EquipmentSlot::OffHand,
        mc_bamboo_protocol::packet::play::EquipmentSlot::Feet,
        mc_bamboo_protocol::packet::play::EquipmentSlot::Legs,
        mc_bamboo_protocol::packet::play::EquipmentSlot::Chest,
        mc_bamboo_protocol::packet::play::EquipmentSlot::Head,
    ];
    let equipment: Vec<_> = slots.iter().copied().zip(stacks.iter().copied()).collect();
    if let Some(packet) = SetEquipment::new(id.0, &equipment) {
        region.links.send(id, &packet);
    }
    let mut inventory = player.inventory.slots.clone();
    // `InventoryMenu.sendAllDataToRemote`：46 个槽 + 光标。
    if inventory.len() < 46 {
        inventory.resize(46, ItemStack::empty());
    }
    let carried = ItemStack::empty();
    region.links.send(
        id,
        &ContainerSetContent {
            container_id: 0,
            state_id: 0,
            items: &inventory[..46],
            carried: &carried,
        },
    );
    region.links.send(
        id,
        &SetHeldSlot {
            slot: i32::from(player.inventory.selected),
        },
    );
    region
        .links
        .send(id, &crate::region::abilities_for(player.game_mode));
    // 下一刻 `player_hurt::tick` 会按 lastSent* 哨兵补 set_health，
    // `experience::sync_dirty` 补 set_experience，和官方 `doTick` 一致。
}
