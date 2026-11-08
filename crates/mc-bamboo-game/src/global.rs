//! 全局状态：世界时间和已进入 Play 的玩家名单。
//!
//! 这里碰不到玩家和链路。方法只返回要广播的帧，由 [`World`](crate::World) 决定发给谁。

use std::collections::BTreeMap;

use mc_bamboo_protocol::{
    packet::{
        play::{GameMode, PlayerInfoEntry, PlayerInfoRemove, PlayerInfoUpdate, SystemChat},
        profile::ProfileProperty,
    },
    text::{Color, Component},
};
use uuid::Uuid;

use crate::{inbound::PlayerId, links::encode_frames};

/// 官方的条件是 `++counter > 600`，所以实际周期是 601 tick。
const LATENCY_BROADCAST_THRESHOLD: u32 = 600;

/// 世界时间。和 [`RegionTick`](crate::region::RegionTick) 同步增长，但不是一回事，不能互相比较。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct GameTime(pub(crate) i64);

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
        }
    }
}

/// 不属于任何区块的状态。
#[derive(Debug, Default)]
pub(crate) struct Global {
    time: GameTime,
    roster: BTreeMap<PlayerId, RosterEntry>,
    ticks_since_latency_broadcast: u32,
}

impl Global {
    pub(crate) fn time(&self) -> GameTime {
        self.time
    }

    pub(crate) fn advance_time(&mut self) {
        self.time.0 += 1;
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
