//! 像客户端一样通过 TCP 驱动真实的服务端：握手、服务器列表和连接保护。

mod common;

use std::time::{Duration, Instant};

use common::{
    Client, STATUS_INTENT, TEST_DEADLINE, TestServer, frame, handshake, json_packet, ping_request,
    pong_value, status_request,
};
use mc_bamboo_protocol::PROTOCOL_VERSION;
use mc_bamboo_server::{Server, ServerConfig};
use tokio::{
    io::AsyncWriteExt,
    net::{TcpListener, TcpStream},
    time::{sleep, timeout},
};

#[tokio::test]
async fn answers_status_and_ping_then_closes() {
    let server = TestServer::start(ServerConfig::default()).await;
    let mut client = server.connect().await;

    // 真实客户端会连续发送这两个包，中间不等回应。
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

    // 'G' 会被读成长度 71 的帧头，所以请求要足够长才能凑满这一帧。
    client
        .send("GET / HTTP/1.1\r\n\r\n".repeat(5).as_bytes())
        .await;

    client.assert_closed().await;
}

#[tokio::test]
async fn oversized_frame_is_refused_from_its_header_alone() {
    let server = TestServer::start(ServerConfig::default()).await;
    let mut client = server.connect().await;

    // 声明一个 2 MiB 的帧，但内容一个字节都不发。
    client.send(&[0xff, 0xff, 0x7f]).await;

    client.assert_closed().await;
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

    // 声明一个 1000 字节的帧，然后用根本发不完的速度慢慢喂。
    // 每个字节都远早于时限到达，所以如果超时在每次读取时重新计时，
    // 这个循环就会一直跑到结束。
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

    // 占着名额的连接不受影响。
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
        // 客户端断开后，服务端很快就会释放名额。
        sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn shutdown_closes_connections_and_frees_the_port() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = Server::new(listener, ServerConfig::default()).unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let running = tokio::spawn(server.run(async {
        let _ = stopped.await;
    }));

    // 这条连接什么都不发，只有关闭流程才能让它结束。
    let mut client = Client::new(TcpStream::connect(address).await.unwrap());
    sleep(Duration::from_millis(50)).await;

    stop.send(()).unwrap();
    timeout(TEST_DEADLINE, running)
        .await
        .expect("server kept running after shutdown")
        .unwrap();

    client.assert_closed().await;
    TcpListener::bind(address)
        .await
        .expect("port should be free right after shutdown");
}
