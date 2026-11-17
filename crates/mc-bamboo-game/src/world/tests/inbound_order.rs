//! 入站按序号归并（岛的实施计划第 6 节第 4 步）：玩家的请求走各自的队列、
//! 别的消息走主控队列，世界每刻按序号归并；处理顺序必须和所有消息走一条通道时相同。
//!
//! Inbound merge by number (island plan, section 6, step 4): player requests
//! use their own queues, everything else the master queue, and the world merges
//! them by number each tick; the handling order must equal a single channel's.

use super::*;

/// 每名玩家的话里都带一个不会在别处出现的记号，按收到的顺序挑出来。
///
/// Every line carries a marker that appears nowhere else; pick them out in
/// the order received.
fn markers_in_order(frames: &[Frame], markers: &[&'static str]) -> Vec<&'static str> {
    let mut seen = Vec::new();
    for frame in frames {
        for &marker in markers {
            if frame
                .body
                .windows(marker.len())
                .any(|window| window == marker.as_bytes())
            {
                seen.push(marker);
            }
        }
    }
    seen
}

const MARKERS: [&str; 9] = [
    "qx-alpha-1",
    "qx-bravo-1",
    "qx-master-1",
    "qx-alpha-2",
    "qx-bravo-2",
    "qx-delta-1",
    "qx-alpha-3",
    "qx-bravo-3",
    "qx-master-2",
];

/// 同一个场景跑一遍，返回旁观者 Carol 收到的全部帧和三名玩家各自收到的原始帧。
/// 两名玩家交错发话，中间夹着主控队列的控制台命令和延迟、同一刻进服并马上说话的
/// Dave，以及 Alice 离开之后 Bob 接着说的话。
///
/// Run one scenario and return what the observer Carol received plus every
/// player's raw frames. Two players interleave lines, with master-queue console
/// commands and latency in between, Dave joining and speaking in the same
/// tick, and Bob speaking right after Alice leaves.
fn run_interleaved(single_channel: bool) -> (Vec<Frame>, Vec<Vec<Frame>>) {
    let mut server = Harness::new(2);
    // 聊天包里有墙上时钟，两遍要比字节就固定住。
    // Chat packets carry the wall clock; pin it so the two runs compare byte for byte.
    server.world.island.region.clock_millis = Some(1_700_000_000_000);
    if single_channel {
        server.use_single_channel();
    }
    let alice = server.join_and_settle("Alice");
    let bob = server.join_and_settle("Bob");
    let carol = server.join_and_settle("Carol");
    for client in [&alice, &bob, &carol] {
        client.take_frames();
    }

    let say = |server: &Harness, client: &Client, text: &str| {
        server.request(client, PlayRequest::unsigned_chat(text));
    };
    say(&server, &alice, "qx-alpha-1");
    say(&server, &bob, "qx-bravo-1");
    server
        .inbox
        .send(Inbound::ConsoleCommand("say qx-master-1".to_owned()))
        .unwrap();
    server
        .inbox
        .send(Inbound::Latency {
            player: alice.id,
            millis: 7,
        })
        .unwrap();
    say(&server, &alice, "qx-alpha-2");
    say(&server, &bob, "qx-bravo-2");
    let dave = server.join("Dave");
    say(&server, &dave, "qx-delta-1");
    say(&server, &alice, "qx-alpha-3");
    server.leave(&alice);
    say(&server, &bob, "qx-bravo-3");
    server
        .inbox
        .send(Inbound::ConsoleCommand("say qx-master-2".to_owned()))
        .unwrap();
    server.tick();

    let observed = carol.take_frames();
    let raw = [&alice, &bob, &dave]
        .into_iter()
        .map(Client::take_frames)
        .collect();
    (observed, raw)
}

#[test]
fn interleaved_players_merge_into_the_single_channel_order() {
    let (merged, merged_raw) = run_interleaved(false);
    let (single, single_raw) = run_interleaved(true);

    // 旁观者看到的先后就是发出的先后，和单通道完全一样。
    // The observer sees the send order, exactly as with a single channel.
    assert_eq!(markers_in_order(&merged, &MARKERS), MARKERS);
    assert_eq!(merged, single);
    assert_eq!(merged_raw, single_raw);
}

#[test]
fn sequence_numbers_increase_across_every_sender() {
    let (master, receiver) = crate::inbound::channel();
    let (first, first_inbox) = master.player_channel();
    let (second, second_inbox) = master.player_channel();
    let leave = |id| Inbound::Leave {
        player: PlayerId(id),
    };
    first.send(leave(1)).unwrap();
    master.send(Inbound::Shutdown).unwrap();
    second.send(leave(2)).unwrap();
    first.send(leave(3)).unwrap();
    master.clone().send(Inbound::Shutdown).unwrap();
    second.send(leave(4)).unwrap();

    let mut batch = Vec::new();
    assert!(!receiver.drain_into(&mut batch));
    first_inbox.drain_into(&mut batch);
    second_inbox.drain_into(&mut batch);
    crate::inbound::merge(&mut batch);
    let order: Vec<(u64, Option<i32>)> = batch
        .iter()
        .map(|sealed| match sealed.message {
            Inbound::Leave { player } => (sealed.seq, Some(player.0)),
            _ => (sealed.seq, None),
        })
        .collect();
    assert_eq!(
        order,
        [
            (0, Some(1)),
            (1, None),
            (2, Some(2)),
            (3, Some(3)),
            (4, None),
            (5, Some(4)),
        ]
    );
}
