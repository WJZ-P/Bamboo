//! 问 session server 这个玩家是不是正版，对应 authlib 的
//! `com.mojang.authlib.minecraft.client.MinecraftClient` 加
//! `YggdrasilMinecraftSessionService.hasJoinedServer`。
//!
//! 客户端在发 key 包之前，会先把 serverId 哈希告诉 Mojang；服务端拿着同一个哈希来问，
//! 两边对得上 Mojang 才会把这个账号的 UUID 和皮肤给我们。

use std::{net::IpAddr, time::Duration};

use mc_bamboo_core::profile::Property;
use serde_json::Value;
use uuid::Uuid;

use crate::http::{self, HttpError};

/// 官方的 session server。authlib 是从一个服务地址清单里取的，取出来就是这个地址。
pub const SESSION_SERVER_URL: &str = "https://sessionserver.mojang.com/session/minecraft/hasJoined";

/// session server 认下来的账号。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub uuid: Uuid,
    pub name: String,
    pub properties: Vec<Property>,
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    /// session server 明确表示这个名字没有在登录这台服务器。官方这时说的是
    /// `multiplayer.disconnect.unverified_username`。
    #[error("the session server did not verify {0}")]
    Unverified(String),
    /// 问不到结果：连不上、超时，或者 session server 自己出错。官方这时说的是
    /// `multiplayer.disconnect.authservers_down`。
    #[error("the session server is unavailable: {0}")]
    Unavailable(String),
}

/// 向 session server 查这次登录。
///
/// `ip` 是开了 `prevent-proxy-connections` 时要带上的客户端地址，对应官方
/// `ServerLoginPacketListenerImpl` 里那个可选的 `ip` 参数。
pub async fn has_joined(
    url: &str,
    name: &str,
    server_id_hash: &str,
    ip: Option<IpAddr>,
    deadline: Duration,
) -> Result<Profile, AuthError> {
    let mut query = format!(
        "{url}?username={}&serverId={}",
        encode_query(name),
        encode_query(server_id_hash)
    );
    if let Some(ip) = ip {
        query.push_str("&ip=");
        query.push_str(&encode_query(&ip.to_string()));
    }

    let response = http::get(&query, deadline).await.map_err(unavailable)?;
    match response.status {
        // 回复体是空的说明 Mojang 没认下这次登录，和 authlib 里 `null` 的那条路一样。
        200..=299 if response.body.iter().all(u8::is_ascii_whitespace) => {
            Err(AuthError::Unverified(name.to_owned()))
        }
        200..=299 => profile(name, &response.body).ok_or_else(|| {
            AuthError::Unavailable("the session server sent a profile we cannot read".to_owned())
        }),
        // 4xx 是我们把请求写坏了或者凭据不对，官方也按没验过处理。
        400..=499 => Err(AuthError::Unverified(name.to_owned())),
        status => Err(AuthError::Unavailable(format!(
            "the session server answered {status}"
        ))),
    }
}

/// 读出回复里的档案。
///
/// 名字用我们问的那个，不用回复里的：官方 `hasJoinedServer` 建的 `GameProfile`
/// 就是拿请求里的用户名配回复里的 UUID。
fn profile(name: &str, body: &[u8]) -> Option<Profile> {
    let json: Value = serde_json::from_slice(body).ok()?;
    let uuid = Uuid::parse_str(json.get("id")?.as_str()?).ok()?;
    let properties = match json.get("properties") {
        Some(Value::Array(items)) => items.iter().map(property).collect::<Option<Vec<_>>>()?,
        // 没有皮肤的账号就是没有这一项。
        None | Some(Value::Null) => Vec::new(),
        Some(_) => return None,
    };
    Some(Profile {
        uuid,
        name: name.to_owned(),
        properties,
    })
}

fn property(json: &Value) -> Option<Property> {
    Some(Property {
        name: json.get("name")?.as_str()?.to_owned(),
        value: json.get("value")?.as_str()?.to_owned(),
        // 皮肤的签名是可选的，但是少了签名客户端就不显示披风。
        signature: json
            .get("signature")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

/// 连不上、超时、回复坏了，都归成「验证服务器不可用」。
fn unavailable(error: HttpError) -> AuthError {
    AuthError::Unavailable(error.to_string())
}

/// 按 `java.net.URLEncoder` 的规矩转义查询串里的值。
///
/// 玩家名里理论上可以有 `&`，不转义会把查询串拼坏。
fn encode_query(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";

    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'*' => {
                encoded.push(char::from(byte));
            }
            b' ' => encoded.push('+'),
            _ => {
                encoded.push('%');
                encoded.push(char::from(HEX[usize::from(byte >> 4)]));
                encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
            }
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: &str = r#"{
        "id": "069a79f444e94726a5befca90e38aaf5",
        "name": "Notch",
        "properties": [
            {"name": "textures", "value": "ewogICJ0aW1lc3RhbXAi", "signature": "Yml0cw=="}
        ]
    }"#;

    #[test]
    fn reads_a_profile() {
        let profile = profile("Notch", BODY.as_bytes()).expect("这段 JSON 应该能读");
        assert_eq!(
            profile.uuid,
            Uuid::parse_str("069a79f4-44e9-4726-a5be-fca90e38aaf5").expect("uuid")
        );
        assert_eq!(profile.name, "Notch");
        assert_eq!(
            profile.properties,
            vec![Property {
                name: "textures".to_owned(),
                value: "ewogICJ0aW1lc3RhbXAi".to_owned(),
                signature: Some("Yml0cw==".to_owned()),
            }]
        );
    }

    /// 回复里的大小写不一定和我们问的一样，但是官方用的是我们问的那个名字。
    #[test]
    fn keeps_the_name_we_asked_about() {
        let profile = profile("notch", BODY.as_bytes()).expect("这段 JSON 应该能读");
        assert_eq!(profile.name, "notch");
    }

    #[test]
    fn a_profile_without_properties_is_fine() {
        let body = br#"{"id":"069a79f444e94726a5befca90e38aaf5","name":"Notch"}"#;
        let profile = profile("Notch", body).expect("这段 JSON 应该能读");
        assert!(profile.properties.is_empty());
    }

    #[test]
    fn a_property_can_come_without_a_signature() {
        let body = br#"{"id":"069a79f444e94726a5befca90e38aaf5","properties":[{"name":"textures","value":"x"}]}"#;
        let profile = profile("Notch", body).expect("这段 JSON 应该能读");
        assert_eq!(profile.properties[0].signature, None);
    }

    #[test]
    fn rejects_bodies_it_cannot_use() {
        for body in [
            &b""[..],
            &b"not json"[..],
            &b"{}"[..],
            // UUID 坏了
            &br#"{"id":"not a uuid"}"#[..],
            &br#"{"id":42}"#[..],
            // 属性坏了
            &br#"{"id":"069a79f444e94726a5befca90e38aaf5","properties":"textures"}"#[..],
            &br#"{"id":"069a79f444e94726a5befca90e38aaf5","properties":[{"name":"textures"}]}"#[..],
        ] {
            assert!(
                profile("Notch", body).is_none(),
                "{}",
                String::from_utf8_lossy(body)
            );
        }
    }

    #[test]
    fn escapes_the_query() {
        assert_eq!(encode_query("Notch"), "Notch");
        assert_eq!(encode_query("-7c9d5b00"), "-7c9d5b00");
        assert_eq!(encode_query("a&b=c d/e"), "a%26b%3Dc+d%2Fe");
        assert_eq!(encode_query("皮"), "%E7%9A%AE");
    }
}
