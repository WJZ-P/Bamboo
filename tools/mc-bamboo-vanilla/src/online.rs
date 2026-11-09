//! 探针检查 `online_mode` 用的两个本地服务。
//!
//! 探针不能启动官方服务端，也不能改 `bamboo` 的 `main`，所以开了正版验证的那一项检查
//! 在这个工具的进程里再起一个 Bamboo，并且用一个假的 session server 顶替 Mojang：
//!
//! * `GET /probe/expect?username=&hash=`：官方客户端代码把自己算出来的 serverId 哈希
//!   报过来，回复里是这次要发的档案，客户端拿它核对服务端最后发下来的 UUID 和皮肤。
//! * `GET /session/minecraft/hasJoined?username=&serverId=`：服务端来问这次登录，
//!   哈希和客户端报的那个一致才给档案，对不上就回 204。
//! * `GET /probe/asked`：服务端问过的哈希，客户端拿它和自己算的那个比较。
//! * `GET /publickeys`：给 Bamboo 的 `playerCertificateKeys`，算法和官方一样是 SHA1withRSA。
//! * `GET /probe/service-private`：上面那把钥匙的 PKCS#8，探针用它签玩家公钥。
//!
//! 这两个服务都只监听 127.0.0.1，探针不会访问外网。

use std::{
    collections::HashMap,
    io,
    net::SocketAddr,
    sync::mpsc,
    thread::{self, JoinHandle},
};

use mc_bamboo_server::{Server, ServerConfig};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    runtime,
    sync::oneshot,
};

/// 假 session server 发的 UUID。它不是任何名字的离线 UUID，所以客户端一看就知道
/// 服务端用的是 session server 给的身份，不是自己算出来的。
const PROFILE_UUID: &str = "2f3d06f5-7b4a-4c9e-9a1b-8d0e5f6a7b8c";
/// 假 session server 发的皮肤。内容是什么不重要，要的是它一字不改地到客户端手里。
const SKIN_VALUE: &str = "ewogICJ0aW1lc3RhbXAiIDogMQp9";
const SKIN_SIGNATURE: &str = "c2lnbmF0dXJl";
/// 请求头的长度上限，超过就不回答。
const MAX_HEAD_LEN: usize = 8 * 1024;
/// 两个服务都只对本机开放，端口由系统分配。
const LOCAL_ADDRESS: &str = "127.0.0.1:0";
/// 工作线程数。一个跑连接，一个跑假 session server，世界在自己的线程上。
const WORKER_THREADS: usize = 2;

/// 启动好的两个服务。地址交给探针，析构时服务跟着停下。
pub struct OnlineProbe {
    server: SocketAddr,
    session_url: String,
    stop: Option<oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl OnlineProbe {
    /// 开了正版验证的服务端地址。
    pub fn server_address(&self) -> String {
        self.server.to_string()
    }

    /// 假 session server 的根地址，探针拿它拼上面那几个路径。
    pub fn session_url(&self) -> &str {
        &self.session_url
    }
}

impl Drop for OnlineProbe {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// 两个服务绑好端口的结果：服务端地址和假 session server 的根地址。
type Addresses = Result<(SocketAddr, String), String>;

/// 启动这两个服务，端口定下来之后再返回。
pub fn start() -> Result<OnlineProbe, String> {
    let (ready, bound) = mpsc::channel();
    let (stop, stopped) = oneshot::channel();
    let thread = thread::Builder::new()
        .name("probe-online".to_owned())
        .spawn(move || run(&ready, stopped))
        .map_err(|error| format!("无法启动 online 探针的线程：{error}"))?;

    let (server, session_url) = bound
        .recv()
        .map_err(|_| "online 探针的服务没有启动".to_owned())??;
    Ok(OnlineProbe {
        server,
        session_url,
        stop: Some(stop),
        thread: Some(thread),
    })
}

fn run(ready: &mpsc::Sender<Addresses>, stopped: oneshot::Receiver<()>) {
    let runtime = runtime::Builder::new_multi_thread()
        .worker_threads(WORKER_THREADS)
        .enable_all()
        .build();
    match runtime {
        Ok(runtime) => runtime.block_on(serve(ready, stopped)),
        Err(error) => {
            let _ = ready.send(Err(format!("无法创建 online 探针的运行时：{error}")));
        }
    }
}

async fn serve(ready: &mpsc::Sender<Addresses>, stopped: oneshot::Receiver<()>) {
    match prepare().await {
        Ok((server, address, session_url)) => {
            let _ = ready.send(Ok((address, session_url)));
            server
                .run(async move {
                    let _ = stopped.await;
                })
                .await;
        }
        Err(error) => {
            let _ = ready.send(Err(error));
        }
    }
}

/// 绑好两个端口，起好假 session server，返回等着跑的服务端。
async fn prepare() -> Result<(Server, SocketAddr, String), String> {
    let session = TcpListener::bind(LOCAL_ADDRESS)
        .await
        .map_err(|error| format!("假 session server 无法监听：{error}"))?;
    let session_address = session
        .local_addr()
        .map_err(|error| format!("拿不到假 session server 的端口：{error}"))?;

    let listener = TcpListener::bind(LOCAL_ADDRESS)
        .await
        .map_err(|error| format!("开了正版验证的服务端无法监听：{error}"))?;
    let address = listener
        .local_addr()
        .map_err(|error| format!("拿不到服务端的端口：{error}"))?;

    // 公钥在进入 accept 循环之前生成。调试构建里的 RSA 很慢，占着唯一的连接时 hasJoined 进不来。
    // Generate the key before accept. Debug RSA is slow, and hasJoined cannot get in while it holds the only connection.
    let service = tokio::task::spawn_blocking(ServiceMaterial::generate)
        .await
        .map_err(|error| format!("生成探针公钥的任务失败：{error}"))?;

    let config = ServerConfig {
        online_mode: true,
        session_server_url: format!("http://{session_address}/session/minecraft/hasJoined"),
        ..ServerConfig::default()
    };
    let server = Server::new(listener, config)
        .map_err(|error| format!("无法创建开了正版验证的服务端：{error}"))?;

    tokio::spawn(session_service(session, service));
    Ok((server, address, format!("http://{session_address}")))
}

/// 客户端报过来的哈希和服务端问过的哈希。每个名字一把哈希，Sakura 以外的名字有自己的 UUID。
///
/// Digests the clients reported, and the digest the server asked about. Each name has its own
/// digest. Names other than Sakura get their own UUID.
#[derive(Default)]
struct Exchange {
    expected: HashMap<String, String>,
    asked: String,
    service: Option<ServiceMaterial>,
}

struct ServiceMaterial {
    public_b64: String,
    private_b64: String,
}

/// 顶替 Mojang 的假 session server。
///
/// 请求一个一个处理：客户端先报哈希，服务端再来问，客户端最后取服务端问过的那个，
/// 三步本来就是前后发生的，不需要并发。
async fn session_service(listener: TcpListener, service: Option<ServiceMaterial>) {
    let mut exchange = Exchange {
        service,
        ..Exchange::default()
    };
    while let Ok((mut stream, _)) = listener.accept().await {
        let Ok(head) = read_head(&mut stream).await else {
            continue;
        };
        let response = respond(request_target(&head), &mut exchange);
        let _ = stream.write_all(response.as_bytes()).await;
        let _ = stream.shutdown().await;
    }
}

/// 读到请求头结束为止。GET 没有请求体，读完头这个请求就完整了。
async fn read_head(stream: &mut TcpStream) -> io::Result<String> {
    let mut head = Vec::new();
    let mut byte = [0];
    while !head.ends_with(b"\r\n\r\n") {
        if head.len() >= MAX_HEAD_LEN || stream.read(&mut byte).await? == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        head.push(byte[0]);
    }
    Ok(String::from_utf8_lossy(&head).into_owned())
}

/// 请求行里的目标，比如 `/session/minecraft/hasJoined?username=Sakura&serverId=-7c9d`。
fn request_target(head: &str) -> &str {
    head.lines()
        .next()
        .unwrap_or_default()
        .split(' ')
        .nth(1)
        .unwrap_or_default()
}

fn respond(target: &str, exchange: &mut Exchange) -> String {
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    match path {
        "/probe/expect" => {
            let name = param(query, "username");
            exchange.expected.insert(name.clone(), param(query, "hash"));
            text(&format!(
                "{} {SKIN_VALUE} {SKIN_SIGNATURE}",
                profile_uuid(&name)
            ))
        }
        "/session/minecraft/hasJoined" => has_joined(query, exchange),
        "/probe/asked" => text(&exchange.asked),
        "/publickeys" => match service_material(exchange) {
            Some(material) => json(&format!(
                r#"{{"playerCertificateKeys":[{{"publicKey":"{}"}}],"profilePropertyKeys":[]}}"#,
                material.public_b64
            )),
            None => status("500 Internal Server Error"),
        },
        "/probe/service-private" => match service_material(exchange) {
            Some(material) => text(&material.private_b64),
            None => status("500 Internal Server Error"),
        },
        _ => status("404 Not Found"),
    }
}

fn service_material(exchange: &Exchange) -> Option<&ServiceMaterial> {
    exchange.service.as_ref()
}

/// Sakura 继续用原来的 UUID。别的名字各自一个，避免第二个玩家被当成重复登录。
///
/// Sakura keeps the original UUID. Every other name gets its own, so a second player is not a duplicate login.
fn profile_uuid(name: &str) -> String {
    if name == "Sakura" {
        return PROFILE_UUID.to_owned();
    }
    let mut bytes = [0u8; 16];
    for (index, byte) in name.bytes().enumerate() {
        bytes[index % 16] ^= byte.wrapping_add(index as u8);
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15]
    )
}

impl ServiceMaterial {
    fn generate() -> Option<Self> {
        use rsa::pkcs8::{EncodePrivateKey, EncodePublicKey};
        use rsa::{RsaPrivateKey, RsaPublicKey};

        let mut rng = rand_core::OsRng;
        let private = RsaPrivateKey::new(&mut rng, 2048).ok()?;
        let public = RsaPublicKey::from(&private);
        let public_der = public.to_public_key_der().ok()?;
        let private_der = private.to_pkcs8_der().ok()?;
        Some(Self {
            public_b64: base64(public_der.as_bytes()),
            private_b64: base64(private_der.as_bytes()),
        })
    }
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let a = chunk[0];
        let b = chunk.get(1).copied().unwrap_or(0);
        let c = chunk.get(2).copied().unwrap_or(0);
        let triple = (u32::from(a) << 16) | (u32::from(b) << 8) | u32::from(c);
        out.push(ALPHABET[((triple >> 18) & 0x3f) as usize] as char);
        out.push(ALPHABET[((triple >> 12) & 0x3f) as usize] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[((triple >> 6) & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[(triple & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// 服务端问这次登录。哈希和这个名字报过的那个一致才给档案。
///
/// The server asks about this login. The profile is returned only when the digest matches the one this name reported.
fn has_joined(query: &str, exchange: &mut Exchange) -> String {
    exchange.asked = param(query, "serverId");
    let name = param(query, "username");
    let expected = exchange.expected.get(&name).cloned().unwrap_or_default();
    if exchange.asked.is_empty() || exchange.asked != expected {
        // 官方的 session server 不认这次登录时回的就是 204。
        return status("204 No Content");
    }

    let uuid = profile_uuid(&name).replace('-', "");
    json(&format!(
        r#"{{"id":"{uuid}","name":"{name}","properties":[{{"name":"textures","value":"{SKIN_VALUE}","signature":"{SKIN_SIGNATURE}"}}]}}"#
    ))
}

/// 查询串里某个参数的值。探针只用 ASCII 的名字和哈希，所以不做转义还原。
fn param(query: &str, name: &str) -> String {
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value.to_owned())
        .unwrap_or_default()
}

fn text(body: &str) -> String {
    response("text/plain", body)
}

fn json(body: &str) -> String {
    response("application/json", body)
}

fn response(content_type: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

/// 没有回复体的那几种回答。
fn status(line: &str) -> String {
    format!("HTTP/1.1 {line}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIGEST: &str = "-7c9d5b00";

    #[test]
    fn reads_the_request_target() {
        assert_eq!(
            request_target("GET /probe/asked HTTP/1.1\r\nHost: x\r\n\r\n"),
            "/probe/asked"
        );
        assert_eq!(request_target("GET /a?b=c HTTP/1.1\r\n\r\n"), "/a?b=c");
        assert_eq!(request_target(""), "");
    }

    #[test]
    fn reads_query_parameters() {
        let query = "username=Sakura&serverId=-7c9d5b00";
        assert_eq!(param(query, "username"), "Sakura");
        assert_eq!(param(query, "serverId"), DIGEST);
        assert_eq!(param(query, "ip"), "");
        assert_eq!(param("", "username"), "");
    }

    /// 客户端先报哈希，服务端问的是同一个哈希才能拿到档案。
    #[test]
    fn the_profile_needs_the_digest_the_client_reported() {
        let mut exchange = Exchange::default();
        let denied = respond(
            "/session/minecraft/hasJoined?serverId=whatever",
            &mut exchange,
        );
        assert!(denied.starts_with("HTTP/1.1 204 No Content"), "{denied}");

        let reported = respond(
            "/probe/expect?username=Sakura&hash=-7c9d5b00",
            &mut exchange,
        );
        assert!(reported.contains(PROFILE_UUID), "{reported}");

        let target = format!("/session/minecraft/hasJoined?username=Sakura&serverId={DIGEST}");
        let profile = respond(&target, &mut exchange);
        assert!(
            profile.contains(&PROFILE_UUID.replace('-', "")),
            "{profile}"
        );
        assert!(profile.contains(SKIN_SIGNATURE), "{profile}");
        assert!(profile.contains("\"name\":\"Sakura\""), "{profile}");

        // 服务端问过的哈希要能取回来，客户端拿它和自己算的比较。
        assert!(respond("/probe/asked", &mut exchange).ends_with(DIGEST));
    }

    #[test]
    fn unknown_paths_are_not_found() {
        let mut exchange = Exchange::default();
        assert!(
            respond("/nope", &mut exchange).starts_with("HTTP/1.1 404 Not Found"),
            "unknown paths should not be answered"
        );
    }
}
