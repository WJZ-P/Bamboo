//! Drives a real server over TCP the way a client does.

use std::{
    net::SocketAddr,
    time::{Duration, Instant},
};

use mc_bamboo::{ServerConfig, serve};
use mc_bamboo_protocol::{
    PROTOCOL_VERSION,
    codec::{Reader, write_string, write_varint},
    frame::{MAX_FRAME_LEN, find_frame},
};
use serde_json::Value;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
    time::{sleep, timeout},
};

const STATUS_INTENT: i32 = 1;
const LOGIN_INTENT: i32 = 2;
const TEST_DEADLINE: Duration = Duration::from_secs(5);

struct TestServer {
    address: SocketAddr,
    task: JoinHandle<()>,
}

impl TestServer {
    async fn start(config: ServerConfig) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            serve(listener, config, std::future::pending())
                .await
                .unwrap();
        });
        Self { address, task }
    }

    async fn connect(&self) -> Client {
        Client {
            stream: TcpStream::connect(self.address).await.unwrap(),
            buffer: Vec::new(),
        }
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

struct Client {
    stream: TcpStream,
    buffer: Vec<u8>,
}

impl Client {
    async fn send(&mut self, bytes: &[u8]) {
        self.stream.write_all(bytes).await.unwrap();
    }

    async fn read_frame(&mut self) -> Vec<u8> {
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

    /// Asserts that the server has closed the connection, cleanly or with a reset.
    async fn assert_closed(&mut self) {
        let mut byte = [0];
        let read = timeout(TEST_DEADLINE, self.stream.read(&mut byte))
            .await
            .expect("connection stayed open");
        assert!(!matches!(read, Ok(1..)), "unexpected data from the server");
    }
}

fn frame(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    write_varint(&mut out, payload.len() as i32);
    out.extend_from_slice(payload);
    out
}

fn handshake(intent: i32) -> Vec<u8> {
    let mut payload = vec![0x00];
    write_varint(&mut payload, PROTOCOL_VERSION);
    write_string(&mut payload, "localhost");
    payload.extend_from_slice(&25565u16.to_be_bytes());
    write_varint(&mut payload, intent);
    frame(&payload)
}

fn status_request() -> Vec<u8> {
    frame(&[0x00])
}

fn ping_request(value: i64) -> Vec<u8> {
    let mut payload = vec![0x01];
    payload.extend_from_slice(&value.to_be_bytes());
    frame(&payload)
}

/// Splits a payload made of a packet id and one string, then parses the string as JSON.
fn json_packet(payload: &[u8]) -> (i32, Value) {
    let mut reader = Reader::new(payload);
    let id = reader.varint().unwrap();
    let json = serde_json::from_str(reader.string(32767).unwrap()).unwrap();
    reader.finish().unwrap();
    (id, json)
}

fn pong_value(payload: &[u8]) -> i64 {
    let mut reader = Reader::new(payload);
    assert_eq!(reader.varint(), Ok(0x01));
    let value = reader.i64().unwrap();
    reader.finish().unwrap();
    value
}

#[tokio::test]
async fn answers_status_and_ping_then_closes() {
    let server = TestServer::start(ServerConfig::default()).await;
    let mut client = server.connect().await;

    // A real client sends these two packets back to back without waiting.
    let mut opening = handshake(STATUS_INTENT);
    opening.extend(status_request());
    client.send(&opening).await;

    let (id, status) = json_packet(&client.read_frame().await);
    assert_eq!(id, 0x00);
    assert_eq!(status["version"]["name"], "Bamboo 26.3");
    assert_eq!(status["version"]["protocol"], PROTOCOL_VERSION);
    assert_eq!(status["players"]["max"], 20);
    assert_eq!(status["players"]["online"], 0);
    assert_eq!(status["description"]["text"], "A Bamboo Server");
    assert_eq!(status["enforcesSecureChat"], false);

    client.send(&ping_request(-123_456_789)).await;
    assert_eq!(pong_value(&client.read_frame().await), -123_456_789);
    client.assert_closed().await;
}

#[tokio::test]
async fn reads_input_that_arrives_one_byte_at_a_time() {
    let server = TestServer::start(ServerConfig::default()).await;
    let mut client = server.connect().await;

    let mut opening = handshake(STATUS_INTENT);
    opening.extend(status_request());
    for byte in opening {
        client.send(&[byte]).await;
        tokio::task::yield_now().await;
    }

    let (id, _) = json_packet(&client.read_frame().await);
    assert_eq!(id, 0x00);
}

#[tokio::test]
async fn ping_works_without_a_status_request() {
    let server = TestServer::start(ServerConfig::default()).await;
    let mut client = server.connect().await;

    let mut opening = handshake(STATUS_INTENT);
    opening.extend(ping_request(7));
    client.send(&opening).await;

    assert_eq!(pong_value(&client.read_frame().await), 7);
    client.assert_closed().await;
}

#[tokio::test]
async fn second_status_request_closes_the_connection() {
    let server = TestServer::start(ServerConfig::default()).await;
    let mut client = server.connect().await;

    let mut opening = handshake(STATUS_INTENT);
    opening.extend(status_request());
    client.send(&opening).await;
    client.read_frame().await;

    client.send(&status_request()).await;
    client.assert_closed().await;
}

#[tokio::test]
async fn unknown_status_packet_closes_the_connection() {
    let server = TestServer::start(ServerConfig::default()).await;
    let mut client = server.connect().await;

    let mut opening = handshake(STATUS_INTENT);
    opening.extend(frame(&[0x7f]));
    client.send(&opening).await;

    client.assert_closed().await;
}

#[tokio::test]
async fn garbage_instead_of_a_handshake_closes_the_connection() {
    let server = TestServer::start(ServerConfig::default()).await;
    let mut client = server.connect().await;

    // 'G' reads as a frame length of 71, so the request has to be long enough to complete it.
    client
        .send("GET / HTTP/1.1\r\n\r\n".repeat(5).as_bytes())
        .await;

    client.assert_closed().await;
}

#[tokio::test]
async fn oversized_frame_is_refused_from_its_header_alone() {
    let server = TestServer::start(ServerConfig::default()).await;
    let mut client = server.connect().await;

    // Declares a 2 MiB frame and then sends none of it.
    client.send(&[0xff, 0xff, 0x7f]).await;

    client.assert_closed().await;
}

#[tokio::test]
async fn login_is_refused_with_a_message() {
    let server = TestServer::start(ServerConfig::default()).await;
    let mut client = server.connect().await;

    client.send(&handshake(LOGIN_INTENT)).await;
    // The login packet lands after the server has already answered. Unread input must not
    // reset the connection and destroy the message on its way to the client.
    sleep(Duration::from_millis(100)).await;
    client
        .send(&frame(&[0x00, 0x06, b'K', b'i', b't', b't', b'e', b'n']))
        .await;

    let (id, reason) = json_packet(&client.read_frame().await);
    assert_eq!(id, 0x00);
    assert!(reason["text"].as_str().unwrap().contains("not available"));
}

#[tokio::test]
async fn silent_connection_times_out() {
    let config = ServerConfig {
        pre_play_timeout: Duration::from_millis(100),
        ..ServerConfig::default()
    };
    let server = TestServer::start(config).await;
    let mut client = server.connect().await;

    client.assert_closed().await;
}

#[tokio::test]
async fn trickling_client_cannot_outlast_the_time_limit() {
    let config = ServerConfig {
        pre_play_timeout: Duration::from_millis(300),
        ..ServerConfig::default()
    };
    let server = TestServer::start(config).await;
    let mut client = server.connect().await;
    let started = Instant::now();

    // Declares a 1000 byte frame and feeds it far more slowly than it could ever finish.
    // Every byte arrives well inside the limit, so a timeout that restarted on each read
    // would keep this loop running to its end.
    client.send(&[0xe8, 0x07]).await;
    for _ in 0..100 {
        sleep(Duration::from_millis(50)).await;
        if client.stream.write_all(&[0]).await.is_err() {
            break;
        }
    }

    assert!(
        started.elapsed() < Duration::from_secs(2),
        "client was still connected after {:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn connections_over_the_limit_are_dropped() {
    let config = ServerConfig {
        max_pre_play_connections: 1,
        ..ServerConfig::default()
    };
    let server = TestServer::start(config).await;

    let mut first = server.connect().await;
    let mut second = server.connect().await;
    second.assert_closed().await;

    // The connection holding the slot keeps working.
    let mut opening = handshake(STATUS_INTENT);
    opening.extend(ping_request(1));
    first.send(&opening).await;
    assert_eq!(pong_value(&first.read_frame().await), 1);
}

#[tokio::test]
async fn a_finished_connection_frees_its_slot() {
    let config = ServerConfig {
        max_pre_play_connections: 1,
        ..ServerConfig::default()
    };
    let server = TestServer::start(config).await;

    for value in 0..3 {
        let mut client = server.connect().await;
        let mut opening = handshake(STATUS_INTENT);
        opening.extend(ping_request(value));
        client.send(&opening).await;
        assert_eq!(pong_value(&client.read_frame().await), value);
        drop(client);
        // The slot is released on the server's side shortly after the client goes away.
        sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn serve_returns_when_shutdown_completes() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(serve(listener, ServerConfig::default(), async {
        let _ = stopped.await;
    }));

    stop.send(()).unwrap();

    let result = timeout(TEST_DEADLINE, server)
        .await
        .expect("server kept running after shutdown");
    assert_eq!(result.unwrap(), Ok(()));
}
