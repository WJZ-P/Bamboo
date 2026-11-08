//! 代理转发：信任代理给的 UUID、名字和皮肤，而不是自己用玩家名算一个。
//!
//! 两种格式都来自官方生态：Velocity 的现代转发在登录时用插件询问交换一段带
//! HMAC-SHA256 签名的数据；BungeeCord 的旧式转发把同样的内容用 NUL 拼进握手包的服务器地址。

use std::net::IpAddr;

use hmac::{Hmac, Mac};
use mc_bamboo_protocol::{
    codec::{DecodeError, Reader},
    frame::append_frame,
    packet::{
        login::{LoginPluginRequest, LoginRequest, MAX_PLAYER_NAME_CHARS},
        profile::{
            MAX_PROFILE_PROPERTIES, MAX_PROPERTY_NAME_CHARS, MAX_PROPERTY_SIGNATURE_CHARS,
            MAX_PROPERTY_VALUE_CHARS, ProfileProperty,
        },
    },
};
use serde::Deserialize;
use sha2::Sha256;
use uuid::Uuid;

use super::{Connection, ConnectionError};
use crate::{config::ForwardingMode, server::Shared};

/// Velocity 现代转发用的插件频道。
const VELOCITY_CHANNEL: &str = "velocity:player_info";
/// 本服务端支持的最高转发版本，4 是 `LAZY_SESSION`。
const MAX_FORWARDING_VERSION: i32 = 4;
const SIGNATURE_LEN: usize = 32;
/// 官方后端解析转发的玩家地址时用的上限，和原版握手的 255 不同。
const MAX_FORWARDED_ADDRESS_CHARS: usize = 32767;
/// Spigot 只把名字匹配 `\w{0,16}` 的属性放进旧式转发的档案。
const MAX_LEGACY_PROPERTY_NAME_CHARS: usize = 16;

/// 代理说客户端不认识频道时的断开原因，和官方一致。
const REQUIRES_VELOCITY: &str = "This server requires you to connect with Velocity.";
/// 签名对不上时的断开原因，和官方一致。
const BAD_SIGNATURE: &str = "Unable to verify player details";
/// legacy 数据不合法时的断开原因，和 Spigot 一致。
pub(super) const BAD_LEGACY_DATA: &str =
    "If you wish to use IP forwarding, please enable it in your BungeeCord config as well!";

/// 这次登录的身份从哪里来。
pub(super) enum Identity {
    /// 不转发：用玩家名算离线 UUID。
    Offline,
    /// 握手地址里已经带着 UUID 和属性。
    Legacy(LegacyIdentity),
    /// 先读玩家名，再向代理要数据。
    Modern,
}

pub(super) struct LegacyIdentity {
    pub(super) uuid: Uuid,
    pub(super) properties: Vec<ProfileProperty>,
}

pub(super) struct ModernProfile {
    pub(super) uuid: Uuid,
    pub(super) name: String,
    pub(super) properties: Vec<ProfileProperty>,
}

/// 按配置决定从握手地址里拿身份，还是等登录时的插件询问。
pub(super) fn identity(mode: ForwardingMode, address: &str) -> Result<Identity, ForwardingError> {
    match mode {
        ForwardingMode::Off => Ok(Identity::Offline),
        ForwardingMode::Legacy => parse_legacy(address).map(Identity::Legacy),
        ForwardingMode::Modern => Ok(Identity::Modern),
    }
}

/// 向代理要玩家信息。玩家不配合转发或签名不对时，已经发出了断开消息，返回 `None`。
pub(super) async fn authenticate(
    connection: &mut Connection,
    shared: &Shared,
) -> Result<Option<ModernProfile>, ConnectionError> {
    let requested_id = Uuid::new_v4().as_u128() as i32;
    let mut frame = Vec::new();
    append_frame(
        &mut frame,
        &LoginPluginRequest {
            message_id: requested_id,
            channel: VELOCITY_CHANNEL,
            data: &[MAX_FORWARDING_VERSION as u8],
        },
    )?;
    connection.write(&frame).await?;

    // 官方会忽略事务 ID 不对的回应，继续等真正的那个。
    let data = loop {
        let Some(payload) = connection.next_frame().await? else {
            return Ok(None);
        };
        let LoginRequest::PluginResponse { message_id, data } = LoginRequest::decode(&payload)?
        else {
            return Err(ConnectionError::UnexpectedPacket);
        };
        if message_id == requested_id {
            // 包缓冲在这一轮结束时释放，回应的内容要复制出来。
            break data.map(<[u8]>::to_vec);
        }
    };
    let Some(data) = data else {
        connection.refuse_login(REQUIRES_VELOCITY).await?;
        return Ok(None);
    };

    match parse_modern(&data, &shared.config.forwarding_secret) {
        Ok(profile) => Ok(Some(profile)),
        Err(ModernError::MissingSignature | ModernError::BadSignature) => {
            connection.refuse_login(BAD_SIGNATURE).await?;
            Ok(None)
        }
        Err(error) => Err(ConnectionError::InvalidForwarding(error.to_string())),
    }
}

/// 校验签名并解析 Velocity 的回应。签名的前半段是 HMAC-SHA256，后面才是内容。
fn parse_modern(payload: &[u8], secret: &str) -> Result<ModernProfile, ModernError> {
    let (signature, data) = payload
        .split_at_checked(SIGNATURE_LEN)
        .ok_or(ModernError::MissingSignature)?;
    if !signature_matches(secret, signature, data) {
        return Err(ModernError::BadSignature);
    }

    let mut reader = Reader::new(data);
    let version = reader.varint()?;
    if !(1..=MAX_FORWARDING_VERSION).contains(&version) {
        return Err(ModernError::UnsupportedVersion(version));
    }
    // 地址只用来校验代理没有乱填，服务端的游戏逻辑用不到它。
    if reader
        .string(MAX_FORWARDED_ADDRESS_CHARS)?
        .parse::<IpAddr>()
        .is_err()
    {
        return Err(ModernError::InvalidAddress);
    }
    let uuid = reader.uuid()?;
    let name = reader.string(MAX_PLAYER_NAME_CHARS)?.to_owned();
    // 版本 2、3 后面还有密钥数据，版本 4 没有；都和官方一样，读到这里就够。
    let properties = read_properties(&mut reader)?;
    Ok(ModernProfile {
        uuid,
        name,
        properties,
    })
}

/// 用配置里的密钥验证 `signature` 是不是 `data` 的 HMAC-SHA256。比较是常数时间的。
fn signature_matches(secret: &str, signature: &[u8], data: &[u8]) -> bool {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC 接受任意长度的密钥");
    mac.update(data);
    mac.verify_slice(signature).is_ok()
}

/// 按官方 `GAME_PROFILE_PROPERTIES` 的格式读属性列表。
fn read_properties(reader: &mut Reader<'_>) -> Result<Vec<ProfileProperty>, DecodeError> {
    let count = reader.count(MAX_PROFILE_PROPERTIES)?;
    let mut properties = Vec::with_capacity(count);
    for _ in 0..count {
        let name = reader.string(MAX_PROPERTY_NAME_CHARS)?.to_owned();
        let value = reader.string(MAX_PROPERTY_VALUE_CHARS)?.to_owned();
        let signature = if reader.bool()? {
            Some(reader.string(MAX_PROPERTY_SIGNATURE_CHARS)?.to_owned())
        } else {
            None
        };
        properties.push(ProfileProperty {
            name,
            value,
            signature,
        });
    }
    Ok(properties)
}

/// 解析 BungeeCord 的旧式握手地址：`服务器地址\0玩家IP\0不带横线的UUID[\0属性JSON]`。
fn parse_legacy(address: &str) -> Result<LegacyIdentity, ForwardingError> {
    let mut parts: Vec<&str> = address.split('\0').collect();
    // Java 的 split 会丢掉末尾的空串，比如 BungeeCord 发来的地址以 NUL 结尾也还是三段。
    while parts.last() == Some(&"") {
        parts.pop();
    }
    if !(3..=4).contains(&parts.len()) {
        return Err(ForwardingError::WrongShape);
    }

    if !is_forwarded_address(parts[1]) {
        return Err(ForwardingError::InvalidAddress);
    }
    let uuid = Uuid::try_parse(parts[2]).map_err(|_| ForwardingError::InvalidUuid)?;
    let properties = match parts.get(3) {
        Some(json) => parse_legacy_properties(json)?,
        None => Vec::new(),
    };
    Ok(LegacyIdentity { uuid, properties })
}

/// Spigot 用 `[0-9a-f.:]{0,45}` 限制代理给的玩家地址，这里保持同样的规则。
fn is_forwarded_address(address: &str) -> bool {
    address.len() <= 45
        && address
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f' | b'.' | b':'))
}

/// BungeeCord 把属性写成 Gson 的 `Property[]`。未知字段和缺失的签名都和官方一样处理。
#[derive(Debug, Deserialize)]
struct LegacyProperty {
    name: String,
    value: String,
    #[serde(default)]
    signature: Option<String>,
}

fn parse_legacy_properties(json: &str) -> Result<Vec<ProfileProperty>, ForwardingError> {
    // Gson 把空字符串和字面量 null 都解析成空列表。
    if json.trim().is_empty() || json.trim() == "null" {
        return Ok(Vec::new());
    }
    let raw: Vec<LegacyProperty> =
        serde_json::from_str(json).map_err(|_| ForwardingError::InvalidProperties)?;

    let mut properties = Vec::with_capacity(raw.len());
    for property in raw {
        // 官方只接受名字匹配 `\w{0,16}` 的属性，其余直接丢掉。
        if utf16_len(&property.name) > MAX_LEGACY_PROPERTY_NAME_CHARS || !is_word(&property.name) {
            continue;
        }
        // 超过协议上限的属性写进客户端会让人被踢，宁可不带。
        let fits = utf16_len(&property.value) <= MAX_PROPERTY_VALUE_CHARS
            && property
                .signature
                .as_deref()
                .is_none_or(|signature| utf16_len(signature) <= MAX_PROPERTY_SIGNATURE_CHARS);
        if !fits {
            continue;
        }
        properties.push(ProfileProperty {
            name: property.name,
            value: property.value,
            signature: property.signature,
        });
    }
    if properties.len() > MAX_PROFILE_PROPERTIES {
        return Err(ForwardingError::InvalidProperties);
    }
    Ok(properties)
}

/// Java 的 `\w` 默认只匹配 ASCII 字母、数字和下划线。
fn is_word(text: &str) -> bool {
    text.bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

/// 协议里的长度上限按 UTF-16 码元计数。
fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

#[derive(Debug, thiserror::Error)]
pub(super) enum ForwardingError {
    #[error("legacy forwarding data does not have 3 or 4 fields")]
    WrongShape,
    #[error("legacy forwarding data has an invalid player address")]
    InvalidAddress,
    #[error("legacy forwarding data has an invalid UUID")]
    InvalidUuid,
    #[error("legacy forwarding data has invalid properties")]
    InvalidProperties,
}

#[derive(Debug, thiserror::Error)]
pub(super) enum ModernError {
    #[error("forwarding response is shorter than a signature")]
    MissingSignature,
    #[error("forwarding response signature does not match")]
    BadSignature,
    #[error(transparent)]
    Decode(#[from] DecodeError),
    #[error("forwarding version {0} is not supported")]
    UnsupportedVersion(i32),
    #[error("forwarded player address is not an IP address")]
    InvalidAddress,
}

#[cfg(test)]
mod tests {
    use mc_bamboo_protocol::codec::{write_bool, write_string, write_uuid, write_varint};

    use super::*;

    const SECRET: &str = "bamboo-secret";
    const UUID: Uuid = Uuid::from_u128(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef);

    fn textures() -> ProfileProperty {
        ProfileProperty {
            name: "textures".to_owned(),
            value: "value".to_owned(),
            signature: Some("signature".to_owned()),
        }
    }

    fn modern_data(
        version: i32,
        address: &str,
        uuid: Uuid,
        name: &str,
        properties: &[ProfileProperty],
    ) -> Vec<u8> {
        let mut data = Vec::new();
        write_varint(&mut data, version);
        write_string(&mut data, address);
        write_uuid(&mut data, uuid);
        write_string(&mut data, name);
        write_varint(&mut data, properties.len() as i32);
        for property in properties {
            write_string(&mut data, &property.name);
            write_string(&mut data, &property.value);
            write_bool(&mut data, property.signature.is_some());
            if let Some(signature) = &property.signature {
                write_string(&mut data, signature);
            }
        }
        data
    }

    fn sign(secret: &str, data: &[u8]) -> [u8; SIGNATURE_LEN] {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(data);
        mac.finalize().into_bytes().into()
    }

    fn signed_payload(data: &[u8]) -> Vec<u8> {
        let mut payload = sign(SECRET, data).to_vec();
        payload.extend_from_slice(data);
        payload
    }

    fn hex(text: &str) -> Vec<u8> {
        let digits: Vec<u8> = text
            .bytes()
            .filter(|byte| !byte.is_ascii_whitespace())
            .collect();
        digits
            .chunks(2)
            .map(|pair| {
                let pair = std::str::from_utf8(pair).unwrap();
                u8::from_str_radix(pair, 16).unwrap()
            })
            .collect()
    }

    #[test]
    fn hmac_matches_the_rfc_test_vector() {
        // RFC 4231 第 2 个用例：key 是 "Jefe"。
        let signature = hex("5b dc c1 46 bf 60 75 4e 6a 04 24 26 08 95 75 c7
             5a 00 3f 08 9d 27 39 83 9d ec 58 b9 64 ec 38 43");
        assert!(signature_matches(
            "Jefe",
            &signature,
            b"what do ya want for nothing?"
        ));
    }

    #[test]
    fn parses_a_signed_modern_response() {
        let data = modern_data(4, "203.0.113.7", UUID, "ProxiedKitten", &[textures()]);
        let profile = parse_modern(&signed_payload(&data), SECRET).unwrap();

        assert_eq!(profile.uuid, UUID);
        assert_eq!(profile.name, "ProxiedKitten");
        assert_eq!(profile.properties, [textures()]);
    }

    #[test]
    fn a_tampered_payload_fails_verification() {
        let data = modern_data(4, "203.0.113.7", UUID, "Kitten", &[]);
        let mut payload = signed_payload(&data);
        let last = payload.len() - 1;
        payload[last] ^= 0x01;

        assert!(matches!(
            parse_modern(&payload, SECRET),
            Err(ModernError::BadSignature)
        ));
    }

    #[test]
    fn a_payload_without_a_signature_is_rejected() {
        assert!(matches!(
            parse_modern(&[0; SIGNATURE_LEN - 1], SECRET),
            Err(ModernError::MissingSignature)
        ));
    }

    #[test]
    fn a_response_for_another_secret_is_rejected() {
        let data = modern_data(4, "203.0.113.7", UUID, "Kitten", &[]);
        let payload = signed_payload(&data);

        assert!(matches!(
            parse_modern(&payload, "another-secret"),
            Err(ModernError::BadSignature)
        ));
    }

    #[test]
    fn newer_forwarding_versions_are_rejected() {
        let data = modern_data(5, "203.0.113.7", UUID, "Kitten", &[]);
        assert!(matches!(
            parse_modern(&signed_payload(&data), SECRET),
            Err(ModernError::UnsupportedVersion(5))
        ));
    }

    #[test]
    fn version_one_without_key_data_is_accepted() {
        let data = modern_data(1, "2001:db8::1", UUID, "Kitten", &[]);
        assert!(parse_modern(&signed_payload(&data), SECRET).is_ok());
    }

    #[test]
    fn the_forwarded_address_must_be_an_ip() {
        let data = modern_data(4, "localhost", UUID, "Kitten", &[]);
        assert!(matches!(
            parse_modern(&signed_payload(&data), SECRET),
            Err(ModernError::InvalidAddress)
        ));
    }

    #[test]
    fn anonymous_properties_round_trip() {
        let property = ProfileProperty {
            name: "textures".to_owned(),
            value: "value".to_owned(),
            signature: None,
        };
        let data = modern_data(
            4,
            "203.0.113.7",
            UUID,
            "Kitten",
            std::slice::from_ref(&property),
        );
        let profile = parse_modern(&signed_payload(&data), SECRET).unwrap();
        assert_eq!(profile.properties, [property]);
    }

    #[test]
    fn a_truncated_response_is_not_panicking() {
        let data = modern_data(4, "203.0.113.7", UUID, "Kitten", &[]);
        let payload = signed_payload(&data);
        for len in SIGNATURE_LEN..payload.len() {
            let _ = parse_modern(&payload[..len], SECRET);
        }
    }

    #[test]
    fn parses_a_legacy_address_without_properties() {
        let identity =
            parse_legacy("localhost\x00203.0.113.7\x000123456789abcdef0123456789abcdef").unwrap();

        assert_eq!(identity.uuid, UUID);
        assert!(identity.properties.is_empty());
    }

    #[test]
    fn parses_a_legacy_address_with_properties() {
        let json = r#"[{"name":"textures","value":"value","signature":"signature"},
                       {"name":"other","value":"value"}]"#;
        let address = format!("localhost\x00203.0.113.7\0{UUID}\0{json}");
        let identity = parse_legacy(&address).unwrap();

        assert_eq!(
            identity.properties,
            [
                textures(),
                ProfileProperty {
                    name: "other".to_owned(),
                    value: "value".to_owned(),
                    signature: None,
                }
            ]
        );
    }

    #[test]
    fn legacy_trailing_nul_is_ignored() {
        let address = format!("localhost\x00203.0.113.7\0{UUID}\0\0");
        assert!(parse_legacy(&address).is_ok());
        assert!(parse_legacy(&format!("localhost\x00203.0.113.7\0{UUID}\0")).is_ok());
    }

    #[test]
    fn legacy_needs_three_fields() {
        for address in ["localhost", "localhost\x00203.0.113.7", "a\0b\0c\0d\0e"] {
            assert!(
                matches!(parse_legacy(address), Err(ForwardingError::WrongShape)),
                "{address:?}"
            );
        }
    }

    #[test]
    fn legacy_address_must_match_the_spigot_pattern() {
        let uuid = UUID.to_string();
        for address in ["A", "g", " ", "-", &"a".repeat(46)] {
            let host = format!("localhost\0{address}\0{uuid}");
            assert!(
                matches!(parse_legacy(&host), Err(ForwardingError::InvalidAddress)),
                "{address:?}"
            );
        }
        // 空地址和 IPv6 都被 Spigot 的规则放行。
        assert!(parse_legacy(&format!("localhost\0\0{uuid}")).is_ok());
        assert!(parse_legacy(&format!("localhost\0::1\0{uuid}")).is_ok());
    }

    #[test]
    fn legacy_uuid_can_be_undashed_or_dashed() {
        let simple = "0123456789abcdef0123456789abcdef";
        let dashed = "01234567-89ab-cdef-0123-456789abcdef";
        for uuid in [simple, dashed] {
            let address = format!("localhost\x00203.0.113.7\0{uuid}");
            assert_eq!(parse_legacy(&address).unwrap().uuid, UUID);
        }

        let address = "localhost\x00203.0.113.7\0not-a-uuid";
        assert!(matches!(
            parse_legacy(address),
            Err(ForwardingError::InvalidUuid)
        ));
    }

    #[test]
    fn legacy_properties_must_be_a_json_array() {
        for json in ["{}", "[{\"name\":1}]"] {
            let address = format!("localhost\x00203.0.113.7\0{UUID}\0{json}");
            assert!(
                matches!(
                    parse_legacy(&address),
                    Err(ForwardingError::InvalidProperties)
                ),
                "{json:?}"
            );
        }
    }

    #[test]
    fn empty_or_null_legacy_properties_mean_none() {
        for json in ["", "null", " [] "] {
            let address = format!("localhost\x00203.0.113.7\0{UUID}\0{json}");
            assert!(parse_legacy(&address).unwrap().properties.is_empty());
        }
    }

    #[test]
    fn odd_legacy_properties_are_dropped_like_the_official_server() {
        let json = r#"[{"name":"textures","value":"kept"},
                       {"name":"has space","value":"dropped"},
                       {"name":"this_name_is_too_long","value":"dropped"},
                       {"name":"wide","value":"kept"}]"#;
        let address = format!("localhost\x00203.0.113.7\0{UUID}\0{json}");
        let names: Vec<String> = parse_legacy(&address)
            .unwrap()
            .properties
            .into_iter()
            .map(|property| property.name)
            .collect();

        assert_eq!(names, ["textures", "wide"]);
    }

    #[test]
    fn legacy_properties_that_do_not_fit_are_dropped() {
        let value = "x".repeat(MAX_PROPERTY_VALUE_CHARS + 1);
        let json = format!(r#"[{{"name":"textures","value":"{value}"}}]"#);
        let address = format!("localhost\x00203.0.113.7\0{UUID}\0{json}");
        assert!(parse_legacy(&address).unwrap().properties.is_empty());
    }

    #[test]
    fn the_mode_decides_where_the_identity_comes_from() {
        assert!(matches!(
            identity(ForwardingMode::Off, "localhost\0a\0b"),
            Ok(Identity::Offline)
        ));
        assert!(matches!(
            identity(ForwardingMode::Modern, "localhost"),
            Ok(Identity::Modern)
        ));
        assert!(matches!(
            identity(
                ForwardingMode::Legacy,
                "localhost\x00203.0.113.7\x000123456789abcdef0123456789abcdef"
            ),
            Ok(Identity::Legacy(_))
        ));
        assert!(identity(ForwardingMode::Legacy, "localhost").is_err());
    }
}
