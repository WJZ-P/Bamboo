//! 菜单和容器。

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::packets::play::clientbound;

use crate::{
    codec::{write_block_pos, write_varint},
    item::ItemStack,
    packet::ClientboundPacket,
    text::Component,
};

/// 打开一个菜单。`menu_type` 是 `minecraft:menu` 注册表 ID，`title` 是窗口标题。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OpenScreen<'a> {
    /// 官方叫 `containerId`，从 1 开始，每个玩家各自计数。
    pub container_id: i32,
    pub menu_type: i32,
    pub title: &'a Component,
}

impl ClientboundPacket for OpenScreen<'_> {
    const ID: i32 = clientbound::OPEN_SCREEN;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_varint(out, self.container_id);
        write_varint(out, self.menu_type);
        self.title.write(out);
    }
}

/// 一个菜单的全部槽位和光标上的物品。打开菜单时发一次。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContainerSetContent<'a> {
    pub container_id: i32,
    pub state_id: i32,
    pub items: &'a [ItemStack],
    pub carried: &'a ItemStack,
}

impl ClientboundPacket for ContainerSetContent<'_> {
    const ID: i32 = clientbound::CONTAINER_SET_CONTENT;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_varint(out, self.container_id);
        write_varint(out, self.state_id);
        write_varint(out, self.items.len() as i32);
        for stack in self.items {
            stack.write(out);
        }
        self.carried.write(out);
    }
}

/// 关掉一个菜单。客户端自己关的时候服务端不回这个包。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContainerClose {
    pub container_id: i32,
}

impl ClientboundPacket for ContainerClose {
    const ID: i32 = clientbound::CONTAINER_CLOSE;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_varint(out, self.container_id);
    }
}

/// 方块事件，目前只用来告诉客户端箱子和潜影盒开合了。
///
/// `block` 是方块注册表 ID。事件 id 和数据由方块的 `triggerEvent` 解释。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockEvent {
    pub pos: BlockPos,
    pub action: u8,
    pub data: u8,
    pub block: i32,
}

impl ClientboundPacket for BlockEvent {
    const ID: i32 = clientbound::BLOCK_EVENT;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_block_pos(out, self.pos);
        out.push(self.action);
        out.push(self.data);
        write_varint(out, self.block);
    }
}

/// 光标上的物品变了。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetCursorItem<'a> {
    pub stack: &'a ItemStack,
}

impl ClientboundPacket for SetCursorItem<'_> {
    const ID: i32 = clientbound::SET_CURSOR_ITEM;

    fn write_body(&self, out: &mut Vec<u8>) {
        self.stack.write(out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::test_util::{body, hex};

    #[test]
    fn open_screen_matches_the_packet_layout() {
        let title = Component::translate("container.chest", []);
        let packet = OpenScreen {
            container_id: 1,
            menu_type: 2,
            title: &title,
        };
        // 容器 ID、菜单类型、标题（一个只有 translate 的 NBT 复合标签）。
        assert_eq!(
            body(&packet),
            hex("01 02
                 0a
                 08 00 09 74 72 61 6e 73 6c 61 74 65
                 00 0f 63 6f 6e 74 61 69 6e 65 72 2e 63 68 65 73 74
                 00")
        );
    }

    #[test]
    fn container_set_content_writes_a_list_and_the_carried_stack() {
        let items = [ItemStack::new(1, 2), ItemStack::empty()];
        let carried = ItemStack::new(3, 4);
        let packet = ContainerSetContent {
            container_id: 7,
            state_id: 300,
            items: &items,
            carried: &carried,
        };
        assert_eq!(body(&packet), hex("07 ac 02 02 02 01 00 00 00 04 03 00 00"));
    }

    #[test]
    fn container_close_is_a_var_int() {
        assert_eq!(
            body(&ContainerClose { container_id: 300 }),
            hex("ac 02")
        );
    }

    #[test]
    fn block_event_writes_the_block_id_last() {
        let packet = BlockEvent {
            pos: BlockPos::new(8, -60, 8),
            action: 1,
            data: 2,
            block: 300,
        };
        assert_eq!(
            body(&packet),
            hex("00 00 02 00 00 00 8f c4 01 02 ac 02")
        );
    }

    #[test]
    fn set_cursor_item_is_an_optional_stack() {
        assert_eq!(
            body(&SetCursorItem {
                stack: &ItemStack::new(1, 2)
            }),
            hex("02 01 00 00")
        );
        assert_eq!(
            body(&SetCursorItem {
                stack: &ItemStack::empty()
            }),
            hex("00")
        );
    }
}
