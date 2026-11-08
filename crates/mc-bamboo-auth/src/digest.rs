//! serverId 哈希，对应官方 `Crypt.digestData` 加上
//! `ServerLoginPacketListenerImpl` 里的 `new BigInteger(digest).toString(16)`。

use sha1::{Digest, Sha1};

use crate::cipher::SharedSecret;

/// 服务端和客户端各自算出这个字符串，再分别交给 session server，Mojang 用它判断
/// 两边说的是不是同一次登录。
///
/// `server_id` 一直是空串：官方 `ServerLoginPacketListenerImpl.serverId` 就是 `""`，
/// 这里留着参数只是为了照着官方的算式写。
pub fn server_id_hash(server_id: &str, secret: &SharedSecret, public_key_der: &[u8]) -> String {
    minecraft_hex_digest(&[server_id.as_bytes(), secret, public_key_der])
}

/// 官方风格的十六进制摘要：SHA-1 的 20 个字节当成一个有符号大整数，再转成十六进制。
///
/// 所以结果可能带负号，也可能短于 40 位（前导零被丢掉了）。Java 那边是
/// `new BigInteger(bytes).toString(16)`，这里只能自己把补码和前导零处理一遍。
pub fn minecraft_hex_digest(parts: &[&[u8]]) -> String {
    let mut digest = Sha1::new();
    for part in parts {
        digest.update(part);
    }
    signed_hex(&digest.finalize())
}

/// 把一串大端补码字节按 Java `BigInteger.toString(16)` 的样子写出来。
fn signed_hex(bytes: &[u8]) -> String {
    let negative = bytes.first().is_some_and(|byte| byte & 0x80 != 0);
    let magnitude = if negative {
        negate(bytes)
    } else {
        bytes.to_vec()
    };

    let mut digits = String::with_capacity(bytes.len() * 2);
    for byte in &magnitude {
        digits.push(hex_digit(byte >> 4));
        digits.push(hex_digit(byte & 0x0f));
    }

    // 十六进制里不写前导零。全是零时 `BigInteger.ZERO.toString(16)` 是 "0"。
    let digits = digits.trim_start_matches('0');
    if digits.is_empty() {
        return "0".to_owned();
    }
    if negative {
        format!("-{digits}")
    } else {
        digits.to_owned()
    }
}

/// 对补码取负：按字节取反再加一。
fn negate(bytes: &[u8]) -> Vec<u8> {
    let mut negated: Vec<u8> = bytes.iter().map(|byte| !byte).collect();
    for byte in negated.iter_mut().rev() {
        *byte = byte.wrapping_add(1);
        if *byte != 0 {
            break;
        }
    }
    negated
}

fn hex_digit(value: u8) -> char {
    char::from(b"0123456789abcdef"[usize::from(value)])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 三个经典样例：Notch 是普通的 40 位，jeb_ 是负数，simon 的首位是零。
    /// 对照值是用 JDK 的 `new BigInteger(MessageDigest.getInstance("SHA-1").digest(...))
    /// .toString(16)` 现跑出来的。
    #[test]
    fn matches_the_classic_samples() {
        assert_eq!(
            minecraft_hex_digest(&[b"Notch"]),
            "4ed1f46bbe04bc756bcb17c0c7ce3e4632f06a48"
        );
        assert_eq!(
            minecraft_hex_digest(&[b"jeb_"]),
            "-7c9d5b0044c130109a5d7b5fb5c317c02b4e28c1"
        );
        assert_eq!(
            minecraft_hex_digest(&[b"simon"]),
            "88e16a1019277b15d58faf0541e11910eb756f6"
        );
    }

    /// 分几段喂进去和一次喂进去必须一样，serverId 哈希就是分三段算的。
    #[test]
    fn concatenates_its_parts() {
        assert_eq!(
            minecraft_hex_digest(&[b"No", b"", b"tch"]),
            minecraft_hex_digest(&[b"Notch"])
        );
    }

    #[test]
    fn hashes_the_server_id_in_the_official_order() {
        let secret = [7; 16];
        let key = [9; 24];
        let mut parts = Vec::new();
        parts.extend_from_slice(&secret);
        parts.extend_from_slice(&key);
        assert_eq!(
            server_id_hash("", &secret, &key),
            minecraft_hex_digest(&[&parts])
        );
        // 顺序错了就得到另一个哈希，所以这个测试不会放过写反的实现。
        assert_ne!(
            server_id_hash("", &secret, &key),
            minecraft_hex_digest(&[&key, &secret])
        );
    }

    #[test]
    fn writes_zero_and_negative_one_like_java() {
        assert_eq!(signed_hex(&[0; 20]), "0");
        assert_eq!(signed_hex(&[0xff; 20]), "-1");
        assert_eq!(signed_hex(&[0x80, 0x00]), "-8000");
        assert_eq!(signed_hex(&[0x00, 0x0f]), "f");
        assert_eq!(signed_hex(&[0xff, 0xf0]), "-10");
    }

    /// 畸形输入：空摘要在 Java 里会抛异常，这里不许崩。
    #[test]
    fn survives_an_empty_digest() {
        assert_eq!(signed_hex(&[]), "0");
    }
}
