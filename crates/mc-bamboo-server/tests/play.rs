//! 真实 TCP 上的多人场景：连接任务和世界线程要一起工作。

mod common;

use std::{
    collections::BTreeSet,
    future::Future,
    time::{Duration, Instant},
};

use common::{
    Client, TEST_DEADLINE, TestServer, accept_teleportation, client_information, client_tick_end,
    disconnect_body, frame, move_player_pos, packet_id, player_loaded, translated,
};
use mc_bamboo_data::packets::play;
use mc_bamboo_protocol::codec::Reader;
use mc_bamboo_server::ServerConfig;
use tokio::time::timeout;

/// 视距 2 时的 `T(2)`。客户端没发 `client_information` 时请求的也是 2。
const CHUNKS: usize = 49;
const SPAWN_Y: f64 = -60.0;

fn small_world() -> ServerConfig {
    ServerConfig {
        view_distance: 2,
        ..ServerConfig::default()
    }
}

fn fast_keep_alive() -> ServerConfig {
    ServerConfig {
        keep_alive_interval: Duration::from_millis(300),
        ..small_world()
    }
}

/// 视距够大，不读数据的客户端才写得满 socket。
fn world_for_stuck_clients() -> ServerConfig {
    ServerConfig {
        view_distance: 8,
        ..ServerConfig::default()
    }
}

/// 读到系统消息，返回它的内容。
async fn read_system_chat(client: &mut Client) -> Vec<u8> {
    client
        .read_until_answering(play::clientbound::SYSTEM_CHAT)
        .await
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|part| part == needle)
}

/// 等 `done` 完成，这段时间一直回心跳，否则这名玩家自己也会超时。
async fn answer_keep_alives_until(client: &mut Client, done: impl Future<Output = ()>) {
    tokio::pin!(done);
    loop {
        tokio::select! {
            () = &mut done => return,
            payload = client.read_frame() => {
                if packet_id(&payload) == play::clientbound::KEEP_ALIVE {
                    client.answer_keep_alive(&payload).await;
                }
            }
        }
    }
}

#[tokio::test]
async fn two_sequential_clients_get_the_whole_world() {
    let server = TestServer::start(small_world()).await;

    let mut first = server.join("Alice").await;
    assert_eq!(first.read_chunks(CHUNKS).await, CHUNKS);
    drop(first);
    server.wait_for_players(&[]).await;

    let mut second = server.join("Bob").await;
    assert_eq!(second.read_chunks(CHUNKS).await, CHUNKS);
    server.wait_for_players(&["Bob"]).await;
}

#[tokio::test]
async fn players_online_at_the_same_time_hear_about_each_other() {
    let server = TestServer::start(small_world()).await;
    let mut first = server.join("Alice").await;
    first.read_chunks(CHUNKS).await;

    let mut second = server.join("Bob").await;
    second.read_chunks(CHUNKS).await;

    let joined = read_system_chat(&mut first).await;
    assert!(contains(&joined, b"multiplayer.player.joined"));
    assert!(contains(&joined, b"Bob"));
    assert!(contains(&joined, b"yellow"));

    drop(second);
    let left = read_system_chat(&mut first).await;
    assert!(contains(&left, b"multiplayer.player.left"));
    assert!(contains(&left, b"Bob"));

    let removal = first
        .read_until_answering(play::clientbound::PLAYER_INFO_REMOVE)
        .await;
    assert_eq!(packet_id(&removal), play::clientbound::PLAYER_INFO_REMOVE);
    server.wait_for_players(&["Alice"]).await;
}

#[tokio::test]
async fn reconnecting_with_the_same_name_kicks_the_old_connection() {
    let server = TestServer::start(small_world()).await;
    let mut old = server.join("Kitten").await;
    old.read_chunks(CHUNKS).await;

    let mut new = server.join("Kitten").await;

    let disconnect = old.read_until(play::clientbound::DISCONNECT).await;
    assert_eq!(
        disconnect_body(&disconnect),
        translated("multiplayer.disconnect.duplicate_login")
    );
    old.assert_closed().await;

    assert_eq!(new.read_chunks(CHUNKS).await, CHUNKS);
    server.wait_for_players(&["Kitten"]).await;
}

#[tokio::test]
async fn new_login_takes_over_a_connection_that_never_reads() {
    let server = TestServer::start(world_for_stuck_clients()).await;
    let _old = server.join_without_reading("Kitten").await;

    // 旧连接写不动时，也不能让新连接等到登录时限。
    let started = Instant::now();
    let mut new = server.join("Kitten").await;

    assert_eq!(new.read_chunks(CHUNKS).await, CHUNKS);
    assert!(started.elapsed() < TEST_DEADLINE);
    server.wait_for_players(&["Kitten"]).await;
}

#[tokio::test]
async fn duplicate_login_during_configuration_just_ends_the_old_connection() {
    let server = TestServer::start(small_world()).await;
    let mut old = server.connect().await;
    old.configure("Kitten").await;

    let mut new = server.join("Kitten").await;

    old.assert_closed().await;
    assert_eq!(new.read_chunks(CHUNKS).await, CHUNKS);
    server.wait_for_players(&["Kitten"]).await;
}

#[tokio::test]
async fn several_reconnects_in_a_row_leave_one_player() {
    let server = TestServer::start(small_world()).await;
    let mut clients = Vec::new();
    for _ in 0..3 {
        clients.push(server.join("Kitten").await);
    }

    let mut last = clients.pop().expect("至少有一个客户端");
    assert_eq!(last.read_chunks(CHUNKS).await, CHUNKS);
    server.wait_for_players(&["Kitten"]).await;
}

#[tokio::test]
async fn client_that_never_reads_does_not_stop_another_client() {
    let config = ServerConfig {
        keep_alive_interval: Duration::from_millis(300),
        ..world_for_stuck_clients()
    };
    let server = TestServer::start(config).await;
    let _stuck = server.join_without_reading("Stuck").await;
    let mut reader = server.join("Reader").await;

    assert_eq!(reader.read_chunks(CHUNKS).await, CHUNKS);

    // 不读数据的客户端也回不了心跳，它的连接要在写 socket 卡住时照样超时。
    answer_keep_alives_until(&mut reader, server.wait_for_players(&["Reader"])).await;

    for _ in 0..3 {
        let keep_alive = reader
            .read_until_answering(play::clientbound::KEEP_ALIVE)
            .await;
        reader.answer_keep_alive(&keep_alive).await;
    }
    server.wait_for_players(&["Reader"]).await;
}

#[tokio::test]
async fn keep_alive_keeps_running_while_the_player_plays() {
    let server = TestServer::start(fast_keep_alive()).await;
    let mut client = server.join("Kitten").await;
    client.read_chunks(CHUNKS).await;
    client.send(&accept_teleportation(8.5, SPAWN_Y, 8.5)).await;
    client.send(&player_loaded()).await;

    let started = Instant::now();
    let mut rounds = 0;
    while started.elapsed() < Duration::from_millis(1200) {
        client.send(&move_player_pos(8.5, SPAWN_Y, 8.5)).await;
        client.send(&client_tick_end()).await;
        let keep_alive = client
            .read_until_answering(play::clientbound::KEEP_ALIVE)
            .await;
        client.answer_keep_alive(&keep_alive).await;
        rounds += 1;
    }

    assert!(rounds >= 3, "only {rounds} keep-alive rounds");
    server.wait_for_players(&["Kitten"]).await;
}

#[tokio::test]
async fn ping_request_is_answered_right_away() {
    let server = TestServer::start(small_world()).await;
    let mut client = server.join("Kitten").await;
    client.read_chunks(CHUNKS).await;

    let mut ping = vec![play::serverbound::PING_REQUEST as u8];
    ping.extend_from_slice(&0x0123_4567_89ab_cdefi64.to_be_bytes());
    client.send(&frame(&ping)).await;

    let pong = client.read_until(play::clientbound::PONG_RESPONSE).await;
    let mut reader = Reader::new(&pong);
    reader.varint().unwrap();
    assert_eq!(reader.i64(), Ok(0x0123_4567_89ab_cdef));
}

#[tokio::test]
async fn shutdown_gives_every_player_the_translated_reason() {
    let mut server = TestServer::start(small_world()).await;
    let mut first = server.join("Alice").await;
    let mut second = server.join("Bob").await;
    first.read_chunks(CHUNKS).await;
    second.read_chunks(CHUNKS).await;

    let shutdown = async {
        server.shutdown().await;
    };
    let read = async {
        for client in [&mut first, &mut second] {
            let disconnect = client.read_until(play::clientbound::DISCONNECT).await;
            assert_eq!(
                disconnect_body(&disconnect),
                translated("multiplayer.disconnect.server_shutdown")
            );
            client.assert_closed().await;
        }
    };
    tokio::join!(shutdown, read);
}

#[tokio::test]
async fn shutdown_does_not_wait_for_a_client_that_never_reads() {
    let mut server = TestServer::start(world_for_stuck_clients()).await;
    let _stuck = server.join_without_reading("Stuck").await;

    server.shutdown().await;
}

#[tokio::test]
async fn client_leaving_in_the_middle_of_the_join_leaves_no_ghost() {
    let server = TestServer::start(small_world()).await;
    let mut watcher = server.join("Watcher").await;
    watcher.read_chunks(CHUNKS).await;

    // 刚发完 finish_configuration 就走了，世界线程可能还没处理这次进服。
    let leaving = server.join("Ghost").await;
    drop(leaving);

    server.wait_for_players(&["Watcher"]).await;

    let removal = watcher
        .read_until_answering(play::clientbound::PLAYER_INFO_REMOVE)
        .await;
    assert_eq!(packet_id(&removal), play::clientbound::PLAYER_INFO_REMOVE);

    // 同名玩家可以马上回来。
    let mut returning = server.join("Ghost").await;
    assert_eq!(returning.read_chunks(CHUNKS).await, CHUNKS);
    server.wait_for_players(&["Ghost", "Watcher"]).await;
}

#[tokio::test]
async fn client_leaving_during_configuration_is_never_announced() {
    let server = TestServer::start(small_world()).await;
    let mut watcher = server.join("Watcher").await;
    watcher.read_chunks(CHUNKS).await;

    let mut leaving = server.connect().await;
    leaving.configure("Ghost").await;
    server.wait_for_players(&["Ghost", "Watcher"]).await;
    drop(leaving);
    server.wait_for_players(&["Watcher"]).await;

    // 世界线程从没见过这名玩家，所以其他人什么都收不到。
    let mut ping = vec![play::serverbound::PING_REQUEST as u8];
    ping.extend_from_slice(&1i64.to_be_bytes());
    watcher.send(&frame(&ping)).await;
    let received = timeout(TEST_DEADLINE, async {
        let mut ids = Vec::new();
        loop {
            let id = packet_id(&watcher.read_frame().await);
            if id == play::clientbound::PONG_RESPONSE {
                return ids;
            }
            ids.push(id);
        }
    })
    .await
    .expect("pong should arrive");
    assert!(!received.contains(&play::clientbound::SYSTEM_CHAT));
    assert!(!received.contains(&play::clientbound::PLAYER_INFO_REMOVE));
}

#[tokio::test]
async fn malformed_packet_closes_only_that_connection() {
    let server = TestServer::start(small_world()).await;
    let mut watcher = server.join("Watcher").await;
    let mut broken = server.join("Broken").await;
    watcher.read_chunks(CHUNKS).await;
    broken.read_chunks(CHUNKS).await;

    // `move_player_pos` 少了内容，解码失败。
    broken
        .send(&frame(&[play::serverbound::MOVE_PLAYER_POS as u8, 0x00]))
        .await;

    broken.assert_closed_after_reading().await;
    server.wait_for_players(&["Watcher"]).await;

    // 这名玩家的加入消息可能已经被上面的 read_chunks 读掉了。
    while !contains(
        &read_system_chat(&mut watcher).await,
        b"multiplayer.player.left",
    ) {}
}

#[tokio::test]
async fn nan_position_is_kicked_with_the_official_reason() {
    let server = TestServer::start(small_world()).await;
    let mut client = server.join("Kitten").await;
    client.read_chunks(CHUNKS).await;

    client.send(&move_player_pos(f64::NAN, 0.0, 0.0)).await;

    let disconnect = client
        .read_until_answering(play::clientbound::DISCONNECT)
        .await;
    assert_eq!(
        disconnect_body(&disconnect),
        translated("multiplayer.disconnect.invalid_player_movement")
    );
    client.assert_closed().await;
    server.wait_for_players(&[]).await;
}

#[tokio::test]
async fn walking_over_a_chunk_border_moves_the_view() {
    let server = TestServer::start(small_world()).await;
    let mut client = server.join("Kitten").await;
    client.read_chunks(CHUNKS).await;
    client.send(&accept_teleportation(8.5, SPAWN_Y, 8.5)).await;
    client.send(&player_loaded()).await;

    client.send(&move_player_pos(16.5, SPAWN_Y, 8.5)).await;
    client.send(&client_tick_end()).await;

    let center = client
        .read_until_answering(play::clientbound::SET_CHUNK_CACHE_CENTER)
        .await;
    assert_eq!(center[1..], [0x01, 0x00]);

    let mut forgotten = 0;
    let mut entered = BTreeSet::new();
    let mut announced = 0;
    while announced < 7 {
        let payload = client.read_frame().await;
        let mut reader = Reader::new(&payload);
        match reader.varint().unwrap() {
            play::clientbound::FORGET_LEVEL_CHUNK => {
                assert!(entered.is_empty(), "forget must come before new chunks");
                forgotten += 1;
            }
            play::clientbound::LEVEL_CHUNK_WITH_LIGHT => {
                entered.insert((reader.i32().unwrap(), reader.i32().unwrap()));
            }
            play::clientbound::CHUNK_BATCH_FINISHED => {
                announced += reader.varint().unwrap();
                client.send(&common::chunk_batch_received(64.0)).await;
            }
            play::clientbound::KEEP_ALIVE => client.answer_keep_alive(&payload).await,
            _ => {}
        }
    }

    assert_eq!(forgotten, 7);
    let expected: BTreeSet<(i32, i32)> = (-3..=3).map(|z| (4, z)).collect();
    assert_eq!(entered, expected);
}

#[tokio::test]
async fn client_information_sets_the_view_distance_before_the_join() {
    let config = ServerConfig {
        view_distance: 8,
        ..ServerConfig::default()
    };
    let server = TestServer::start(config).await;
    let mut client = server.connect().await;

    client
        .configure_with(&client_information(3), "Kitten")
        .await;
    client.finish_configuration().await;

    assert_eq!(client.read_chunks(81).await, 81);
}
