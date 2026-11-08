//! 方块变化和物品栏同步。

use mc_bamboo_core::{BlockPos, sign::SignTextSlot};
use mc_bamboo_data::packets::play::clientbound;

use crate::{
    codec::{
        write_block_pos, write_bool, write_i16, write_i32, write_i64, write_varint, write_varlong,
    },
    item::ItemStack,
    packet::{
        ClientboundPacket,
        chunk::{LightData, write_light_data},
    },
};

/// 一个区块的光照变了，官方的 `ClientboundLightUpdatePacket`。只带变了的段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LightUpdate<'a> {
    pub x: i32,
    pub z: i32,
    pub light: LightData<'a>,
}

impl ClientboundPacket for LightUpdate<'_> {
    const ID: i32 = clientbound::LIGHT_UPDATE;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_varint(out, self.x);
        write_varint(out, self.z);
        write_light_data(out, &self.light);
    }
}

/// 一个方块变了。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockUpdate {
    pub pos: BlockPos,
    /// 方块状态 ID。非法的 ID 会让客户端抛异常。
    pub state: i32,
}

impl ClientboundPacket for BlockUpdate {
    const ID: i32 = clientbound::BLOCK_UPDATE;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_block_pos(out, self.pos);
        write_varint(out, self.state);
    }
}

/// 让客户端播放方块动画，官方的 `ClientboundBlockEventPacket`。活塞伸出、收回靠它。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockEvent {
    pub pos: BlockPos,
    /// `minecraft:block` 注册表 ID，不是状态 ID。官方线上写的是 `writeByte` 的低 8 位。
    pub block: i32,
    pub param_a: i32,
    pub param_b: i32,
}

impl ClientboundPacket for BlockEvent {
    const ID: i32 = clientbound::BLOCK_EVENT;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_block_pos(out, self.pos);
        out.push(self.param_a as u8);
        out.push(self.param_b as u8);
        write_varint(out, self.block);
    }
}

/// 一个区块段里这一 tick 改了不止一格，官方的 `ClientboundSectionBlocksUpdatePacket`。
///
/// 只改了一格时官方发 [`BlockUpdate`]。客户端对两种包做的事一样，逐格写进世界。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SectionBlocksUpdate<'a> {
    /// 段坐标：x、z 就是区块坐标，y 是方块 `y >> 4`。
    pub x: i32,
    pub y: i32,
    pub z: i32,
    /// 按这个顺序发，客户端也按这个顺序写。
    pub blocks: &'a [SectionBlock],
}

/// [`SectionBlocksUpdate`] 里的一格。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SectionBlock {
    /// 段内坐标，官方的 `SectionPos.sectionRelativePos`：`x << 8 | z << 4 | y`，
    /// 每个分量是世界坐标的低 4 位。
    pub local: u16,
    /// 方块状态 ID。
    pub state: i32,
}

impl ClientboundPacket for SectionBlocksUpdate<'_> {
    const ID: i32 = clientbound::SECTION_BLOCKS_UPDATE;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_i64(out, pack_section(self.x, self.y, self.z));
        write_varint(out, self.blocks.len() as i32);
        for block in self.blocks {
            write_varlong(out, (i64::from(block.state) << 12) | i64::from(block.local));
        }
    }
}

/// 官方的 `SectionPos.asLong`：从高位到低位是 `x(22) | z(22) | y(20)`。
const fn pack_section(x: i32, y: i32, z: i32) -> i64 {
    ((x as i64 & 0x3f_ffff) << 42) | ((z as i64 & 0x3f_ffff) << 20) | (y as i64 & 0xf_ffff)
}

/// 确认到 `sequence` 为止的所有预测。必须排在这些改动的 [`BlockUpdate`] 和
/// [`SectionBlocksUpdate`] 后面。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockChangedAck {
    pub sequence: i32,
}

impl ClientboundPacket for BlockChangedAck {
    const ID: i32 = clientbound::BLOCK_CHANGED_ACK;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_varint(out, self.sequence);
    }
}

/// 打开告示牌编辑器。只有被允许编辑的那名玩家会收到。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenSignEditor {
    pub pos: BlockPos,
    pub slot: SignTextSlot,
}

impl ClientboundPacket for OpenSignEditor {
    const ID: i32 = clientbound::OPEN_SIGN_EDITOR;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_block_pos(out, self.pos);
        write_varint(out, self.slot.id());
    }
}

/// 一个方块实体现在的内容，官方的 `ClientboundBlockEntityDataPacket`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockEntityData<'a> {
    pub pos: BlockPos,
    /// `minecraft:block_entity_type` 注册表 ID。
    pub kind: i32,
    /// 方块实体保存的数据，官方的 `getUpdateTag`，网络格式的 NBT。
    pub data: &'a [u8],
}

impl ClientboundPacket for BlockEntityData<'_> {
    const ID: i32 = clientbound::BLOCK_ENTITY_DATA;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_block_pos(out, self.pos);
        write_varint(out, self.kind);
        out.extend_from_slice(self.data);
    }
}

/// 让客户端播放一个世界事件。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelEvent {
    pub event: i32,
    pub pos: BlockPos,
    pub data: i32,
    pub global: bool,
}

impl LevelEvent {
    /// 破坏方块的粒子和声音，`data` 是被破坏方块的状态 ID。
    pub const DESTROY_BLOCK: i32 = 2001;
    /// 告示牌上蜡成功，官方 `HoneycombItem.tryApplyToSign` 里的 `levelEvent(null, 3003, pos, 0)`。
    pub const WAX_ON_SIGN: i32 = 3003;

    /// 红石火把烧坏时的烟。
    pub const REDSTONE_TORCH_BURNOUT: i32 = 1502;

    /// 红石火把烧坏时的烟。
    pub const fn redstone_torch_burnout(pos: BlockPos) -> Self {
        Self {
            event: Self::REDSTONE_TORCH_BURNOUT,
            pos,
            data: 0,
            global: false,
        }
    }

    /// 破坏方块的粒子和声音。发起者已经在本地预测过，不要发给他。
    pub const fn destroy_block(pos: BlockPos, state: i32) -> Self {
        Self {
            event: Self::DESTROY_BLOCK,
            pos,
            data: state,
            global: false,
        }
    }

    /// 告示牌上蜡的光照效果，附近的玩家都看得到。
    pub const fn wax_on_sign(pos: BlockPos) -> Self {
        Self {
            event: Self::WAX_ON_SIGN,
            pos,
            data: 0,
            global: false,
        }
    }
}

impl ClientboundPacket for LevelEvent {
    const ID: i32 = clientbound::LEVEL_EVENT;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_i32(out, self.event);
        write_block_pos(out, self.pos);
        write_i32(out, self.data);
        write_bool(out, self.global);
    }
}

/// 改变一个容器槽。物品带组件，所以只能发给物品的主人，见 [`ItemStack`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContainerSetSlot<'a> {
    /// 玩家物品栏是 0。
    pub container_id: i32,
    pub state_id: i32,
    /// `InventoryMenu` 的编号。
    pub slot: i16,
    pub stack: &'a ItemStack,
}

impl ClientboundPacket for ContainerSetSlot<'_> {
    const ID: i32 = clientbound::CONTAINER_SET_SLOT;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_varint(out, self.container_id);
        write_varint(out, self.state_id);
        write_i16(out, self.slot);
        self.stack.write(out);
    }
}

/// 改变选中的快捷栏格，`slot` 是 0 到 8。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetHeldSlot {
    pub slot: i32,
}

impl ClientboundPacket for SetHeldSlot {
    const ID: i32 = clientbound::SET_HELD_SLOT;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_varint(out, self.slot);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::test_util::{body, hex};

    #[test]
    fn block_update_matches_official_bytes() {
        let stone = BlockUpdate {
            pos: BlockPos::new(8, -60, 8),
            state: 1,
        };
        assert_eq!(body(&stone), hex("00 00 02 00 00 00 8f c4 01"));

        let oak_log = BlockUpdate {
            state: 140,
            ..stone
        };
        assert_eq!(body(&oak_log), hex("00 00 02 00 00 00 8f c4 8c 01"));
    }

    #[test]
    fn section_blocks_update_matches_official_bytes() {
        // `blocks-inventory.md` 3.2 的实测字节：段 (1, -4, -1)，(18, -61, -7) 设成 stone(1)，
        // (31, -49, -1) 设成 oak_slab 的默认状态 15180。
        let packet = SectionBlocksUpdate {
            x: 1,
            y: -4,
            z: -1,
            blocks: &[
                SectionBlock {
                    local: 0x293,
                    state: 1,
                },
                SectionBlock {
                    local: 0xfff,
                    state: 15180,
                },
            ],
        };
        assert_eq!(
            body(&packet),
            hex("00 00 07 ff ff ff ff fc 02 93 25 ff 9f d3 1d")
        );
    }

    #[test]
    fn section_position_keeps_the_low_bits_of_each_axis() {
        // x 占高 22 位，z 占中间 22 位，y 占低 20 位，负数按补码截断。
        assert_eq!(pack_section(0, 0, 0), 0);
        assert_eq!(pack_section(-1, -1, -1), -1);
        assert_eq!(pack_section(0x20_0000, 0, 0), i64::MIN);
        assert_eq!(pack_section(0, -1, 0), 0xf_ffff);
        assert_eq!(pack_section(0, 0, -1), 0x3f_ffff << 20);
    }

    #[test]
    fn section_blocks_update_puts_the_state_above_the_position() {
        // 状态 ID 左移 12 位再拼上段内坐标，VarLong 写出来要 4 个字节。
        let packet = SectionBlocksUpdate {
            x: 0,
            y: 0,
            z: 0,
            blocks: &[SectionBlock {
                local: 0x80f,
                state: 30_000,
            }],
        };
        let mut expected = hex("00 00 00 00 00 00 00 00 01");
        write_varlong(&mut expected, (30_000 << 12) | 0x80f);
        assert_eq!(expected.len(), 8 + 1 + 4);
        assert_eq!(body(&packet), expected);
    }

    #[test]
    fn light_update_writes_varint_coordinates_before_the_light_data() {
        let layer = [0x12; 2048];
        let packet = LightUpdate {
            x: -1,
            z: 300,
            light: LightData {
                sky_mask: 0b10,
                block_mask: 0,
                empty_sky_mask: 0,
                empty_block_mask: 0b100,
                sky_updates: &[&layer],
                block_updates: &[],
            },
        };

        let mut expected = hex("ff ff ff ff 0f ac 02 01 02 00 00 01 04 01 80 10");
        expected.extend_from_slice(&layer);
        expected.push(0);
        assert_eq!(body(&packet), expected);
    }

    #[test]
    fn open_sign_editor_matches_official_bytes() {
        let packet = OpenSignEditor {
            pos: BlockPos::new(8, -60, 8),
            slot: SignTextSlot::Front,
        };
        assert_eq!(body(&packet), hex("00 00 02 00 00 00 8f c4 01"));
    }

    #[test]
    fn block_entity_data_copies_the_tag_after_the_type() {
        let packet = BlockEntityData {
            pos: BlockPos::new(8, -60, 8),
            kind: 42,
            data: &[0x08, 0x00],
        };
        assert_eq!(body(&packet), hex("00 00 02 00 00 00 8f c4 2a 08 00"));
    }

    #[test]
    fn block_event_writes_the_block_and_two_bytes() {
        let packet = BlockEvent {
            pos: BlockPos::new(8, -60, 8),
            block: 42,
            param_a: 0,
            param_b: 1,
        };
        assert_eq!(body(&packet), hex("00 00 02 00 00 00 8f c4 00 01 2a"));
    }

    #[test]
    fn block_changed_ack_matches_official_bytes() {
        assert_eq!(body(&BlockChangedAck { sequence: 300 }), hex("ac 02"));
    }

    #[test]
    fn level_event_matches_official_bytes() {
        let packet = LevelEvent::destroy_block(BlockPos::new(8, -61, 8), 9);
        assert_eq!(
            body(&packet),
            hex("00 00 07 d1 00 00 02 00 00 00 8f c3 00 00 00 09 00")
        );
    }

    #[test]
    fn container_set_slot_matches_official_bytes() {
        let stack = ItemStack::new(1, 1);
        let packet = ContainerSetSlot {
            container_id: 0,
            state_id: 2,
            slot: 36,
            stack: &stack,
        };
        assert_eq!(body(&packet), hex("00 02 00 24 01 01 00 00"));
    }

    #[test]
    fn container_set_slot_keeps_components_for_their_owner() {
        let stack = ItemStack {
            item: 1,
            count: 2,
            added: vec![(1, vec![0x10])],
            removed: vec![12],
        };
        let packet = ContainerSetSlot {
            container_id: 0,
            state_id: 2,
            slot: 36,
            stack: &stack,
        };
        assert_eq!(body(&packet), hex("00 02 00 24 02 01 01 01 01 10 0c"));
    }

    #[test]
    fn set_held_slot_is_a_var_int() {
        assert_eq!(body(&SetHeldSlot { slot: 3 }), hex("03"));
    }
}
