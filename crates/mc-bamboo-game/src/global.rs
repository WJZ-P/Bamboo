//! 全局状态：世界时间和已进入 Play 的玩家名单。
//!
//! 这里碰不到玩家和链路。方法只返回要广播的帧，由 [`World`](crate::World) 决定发给谁。

use std::{collections::BTreeMap, sync::Arc};

use mc_bamboo_blocks::{BlockGameRules, BlockWeather, WorldBorder};
use mc_bamboo_protocol::{
    packet::{
        play::{
            ChangeDifficulty, GameMode, PlayerInfoEntry, PlayerInfoRemove, PlayerInfoUpdate,
            SetDefaultSpawnPosition, SystemChat,
        },
        profile::ProfileProperty,
    },
    text::{Color, Component},
};
use uuid::Uuid;

use crate::{
    gamerules::GameRuleSet,
    inbound::PlayerId,
    links::encode_frames,
    region::Region,
    scoreboard::Scoreboard,
    world_state::{Difficulty, RespawnData, Weather, WorldState},
};

/// 官方的条件是 `++counter > 600`，所以实际周期是 601 tick。
const LATENCY_BROADCAST_THRESHOLD: u32 = 600;

/// 世界时间。写进 `level.dat` 的 `Time`，开服从那里恢复（官方
/// `PrimaryLevelData.parse` 的 `gameTime`）。和
/// [`RegionTick`](crate::region::RegionTick) 同步增长，但不是一回事，不能互相比较。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct GameTime(pub(crate) i64);

/// 一个世界时钟的状态，官方 `ServerClockInstance`；字段和
/// `ClockState.CODEC`（`world_clocks.dat`）一一对应。
///
/// 26.x 起主世界和末地各有一个。两者都用 [`Clock::default`] 的官方初值
/// （0 刻、流速 1、不暂停）；老存档由 `world_clocks.dat` 覆盖。
///
/// Since 26.x the overworld and the End each have one. Both use the official
/// [`Clock::default`] (tick 0, rate 1, running); a saved `world_clocks.dat`
/// overwrites them at startup.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Clock {
    pub(crate) total_ticks: i64,
    /// 不足一整刻的部分。流速不是整数时靠它攒出下一整刻。
    ///
    /// The fraction of a tick. A non-integer rate accumulates the next whole tick here.
    pub(crate) partial_tick: f32,
    pub(crate) rate: f32,
    pub(crate) paused: bool,
}

/// 官方 `ServerClockInstance` 的构造器：四个字段都是 Java 的缺省值。
///
/// The official `ServerClockInstance` constructor: all four fields are Java's defaults.
impl Default for Clock {
    fn default() -> Self {
        Self {
            total_ticks: 0,
            partial_tick: 0.0,
            rate: 1.0,
            paused: false,
        }
    }
}

impl Clock {
    /// 官方 `ServerClockInstance.tick`。暂停时什么都不做。
    ///
    /// Official `ServerClockInstance.tick`. A paused clock does nothing.
    pub(crate) fn tick(&mut self) {
        if self.paused {
            return;
        }
        self.partial_tick += self.rate;
        let full = self.partial_tick.floor();
        self.partial_tick -= full;
        self.total_ticks += full as i64;
    }

    /// 官方 `setTotalTicks`：不足一整刻的部分清零。
    ///
    /// Official `setTotalTicks`: the partial tick is cleared.
    pub(crate) fn set_total_ticks(&mut self, ticks: i64) {
        self.total_ticks = ticks;
        self.partial_tick = 0.0;
    }

    /// 官方 `addTicks`：加上这些刻，结果不低于 0，不足一整刻的部分保留。
    ///
    /// Official `addTicks`: add this many ticks, clamp at 0, and keep the partial tick.
    pub(crate) fn add_ticks(&mut self, ticks: i64) {
        self.total_ticks = (self.total_ticks + ticks).max(0);
    }

    /// 发给客户端的流速。官方 `packNetworkState` 在时钟暂停或
    /// `advance_time` 规则为假时发 0，客户端才不会自己把时间往前推。
    ///
    /// The rate sent to the client. Official `packNetworkState` sends 0 while
    /// the clock is paused or `advance_time` is false, so the client does not
    /// advance time on its own.
    pub(crate) fn network_rate(&self, advance_time: bool) -> f32 {
        if self.paused || !advance_time {
            0.0
        } else {
            self.rate
        }
    }

    /// 官方 `setPaused`。
    ///
    /// Official `setPaused`.
    pub(crate) fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    /// 官方 `setRate`。
    ///
    /// Official `setRate`.
    pub(crate) fn set_rate(&mut self, rate: f32) {
        self.rate = rate;
    }
}

/// 名单上的一名玩家。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RosterEntry {
    pub(crate) uuid: Uuid,
    pub(crate) name: String,
    /// 皮肤等属性，由 session server 或者代理给出。名单上的人都要带着，
    /// 别人才能看到真实皮肤。
    pub(crate) properties: Vec<ProfileProperty>,
    pub(crate) game_mode: GameMode,
    /// 毫秒。
    pub(crate) latency: i32,
    pub(crate) show_hat: bool,
    /// 验过的聊天会话。进服时还没有，等 `chat_session_update` 之后再单独广播。
    ///
    /// Validated chat session. Join has none; `chat_session_update` broadcasts it afterwards.
    pub(crate) chat_session: Option<crate::secure_chat::SessionUpdate>,
}

impl RosterEntry {
    fn tab_entry(&self) -> PlayerInfoEntry<'_> {
        PlayerInfoEntry {
            uuid: self.uuid,
            name: &self.name,
            properties: &self.properties,
            game_mode: self.game_mode,
            listed: true,
            latency: self.latency,
            display_name: None,
            list_order: 0,
            show_hat: self.show_hat,
            chat_session: self.chat_session.as_ref().map(|session| {
                mc_bamboo_protocol::packet::play::ChatSessionView {
                    session_id: session.session_id,
                    expires_at_millis: session.expires_at_millis,
                    public_key: &session.public_key,
                    key_signature: &session.key_signature,
                }
            }),
        }
    }
}

/// 不属于任何区块的状态。`WorldState` 里的随机序列没有 `Debug`。
pub(crate) struct Global {
    time: GameTime,
    /// 主世界时钟。官方初值；老存档从 `world_clocks.dat` 恢复。
    ///
    /// The overworld clock, with official defaults; a saved world restores
    /// `world_clocks.dat`.
    clock: Clock,
    /// 末地时钟，和主世界用同一个官方初值。
    ///
    /// The End clock, with the same official defaults.
    end_clock: Clock,
    roster: BTreeMap<PlayerId, RosterEntry>,
    /// 官方 `MinecraftServer.tickCount`：每次开服从 0 数起，不存档。
    /// `tickServer` 在 `tickChildren` 之前 `++tickCount`，Bamboo 在 `World::tick`
    /// 开头做同样的事。不能拿 `GameTime` 代替：读档时 `level.dat` 的 `Time`
    /// 接着上次的走，和 `tickCount` 的相位一般不同。
    ///
    /// Official `MinecraftServer.tickCount`: starts at 0 every launch and is
    /// not saved. `tickServer` does `++tickCount` before `tickChildren`;
    /// Bamboo does the same at the start of `World::tick`. It cannot be
    /// replaced by `GameTime`, which resumes from `level.dat`'s `Time`.
    server_ticks: i32,
    /// 测试专用：故意按 `gameTime` 判断周期同步（旧的错相位），改坏确认。
    #[cfg(test)]
    pub(crate) time_sync_break_by_game_time: bool,
    ticks_since_latency_broadcast: u32,
    /// 游戏规则。按官方 `GameRuleMap.CODEC` 写进 `data/minecraft/game_rules.dat`，
    /// 启动时读回。和 [`GlobalView`] 共享，改的时候才复制一份。
    ///
    /// Game rules. Saved to `data/minecraft/game_rules.dat` with the official
    /// `GameRuleMap.CODEC` and read back at startup. Shared with [`GlobalView`];
    /// copied only when written.
    rules: Arc<GameRuleSet>,
    /// 难度、出生点、天气等世界设置。难度、默认游戏模式、出生点写进 `level.dat` 并读回；
    /// 天气、种子、随机序列不存档。
    ///
    /// Difficulty, spawn, weather and other world settings. Difficulty, default
    /// game mode and spawn are saved to `level.dat` and read back; weather, seed
    /// and random sequences are not saved.
    state: WorldState,
    /// 这一刻主控决定跳过夜晚；发布给每个岛，让它们在自己一刻开头叫醒玩家。
    /// 每刻开头重置，只在跳早的那一刻为真。
    ///
    /// The master decided to skip the night this tick; published to every
    /// island so each wakes its players at the start of its own tick. Reset
    /// at the start of each tick.
    pub(crate) sleep_skip: bool,
    /// 全服一份的计分板和队伍。不写存档。和 [`GlobalView`] 共享，改的时候才复制一份。
    ///
    /// The one scoreboard and team set for the whole server. Never saved. Shared
    /// with [`GlobalView`]; copied only when written.
    scoreboard: Arc<Scoreboard>,
    /// 全服一份的自定义 Boss 栏。不写存档。
    ///
    /// The one set of custom boss bars for the whole server. Never saved.
    boss_bars: crate::scoreboard::BossBars,
    /// 主世界的边界。官方按维度各有一份，这里按设计稿 6.6 放在 `Global`（全局状态）：
    /// 同一个维度以后拆成多个 region 时它们要看到同一份，region 每刻只读一份快照。
    /// 权威副本写进按维度存放的 `world_border.dat`（`export_settings` / `restore`，
    /// `restore` 传存档里的游戏时间），启动时读回。
    ///
    /// The overworld's world border. Vanilla keeps one per dimension; here it lives on
    /// `Global` (§6.6 global state) so future regions of the same dimension share it,
    /// each reading a snapshot every tick. It is saved to the per-dimension
    /// `world_border.dat` (`export_settings` / `restore` with the saved game time)
    /// and read back at startup.
    world_border: WorldBorder,
    /// OP、封禁和白名单的权威副本。世界线程是唯一写者。
    pub(crate) admin: crate::admin::AdminState,
    /// 名单变更后发布给连接层并写盘的通道。没有连接层时（单元测试）是 `None`。
    pub(crate) admin_messages: Option<std::sync::mpsc::Sender<crate::admin::AdminMessage>>,
    /// 命令压进来的存档请求，由 `World` 在 tick 里消费（命令动作不碰 IO）。
    pub(crate) save_requests: Vec<crate::save_session::SaveCommand>,
    /// 自动保存开关的权威镜像（命令改它，`World` 同步到存档会话）。
    pub(crate) save_auto: bool,
    /// 官方 `CommandStorage`：`/data storage` 的全服 NBT。不写存档。
    ///
    /// Official `CommandStorage`: the global `/data storage` NBT. Not saved.
    pub(crate) command_storage: crate::command::data_source::CommandStorage,
    /// `/random` 和 `/loot` 的按名随机序列（官方 `ServerLevel.getRandomSequences()`，全世界
    /// 一份）。按世界种子建，不存档。岛的实施计划 3.3「岛的随机源」第 2 条：放在主控上，
    /// 用它的命令归全局类，所以多岛时同名序列也只有一条。
    ///
    /// The named random sequences of `/random` and `/loot` (vanilla
    /// `ServerLevel.getRandomSequences()`, one per world), built from the world
    /// seed and not saved. Island plan 3.3 item 2: they live on the master and
    /// the commands using them are global, so a name has one sequence even with
    /// several islands.
    pub(crate) random_sequences: mc_bamboo_loot::RandomSequences,
    /// 多岛时天气自己的随机源（自然推进和 `/weather` 不给时长时取数，计划 3.3「岛的随机源」
    /// 第 4 条、8.2 第 6 条）。`None` 是只有一个岛：天气照官方用那个岛的 `Level.random`。
    ///
    /// The weather's own random source with several islands (the cycle and
    /// `/weather` without a duration draw from it; plan 3.3 item 4, 8.2 item 6).
    /// `None` with one island: the weather uses that island's `Level.random`
    /// as vanilla does.
    pub(crate) weather_random: Option<mc_bamboo_blocks::random::LevelRandom>,
    /// 权威的世界生成参数（种子、世界类型、flat 预设、结构开关）。启动时定下来，
    /// 发布到岛上的全局快照，之后整局不变。
    ///
    /// The authoritative world generation parameters (seed, level type, flat
    /// preset, structures). Fixed at startup and published to the islands'
    /// global snapshot; it never changes during a session.
    pub(crate) world_gen: crate::world_options::WorldGenConfig,
}

impl Default for Global {
    fn default() -> Self {
        Self {
            time: GameTime::default(),
            clock: Clock::default(),
            end_clock: Clock::default(),
            roster: BTreeMap::new(),
            server_ticks: 0,
            #[cfg(test)]
            time_sync_break_by_game_time: false,
            ticks_since_latency_broadcast: 0,
            rules: Arc::new(GameRuleSet::vanilla()),
            state: WorldState::default(),
            sleep_skip: false,
            scoreboard: Arc::default(),
            boss_bars: crate::scoreboard::BossBars::default(),
            world_border: WorldBorder::default(),
            admin: crate::admin::AdminState::default(),
            admin_messages: None,
            save_requests: Vec::new(),
            save_auto: true,
            command_storage: crate::command::data_source::CommandStorage::default(),
            random_sequences: mc_bamboo_loot::RandomSequences::new(crate::world_state::WORLD_SEED),
            weather_random: None,
            world_gen: crate::world_options::WorldGenConfig::default(),
        }
    }
}

impl Global {
    pub(crate) fn time(&self) -> GameTime {
        self.time
    }

    /// 开服从 `level.dat` 的 `Time` 恢复游戏时间（`PrimaryLevelData.parse` 的 `gameTime`）。
    /// 在任何 tick 之前调用，岛的时钟随后从这个值起算。
    ///
    /// Restore the game time from `level.dat`'s `Time` at startup (vanilla's
    /// `PrimaryLevelData.parse` `gameTime`). Called before any tick; the island
    /// clock then starts from this value.
    pub(crate) fn restore_game_time(&mut self, time: i64) {
        self.time = GameTime(time);
    }

    /// 开服从 `world_clocks.dat` 恢复一个世界时钟（`ServerClockInstance.loadFrom`）。
    ///
    /// Restore one world clock from `world_clocks.dat` at startup
    /// (`ServerClockInstance.loadFrom`).
    pub(crate) fn restore_clock(&mut self, name: &str, state: Clock) {
        if let Some(clock) = self.clock_named_mut(name) {
            *clock = state;
        }
    }

    /// 两份世界时钟，按注册名。`world_clocks.dat` 的编码从这里读。
    ///
    /// Both world clocks keyed by registry name; the `world_clocks.dat`
    /// encoder reads them.
    pub(crate) fn clocks(&self) -> [(&'static str, Clock); 2] {
        [
            ("minecraft:overworld", self.clock),
            ("minecraft:the_end", self.end_clock),
        ]
    }

    /// 权威的游戏规则。
    ///
    /// The authoritative game rules.
    pub(crate) fn rules(&self) -> &GameRuleSet {
        &self.rules
    }

    /// 权威的游戏规则，可写。改完要调用 [`publish`](Self::publish)，岛才看得到。
    ///
    /// The authoritative game rules, writable. Call [`publish`](Self::publish)
    /// afterwards so the island sees the change.
    pub(crate) fn rules_mut(&mut self) -> &mut GameRuleSet {
        Arc::make_mut(&mut self.rules)
    }

    /// 全服的计分板和队伍。
    ///
    /// The server-wide scoreboard and teams.
    pub(crate) fn scoreboard(&self) -> &Scoreboard {
        &self.scoreboard
    }

    /// [`scoreboard`](Self::scoreboard) 的可变版本。快照还拿着同一份时先复制，
    /// 所以只在真的要写的时候调用。
    ///
    /// Mutable [`scoreboard`](Self::scoreboard). Copies first while a snapshot
    /// still shares it, so call it only when actually writing.
    pub(crate) fn scoreboard_mut(&mut self) -> &mut Scoreboard {
        Arc::make_mut(&mut self.scoreboard)
    }

    /// 全服的自定义 Boss 栏。
    ///
    /// The server-wide custom boss bars.
    pub(crate) fn boss_bars(&self) -> &crate::scoreboard::BossBars {
        &self.boss_bars
    }

    /// [`boss_bars`](Self::boss_bars) 的可变版本。
    ///
    /// Mutable [`boss_bars`](Self::boss_bars).
    pub(crate) fn boss_bars_mut(&mut self) -> &mut crate::scoreboard::BossBars {
        &mut self.boss_bars
    }

    /// 权威的世界边界。
    ///
    /// The authoritative world border.
    pub(crate) fn world_border(&self) -> &WorldBorder {
        &self.world_border
    }

    /// 权威的世界边界，可写。命令改完要调用 [`publish`](Self::publish)。
    ///
    /// The authoritative world border, writable. Call [`publish`](Self::publish)
    /// after a command changes it.
    pub(crate) fn world_border_mut(&mut self) -> &mut WorldBorder {
        &mut self.world_border
    }

    /// 官方 `ServerLevel.tick` 开头的 `getWorldBorder().tick()`。
    ///
    /// Vanilla `getWorldBorder().tick()` at the start of `ServerLevel.tick`.
    pub(crate) fn tick_world_border(&mut self) {
        self.world_border.tick();
    }

    /// 岛读的只读快照（岛的实施计划第 6 节第 5a 步）。
    ///
    /// The read-only snapshot the island reads (island plan, section 6, step 5a).
    pub(crate) fn view(&self) -> GlobalView {
        GlobalView {
            time: self.time,
            clock: self.clock,
            end_clock: self.end_clock,
            block_rules: self.rules.block_snapshot(),
            send_command_feedback: self
                .rules
                .bool(mc_bamboo_data::gamerules::SEND_COMMAND_FEEDBACK),
            log_admin_commands: self
                .rules
                .bool(mc_bamboo_data::gamerules::LOG_ADMIN_COMMANDS),
            command_blocks_work: self
                .rules
                .bool(mc_bamboo_data::gamerules::COMMAND_BLOCKS_WORK),
            rules: Arc::clone(&self.rules),
            weather: self.state.weather,
            block_weather: self.state.weather.snapshot(),
            sky_darken: self.sky_darken(),
            villager_activity: self.villager_activity(),
            baby_villager_activity: self.baby_villager_activity(),
            sleep_skip: self.sleep_skip,
            world_border: self.world_border.clone(),
            difficulty: self.state.difficulty,
            difficulty_locked: self.state.difficulty_locked,
            spawn: self.state.spawn,
            command_storage: self.command_storage.clone(),
            default_game_mode: self.state.default_game_mode,
            seed: self.state.seed,
            world_gen: self.world_gen.clone(),
            idle_timeout_minutes: self.admin.idle_timeout_minutes,
            scoreboard: Arc::clone(&self.scoreboard),
        }
    }

    /// 官方 `Level.updateSkyBrightness()`：按当前主世界时钟和天气渐变强度算天色。
    ///
    /// `ServerLevel.tick` 在天气推进之后、`tickTime` 和方块刻之前调一次；这里
    /// 每次发快照时重算，等价于在那一刻用最新状态调用它。
    ///
    /// Official `Level.updateSkyBrightness()`: derive the sky darkening from the
    /// current overworld clock and smoothed weather levels. Vanilla calls it
    /// once per `ServerLevel.tick`, after the weather step and before `tickTime`
    /// and block ticks; recomputing at every publish is equivalent.
    pub(crate) fn sky_darken(&self) -> i32 {
        let weather = &self.state.weather;
        let (rain, thunder) = mc_bamboo_blocks::sky::weather_levels(
            weather.o_rain_level,
            weather.rain_level,
            weather.o_thunder_level,
            weather.thunder_level,
        );
        mc_bamboo_blocks::sky::sky_darken(self.clock.total_ticks, rain, thunder)
    }

    /// 成年村民当前日程活动（`villager_schedule.json`），和 `sky_darken` 同路径发布给各岛。
    ///
    /// Current adult villager scheduled activity (`villager_schedule.json`), published
    /// to islands along the same path as `sky_darken`.
    pub(crate) fn villager_activity(&self) -> mc_bamboo_blocks::entity::mob::brain::Activity {
        mc_bamboo_blocks::sky::villager_activity(self.clock.total_ticks)
    }

    /// 幼年村民当前日程活动（`villager_schedule.json`），和 `sky_darken` 同路径发布给各岛。
    ///
    /// Current baby villager scheduled activity (`villager_schedule.json`), published
    /// to islands along the same path as `sky_darken`.
    pub(crate) fn baby_villager_activity(&self) -> mc_bamboo_blocks::entity::mob::brain::Activity {
        mc_bamboo_blocks::sky::baby_villager_activity(self.clock.total_ticks)
    }

    /// 把快照发布给岛。主控每刻在固定的几处发布（开头、边界推进后、天气和时间推进后）；
    /// 只有一个岛、直接改 `Global` 的时候，每次改完也当场重发，所以岛读到的永远是最新的，
    /// 行为和直接读 `Global` 逐位相同。
    ///
    /// Publish the snapshot to the island. The master publishes at fixed points
    /// each tick (start, after the border step, after weather and time); while
    /// there is one island writing `Global` directly, every write republishes at
    /// once, so the island always reads the latest state, bit for bit like
    /// reading `Global` itself.
    pub(crate) fn publish(&self, region: &mut Region) {
        let view = self.view();
        // 结构刷怪索引是按世界生成配置（种子 / 预设 / 超平坦设置）算的纯函数结果；
        // 配置换掉时整张清空，下一次查询按新配置重算。
        //
        // The structure spawn index is a pure function of the world-generation
        // config; clear it when the config changes so the next query rebuilds it.
        if view.world_gen != region.global.world_gen {
            *region.structure_spawn_index.borrow_mut() =
                crate::structure_spawn::StructureSpawnIndex::default();
        }
        region.global = view;
    }

    #[cfg(test)]
    pub(crate) fn clock(&self) -> Clock {
        self.clock
    }

    /// 权威的世界设置。
    ///
    /// The authoritative world settings.
    pub(crate) fn state(&self) -> &WorldState {
        &self.state
    }

    /// 权威的世界设置，可写。
    ///
    /// The authoritative world settings, writable.
    pub(crate) fn state_mut(&mut self) -> &mut WorldState {
        &mut self.state
    }

    /// 官方 `MinecraftServer.sendDifficultyUpdate`：给所有人发当前难度和锁定标志。
    ///
    /// Official `MinecraftServer.sendDifficultyUpdate`: broadcast the current difficulty
    /// and lock flag to everyone.
    pub(crate) fn difficulty_frames(&self) -> Vec<u8> {
        difficulty_frames(self.state.difficulty, self.state.difficulty_locked)
    }

    /// 当前难度的显示名，官方 `Difficulty.getDisplayName()` 的翻译键。
    pub(crate) fn difficulty_name(&self) -> String {
        self.state.difficulty.display_name()
    }

    pub(crate) fn set_difficulty(&mut self, difficulty: Difficulty) {
        self.state.difficulty = difficulty;
    }

    /// 官方 `MinecraftServer.setRespawnData`：只在真的变化时广播出生点包。
    pub(crate) fn set_spawn(&mut self, spawn: RespawnData) -> Vec<u8> {
        if self.state.spawn == spawn {
            return Vec::new();
        }
        self.state.spawn = spawn;
        encode_frames(&SetDefaultSpawnPosition {
            dimension: crate::region::DIMENSION,
            pos: spawn.pos,
            yaw: spawn.yaw,
            pitch: spawn.pitch,
        })
    }

    /// 按注册名取时钟。只有主世界和末地，别的名字返回 `None`。
    ///
    /// Looks up a clock by registry name. Only the overworld and the End exist.
    pub(crate) fn clock_named(&self, name: &str) -> Option<&Clock> {
        match name {
            "minecraft:overworld" => Some(&self.clock),
            "minecraft:the_end" => Some(&self.end_clock),
            _ => None,
        }
    }

    /// [`clock_named`](Self::clock_named) 的可变版本。
    ///
    /// Mutable [`clock_named`](Self::clock_named).
    pub(crate) fn clock_named_mut(&mut self, name: &str) -> Option<&mut Clock> {
        match name {
            "minecraft:overworld" => Some(&mut self.clock),
            "minecraft:the_end" => Some(&mut self.end_clock),
            _ => None,
        }
    }

    /// 官方 `MinecraftServer.tickChildren` 的 "clocks" 阶段和
    /// `ServerLevel.tickTime`：游戏时间总在加，世界时钟只在 `advance_time` 为真时走。
    ///
    /// Official `MinecraftServer.tickChildren`'s "clocks" phase and
    /// `ServerLevel.tickTime`: the game time always advances; the world clocks
    /// tick only while `advance_time` is true.
    /// 官方 `MinecraftServer.tickChildren` 的 "clocks" 阶段：所有世界时钟在
    /// `ServerLevel.tick` 之前推进，只在 `advance_time` 为真时走。
    ///
    /// Official `MinecraftServer.tickChildren`'s "clocks" phase: all world
    /// clocks tick before `ServerLevel.tick`, gated by `advance_time`.
    pub(crate) fn tick_clocks(&mut self) {
        if !self.rules.bool(mc_bamboo_data::gamerules::ADVANCE_TIME) {
            return;
        }
        self.clock.tick();
        self.end_clock.tick();
    }

    /// 官方 `ServerLevel.tickTime`：游戏时间每刻照常加一。
    ///
    /// Official `ServerLevel.tickTime`: the game time always advances.
    pub(crate) fn increment_game_time(&mut self) {
        self.time.0 += 1;
    }

    /// 时钟和游戏时间同一步推进；对拍重放和单元测试用。
    /// Advance clocks and game time together; used by replay and unit tests.
    #[cfg(any(test, feature = "oracle-samples"))]
    pub(crate) fn advance_time(&mut self) {
        self.tick_clocks();
        self.increment_game_time();
    }

    /// 官方 `MinecraftServer.tickServer` 开头的 `++tickCount`：每刻加一，
    /// 开服从 0 起、不存档。`World::tick` 开头调用。
    ///
    /// Official `MinecraftServer.tickServer`'s `++tickCount` at the start of
    /// each tick: from 0 on every launch, never saved. Called from `World::tick`.
    pub(crate) fn increment_server_ticks(&mut self) {
        self.server_ticks = self.server_ticks.wrapping_add(1);
    }

    /// 开服以来的刻数；单元测试断言周期相位用。
    #[cfg(test)]
    pub(crate) fn server_ticks(&self) -> i32 {
        self.server_ticks
    }

    /// 官方 `MinecraftServer.tickChildren`：`tickCount % 20 == 0` 时给所有人发一次
    /// 只带 `gameTime`、时钟列表为空的 `set_time`。发包在 `ServerLevel.tick` 之前，
    /// 包里是这一刻 `tickTime` 加一之前的 `gameTime`（读档时它未必和 `tickCount`
    /// 有固定相位，所以要分开记）。
    ///
    /// Official `MinecraftServer.tickChildren`: when `tickCount % 20 == 0`,
    /// broadcast a `set_time` carrying only `gameTime` with an empty clock map.
    /// It goes out before `ServerLevel.tick`, so the packet carries this
    /// tick's not-yet-incremented `gameTime` (which has no fixed phase
    /// relation to `tickCount` when a save was loaded).
    pub(crate) fn time_sync_packet_time(&self) -> Option<GameTime> {
        // 改坏确认：回到按 `gameTime` 判断的旧实现。
        #[cfg(test)]
        let due = if self.time_sync_break_by_game_time {
            (self.time.0 + 1) % 20 == 0
        } else {
            self.server_ticks % 20 == 0
        };
        #[cfg(not(test))]
        let due = self.server_ticks % 20 == 0;
        due.then_some(self.time)
    }

    /// 名单上的所有人，发给刚进入的玩家。名单是空的时候返回 `None`。
    pub(crate) fn tab_list(&self) -> Option<Vec<u8>> {
        let entries: Vec<PlayerInfoEntry<'_>> =
            self.roster.values().map(RosterEntry::tab_entry).collect();
        update_frames(PlayerInfoUpdate::ALL_ACTIONS, &entries)
    }

    /// 把玩家加入名单，返回发给所有人的 Tab 列表更新。
    pub(crate) fn add_player(&mut self, id: PlayerId, entry: RosterEntry) -> Vec<u8> {
        let frames = encode_frames(&PlayerInfoUpdate {
            actions: PlayerInfoUpdate::ALL_ACTIONS,
            entries: &[entry.tab_entry()],
        });
        self.roster.insert(id, entry);
        frames
    }

    /// 只带 `INITIALIZE_CHAT` 的名单更新。官方 `resetPlayerChatState` 广播的就是这一个动作。
    ///
    /// A tab update that only carries `INITIALIZE_CHAT`. That is what official `resetPlayerChatState` broadcasts.
    pub(crate) fn initialize_chat(
        &mut self,
        id: PlayerId,
        session: &crate::secure_chat::SessionUpdate,
    ) -> Option<Vec<u8>> {
        let entry = self.roster.get_mut(&id)?;
        entry.chat_session = Some(session.clone());
        let view = entry.tab_entry();
        Some(encode_frames(&PlayerInfoUpdate {
            actions: PlayerInfoUpdate::INITIALIZE_CHAT,
            entries: &[view],
        }))
    }

    /// 把玩家从名单上删掉，返回发给所有人的 Tab 列表更新。不在名单上时返回 `None`。
    pub(crate) fn remove_player(&mut self, id: PlayerId) -> Option<Vec<u8>> {
        let entry = self.roster.remove(&id)?;
        Some(encode_frames(&PlayerInfoRemove {
            profile_ids: &[entry.uuid],
        }))
    }

    /// 名单里这名玩家的名字。
    ///
    /// This player's name in the roster.
    pub(crate) fn roster_name(&self, id: PlayerId) -> Option<&str> {
        self.roster.get(&id).map(|entry| entry.name.as_str())
    }

    /// 这名玩家的加入消息。
    pub(crate) fn joined_message(&self, id: PlayerId) -> Option<Vec<u8>> {
        self.player_message(id, "multiplayer.player.joined")
    }

    /// 这名玩家的离开消息。
    pub(crate) fn left_message(&self, id: PlayerId) -> Option<Vec<u8>> {
        self.player_message(id, "multiplayer.player.left")
    }

    fn player_message(&self, id: PlayerId, key: &str) -> Option<Vec<u8>> {
        let entry = self.roster.get(&id)?;
        let content = Component::translate(key, [entry.name.as_str().into()]).color(Color::Yellow);
        Some(encode_frames(&SystemChat {
            content: &content,
            overlay: false,
        }))
    }

    /// 记下心跳算出的新延迟。下一次周期广播时才发出去。
    pub(crate) fn set_latency(&mut self, id: PlayerId, millis: i32) {
        if let Some(entry) = self.roster.get_mut(&id) {
            entry.latency = millis;
        }
    }

    /// 更新帽子的显示。真的变了时返回发给所有人的 Tab 列表更新。
    pub(crate) fn set_hat(&mut self, id: PlayerId, show_hat: bool) -> Option<Vec<u8>> {
        let entry = self.roster.get_mut(&id)?;
        if entry.show_hat == show_hat {
            return None;
        }
        entry.show_hat = show_hat;
        update_frames(PlayerInfoUpdate::UPDATE_HAT, &[entry.tab_entry()])
    }

    /// 更新名单上的游戏模式，返回发给所有人的 Tab 列表更新。`/gamemode` 靠它让别的玩家看到新模式。
    ///
    /// Updates the roster game mode and returns the tab-list update for everyone.
    /// `/gamemode` uses this so other players see the new mode.
    pub(crate) fn set_game_mode(&mut self, id: PlayerId, game_mode: GameMode) -> Option<Vec<u8>> {
        let entry = self.roster.get_mut(&id)?;
        if entry.game_mode == game_mode {
            return None;
        }
        entry.game_mode = game_mode;
        update_frames(PlayerInfoUpdate::UPDATE_GAME_MODE, &[entry.tab_entry()])
    }

    /// 每 tick 调用一次，到周期时返回所有人的延迟。
    pub(crate) fn tick_latency_broadcast(&mut self) -> Option<Vec<u8>> {
        self.ticks_since_latency_broadcast += 1;
        if self.ticks_since_latency_broadcast <= LATENCY_BROADCAST_THRESHOLD {
            return None;
        }
        self.ticks_since_latency_broadcast = 0;
        let entries: Vec<PlayerInfoEntry<'_>> =
            self.roster.values().map(RosterEntry::tab_entry).collect();
        update_frames(PlayerInfoUpdate::UPDATE_LATENCY, &entries)
    }
}

/// 主控发布给岛的全局状态只读快照（岛的实施计划第 2 节、第 6 节第 5a 步）。
/// 岛里读规则、天气、时间和时钟、边界、难度、出生点、默认模式、种子哈希、闲置时限和
/// 计分板的地方都读这一份，不碰 `Global`。规则和计分板是共享的，发布只是加引用计数。
/// 方块逻辑读的几项（`block_rules`、`block_weather`、`world_border` 和三个命令布尔）
/// 是热路径上的现成副本，和权威副本同时发布。
///
/// The read-only snapshot of global state the master publishes to the island
/// (island plan, sections 2 and 6, step 5a). Island code reading rules, weather,
/// time and clock, the border, difficulty, spawn, default mode, seed hash, idle
/// timeout and the scoreboard reads this, never `Global`. Rules and the
/// scoreboard are shared, so publishing only bumps reference counts. The few
/// items block logic reads (`block_rules`, `block_weather`, `world_border` and
/// the three command booleans) are ready-made hot-path copies published along.
#[derive(Clone)]
pub(crate) struct GlobalView {
    pub(crate) time: GameTime,
    /// 主世界时钟。
    pub(crate) clock: Clock,
    /// 末地时钟，整表同步时和主世界一起发。
    pub(crate) end_clock: Clock,
    pub(crate) rules: Arc<GameRuleSet>,
    /// 方块逻辑读的那几条规则（挤压上限恒为 0，实体逻辑另取一份带配置的快照）。
    pub(crate) block_rules: BlockGameRules,
    /// `GameRules.SEND_COMMAND_FEEDBACK`。
    pub(crate) send_command_feedback: bool,
    /// `GameRules.LOG_ADMIN_COMMANDS`。
    pub(crate) log_admin_commands: bool,
    /// `GameRules.COMMAND_BLOCKS_WORK`。
    pub(crate) command_blocks_work: bool,
    pub(crate) weather: Weather,
    /// 方块逻辑读的天气。
    pub(crate) block_weather: BlockWeather,
    /// `Level.getSkyDarken()`：按官方 `updateSkyBrightness` 从主世界时钟和天气算出，
    /// 每刻随全局快照一起发布。精确模式的 `World`、并行的实体视图和两种驱动读同一份。
    ///
    /// `Level.getSkyDarken()`: computed from the overworld clock and weather as
    /// vanilla's `updateSkyBrightness` does, published with the global snapshot
    /// every tick. The exact-mode `World`, the parallel entity view and both
    /// drives read this one value.
    pub(crate) sky_darken: i32,
    /// `EnvironmentAttributes.VILLAGER_ACTIVITY` / `BABY_VILLAGER_ACTIVITY`：
    /// 由主控按主世界时钟和 `villager_schedule.json` 关键帧算出，最多旧一刻，与 `sky_darken` 同路径。
    ///
    /// `EnvironmentAttributes.VILLAGER_ACTIVITY` / `BABY_VILLAGER_ACTIVITY`: computed by
    /// the master from the overworld clock and `villager_schedule.json`, at most one tick
    /// stale, on the same path as `sky_darken`.
    pub(crate) villager_activity: mc_bamboo_blocks::entity::mob::brain::Activity,
    pub(crate) baby_villager_activity: mc_bamboo_blocks::entity::mob::brain::Activity,
    /// 主控这一刻决定跳过夜晚；岛在自己一刻开头据此叫醒玩家。
    ///
    /// The master decided to skip the night this tick; islands wake their
    /// players at the start of their tick when this is set.
    pub(crate) sleep_skip: bool,
    /// 这一维度的世界边界。方块逻辑、移动校验和实体碰撞都读这一份。
    pub(crate) world_border: WorldBorder,
    pub(crate) difficulty: Difficulty,
    pub(crate) difficulty_locked: bool,
    pub(crate) spawn: RespawnData,
    /// 官方 `CommandStorage` 的只读快照；权威副本在 `Global`。
    pub(crate) command_storage: crate::command::data_source::CommandStorage,
    pub(crate) default_game_mode: GameMode,
    /// 世界种子。哈希要算 SHA-256，只在进服时用到，所以发布时不算。
    pub(crate) seed: i64,
    /// 一局的世界生成参数。`ServerLevel.isFlat()` / `ChunkGenerator.getSeaLevel()`
    /// 和落脚点搜索都读它；存档里的 `world_gen_settings.dat` 比配置文件优先。
    ///
    /// This session's world generation parameters. `ServerLevel.isFlat()`,
    /// `ChunkGenerator.getSeaLevel()` and the spawn landing search all read it;
    /// a saved `world_gen_settings.dat` wins over the config file.
    pub(crate) world_gen: crate::world_options::WorldGenConfig,
    /// `/setidletimeout` 的分钟数，0 表示不踢。
    pub(crate) idle_timeout_minutes: i32,
    pub(crate) scoreboard: Arc<Scoreboard>,
}

impl Default for GlobalView {
    /// 新建的 `Global` 的快照，和发布第一份之前岛上原来的初值一致。
    fn default() -> Self {
        Global::default().view()
    }
}

impl GlobalView {
    /// 官方 `MinecraftServer.sendDifficultyUpdate` 的那一包。
    pub(crate) fn difficulty_frames(&self) -> Vec<u8> {
        difficulty_frames(self.difficulty, self.difficulty_locked)
    }

    /// `GameRules.ADVANCE_TIME`，发给客户端的时钟流速要跟着它归零。
    ///
    /// `GameRules.ADVANCE_TIME`: the network clock rate becomes zero when it is off.
    pub(crate) fn time_advances(&self) -> bool {
        self.rules.bool(mc_bamboo_data::gamerules::ADVANCE_TIME)
    }

    /// 快照里的两份世界时钟，按注册名。进服和规则变化时的整表同步从这里读。
    ///
    /// Both world clocks in this snapshot, keyed by registry name. Full syncs
    /// on join and game-rule changes read from here.
    pub(crate) fn clocks(&self) -> [(&'static str, Clock); 2] {
        [
            ("minecraft:overworld", self.clock),
            ("minecraft:the_end", self.end_clock),
        ]
    }

    /// 写进 `login` 包的种子哈希（官方 `BiomeManager.obfuscateSeed`）。
    pub(crate) fn hashed_seed(&self) -> i64 {
        crate::world_state::hashed_seed(self.seed)
    }
}

fn difficulty_frames(difficulty: Difficulty, locked: bool) -> Vec<u8> {
    encode_frames(&ChangeDifficulty {
        difficulty: difficulty as u8,
        locked,
    })
}

fn update_frames(actions: u8, entries: &[PlayerInfoEntry<'_>]) -> Option<Vec<u8>> {
    (!entries.is_empty()).then(|| encode_frames(&PlayerInfoUpdate { actions, entries }))
}

/// `clock-oracle` 的逐刻重放：用与 `Global::advance_time` / `Clock::tick` /
/// `Global::sky_darken` 完全相同的产品实现，把官方场景里的输入重放一遍。
/// 只在 `oracle-samples` feature 下编进库里。
///
/// The `clock-oracle` replay: run the official scene's inputs through the very
/// same production `Global::advance_time` / `Clock::tick` / `Global::sky_darken`
/// implementation. Gated behind the `oracle-samples` feature.
#[cfg(feature = "oracle-samples")]
pub(crate) fn replay_clock_scene(scene: &crate::ClockReplayScene) -> Vec<crate::ClockReplayStep> {
    let mut global = Global::default();
    global.rules_mut().assign(
        mc_bamboo_data::gamerules::ADVANCE_TIME,
        if scene.advance_time { "true" } else { "false" },
    );
    for clock in [&mut global.clock, &mut global.end_clock] {
        clock.total_ticks = scene.start_ticks;
        clock.partial_tick = 0.0;
        clock.rate = scene.rate;
        clock.paused = scene.paused;
    }
    {
        let weather = &mut global.state.weather;
        weather.o_rain_level = scene.o_rain;
        weather.rain_level = scene.rain;
        weather.o_thunder_level = scene.o_thunder;
        weather.thunder_level = scene.thunder;
    }
    let mut steps = Vec::with_capacity((scene.ticks + 1) as usize);
    for tick in 0..=scene.ticks {
        // 官方场景在某刻执行 `/time set`；和命令实现一样清掉不足一整刻的部分。
        if scene.jumps.iter().any(|jump| i64::from(*jump) == tick) {
            global.clock.set_total_ticks(tick);
        }
        if scene.rising_rain && tick > 0 {
            let weather = &mut global.state.weather;
            weather.o_rain_level = (scene.rain + 0.01 * (tick - 1) as f32).min(1.0);
            weather.rain_level = (scene.rain + 0.01 * tick as f32).min(1.0);
            weather.o_thunder_level = 0.0;
            weather.thunder_level = 0.0;
        }
        steps.push(crate::ClockReplayStep {
            tick,
            overworld_ticks: global.clock.total_ticks,
            overworld_partial: global.clock.partial_tick,
            overworld_rate: global.clock.rate,
            overworld_paused: global.clock.paused,
            end_ticks: global.end_clock.total_ticks,
            end_partial: global.end_clock.partial_tick,
            end_rate: global.end_clock.rate,
            end_paused: global.end_clock.paused,
            sky_darken: global.sky_darken(),
        });
        // 官方 `MinecraftServer.tickChildren` 的时钟阶段在这一刻末尾。
        global.advance_time();
    }
    steps
}

#[cfg(test)]
mod tests {
    use mc_bamboo_protocol::codec::Reader;

    use super::*;

    const STEVE: PlayerId = PlayerId(7);
    const STEVE_UUID: Uuid = Uuid::from_u128(0x069a79f4_44e9_4726_a5be_fca90e38aaf5);

    fn steve() -> RosterEntry {
        RosterEntry {
            uuid: STEVE_UUID,
            name: "Steve".to_owned(),
            properties: Vec::new(),
            game_mode: GameMode::Creative,
            latency: 42,
            show_hat: true,
            chat_session: None,
        }
    }

    /// 帧头、包 ID 之后的内容。
    fn body(frames: &[u8]) -> Vec<u8> {
        let mut reader = Reader::new(frames);
        reader.varint().unwrap();
        reader.varint().unwrap();
        reader.rest().to_vec()
    }

    fn roster_with_steve() -> Global {
        let mut global = Global::default();
        global.add_player(STEVE, steve());
        global
    }

    #[test]
    fn new_player_goes_out_with_all_eight_actions() {
        let frames = Global::default().add_player(STEVE, steve());

        let mut expected = vec![0xff, 0x01];
        expected.extend_from_slice(STEVE_UUID.as_bytes());
        expected.extend_from_slice(&[5, b'S', b't', b'e', b'v', b'e']);
        // 没有属性、没有聊天会话、创造、显示、延迟 42、没有显示名、顺序 0、显示帽子。
        expected.extend_from_slice(&[0x00, 0x00, 0x01, 0x01, 0x2a, 0x00, 0x00, 0x01]);
        assert_eq!(body(&frames), expected);
    }

    /// 皮肤跟着新玩家的 Tab 列表条目一起广播出去，别人才看得到。
    #[test]
    fn a_new_player_brings_their_skin() {
        let mut entry = steve();
        entry.properties = vec![ProfileProperty {
            name: "textures".to_owned(),
            value: "ewo=".to_owned(),
            signature: None,
        }];
        let frames = Global::default().add_player(STEVE, entry);

        let mut expected = vec![0xff, 0x01];
        expected.extend_from_slice(STEVE_UUID.as_bytes());
        expected.extend_from_slice(&[5, b'S', b't', b'e', b'v', b'e']);
        // 一项属性：名字 textures，值 ewo=，没有签名。
        expected.push(0x01);
        expected.extend_from_slice(&[8, b't', b'e', b'x', b't', b'u', b'r', b'e', b's']);
        expected.extend_from_slice(&[4, b'e', b'w', b'o', b'=']);
        expected.push(0x00);
        expected.extend_from_slice(&[0x00, 0x01, 0x01, 0x2a, 0x00, 0x00, 0x01]);
        assert_eq!(body(&frames), expected);
    }

    #[test]
    fn empty_roster_has_no_tab_list() {
        assert_eq!(Global::default().tab_list(), None);
        assert!(roster_with_steve().tab_list().is_some());
    }

    #[test]
    fn removing_a_player_twice_only_reports_it_once() {
        let mut global = roster_with_steve();

        let frames = global.remove_player(STEVE).expect("Steve 在名单上");
        let mut expected = vec![0x01];
        expected.extend_from_slice(STEVE_UUID.as_bytes());
        assert_eq!(body(&frames), expected);
        assert_eq!(global.remove_player(STEVE), None);
    }

    #[test]
    fn messages_are_yellow_translations() {
        let global = roster_with_steve();

        for (frames, key) in [
            (global.joined_message(STEVE), "multiplayer.player.joined"),
            (global.left_message(STEVE), "multiplayer.player.left"),
        ] {
            let content = Component::translate(key, ["Steve".into()]).color(Color::Yellow);
            let expected = encode_frames(&SystemChat {
                content: &content,
                overlay: false,
            });
            assert_eq!(frames, Some(expected));
        }
        assert_eq!(global.joined_message(PlayerId(8)), None);
    }

    #[test]
    fn hat_update_is_only_sent_when_the_hat_changes() {
        let mut global = roster_with_steve();

        assert_eq!(global.set_hat(STEVE, true), None);
        let frames = global.set_hat(STEVE, false).expect("帽子变了");
        let mut expected = vec![0x80, 0x01];
        expected.extend_from_slice(STEVE_UUID.as_bytes());
        expected.push(0x00);
        assert_eq!(body(&frames), expected);
        assert_eq!(global.set_hat(PlayerId(8), false), None);
    }

    #[test]
    fn latency_is_broadcast_every_601_ticks() {
        let mut global = roster_with_steve();
        global.set_latency(STEVE, 100);
        global.set_latency(PlayerId(8), 5);

        for period in 0..2 {
            for tick in 0..600 {
                assert_eq!(global.tick_latency_broadcast(), None, "{period}/{tick}");
            }
            let frames = global.tick_latency_broadcast().expect("到周期了");
            let mut expected = vec![0x10, 0x01];
            expected.extend_from_slice(STEVE_UUID.as_bytes());
            expected.extend_from_slice(&[0x64]);
            assert_eq!(body(&frames), expected);
        }
    }

    /// 官方 `ServerClockInstance` 的初值和 `packNetworkState` 的流速。
    #[test]
    fn fresh_clocks_follow_vanilla_and_freeze_with_the_game_rule() {
        let mut global = Global::default();
        let clock = global.clock();
        assert_eq!(clock.total_ticks, 0);
        assert_eq!(clock.partial_tick, 0.0);
        assert_eq!(clock.rate, 1.0);
        assert!(!clock.paused);
        assert_eq!(clock.network_rate(true), 1.0);
        assert_eq!(clock.network_rate(false), 0.0);
        assert_eq!(global.clocks()[1].1, clock, "末地时钟用同一个官方初值");

        // `advance_time` 为真时两份时钟都走，为假时都不走；游戏时间总在加。
        global.advance_time();
        assert_eq!(global.time().0, 1);
        assert_eq!(global.clock().total_ticks, 1);
        assert_eq!(global.clocks()[1].1.total_ticks, 1);
        global
            .rules_mut()
            .assign(mc_bamboo_data::gamerules::ADVANCE_TIME, "false");
        global.advance_time();
        assert_eq!(global.time().0, 2);
        assert_eq!(global.clock().total_ticks, 1);
        assert_eq!(global.clocks()[1].1.total_ticks, 1);
    }

    /// 天色从主世界时钟和天气算出；夜里、雨和雷暴都要变暗。
    #[test]
    fn sky_darken_follows_the_clock_and_weather() {
        let mut global = Global::default();
        assert_eq!(
            global.sky_darken(),
            mc_bamboo_blocks::sky::sky_darken(0, 0.0, 0.0)
        );
        global
            .clock_named_mut("minecraft:overworld")
            .unwrap()
            .total_ticks = 18_000;
        let night = global.sky_darken();
        assert!(night > 0, "夜里天色要变暗");
        let weather = &mut global.state.weather;
        weather.o_rain_level = 1.0;
        weather.rain_level = 1.0;
        weather.o_thunder_level = 1.0;
        weather.thunder_level = 1.0;
        let storm = global.sky_darken();
        assert!(storm > 0, "雷暴天色要变暗");
        // 快照给精确模式 / 并行视图的是同一份权威值。
        assert_eq!(global.view().sky_darken, storm);
    }

    #[test]
    fn latency_period_with_nobody_online_sends_nothing() {
        let mut global = Global::default();
        for _ in 0..1000 {
            assert_eq!(global.tick_latency_broadcast(), None);
        }
    }
}
