//! 用假的客户端驱动世界线程，检查发出去的字节。期望值用 NBT 手写，不经过 [`Component`]。
//!
//! 这里只看聊天相关的包，所以别的模块在同一 tick 多发什么都不影响结果。

use mc_bamboo_data::packets::play::{clientbound, serverbound};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::{
    codec::{DecodeError, write_string, write_varint},
    packet::play::{ClientInformation, PlayRequest},
};

use super::*;
use crate::world::tests::{Client, Frame, Harness, client_requesting, frames_with_id};

const MAX_COMMAND_UTF16_UNITS: usize = 32767;
/// 第 10 条才到 200，这么多条还不算刷屏。
const MOST_IN_A_BURST: usize = 9;

fn text(value: &str) -> Tag {
    Tag::String(value.to_owned())
}

fn compound<const N: usize>(entries: [(&str, Tag); N]) -> Tag {
    let entries = entries.map(|(key, value)| (key.to_owned(), value));
    Tag::Compound(entries.into())
}

fn network_bytes(tag: &Tag) -> Vec<u8> {
    let mut out = Vec::new();
    tag.write_network(&mut out);
    out
}

/// `disguised_chat` 的包体：内容和名字都是纯文本，`minecraft:chat`，没有 `targetName`。
fn chat_body(name: &str, message: &str) -> Vec<u8> {
    let mut body = network_bytes(&text(message));
    body.push(0x01);
    body.extend(network_bytes(&text(name)));
    body.push(0x00);
    body
}

/// `system_chat` 的包体：内容，然后是 `overlay=false`。
fn system_chat_body(content: &Tag) -> Vec<u8> {
    let mut body = network_bytes(content);
    body.push(0x00);
    body
}

fn unknown_command_line() -> Tag {
    let message = compound([("translate", text("command.unknown.command"))]);
    compound([
        ("text", text("")),
        ("color", text("red")),
        ("extra", Tag::List(vec![message])),
    ])
}

/// 第二行红字。`children` 是 `click_event` 所在组件的子组件。
fn context_line(suggested: &str, children: Vec<Tag>) -> Tag {
    let click_event = compound([
        ("action", text("suggest_command")),
        ("command", text(suggested)),
    ]);
    let context = compound([
        ("text", text("")),
        ("color", text("gray")),
        ("click_event", click_event),
        ("extra", Tag::List(children)),
    ]);
    compound([
        ("text", text("")),
        ("color", text("red")),
        ("extra", Tag::List(vec![context])),
    ])
}

fn here_marker() -> Tag {
    compound([
        ("translate", text("command.context.here")),
        ("color", text("red")),
        ("italic", Tag::Byte(1)),
    ])
}

fn context_line_echoing(command: &str) -> Tag {
    let echoed = compound([
        ("text", text(command)),
        ("color", text("red")),
        ("underlined", Tag::Byte(1)),
    ]);
    context_line(&format!("/{command}"), vec![echoed, here_marker()])
}

fn chat(message: &str) -> PlayRequest {
    PlayRequest::Chat {
        message: message.to_owned(),
    }
}

fn command(command: &str) -> PlayRequest {
    PlayRequest::ChatCommand {
        command: command.to_owned(),
    }
}

/// 离线客户端的 `chat` 包：没有签名，`lastSeen` 总是 `00 00 00 00 01`。
fn chat_payload(message: &str) -> Vec<u8> {
    let mut payload = Vec::new();
    write_varint(&mut payload, serverbound::CHAT);
    write_string(&mut payload, message);
    payload.extend_from_slice(&[0; 16]);
    payload.extend_from_slice(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x01]);
    payload
}

fn take_bodies(client: &Client, id: i32) -> Vec<Vec<u8>> {
    client
        .take_frames()
        .into_iter()
        .filter(|frame| frame.id == id)
        .map(|frame| frame.body)
        .collect()
}

/// 两名已经进入世界的玩家，收到的包都清空了。
fn alice_and_bob() -> (Harness, Client, Client) {
    let mut server = Harness::new(2);
    let alice = server.join_and_settle("Alice");
    let bob = server.join_and_settle("Bob");
    alice.take_frames();
    bob.take_frames();
    (server, alice, bob)
}

fn assert_kicked_for_illegal_characters(client: &Client) {
    let reason = compound([(
        "translate",
        text("multiplayer.disconnect.illegal_characters"),
    )]);
    assert_eq!(
        take_bodies(client, clientbound::DISCONNECT),
        [network_bytes(&reason)]
    );
    assert!(client.is_closed());
}

/// 这是这名玩家收到的最后一个包，连接也关了。
fn assert_kicked_for_spam(frames: &[Frame], client: &Client) {
    let reason = compound([("translate", text("disconnect.spam"))]);
    let last = frames.last().expect("至少有断开的包");
    assert_eq!(last.id, clientbound::DISCONNECT);
    assert_eq!(last.body, network_bytes(&reason));
    assert!(client.is_closed());
}

fn send_chats(server: &Harness, client: &Client, count: usize) {
    for index in 0..count {
        server.request(client, chat(&format!("message {index}")));
    }
}

fn send_commands(server: &Harness, client: &Client, count: usize) {
    for _ in 0..count {
        server.request(client, command("foo"));
    }
}

fn idle(server: &mut Harness, ticks: usize) {
    for _ in 0..ticks {
        server.tick();
    }
}

#[test]
fn chat_frame_matches_the_official_bytes() {
    let expected = [
        0x11, // 帧长
        0x21, // 包 ID
        0x08, 0x00, 0x02, 0x68, 0x69, // message "hi"
        0x01, // chatType：minecraft:chat
        0x08, 0x00, 0x06, 0x4b, 0x69, 0x74, 0x74, 0x65, 0x6e, // name "Kitten"
        0x00, // 没有 targetName
    ];
    assert_eq!(chat_frames("Kitten", "hi".to_owned()), expected);
}

#[test]
fn chat_reaches_everyone_in_the_world_including_the_sender() {
    let mut server = Harness::new(2);
    let kitten = server.join_and_settle("Kitten");
    let bob = server.join_and_settle("Bob");
    let carol = server.join_and_settle("Carol");
    for client in [&kitten, &bob, &carol] {
        client.take_frames();
    }

    server.request(&kitten, chat("hi"));
    server.tick();

    let expected = [
        0x08, 0x00, 0x02, 0x68, 0x69, 0x01, 0x08, 0x00, 0x06, 0x4b, 0x69, 0x74, 0x74, 0x65, 0x6e,
        0x00,
    ];
    for client in [&kitten, &bob, &carol] {
        assert_eq!(take_bodies(client, clientbound::DISGUISED_CHAT), [expected]);
        assert!(!client.is_closed());
    }
}

#[test]
fn the_official_chat_packet_goes_all_the_way_to_the_broadcast() {
    let (mut server, alice, bob) = alice_and_bob();
    let payload = [
        0x09, // 包 ID
        0x02, 0x68, 0x69, // message
        0x00, 0x00, 0x01, 0x8b, 0xcf, 0xe5, 0x68, 0x00, // timeStamp
        0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, // salt
        0x00, // 没有签名
        0x00, 0x00, 0x00, 0x00, 0x01, // lastSeen
    ];

    let request = PlayRequest::decode(&payload).expect("这是官方写出来的字节");
    server.request(&alice, request);
    server.tick();

    assert_eq!(
        take_bodies(&bob, clientbound::DISGUISED_CHAT),
        [chat_body("Alice", "hi")]
    );
}

#[test]
fn chinese_and_emoji_keep_their_bytes() {
    let (mut server, alice, bob) = alice_and_bob();

    server.request(&alice, chat("竹子🎋"));
    server.tick();

    // 竹和子各 3 个字节。🎋 是一个代理对，每半个 3 个字节。
    let mut expected = vec![0x08, 0x00, 0x0c];
    expected.extend([0xe7, 0xab, 0xb9, 0xe5, 0xad, 0x90]);
    expected.extend([0xed, 0xa0, 0xbc, 0xed, 0xbe, 0x8b]);
    expected.push(0x01);
    expected.extend([0x08, 0x00, 0x05, 0x41, 0x6c, 0x69, 0x63, 0x65]);
    expected.push(0x00);
    assert_eq!(
        take_bodies(&bob, clientbound::DISGUISED_CHAT),
        std::slice::from_ref(&expected)
    );
    assert_eq!(take_bodies(&alice, clientbound::DISGUISED_CHAT), [expected]);
    assert!(!alice.is_closed());
}

#[test]
fn illegal_characters_kick_the_sender_and_reach_nobody() {
    for message in ["§a红字", "a\u{7f}", "a\nb", "tab\t", "\0", "\u{1f}"] {
        let (mut server, alice, bob) = alice_and_bob();

        server.request(&alice, chat(message));
        server.tick();

        assert_kicked_for_illegal_characters(&alice);
        assert!(
            take_bodies(&bob, clientbound::DISGUISED_CHAT).is_empty(),
            "{message:?}"
        );
        assert!(!bob.is_closed());
    }
}

#[test]
fn characters_next_to_the_illegal_ones_are_allowed() {
    let (mut server, alice, bob) = alice_and_bob();
    let message = " ~\u{80}¦¨";

    server.request(&alice, chat(message));
    server.tick();

    assert_eq!(
        take_bodies(&bob, clientbound::DISGUISED_CHAT),
        [chat_body("Alice", message)]
    );
    assert!(!alice.is_closed());
}

#[test]
fn the_chat_limit_is_256_utf16_units_at_the_protocol_layer() {
    for (unit, longest) in [("a", 256), ("竹", 256), ("🎋", 128)] {
        let (mut server, alice, bob) = alice_and_bob();
        let message = unit.repeat(longest);

        let request = PlayRequest::decode(&chat_payload(&message)).expect("刚好到上限");
        server.request(&alice, request);
        server.tick();
        assert_eq!(
            take_bodies(&bob, clientbound::DISGUISED_CHAT),
            [chat_body("Alice", &message)]
        );

        // 解码失败时连接任务断开连接，这个包到不了世界线程。
        assert_eq!(
            PlayRequest::decode(&chat_payload(&unit.repeat(longest + 1))),
            Err(DecodeError::StringTooLong { max_chars: 256 })
        );
    }
}

#[test]
fn a_kicked_player_is_out_of_the_chat_until_it_leaves() {
    let (mut server, alice, bob) = alice_and_bob();

    server.request(&alice, chat("§"));
    server.request(&alice, chat("still talking"));
    server.request(&alice, command("foo"));
    server.request(
        &alice,
        PlayRequest::ClientCommand {
            action: ClientCommandKind::RequestStats,
        },
    );
    server.request(&bob, chat("hello"));
    server.tick();

    let frames = alice.take_frames();
    assert_eq!(
        frames.last().map(|frame| frame.id),
        Some(clientbound::DISCONNECT)
    );
    assert!(frames_with_id(&frames, clientbound::DISGUISED_CHAT).is_empty());
    assert!(frames_with_id(&frames, clientbound::SYSTEM_CHAT).is_empty());
    assert!(frames_with_id(&frames, clientbound::AWARD_STATS).is_empty());
    assert_eq!(
        take_bodies(&bob, clientbound::DISGUISED_CHAT),
        [chat_body("Bob", "hello")]
    );

    assert!(server.region().players.contains_key(&alice.id));
    server.leave(&alice);
    server.tick();
    assert!(!server.region().players.contains_key(&alice.id));
}

#[test]
fn the_burst_limit_is_counted_for_each_player_on_its_own() {
    let (mut server, alice, bob) = alice_and_bob();

    send_chats(&server, &alice, MOST_IN_A_BURST);
    send_chats(&server, &bob, MOST_IN_A_BURST);
    server.tick();

    assert_eq!(
        take_bodies(&alice, clientbound::DISGUISED_CHAT).len(),
        2 * MOST_IN_A_BURST
    );
    assert!(!alice.is_closed());
    assert!(!bob.is_closed());
}

/// 官方先广播再计数，所以到阈值的这一条所有人都收得到，发送者接着被踢。
#[test]
fn the_message_that_reaches_the_limit_is_broadcast_before_the_kick() {
    let (mut server, alice, bob) = alice_and_bob();

    send_chats(&server, &alice, MOST_IN_A_BURST + 1);
    server.tick();

    let frames = alice.take_frames();
    assert_kicked_for_spam(&frames, &alice);
    let last_chat = chat_body("Alice", "message 9");
    let chats = frames_with_id(&frames, clientbound::DISGUISED_CHAT);
    assert_eq!(chats.len(), MOST_IN_A_BURST + 1);
    assert_eq!(chats[MOST_IN_A_BURST].body, last_chat);
    let seen_by_bob = take_bodies(&bob, clientbound::DISGUISED_CHAT);
    assert_eq!(seen_by_bob.len(), MOST_IN_A_BURST + 1);
    assert_eq!(seen_by_bob[MOST_IN_A_BURST], last_chat);
    assert!(!bob.is_closed());
}

#[test]
fn messages_after_the_kick_reach_nobody() {
    let (mut server, alice, bob) = alice_and_bob();

    send_chats(&server, &alice, MOST_IN_A_BURST + 5);
    server.request(&bob, chat("hello"));
    server.tick();

    let seen_by_bob = take_bodies(&bob, clientbound::DISGUISED_CHAT);
    assert_eq!(seen_by_bob.len(), MOST_IN_A_BURST + 2);
    assert_eq!(seen_by_bob.last(), Some(&chat_body("Bob", "hello")));
    assert!(server.region().players.contains_key(&alice.id));
}

/// 计数的变化是 180、179、199、198、218：199 还不到阈值，下一条才到。
#[test]
fn each_tick_takes_exactly_one_off_the_count() {
    let (mut server, alice, _bob) = alice_and_bob();

    send_chats(&server, &alice, MOST_IN_A_BURST);
    server.tick();
    server.request(&alice, chat("under the limit by one"));
    server.tick();
    assert!(!alice.is_closed());

    server.request(&alice, chat("over the limit"));
    server.tick();
    assert_kicked_for_spam(&alice.take_frames(), &alice);
}

#[test]
fn one_message_a_second_never_reaches_the_limit() {
    let (mut server, alice, _bob) = alice_and_bob();

    for _ in 0..3 * MOST_IN_A_BURST {
        server.request(&alice, chat("steady"));
        idle(&mut server, 20);
    }

    assert!(!alice.is_closed());
}

/// 计数停在 0：沉默再久也攒不下额度，下一轮连发还是第 10 条被踢。
#[test]
fn silence_clears_the_count_but_saves_up_nothing() {
    let (mut server, alice, _bob) = alice_and_bob();

    send_chats(&server, &alice, MOST_IN_A_BURST);
    idle(&mut server, 400);
    send_chats(&server, &alice, MOST_IN_A_BURST);
    server.tick();
    assert!(!alice.is_closed());

    idle(&mut server, 400);
    send_chats(&server, &alice, MOST_IN_A_BURST + 1);
    server.tick();
    assert_kicked_for_spam(&alice.take_frames(), &alice);
}

#[test]
fn the_command_that_reaches_the_limit_is_answered_before_the_kick() {
    let (mut server, alice, _bob) = alice_and_bob();

    send_commands(&server, &alice, MOST_IN_A_BURST);
    server.tick();
    assert!(!alice.is_closed());
    alice.take_frames();
    idle(&mut server, 180);

    send_commands(&server, &alice, MOST_IN_A_BURST + 1);
    server.tick();

    let frames = alice.take_frames();
    assert_kicked_for_spam(&frames, &alice);
    assert_eq!(
        frames_with_id(&frames, clientbound::SYSTEM_CHAT).len(),
        2 * (MOST_IN_A_BURST + 1)
    );
}

/// 官方的聊天和命令是两个计数器。
#[test]
fn chat_and_commands_have_separate_limits() {
    let (mut server, alice, _bob) = alice_and_bob();

    send_chats(&server, &alice, MOST_IN_A_BURST);
    send_commands(&server, &alice, MOST_IN_A_BURST);
    server.tick();

    assert!(!alice.is_closed());
}

/// 官方在进计数之前就回绝了隐藏聊天的玩家，它们的消息本来也到不了别人那里。
#[test]
fn hidden_chat_does_not_count_as_spam() {
    let mut server = Harness::new(2);
    let information = ClientInformation {
        chat_visibility: ChatVisibility::Hidden,
        ..client_requesting(2)
    };
    let hidden = server.join_with("Bob", information);
    server.tick();
    hidden.take_frames();

    send_chats(&server, &hidden, 3 * MOST_IN_A_BURST);
    server.tick();

    assert_eq!(
        take_bodies(&hidden, clientbound::SYSTEM_CHAT).len(),
        3 * MOST_IN_A_BURST
    );
    assert!(!hidden.is_closed());
}

#[test]
fn stats_requests_do_not_count_as_spam() {
    let (mut server, alice, _bob) = alice_and_bob();

    for _ in 0..3 * MOST_IN_A_BURST {
        server.request(
            &alice,
            PlayRequest::ClientCommand {
                action: ClientCommandKind::RequestStats,
            },
        );
    }
    server.tick();

    assert!(!alice.is_closed());
}

#[test]
fn chat_only_reaches_players_already_in_the_world() {
    let mut server = Harness::new(2);
    let alice = server.join_and_settle("Alice");
    alice.take_frames();

    // 队列按顺序处理，`Join` 之前的消息新玩家收不到。
    server.request(&alice, chat("before"));
    let bob = server.join("Bob");
    server.request(&alice, chat("after"));
    server.tick();

    let frames = bob.take_frames();
    assert_eq!(frames[0].id, clientbound::LOGIN);
    let chats: Vec<&[u8]> = frames_with_id(&frames, clientbound::DISGUISED_CHAT)
        .into_iter()
        .map(|frame| frame.body.as_slice())
        .collect();
    assert_eq!(chats, [chat_body("Alice", "after")]);
    assert_eq!(
        take_bodies(&alice, clientbound::DISGUISED_CHAT),
        [chat_body("Alice", "before"), chat_body("Alice", "after")]
    );

    server.leave(&bob);
    server.request(&alice, chat("gone"));
    server.tick();
    assert!(take_bodies(&bob, clientbound::DISGUISED_CHAT).is_empty());
    assert_eq!(
        take_bodies(&alice, clientbound::DISGUISED_CHAT),
        [chat_body("Alice", "gone")]
    );
}

#[test]
fn hidden_chat_gets_a_red_notice_and_is_not_broadcast() {
    let mut server = Harness::new(2);
    let watcher = server.join_and_settle("Alice");
    let information = ClientInformation {
        chat_visibility: ChatVisibility::Hidden,
        ..client_requesting(2)
    };
    let hidden = server.join_with("Bob", information);
    server.tick();
    watcher.take_frames();
    hidden.take_frames();

    server.request(&hidden, chat("hi"));
    server.tick();

    let notice = compound([
        ("translate", text("chat.disabled.options")),
        ("color", text("red")),
    ]);
    let frames = hidden.take_frames();
    assert!(frames_with_id(&frames, clientbound::DISGUISED_CHAT).is_empty());
    assert_eq!(
        frames_with_id(&frames, clientbound::SYSTEM_CHAT)[0].body,
        system_chat_body(&notice)
    );
    assert!(take_bodies(&watcher, clientbound::DISGUISED_CHAT).is_empty());
    assert!(!hidden.is_closed());

    // 官方的命令分支不看可见性。
    server.request(&hidden, command("foo"));
    server.tick();
    assert_eq!(take_bodies(&hidden, clientbound::SYSTEM_CHAT).len(), 2);

    // 非法字符排在可见性前面检查。
    server.request(&hidden, chat("§"));
    server.tick();
    assert_kicked_for_illegal_characters(&hidden);
}

#[test]
fn unknown_command_gets_the_two_red_lines_and_only_the_sender_sees_them() {
    let (mut server, alice, bob) = alice_and_bob();

    server.request(&alice, command("foo bar"));
    server.tick();

    assert_eq!(
        take_bodies(&alice, clientbound::SYSTEM_CHAT),
        [
            system_chat_body(&unknown_command_line()),
            system_chat_body(&context_line_echoing("foo bar")),
        ]
    );
    assert!(take_bodies(&bob, clientbound::SYSTEM_CHAT).is_empty());
    assert!(!alice.is_closed());
}

#[test]
fn a_bare_slash_has_nothing_to_underline() {
    let (mut server, alice, _bob) = alice_and_bob();

    server.request(&alice, command(""));
    server.tick();

    assert_eq!(
        take_bodies(&alice, clientbound::SYSTEM_CHAT),
        [
            system_chat_body(&unknown_command_line()),
            system_chat_body(&context_line("/", vec![here_marker()])),
        ]
    );
}

#[test]
fn everything_the_input_box_can_hold_is_echoed_in_full() {
    let (mut server, alice, _bob) = alice_and_bob();
    // 输入框放 256 个字符，开头的 `/` 占了一个。
    let longest = "x".repeat(255);

    server.request(&alice, command(&longest));
    server.tick();

    assert_eq!(
        take_bodies(&alice, clientbound::SYSTEM_CHAT)[1],
        system_chat_body(&context_line_echoing(&longest))
    );
}

#[test]
fn the_longest_commands_are_echoed_in_full() {
    for (unit, utf16_units) in [("a", 1), ("竹", 1), ("🎋", 2)] {
        let (mut server, alice, _bob) = alice_and_bob();
        let sent = unit.repeat(MAX_COMMAND_UTF16_UNITS / utf16_units);
        let mut payload = Vec::new();
        write_varint(&mut payload, serverbound::CHAT_COMMAND);
        write_string(&mut payload, &sent);

        let request = PlayRequest::decode(&payload).expect("没有超过协议的上限");
        server.request(&alice, request);
        server.tick();

        // 官方 `Commands.finishParsing` 不截断回显，整段输入都画在红下划线里。
        //
        // Official `Commands.finishParsing` does not truncate the echo. The whole input is underlined.
        assert_eq!(
            take_bodies(&alice, clientbound::SYSTEM_CHAT)[1],
            system_chat_body(&context_line_echoing(&sent))
        );
        assert!(!alice.is_closed());
    }
}

#[test]
fn commands_with_illegal_characters_kick_like_chat() {
    for sent in ["§", "say \u{7f}", "a\nb"] {
        let (mut server, alice, _bob) = alice_and_bob();

        server.request(&alice, command(sent));
        server.tick();

        assert_kicked_for_illegal_characters(&alice);
    }
}

#[test]
fn request_stats_is_answered_with_empty_stats_for_the_sender_only() {
    let (mut server, alice, bob) = alice_and_bob();

    server.request(
        &alice,
        PlayRequest::ClientCommand {
            action: ClientCommandKind::RequestStats,
        },
    );
    server.tick();

    assert_eq!(take_bodies(&alice, clientbound::AWARD_STATS), [[0x00]]);
    assert!(take_bodies(&bob, clientbound::AWARD_STATS).is_empty());
}

#[test]
fn other_client_commands_are_ignored() {
    let (mut server, alice, _bob) = alice_and_bob();

    for action in [
        ClientCommandKind::PerformRespawn,
        ClientCommandKind::RequestGameRuleValues,
    ] {
        server.request(&alice, PlayRequest::ClientCommand { action });
    }
    server.tick();

    assert!(take_bodies(&alice, clientbound::AWARD_STATS).is_empty());
    assert!(!alice.is_closed());
}

#[test]
fn join_and_left_messages_match_the_spec_and_skip_the_player_itself() {
    let mut server = Harness::new(2);
    let alice = server.join_and_settle("Alice");
    alice.take_frames();
    let message = |key: &str| {
        system_chat_body(&compound([
            ("translate", text(key)),
            ("with", Tag::List(vec![text("Kitten")])),
            ("color", text("yellow")),
        ]))
    };

    let kitten = server.join("Kitten");
    server.tick();

    let joined = message("multiplayer.player.joined");
    assert_eq!(joined.len(), 78);
    assert_eq!(take_bodies(&alice, clientbound::SYSTEM_CHAT), [joined]);
    assert!(take_bodies(&kitten, clientbound::SYSTEM_CHAT).is_empty());

    server.leave(&kitten);
    server.tick();

    assert_eq!(
        take_bodies(&alice, clientbound::SYSTEM_CHAT),
        [message("multiplayer.player.left")]
    );
    assert!(take_bodies(&kitten, clientbound::SYSTEM_CHAT).is_empty());
}

#[test]
fn join_sends_the_command_tree() {
    let mut server = Harness::new(2);
    let client = server.join("Kitten");

    server.tick();

    // `take_frames` 只能拿一次，两个包都在这一批里。
    //
    // `take_frames` can be called only once. Both packets are in this batch.
    let frames = client.take_frames();
    assert_eq!(frames_with_id(&frames, clientbound::COMMANDS).len(), 1);
    assert_eq!(frames_with_id(&frames, clientbound::ENTITY_EVENT).len(), 1);
}
