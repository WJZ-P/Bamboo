//! 玩家档案属性。离线模式没有属性，代理转发时带着皮肤（`textures`）。

use crate::codec::{write_bool, write_string, write_varint};

/// 官方客户端解码档案属性时允许的最多条数。
pub const MAX_PROFILE_PROPERTIES: usize = 16;
/// 官方客户端对属性名、值和签名的长度上限，按 UTF-16 码元计数。
pub const MAX_PROPERTY_NAME_CHARS: usize = 64;
pub const MAX_PROPERTY_VALUE_CHARS: usize = 32767;
pub const MAX_PROPERTY_SIGNATURE_CHARS: usize = 1024;

/// 一条档案属性，比如签名过的皮肤。
///
/// 类型定义在 `mc-bamboo-core` 里：正版验证层算出它，协议层写进包里，两边都够不着对方。
pub use mc_bamboo_core::profile::Property as ProfileProperty;

/// 按官方 `GAME_PROFILE_PROPERTIES` 的格式写属性列表：条数、名字、值、有无签名、签名。
pub(crate) fn write_profile_properties(out: &mut Vec<u8>, properties: &[ProfileProperty]) {
    write_varint(out, properties.len() as i32);
    for property in properties {
        write_string(out, &property.name);
        write_string(out, &property.value);
        write_bool(out, property.signature.is_some());
        if let Some(signature) = &property.signature {
            write_string(out, signature);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::Reader;

    #[test]
    fn properties_round_trip_like_the_game() {
        let properties = [
            ProfileProperty {
                name: "textures".to_owned(),
                value: "value".to_owned(),
                signature: Some("signature".to_owned()),
            },
            ProfileProperty {
                name: "other".to_owned(),
                value: "value".to_owned(),
                signature: None,
            },
        ];
        let mut out = Vec::new();
        write_profile_properties(&mut out, &properties);

        let mut reader = Reader::new(&out);
        assert_eq!(reader.count(MAX_PROFILE_PROPERTIES), Ok(2));
        assert_eq!(reader.string(MAX_PROPERTY_NAME_CHARS), Ok("textures"));
        assert_eq!(reader.string(MAX_PROPERTY_VALUE_CHARS), Ok("value"));
        assert_eq!(reader.bool(), Ok(true));
        assert_eq!(reader.string(MAX_PROPERTY_SIGNATURE_CHARS), Ok("signature"));
        assert_eq!(reader.string(MAX_PROPERTY_NAME_CHARS), Ok("other"));
        assert_eq!(reader.string(MAX_PROPERTY_VALUE_CHARS), Ok("value"));
        assert_eq!(reader.bool(), Ok(false));
        assert_eq!(reader.finish(), Ok(()));
    }

    #[test]
    fn an_empty_list_is_just_a_zero_count() {
        let mut out = Vec::new();
        write_profile_properties(&mut out, &[]);
        assert_eq!(out, [0]);
    }

    /// 逐字节对照一遍，省得解码端和编码端一起写错。
    #[test]
    fn properties_are_written_like_a_property_map() {
        let mut out = Vec::new();
        write_profile_properties(
            &mut out,
            &[
                ProfileProperty {
                    name: "a".to_owned(),
                    value: "b".to_owned(),
                    signature: Some("c".to_owned()),
                },
                ProfileProperty {
                    name: "d".to_owned(),
                    value: "e".to_owned(),
                    signature: None,
                },
            ],
        );
        assert_eq!(
            out,
            [
                0x02, // 两项
                0x01, b'a', 0x01, b'b', 0x01, 0x01, b'c', // 带签名
                0x01, b'd', 0x01, b'e', 0x00, // 不带签名
            ]
        );
    }
}
