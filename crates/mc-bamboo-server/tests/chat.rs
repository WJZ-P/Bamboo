//! 聊天、命令、统计和加入离开消息。
//!
//! 期望值全部来自 `chat-join.md` 第 2 到 8 节，不看实现。

mod common;

use std::time::Duration;

use common::{
    TestServer, frame,
    player::{Player, QUIET},
    wire::{
        AwardStats, Disconnect, DisguisedChat, Nbt, PlayerInfoRemove, PlayerInfoUpdate, SystemChat,
        chat, chat_command, client_command,
    },
};
use mc_bamboo_data::packets::play::{clientbound, serverbound};
use mc_bamboo_server::ServerConfig;
use tokio::time::timeout;

/// `chat_type` 注册表里 `minecraft:chat` 的线上值：ID 加一。
const CHAT_TYPE: i32 = 1;
const REQUEST_STATS: i32 = 1;
const JOINED: &str = "multiplayer.player.joined";
const LEFT: &str = "multiplayer.player.left";
const UNKNOWN_COMMAND: &str = "command.unknown.command";
const ILLEGAL_CHARACTERS: &str = "multiplayer.disconnect.illegal_characters";
const SPAM: &str = "disconnect.spam";
/// `String(256)`。
const MAX_CHAT_CHARS: usize = 256;
/// 一口气连发这么多条还不算刷屏，再多一条就踢：`chat-join.md` 第 4 节，每条加 20，阈值 200。
const MOST_IN_A_BURST: usize = 9;

fn small_world() -> ServerConfig {
    ServerConfig {
        view_distance: 2,
        ..ServerConfig::default()
    }
}

/// 两名玩家都进了世界，从现在开始检查。
async fn two_players(server: &TestServer) -> (Player, Player) {
    let mut alice = server.enter("Alice").await;
    let mut bob = server.enter("Bob").await;
    alice.skip_received();
    bob.skip_received();
    (alice, bob)
}

fn system_message_with(key: &'static str) -> impl Fn(&SystemChat) -> bool {
    move |packet: &SystemChat| packet.content.translate_keys().contains(&key)
}

/// 断言这是 `{translate:key,with:[name],color:"yellow"}`，显示在聊天栏。
fn assert_player_message(packet: &SystemChat, key: &str, name: &str) {
    assert!(!packet.overlay);
    let content = &packet.content;
    assert_eq!(content.translate_keys(), [key]);
    assert_eq!(content.color_of(key), Some("yellow"));
    let args = content
        .get("with")
        .and_then(Nbt::as_list)
        .expect("the player name is a translation argument");
    assert_eq!(args.len(), 1);
    assert_eq!(args[0].text(), Some(name));
}

fn chat_burst(messages: usize) -> Vec<u8> {
    let mut burst = Vec::new();
    for index in 0..messages {
        burst.extend_from_slice(&chat(&format!("message {index}")));
    }
    burst
}

// ---- 聊天 ----

#[tokio::test]
async fn chat_reaches_everyone_including_the_sender() {
    let server = TestServer::start(small_world()).await;
    let (mut alice, mut bob) = two_players(&server).await;

    alice.send(&chat("hello")).await;

    for viewer in [&mut alice, &mut bob] {
        let message = viewer.expect::<DisguisedChat>(|_| true).await;
        assert_eq!(message.message.as_str(), Some("hello"));
        assert_eq!(message.chat_type, CHAT_TYPE);
        assert_eq!(message.name.as_str(), Some("Alice"));
        assert_eq!(message.target, None);
    }
}

#[tokio::test]
async fn chat_frame_matches_the_official_bytes() {
    let server = TestServer::start(small_world()).await;
    let mut kitten = server.enter("Kitten").await;
    kitten.skip_received();
    let mark = kitten.mark();

    kitten.send(&chat("hi")).await;
    kitten.expect::<DisguisedChat>(|_| true).await;

    // `chat-join.md` 第 5 节的帧去掉帧长和包 ID。
    let frames = kitten.frames();
    let body = &frames[mark..]
        .iter()
        .find(|frame| frame.id == clientbound::DISGUISED_CHAT)
        .expect("just received it")
        .body;
    let expected = [
        0x08, 0x00, 0x02, 0x68, 0x69, 0x01, 0x08, 0x00, 0x06, 0x4b, 0x69, 0x74, 0x74, 0x65, 0x6e,
        0x00,
    ];
    assert_eq!(body, &expected);
}

#[tokio::test]
async fn the_official_chat_packet_is_accepted_byte_for_byte() {
    let server = TestServer::start(small_world()).await;
    let mut alice = server.enter("Alice").await;
    alice.skip_received();

    // `chat-join.md` 第 4 节，离线客户端发送 `hi` 的 25 字节。
    let mut packet = vec![serverbound::CHAT as u8];
    packet.extend_from_slice(&[
        0x02, 0x68, 0x69, 0x00, 0x00, 0x01, 0x8b, 0xcf, 0xe5, 0x68, 0x00, 0x11, 0x22, 0x33, 0x44,
        0x55, 0x66, 0x77, 0x88, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
    ]);
    alice.send(&frame(&packet)).await;

    let message = alice.expect::<DisguisedChat>(|_| true).await;
    assert_eq!(message.message.as_str(), Some("hi"));
}

#[tokio::test]
async fn chinese_and_emoji_survive_the_round_trip() {
    let server = TestServer::start(small_world()).await;
    let (alice, mut bob) = two_players(&server).await;
    let text = "竹子说你好🐾🎋";

    alice.send(&chat(text)).await;

    let message = bob.expect::<DisguisedChat>(|_| true).await;
    assert_eq!(message.message.as_str(), Some(text));
}

#[tokio::test]
async fn the_longest_message_goes_through() {
    let server = TestServer::start(small_world()).await;
    let (alice, mut bob) = two_players(&server).await;
    let text = "竹".repeat(MAX_CHAT_CHARS);

    alice.send(&chat(&text)).await;

    let message = bob.expect::<DisguisedChat>(|_| true).await;
    assert_eq!(message.message.as_str(), Some(text.as_str()));
}

#[tokio::test]
async fn a_message_above_the_limit_only_kicks_its_sender() {
    let server = TestServer::start(small_world()).await;
    let (alice, mut bob) = two_players(&server).await;
    let mark = bob.mark();

    alice.send(&chat(&"x".repeat(MAX_CHAT_CHARS + 1))).await;

    alice.wait_closed().await;
    server.wait_for_players(&["Bob"]).await;
    bob.expect_none::<DisguisedChat>(mark, |_| true).await;
    bob.assert_world_ticks().await;
}

#[tokio::test]
async fn chat_keeps_the_order_of_each_sender() {
    let server = TestServer::start(small_world()).await;
    let (alice, mut bob) = two_players(&server).await;

    alice.send(&chat_burst(MOST_IN_A_BURST)).await;

    for index in 0..MOST_IN_A_BURST {
        let expected = format!("message {index}");
        let message = bob.expect::<DisguisedChat>(|_| true).await;
        assert_eq!(message.message.as_str(), Some(expected.as_str()));
    }
}

#[tokio::test]
async fn a_flood_disconnects_the_sender_after_the_message_that_reaches_the_limit() {
    let server = TestServer::start(small_world()).await;
    let (mut alice, mut bob) = two_players(&server).await;
    let mark = bob.mark();

    alice.send(&chat_burst(MOST_IN_A_BURST + 5)).await;

    let kicked = alice.expect::<Disconnect>(|_| true).await;
    assert_eq!(kicked.reason.translate_keys(), [SPAM]);
    alice.wait_closed().await;
    server.wait_for_players(&["Bob"]).await;
    bob.assert_world_ticks().await;
    assert_eq!(bob.since::<DisguisedChat>(mark).len(), MOST_IN_A_BURST + 1);
}

#[tokio::test]
async fn every_player_sees_chat_from_several_senders_in_the_same_order() {
    let server = TestServer::start(small_world()).await;
    let mut alice = server.enter("Alice").await;
    let mut bob = server.enter("Bob").await;
    let mut carol = server.enter("Carol").await;
    alice.skip_received();
    bob.skip_received();
    carol.skip_received();
    let marks = [alice.mark(), bob.mark(), carol.mark()];

    for index in 0..MOST_IN_A_BURST {
        let (first, second, third) = (
            chat(&format!("a{index}")),
            chat(&format!("b{index}")),
            chat(&format!("c{index}")),
        );
        tokio::join!(alice.send(&first), bob.send(&second), carol.send(&third));
    }

    for viewer in [&alice, &bob, &carol] {
        viewer
            .wait_until("every message", |frames| {
                frames
                    .iter()
                    .filter(|frame| frame.id == clientbound::DISGUISED_CHAT)
                    .count()
                    >= 3 * MOST_IN_A_BURST
            })
            .await;
    }
    let order = |player: &Player, mark: usize| -> Vec<String> {
        player
            .since::<DisguisedChat>(mark)
            .iter()
            .filter_map(|packet| packet.message.as_str().map(str::to_owned))
            .collect()
    };
    let seen_by_alice = order(&alice, marks[0]);
    assert_eq!(seen_by_alice.len(), 3 * MOST_IN_A_BURST);
    assert_eq!(order(&bob, marks[1]), seen_by_alice);
    assert_eq!(order(&carol, marks[2]), seen_by_alice);
}

#[tokio::test]
async fn illegal_characters_disconnect_the_sender_with_the_translated_reason() {
    for (name, text) in [
        ("Section", "a\u{a7}b"),
        ("Control", "a\u{7}b"),
        ("Delete", "a\u{7f}b"),
    ] {
        let server = TestServer::start(small_world()).await;
        let mut sender = server.enter(name).await;
        let mut bob = server.enter("Bob").await;
        sender.skip_received();
        let mark = bob.mark();

        sender.send(&chat(text)).await;

        let kicked = sender.expect::<Disconnect>(|_| true).await;
        assert_eq!(kicked.reason.translate_keys(), [ILLEGAL_CHARACTERS]);
        sender.wait_closed().await;
        server.wait_for_players(&["Bob"]).await;
        bob.expect_none::<DisguisedChat>(mark, |_| true).await;
        bob.assert_world_ticks().await;
    }
}

#[tokio::test]
async fn illegal_characters_in_a_command_disconnect_too() {
    let server = TestServer::start(small_world()).await;
    let mut alice = server.enter("Alice").await;
    alice.skip_received();

    alice.send(&chat_command("foo \u{a7}c")).await;

    let kicked = alice.expect::<Disconnect>(|_| true).await;
    assert_eq!(kicked.reason.translate_keys(), [ILLEGAL_CHARACTERS]);
}

#[tokio::test]
async fn a_connection_still_configuring_gets_no_play_packets() {
    let server = TestServer::start(small_world()).await;
    let alice = server.enter("Alice").await;
    let mut carol = server.connect().await;
    carol.configure("Carol").await;

    alice.send(&chat("hello")).await;
    let silence = timeout(QUIET, carol.read_frame()).await;
    assert!(
        silence.is_err(),
        "a Configuration connection parses Play packets with the wrong ids"
    );

    carol.finish_configuration().await;
    let first = carol.read_frame().await;
    assert_eq!(common::packet_id(&first), clientbound::LOGIN);
}

// ---- 命令 ----

#[tokio::test]
async fn an_unknown_command_gets_a_red_error_only_for_its_sender() {
    let server = TestServer::start(small_world()).await;
    let (mut alice, bob) = two_players(&server).await;
    let mark = bob.mark();
    let own_mark = alice.mark();

    alice.send(&chat_command("foo bar")).await;

    let error = alice
        .expect::<SystemChat>(system_message_with(UNKNOWN_COMMAND))
        .await;
    assert!(!error.overlay);
    assert_eq!(error.content.color_of(UNKNOWN_COMMAND), Some("red"));

    // 规格允许只回第一条。回第二条时内容是命令文本和 `<--[HERE]`。
    alice.settle().await;
    let messages = alice.since::<SystemChat>(own_mark);
    assert!(messages.len() <= 2);
    if let Some(context) = messages.get(1) {
        assert!(
            format!("{:?}", context.content).contains("foo bar"),
            "the second line repeats the command"
        );
    }
    bob.expect_none::<SystemChat>(mark, |_| true).await;
    alice.expect_none::<DisguisedChat>(own_mark, |_| true).await;
}

#[tokio::test]
async fn an_empty_command_gets_the_same_error() {
    let server = TestServer::start(small_world()).await;
    let mut alice = server.enter("Alice").await;
    alice.skip_received();

    alice.send(&chat_command("")).await;

    alice
        .expect::<SystemChat>(system_message_with(UNKNOWN_COMMAND))
        .await;
}

#[tokio::test]
async fn the_official_command_packet_is_accepted_byte_for_byte() {
    let server = TestServer::start(small_world()).await;
    let mut alice = server.enter("Alice").await;
    alice.skip_received();

    // `chat-join.md` 第 4 节：`/foo bar`。
    let mut packet = vec![serverbound::CHAT_COMMAND as u8];
    packet.extend_from_slice(&[0x07, 0x66, 0x6f, 0x6f, 0x20, 0x62, 0x61, 0x72]);
    alice.send(&frame(&packet)).await;

    alice
        .expect::<SystemChat>(system_message_with(UNKNOWN_COMMAND))
        .await;
}

// ---- 统计 ----

#[tokio::test]
async fn requesting_stats_gets_an_empty_answer_only_for_the_requester() {
    let server = TestServer::start(small_world()).await;
    let (mut alice, bob) = two_players(&server).await;
    let mark = bob.mark();

    alice.send(&client_command(REQUEST_STATS)).await;

    let stats = alice.expect::<AwardStats>(|_| true).await;
    assert_eq!(stats.count, 0);
    bob.expect_none::<AwardStats>(mark, |_| true).await;
}

#[tokio::test]
async fn the_other_client_commands_are_ignored() {
    let server = TestServer::start(small_world()).await;
    let mut alice = server.enter("Alice").await;
    let mark = alice.mark();

    alice.send(&client_command(0)).await;
    alice.send(&client_command(2)).await;

    alice.expect_none::<AwardStats>(mark, |_| true).await;
    alice.assert_world_ticks().await;
}

// ---- 加入和离开 ----

#[tokio::test]
async fn others_see_a_yellow_join_message_and_the_new_player_does_not() {
    let server = TestServer::start(small_world()).await;
    let mut alice = server.enter("Alice").await;
    alice.skip_received();

    let bob = server.enter("Bob").await;

    let joined = alice
        .expect::<SystemChat>(system_message_with(JOINED))
        .await;
    assert_player_message(&joined, JOINED, "Bob");
    bob.settle().await;
    assert!(
        bob.since::<SystemChat>(0).is_empty(),
        "the new player is not on the list yet when the message goes out"
    );
}

#[tokio::test]
async fn the_first_player_gets_no_join_message() {
    let server = TestServer::start(small_world()).await;
    let alice = server.enter("Alice").await;

    alice.settle().await;

    assert!(alice.since::<SystemChat>(0).is_empty());
}

#[tokio::test]
async fn the_join_message_comes_after_the_tab_entry() {
    let server = TestServer::start(small_world()).await;
    let mut alice = server.enter("Alice").await;
    alice.skip_received();

    let bob = server.enter("Bob").await;

    alice
        .expect::<PlayerInfoUpdate>(|packet| packet.adds(bob.uuid))
        .await;
    alice
        .expect::<SystemChat>(system_message_with(JOINED))
        .await;
}

#[tokio::test]
async fn others_see_a_yellow_left_message_before_the_tab_entry_goes() {
    let server = TestServer::start(small_world()).await;
    let (mut alice, bob) = two_players(&server).await;
    let uuid = bob.uuid;

    drop(bob);

    let left = alice.expect::<SystemChat>(system_message_with(LEFT)).await;
    assert_player_message(&left, LEFT, "Bob");
    alice
        .expect::<PlayerInfoRemove>(|packet| packet.uuids == [uuid])
        .await;
}

#[tokio::test]
async fn a_connection_lost_in_configuration_is_not_announced() {
    let server = TestServer::start(small_world()).await;
    let mut alice = server.enter("Alice").await;
    alice.skip_received();
    let mark = alice.mark();

    let mut ghost = server.connect().await;
    ghost.configure("Ghost").await;
    drop(ghost);
    server.wait_for_players(&["Alice"]).await;

    alice.expect_none::<SystemChat>(mark, |_| true).await;
    alice.expect_none::<PlayerInfoRemove>(mark, |_| true).await;
}

#[tokio::test]
async fn chat_from_a_player_who_just_left_is_still_delivered_in_order() {
    let server = TestServer::start(small_world()).await;
    let (alice, mut bob) = two_players(&server).await;

    alice.send(&chat("bye")).await;
    tokio::time::sleep(Duration::from_millis(10)).await;
    drop(alice);

    let message = bob.expect::<DisguisedChat>(|_| true).await;
    assert_eq!(message.message.as_str(), Some("bye"));
    bob.expect::<SystemChat>(system_message_with(LEFT)).await;
}
