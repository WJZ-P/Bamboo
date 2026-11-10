//! 完整的进服流程，以及它的各种异常分支。

mod common;

use std::time::Duration;

use common::{
    LOGIN_INTENT, STATUS_INTENT, TEST_DEADLINE, TestServer, disconnect_body, frame, handshake,
    handshake_with_protocol, hello, json_packet, packet_id, status_request, translated,
};
use mc_bamboo_data::packets::{configuration, login, play};
use mc_bamboo_protocol::codec::{Reader, write_varint};
use mc_bamboo_server::{PlayerInfo, ServerConfig, ServerEvent};
use tokio::time::{sleep, timeout};
use uuid::Uuid;

/// 配置写 1 会被修正成官方的下限 2，所以实际是 `T(2)` 的 49 个区块，测试跑得快。
const VIEW_DISTANCE: u8 = 1;
const CHUNKS: usize = 49;

fn small_world() -> ServerConfig {
    ServerConfig {
        view_distance: VIEW_DISTANCE,
        ..ServerConfig::default()
    }
}

/// 等服务端处理完断开，玩家表的更新发生在连接任务结束时。
async fn wait_until_empty(server: &TestServer) {
    timeout(TEST_DEADLINE, async {
        while !server.handle.online_players().is_empty() {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("player stayed in the player list");
}

/// 读取 Configuration 状态的断开原因。原因是 NBT 字符串：类型、两字节长度、内容。
fn disconnect_reason(payload: &[u8]) -> String {
    let mut reader = Reader::new(payload);
    reader.varint().unwrap();
    assert_eq!(reader.u8(), Ok(8), "reason should be an NBT string");
    let len = usize::from(reader.u16().unwrap());
    let text = reader.rest();
    assert_eq!(text.len(), len);
    String::from_utf8(text.to_vec()).unwrap()
}

#[tokio::test]
async fn player_joins_and_receives_the_world() {
    let server = TestServer::start(small_world()).await;
    let mut client = server.join("Kitten").await;

    let login_packet = client.read_frame().await;
    assert_eq!(packet_id(&login_packet), play::clientbound::LOGIN);

    assert_eq!(client.read_chunks(CHUNKS).await, CHUNKS);
    let players = server.handle.online_players();
    assert_eq!(players.len(), 1);
    assert_eq!(players[0].name, "Kitten");
}

/// 默认配置和官方一样：登录时收到阈值 256，之后两个方向都是压缩帧。
#[tokio::test]
async fn the_server_turns_compression_on_by_default() {
    let server = TestServer::start(small_world()).await;
    let mut client = server.join("Kitten").await;

    assert_eq!(client.compression_threshold(), Some(256));
    // 登录之后的大包都走压缩，假客户端解不开就会在这里失败。
    assert_eq!(
        packet_id(&client.read_frame().await),
        play::clientbound::LOGIN
    );
    assert_eq!(client.read_chunks(CHUNKS).await, CHUNKS);
}

/// 负的阈值关掉压缩，客户端照常能进服。
#[tokio::test]
async fn players_can_still_join_with_compression_off() {
    let config = ServerConfig {
        compression_threshold: -1,
        ..small_world()
    };
    let server = TestServer::start(config).await;
    let mut client = server.join("Kitten").await;

    assert_eq!(client.compression_threshold(), None);
    assert_eq!(
        packet_id(&client.read_frame().await),
        play::clientbound::LOGIN
    );
    assert_eq!(client.read_chunks(CHUNKS).await, CHUNKS);
}

#[tokio::test]
async fn join_order_matches_the_official_server() {
    let server = TestServer::start(small_world()).await;
    let mut client = server.join("Kitten").await;

    let mut ids = Vec::new();
    loop {
        let id = packet_id(&client.read_frame().await);
        ids.push(id);
        if id == play::clientbound::SET_CHUNK_CACHE_CENTER {
            break;
        }
    }

    assert_eq!(
        ids,
        [
            play::clientbound::LOGIN,
            play::clientbound::CHANGE_DIFFICULTY,
            play::clientbound::PLAYER_ABILITIES,
            play::clientbound::ENTITY_EVENT,
            play::clientbound::COMMANDS,
            play::clientbound::PLAYER_POSITION,
            play::clientbound::PLAYER_INFO_UPDATE,
            play::clientbound::SET_ENTITY_DATA,
            play::clientbound::UPDATE_ATTRIBUTES,
            play::clientbound::SET_TIME,
            play::clientbound::SET_DEFAULT_SPAWN_POSITION,
            play::clientbound::GAME_EVENT,
            play::clientbound::SET_CHUNK_CACHE_CENTER,
        ]
    );
}

#[tokio::test]
async fn events_report_players_coming_and_going() {
    let server = TestServer::start(small_world()).await;
    let mut events = server.handle.subscribe();

    let client = server.join("Kitten").await;
    let joined = timeout(TEST_DEADLINE, events.recv())
        .await
        .unwrap()
        .unwrap();
    let ServerEvent::PlayerJoined(PlayerInfo { name, .. }) = joined else {
        panic!("expected a join event, got {joined:?}");
    };
    assert_eq!(name, "Kitten");

    drop(client);
    let left = timeout(TEST_DEADLINE, events.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(left, ServerEvent::PlayerLeft(_)));
}

#[tokio::test]
async fn status_follows_the_online_count() {
    let server = TestServer::start(small_world()).await;
    let client = server.join("Kitten").await;

    let online = async || {
        let mut pinger = server.connect().await;
        let mut opening = handshake(STATUS_INTENT);
        opening.extend(status_request());
        pinger.send(&opening).await;
        let (_, status) = json_packet(&pinger.read_frame().await);
        status["players"]["online"].as_i64().unwrap()
    };

    assert_eq!(online().await, 1);
    drop(client);
    wait_until_empty(&server).await;
    assert_eq!(online().await, 0);
}

#[tokio::test]
async fn keep_alive_keeps_the_player_connected() {
    let config = ServerConfig {
        keep_alive_interval: Duration::from_millis(100),
        ..small_world()
    };
    let server = TestServer::start(config).await;
    let mut client = server.join("Kitten").await;
    client.read_chunks(CHUNKS).await;

    for _ in 0..3 {
        let keep_alive = client.read_until(play::clientbound::KEEP_ALIVE).await;
        client.answer_keep_alive(&keep_alive).await;
    }

    assert_eq!(server.handle.online_players().len(), 1);
}

#[tokio::test]
async fn silent_player_is_disconnected_with_a_reason() {
    let config = ServerConfig {
        keep_alive_interval: Duration::from_millis(100),
        ..small_world()
    };
    let server = TestServer::start(config).await;
    let mut client = server.join("Kitten").await;
    client.read_chunks(CHUNKS).await;

    let disconnect = client.read_until(play::clientbound::DISCONNECT).await;

    assert_eq!(
        disconnect_body(&disconnect),
        translated("disconnect.timeout")
    );
    client.assert_closed().await;
    wait_until_empty(&server).await;
}

#[tokio::test]
async fn wrong_keep_alive_answer_disconnects_with_the_timeout_reason() {
    let config = ServerConfig {
        keep_alive_interval: Duration::from_millis(100),
        ..small_world()
    };
    let server = TestServer::start(config).await;
    let mut client = server.join("Kitten").await;
    client.read_chunks(CHUNKS).await;

    client.read_until(play::clientbound::KEEP_ALIVE).await;
    let mut answer = vec![play::serverbound::KEEP_ALIVE as u8];
    answer.extend_from_slice(&(-1i64).to_be_bytes());
    client.send(&frame(&answer)).await;

    let disconnect = client.read_until(play::clientbound::DISCONNECT).await;
    assert_eq!(
        disconnect_body(&disconnect),
        translated("disconnect.timeout")
    );
    client.assert_closed().await;
}

/// 官方 `handleKeepAlive`：当前没在等心跳时，任何 id 都是 `disconnect.timeout`。
///
/// Official `handleKeepAlive`: any id disconnects with `disconnect.timeout` when no
/// challenge is pending.
#[tokio::test]
async fn unsolicited_keep_alive_disconnects_with_the_timeout_reason() {
    let config = ServerConfig {
        // 第一跳在这一跳发出之前，回应都算“没在等”。读完区块用不到这么久。
        // The first challenge has not been sent yet, so a reply is unsolicited.
        // Reading the chunks finishes well inside this interval.
        keep_alive_interval: Duration::from_secs(20),
        ..small_world()
    };
    let server = TestServer::start(config).await;
    let mut client = server.join("Kitten").await;
    client.read_chunks(CHUNKS).await;

    // 服务端的第一跳 id 是 1。如果其实已经在等，这个回应会被接受，测试就会失败。
    // The first challenge id is 1. If one is already pending, this reply is accepted and the test fails.
    let mut answer = vec![play::serverbound::KEEP_ALIVE as u8];
    answer.extend_from_slice(&1i64.to_be_bytes());
    client.send(&frame(&answer)).await;

    let disconnect = client.read_until(play::clientbound::DISCONNECT).await;
    assert_eq!(
        disconnect_body(&disconnect),
        translated("disconnect.timeout")
    );
    client.assert_closed().await;
}

#[tokio::test]
async fn unknown_play_packets_are_ignored() {
    let config = ServerConfig {
        keep_alive_interval: Duration::from_millis(100),
        ..small_world()
    };
    let server = TestServer::start(config).await;
    let mut client = server.join("Kitten").await;
    client.read_chunks(CHUNKS).await;

    client
        .send(&frame(&[play::serverbound::CLIENT_TICK_END as u8]))
        .await;
    client
        .send(&frame(&[play::serverbound::PLAYER_LOADED as u8]))
        .await;

    let keep_alive = client.read_until(play::clientbound::KEEP_ALIVE).await;
    assert_eq!(packet_id(&keep_alive), play::clientbound::KEEP_ALIVE);
}

/// 满员时普通玩家被拒、`bypassesPlayerLimit` 为真的 OP 能进、为假的 OP 不能进。
async fn full_server_case(ops: &str, name: &str, bypasses: bool) {
    let lists = std::env::temp_dir().join(format!("bamboo-full-ops-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&lists).unwrap();
    std::fs::write(lists.join("ops.json"), ops).unwrap();
    let config = ServerConfig {
        max_players: 1,
        view_distance: VIEW_DISTANCE,
        list_directory: lists.clone(),
        ..ServerConfig::default()
    };
    let server = TestServer::start(config).await;
    let _first = server.join("Kitten").await;

    let mut second = server.connect().await;
    second.send(&handshake(LOGIN_INTENT)).await;
    second.send(&hello(name)).await;
    let payload = second.read_frame().await;
    if bypasses {
        assert_eq!(packet_id(&payload), login::clientbound::LOGIN_FINISHED);
        // 连接层放行还不够，权威名单复查也得放行：走完配置进到 Play 才算真的进服。
        second
            .send(&frame(&[login::serverbound::LOGIN_ACKNOWLEDGED as u8]))
            .await;
        second
            .read_until(configuration::clientbound::SELECT_KNOWN_PACKS)
            .await;
        second.send(&common::official_known_packs()).await;
        second
            .read_until(configuration::clientbound::FINISH_CONFIGURATION)
            .await;
        second.finish_configuration().await;
        let login = second.read_until(play::clientbound::LOGIN).await;
        assert_eq!(packet_id(&login), play::clientbound::LOGIN);
    } else {
        let (id, reason) = json_packet(&payload);
        assert_eq!(id, login::clientbound::LOGIN_DISCONNECT);
        assert_eq!(reason["translate"], "multiplayer.disconnect.server_full");
    }
    let _ = std::fs::remove_dir_all(&lists);
}

#[tokio::test]
async fn full_server_refuses_with_a_message() {
    full_server_case("[]", "Bamboo", false).await;
}

#[tokio::test]
async fn an_op_with_bypasses_player_limit_gets_into_a_full_server() {
    let uuid = common::player::offline_uuid("Bypass");
    let ops =
        format!(r#"[{{"uuid":"{uuid}","name":"Bypass","level":4,"bypassesPlayerLimit":true}}]"#);
    full_server_case(&ops, "Bypass", true).await;
}

#[tokio::test]
async fn an_op_without_bypasses_player_limit_is_refused_by_a_full_server() {
    let uuid = common::player::offline_uuid("Plain");
    let ops =
        format!(r#"[{{"uuid":"{uuid}","name":"Plain","level":4,"bypassesPlayerLimit":false}}]"#);
    full_server_case(&ops, "Plain", false).await;
}

/// 官方 `ServerLoginPacketListenerImpl.tick` 在 600 刻（30 秒）时说 `slow_login`，
/// Bamboo 的进服时限到点时也说这句话。
#[tokio::test]
async fn a_slow_login_is_told_why_it_was_dropped() {
    let config = ServerConfig {
        pre_play_timeout: Duration::from_millis(200),
        ..small_world()
    };
    let server = TestServer::start(config).await;
    let mut client = server.connect().await;

    // 握手说了要登录，然后什么都不发：官方的 600 刻就是从这里开始算的。
    client.send(&handshake(LOGIN_INTENT)).await;

    let (id, reason) = json_packet(&client.read_frame().await);
    assert_eq!(id, login::clientbound::LOGIN_DISCONNECT);
    assert_eq!(reason["translate"], "multiplayer.disconnect.slow_login");
    client.assert_closed().await;
}

/// 还没说要登录的连接直接断开，官方的登录时限也管不到它。
#[tokio::test]
async fn a_connection_that_never_handshakes_is_dropped_silently() {
    let config = ServerConfig {
        pre_play_timeout: Duration::from_millis(200),
        ..small_world()
    };
    let server = TestServer::start(config).await;
    let mut client = server.connect().await;

    client.assert_closed().await;
}

#[tokio::test]
async fn invalid_name_closes_the_connection() {
    let server = TestServer::start(small_world()).await;
    let mut client = server.connect().await;

    client.send(&handshake(LOGIN_INTENT)).await;
    client.send(&hello("has space")).await;

    client.assert_closed().await;
    assert!(server.handle.online_players().is_empty());
}

#[tokio::test]
async fn other_protocol_versions_are_told_which_version_to_use() {
    let server = TestServer::start(small_world()).await;
    let mut client = server.connect().await;

    client
        .send(&handshake_with_protocol(776, LOGIN_INTENT))
        .await;
    // 登录包在服务端已经回应之后才到。未读的输入不能触发 reset，
    // 否则发给客户端的提示可能会被丢掉。
    sleep(Duration::from_millis(100)).await;
    client.send(&hello("Kitten")).await;

    let (id, reason) = json_packet(&client.read_frame().await);
    assert_eq!(id, login::clientbound::LOGIN_DISCONNECT);
    assert!(reason["text"].as_str().unwrap().contains("26.3"));
}

#[tokio::test]
async fn transfer_intent_is_refused() {
    let server = TestServer::start(small_world()).await;
    let mut client = server.connect().await;

    client.send(&handshake(3)).await;

    let (id, _) = json_packet(&client.read_frame().await);
    assert_eq!(id, login::clientbound::LOGIN_DISCONNECT);
}

#[tokio::test]
async fn client_without_the_official_packs_is_refused() {
    let server = TestServer::start(small_world()).await;
    let mut client = server.connect().await;
    client.send(&handshake(LOGIN_INTENT)).await;
    client.send(&hello("Kitten")).await;
    client.read_frame().await;
    client
        .send(&frame(&[login::serverbound::LOGIN_ACKNOWLEDGED as u8]))
        .await;
    client
        .read_until(configuration::clientbound::SELECT_KNOWN_PACKS)
        .await;

    let mut no_packs = vec![configuration::serverbound::SELECT_KNOWN_PACKS as u8];
    write_varint(&mut no_packs, 0);
    client.send(&frame(&no_packs)).await;

    let disconnect = client
        .read_until(configuration::clientbound::DISCONNECT)
        .await;
    assert!(disconnect_reason(&disconnect).contains("官方数据"));
    wait_until_empty(&server).await;
}

#[tokio::test]
async fn finishing_configuration_too_early_closes_the_connection() {
    let server = TestServer::start(small_world()).await;
    let mut client = server.connect().await;
    client.send(&handshake(LOGIN_INTENT)).await;
    client.send(&hello("Kitten")).await;
    client.read_frame().await;
    client
        .send(&frame(&[login::serverbound::LOGIN_ACKNOWLEDGED as u8]))
        .await;
    client
        .send(&frame(&[
            configuration::serverbound::FINISH_CONFIGURATION as u8
        ]))
        .await;

    wait_until_empty(&server).await;
}

#[tokio::test]
async fn shutdown_tells_players_why_they_are_leaving() {
    let mut server = TestServer::start(small_world()).await;
    let mut client = server.join("Kitten").await;
    client.read_chunks(CHUNKS).await;

    let shutdown = async {
        server.shutdown().await;
    };
    let read = async {
        let disconnect = client.read_until(play::clientbound::DISCONNECT).await;
        assert_eq!(
            disconnect_body(&disconnect),
            translated("multiplayer.disconnect.server_shutdown")
        );
        drop(client);
    };
    tokio::join!(shutdown, read);
}
