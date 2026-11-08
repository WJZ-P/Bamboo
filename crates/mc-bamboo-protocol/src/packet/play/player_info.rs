//! Tab 列表。所有在线玩家都在每个客户端的列表里，和距离无关。

use mc_bamboo_data::packets::play::clientbound;
use uuid::Uuid;

use super::GameMode;
use crate::{
    codec::{write_bool, write_string, write_uuid, write_varint},
    packet::{
        ClientboundPacket,
        profile::{ProfileProperty, write_profile_properties},
    },
    text::Component,
};

/// Tab 列表里的一名玩家。实际写哪些字段由 [`PlayerInfoUpdate::actions`] 决定。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerInfoEntry<'a> {
    pub uuid: Uuid,
    pub name: &'a str,
    /// 皮肤等属性。别人的客户端靠这些字段显示皮肤，离线模式没有属性。
    pub properties: &'a [ProfileProperty],
    pub game_mode: GameMode,
    pub listed: bool,
    /// 毫秒。
    pub latency: i32,
    pub display_name: Option<&'a Component>,
    pub list_order: i32,
    pub show_hat: bool,
}

impl PlayerInfoEntry<'_> {
    fn write(&self, actions: u8, out: &mut Vec<u8>) {
        write_uuid(out, self.uuid);
        if actions & PlayerInfoUpdate::ADD_PLAYER != 0 {
            write_string(out, self.name);
            write_profile_properties(out, self.properties);
        }
        if actions & PlayerInfoUpdate::INITIALIZE_CHAT != 0 {
            // 聊天会话。离线模式没有。
            write_bool(out, false);
        }
        if actions & PlayerInfoUpdate::UPDATE_GAME_MODE != 0 {
            write_varint(out, self.game_mode as i32);
        }
        if actions & PlayerInfoUpdate::UPDATE_LISTED != 0 {
            write_bool(out, self.listed);
        }
        if actions & PlayerInfoUpdate::UPDATE_LATENCY != 0 {
            write_varint(out, self.latency);
        }
        if actions & PlayerInfoUpdate::UPDATE_DISPLAY_NAME != 0 {
            write_bool(out, self.display_name.is_some());
            if let Some(display_name) = self.display_name {
                display_name.write(out);
            }
        }
        if actions & PlayerInfoUpdate::UPDATE_LIST_ORDER != 0 {
            write_varint(out, self.list_order);
        }
        if actions & PlayerInfoUpdate::UPDATE_HAT != 0 {
            write_bool(out, self.show_hat);
        }
    }
}

/// 更新 Tab 列表。一个包里的所有条目共用同一组动作。
///
/// 客户端只有收到带 [`ADD_PLAYER`](Self::ADD_PLAYER) 的包才会新建条目，
/// 而且玩家实体要有条目才能创建，所以这个包必须排在 `add_entity` 前面。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerInfoUpdate<'a> {
    /// 这个类型上的动作常量的组合。
    pub actions: u8,
    pub entries: &'a [PlayerInfoEntry<'a>],
}

impl PlayerInfoUpdate<'_> {
    pub const ADD_PLAYER: u8 = 0x01;
    pub const INITIALIZE_CHAT: u8 = 0x02;
    pub const UPDATE_GAME_MODE: u8 = 0x04;
    pub const UPDATE_LISTED: u8 = 0x08;
    pub const UPDATE_LATENCY: u8 = 0x10;
    pub const UPDATE_DISPLAY_NAME: u8 = 0x20;
    pub const UPDATE_LIST_ORDER: u8 = 0x40;
    pub const UPDATE_HAT: u8 = 0x80;
    /// 玩家进服时用的全部动作。
    pub const ALL_ACTIONS: u8 = 0xff;
}

impl ClientboundPacket for PlayerInfoUpdate<'_> {
    const ID: i32 = clientbound::PLAYER_INFO_UPDATE;

    fn write_body(&self, out: &mut Vec<u8>) {
        out.push(self.actions);
        write_varint(out, self.entries.len() as i32);
        for entry in self.entries {
            entry.write(self.actions, out);
        }
    }
}

/// 把玩家从 Tab 列表移除。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerInfoRemove<'a> {
    pub profile_ids: &'a [Uuid],
}

impl ClientboundPacket for PlayerInfoRemove<'_> {
    const ID: i32 = clientbound::PLAYER_INFO_REMOVE;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_varint(out, self.profile_ids.len() as i32);
        for &id in self.profile_ids {
            write_uuid(out, id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::test_util::{body, hex};

    const STEVE: Uuid = Uuid::from_u128(0x069a79f4_44e9_4726_a5be_fca90e38aaf5);
    const STEVE_HEX: &str = "06 9a 79 f4 44 e9 47 26 a5 be fc a9 0e 38 aa f5";
    const KITTEN: Uuid = Uuid::from_u128(0xdb80816f_ce8f_3340_b6e5_6defcb8c28ef);
    const KITTEN_HEX: &str = "db 80 81 6f ce 8f 33 40 b6 e5 6d ef cb 8c 28 ef";

    fn steve() -> PlayerInfoEntry<'static> {
        PlayerInfoEntry {
            uuid: STEVE,
            name: "Steve",
            properties: &[],
            game_mode: GameMode::Creative,
            listed: true,
            latency: 42,
            display_name: None,
            list_order: 0,
            show_hat: true,
        }
    }

    fn update(actions: u8, entries: &[PlayerInfoEntry<'_>]) -> Vec<u8> {
        body(&PlayerInfoUpdate { actions, entries })
    }

    #[test]
    fn all_actions_match_official_bytes() {
        assert_eq!(
            update(PlayerInfoUpdate::ALL_ACTIONS, &[steve()]),
            hex(&format!(
                "ff
                 01
                 {STEVE_HEX}
                 05 53 74 65 76 65
                 00
                 00
                 01
                 01
                 2a
                 00
                 00
                 01"
            ))
        );
    }

    #[test]
    fn adding_a_player_writes_the_skin_properties() {
        let properties = [ProfileProperty {
            name: "textures".to_owned(),
            value: "value".to_owned(),
            signature: Some("signature".to_owned()),
        }];
        let entry = PlayerInfoEntry {
            properties: &properties,
            ..steve()
        };

        let mut expected = format!("01 01 {STEVE_HEX} 05 53 74 65 76 65 01");
        expected.push_str(" 08 74 65 78 74 75 72 65 73 05 76 61 6c 75 65 01");
        expected.push_str(" 09 73 69 67 6e 61 74 75 72 65");
        assert_eq!(
            update(PlayerInfoUpdate::ADD_PLAYER, &[entry]),
            hex(&expected)
        );
    }

    #[test]
    fn single_action_updates_match_official_bytes() {
        assert_eq!(
            update(PlayerInfoUpdate::UPDATE_HAT, &[steve()]),
            hex(&format!("80 01 {STEVE_HEX} 01"))
        );
        assert_eq!(
            update(PlayerInfoUpdate::UPDATE_LATENCY, &[steve()]),
            hex(&format!("10 01 {STEVE_HEX} 2a"))
        );
    }

    #[test]
    fn actions_can_be_combined() {
        let kitten = PlayerInfoEntry {
            uuid: KITTEN,
            name: "Kitten",
            ..steve()
        };
        let actions = PlayerInfoUpdate::ADD_PLAYER
            | PlayerInfoUpdate::UPDATE_GAME_MODE
            | PlayerInfoUpdate::UPDATE_LISTED;
        assert_eq!(
            update(actions, &[kitten]),
            hex(&format!(
                "0d
                 01
                 {KITTEN_HEX}
                 06 4b 69 74 74 65 6e
                 00
                 01
                 01"
            ))
        );
    }

    #[test]
    fn entries_share_the_same_actions() {
        let kitten = PlayerInfoEntry {
            uuid: KITTEN,
            latency: 300,
            ..steve()
        };
        assert_eq!(
            update(PlayerInfoUpdate::UPDATE_LATENCY, &[steve(), kitten]),
            hex(&format!("10 02 {STEVE_HEX} 2a {KITTEN_HEX} ac 02"))
        );
    }

    #[test]
    fn display_name_and_list_order_are_written_when_asked() {
        let display_name = Component::text("hi");
        let entry = PlayerInfoEntry {
            display_name: Some(&display_name),
            list_order: 300,
            ..steve()
        };
        let actions = PlayerInfoUpdate::UPDATE_DISPLAY_NAME | PlayerInfoUpdate::UPDATE_LIST_ORDER;
        assert_eq!(
            update(actions, &[entry]),
            hex(&format!("60 01 {STEVE_HEX} 01 08 00 02 68 69 ac 02"))
        );
    }

    /// 皮肤跟着 ADD_PLAYER 一起发出去，别人的客户端才看得到。
    #[test]
    fn add_player_carries_the_skin() {
        let properties = [ProfileProperty {
            name: "textures".to_owned(),
            value: "ewo=".to_owned(),
            signature: Some("c2ln".to_owned()),
        }];
        let entry = PlayerInfoEntry {
            properties: &properties,
            ..steve()
        };
        assert_eq!(
            update(PlayerInfoUpdate::ADD_PLAYER, &[entry]),
            hex(&format!(
                "01
                 01
                 {STEVE_HEX}
                 05 53 74 65 76 65
                 01
                 08 74 65 78 74 75 72 65 73
                 04 65 77 6f 3d
                 01
                 04 63 32 6c 6e"
            ))
        );
    }

    #[test]
    fn player_info_remove_matches_official_bytes() {
        assert_eq!(
            body(&PlayerInfoRemove {
                profile_ids: &[KITTEN]
            }),
            hex(&format!("01 {KITTEN_HEX}"))
        );
    }
}
