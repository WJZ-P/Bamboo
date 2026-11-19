//! 官方 `ServerPlayer.startSleepInBed` / `ServerLevel` 的睡眠统计。
//!
//! 方块侧（`AbstractBedBlock.useWithoutItem`）已经把床的 `occupied` 置位或者给出
//! 失败原因；这里管玩家睡姿、重生点、快捷栏提示、每刻的 `SleepStatus` 统计，
//! 以及人数够了之后跳到早上、清除天气、叫醒所有人。
//!
//! Official `ServerPlayer.startSleepInBed` / `ServerLevel` sleep bookkeeping.
//! Block logic (`AbstractBedBlock.useWithoutItem`) sets the bed's `occupied`
//! flag or reports the failure; this module owns the player's sleeping pose,
//! spawn point, action-bar messages, the per-tick `SleepStatus` and the
//! morning skip (clock marker, weather reset and waking everyone).

use mc_bamboo_blocks::BedProblem;
use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_data::gamerules;
use mc_bamboo_protocol::{
    packet::play::{EntityData, PlayerPosition, Pose as EntityPose, SetEntityData},
    text::Component,
};

use crate::{
    global_port::GlobalEffect, inbound::PlayerId, links::encode_frames, region::Region,
    world_state::RespawnData,
};

/// 官方 `Player.sleepCounter` 到 100 才算「睡够」。
const DEEP_SLEEP_TICKS: i32 = 100;
/// 官方 `minecraft:wake_up_from_sleep` 时间标记：0 刻，周期 24000。
const DAY_PERIOD: i64 = 24_000;
/// `ServerPlayer.isReachableBedBlock` 里 `Vec3.atBottomCenterOf` 的高度偏移
/// （床 VoxelShape 的上表面，`AbstractBedBlock.getSleepHeight`）。
///
/// The bed's shape top, used by `LivingEntity.setPosToBed`.
const BED_SLEEP_HEIGHT: f64 = 0.5625;

/// 一名玩家的睡眠状态，官方 `Player.sleepCounter` / `SLEEPING_POS_ID`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Sleeping {
    pub(crate) pos: BlockPos,
    pub(crate) counter: i32,
}

/// 官方 `SleepStatus`：`activePlayers`（不算旁观者）和 `sleepingPlayers`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct SleepStatus {
    active: i32,
    sleeping: i32,
    previous_active: i32,
    previous_sleeping: i32,
}

impl SleepStatus {
    /// 对拍用：直接按官方计数构造一份 `SleepStatus`。
    ///
    /// For the oracle: build a `SleepStatus` with the official counters.
    #[cfg(feature = "oracle-samples")]
    pub(crate) fn of_counts(active: i32, sleeping: i32) -> Self {
        Self {
            active,
            sleeping,
            previous_active: active,
            previous_sleeping: sleeping,
        }
    }

    /// `SleepStatus.sleepersNeeded(percentage)`：
    /// `max(1, ceil(activePlayers * percentage / 100))`。
    pub(crate) fn sleepers_needed(&self, percentage: i32) -> i32 {
        let scaled = (self.active as f32 * percentage as f32 / 100.0).ceil() as i32;
        scaled.max(1)
    }

    pub(crate) fn enough_sleeping(&self, percentage: i32) -> bool {
        self.sleeping >= self.sleepers_needed(percentage)
    }

    /// 正在熟睡（`isSleepingLongEnough`）的人数。
    pub(crate) fn deep_sleeping(region: &Region, percentage: i32) -> bool {
        let status = &region.sleep;
        let deep = region
            .players
            .values()
            .filter(|player| player.sleep.is_some_and(|s| s.counter >= DEEP_SLEEP_TICKS))
            .count() as i32;
        deep >= status.sleepers_needed(percentage)
    }

    /// `SleepStatus.update(players)`：重数活跃 / 睡觉的人数，返回名单有没有变。
    /// 单独传玩家表，免得和 `Region.sleep` 的可变借用撞上。
    ///
    /// Takes the player table separately so it does not clash with the
    /// mutable borrow of `Region.sleep`.
    pub(crate) fn update(
        &mut self,
        players: &std::collections::BTreeMap<PlayerId, crate::player::Player>,
    ) -> bool {
        self.previous_active = self.active;
        self.previous_sleeping = self.sleeping;
        self.active = 0;
        self.sleeping = 0;
        for player in players.values() {
            if player.game_mode == mc_bamboo_protocol::packet::play::GameMode::Spectator {
                continue;
            }
            self.active += 1;
            if player.sleep.is_some() {
                self.sleeping += 1;
            }
        }
        (self.previous_sleeping > 0 || self.sleeping > 0)
            && (self.previous_active != self.active || self.previous_sleeping != self.sleeping)
    }

    /// `wakeUpAllPlayers` 开头把计数器清零。
    pub(crate) fn clear(&mut self) {
        self.sleeping = 0;
        self.previous_sleeping = 0;
    }
}

/// `Effect::SleepInBed`：方块逻辑把结果交给游戏层。
///
/// `Effect::SleepInBed`: block logic hands its result to the game layer.
pub(crate) fn on_sleep_in_bed(
    region: &mut Region,
    id: PlayerId,
    pos: BlockPos,
    set_spawn: bool,
    problem: Option<BedProblem>,
) {
    let Some(player) = region.players.get(&id) else {
        return;
    };
    if player.sleep.is_some() {
        return;
    }
    let percentage = region
        .global
        .rules
        .int(gamerules::PLAYERS_SLEEPING_PERCENTAGE);
    let mut messages: Vec<Component> = Vec::new();
    if let Some(problem) = problem {
        // `BedRule.asProblem` 的快捷栏提示，和 `Either.ifLeft` 的顺序一致。
        messages.push(Component::translate(problem.message(), []));
    } else {
        // `canSetSpawn` 为真时 `setRespawnPosition(RespawnConfig, true)`：
        // 位置变了发一条 `block.minecraft.set_spawn`。
        if set_spawn {
            let spawn = RespawnData::of(pos, player.yaw, player.pitch);
            let changed = player.respawn != Some(spawn);
            if changed {
                messages.push(Component::translate("block.minecraft.set_spawn", []));
            }
            region.players.get_mut(&id).expect("玩家还在").respawn = Some(spawn);
        }
        // `LivingEntity.startSleeping`：把玩家挪到床上、换成睡姿。
        let sleep_pos = Vec3::new(
            f64::from(pos.x) + 0.5,
            f64::from(pos.y) + BED_SLEEP_HEIGHT,
            f64::from(pos.z) + 0.5,
        );
        let player = region.players.get_mut(&id).expect("玩家还在");
        player.set_position(&mut region.player_grid, sleep_pos);
        player.sync.sleeping = true;
        player.sleep = Some(Sleeping { pos, counter: 0 });
        // `ServerPlayer.startSleeping` 的连接层 teleport：给这名玩家发位置包。
        region.links.send(
            id,
            &PlayerPosition {
                teleport_id: 0,
                x: sleep_pos.x,
                y: sleep_pos.y,
                z: sleep_pos.z,
                yaw: player.yaw,
                pitch: player.pitch,
                relative: 0,
            },
        );
        // 睡姿同时发给所有看得到他的人。
        let frames = encode_frames(&SetEntityData {
            entity_id: id.0,
            items: &[EntityData::Pose(EntityPose::Sleeping)],
        });
        player.sync.send_to_viewers(&mut region.links, &frames);
        region.links.send_frames(id, &frames);
    }
    if percentage > 100 {
        // `ServerLevel.canSleepThroughNights()` 为假时官方无条件发这条提示。
        messages.push(Component::translate("sleep.not_possible", []));
    }
    for message in &messages {
        crate::chat::send_system_message(
            region,
            id,
            message,
            problem.is_some() || percentage <= 100,
        );
    }
    // `ServerLevel.updateSleepingPlayerList()` 在 `startSleepInBed` 末尾。
    update_sleeping_player_list(region);
}

/// `ServerLevel.updateSleepingPlayerList`：人数变了才发 `sleep.players_sleeping`。
pub(crate) fn update_sleeping_player_list(region: &mut Region) {
    if region.players.is_empty() {
        return;
    }
    let changed = {
        let status = &mut region.sleep;
        status.update(&region.players)
    };
    if !changed {
        return;
    }
    announce_sleep_status(region);
}

/// `ServerLevel.announceSleepStatus`：多人服（非单人）按 `N/M players sleeping` 广播。
fn announce_sleep_status(region: &mut Region) {
    let percentage = region
        .global
        .rules
        .int(gamerules::PLAYERS_SLEEPING_PERCENTAGE);
    if percentage > 100 {
        return;
    }
    let content = Component::translate(
        "sleep.players_sleeping",
        [
            region.sleep.sleeping.into(),
            region.sleep.sleepers_needed(percentage).into(),
        ],
    );
    let ids: Vec<_> = region.players.keys().copied().collect();
    for id in ids {
        crate::chat::send_system_message(region, id, &content, true);
    }
}

/// 官方 `ServerPlayer.doTick` 每刻把 `sleepCounter` 加到 100；`ServerLevel.tick`
/// 在天气 / 时间之前检查够不够人熟睡，够了就跳早、清天气、叫醒所有人。
///
/// Official `ServerPlayer.doTick` increments `sleepCounter`; `ServerLevel.tick`
/// checks the deep-sleeping count before the weather / time steps and skips
/// the night when enough players are asleep.
pub(crate) fn tick_sleep(region: &mut Region) -> Vec<GlobalEffect> {
    // `SleepStatus.update` 由 `updateSleepingPlayerList` 在每次名单变化时调用，
    // 这里每刻也重算一次，人数变了才广播。
    update_sleeping_player_list(region);
    for player in region.players.values_mut() {
        if let Some(sleep) = player.sleep.as_mut() {
            sleep.counter = (sleep.counter + 1).min(DEEP_SLEEP_TICKS);
        }
    }
    let percentage = region
        .global
        .rules
        .int(gamerules::PLAYERS_SLEEPING_PERCENTAGE);
    let enough =
        region.sleep.enough_sleeping(percentage) && SleepStatus::deep_sleeping(region, percentage);
    if !enough {
        return Vec::new();
    }
    wake_all(region);
    vec![GlobalEffect::SkipNight]
}

/// `ServerLevel.wakeUpAllPlayers`：清掉 `occupied`、改回站姿、补发位置包。
pub(crate) fn wake_all(region: &mut Region) {
    region.sleep.clear();
    let sleeping: Vec<(PlayerId, BlockPos, Vec3, f32, f32)> = region
        .players
        .iter()
        .filter_map(|(&id, player)| {
            player
                .sleep
                .map(|sleep| (id, sleep.pos, player.position(), player.yaw, player.pitch))
        })
        .collect();
    for (id, pos, position, yaw, pitch) in sleeping {
        // 只清自己那半床；形状更新会把另一半一起清掉。
        use mc_bamboo_blocks::{BlockState, World, state::OCCUPIED};
        let state = region
            .chunks
            .block(pos)
            .map(BlockState)
            .unwrap_or(BlockState::AIR);
        if state.get(OCCUPIED) {
            let (mut level, ticks) = region.block_level();
            let mut world = World::new(&mut level, ticks);
            world.set_block_and_update(pos, state.with(OCCUPIED, false));
        }
        let player = region.players.get_mut(&id).expect("玩家还在");
        player.sleep = None;
        player.sync.sleeping = false;
        let frames = encode_frames(&SetEntityData {
            entity_id: id.0,
            items: &[EntityData::Pose(EntityPose::Standing)],
        });
        player.sync.send_to_viewers(&mut region.links, &frames);
        region.links.send_frames(id, &frames);
        region.links.send(
            id,
            &PlayerPosition {
                teleport_id: 0,
                x: position.x,
                y: position.y,
                z: position.z,
                yaw,
                pitch,
                relative: 0,
            },
        );
    }
}

/// `player_command StopSleeping`：客户端请求起床。
pub(crate) fn stop_sleeping(region: &mut Region, id: PlayerId) {
    if region.players.get(&id).and_then(|p| p.sleep).is_none() {
        return;
    }
    wake_all(region);
    update_sleeping_player_list(region);
}

/// 官方 `ClockTimeMarker.resolveTimeToMoveTo`：`wake_up_from_sleep` 在 0 刻、
/// 周期 24000。字节码是 `total + durationToNext(period, total % period, 0)`，
/// 其中 Java 的 `%` 是截断取余，`durationToNext` 在差 ≤ 0 时加一整个周期，
/// 所以已经在标记上也会跳到下一个周期。
///
/// Official `ClockTimeMarker.resolveTimeToMoveTo` for `wake_up_from_sleep`
/// (tick 0, period 24000). The bytecode is
/// `total + durationToNext(period, total % period, 0)`; Java truncates `%`,
/// and `durationToNext` adds a whole period when the difference is ≤ 0, so
/// being exactly on the marker still advances to the next one.
pub(crate) fn wake_up_ticks(total: i64) -> i64 {
    let rem = total % DAY_PERIOD;
    let duration = -rem;
    let step = if duration > 0 {
        duration
    } else {
        DAY_PERIOD + duration
    };
    total + step
}

/// 供 `GlobalEffect::SkipNight` 使用：跳到早上并清天气。
pub(crate) fn skip_night(global: &mut crate::global::Global) {
    let next = wake_up_ticks(global.clocks()[0].1.total_ticks);
    global
        .clock_named_mut("minecraft:overworld")
        .expect("overworld clock")
        .set_total_ticks(next);
    // `ServerLevel.resetWeatherCycle`：把雨和雷暴的倒计时与开关都清零。
    let weather = &mut global.state_mut().weather;
    weather.rain_time = 0;
    weather.raining = false;
    weather.thunder_time = 0;
    weather.thundering = false;
}
