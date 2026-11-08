//! 点击事件和对话框里的 URI。
//!
//! 官方是 `new java.net.URI` 再要求协议是 `http` 或 `https`
//! （`Util.parseAndValidateUntrustedUri`）。这里只收下一小类写法：协议、`//`、
//! 主机、可选端口、路径、查询和片段，字符都在官方一定会收下的集合里。`%` 后面必须是
//! 两位十六进制。方括号只允许包住 IPv6 主机。拿不准的，包括不透明的 `http:foo`、
//! 用户信息、空主机、空端口、区域标识和非 ASCII，一律拒绝。拒绝只断开发来这件物品的
//! 那名玩家。
//!
//! URIs in click events and dialogs.
//!
//! Official code does `new java.net.URI` and then requires the scheme to be `http`
//! or `https` (`Util.parseAndValidateUntrustedUri`). This accepts only a small
//! shape: scheme, `//`, host, optional port, path, query and fragment, using
//! characters official code is sure to accept. `%` must be followed by two hex
//! digits. Brackets may only wrap an IPv6 host. Anything uncertain is rejected,
//! including an opaque `http:foo`, user info, an empty host, an empty port, a zone
//! id and non-ASCII. A rejection disconnects only the player who sent the item.

/// 这段 URI 官方的 `parseAndValidateUntrustedUri` 一定会收下。
///
/// Official `parseAndValidateUntrustedUri` is sure to accept this URI.
pub fn accepts(url: &str) -> bool {
    check(url).is_ok()
}

pub(crate) fn check(url: &str) -> Result<(), &'static str> {
    let bytes = url.as_bytes();
    if bytes
        .iter()
        .any(|byte| *byte < 0x20 || *byte == 0x7f || *byte >= 0x80)
    {
        return Err("url is not valid");
    }
    let Some(scheme_end) = bytes.iter().position(|byte| *byte == b':') else {
        return Err("url needs a protocol");
    };
    let scheme = &bytes[..scheme_end];
    if !scheme.eq_ignore_ascii_case(b"http") && !scheme.eq_ignore_ascii_case(b"https") {
        return Err("url protocol is not allowed");
    }
    let rest = &bytes[scheme_end + 1..];
    if !rest.starts_with(b"//") {
        return Err("url is not valid");
    }
    let rest = &rest[2..];
    let authority_end = rest
        .iter()
        .position(|byte| matches!(byte, b'/' | b'?' | b'#'))
        .unwrap_or(rest.len());
    authority(&rest[..authority_end])?;
    tail(&rest[authority_end..])?;
    Ok(())
}

fn authority(auth: &[u8]) -> Result<(), &'static str> {
    if auth.is_empty() || auth.contains(&b'@') {
        return Err("url is not valid");
    }
    if auth[0] == b'[' {
        let Some(end) = auth.iter().position(|byte| *byte == b']') else {
            return Err("url is not valid");
        };
        if auth[1..end].contains(&b'[') || auth[1..end].contains(&b']') {
            return Err("url is not valid");
        }
        ipv6(&auth[1..end])?;
        let after = &auth[end + 1..];
        if after.is_empty() {
            return Ok(());
        }
        if !after.starts_with(b":") {
            return Err("url is not valid");
        }
        return port(&after[1..]);
    }
    if auth.contains(&b'[') || auth.contains(&b']') {
        return Err("url is not valid");
    }
    match auth.iter().position(|byte| *byte == b':') {
        Some(index) => {
            host(&auth[..index])?;
            port(&auth[index + 1..])?;
        }
        None => host(auth)?,
    }
    Ok(())
}

fn host(host: &[u8]) -> Result<(), &'static str> {
    if host.is_empty() {
        return Err("url is not valid");
    }
    if ipv4_shape(host) {
        ipv4(host)
    } else {
        hostname(host)
    }
}

fn ipv4_shape(host: &[u8]) -> bool {
    host.iter().filter(|byte| **byte == b'.').count() == 3
        && host
            .iter()
            .all(|byte| byte.is_ascii_digit() || *byte == b'.')
}

fn ipv4(host: &[u8]) -> Result<(), &'static str> {
    let mut parts = host.split(|byte| *byte == b'.');
    for _ in 0..4 {
        let Some(part) = parts.next() else {
            return Err("url is not valid");
        };
        if part.is_empty() || part.len() > 3 || (part.len() > 1 && part[0] == b'0') {
            return Err("url is not valid");
        }
        let value = std::str::from_utf8(part)
            .ok()
            .and_then(|text| text.parse::<u32>().ok())
            .ok_or("url is not valid")?;
        if value > 255 {
            return Err("url is not valid");
        }
    }
    if parts.next().is_some() {
        return Err("url is not valid");
    }
    Ok(())
}

fn hostname(host: &[u8]) -> Result<(), &'static str> {
    if host.is_empty() || host.starts_with(b".") || host.ends_with(b".") {
        return Err("url is not valid");
    }
    for label in host.split(|byte| *byte == b'.') {
        if label.is_empty()
            || !label[0].is_ascii_alphanumeric()
            || !label[label.len() - 1].is_ascii_alphanumeric()
            || !label
                .iter()
                .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
        {
            return Err("url is not valid");
        }
    }
    Ok(())
}

fn port(text: &[u8]) -> Result<(), &'static str> {
    if text.is_empty() || text.len() > 5 || !text.iter().all(|byte| byte.is_ascii_digit()) {
        return Err("url is not valid");
    }
    let value = std::str::from_utf8(text)
        .ok()
        .and_then(|text| text.parse::<u32>().ok())
        .ok_or("url is not valid")?;
    if value > 65535 {
        return Err("url is not valid");
    }
    Ok(())
}

/// 不含 IPv4 尾段、不含区域标识。没有 `::` 时必须正好 8 组；有一次 `::` 时两组加起来少于 8。
fn ipv6(body: &[u8]) -> Result<(), &'static str> {
    if body.is_empty() || body.contains(&b'%') || body.contains(&b'.') {
        return Err("url is not valid");
    }
    let text = std::str::from_utf8(body).map_err(|_| "url is not valid")?;
    if text.matches("::").count() > 1 || text.contains(":::") {
        return Err("url is not valid");
    }
    let groups = |side: &str| -> Result<usize, ()> {
        if side.is_empty() {
            return Ok(0);
        }
        let mut count = 0;
        for part in side.split(':') {
            if part.is_empty()
                || part.len() > 4
                || !part.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return Err(());
            }
            count += 1;
        }
        Ok(count)
    };
    match text.split_once("::") {
        Some((left, right)) => {
            let total = groups(left).map_err(|_| "url is not valid")?
                + groups(right).map_err(|_| "url is not valid")?;
            if total >= 8 {
                return Err("url is not valid");
            }
        }
        None => {
            if groups(text).map_err(|_| "url is not valid")? != 8 {
                return Err("url is not valid");
            }
        }
    }
    Ok(())
}

fn tail(tail: &[u8]) -> Result<(), &'static str> {
    if tail.is_empty() {
        return Ok(());
    }
    if !matches!(tail[0], b'/' | b'?' | b'#') {
        return Err("url is not valid");
    }
    let mut index = 0;
    let mut in_query = false;
    let mut in_fragment = false;
    while index < tail.len() {
        let byte = tail[index];
        if byte == b'?' && !in_query && !in_fragment {
            in_query = true;
            index += 1;
            continue;
        }
        if byte == b'#' && !in_fragment {
            in_fragment = true;
            in_query = false;
            index += 1;
            continue;
        }
        if byte == b'%' {
            if index + 2 >= tail.len()
                || !tail[index + 1].is_ascii_hexdigit()
                || !tail[index + 2].is_ascii_hexdigit()
            {
                return Err("url is not valid");
            }
            index += 3;
            continue;
        }
        if tail_byte(byte, in_query || in_fragment) {
            index += 1;
            continue;
        }
        return Err("url is not valid");
    }
    Ok(())
}

fn tail_byte(byte: u8, query_or_fragment: bool) -> bool {
    if byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'-' | b'.'
                | b'_'
                | b'~'
                | b'!'
                | b'$'
                | b'&'
                | b'\''
                | b'('
                | b')'
                | b'*'
                | b'+'
                | b','
                | b';'
                | b'='
                | b':'
                | b'@'
                | b'/'
        )
    {
        return true;
    }
    query_or_fragment && byte == b'?'
}

#[cfg(test)]
mod tests {
    use super::accepts;

    #[test]
    fn accepts_only_a_safe_http_shape() {
        assert!(accepts("http://example.com"));
        assert!(accepts("HTTPS://Example.COM:443/a/b?x=1&y=2#frag"));
        assert!(accepts("http://127.0.0.1/%2e%2e/"));
        assert!(accepts("http://[2001:db8::1]:8080/"));
        assert!(accepts("http://[::1]"));
        assert!(accepts("http://localhost"));

        assert!(!accepts("http:foo"));
        assert!(!accepts("ftp://example.com"));
        assert!(!accepts("http://"));
        assert!(!accepts("http://example.com:"));
        assert!(!accepts("http://example.com:65536"));
        assert!(!accepts("http://user@example.com"));
        assert!(!accepts("http://example.com/%"));
        assert!(!accepts("http://example.com/%2"));
        assert!(!accepts("http://example.com/%2G"));
        assert!(!accepts("http://example.com/a b"));
        assert!(!accepts("http://[::1"));
        assert!(!accepts("http://example.com/a[b]"));
        assert!(!accepts("http://[fe80::1%eth0]/"));
        assert!(!accepts("http://例子.com"));
        assert!(!accepts("http://ex_ample.com"));
    }
}
