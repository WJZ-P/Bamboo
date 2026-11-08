//! Mojang 的玩家公钥签名钥匙。
//!
//! 官方 authlib 先问 `https://discovery.minecraftservices.com/minecraft/client`，
//! 再取名叫 `getPublicKeys` 的地址。正式环境那个地址是
//! `https://api.minecraftservices.com/publickeys`。session server 不是 Mojang 时，
//! 改问同一个源的 `/publickeys`，这样本地探针不用访问外网。
//!
//! 钥匙本体是 `playerCertificateKeys[].publicKey`，标准 Base64 的 X.509 SPKI。
//! 验签算法是 `SHA1withRSA`，在游戏逻辑里做，这里只取字节。
//!
//! Mojang's keys that sign player chat certificates.
//!
//! Official authlib asks `https://discovery.minecraftservices.com/minecraft/client` and then
//! follows the endpoint named `getPublicKeys`. In production that URL is
//! `https://api.minecraftservices.com/publickeys`. When the session server is not Mojang's,
//! this asks `{origin}/publickeys` instead, so a local probe never leaves the machine.
//!
//! The key material is `playerCertificateKeys[].publicKey`, standard Base64 of an X.509 SPKI.
//! Verification is `SHA1withRSA` and happens in the game crate. This module only fetches bytes.

use serde_json::Value;

use crate::http::{self, Timeouts};

const OFFICIAL_PUBLIC_KEYS: &str = "https://api.minecraftservices.com/publickeys";
const OFFICIAL_SESSION_HOST: &str = "sessionserver.mojang.com";

/// 由 session server 的地址推出公钥接口。
///
/// The public-key URL that belongs to a session server address.
pub fn public_keys_url(session_server_url: &str) -> String {
    let Some(scheme_end) = session_server_url.find("://") else {
        return OFFICIAL_PUBLIC_KEYS.to_owned();
    };
    let after_scheme = &session_server_url[scheme_end + 3..];
    let host_port = after_scheme.split('/').next().unwrap_or("");
    let host = host_port.split(':').next().unwrap_or("");
    if host.eq_ignore_ascii_case(OFFICIAL_SESSION_HOST) {
        return OFFICIAL_PUBLIC_KEYS.to_owned();
    }
    let origin_len = scheme_end + 3 + host_port.len();
    format!("{}/publickeys", &session_server_url[..origin_len])
}

/// 取 `playerCertificateKeys` 里的公钥 DER。字段缺失或是 `null` 时返回空列表，
/// 和 Gson 把缺字段读成 `null`、authlib 得到空钥匙集一样。
///
/// Fetches the public-key DER values inside `playerCertificateKeys`. A missing or `null` field
/// is an empty list, matching Gson reading an absent field as `null` and authlib ending with no keys.
pub async fn fetch_profile_keys(
    url: &str,
    timeouts: Timeouts,
) -> Result<Vec<Vec<u8>>, ServicesKeyError> {
    let response = http::get(url, timeouts)
        .await
        .map_err(|error| ServicesKeyError::Http(error.to_string()))?;
    if response.status != 200 {
        return Err(ServicesKeyError::Status(response.status));
    }
    parse_profile_keys(&response.body)
}

pub(crate) fn parse_profile_keys(body: &[u8]) -> Result<Vec<Vec<u8>>, ServicesKeyError> {
    let value: Value = serde_json::from_slice(body).map_err(|_| ServicesKeyError::Malformed)?;
    let Some(entries) = value.get("playerCertificateKeys") else {
        return Ok(Vec::new());
    };
    if entries.is_null() {
        return Ok(Vec::new());
    }
    let Some(entries) = entries.as_array() else {
        return Err(ServicesKeyError::Malformed);
    };
    let mut keys = Vec::with_capacity(entries.len());
    for entry in entries {
        let Some(encoded) = entry.get("publicKey").and_then(Value::as_str) else {
            return Err(ServicesKeyError::Malformed);
        };
        let Some(der) = decode_base64(encoded) else {
            return Err(ServicesKeyError::Malformed);
        };
        keys.push(der);
    }
    Ok(keys)
}

/// `java.util.Base64.getDecoder()`：标准字母表，长度必须是 4 的倍数，填充用 `=`。
///
/// `java.util.Base64.getDecoder()`: the standard alphabet, a length that is a multiple of 4, padded with `=`.
fn decode_base64(input: &str) -> Option<Vec<u8>> {
    if !input.len().is_multiple_of(4) {
        return None;
    }
    let mut output = Vec::with_capacity(input.len() / 4 * 3);
    let bytes = input.as_bytes();
    for chunk in bytes.chunks(4) {
        let a = u16::from(alphabet(chunk[0])?);
        let b = u16::from(alphabet(chunk[1])?);
        let c = if chunk[2] == b'=' {
            None
        } else {
            Some(u16::from(alphabet(chunk[2])?))
        };
        let d = if chunk[3] == b'=' {
            None
        } else {
            Some(u16::from(alphabet(chunk[3])?))
        };
        if c.is_none() && d.is_some() {
            return None;
        }
        output.push(((a << 2) | (b >> 4)) as u8);
        if let Some(c) = c {
            output.push(((b << 4) | (c >> 2)) as u8);
        }
        if let Some(d) = d {
            let c = c?;
            output.push(((c << 6) | d) as u8);
        }
    }
    Some(output)
}

fn alphabet(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ServicesKeyError {
    #[error("the profile key service returned HTTP {0}")]
    Status(u16),
    #[error("the profile key document is malformed")]
    Malformed,
    #[error("{0}")]
    Http(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_session_server_uses_the_services_api() {
        let url = public_keys_url("https://sessionserver.mojang.com/session/minecraft/hasJoined");
        assert_eq!(url, OFFICIAL_PUBLIC_KEYS);
    }

    #[test]
    fn a_local_session_server_keeps_its_own_origin() {
        let url = public_keys_url("http://127.0.0.1:34567/session/minecraft/hasJoined");
        assert_eq!(url, "http://127.0.0.1:34567/publickeys");
    }

    #[test]
    fn a_missing_certificate_list_is_an_empty_set() {
        let parsed = parse_profile_keys(br#"{"profilePropertyKeys":[]}"#).unwrap();
        assert!(parsed.is_empty());
    }

    #[test]
    fn a_certificate_key_is_standard_base64() {
        let parsed = parse_profile_keys(
            br#"{"playerCertificateKeys":[{"publicKey":"AQIDBA=="}],"profilePropertyKeys":[]}"#,
        )
        .unwrap();
        assert_eq!(parsed, vec![vec![1, 2, 3, 4]]);
    }
}
