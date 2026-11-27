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
use mc_bamboo_data::{gamerules, registries::custom_stat};
use mc_bamboo_protocol::{
    packet::play::{EntityData, Pose as EntityPose, SetEntityData},
    text::Component,
};

use crate::{
    inbound::PlayerId,
    item_use_stats::{ScoreChange, ScoreKey},
    links::encode_frames,
    region::Region,
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

    pub(crate) fn active(&self) -> i32 {
        self.active
    }

    pub(crate) fn sleeping(&self) -> i32 {
        self.sleeping
    }

    /// `wakeUpAllPlayers` 开头把计数器清零。
    pub(crate) fn clear(&mut self) {
        self.sleeping = 0;
        self.previous_sleeping = 0;
    }
}

/// 一个岛在上一刻末尾交给主控的睡眠计数（官方 `SleepStatus` 的
/// `activePlayers` / `sleepingPlayers` 加上 `isSleepingLongEnough` 的人数）。
/// 主控把它们加起来，在下一刻开头、天气推进之后决定要不要跳早。
///
/// One island's sleep counts handed to the master at the end of the previous
/// tick (`SleepStatus.activePlayers` / `sleepingPlayers` plus the number of
/// `isSleepingLongEnough` players). The master sums them at the start of the
/// next tick, right after the weather step, to decide the morning skip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct SleepCounts {
    pub(crate) active: i32,
    pub(crate) sleeping: i32,
    pub(crate) deep: i32,
}

impl SleepCounts {
    /// 官方 `ServerLevel.tick`：`areEnoughSleeping(percentage)` 和
    /// `areEnoughDeepSleeping(percentage, players)` 都成立才跳早。
    ///
    /// Official `ServerLevel.tick`: skip only when both
    /// `areEnoughSleeping` and `areEnoughDeepSleeping` hold.
    pub(crate) fn should_skip(self, percentage: i32) -> bool {
        let status = SleepStatus::of_counts(self.active, self.sleeping);
        status.enough_sleeping(percentage) && self.deep >= status.sleepers_needed(percentage)
    }

    pub(crate) fn add(&mut self, other: Self) {
        self.active += other.active;
        self.sleeping += other.sleeping;
        self.deep += other.deep;
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
    // (消息, 是不是快捷栏提示)。`BedSleepingProblem` / `sleep.not_possible`
    // 走 `sendOverlayMessage`，`set_spawn` 是普通系统消息。
    let mut messages: Vec<(Component, bool)> = Vec::new();
    if let Some(problem) = problem {
        // `BedRule.asProblem` 的快捷栏提示，和 `Either.ifLeft` 的顺序一致。
        messages.push((Component::translate(problem.message(), []), true));
    } else {
        // `canSetSpawn` 为真时 `setRespawnPosition(RespawnConfig, true)`：
        // 位置变了发一条 `block.minecraft.set_spawn`。
        if set_spawn {
            let spawn = RespawnData::of(pos, player.yaw, player.pitch);
            let changed = player.respawn != Some(spawn);
            if changed {
                messages.push((Component::translate("block.minecraft.set_spawn", []), false));
            }
            region.players.get_mut(&id).expect("玩家还在").respawn = Some(spawn);
        }
        // `LivingEntity.startSleeping`：把玩家挪到床上、换成睡姿。
        let sleep_pos = Vec3::new(
            f64::from(pos.x) + 0.5,
            f64::from(pos.y) + BED_SLEEP_HEIGHT,
            f64::from(pos.z) + 0.5,
        );
        // `ServerPlayer.startSleeping` 的连接层 `teleport`：走正式的
        // `movement::teleport`，它会递增 teleport id 并记下等待客户端确认的位置。
        // 官方把玩家位置先交给 `LivingEntity.setPosToBed`，再在这里发传送包。
        let player = region.players.get_mut(&id).expect("玩家还在");
        player.sync.sleeping = true;
        player.sleep = Some(Sleeping { pos, counter: 0 });
        // `ServerPlayer.startSleeping`：`resetStat(TIME_SINCE_REST)` 清零。
        // `ServerPlayer.startSleeping`: `resetStat(TIME_SINCE_REST)` clears it.
        player.time_since_rest = 0;
        player.item_use_stats.score(
            ScoreKey::Custom(custom_stat::TIME_SINCE_REST),
            ScoreChange::Reset,
        );
        let now = region.tick;
        crate::movement::teleport(
            player,
            &mut region.links,
            &mut region.player_grid,
            now,
            sleep_pos,
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
        messages.push((Component::translate("sleep.not_possible", []), true));
    }
    for (message, overlay) in &messages {
        crate::chat::send_system_message(region, id, message, *overlay);
    }
    // `ServerLevel.updateSleepingPlayerList()` 在 `startSleepInBed` 末尾。
    update_sleeping_player_list(region);
    // 睡眠成功后触发；放在玩家借用结束之后，避免和传送的可变借用冲突。
    crate::advancement_triggers::record(
        region,
        id,
        "minecraft:slept_in_bed",
        serde_json::json!({"x": pos.x, "y": pos.y, "z": pos.z}),
    );
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

/// 官方 `ServerPlayer.doTick` 每刻把 `sleepCounter` 加到 100，并刷新
/// `SleepStatus`。岛在这一刻末尾把计数交给主控；跳早的判断在下一刻开头
/// （`World::advance_world_clock` 的天气之后、时间推进之前）用这份计数做，
/// 和官方 `ServerLevel.tick` 同一刻。
///
/// Official `ServerPlayer.doTick` increments `sleepCounter` and refreshes
/// `SleepStatus` each tick. The island hands the counts to the master at the
/// end of the tick; the master decides the skip at the start of the next tick
/// (after weather, before time), the same point as `ServerLevel.tick`.
pub(crate) fn tick_sleep_counters(region: &mut Region) {
    // `SleepStatus.update` 由 `updateSleepingPlayerList` 在每次名单变化时调用，
    // 这里每刻也重算一次，人数变了才广播。
    update_sleeping_player_list(region);
    for player in region.players.values_mut() {
        // `ServerPlayer.doTick`：`TIME_SINCE_REST` 在没睡觉的每一刻 +1。官方用
        // `awardStat` 累加到同一份 int 上；这里也按 int 环绕，和 `StatsCounter` 一致。
        // `ServerPlayer.doTick`: `TIME_SINCE_REST` gains 1 on every non-sleeping tick.
        // Vanilla accumulates into one int via `awardStat`; wrap the same way.
        if player.sleep.is_none() {
            player.time_since_rest = player.time_since_rest.wrapping_add(1);
        }
        if let Some(sleep) = player.sleep.as_mut() {
            sleep.counter = (sleep.counter + 1).min(DEEP_SLEEP_TICKS);
        }
    }
}

/// 主控在岛交回、停放之后读它上一刻的睡眠计数（不改状态）。
///
/// The master reads an island's sleep counts after hand-back, without
/// modifying any state.
pub(crate) fn counts_of(region: &Region) -> SleepCounts {
    SleepCounts {
        active: region.sleep.active(),
        sleeping: region.sleep.sleeping(),
        deep: region
            .players
            .values()
            .filter(|player| player.sleep.is_some_and(|s| s.counter >= DEEP_SLEEP_TICKS))
            .count() as i32,
    }
}

/// 岛在这一刻开头看到主控发布的跳早标志后叫醒自己的玩家。
///
/// An island wakes its players at the start of its tick when the master has
/// published the morning-skip flag.
pub(crate) fn wake_from_skip(region: &mut Region) {
    if region.global.sleep_skip {
        wake_all(region);
        update_sleeping_player_list(region);
    }
}

/// `ServerLevel.wakeUpAllPlayers`：清掉 `occupied`、改回站姿、补发位置包。
pub(crate) fn wake_all(region: &mut Region) {
    region.sleep.clear();
    let sleeping: Vec<(PlayerId, BlockPos, Vec3)> = region
        .players
        .iter()
        .filter_map(|(&id, player)| player.sleep.map(|sleep| (id, sleep.pos, player.position())))
        .collect();
    for (id, pos, position) in sleeping {
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
        // `ServerPlayer.stopSleepInBed` 末尾走连接层 `teleport`，和进服 / `/tp`
        // 共用递增的 teleport id 与等待确认状态。
        let now = region.tick;
        crate::movement::teleport(
            player,
            &mut region.links,
            &mut region.player_grid,
            now,
            position,
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

/// 主控在 `World::advance_world_clock` 里调用：跳到早上、清天气，并打上
/// 这一刻的 `sleep_skip` 标志让每个岛叫醒自己的玩家。
///
/// Called by the master in `World::advance_world_clock`: advance the clock,
/// clear the weather and publish this tick's `sleep_skip` flag so every island
/// wakes its players.
pub(crate) fn skip_night(
    global: &mut crate::global::Global,
    advance_time: bool,
    advance_weather: bool,
) -> bool {
    // `wakeUpAllPlayers` 无条件发生：主控发布这一刻的 `sleep_skip` 标志，
    // 每个岛在自己一刻开头叫醒玩家（见 `wake_from_skip`）。
    global.sleep_skip = true;
    // `ServerLevel.resetWeatherCycle`：`advance_weather` 为真且 `isRaining()`，
    // 和 `advance_time` 无关（字节码里是两个独立的条件）。
    if advance_weather && global.state().weather.is_raining() {
        let weather = &mut global.state_mut().weather;
        weather.rain_time = 0;
        weather.raining = false;
        weather.thunder_time = 0;
        weather.thundering = false;
    }
    // `ServerClockManager.moveToTimeMarker` 只在 `advance_time` 为真时执行；
    // 官方 `ServerLevel.tick` 在同一步之后才 `tickTime`（游戏时间 +1）。
    if !advance_time {
        return false;
    }
    let next = wake_up_ticks(global.clocks()[0].1.total_ticks);
    global
        .clock_named_mut("minecraft:overworld")
        .expect("overworld clock")
        .set_total_ticks(next);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_bamboo_core::Vec3;
    use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode};
    use uuid::Uuid;

    use crate::{inbound::PlayerId, player::Player, region::Region};

    const ID: PlayerId = PlayerId(1);

    fn region_with_player() -> Region {
        let mut region = Region::new(10, 20, false);
        region.insert_player(Player::new(
            ID,
            Uuid::from_u128(1),
            "Tester".to_owned(),
            GameMode::Survival,
            ClientInformation::default(),
            Vec3::new(8.5, 64.0, 8.5),
        ));
        region
    }

    #[test]
    fn time_since_rest_counts_awake_ticks_and_stops_while_sleeping() {
        let mut region = region_with_player();
        for _ in 0..5 {
            tick_sleep_counters(&mut region);
        }
        assert_eq!(region.players[&ID].time_since_rest, 5);

        // 睡觉的每一刻不再 +1；`startSleeping` 的清零见下一条。
        // Sleeping ticks do not add; the `startSleeping` reset is checked next.
        region.players.get_mut(&ID).unwrap().sleep = Some(Sleeping {
            pos: BlockPos::new(8, 64, 8),
            counter: 0,
        });
        for _ in 0..3 {
            tick_sleep_counters(&mut region);
        }
        assert_eq!(region.players[&ID].time_since_rest, 5);
    }

    #[test]
    fn starting_to_sleep_clears_time_since_rest() {
        let mut region = region_with_player();
        for _ in 0..7 {
            tick_sleep_counters(&mut region);
        }
        assert_eq!(region.players[&ID].time_since_rest, 7);
        on_sleep_in_bed(&mut region, ID, BlockPos::new(8, 64, 8), false, None);
        assert!(region.players[&ID].sleep.is_some());
        assert_eq!(region.players[&ID].time_since_rest, 0);
    }
}
