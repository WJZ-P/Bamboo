//! 问 session server 这个玩家是不是正版，对应 authlib 的
//! `com.mojang.authlib.minecraft.client.MinecraftClient` 加
//! `YggdrasilMinecraftSessionService.hasJoinedServer`。
//!
//! 客户端在发 key 包之前，会先把 serverId 哈希告诉 Mojang；服务端拿着同一个哈希来问，
//! 两边对得上 Mojang 才会把这个账号的 UUID 和皮肤给我们。

use std::net::IpAddr;

use mc_bamboo_core::profile::Property;
use serde_json::Value;
use uuid::Uuid;

use crate::http::{self, HttpError, Timeouts};

/// 官方的 session server。authlib 是从一个服务地址清单里取的，取出来就是这个地址。
pub const SESSION_SERVER_URL: &str = "https://sessionserver.mojang.com/session/minecraft/hasJoined";

/// authlib `MinecraftClientHttpException.toAuthenticationException` 按名字认出来的几种错误。
///
/// 它们都变成 `AuthenticationException`，而不是 `AuthenticationUnavailableException`
/// （`javap` 确认这四个类都直接继承 `AuthenticationException`），所以服务端按没验过处理，
/// 哪怕状态码是 5xx。
const UNVERIFIED_ERRORS: [&str; 4] = [
    "ForbiddenOperationException",
    "multiplayer.access.banned",
    "FORCED_USERNAME_CHANGE",
    "InsufficientPrivilegesException",
];
/// 这个状态码在 authlib 里单独判一次，结果是「凭据不对」，也就是没验过。
const UNAUTHORIZED: u16 = 401;
/// authlib 从这个状态码起才认为是服务端自己坏了。
const FIRST_UNAVAILABLE_STATUS: u16 = 500;
/// 304 在 authlib 里和 4xx 一起走错误那条路。
const NOT_MODIFIED: u16 = 304;

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
    /// 问不到结果：连不上、超时，或者 session server 回了 5xx。官方这时说的是
    /// `multiplayer.disconnect.authservers_down`。
    ///
    /// 只有 authlib 的 `AuthenticationUnavailableException` 对应这一项，别的失败都算
    /// [`Unverified`](Self::Unverified)。
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
    timeouts: Timeouts,
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

    let response = http::get(&query, timeouts).await.map_err(unavailable)?;
    if is_error_status(response.status) {
        return Err(error(name, response.status, &response.body));
    }
    // 档案读不出来（回复体是空的、不是 JSON、没有 UUID）在 authlib 里都走 `null` 那条路，
    // 服务端按没验过处理，不是「验证服务器不可用」。
    profile(name, &response.body).ok_or_else(|| AuthError::Unverified(name.to_owned()))
}

/// authlib 的 `MinecraftClient` 对这些状态码抛 `MinecraftClientHttpException`，
/// 其余状态码都会去解析回复体。
fn is_error_status(status: u16) -> bool {
    status == NOT_MODIFIED || status >= 400
}

/// 照搬 authlib `MinecraftClientHttpException.toAuthenticationException` 的判断顺序。
///
/// 先按回复体里的 `error` 名字认几种特例，再看 401，再看 5xx，剩下的都是没验过。
/// 顺序要紧：带这几个名字的 5xx 在官方那边也是没验过，不是服务不可用。
fn error(name: &str, status: u16, body: &[u8]) -> AuthError {
    let unverified = || AuthError::Unverified(name.to_owned());
    if names_an_unverified_error(body) {
        return unverified();
    }
    if status == UNAUTHORIZED {
        return unverified();
    }
    if status >= FIRST_UNAVAILABLE_STATUS {
        return AuthError::Unavailable(format!("the session server answered {status}"));
    }
    unverified()
}

/// 错误回复里的 `error` 名字是不是 authlib 单独认出来的那几个。
///
/// 对应 authlib 的 `ErrorResponse.error` 加 `hasError`，比较时忽略大小写。
/// 回复体读不出来就当没有这个字段，和 authlib 里那个空的 `Optional` 一样。
fn names_an_unverified_error(body: &[u8]) -> bool {
    let Ok(json) = serde_json::from_slice::<Value>(body) else {
        return false;
    };
    let Some(error) = json.get("error").and_then(Value::as_str) else {
        return false;
    };
    UNVERIFIED_ERRORS
        .iter()
        .any(|known| known.eq_ignore_ascii_case(error))
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

/// 连不上、超时、回复不是合法的 HTTP，都归成「验证服务器不可用」。
///
/// 这些在 authlib 那边都是 `IOException`，而 `MinecraftClient` 把 `IOException` 包成
/// `SERVICE_UNAVAILABLE`。注意回复体不是 JSON 不走这条路，那是 `JSON_ERROR`，算没验过。
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

    /// 只有 2xx（以及任何不走错误那条路的状态码）才去读档案。
    #[test]
    fn only_some_statuses_are_errors() {
        for status in [200, 201, 204, 300, 302, 399] {
            assert!(!is_error_status(status), "{status}");
        }
        for status in [304, 400, 401, 403, 404, 429, 500, 503] {
            assert!(is_error_status(status), "{status}");
        }
    }

    /// 4xx、304，以及任何 500 以下的错误状态码，在 authlib 里都变成
    /// `AuthenticationException`，服务端说的是 `unverified_username`。
    #[test]
    fn statuses_below_five_hundred_are_unverified() {
        for status in [304, 400, 401, 403, 404, 429, 451, 499] {
            assert!(
                matches!(error("Notch", status, b""), AuthError::Unverified(_)),
                "{status}"
            );
        }
    }

    /// 5xx 是 authlib 里唯一会抛 `AuthenticationUnavailableException` 的状态码段。
    #[test]
    fn five_hundreds_mean_the_auth_servers_are_down() {
        for status in [500, 502, 503, 599] {
            assert!(
                matches!(error("Notch", status, b""), AuthError::Unavailable(_)),
                "{status}"
            );
        }
    }

    /// 回复体点名了这几种错误时，连 5xx 都算没验过：authlib 先按名字判，再看状态码。
    #[test]
    fn named_errors_win_over_the_status_code() {
        for name in UNVERIFIED_ERRORS {
            let body = format!(r#"{{"error":"{name}","errorMessage":"nope"}}"#);
            assert!(
                matches!(
                    error("Notch", 503, body.as_bytes()),
                    AuthError::Unverified(_)
                ),
                "{name}"
            );
        }
        // 比较忽略大小写，和 authlib 的 `equalsIgnoreCase` 一样。
        let body = br#"{"error":"forbiddenoperationexception"}"#;
        assert!(matches!(
            error("Notch", 500, body),
            AuthError::Unverified(_)
        ));
    }

    /// 别的错误名字不是特例，状态码照常说话。
    #[test]
    fn other_error_names_change_nothing() {
        let body = br#"{"error":"SomethingElse"}"#;
        assert!(matches!(
            error("Notch", 503, body),
            AuthError::Unavailable(_)
        ));
        assert!(matches!(
            error("Notch", 403, body),
            AuthError::Unverified(_)
        ));

        // 回复体不是 JSON、没有 error 字段、error 不是字符串，都当没点名。
        for body in [
            &b""[..],
            &b"<html>oops</html>"[..],
            &br#"{"errorMessage":"nope"}"#[..],
            &br#"{"error":42}"#[..],
        ] {
            assert!(
                matches!(error("Notch", 500, body), AuthError::Unavailable(_)),
                "{:?}",
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
