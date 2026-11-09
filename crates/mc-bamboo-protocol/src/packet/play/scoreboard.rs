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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
