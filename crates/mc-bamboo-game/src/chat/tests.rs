//! 用假的客户端驱动世界线程，检查发出去的字节。期望值用 NBT 手写，不经过 [`Component`]。
//!
//! 这里只看聊天相关的包，所以别的模块在同一 tick 多发什么都不影响结果。

use mc_bamboo_data::packets::play::{clientbound, serverbound};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::{
    codec::{DecodeError, Reader, write_string, write_varint},
    packet::play::{ClientInformation, PlayRequest},
};
use uuid::Uuid;

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

/// 未签名的 `player_chat`：发送者是玩家，没有签名，盐是 0，已读列表是空的。
fn assert_player_chat(body: &[u8], global_index: i32, sender: Uuid, name: &str, message: &str) {
    let mut reader = Reader::new(body);
    assert_eq!(reader.varint().unwrap(), global_index);
    assert_eq!(reader.uuid().unwrap(), sender);
    assert_eq!(reader.varint().unwrap(), 0);
    assert_eq!(reader.u8().unwrap(), 0, "no signature");
    assert_eq!(reader.string(256).unwrap(), message);
    let time = reader.i64().unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    assert!(
        (time - now).abs() < 120_000,
        "timestamp {time} is not the server clock {now}"
    );
    assert_eq!(reader.i64().unwrap(), 0, "salt");
    assert_eq!(reader.varint().unwrap(), 0, "last seen");
    assert_eq!(reader.u8().unwrap(), 0, "unsigned content");
    assert_eq!(reader.varint().unwrap(), 0, "filter");
    assert_eq!(reader.varint().unwrap(), 1, "chat type");
    assert_eq!(read_text(&mut reader), name);
    assert_eq!(reader.u8().unwrap(), 0, "target");
    reader.finish().unwrap();
}

fn assert_player_chats(bodies: &[Vec<u8>], sender: Uuid, name: &str, messages: &[&str]) {
    assert_eq!(bodies.len(), messages.len());
    for (index, (body, message)) in bodies.iter().zip(messages).enumerate() {
        assert_player_chat(body, index as i32, sender, name, message);
    }
}

fn read_text(reader: &mut Reader<'_>) -> String {
    assert_eq!(reader.u8().unwrap(), 8);
    let len = reader.u16().unwrap() as usize;
    let mut bytes = vec![0; len];
    for byte in &mut bytes {
        *byte = reader.u8().unwrap();
    }
    String::from_utf8(bytes).unwrap()
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
    PlayRequest::unsigned_chat(message)
}

fn command(command: &str) -> PlayRequest {
    PlayRequest::unsigned_command(command)
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
    // 未签名 `player_chat`：全局序号 0、发送者 UUID 1、链序号 0、没有签名、
    // 时间 0、盐 0、已读列表空、`minecraft:chat`、名字 Kitten、没有目标。
    //
    // Unsigned `player_chat`: global index 0, sender UUID 1, chain index 0, no signature,
    // time 0, salt 0, empty last-seen, `minecraft:chat`, name Kitten, no target.
    let expected = [
        0x35, // 帧长
        0x42, // 包 ID
        0x00, // global index
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x01, // sender
        0x00, // index
        0x00, // 没有签名
        0x02, 0x68, 0x69, // "hi"
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // time
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // salt
        0x00, // last seen
        0x00, // 没有 unsigned content
        0x00, // filter
        0x01, // chat type
        0x08, 0x00, 0x06, 0x4b, 0x69, 0x74, 0x74, 0x65, 0x6e, // name
        0x00, // 没有 target
    ];
    let name = mc_bamboo_protocol::text::Component::text("Kitten");
    let frames = crate::links::encode_frames(&mc_bamboo_protocol::packet::play::PlayerChat {
        global_index: 0,
        sender: Uuid::from_u128(1),
        index: 0,
        signature: None,
        content: "hi",
        time_millis: 0,
        salt: 0,
        last_seen: &[],
        unsigned_content: None,
        chat_type: mc_bamboo_data::registries::chat_type::CHAT,
        name: &name,
        target_name: None,
    });
    assert_eq!(frames, expected);
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

    for client in [&kitten, &bob, &carol] {
        let bodies = take_bodies(client, clientbound::PLAYER_CHAT);
        assert_player_chats(&bodies, kitten.uuid, "Kitten", &["hi"]);
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

    assert_player_chats(
        &take_bodies(&bob, clientbound::PLAYER_CHAT),
        alice.uuid,
        "Alice",
        &["hi"],
    );
}

#[test]
fn chinese_and_emoji_keep_their_bytes() {
    let (mut server, alice, bob) = alice_and_bob();

    server.request(&alice, chat("竹子🎋"));
    server.tick();

    assert_player_chats(
        &take_bodies(&bob, clientbound::PLAYER_CHAT),
        alice.uuid,
        "Alice",
        &["竹子🎋"],
    );
    assert_player_chats(
        &take_bodies(&alice, clientbound::PLAYER_CHAT),
        alice.uuid,
        "Alice",
        &["竹子🎋"],
    );
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
            take_bodies(&bob, clientbound::PLAYER_CHAT).is_empty(),
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

    assert_player_chats(
        &take_bodies(&bob, clientbound::PLAYER_CHAT),
        alice.uuid,
        "Alice",
        &[message],
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
        assert_player_chats(
            &take_bodies(&bob, clientbound::PLAYER_CHAT),
            alice.uuid,
            "Alice",
            &[&message],
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
    assert!(frames_with_id(&frames, clientbound::PLAYER_CHAT).is_empty());
    assert!(frames_with_id(&frames, clientbound::SYSTEM_CHAT).is_empty());
    assert!(frames_with_id(&frames, clientbound::AWARD_STATS).is_empty());
    assert_player_chats(
        &take_bodies(&bob, clientbound::PLAYER_CHAT),
        bob.uuid,
        "Bob",
        &["hello"],
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
        take_bodies(&alice, clientbound::PLAYER_CHAT).len(),
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
    let chats = frames_with_id(&frames, clientbound::PLAYER_CHAT);
    assert_eq!(chats.len(), MOST_IN_A_BURST + 1);
    assert_player_chat(
        &chats[MOST_IN_A_BURST].body,
        MOST_IN_A_BURST as i32,
        alice.uuid,
        "Alice",
        "message 9",
    );
    let seen_by_bob = take_bodies(&bob, clientbound::PLAYER_CHAT);
    assert_eq!(seen_by_bob.len(), MOST_IN_A_BURST + 1);
    assert_player_chat(
        &seen_by_bob[MOST_IN_A_BURST],
        MOST_IN_A_BURST as i32,
        alice.uuid,
        "Alice",
        "message 9",
    );
    assert!(!bob.is_closed());
}

#[test]
fn messages_after_the_kick_reach_nobody() {
    let (mut server, alice, bob) = alice_and_bob();

    send_chats(&server, &alice, MOST_IN_A_BURST + 5);
    server.request(&bob, chat("hello"));
    server.tick();

    let seen_by_bob = take_bodies(&bob, clientbound::PLAYER_CHAT);
    assert_eq!(seen_by_bob.len(), MOST_IN_A_BURST + 2);
    assert_player_chat(
        seen_by_bob.last().unwrap(),
        (MOST_IN_A_BURST + 1) as i32,
        bob.uuid,
        "Bob",
        "hello",
    );
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
    let chats: Vec<Vec<u8>> = frames_with_id(&frames, clientbound::PLAYER_CHAT)
        .into_iter()
        .map(|frame| frame.body.clone())
        .collect();
    assert_player_chats(&chats, alice.uuid, "Alice", &["after"]);
    assert_player_chats(
        &take_bodies(&alice, clientbound::PLAYER_CHAT),
        alice.uuid,
        "Alice",
        &["before", "after"],
    );

    server.leave(&bob);
    server.request(&alice, chat("gone"));
    server.tick();
    assert!(take_bodies(&bob, clientbound::PLAYER_CHAT).is_empty());
    assert_player_chat(
        &take_bodies(&alice, clientbound::PLAYER_CHAT)[0],
        2,
        alice.uuid,
        "Alice",
        "gone",
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
    assert!(frames_with_id(&frames, clientbound::PLAYER_CHAT).is_empty());
    assert_eq!(
        frames_with_id(&frames, clientbound::SYSTEM_CHAT)[0].body,
        system_chat_body(&notice)
    );
    assert!(take_bodies(&watcher, clientbound::PLAYER_CHAT).is_empty());
    assert!(!hidden.is_closed());

    // 官方 `sendFailure` 走 `sendSystemMessage`，隐藏聊天时不发失败消息。
    //
    // Official `sendFailure` goes through `sendSystemMessage`, which suppresses the
    // failure text for hidden chat.
    server.request(&hidden, command("foo"));
    server.tick();
    assert!(take_bodies(&hidden, clientbound::SYSTEM_CHAT).is_empty());

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

fn frames_have_key(frames: &[Frame], key: &str) -> bool {
    frames_with_id(frames, clientbound::SYSTEM_CHAT)
        .iter()
        .any(|frame| {
            frame
                .body
                .windows(key.len())
                .any(|window| window == key.as_bytes())
        })
}

fn service_key() -> (rsa::RsaPrivateKey, Vec<u8>) {
    use rsa::pkcs8::EncodePublicKey;

    let private_key = rsa::RsaPrivateKey::new(&mut rand_core::OsRng, 2048).expect("rsa");
    let der = rsa::RsaPublicKey::from(&private_key)
        .to_public_key_der()
        .expect("spki")
        .as_bytes()
        .to_vec();
    (private_key, der)
}

fn sign_profile(
    private_key: &rsa::RsaPrivateKey,
    player: Uuid,
    expires: i64,
    der: &[u8],
) -> Vec<u8> {
    use rsa::pkcs1v15::Pkcs1v15Sign;
    use sha1::Sha1;
    use sha1::digest::Digest;

    let mut payload = Vec::new();
    payload.extend_from_slice(player.as_bytes());
    payload.extend_from_slice(&expires.to_be_bytes());
    payload.extend_from_slice(der);
    let hashed = Sha1::digest(&payload);
    private_key
        .sign(Pkcs1v15Sign::new::<Sha1>(), &hashed)
        .expect("sign profile")
}

/// 解析失败的签名命令仍要收集参数。名字对不上就打断链，下一句签名消息被拒。
#[test]
fn a_failed_signed_command_breaks_the_chain() {
    use mc_bamboo_protocol::packet::play::{
        ArgumentSignature, LastSeenUpdate, RemoteChatSession, SignedCommand,
    };

    let (private_key, der) = service_key();
    let mut server = Harness::new(2);
    assert!(
        server
            .region()
            .service_keys
            .replace(std::slice::from_ref(&der))
    );
    let alice = server.join_and_settle("Alice");
    alice.take_frames();
    let expires = 4_102_444_800_000;
    server.request(
        &alice,
        PlayRequest::ChatSession(RemoteChatSession {
            session_id: Uuid::from_u128(50),
            expires_at_millis: expires,
            public_key: der.clone(),
            key_signature: sign_profile(&private_key, alice.uuid, expires, &der),
        }),
    );
    server.tick();
    alice.take_frames();

    server.request(
        &alice,
        PlayRequest::ChatCommand {
            command: "notacommand".to_owned(),
            signed: Some(SignedCommand {
                time_millis: 5_000,
                salt: 1,
                arguments: vec![ArgumentSignature {
                    name: "message".to_owned(),
                    signature: [9; 256],
                }],
                last_seen: LastSeenUpdate::EMPTY,
            }),
        },
    );
    server.tick();
    let failed = alice.take_frames();
    assert!(frames_have_key(
        &failed,
        "chat.disabled.invalid_command_signature"
    ));
    assert!(frames_with_id(&failed, clientbound::PLAYER_CHAT).is_empty());

    server.request(
        &alice,
        PlayRequest::Chat {
            message: "next".to_owned(),
            signature: Some(Box::new([4; 256])),
            time_millis: 6_000,
            salt: 1,
            last_seen: LastSeenUpdate::EMPTY,
        },
    );
    server.tick();
    let broken = alice.take_frames();
    assert!(frames_have_key(&broken, "chat.disabled.chain_broken"));
    assert!(frames_with_id(&broken, clientbound::PLAYER_CHAT).is_empty());

    server.request(
        &alice,
        PlayRequest::ClientInformation(ClientInformation {
            chat_visibility: ChatVisibility::Hidden,
            ..client_requesting(2)
        }),
    );
    server.tick();
    alice.take_frames();
    server.request(
        &alice,
        PlayRequest::ChatCommand {
            command: "still-unknown".to_owned(),
            signed: Some(SignedCommand {
                time_millis: 7_000,
                salt: 1,
                arguments: vec![ArgumentSignature {
                    name: "message".to_owned(),
                    signature: [3; 256],
                }],
                last_seen: LastSeenUpdate::EMPTY,
            }),
        },
    );
    server.tick();
    assert!(
        take_bodies(&alice, clientbound::SYSTEM_CHAT).is_empty(),
        "hidden chat does not receive the failure"
    );
}

/// 钥匙还没到时未签名聊天照常转发。钥匙到了之后，同一名玩家的未签名聊天按强制处理。
#[test]
fn unsigned_chat_is_enforced_after_profile_keys_arrive() {
    let mut server = Harness::new(2);
    {
        let region = server.region();
        region.online_mode = true;
        region.enforce_secure_profile = true;
    }
    let alice = server.join_and_settle("Alice");
    alice.take_frames();

    server.request(&alice, chat("before keys"));
    server.tick();
    assert_player_chats(
        &take_bodies(&alice, clientbound::PLAYER_CHAT),
        alice.uuid,
        "Alice",
        &["before keys"],
    );

    let (_private_key, der) = service_key();
    assert!(
        server
            .region()
            .service_keys
            .replace(std::slice::from_ref(&der))
    );
    alice.take_frames();
    server.request(&alice, chat("after keys"));
    server.tick();
    let rejected = alice.take_frames();
    assert!(frames_with_id(&rejected, clientbound::PLAYER_CHAT).is_empty());
    assert!(frames_have_key(
        &rejected,
        "chat.disabled.missingProfileKey"
    ));
    assert!(!alice.is_closed());
}

/// 隐藏聊天的玩家发带可签参数的无签名命令：官方 `sendSystemMessage` 抑制那条红色提示，
/// 但全可见性的玩家仍收到，证明命令确实被拒了。
///
/// A hidden-chat player sends an unsigned command with signable arguments: official
/// `sendSystemMessage` suppresses the red notice, while a full-visibility player still
/// receives it, proving the command was rejected.
#[test]
fn hidden_player_does_not_get_the_invalid_command_signature_notice() {
    let (_private_key, der) = service_key();
    let mut server = Harness::new(2);
    {
        let region = server.region();
        region.online_mode = true;
        region.enforce_secure_profile = true;
    }
    let hidden = server.join_with(
        "Hidden",
        ClientInformation {
            chat_visibility: ChatVisibility::Hidden,
            ..client_requesting(2)
        },
    );
    let watcher = server.join("Watcher");
    server.tick();
    hidden.take_frames();
    watcher.take_frames();
    assert!(
        server
            .region()
            .service_keys
            .replace(std::slice::from_ref(&der))
    );
    hidden.take_frames();
    watcher.take_frames();

    server.request(&hidden, command("me hi"));
    server.request(&watcher, command("me hi"));
    server.tick();

    let hidden_frames = hidden.take_frames();
    assert!(!frames_have_key(
        &hidden_frames,
        "chat.disabled.invalid_command_signature"
    ));
    assert!(frames_with_id(&hidden_frames, clientbound::PLAYER_CHAT).is_empty());
    assert!(frames_have_key(
        &watcher.take_frames(),
        "chat.disabled.invalid_command_signature"
    ));
}

/// 官方 `sendSystemMessage(message, true)` 的 overlay 消息连隐藏聊天也发；
/// 同一个函数用 `overlay=false` 时抑制。
///
/// Official `sendSystemMessage(message, true)` delivers overlay messages even for hidden
/// chat; the same function with `overlay=false` suppresses them.
#[test]
fn overlay_system_messages_reach_hidden_players() {
    let mut server = Harness::new(2);
    let hidden = server.join_with(
        "Hidden",
        ClientInformation {
            chat_visibility: ChatVisibility::Hidden,
            ..client_requesting(2)
        },
    );
    server.tick();
    hidden.take_frames();

    let content = mc_bamboo_protocol::text::Component::text("hello");
    crate::chat::send_system_message(server.region(), hidden.id, &content, false);
    server.tick();
    assert!(take_bodies(&hidden, clientbound::SYSTEM_CHAT).is_empty());

    crate::chat::send_system_message(server.region(), hidden.id, &content, true);
    server.tick();
    let bodies = take_bodies(&hidden, clientbound::SYSTEM_CHAT);
    assert_eq!(bodies.len(), 1);
    assert_eq!(bodies[0].last(), Some(&1), "overlay flag is set");
}

/// 被踢但还没 `Leave` 的玩家不在签名聊天的接收名单里，`next_index` 和 `seen` 不被推进。
///
/// A kicked player who has not left yet is left out of the signed-chat recipient list;
/// its `next_index` and `seen` are not advanced.
#[test]
fn kicked_players_are_left_out_of_the_signed_chat_list() {
    let mut server = Harness::new(2);
    let alice = server.join_and_settle("Alice");
    let bob = server.join_and_settle("Bob");
    alice.take_frames();
    bob.take_frames();

    server
        .region()
        .links
        .kick(bob.id, &mc_bamboo_protocol::text::Component::text("bye"));
    bob.take_frames();
    assert!(!server.region().links.is_open(bob.id));
    assert!(server.region().players.contains_key(&bob.id));

    let before_next = server.region().players[&bob.id].chat.next_index;
    let before_seen = server.region().players[&bob.id].chat.seen.len();

    let signed = crate::secure_chat::SignedMessage {
        sender: alice.uuid,
        index: 0,
        signature: Some([7u8; 256]),
        content: "hello".to_owned(),
        time_millis: 1_000,
        salt: 1,
        last_seen: vec![],
    };
    let name = mc_bamboo_protocol::text::Component::text("Alice");
    crate::secure_chat::broadcast_signed(
        server.region(),
        &signed,
        mc_bamboo_data::registries::chat_type::CHAT,
        &name,
        None,
        None,
    );
    server.tick();

    assert_eq!(
        server.region().players[&bob.id].chat.next_index,
        before_next
    );
    assert_eq!(
        server.region().players[&bob.id].chat.seen.len(),
        before_seen
    );
    assert_eq!(
        server.region().players[&alice.id].chat.next_index,
        before_next + 1
    );
    assert_eq!(
        server.region().players[&alice.id].chat.seen.len(),
        before_seen + 1
    );
    assert!(take_bodies(&bob, clientbound::PLAYER_CHAT).is_empty());
    assert_eq!(take_bodies(&alice, clientbound::PLAYER_CHAT).len(), 1);
}
