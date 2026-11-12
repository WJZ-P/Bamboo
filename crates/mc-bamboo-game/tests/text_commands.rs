//! 四个文本/展示类命令的端到端测试：命令树、发出的包和反馈。

use std::sync::{Arc, Mutex};

use mc_bamboo_data::packets::play::clientbound;
use mc_bamboo_game::{
    InFlight, Inbound, JoinRequest, Outgoing, PlayerId, PlayerSink, World, WorldConfig,
};
use mc_bamboo_protocol::{
    codec::Reader,
    frame::{MAX_FRAME_LEN, find_frame},
    packet::play::{ClientInformation, PlayRequest},
};
use uuid::Uuid;

#[derive(Clone, Default)]
struct Sink(Arc<Mutex<Vec<u8>>>);

impl PlayerSink for Sink {
    fn send(&mut self, frames: Outgoing) -> bool {
        self.0.lock().unwrap().extend(frames.to_raw_bytes());
        true
    }

    fn close(&mut self) {}
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Frame {
    id: i32,
    body: Vec<u8>,
}

struct Harness {
    inbox: std::sync::mpsc::Sender<Inbound>,
    world: World,
    quota: Arc<std::sync::atomic::AtomicUsize>,
    next_id: i32,
    sinks: Vec<(PlayerId, Sink)>,
}

impl Harness {
    fn new() -> Self {
        let (inbox, receiver) = std::sync::mpsc::channel();
        let world = World::new(
            WorldConfig {
                view_distance: 2,
                max_players: 20,
                online_mode: false,
                enforce_secure_profile: false,
                admin: Default::default(),
                ..Default::default()
            },
            receiver,
        );
        Self {
            inbox,
            world,
            quota: Arc::default(),
            next_id: 1,
            sinks: Vec::new(),
        }
    }

    fn join(&mut self, name: &str, level: u8) -> PlayerId {
        let id = PlayerId(self.next_id);
        self.next_id += 1;
        let sink = Sink::default();
        let join = JoinRequest {
            id,
            uuid: Uuid::from_u128(u128::from(id.0 as u32)),
            name: name.to_owned(),
            properties: Vec::new(),
            client: ClientInformation::default(),
            permission_level: level,
            is_op: level > 0,
            peer_ip: "127.0.0.1".to_owned(),
            online_at_login: 0,
            sink: Box::new(sink.clone()),
        };
        self.inbox.send(Inbound::Join(Box::new(join))).unwrap();
        self.sinks.push((id, sink));
        id
    }

    fn command(&self, from: PlayerId, command: &str) {
        let in_flight = InFlight::acquire(&self.quota, 4096).expect("测试不会用完配额");
        self.inbox
            .send(Inbound::Request {
                player: from,
                request: PlayRequest::unsigned_command(command),
                in_flight,
            })
            .unwrap();
    }

    /// 结束这一轮：`run` 处理完队列、flush 帧后遇到 `Shutdown` 退出。
    /// 每个测试只在最后调一次，所以所有消息都在同一个 tick 里。
    fn tick(&mut self) {
        self.inbox.send(Inbound::Shutdown).unwrap();
        let world = std::mem::replace(
            &mut self.world,
            World::new(
                WorldConfig {
                    view_distance: 2,
                    max_players: 20,
                    online_mode: false,
                    enforce_secure_profile: false,
                    admin: Default::default(),
                    ..Default::default()
                },
                std::sync::mpsc::channel().1,
            ),
        );
        world.run();
    }

    fn frames(&self, id: PlayerId) -> Vec<Frame> {
        let sink = &self
            .sinks
            .iter()
            .find(|(player, _)| *player == id)
            .unwrap()
            .1;
        let bytes = std::mem::take(&mut *sink.0.lock().unwrap());
        let mut rest = bytes.as_slice();
        let mut frames = Vec::new();
        while !rest.is_empty() {
            let bounds = find_frame(rest, MAX_FRAME_LEN)
                .expect("服务端只发合法的帧")
                .expect("flush 的都是完整的帧");
            let end = bounds.header_len + bounds.payload_len;
            let mut reader = Reader::new(&rest[bounds.header_len..end]);
            let id = reader.varint().unwrap();
            frames.push(Frame {
                id,
                body: reader.rest().to_vec(),
            });
            rest = &rest[end..];
        }
        frames
    }
}

/// 一层 NBT 的字节里查一段 ASCII 文本，供断言用。
fn contains_ascii(bytes: &[u8], needle: &str) -> bool {
    bytes
        .windows(needle.len())
        .any(|window| window == needle.as_bytes())
}

/// 进服帧、命令帧都在同一 tick。`Shutdown` 会额外发一个 disconnect，过滤掉它。
fn chats(harness: &Harness, id: PlayerId) -> Vec<Vec<u8>> {
    let frames = harness.frames(id);
    frames
        .iter()
        .filter(|frame| frame.id == clientbound::SYSTEM_CHAT)
        .map(|frame| frame.body.clone())
        .collect()
}

#[test]
fn tellraw_sends_one_system_chat_to_each_target() {
    let mut harness = Harness::new();
    let source = harness.join("Kitten", 2);
    let target = harness.join("Alex", 0);
    harness.command(source, "tellraw @a {text:\"hi\",color:red}");
    harness.tick();

    let source_chats = chats(&harness, source);
    let target_chats = chats(&harness, target);
    // 进服时还会收到一条「加入了游戏」的系统消息，先按组件内容过滤掉它。
    let tellraw: Vec<_> = source_chats
        .iter()
        .filter(|body| contains_ascii(body, "hi"))
        .collect();
    let target_tellraw: Vec<_> = target_chats
        .iter()
        .filter(|body| contains_ascii(body, "hi"))
        .collect();
    assert_eq!(tellraw.len(), 1, "没有成功反馈");
    assert_eq!(target_tellraw.len(), 1);
    assert_eq!(tellraw[0], target_tellraw[0]);
    assert_eq!(tellraw[0].last(), Some(&0));
    assert!(contains_ascii(tellraw[0], "hi"));
    assert!(contains_ascii(tellraw[0], "hi"));
}

#[test]
fn tellraw_resolves_selectors_per_target() {
    let mut harness = Harness::new();
    let source = harness.join("Kitten", 2);
    harness.command(source, "tellraw @a {selector:\"@s\"}");
    harness.tick();

    let chats = chats(&harness, source);
    let resolved: Vec<_> = chats
        .iter()
        .filter(|body| contains_ascii(body, "Kitten") && !contains_ascii(body, "joined"))
        .collect();
    assert_eq!(resolved.len(), 1);
}

#[test]
fn tellraw_level_one_cannot_use_the_command() {
    let mut harness = Harness::new();
    let source = harness.join("Kitten", 1);
    harness.command(source, "tellraw @a \"not_delivered\"");
    harness.tick();

    // 等级不足时命令树里没有 `tellraw`，只有解析错误的两行系统消息。
    // 第二行 `command.context.here` 会把命令原文带回来，所以按「第二行存在」判断。
    let frames = harness.frames(source);
    let system_chats: Vec<_> = frames
        .iter()
        .filter(|frame| frame.id == clientbound::SYSTEM_CHAT)
        .collect();
    assert_eq!(system_chats.len(), 2, "解析错误有正文和 here 两行");
    assert!(
        system_chats
            .iter()
            .any(|frame| contains_ascii(&frame.body, "command.context.here"))
    );
    assert!(
        system_chats
            .iter()
            .any(|frame| contains_ascii(&frame.body, "command.unknown.command"))
    );
}

#[test]
fn title_bursts_send_the_five_packets_and_report_success() {
    use mc_bamboo_data::packets::play::clientbound;

    let mut harness = Harness::new();
    let source = harness.join("Kitten", 2);
    harness.command(source, "title @s clear");
    harness.command(source, "title @s reset");
    harness.command(source, "title @s title \"hi\"");
    harness.command(source, "title @s subtitle \"sub\"");
    harness.command(source, "title @s actionbar \"bar\"");
    harness.command(source, "title @s times 10t 70t 20t");
    harness.tick();

    let frames = harness.frames(source);
    for (id, expected) in [
        (clientbound::CLEAR_TITLES, 2),
        (clientbound::SET_TITLE_TEXT, 1),
        (clientbound::SET_SUBTITLE_TEXT, 1),
        (clientbound::SET_ACTION_BAR_TEXT, 1),
        (clientbound::SET_TITLES_ANIMATION, 1),
    ] {
        let count = frames.iter().filter(|frame| frame.id == id).count();
        assert_eq!(count, expected, "packet {id}");
    }
    let clears: Vec<_> = frames
        .iter()
        .filter(|frame| frame.id == clientbound::CLEAR_TITLES)
        .map(|frame| frame.body[0])
        .collect();
    assert_eq!(clears, [0, 1]);
    let animation = frames
        .iter()
        .find(|frame| frame.id == clientbound::SET_TITLES_ANIMATION)
        .unwrap();
    assert_eq!(animation.body, vec![0, 0, 0, 10, 0, 0, 0, 70, 0, 0, 0, 20]);
    // 六条命令各一条成功消息（`.single`，因为 `@s` 只选到一个人）。
    let successes = frames
        .iter()
        .filter(|frame| frame.id == clientbound::SYSTEM_CHAT)
        .filter(|frame| contains_ascii(&frame.body, "commands.title."))
        .count();
    assert_eq!(successes, 6);
}

#[test]
fn playsound_sends_an_inline_sound_and_uses_min_volume_far_away() {
    use mc_bamboo_data::packets::play::clientbound;

    let mut harness = Harness::new();
    let source = harness.join("Kitten", 2);
    // 原点在很远处，玩家自己听不到；minVolume>0 时把音源挪到玩家附近。
    harness.command(
        source,
        "playsound minecraft:entity.player.levelup master @s 1000 100 1000 1.0 1.0 0.5",
    );
    // 同一来源、默认参数：只写 sound。
    harness.command(source, "playsound minecraft:ui.button.click");
    harness.tick();

    let sounds: Vec<_> = harness
        .frames(source)
        .into_iter()
        .filter(|frame| frame.id == clientbound::SOUND)
        .collect();
    assert_eq!(sounds.len(), 2);
    // 内联 Holder：VarInt 0、名字、没有固定范围、来源 VarInt 0。
    assert_eq!(sounds[0].body[0], 0);
    assert!(contains_ascii(
        &sounds[0].body,
        "minecraft:entity.player.levelup"
    ));
    assert!(contains_ascii(&sounds[1].body, "minecraft:ui.button.click"));
    // 远处那条音量是 minVolume=0.5，位置被拉到玩家附近两格。
    let body = &sounds[0].body;
    assert!(contains_f32(body, 0.5), "minVolume 应该进音量字段");
}

fn contains_f32(body: &[u8], value: f32) -> bool {
    body.windows(4).any(|w| w == value.to_be_bytes())
}

#[test]
fn playsound_reports_failure_when_nobody_can_hear_it() {
    use mc_bamboo_data::packets::play::clientbound;

    let mut harness = Harness::new();
    let source = harness.join("Kitten", 2);
    harness.command(
        source,
        "playsound minecraft:entity.player.levelup master @s 1000 100 1000 1.0 1.0 0.0",
    );
    harness.tick();

    let frames = harness.frames(source);
    assert_eq!(
        frames
            .iter()
            .filter(|frame| frame.id == clientbound::SOUND)
            .count(),
        0
    );
    assert!(
        frames
            .iter()
            .any(|frame| frame.id == clientbound::SYSTEM_CHAT
                && contains_ascii(&frame.body, "commands.playsound.failed"))
    );
}

#[test]
fn particle_sends_a_level_particles_packet_to_nearby_players() {
    use mc_bamboo_data::packets::play::clientbound;

    let mut harness = Harness::new();
    let source = harness.join("Kitten", 2);
    harness.command(source, "particle flame");
    harness.command(
        source,
        "particle dust{color:[1.0,0.0,0.0],scale:1.0} ~ ~1 ~ 0 0 0 1.0 1 force",
    );
    harness.tick();

    let packets: Vec<_> = harness
        .frames(source)
        .into_iter()
        .filter(|frame| frame.id == clientbound::LEVEL_PARTICLES)
        .collect();
    assert_eq!(packets.len(), 2);
    assert_eq!(packets[0].body[0], 0x27, "flame 的注册表 ID");
    assert_eq!(packets[1].body[0], 0x15, "dust 的注册表 ID");
    assert_eq!(packets[1].body[9], 1, "force 写进 overrideLimiter");
    assert!(harness.frames(source).is_empty());
}

#[test]
fn particle_reports_failure_when_nobody_is_close_enough() {
    use mc_bamboo_data::packets::play::clientbound;

    let mut harness = Harness::new();
    let source = harness.join("Kitten", 2);
    harness.command(source, "particle flame 1000 100 1000");
    harness.tick();

    let frames = harness.frames(source);
    assert_eq!(
        frames
            .iter()
            .filter(|frame| frame.id == clientbound::LEVEL_PARTICLES)
            .count(),
        0
    );
    assert!(
        frames
            .iter()
            .any(|frame| frame.id == clientbound::SYSTEM_CHAT
                && contains_ascii(&frame.body, "commands.particle.failed"))
    );
}
