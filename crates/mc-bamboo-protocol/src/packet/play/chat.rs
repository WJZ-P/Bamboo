//! 服务端发给客户端的聊天消息。

use mc_bamboo_data::packets::play::clientbound;

use crate::{
    codec::{write_bool, write_varint},
    packet::ClientboundPacket,
    text::Component,
};

/// 系统消息：加入和离开提示、错误提示。玩家聊天用 [`DisguisedChat`]。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SystemChat<'a> {
    pub content: &'a Component,
    /// 显示在快捷栏上方，而不是聊天框。
    pub overlay: bool,
}

impl ClientboundPacket for SystemChat<'_> {
    const ID: i32 = clientbound::SYSTEM_CHAT;

    fn write_body(&self, out: &mut Vec<u8>) {
        self.content.write(out);
        write_bool(out, self.overlay);
    }
}

/// 没有签名的玩家聊天，客户端按聊天类型拼出 `<Name> message`。
///
/// 内容和接收者无关，所以编码一次就可以发给所有人。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DisguisedChat<'a> {
    pub message: &'a Component,
    /// `minecraft:chat_type` 注册表 ID。线上的值比它大一，由这里加上。
    pub chat_type: i32,
    /// 发送者的名字。
    pub name: &'a Component,
    /// 目标的名字，普通聊天没有。
    pub target_name: Option<&'a Component>,
}

impl ClientboundPacket for DisguisedChat<'_> {
    const ID: i32 = clientbound::DISGUISED_CHAT;

    fn write_body(&self, out: &mut Vec<u8>) {
        self.message.write(out);
        write_varint(out, self.chat_type + 1);
        self.name.write(out);
        write_bool(out, self.target_name.is_some());
        if let Some(target_name) = self.target_name {
            target_name.write(out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        packet::test_util::{body, frame, hex, string_entry},
        text::Color,
    };

    #[test]
    fn join_message_frame_matches_official_bytes() {
        let content = Component::translate("multiplayer.player.joined", ["Kitten".into()])
            .color(Color::Yellow);
        let packet = SystemChat {
            content: &content,
            overlay: false,
        };
        assert_eq!(
            frame(&packet),
            hex("4f 7c
                 0a
                 08 00 09 74 72 61 6e 73 6c 61 74 65
                    00 19 6d 75 6c 74 69 70 6c 61 79 65 72 2e 70 6c 61 79 65 72 2e 6a 6f 69 6e 65 64
                 09 00 04 77 69 74 68
                    08 00 00 00 01
                    00 06 4b 69 74 74 65 6e
                 08 00 05 63 6f 6c 6f 72
                    00 06 79 65 6c 6c 6f 77
                 00
                 00")
        );
    }

    #[test]
    fn overlay_message_has_the_official_entries() {
        let content = Component::translate("build.tooHigh", [319.into()]).color(Color::Red);
        let packet = SystemChat {
            content: &content,
            overlay: true,
        };

        // 官方的键序是 with、color、translate，顺序不影响解码。条目本身的字节和官方相同。
        let mut with = hex("09 00 04");
        with.extend_from_slice(b"with");
        with.extend(hex("03 00 00 00 01 00 00 01 3f"));

        let mut expected = vec![0x0a];
        expected.extend(string_entry("translate", "build.tooHigh"));
        expected.extend(with);
        expected.extend(string_entry("color", "red"));
        expected.extend([0x00, 0x01]);
        assert_eq!(body(&packet), expected);
    }

    #[test]
    fn disguised_chat_frame_matches_official_bytes() {
        let message = Component::text("hi");
        let name = Component::text("Kitten");
        let packet = DisguisedChat {
            message: &message,
            chat_type: 0,
            name: &name,
            target_name: None,
        };
        assert_eq!(
            frame(&packet),
            hex("11 21
                 08 00 02 68 69
                 01
                 08 00 06 4b 69 74 74 65 6e
                 00")
        );
    }

    #[test]
    fn disguised_chat_can_name_a_target() {
        let text = Component::text("a");
        let packet = DisguisedChat {
            message: &text,
            chat_type: 1,
            name: &text,
            target_name: Some(&text),
        };
        assert_eq!(
            body(&packet),
            hex("08 00 01 61  02  08 00 01 61  01  08 00 01 61")
        );
    }
}
