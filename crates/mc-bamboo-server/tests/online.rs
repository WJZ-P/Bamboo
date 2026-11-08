//! 正版验证：加密握手、问 session server，以及各种失败分支。
//!
//! session server 是一个跑在本地的假服务，测试不会访问外网。

mod common;

use std::{
    io,
    sync::{Arc, Mutex},
    time::Duration,
};

use common::{
    LOGIN_INTENT, TEST_DEADLINE, TestServer, client_information, frame, handshake, hello,
    json_packet, packet_id, wire,
};
use mc_bamboo_auth::{SHARED_SECRET_LEN, SharedSecret, encrypt_to_server, server_id_hash};
use mc_bamboo_data::packets::{login, play};
use mc_bamboo_protocol::codec::{Reader, write_byte_array};
use mc_bamboo_server::ServerConfig;
use serde_json::json;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
    time::sleep,
};
use uuid::Uuid;

/// 假 session server 给出的皮肤。内容是什么不重要，要的是它一字不改地到客户端手里。
const SKIN_VALUE: &str = "ewogICJ0aW1lc3RhbXAiIDogMQp9";
const SKIN_SIGNATURE: &str = "c2lnbmF0dXJl";
/// 客户端自己选的 AES 密钥，官方是 `Crypt.generateSecretKey` 随机生成的。
const SECRET: SharedSecret = [0x5a; SHARED_SECRET_LEN];
/// 官方 `ServerLoginPacketListenerImpl.serverId` 是空串。
const SERVER_ID: &str = "";
/// 服务端等不到回答时用的时限，要短到测试等得起。
const SHORT_AUTH_TIMEOUT: Duration = Duration::from_millis(200);

/// 假 session server 对这次登录的回答。
#[derive(Debug, Clone, Copy)]
enum Answer {
    /// 认下这次登录，给出 UUID 和皮肤。
    Verified(Uuid),
    /// 没认下来。官方的 session server 这时回 204。
    Denied,
    /// 自己出错了。
    Down,
    /// 连得上，但是一个字节也不回。
    Silent,
}

impl Answer {
    /// 要写回去的整个 HTTP 回复。`Silent` 不回答。
    fn response(self) -> Option<String> {
        let (status, body) = match self {
            Self::Verified(uuid) => (
                "200 OK",
                json!({
                    "id": uuid.simple().to_string(),
                    "name": "Kitten",
                    "properties": [{
                        "name": "textures",
                        "value": SKIN_VALUE,
                        "signature": SKIN_SIGNATURE,
                    }],
                })
                .to_string(),
            ),
            Self::Denied => return Some("HTTP/1.1 204 No Content\r\n\r\n".to_owned()),
            Self::Down => ("500 Internal Server Error", String::new()),
            Self::Silent => return None,
        };
        Some(format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        ))
    }
}

/// 本地的假 session server，只认一个 `hasJoined` 请求。
struct FakeSession {
    url: String,
    /// 收到的请求行，测试用它检查我们问了什么。
    requests: Arc<Mutex<Vec<String>>>,
    task: JoinHandle<()>,
}

impl FakeSession {
    async fn start(answer: Answer) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&requests);

        let task = tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let Ok(line) = request_line(&mut stream).await else {
                    continue;
                };
                recorded.lock().unwrap().push(line);
                match answer.response() {
                    Some(response) => {
                        let _ = stream.write_all(response.as_bytes()).await;
                        let _ = stream.shutdown().await;
                    }
                    // 连接留着不动，服务端只能等到超时。
                    None => sleep(TEST_DEADLINE * 2).await,
                }
            }
        });

        Self {
            url: format!("http://{address}/session/minecraft/hasJoined"),
            requests,
            task,
        }
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }

    /// `hasJoined` 那一行。公钥接口是另一次请求，不在这里。
    ///
    /// The `hasJoined` line. The profile-key request is separate and is not returned here.
    fn has_joined_request(&self) -> String {
        let requests: Vec<String> = self
            .requests()
            .into_iter()
            .filter(|line| line.contains("hasJoined"))
            .collect();
        assert_eq!(requests.len(), 1, "{requests:?}");
        requests.into_iter().next().unwrap()
    }
}

impl Drop for FakeSession {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// 读到请求头的第一行为止。
async fn request_line(stream: &mut TcpStream) -> io::Result<String> {
    let mut head = Vec::new();
    let mut byte = [0];
    while !head.ends_with(b"\r\n") {
        if stream.read(&mut byte).await? == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        head.push(byte[0]);
    }
    Ok(String::from_utf8_lossy(&head).trim_end().to_owned())
}

fn online_server(session: &FakeSession) -> ServerConfig {
    ServerConfig {
        online_mode: true,
        session_server_url: session.url.clone(),
        auth_connect_timeout: SHORT_AUTH_TIMEOUT,
        auth_read_timeout: SHORT_AUTH_TIMEOUT,
        // 配置写 1 会被修正成官方的下限 2，区块少，测试跑得快。
        view_distance: 1,
        ..ServerConfig::default()
    }
}

/// 服务端公布的公钥和 challenge。
fn read_login_hello(payload: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let mut reader = Reader::new(payload);
    assert_eq!(reader.varint(), Ok(login::clientbound::HELLO));
    assert_eq!(reader.string(20).unwrap(), SERVER_ID);
    let public_key = reader.byte_array(512).unwrap().to_vec();
    let challenge = reader.byte_array(512).unwrap().to_vec();
    assert!(
        reader.bool().unwrap(),
        "the server is going to ask Mojang, so this flag is set"
    );
    reader.finish().unwrap();
    (public_key, challenge)
}

/// 客户端的 key 包：AES 密钥和 challenge 都用服务端公钥加密。
fn key_packet(public_key: &[u8], secret: &SharedSecret, challenge: &[u8]) -> Vec<u8> {
    let mut payload = vec![login::serverbound::KEY as u8];
    write_byte_array(
        &mut payload,
        &encrypt_to_server(public_key, secret).expect("encrypt the secret"),
    );
    write_byte_array(
        &mut payload,
        &encrypt_to_server(public_key, challenge).expect("encrypt the challenge"),
    );
    frame(&payload)
}

/// 扮演正版客户端走完密钥交换，返回客户端自己算出来的 serverId 哈希。
///
/// `challenge` 决定送回去的是哪几个字节，`None` 表示照规矩原样送回。
async fn exchange_keys(client: &mut common::Client, name: &str, tampered: Option<&[u8]>) -> String {
    client.send(&handshake(LOGIN_INTENT)).await;
    client.send(&hello(name)).await;

    let (public_key, challenge) = read_login_hello(&client.read_frame().await);
    let digest = server_id_hash(SERVER_ID, &SECRET, &public_key);
    let answer = tampered.unwrap_or(&challenge);
    client.send(&key_packet(&public_key, &SECRET, answer)).await;
    // 官方客户端发完 key 包就切换加密，之后收发的每个字节都是加密的。
    client.enable_encryption(&SECRET);
    digest
}

/// Login 阶段的断开原因是 JSON，不是 NBT。
fn login_disconnect_key(payload: &[u8]) -> String {
    let (id, reason) = json_packet(payload);
    assert_eq!(id, login::clientbound::LOGIN_DISCONNECT);
    reason["translate"]
        .as_str()
        .expect("the reason should be a translation key")
        .to_owned()
}

/// 正版进服的全程：加密、问 session server、拿着 Mojang 的 UUID 和皮肤进入游戏。
#[tokio::test]
async fn an_online_player_joins_with_the_profile_from_the_session_server() {
    let uuid = Uuid::parse_str("069a79f4-44e9-4726-a5be-fca90e38aaf5").unwrap();
    let session = FakeSession::start(Answer::Verified(uuid)).await;
    let server = TestServer::start(online_server(&session)).await;

    let mut client = server.connect().await;
    let digest = exchange_keys(&mut client, "Kitten", None).await;
    let finished = client.configure_after_hello(&client_information(1)).await;
    client.finish_configuration().await;

    // 服务端问的是我们这边算出来的同一个哈希。
    let request = session.has_joined_request();
    assert!(
        request.contains(&format!("username=Kitten&serverId={digest}")),
        "{request}"
    );
    // 没开 prevent-proxy-connections 就不带 IP。
    assert!(!request.contains("&ip="), "{request}");

    // 登录成功包里是 Mojang 给的 UUID 和皮肤，不是离线模式算出来的那个。
    let mut reader = Reader::new(&finished);
    assert_eq!(reader.varint(), Ok(login::clientbound::LOGIN_FINISHED));
    assert_eq!(reader.uuid(), Ok(uuid));
    assert_eq!(reader.string(16).unwrap(), "Kitten");

    assert_eq!(
        packet_id(&client.read_frame().await),
        play::clientbound::LOGIN
    );
    server.wait_for_players(&["Kitten"]).await;
}

/// 皮肤要跟着 Tab 列表发出去，别的玩家才看得到。
#[tokio::test]
async fn the_skin_reaches_the_tab_list() {
    let uuid = Uuid::from_u128(0x1234);
    let session = FakeSession::start(Answer::Verified(uuid)).await;
    let server = TestServer::start(online_server(&session)).await;

    let mut client = server.connect().await;
    exchange_keys(&mut client, "Kitten", None).await;
    client.configure_after_hello(&client_information(1)).await;
    client.finish_configuration().await;

    let payload = client
        .read_until(play::clientbound::PLAYER_INFO_UPDATE)
        .await;
    let update = wire::Frame::parse(&payload).decode::<wire::PlayerInfoUpdate>();
    let entry = update.entry(uuid).expect("the player is on the tab list");
    assert_eq!(
        entry.properties,
        vec![wire::TabProperty {
            name: "textures".to_owned(),
            value: SKIN_VALUE.to_owned(),
            signature: Some(SKIN_SIGNATURE.to_owned()),
        }]
    );
}

/// session server 不认这次登录，官方说的是 `unverified_username`。
#[tokio::test]
async fn an_unverified_player_is_refused() {
    let session = FakeSession::start(Answer::Denied).await;
    let server = TestServer::start(online_server(&session)).await;

    let mut client = server.connect().await;
    exchange_keys(&mut client, "Kitten", None).await;

    let disconnect = client.read_frame().await;
    assert_eq!(
        login_disconnect_key(&disconnect),
        "multiplayer.disconnect.unverified_username"
    );
    client.assert_closed().await;
    assert!(server.handle.online_players().is_empty());
}

/// session server 自己出错，官方说的是 `authservers_down`。
#[tokio::test]
async fn a_broken_session_server_means_the_auth_servers_are_down() {
    let session = FakeSession::start(Answer::Down).await;
    let server = TestServer::start(online_server(&session)).await;

    let mut client = server.connect().await;
    exchange_keys(&mut client, "Kitten", None).await;

    assert_eq!(
        login_disconnect_key(&client.read_frame().await),
        "multiplayer.disconnect.authservers_down"
    );
    client.assert_closed().await;
}

/// 问不出结果的 session server 也算不可用，而且不能把连接永远挂住。
#[tokio::test]
async fn a_session_server_that_never_answers_times_out() {
    let session = FakeSession::start(Answer::Silent).await;
    let server = TestServer::start(online_server(&session)).await;

    let mut client = server.connect().await;
    exchange_keys(&mut client, "Kitten", None).await;

    assert_eq!(
        login_disconnect_key(&client.read_frame().await),
        "multiplayer.disconnect.authservers_down"
    );
    client.assert_closed().await;
}

/// challenge 对不上就直接断开，连 session server 都不问。
#[tokio::test]
async fn a_wrong_challenge_ends_the_connection() {
    let session = FakeSession::start(Answer::Verified(Uuid::from_u128(1))).await;
    let server = TestServer::start(online_server(&session)).await;

    let mut client = server.connect().await;
    exchange_keys(&mut client, "Kitten", Some(&[0, 0, 0, 0])).await;

    client.assert_closed().await;
    // challenge 对不上时不去问 `hasJoined`。公钥接口和这次登录无关，服务端启动时就会取。
    //
    // A mismatched challenge does not ask `hasJoined`. The profile-key fetch is unrelated to this
    // login and starts when the server does.
    assert!(
        session
            .requests()
            .iter()
            .all(|line| !line.contains("hasJoined"))
    );
    assert!(server.handle.online_players().is_empty());
}

/// 关掉正版验证时服务端不发 hello，客户端照常进服，而且没有皮肤。
#[tokio::test]
async fn offline_mode_does_not_ask_for_a_key() {
    let session = FakeSession::start(Answer::Verified(Uuid::from_u128(1))).await;
    let server = TestServer::start(ServerConfig {
        online_mode: false,
        ..online_server(&session)
    })
    .await;

    let mut client = server.connect().await;
    client.send(&handshake(LOGIN_INTENT)).await;
    client.send(&hello("Notch")).await;
    let finished = client.configure_after_hello(&client_information(1)).await;
    client.finish_configuration().await;

    let mut reader = Reader::new(&finished);
    assert_eq!(reader.varint(), Ok(login::clientbound::LOGIN_FINISHED));
    // 离线模式的 UUID 由玩家名算出来，不是 session server 给的那个。
    assert_eq!(
        reader.uuid(),
        Ok(Uuid::parse_str("b50ad385-829d-3141-a216-7e7d7539ba7f").unwrap())
    );
    assert_eq!(reader.string(16).unwrap(), "Notch");
    assert_eq!(reader.count(8), Ok(0), "offline players have no skin");
    assert!(session.requests().is_empty());
    server.wait_for_players(&["Notch"]).await;
}
