//! 聊天的控制台记录。
//!
//! 这个测试单独一个进程：`tracing` 按日志调用点缓存要不要记录，缓存是全进程共享的。
//! 放进单元测试的话，别的线程先用“没有订阅者”的状态碰到同一条日志，这里就收不到了。

use std::{
    fmt,
    sync::{Arc, Mutex, atomic::AtomicUsize, mpsc},
};

use mc_bamboo_game::{InFlight, Inbound, JoinRequest, PlayerId, PlayerSink, World, WorldConfig};
use mc_bamboo_protocol::packet::play::{ClientInformation, PlayRequest};
use tracing::{
    Event, Level, Metadata, Subscriber,
    field::{Field, Visit},
    span::{Attributes, Id, Record},
    subscriber::with_default,
};
use uuid::Uuid;

/// 记下聊天模块的每一条日志。
#[derive(Clone, Default)]
struct Console(Arc<Mutex<Vec<(Level, String)>>>);

impl Subscriber for Console {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.target() == "mc_bamboo_game::chat"
    }

    fn event(&self, event: &Event<'_>) {
        let mut line = Line::default();
        event.record(&mut line);
        let level = *event.metadata().level();
        self.0.lock().unwrap().push((level, line.0));
    }

    fn new_span(&self, _span: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _span: &Id, _values: &Record<'_>) {}

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn enter(&self, _span: &Id) {}

    fn exit(&self, _span: &Id) {}
}

#[derive(Default)]
struct Line(String);

impl Visit for Line {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{value:?}");
        }
    }
}

struct DiscardingSink;

impl PlayerSink for DiscardingSink {
    fn send(&mut self, _frames: Vec<u8>) -> bool {
        true
    }

    fn close(&mut self) {}
}

#[test]
fn chat_is_logged_as_name_and_message_without_the_not_secure_tag() {
    let (inbox, receiver) = mpsc::channel();
    let config = WorldConfig {
        view_distance: 2,
        max_players: 20,
        online_mode: false,
    };
    let world = World::new(config, receiver);
    let player = PlayerId(1);
    let join = JoinRequest {
        id: player,
        uuid: Uuid::from_u128(1),
        name: "Kitten".to_owned(),
        properties: Vec::new(),
        client: ClientInformation::default(),
        sink: Box::new(DiscardingSink),
    };
    let quota = Arc::new(AtomicUsize::new(0));
    let chat = Inbound::Request {
        player,
        request: PlayRequest::Chat {
            message: "竹子 hi".to_owned(),
        },
        in_flight: InFlight::acquire(&quota, 1).expect("配额还没用"),
    };
    inbox.send(Inbound::Join(Box::new(join))).unwrap();
    inbox.send(chat).unwrap();
    inbox.send(Inbound::Shutdown).unwrap();

    let console = Console::default();
    with_default(console.clone(), || world.run());

    let lines = console.0.lock().unwrap();
    assert_eq!(*lines, [(Level::INFO, "<Kitten> 竹子 hi".to_owned())]);
}
