//! 够用就行的 HTTP GET。
//!
//! session server 是服务端唯一要访问的外部服务，一个 GET 请求、一小段 JSON 回复，
//! 为它拉一整套 HTTP 客户端不值得。TLS 交给系统（Windows 上是 schannel）。

use std::time::Duration;

use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpStream,
    time::timeout,
};

/// 回复的上限。正常回复是一小段 JSON，超过这个数说明对面不对劲。
const MAX_RESPONSE_LEN: usize = 64 * 1024;

#[derive(Debug, thiserror::Error)]
pub(crate) enum HttpError {
    #[error("{0} is not a usable http url")]
    Url(String),
    #[error("cannot connect to {0}: {1}")]
    Connect(String, std::io::Error),
    #[error("cannot set up tls for {0}: {1}")]
    Tls(String, String),
    #[error("the connection broke: {0}")]
    Io(std::io::Error),
    #[error("the response is not valid http: {0}")]
    Malformed(&'static str),
    #[error("the response is longer than {MAX_RESPONSE_LEN} bytes")]
    TooLong,
    #[error("the request timed out")]
    Timeout,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

/// 发一个 GET 请求，整个来回不许超过 `deadline`。
pub(crate) async fn get(url: &str, deadline: Duration) -> Result<Response, HttpError> {
    timeout(deadline, request(url))
        .await
        .map_err(|_| HttpError::Timeout)?
}

async fn request(url: &str) -> Result<Response, HttpError> {
    let target = Target::parse(url)?;
    let stream = TcpStream::connect((target.host, target.port))
        .await
        .map_err(|error| HttpError::Connect(target.authority.to_owned(), error))?;
    let head = target.request_head();

    if target.tls {
        let connector = native_tls::TlsConnector::new()
            .map_err(|error| HttpError::Tls(target.host.to_owned(), error.to_string()))?;
        let stream = tokio_native_tls::TlsConnector::from(connector)
            .connect(target.host, stream)
            .await
            .map_err(|error| HttpError::Tls(target.host.to_owned(), error.to_string()))?;
        exchange(stream, &head).await
    } else {
        exchange(stream, &head).await
    }
}

/// 把请求写出去，再读回一个完整的回复。
///
/// 回复一读满就返回，不等对面关连接：session server 回的是 `Connection: keep-alive`，
/// 一直读到 EOF 会白等到超时。
async fn exchange<S>(mut stream: S, head: &str) -> Result<Response, HttpError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    stream
        .write_all(head.as_bytes())
        .await
        .map_err(HttpError::Io)?;
    stream.flush().await.map_err(HttpError::Io)?;

    let mut buffer = Vec::new();
    let mut chunk = [0; 4096];
    loop {
        if let Some(response) = parse(&buffer, false)? {
            return Ok(response);
        }
        let read = stream.read(&mut chunk).await.map_err(HttpError::Io)?;
        if read == 0 {
            // 对面关了连接。没有长度头的回复到这里才算完整。
            return parse(&buffer, true)?
                .ok_or(HttpError::Malformed("the response ends too early"));
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.len() > MAX_RESPONSE_LEN {
            return Err(HttpError::TooLong);
        }
    }
}

/// 回复攒够了就解析出来，还差东西就返回 `None` 让调用方继续读。
fn parse(bytes: &[u8], at_end: bool) -> Result<Option<Response>, HttpError> {
    let Some(head_len) = find(bytes, b"\r\n\r\n") else {
        return Ok(None);
    };
    let head = str::from_utf8(&bytes[..head_len])
        .map_err(|_| HttpError::Malformed("the header is not text"))?;
    let mut lines = head.split("\r\n");
    let status = status_code(lines.next().unwrap_or_default())?;
    let body = &bytes[head_len + 4..];

    let body = match framing(status, lines)? {
        Framing::Empty => Some(Vec::new()),
        Framing::Chunked => dechunk(body)?,
        Framing::Length(length) => (body.len() >= length).then(|| body[..length].to_vec()),
        Framing::UntilClose => at_end.then(|| body.to_vec()),
    };
    Ok(body.map(|body| Response { status, body }))
}

/// 回复体的长度是怎么定下来的。
enum Framing {
    /// 这个状态码不许带回复体。
    Empty,
    Chunked,
    Length(usize),
    UntilClose,
}

/// 按 RFC 9112 的顺序判断：状态码说了不许有回复体就没有，然后是分块，然后是长度头，
/// 都没有就只能读到对面关连接为止。
fn framing<'a>(status: u16, headers: impl Iterator<Item = &'a str>) -> Result<Framing, HttpError> {
    if status < 200 || status == 204 || status == 304 {
        return Ok(Framing::Empty);
    }

    let mut length = None;
    let mut chunked = false;
    for line in headers {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        if name.eq_ignore_ascii_case("content-length") {
            length = Some(
                value
                    .parse::<usize>()
                    .map_err(|_| HttpError::Malformed("content-length is not a number"))?,
            );
        } else if name.eq_ignore_ascii_case("transfer-encoding")
            && value.eq_ignore_ascii_case("chunked")
        {
            chunked = true;
        }
    }

    if chunked {
        Ok(Framing::Chunked)
    } else if let Some(length) = length {
        Ok(Framing::Length(length))
    } else {
        Ok(Framing::UntilClose)
    }
}

/// 拼起分块传输的各段。最后那个零长度分块后面还可能有尾部头，我们不关心。
fn dechunk(mut rest: &[u8]) -> Result<Option<Vec<u8>>, HttpError> {
    let mut body = Vec::new();
    loop {
        let Some(line_len) = find(rest, b"\r\n") else {
            return Ok(None);
        };
        let line = str::from_utf8(&rest[..line_len])
            .map_err(|_| HttpError::Malformed("a chunk size is not text"))?;
        // 分号后面是分块扩展。
        let size = line.split(';').next().unwrap_or_default().trim();
        let size = usize::from_str_radix(size, 16)
            .map_err(|_| HttpError::Malformed("a chunk size is not a hex number"))?;
        rest = &rest[line_len + 2..];

        if size == 0 {
            return Ok(Some(body));
        }
        // 每段数据后面还跟着一个 CRLF。
        if rest.len() < size + 2 {
            return Ok(None);
        }
        body.extend_from_slice(&rest[..size]);
        rest = &rest[size + 2..];
    }
}

/// `HTTP/1.1 200 OK` 里的那个数字。
fn status_code(line: &str) -> Result<u16, HttpError> {
    if !line.starts_with("HTTP/") {
        return Err(HttpError::Malformed("the status line is not http"));
    }
    line.split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .ok_or(HttpError::Malformed("the status line has no status code"))
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// 请求要发往哪里。
struct Target<'a> {
    tls: bool,
    host: &'a str,
    port: u16,
    /// URL 里写的 `主机[:端口]`，原样放进 `Host` 头。
    authority: &'a str,
    /// 路径加查询串。
    path: &'a str,
}

impl<'a> Target<'a> {
    fn parse(url: &'a str) -> Result<Self, HttpError> {
        let malformed = || HttpError::Url(url.to_owned());
        let (tls, rest) = if let Some(rest) = url.strip_prefix("https://") {
            (true, rest)
        } else if let Some(rest) = url.strip_prefix("http://") {
            (false, rest)
        } else {
            return Err(malformed());
        };

        let (authority, path) = match rest.find('/') {
            Some(index) => (&rest[..index], &rest[index..]),
            None => (rest, "/"),
        };
        let (host, port) = match authority.rsplit_once(':') {
            Some((host, port)) => (host, port.parse().map_err(|_| malformed())?),
            None => (authority, if tls { 443 } else { 80 }),
        };
        if host.is_empty() || port == 0 {
            return Err(malformed());
        }

        Ok(Self {
            tls,
            host,
            port,
            authority,
            path,
        })
    }

    /// 明确要求对面读完一个回复就关连接，省掉连接复用的那套状态。
    fn request_head(&self) -> String {
        format!(
            "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: Bamboo\r\nAccept: application/json\r\nConnection: close\r\n\r\n",
            self.path, self.authority
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(bytes: &[u8]) -> Option<Response> {
        parse(bytes, false).expect("这段回复应该是合法的")
    }

    #[test]
    fn parses_a_url() {
        let target = Target::parse(
            "https://sessionserver.mojang.com/session/minecraft/hasJoined?username=Notch",
        )
        .expect("url");
        assert!(target.tls);
        assert_eq!(target.host, "sessionserver.mojang.com");
        assert_eq!(target.port, 443);
        assert_eq!(target.path, "/session/minecraft/hasJoined?username=Notch");

        let target = Target::parse("http://127.0.0.1:8080/hasJoined").expect("url");
        assert!(!target.tls);
        assert_eq!((target.host, target.port), ("127.0.0.1", 8080));
        assert_eq!(target.authority, "127.0.0.1:8080");

        let target = Target::parse("http://localhost").expect("url");
        assert_eq!((target.port, target.path), (80, "/"));
    }

    #[test]
    fn rejects_urls_it_cannot_use() {
        for url in [
            "sessionserver.mojang.com/hasJoined",
            "ftp://example.com/",
            "http://",
            "http://example.com:/",
            "http://example.com:0/",
            "http://example.com:70000/",
        ] {
            assert!(
                matches!(Target::parse(url), Err(HttpError::Url(_))),
                "{url}"
            );
        }
    }

    #[test]
    fn waits_for_the_whole_header() {
        assert_eq!(parsed(b""), None);
        assert_eq!(parsed(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n"), None);
    }

    #[test]
    fn reads_a_body_with_a_length() {
        let bytes = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello, and then some".to_vec();
        assert_eq!(
            parsed(&bytes),
            Some(Response {
                status: 200,
                body: b"hello".to_vec()
            })
        );
        assert_eq!(
            parsed(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhell"),
            None
        );
    }

    #[test]
    fn reads_a_chunked_body() {
        let bytes = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n3;x=y\r\n it\r\n0\r\n\r\n";
        assert_eq!(
            parsed(bytes),
            Some(Response {
                status: 200,
                body: b"hello it".to_vec()
            })
        );
        // 最后那个零长度分块还没到，就还不算读完。
        assert_eq!(
            parsed(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n"),
            None
        );
    }

    /// 204 不许带回复体，对面不关连接也不能等。session server 拒绝登录时回的就是 204。
    #[test]
    fn a_204_has_no_body() {
        let bytes = b"HTTP/1.1 204 No Content\r\nConnection: keep-alive\r\n\r\n";
        assert_eq!(
            parsed(bytes),
            Some(Response {
                status: 204,
                body: Vec::new()
            })
        );
    }

    #[test]
    fn a_body_without_a_length_ends_at_the_close() {
        let bytes = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{}";
        assert_eq!(parsed(bytes), None);
        assert_eq!(
            parse(bytes, true).expect("回复应该是合法的"),
            Some(Response {
                status: 200,
                body: b"{}".to_vec()
            })
        );
    }

    #[test]
    fn header_names_are_case_insensitive() {
        let bytes = b"HTTP/1.1 500 Internal Server Error\r\ncontent-length: 3\r\n\r\noof";
        assert_eq!(
            parsed(bytes),
            Some(Response {
                status: 500,
                body: b"oof".to_vec()
            })
        );
    }

    #[test]
    fn rejects_malformed_responses() {
        for bytes in [
            &b"not http at all\r\n\r\n"[..],
            &b"HTTP/1.1\r\n\r\n"[..],
            &b"HTTP/1.1 nope OK\r\n\r\n"[..],
            &b"HTTP/1.1 200 OK\r\nContent-Length: later\r\n\r\n"[..],
            &b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n"[..],
        ] {
            assert!(
                matches!(parse(bytes, true), Err(HttpError::Malformed(_))),
                "{:?}",
                String::from_utf8_lossy(bytes)
            );
        }
    }
}
