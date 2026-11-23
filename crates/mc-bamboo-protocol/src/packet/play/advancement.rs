//! 进度系统的三个网络包，官方 26.3。
//!
//! `UpdateAdvancements` 的 `added` 条目带着 `PositionedAdvancement` 的坐标；坐标由
//! `mc-bamboo-advancements` 的共享树算好传进来。进度条目的线上格式也由那个 crate
//! 写，协议层只负责长度、键和布尔值。

use mc_bamboo_data::packets::play::{clientbound, serverbound};

use crate::{
    codec::{DecodeError, Reader, write_bool, write_f32, write_string, write_varint},
    packet::ClientboundPacket,
};

/// 一条 `added` 条目：进度 id、定义、坐标。
#[derive(Debug, Clone)]
pub struct PositionedAdvancement<'a> {
    pub id: &'a str,
    /// `Advancement.STREAM_CODEC` 写好的定义字节（不含 id）。
    pub definition: &'a [u8],
    pub x: f32,
    pub y: f32,
}

/// 一条进度变化：`id` 和 `AdvancementProgress.STREAM_CODEC` 写好的字节。
#[derive(Debug, Clone)]
pub struct ProgressEntry<'a> {
    pub id: &'a str,
    pub progress: &'a [u8],
}

/// 官方 `ClientboundUpdateAdvancementsPacket`。
#[derive(Debug, Clone)]
pub struct UpdateAdvancements<'a> {
    pub reset: bool,
    pub added: &'a [PositionedAdvancement<'a>],
    pub removed: &'a [&'a str],
    pub progress: &'a [ProgressEntry<'a>],
    /// 官方 `PlayerAdvancements.flushDirty` 的固定值 `true`（首次包和增量都是）。
    pub show_advancements: bool,
}

impl ClientboundPacket for UpdateAdvancements<'_> {
    const ID: i32 = clientbound::UPDATE_ADVANCEMENTS;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_bool(out, self.reset);
        write_varint(out, self.added.len() as i32);
        for item in self.added {
            write_string(out, item.id);
            out.extend_from_slice(item.definition);
            write_f32(out, item.x);
            write_f32(out, item.y);
        }
        write_varint(out, self.removed.len() as i32);
        for id in self.removed {
            write_string(out, id);
        }
        write_varint(out, self.progress.len() as i32);
        for item in self.progress {
            write_string(out, item.id);
            out.extend_from_slice(item.progress);
        }
        write_bool(out, self.show_advancements);
    }
}

/// 官方 `ClientboundSelectAdvancementsTabPacket`：可选的标签页 id。
#[derive(Debug, Clone, Copy)]
pub struct SelectAdvancementsTab<'a> {
    pub tab: Option<&'a str>,
}

impl ClientboundPacket for SelectAdvancementsTab<'_> {
    const ID: i32 = clientbound::SELECT_ADVANCEMENTS_TAB;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_bool(out, self.tab.is_some());
        if let Some(tab) = self.tab {
            write_string(out, tab);
        }
    }
}

/// 客户端打开进度界面时报的标签页，官方 `ServerboundSeenAdvancementsPacket`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeenAdvancements {
    OpenedTab(String),
    ClosedScreen,
}

impl SeenAdvancements {
    /// 从包体（不含 id）解出来。非法动作或越界 id 都只返回错误。
    pub fn read_body(reader: &mut Reader<'_>) -> Result<Self, DecodeError> {
        let action = reader.varint()?;
        match action {
            0 => Ok(Self::OpenedTab(reader.string(MAX_IDENTIFIER)?.to_owned())),
            1 => Ok(Self::ClosedScreen),
            _ => Err(DecodeError::InvalidEnum(action)),
        }
    }
}

/// 标识符长度上限，官方 `Identifier` 用 `String` 的 32767 字节上限。
const MAX_IDENTIFIER: usize = 32767;

/// `PlayRequest` 里的进度界面事件，由 `request.rs` 转发。
pub fn decode_seen_advancements(
    reader: &mut Reader<'_>,
    id: i32,
) -> Result<crate::packet::play::PlayRequest, DecodeError> {
    debug_assert_eq!(id, serverbound::SEEN_ADVANCEMENTS);
    Ok(crate::packet::play::PlayRequest::SeenAdvancements(
        SeenAdvancements::read_body(reader)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::test_util::body;

    #[test]
    fn update_packet_writes_the_official_layout() {
        let definition = [0x01u8, 0x02];
        let progress = [0x00u8];
        let added = [PositionedAdvancement {
            id: "minecraft:story/root",
            definition: &definition,
            x: 0.0,
            y: 1.5,
        }];
        let removed = ["minecraft:recipes/root"];
        let entries = [ProgressEntry {
            id: "minecraft:story/root",
            progress: &progress,
        }];
        let packet = UpdateAdvancements {
            reset: true,
            added: &added,
            removed: &removed,
            progress: &entries,
            show_advancements: true,
        };
        let mut expected = vec![1, 1];
        write_string(&mut expected, "minecraft:story/root");
        expected.extend_from_slice(&definition);
        write_f32(&mut expected, 0.0);
        write_f32(&mut expected, 1.5);
        write_varint(&mut expected, 1);
        write_string(&mut expected, "minecraft:recipes/root");
        write_varint(&mut expected, 1);
        write_string(&mut expected, "minecraft:story/root");
        expected.push(0);
        expected.push(1);
        assert_eq!(body(&packet), expected);
    }

    #[test]
    fn select_tab_is_a_nullable_identifier() {
        assert_eq!(body(&SelectAdvancementsTab { tab: None }), vec![0]);
        assert_eq!(
            body(&SelectAdvancementsTab {
                tab: Some("minecraft:story/root"),
            }),
            {
                let mut out = vec![1];
                write_string(&mut out, "minecraft:story/root");
                out
            }
        );
    }

    #[test]
    fn seen_advancements_rejects_bad_actions() {
        let bytes = [
            0u8, 18, b'm', b'i', b'n', b'e', b'c', b'r', b'a', b'f', b't', b':', b'a', b'/', b'b',
            b'c', b'd', b'e', b'f',
        ];
        // 长度 18 但字符串只给了 16 字节，先测合法输入。
        let mut valid = vec![0u8];
        write_string(&mut valid, "minecraft:story/root");
        let mut reader = Reader::new(&valid);
        assert_eq!(
            SeenAdvancements::read_body(&mut reader),
            Ok(SeenAdvancements::OpenedTab(
                "minecraft:story/root".to_owned()
            ))
        );
        reader.finish().unwrap();
        let closed = vec![1u8];
        let mut reader = Reader::new(&closed);
        assert_eq!(
            SeenAdvancements::read_body(&mut reader),
            Ok(SeenAdvancements::ClosedScreen)
        );
        let bad = vec![2u8];
        assert!(SeenAdvancements::read_body(&mut Reader::new(&bad)).is_err());
        assert!(SeenAdvancements::read_body(&mut Reader::new(&bytes[..])).is_err());
    }
}
