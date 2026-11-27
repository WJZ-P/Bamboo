//! 地图数据包：`ClientboundMapItemDataPacket`。
//!
//! 协议格式照 26.3 官方 `ClientboundMapItemDataPacket.STREAM_CODEC`：
//! - `mapId`: `VarInt`（`MapId.STREAM_CODEC`）
//! - `scale`: `i8`
//! - `locked`: `bool`
//! - `decorations`: `Optional<List<MapDecoration>>`（1 字节存在标志 + `VarInt` 长度 + 条目）
//!   - 每条 `MapDecoration`：`type`（`minecraft:map_decoration_type` 注册表 `VarInt` ID）、
//!     `x`（`i8`）、`y`（`i8`）、`rot`（`i8`，`& 15`）、`name`（`Optional<Component>`）
//! - `colorPatch`: `MapPatch.STREAM_CODEC`：无补丁写 1 字节 `0`；有补丁写
//!   `width`（`u8`）、`height`（`u8`）、`startX`（`u8`）、`startY`（`u8`）、
//!   `colors`（`VarInt` 长度 + 原始字节）
//!
//! Map data packet (`ClientboundMapItemDataPacket`), following vanilla 26.3's
//! `ClientboundMapItemDataPacket.STREAM_CODEC`.

use mc_bamboo_data::packets::play::clientbound;

use crate::{
    codec::{write_bool, write_varint},
    packet::ClientboundPacket,
    text::Component,
};

/// 地图上的一个标记，对应官方 `MapDecoration`。
///
/// One decoration icon on a map, corresponding to vanilla `MapDecoration`.
#[derive(Debug, Clone, PartialEq)]
pub struct MapDecoration {
    /// `minecraft:map_decoration_type` 注册表 ID（0 起）。
    pub kind: i32,
    /// 地图坐标 X（`-128..=127`）。
    pub x: i8,
    /// 地图坐标 Y（`-128..=127`）。
    pub y: i8,
    /// 旋转档位（`0..=15`，构造 / 编码时按 `& 15` 截断）。
    pub rot: i8,
    /// 可选显示名称（例如命名旗帜）。
    pub name: Option<Component>,
}

impl MapDecoration {
    fn write(&self, out: &mut Vec<u8>) {
        write_varint(out, self.kind);
        out.push(self.x as u8);
        out.push(self.y as u8);
        out.push((self.rot & 15) as u8);
        match &self.name {
            Some(name) => {
                write_bool(out, true);
                name.write(out);
            }
            None => write_bool(out, false),
        }
    }
}

/// 地图颜色脏区补丁，对应官方 `MapItemSavedData$MapPatch`。
///
/// A rectangular color update patch, corresponding to vanilla
/// `MapItemSavedData.MapPatch`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapPatch<'a> {
    pub start_x: u8,
    pub start_y: u8,
    pub width: u8,
    pub height: u8,
    pub colors: &'a [u8],
}

/// `ClientboundMapItemDataPacket`（Play clientbound ID 52）。
///
/// `ClientboundMapItemDataPacket` (play clientbound ID 52).
#[derive(Debug, Clone, PartialEq)]
pub struct MapItemData<'a> {
    pub map_id: i32,
    pub scale: i8,
    pub locked: bool,
    pub decorations: Option<&'a [MapDecoration]>,
    pub color_patch: Option<MapPatch<'a>>,
}

impl ClientboundPacket for MapItemData<'_> {
    const ID: i32 = clientbound::MAP_ITEM_DATA;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_varint(out, self.map_id);
        out.push(self.scale as u8);
        write_bool(out, self.locked);
        match self.decorations {
            Some(list) => {
                write_bool(out, true);
                write_varint(out, list.len() as i32);
                for decoration in list {
                    decoration.write(out);
                }
            }
            None => write_bool(out, false),
        }
        match &self.color_patch {
            Some(patch) => {
                out.push(patch.width);
                out.push(patch.height);
                out.push(patch.start_x);
                out.push(patch.start_y);
                write_varint(out, patch.colors.len() as i32);
                out.extend_from_slice(patch.colors);
            }
            None => out.push(0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::test_util::{body, hex};

    #[test]
    fn empty_update_writes_optional_markers() {
        let packet = MapItemData {
            map_id: 5,
            scale: 2,
            locked: true,
            decorations: None,
            color_patch: None,
        };
        assert_eq!(body(&packet), hex("05 02 01 00 00"));
    }

    #[test]
    fn decorations_and_color_patch_encode_in_official_order() {
        let decorations = [
            MapDecoration {
                kind: 0,
                x: -10,
                y: 20,
                rot: 19, // 19 & 15 = 3
                name: None,
            },
            MapDecoration {
                kind: 1,
                x: 127,
                y: -128,
                rot: 4,
                name: Some(Component::text("A")),
            },
        ];
        let colors = [12u8, 34, 56, 78];
        let packet = MapItemData {
            map_id: 0,
            scale: 0,
            locked: false,
            decorations: Some(&decorations),
            color_patch: Some(MapPatch {
                start_x: 3,
                start_y: 4,
                width: 2,
                height: 2,
                colors: &colors,
            }),
        };
        // mapId=0, scale=0, locked=false
        // decorations: present(01), len=2(02)
        //   [0]: type=0(00), x=-10(f6), y=20(14), rot=3(03), name=None(00)
        //   [1]: type=1(01), x=127(7f), y=-128(80), rot=4(04), name=Some(01) + TAG_String("A")(08 00 01 41)
        // colorPatch: width=2(02), height=2(02), startX=3(03), startY=4(04), len=4(04), bytes(0c 22 38 4e)
        assert_eq!(
            body(&packet),
            hex(
                "00 00 00 01 02 00 f6 14 03 00 01 7f 80 04 01 08 00 01 41 02 02 03 04 04 0c 22 38 4e"
            )
        );
    }
}
