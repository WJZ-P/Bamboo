//! 集成测试共用的测试服务端和假客户端。

#![allow(dead_code)]

use std::{net::SocketAddr, time::Duration};

use mc_bamboo_data::packets::{configuration, login, play};
use mc_bamboo_protocol::{
    PROTOCOL_VERSION,
    codec::{Reader, write_string, write_uuid, write_varint},
    frame::{MAX_FRAME_LEN, find_frame},
};
use mc_bamboo_server::{Server, ServerConfig, ServerHandle};
use serde_json::Value;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::oneshot,
    task::JoinHandle,
    time::timeout,
};
use uuid::Uuid;

pub const STATUS_INTENT: i32 = 1;
pub const LOGIN_INTENT: i32 = 2;
pub const TEST_DEADLINE: Duration = Duration::from_secs(5);

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
        Client {
            stream: TcpStream::connect(self.address).await.unwrap(),
            buffer: Vec::new(),
        }
    }

    /// 走完整个登录和配置流程，返回已经进入 Play 状态的客户端。
    pub async fn join(&self, name: &str) -> Client {
        let mut client = self.connect().await;
        client.send(&handshake(LOGIN_INTENT)).await;
        client.send(&hello(name)).await;

        let finished = client.read_frame().await;
        assert_eq!(packet_id(&finished), login::clientbound::LOGIN_FINISHED);
        client
            .send(&frame(&[login::serverbound::LOGIN_ACKNOWLEDGED as u8]))
            .await;

        client
            .read_until(configuration::clientbound::SELECT_KNOWN_PACKS)
            .await;
        client.send(&official_known_packs()).await;
        client
            .read_until(configuration::clientbound::FINISH_CONFIGURATION)
            .await;
        client
            .send(&frame(&[
                configuration::serverbound::FINISH_CONFIGURATION as u8
            ]))
            .await;
        client
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
}

impl Client {
    pub async fn send(&mut self, bytes: &[u8]) {
        self.stream.write_all(bytes).await.unwrap();
    }

    pub async fn read_frame(&mut self) -> Vec<u8> {
        timeout(TEST_DEADLINE, async {
            loop {
                if let Some(frame) = find_frame(&self.buffer, MAX_FRAME_LEN).unwrap() {
                    let end = frame.header_len + frame.payload_len;
                    let payload = self.buffer[frame.header_len..end].to_vec();
                    self.buffer.drain(..end);
                    return payload;
                }
                let read = self.stream.read_buf(&mut self.buffer).await.unwrap();
                assert_ne!(read, 0, "server closed the connection before a full frame");
            }
        })
        .await
        .expect("no frame arrived in time")
    }

    /// 一直读到指定 ID 的数据包，返回它。
    pub async fn read_until(&mut self, id: i32) -> Vec<u8> {
        loop {
            let payload = self.read_frame().await;
            if packet_id(&payload) == id {
                return payload;
            }
        }
    }

    /// 读完所有区块批次，返回收到的区块数量。
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
                }
                _ => {}
            }
        }
        chunks
    }

    /// 断言服务端已经关闭连接，正常关闭或 reset 都算。
    pub async fn assert_closed(&mut self) {
        let mut byte = [0];
        let read = timeout(TEST_DEADLINE, self.stream.read(&mut byte))
            .await
            .expect("connection stayed open");
        assert!(!matches!(read, Ok(1..)), "unexpected data from the server");
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
