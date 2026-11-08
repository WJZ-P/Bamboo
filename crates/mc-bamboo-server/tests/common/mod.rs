//! 集成测试共用的测试服务端和假客户端。

#![allow(dead_code)]

pub mod player;
pub mod wire;

use std::{net::SocketAddr, time::Duration};

use mc_bamboo_auth::{DecryptHalf, EncryptHalf, Encryption, SharedSecret};
use mc_bamboo_data::packets::{configuration, login, play};
use mc_bamboo_protocol::{
    PROTOCOL_VERSION,
    codec::{Reader, write_string, write_uuid, write_varint},
    compression,
    frame::{MAX_FRAME_LEN, find_frame},
    text::Component,
};
use mc_bamboo_server::{Server, ServerConfig, ServerHandle};
use player::{Answers, Player};
use serde_json::Value;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpSocket, TcpStream},
    sync::oneshot,
    task::JoinHandle,
    time::{sleep, timeout},
};
use uuid::Uuid;

pub const STATUS_INTENT: i32 = 1;
pub const LOGIN_INTENT: i32 = 2;
pub const TEST_DEADLINE: Duration = Duration::from_secs(5);
/// 客户端确认一批区块时要的发送速度：官方允许的上限。
const FAST_CHUNKS_PER_TICK: f32 = 64.0;
/// 不读数据的客户端用的接收缓冲。缓冲越小，服务端的写入越早卡住。
const SMALL_RECEIVE_BUFFER: u32 = 1024;
/// 不读数据的客户端请求的视距。`T(8)` 有 329 个区块，大约 18 MB。
const STUCK_VIEW_DISTANCE: i8 = 8;
/// 等服务端把区块都写进 socket，直到写不动。
const STUCK_SETTLE_TIME: Duration = Duration::from_millis(500);
/// 官方客户端默认打开全部皮肤外层。
pub const ALL_MODEL_PARTS: u8 = 0x7f;

pub struct TestServer {
    pub address: SocketAddr,
    pub handle: ServerHandle,
    stop: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
}

impl TestServer {
    pub async fn start(config: ServerConfig) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = Server::new(listener, config).unwrap();
        let handle = server.handle();
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(server.run(async {
            let _ = stopped.await;
        }));
        Self {
            address,
            handle,
            stop: Some(stop),
            task,
        }
    }

    pub async fn connect(&self) -> Client {
        Client::new(TcpStream::connect(self.address).await.unwrap())
    }

    /// 走完整个登录和配置流程，返回已经进入 Play 状态的客户端。
    pub async fn join(&self, name: &str) -> Client {
        let mut client = self.connect().await;
        client.configure(name).await;
        client.finish_configuration().await;
        client
    }

    /// 像真实客户端一样进服，不等区块。
    pub async fn spawn_player(&self, name: &str, view_distance: i8, answers: Answers) -> Player {
        self.spawn_player_with(name, &client_information(view_distance), answers)
            .await
    }

    /// 和 [`spawn_player`](Self::spawn_player) 相同，Configuration 里发 `information`。
    pub async fn spawn_player_with(
        &self,
        name: &str,
        information: &[u8],
        answers: Answers,
    ) -> Player {
        let mut client = self.connect().await;
        client.configure_with(information, name).await;
        client.finish_configuration().await;
        Player::attach(client, name, answers).await
    }

    /// 视距 2 的玩家，返回时出生点周围的区块都到了。
    pub async fn enter(&self, name: &str) -> Player {
        self.enter_with_view_distance(name, 2).await
    }

    /// `view_distance` 不能超过服务端视距，否则等不到这么多区块。
    pub async fn enter_with_view_distance(&self, name: &str, view_distance: i8) -> Player {
        let player = self
            .spawn_player(name, view_distance, Answers::default())
            .await;
        player
            .wait_for_chunks((0, 0), i32::from(view_distance))
            .await;
        player
    }

    /// 进服之后一个字节也不读的客户端，返回时服务端写这条连接已经卡住了。
    ///
    /// 只靠第一批区块卡不住：Windows 的 socket 一次就能收下 490 KB。
    /// 所以这里确认一次批次，服务端就会把 `T(8)` 的区块连续发过来。服务端视距要至少是 8。
    pub async fn join_without_reading(&self, name: &str) -> Client {
        let socket = TcpSocket::new_v4().unwrap();
        socket.set_recv_buffer_size(SMALL_RECEIVE_BUFFER).unwrap();
        let mut client = Client::new(socket.connect(self.address).await.unwrap());
        client
            .configure_with(&client_information(STUCK_VIEW_DISTANCE), name)
            .await;
        client.finish_configuration().await;
        client
            .send(&chunk_batch_received(FAST_CHUNKS_PER_TICK))
            .await;
        sleep(STUCK_SETTLE_TIME).await;
        client
    }

    /// 等到在线玩家正好是 `expected`，顺序不限。
    pub async fn wait_for_players(&self, expected: &[&str]) {
        let mut expected: Vec<&str> = expected.to_vec();
        expected.sort_unstable();
        timeout(TEST_DEADLINE, async {
            loop {
                let players = self.handle.online_players();
                let mut names: Vec<&str> =
                    players.iter().map(|player| player.name.as_str()).collect();
                names.sort_unstable();
                if names == expected {
                    return;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("online players did not reach the expected list");
    }

    /// 停止服务端，等它完全结束。
    pub async fn shutdown(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        timeout(TEST_DEADLINE, &mut self.task)
            .await
            .expect("server kept running after shutdown")
            .unwrap();
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub struct Client {
    pub stream: TcpStream,
    pub buffer: Vec<u8>,
    /// 服务端发过登录压缩包之后是它的阈值，两个方向都换成压缩帧。
    compression: Option<i32>,
    /// 见过 `login_finished` 之后，Play 状态里同样的包 ID 不再当作压缩包。
    login_done: bool,
    /// 交换过密钥之后两个方向的 AES 流。
    encrypt: Option<EncryptHalf>,
    decrypt: Option<DecryptHalf>,
}

impl Client {
    /// 刚连上的假客户端：没有读缓冲，还没收到登录压缩包。
    pub fn new(stream: TcpStream) -> Self {
        Self {
            stream,
            buffer: Vec::new(),
            compression: None,
            login_done: false,
            encrypt: None,
            decrypt: None,
        }
    }

    /// 从下一个字节开始两个方向都加密，和服务端收到 key 包之后的切换对应。
    pub fn enable_encryption(&mut self, secret: &SharedSecret) {
        let (encrypt, decrypt) = Encryption::new(secret).split();
        self.encrypt = Some(encrypt);
        self.decrypt = Some(decrypt);
    }

    /// 走到服务端说 Configuration 结束为止，这时玩家还没有进入世界。
    pub async fn configure(&mut self, name: &str) {
        self.configure_with(&[], name).await;
    }

    /// 和 [`configure`](Self::configure) 相同，登录确认之后先发 `first_packets`，
    /// 真实客户端在这里发 `client_information`。
    pub async fn configure_with(&mut self, first_packets: &[u8], name: &str) {
        self.send(&handshake(LOGIN_INTENT)).await;
        self.send(&hello(name)).await;
        self.configure_after_hello(first_packets).await;
    }

    /// 从等 `login_finished` 开始走完 Login 和 Configuration，返回那个包的负载。
    ///
    /// 正版验证的客户端在 `hello` 之后还要交换密钥，所以那条路在交换完之后接着走这一段。
    pub async fn configure_after_hello(&mut self, first_packets: &[u8]) -> Vec<u8> {
        let finished = self.read_frame().await;
        assert_eq!(packet_id(&finished), login::clientbound::LOGIN_FINISHED);
        self.send(&frame(&[login::serverbound::LOGIN_ACKNOWLEDGED as u8]))
            .await;
        self.send(first_packets).await;

        self.read_until(configuration::clientbound::SELECT_KNOWN_PACKS)
            .await;
        self.send(&official_known_packs()).await;
        self.read_until(configuration::clientbound::FINISH_CONFIGURATION)
            .await;
        finished
    }

    /// 告诉服务端 Configuration 结束，之后就是 Play 状态了。
    pub async fn finish_configuration(&mut self) {
        self.send(&frame(&[
            configuration::serverbound::FINISH_CONFIGURATION as u8
        ]))
        .await;
    }

    /// 发一批帧。压缩启用后像客户端一样逐帧重新包装，加密启用后整段再加密。
    pub async fn send(&mut self, bytes: &[u8]) {
        let mut bytes = encode_outgoing(bytes, self.compression);
        if let Some(encrypt) = &mut self.encrypt {
            encrypt.encrypt(&mut bytes);
        }
        self.stream.write_all(&bytes).await.unwrap();
    }

    /// 收到的登录压缩包里的阈值。关着压缩时是 `None`。
    pub fn compression_threshold(&self) -> Option<i32> {
        self.compression
    }

    /// 读一个包。登录压缩包由客户端自己消化，就像真实客户端那样。
    pub async fn read_frame(&mut self) -> Vec<u8> {
        timeout(TEST_DEADLINE, async {
            loop {
                if let Some(frame) = find_frame(&self.buffer, MAX_FRAME_LEN).unwrap() {
                    let end = frame.header_len + frame.payload_len;
                    let mut payload = self.buffer[frame.header_len..end].to_vec();
                    self.buffer.drain(..end);

                    if self.compression.is_none()
                        && !self.login_done
                        && packet_id(&payload) == login::clientbound::LOGIN_COMPRESSION
                    {
                        self.compression = Some(login_compression_threshold(&payload));
                        continue;
                    }

                    if let Some(threshold) = self.compression {
                        payload = compression::decode(&payload, threshold)
                            .expect("the server should send valid compressed packets");
                    }
                    if packet_id(&payload) == login::clientbound::LOGIN_FINISHED {
                        self.login_done = true;
                    }
                    return payload;
                }
                let read_from = self.buffer.len();
                let read = self.stream.read_buf(&mut self.buffer).await.unwrap();
                assert_ne!(read, 0, "server closed the connection before a full frame");
                if let Some(decrypt) = &mut self.decrypt {
                    decrypt.decrypt(&mut self.buffer[read_from..]);
                }
            }
        })
        .await
        .expect("no frame arrived in time")
    }

    /// 一直读到指定 ID 的数据包，返回它。
    ///
    /// 时限是整个等待过程的：服务端一直发别的包时也要失败，不能挂住测试。
    pub async fn read_until(&mut self, id: i32) -> Vec<u8> {
        timeout(TEST_DEADLINE, async {
            loop {
                let payload = self.read_frame().await;
                if packet_id(&payload) == id {
                    return payload;
                }
            }
        })
        .await
        .expect("the packet did not arrive in time")
    }

    /// 读完所有区块批次，返回收到的区块数量。
    ///
    /// 像真实客户端一样确认每一批，否则服务端发完第一批就停了。心跳也要回，
    /// 测试用的心跳间隔很短，读区块的这段时间就会到期。
    pub async fn read_chunks(&mut self, expected: usize) -> usize {
        let mut chunks = 0;
        let mut announced = 0;
        while announced < expected {
            let payload = self.read_frame().await;
            let mut reader = Reader::new(&payload);
            match reader.varint().unwrap() {
                play::clientbound::LEVEL_CHUNK_WITH_LIGHT => chunks += 1,
                play::clientbound::CHUNK_BATCH_FINISHED => {
                    announced += reader.varint().unwrap() as usize;
                    self.send(&chunk_batch_received(FAST_CHUNKS_PER_TICK)).await;
                }
                play::clientbound::KEEP_ALIVE => self.answer_keep_alive(&payload).await,
                _ => {}
            }
        }
        chunks
    }

    /// 把心跳原样发回去。
    pub async fn answer_keep_alive(&mut self, keep_alive: &[u8]) {
        let mut answer = vec![play::serverbound::KEEP_ALIVE as u8];
        answer.extend_from_slice(&keep_alive[1..]);
        self.send(&frame(&answer)).await;
    }

    /// 和 [`read_until`](Self::read_until) 相同，路上的心跳都回应。
    pub async fn read_until_answering(&mut self, id: i32) -> Vec<u8> {
        timeout(TEST_DEADLINE, async {
            loop {
                let payload = self.read_frame().await;
                let packet = packet_id(&payload);
                if packet == id {
                    return payload;
                }
                if packet == play::clientbound::KEEP_ALIVE {
                    self.answer_keep_alive(&payload).await;
                }
            }
        })
        .await
        .expect("the packet did not arrive in time")
    }

    /// 断言服务端已经关闭连接，正常关闭或 reset 都算。
    pub async fn assert_closed(&mut self) {
        let mut byte = [0];
        let read = timeout(TEST_DEADLINE, self.stream.read(&mut byte))
            .await
            .expect("connection stayed open");
        assert!(!matches!(read, Ok(1..)), "unexpected data from the server");
    }

    /// 丢掉还没读的包，等服务端关闭连接。
    pub async fn assert_closed_after_reading(&mut self) {
        let mut discarded = [0; 4096];
        let closed = timeout(TEST_DEADLINE, async {
            while matches!(self.stream.read(&mut discarded).await, Ok(1..)) {}
        })
        .await;
        assert!(closed.is_ok(), "connection stayed open");
    }
}

/// 按当前压缩状态把测试要发出去的帧逐帧重新包装，和真实客户端一样。
pub fn encode_outgoing(bytes: &[u8], threshold: Option<i32>) -> Vec<u8> {
    match threshold {
        Some(threshold) => compression::encode_frames(bytes, threshold)
            .expect("the test frames should be complete packets"),
        None => bytes.to_vec(),
    }
}

pub fn frame(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    write_varint(&mut out, payload.len() as i32);
    out.extend_from_slice(payload);
    out
}

pub fn handshake(intent: i32) -> Vec<u8> {
    handshake_with_protocol(PROTOCOL_VERSION, intent)
}

pub fn handshake_with_protocol(protocol: i32, intent: i32) -> Vec<u8> {
    let mut payload = vec![0x00];
    write_varint(&mut payload, protocol);
    write_string(&mut payload, "localhost");
    payload.extend_from_slice(&25565u16.to_be_bytes());
    write_varint(&mut payload, intent);
    frame(&payload)
}

pub fn hello(name: &str) -> Vec<u8> {
    let mut payload = vec![login::serverbound::HELLO as u8];
    write_string(&mut payload, name);
    write_uuid(&mut payload, Uuid::nil());
    frame(&payload)
}

pub fn status_request() -> Vec<u8> {
    frame(&[0x00])
}

/// Configuration 状态的 `client_information`，除了视距都是真实客户端的常见值。
pub fn client_information(view_distance: i8) -> Vec<u8> {
    client_information_with(view_distance, ALL_MODEL_PARTS, wire::ARM_RIGHT)
}

/// Configuration 状态的 `client_information`，皮肤外层和主手都可以指定。
pub fn client_information_with(view_distance: i8, model_parts: u8, main_hand: i32) -> Vec<u8> {
    let mut payload = vec![configuration::serverbound::CLIENT_INFORMATION as u8];
    wire::write_client_information(&mut payload, view_distance, model_parts, main_hand);
    frame(&payload)
}

pub fn chunk_batch_received(desired_chunks_per_tick: f32) -> Vec<u8> {
    let mut payload = vec![play::serverbound::CHUNK_BATCH_RECEIVED as u8];
    payload.extend_from_slice(&desired_chunks_per_tick.to_be_bytes());
    frame(&payload)
}

/// 确认第一次传送，真实客户端收到 `player_position` 就会马上发。
pub fn accept_teleportation(x: f64, y: f64, z: f64) -> Vec<u8> {
    let mut payload = vec![play::serverbound::ACCEPT_TELEPORTATION as u8, 1];
    for coordinate in [x, y, z] {
        payload.extend_from_slice(&coordinate.to_be_bytes());
    }
    payload.extend_from_slice(&[0; 8]);
    frame(&payload)
}

pub fn move_player_pos(x: f64, y: f64, z: f64) -> Vec<u8> {
    let mut payload = vec![play::serverbound::MOVE_PLAYER_POS as u8];
    for coordinate in [x, y, z] {
        payload.extend_from_slice(&coordinate.to_be_bytes());
    }
    payload.push(0x01);
    frame(&payload)
}

/// 没有参数的翻译文本的线上字节，用来和断开包比较。
pub fn translated(key: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    Component::translate(key, []).write(&mut bytes);
    bytes
}

/// `disconnect` 包的内容：包 ID 之后都是原因。
pub fn disconnect_body(payload: &[u8]) -> &[u8] {
    let mut reader = Reader::new(payload);
    reader.varint().unwrap();
    reader.rest()
}

pub fn ping_request(value: i64) -> Vec<u8> {
    let mut payload = vec![0x01];
    payload.extend_from_slice(&value.to_be_bytes());
    frame(&payload)
}

/// 回应服务端询问的内容包：声明认识官方内容包。
pub fn official_known_packs() -> Vec<u8> {
    let mut payload = vec![configuration::serverbound::SELECT_KNOWN_PACKS as u8];
    write_varint(&mut payload, 1);
    write_string(&mut payload, "minecraft");
    write_string(&mut payload, "core");
    write_string(&mut payload, mc_bamboo_protocol::GAME_VERSION);
    frame(&payload)
}

pub fn packet_id(payload: &[u8]) -> i32 {
    Reader::new(payload).varint().unwrap()
}

/// `login_compression` 包里的阈值。
fn login_compression_threshold(payload: &[u8]) -> i32 {
    let mut reader = Reader::new(payload);
    assert_eq!(reader.varint(), Ok(login::clientbound::LOGIN_COMPRESSION));
    let threshold = reader.varint().unwrap();
    reader.finish().unwrap();
    threshold
}

/// 拆开由数据包 ID 和一个字符串组成的负载，并把字符串解析成 JSON。
pub fn json_packet(payload: &[u8]) -> (i32, Value) {
    let mut reader = Reader::new(payload);
    let id = reader.varint().unwrap();
    let json = serde_json::from_str(reader.string(32767).unwrap()).unwrap();
    reader.finish().unwrap();
    (id, json)
}

pub fn pong_value(payload: &[u8]) -> i64 {
    let mut reader = Reader::new(payload);
    assert_eq!(reader.varint(), Ok(0x01));
    let value = reader.i64().unwrap();
    reader.finish().unwrap();
    value
}
