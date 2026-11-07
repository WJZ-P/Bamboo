//! 各连接状态下的数据包。
//!
//! 每个连接状态对应一个模块，数据包 ID 都来自 `mc-bamboo-data`。

pub mod chunk;
pub mod configuration;
pub mod handshake;
pub mod login;
pub mod play;
pub mod status;

/// 服务端发给客户端的数据包。
pub trait ClientboundPacket {
    const ID: i32;

    /// 写入数据包 ID 之后的全部内容。
    fn write_body(&self, out: &mut Vec<u8>);
}

#[cfg(test)]
pub(crate) mod test_util {
    use super::ClientboundPacket;
    use crate::frame::append_frame;

    /// 把规格里的十六进制文本转成字节，空白被忽略。
    pub(crate) fn hex(text: &str) -> Vec<u8> {
        let digits: Vec<u8> = text
            .bytes()
            .filter(|byte| !byte.is_ascii_whitespace())
            .collect();
        digits
            .chunks(2)
            .map(|pair| {
                let pair = str::from_utf8(pair).expect("十六进制文本应该是 ASCII");
                u8::from_str_radix(pair, 16).expect("十六进制文本写错了")
            })
            .collect()
    }

    /// Compound 里的一个 `TAG_String` 条目：类型、键、值。
    pub(crate) fn string_entry(key: &str, value: &str) -> Vec<u8> {
        let mut out = vec![8];
        for text in [key, value] {
            out.extend_from_slice(&(text.len() as u16).to_be_bytes());
            out.extend_from_slice(text.as_bytes());
        }
        out
    }

    /// 数据包 ID 之后的全部内容。
    pub(crate) fn body<P: ClientboundPacket>(packet: &P) -> Vec<u8> {
        let mut out = Vec::new();
        packet.write_body(&mut out);
        out
    }

    /// 完整的一帧。
    pub(crate) fn frame<P: ClientboundPacket>(packet: &P) -> Vec<u8> {
        let mut out = Vec::new();
        append_frame(&mut out, packet).expect("测试里的数据包不会超长");
        out
    }
}
