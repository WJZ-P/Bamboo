//! 全局状态：世界时间和已进入 Play 的玩家名单。
//!
//! 这里碰不到玩家和链路。方法只返回要广播的帧，由 [`World`](crate::World) 决定发给谁。

use std::collections::BTreeMap;

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
    inbound::PlayerId,
    links::encode_frames,
    world_state::{Difficulty, RespawnData, WorldState},
};

/// 官方的条件是 `++counter > 600`，所以实际周期是 601 tick。
const LATENCY_BROADCAST_THRESHOLD: u32 = 600;

/// 主世界时钟刚开服时停在正午。`/time` 的 `resume` 可以让它重新走起来。
///
/// The overworld clock starts paused at noon. `/time resume` can start it again.
pub(crate) const NOON_TICKS: i64 = 6000;

/// 世界时间。和 [`RegionTick`](crate::region::RegionTick) 同步增长，但不是一回事，不能互相比较。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct GameTime(pub(crate) i64);

/// 一个世界时钟的状态，官方 `ServerClockInstance`。
///
/// 26.x 起主世界和末地各有一个。主世界开服时停在正午，末地用 [`Clock::official`]。
///
/// Since 26.x the overworld and the End each have one. The overworld starts paused at noon;
/// the End uses [`Clock::official`].
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

impl Default for Clock {
    fn default() -> Self {
        Self {
            total_ticks: NOON_TICKS,
            partial_tick: 0.0,
            rate: 1.0,
            paused: true,
        }
    }
}

impl Clock {
    /// 官方新时钟的初值，不是 Bamboo 主世界那个停在正午的特例。
    ///
    /// The official initial clock, rather than Bamboo's overworld clock paused at noon.
    pub(crate) fn official() -> Self {
        Self {
            total_ticks: 0,
            partial_tick: 0.0,
            rate: 1.0,
            paused: false,
        }
    }

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

    /// 发给客户端的流速。暂停时是 0，官方的 `packNetworkState` 也是这样。
    ///
    /// The rate sent to the client. A paused clock sends 0, matching official `packNetworkState`.
    pub(crate) fn network_rate(&self) -> f32 {
        if self.paused { 0.0 } else { self.rate }
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
    /// 主世界时钟。开服时停在正午，和现有的 `set_time` 包一致。
    ///
    /// The overworld clock. It starts paused at noon, matching the existing `set_time` packet.
    clock: Clock,
    /// 末地时钟。官方 `ServerClockInstance` 的初值：0 刻、流速 1、没有暂停。
    ///
    /// The End clock. Official `ServerClockInstance` defaults: tick 0, rate 1, not paused.
    end_clock: Clock,
    roster: BTreeMap<PlayerId, RosterEntry>,
    ticks_since_latency_broadcast: u32,
    /// 游戏规则。不写存档，重启回到官方默认值。
    ///
    /// Game rules. They are not saved, so a restart restores the official defaults.
    rules: crate::gamerules::GameRuleSet,
    /// 难度、出生点、天气等世界设置。不写存档。
    ///
    /// Difficulty, spawn, weather and other world settings. Nothing is saved.
    state: WorldState,
}

impl Default for Global {
    fn default() -> Self {
        Self {
            time: GameTime::default(),
            clock: Clock::default(),
            end_clock: Clock::official(),
            roster: BTreeMap::new(),
            ticks_since_latency_broadcast: 0,
            rules: crate::gamerules::GameRuleSet::vanilla(),
            state: WorldState::default(),
        }
    }
}

impl Global {
    pub(crate) fn time(&self) -> GameTime {
        self.time
    }

    /// 权威的游戏规则。
    ///
    /// The authoritative game rules.
    pub(crate) fn rules(&self) -> &crate::gamerules::GameRuleSet {
        &self.rules
    }

    /// 权威的游戏规则，可写。
    ///
    /// The authoritative game rules, writable.
    pub(crate) fn rules_mut(&mut self) -> &mut crate::gamerules::GameRuleSet {
        &mut self.rules
    }

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
        encode_frames(&ChangeDifficulty {
            difficulty: self.state.difficulty as u8,
            locked: self.state.difficulty_locked,
        })
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

    pub(crate) fn advance_time(&mut self) {
        self.time.0 += 1;
        self.clock.tick();
        // 末地时钟和主世界一起走。登录时的 `set_time` 仍然只带主世界。
        //
        // The End clock advances with the overworld. The login `set_time` packet still carries
        // only the overworld.
        self.end_clock.tick();
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

fn update_frames(actions: u8, entries: &[PlayerInfoEntry<'_>]) -> Option<Vec<u8>> {
    (!entries.is_empty()).then(|| encode_frames(&PlayerInfoUpdate { actions, entries }))
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

    #[test]
    fn latency_period_with_nobody_online_sends_nothing() {
        let mut global = Global::default();
        for _ in 0..1000 {
            assert_eq!(global.tick_latency_broadcast(), None);
        }
    }
}
