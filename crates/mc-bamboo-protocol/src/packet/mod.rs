//! 各连接状态下的数据包。
//!
//! 每个文件对应一个连接状态，数据包 ID 都来自 `mc-bamboo-data`。

pub mod chunk;
pub mod configuration;
pub mod handshake;
pub mod login;
pub mod play;
pub mod status;

use mc_bamboo_nbt::Tag;

/// 服务端发给客户端的数据包。
pub trait ClientboundPacket {
    const ID: i32;

    /// 写入数据包 ID 之后的全部内容。
    fn write_body(&self, out: &mut Vec<u8>);
}

/// 写入一段纯文本的聊天组件。Configuration 和 Play 状态用 NBT 传输文本组件。
fn write_text_component(out: &mut Vec<u8>, text: &str) {
    Tag::String(text.to_owned()).write_network(out);
}
