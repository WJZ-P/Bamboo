//! 计分板、队伍和 Boss 栏的客户端包。
//!
//! 字段顺序按官方 `ClientboundSetObjectivePacket`、`ClientboundSetScorePacket`、
//! `ClientboundResetScorePacket`、`ClientboundSetDisplayObjectivePacket`、
//! `ClientboundSetPlayerTeamPacket` 和 `ClientboundBossEventPacket`。
//!
//! Clientbound scoreboard, team, and boss-bar packets.
//!
//! Field order follows the official `ClientboundSetObjectivePacket`,
//! `ClientboundSetScorePacket`, `ClientboundResetScorePacket`,
//! `ClientboundSetDisplayObjectivePacket`, `ClientboundSetPlayerTeamPacket`,
//! and `ClientboundBossEventPacket`.

use mc_bamboo_data::packets::play::clientbound;
use mc_bamboo_nbt::Tag;
use uuid::Uuid;

use super::ClientboundPacket;
use crate::{
    codec::{write_bool, write_f32, write_string, write_uuid, write_varint},
    text::Component,
};

/// 分数的数字格式。类型号按 `NumberFormatTypes.bootstrap`：blank、styled、fixed。
///
/// A score number format. Type ids follow `NumberFormatTypes.bootstrap`: blank, styled, fixed.
#[derive(Debug, Clone, PartialEq)]
pub enum NumberFormat {
    Blank,
    Styled(Tag),
    Fixed(Component),
}

impl NumberFormat {
    fn write(&self, out: &mut Vec<u8>) {
        match self {
            Self::Blank => write_varint(out, 0),
            Self::Styled(style) => {
                write_varint(out, 1);
                style.write_network(out);
            }
            Self::Fixed(component) => {
                write_varint(out, 2);
                component.write(out);
            }
        }
    }
}

fn write_optional_format(out: &mut Vec<u8>, format: Option<&NumberFormat>) {
    match format {
        None => write_bool(out, false),
        Some(format) => {
            write_bool(out, true);
            format.write(out);
        }
    }
}

fn write_optional_component(out: &mut Vec<u8>, component: Option<&Component>) {
    match component {
        None => write_bool(out, false),
        Some(component) => {
            write_bool(out, true);
            component.write(out);
        }
    }
}

/// `set_objective`。`method` 是 0 添加、1 删除、2 修改。
///
/// `set_objective`. `method` is 0 to add, 1 to remove, and 2 to change.
#[derive(Debug, Clone, PartialEq)]
pub struct SetObjective<'a> {
    pub name: &'a str,
    pub method: u8,
    pub display: &'a Component,
    pub render_type: i32,
    pub number_format: Option<&'a NumberFormat>,
}

impl ClientboundPacket for SetObjective<'_> {
    const ID: i32 = clientbound::SET_OBJECTIVE;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_string(out, self.name);
        out.push(self.method);
        if self.method == 0 || self.method == 2 {
            self.display.write(out);
            write_varint(out, self.render_type);
            write_optional_format(out, self.number_format);
        }
    }
}

/// `set_score`。分数是 VarInt。
///
/// `set_score`. The score is a VarInt.
#[derive(Debug, Clone, PartialEq)]
pub struct SetScore<'a> {
    pub owner: &'a str,
    pub objective: &'a str,
    pub score: i32,
    pub display: Option<&'a Component>,
    pub number_format: Option<&'a NumberFormat>,
}

impl ClientboundPacket for SetScore<'_> {
    const ID: i32 = clientbound::SET_SCORE;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_string(out, self.owner);
        write_string(out, self.objective);
        write_varint(out, self.score);
        write_optional_component(out, self.display);
        write_optional_format(out, self.number_format);
    }
}

/// `reset_score`。目标名可以空着，表示清掉这个名字的全部分数。
///
/// `reset_score`. The objective may be absent, which clears every score of that name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResetScore<'a> {
    pub owner: &'a str,
    pub objective: Option<&'a str>,
}

impl ClientboundPacket for ResetScore<'_> {
    const ID: i32 = clientbound::RESET_SCORE;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_string(out, self.owner);
        match self.objective {
            None => write_bool(out, false),
            Some(objective) => {
                write_bool(out, true);
                write_string(out, objective);
            }
        }
    }
}

/// `set_display_objective`。目标名是空字符串时清空这个位置。
///
/// `set_display_objective`. An empty objective name clears the slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetDisplayObjective<'a> {
    pub slot: i32,
    pub objective: &'a str,
}

impl ClientboundPacket for SetDisplayObjective<'_> {
    const ID: i32 = clientbound::SET_DISPLAY_OBJECTIVE;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_varint(out, self.slot);
        write_string(out, self.objective);
    }
}

/// 队伍参数。颜色空着时不写 id。
///
/// Team parameters. An absent color omits the id.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TeamParameters<'a> {
    pub display: &'a Component,
    pub prefix: &'a Component,
    pub suffix: &'a Component,
    pub visibility: i32,
    pub collision: i32,
    pub color: Option<i32>,
    pub options: u8,
}

/// `set_player_team` 的动作。
///
/// One action of `set_player_team`.
#[derive(Debug, Clone, PartialEq)]
pub enum TeamAction<'a> {
    Add {
        parameters: TeamParameters<'a>,
        players: &'a [String],
    },
    Remove,
    Change {
        parameters: TeamParameters<'a>,
    },
    Join {
        players: &'a [String],
    },
    Leave {
        players: &'a [String],
    },
}

/// `set_player_team`。
///
/// `set_player_team`.
#[derive(Debug, Clone, PartialEq)]
pub struct SetPlayerTeam<'a> {
    pub name: &'a str,
    pub action: TeamAction<'a>,
}

impl ClientboundPacket for SetPlayerTeam<'_> {
    const ID: i32 = clientbound::SET_PLAYER_TEAM;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_string(out, self.name);
        match &self.action {
            TeamAction::Add {
                parameters,
                players,
            } => {
                out.push(0);
                write_team_parameters(out, parameters);
                write_player_list(out, players);
            }
            TeamAction::Remove => out.push(1),
            TeamAction::Change { parameters } => {
                out.push(2);
                write_team_parameters(out, parameters);
            }
            TeamAction::Join { players } => {
                out.push(3);
                write_player_list(out, players);
            }
            TeamAction::Leave { players } => {
                out.push(4);
                write_player_list(out, players);
            }
        }
    }
}

fn write_team_parameters(out: &mut Vec<u8>, parameters: &TeamParameters<'_>) {
    parameters.display.write(out);
    parameters.prefix.write(out);
    parameters.suffix.write(out);
    write_varint(out, parameters.visibility);
    write_varint(out, parameters.collision);
    match parameters.color {
        None => write_bool(out, false),
        Some(color) => {
            write_bool(out, true);
            write_varint(out, color);
        }
    }
    out.push(parameters.options);
}

fn write_player_list(out: &mut Vec<u8>, players: &[String]) {
    write_varint(out, players.len() as i32);
    for player in players {
        write_string(out, player);
    }
}

/// `boss_event` 的动作。序号是官方 `OperationType` 的枚举顺序。
///
/// One `boss_event` action. The ids are the official `OperationType` ordinals.
#[derive(Debug, Clone, PartialEq)]
pub enum BossAction<'a> {
    Add {
        name: &'a Component,
        progress: f32,
        color: i32,
        overlay: i32,
        flags: u8,
    },
    Remove,
    UpdateProgress(f32),
    UpdateName(&'a Component),
    UpdateStyle {
        color: i32,
        overlay: i32,
    },
    UpdateProperties(u8),
}

/// `boss_event`。
///
/// `boss_event`.
#[derive(Debug, Clone, PartialEq)]
pub struct BossEvent<'a> {
    pub id: Uuid,
    pub action: BossAction<'a>,
}

impl ClientboundPacket for BossEvent<'_> {
    const ID: i32 = clientbound::BOSS_EVENT;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_uuid(out, self.id);
        match &self.action {
            BossAction::Add {
                name,
                progress,
                color,
                overlay,
                flags,
            } => {
                write_varint(out, 0);
                name.write(out);
                write_f32(out, *progress);
                write_varint(out, *color);
                write_varint(out, *overlay);
                out.push(*flags);
            }
            BossAction::Remove => write_varint(out, 1),
            BossAction::UpdateProgress(progress) => {
                write_varint(out, 2);
                write_f32(out, *progress);
            }
            BossAction::UpdateName(name) => {
                write_varint(out, 3);
                name.write(out);
            }
            BossAction::UpdateStyle { color, overlay } => {
                write_varint(out, 4);
                write_varint(out, *color);
                write_varint(out, *overlay);
            }
            BossAction::UpdateProperties(flags) => {
                write_varint(out, 5);
                out.push(*flags);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::test_util::{body, hex};

    /// 纯文本组件就是一个 `TAG_String`。
    ///
    /// A plain text component is a single `TAG_String`.
    const PLAIN: &str = "08 00 02 68 69";

    #[test]
    fn set_objective_add_writes_the_official_fields() {
        let display = Component::text("hi");
        let format = NumberFormat::Blank;
        let packet = SetObjective {
            name: "kills",
            method: 0,
            display: &display,
            render_type: 1,
            number_format: Some(&format),
        };
        // name, method, display, render type, optional format present, blank.
        assert_eq!(
            body(&packet),
            hex(&format!("05 6b 69 6c 6c 73  00  {PLAIN}  01  01  00"))
        );
    }

    #[test]
    fn set_objective_remove_omits_everything_after_the_method() {
        let display = Component::text("hi");
        let packet = SetObjective {
            name: "k",
            method: 1,
            display: &display,
            render_type: 1,
            number_format: None,
        };
        assert_eq!(body(&packet), hex("01 6b 01"));
    }

    #[test]
    fn number_formats_use_the_registry_ids_and_nbt_bodies() {
        let styled = NumberFormat::Styled(Tag::Compound(vec![(
            "color".to_owned(),
            Tag::String("red".to_owned()),
        )]));
        // 样式是复合标签：类型 10、字段名、TAG_String 的值、结束。
        //
        // A style is a compound tag: type 10, field name, TAG_String value, end.
        let mut out = Vec::new();
        styled.write(&mut out);
        assert_eq!(out, hex("01 0a 08 00 05 63 6f 6c 6f 72 00 03 72 65 64 00"));

        let fixed = NumberFormat::Fixed(Component::text("hi"));
        let mut out = Vec::new();
        fixed.write(&mut out);
        assert_eq!(out, hex(&format!("02 {PLAIN}")));
    }

    #[test]
    fn set_score_writes_optional_display_and_format() {
        let display = Component::text("hi");
        let format = NumberFormat::Blank;
        let packet = SetScore {
            owner: "Kitten",
            objective: "kills",
            score: 7,
            display: Some(&display),
            number_format: Some(&format),
        };
        assert_eq!(
            body(&packet),
            hex(&format!(
                "06 4b 69 74 74 65 6e  05 6b 69 6c 6c 73  07  01  {PLAIN}  01 00"
            ))
        );
        let absent = SetScore {
            owner: "Kitten",
            objective: "kills",
            score: -1,
            display: None,
            number_format: None,
        };
        assert_eq!(
            body(&absent),
            hex("06 4b 69 74 74 65 6e 05 6b 69 6c 6c 73 ff ff ff ff 0f 00 00")
        );
    }

    #[test]
    fn reset_score_only_names_an_objective_when_present() {
        assert_eq!(
            body(&ResetScore {
                owner: "Kitten",
                objective: Some("kills"),
            }),
            hex("06 4b 69 74 74 65 6e 01 05 6b 69 6c 6c 73")
        );
        assert_eq!(
            body(&ResetScore {
                owner: "Kitten",
                objective: None,
            }),
            hex("06 4b 69 74 74 65 6e 00")
        );
    }

    #[test]
    fn display_slots_are_varints() {
        assert_eq!(
            body(&SetDisplayObjective {
                slot: 18,
                objective: "kills",
            }),
            hex("12 05 6b 69 6c 6c 73")
        );
    }

    #[test]
    fn team_add_writes_parameters_and_the_member_list() {
        let display = Component::text("hi");
        let prefix = Component::text("[");
        let suffix = Component::text("]");
        let parameters = TeamParameters {
            display: &display,
            prefix: &prefix,
            suffix: &suffix,
            visibility: 2,
            collision: 3,
            color: Some(4),
            options: 3,
        };
        let players = ["Kitten".to_owned(), "Alex".to_owned()];
        let packet = SetPlayerTeam {
            name: "red",
            action: TeamAction::Add {
                parameters,
                players: &players,
            },
        };
        // 名字、动作、三个组件、两个 VarInt、可选颜色、标志、成员数、两个名字。
        assert_eq!(
            body(&packet),
            hex(&format!(
                "03 72 65 64  00  {PLAIN}  08 00 01 5b  08 00 01 5d  \
                 02 03 01 04 03  \
                 02 06 4b 69 74 74 65 6e 04 41 6c 65 78"
            ))
        );
    }

    #[test]
    fn team_remove_has_no_parameters_or_players() {
        assert_eq!(
            body(&SetPlayerTeam {
                name: "red",
                action: TeamAction::Remove,
            }),
            hex("03 72 65 64 01")
        );
    }

    #[test]
    fn team_change_omits_the_member_list() {
        let display = Component::text("hi");
        let parameters = TeamParameters {
            display: &display,
            prefix: &display,
            suffix: &display,
            visibility: 0,
            collision: 0,
            color: None,
            options: 0,
        };
        assert_eq!(
            body(&SetPlayerTeam {
                name: "red",
                action: TeamAction::Change { parameters },
            }),
            hex(&format!(
                "03 72 65 64 02 {PLAIN} {PLAIN} {PLAIN} 00 00 00 00"
            ))
        );
    }

    #[test]
    fn team_join_and_leave_only_carry_the_member_list() {
        let players = ["Kitten".to_owned()];
        for (action, method) in [
            (TeamAction::Join { players: &players }, 3),
            (TeamAction::Leave { players: &players }, 4),
        ] {
            assert_eq!(
                body(&SetPlayerTeam {
                    name: "red",
                    action,
                }),
                hex(&format!("03 72 65 64 {method:02x} 01 06 4b 69 74 74 65 6e"))
            );
        }
    }

    #[test]
    fn boss_event_add_writes_every_field_in_order() {
        let name = Component::text("hi");
        let uuid = Uuid::from_u128(0x0102_0304_0506_0708_090a_0b0c_0d0e_0f10);
        let packet = BossEvent {
            id: uuid,
            action: BossAction::Add {
                name: &name,
                progress: 0.5,
                color: 3,
                overlay: 2,
                flags: 5,
            },
        };
        assert_eq!(
            body(&packet),
            hex(&format!(
                "01 02 03 04 05 06 07 08 09 0a 0b 0c 0d 0e 0f 10  \
                 00 {PLAIN} 3f 00 00 00 03 02 05"
            ))
        );
    }

    #[test]
    fn boss_event_updates_carry_only_their_own_field() {
        let uuid = Uuid::from_u128(0x0102_0304_0506_0708_090a_0b0c_0d0e_0f10);
        let prefix = "01 02 03 04 05 06 07 08 09 0a 0b 0c 0d 0e 0f 10";
        let name = Component::text("hi");
        for (action, suffix) in [
            (BossAction::Remove, "01".to_owned()),
            (
                BossAction::UpdateProgress(0.25),
                "02 3e 80 00 00".to_owned(),
            ),
            (BossAction::UpdateName(&name), format!("03 {PLAIN}")),
            (
                BossAction::UpdateStyle {
                    color: 6,
                    overlay: 4,
                },
                "04 06 04".to_owned(),
            ),
            (BossAction::UpdateProperties(7), "05 07".to_owned()),
        ] {
            assert_eq!(
                body(&BossEvent {
                    id: uuid,
                    action: action.clone(),
                }),
                hex(&format!("{prefix} {suffix}")),
                "{action:?}"
            );
        }
    }
}
