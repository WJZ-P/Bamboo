//! `/weather`：世界天气状态与 `game_event` 包。
//!
//! 只做状态和包，不做方块侧的 `tickPrecipitation`（积雪、结冰、炼药锅接雨）。
//! 时间到点后的自动切换也不在世界循环里跑。

use mc_bamboo_blocks::random::RandomSource;
use mc_bamboo_command::{NodeKind, ParsedCommand, Value};
use mc_bamboo_protocol::{packet::play::GameEvent, text::Component};

use crate::{
    command::feedback::{self, CommandFail, fail},
    global::Global,
    inbound::PlayerId,
    links::encode_frames,
    region::Region,
};

/// 官方 `ServerLevel` 的三个 `IntProvider`。没写 duration 时按它们取随机数。
const RAIN_DELAY: (i32, i32) = (12_000, 180_000);
const RAIN_DURATION: (i32, i32) = (12_000, 24_000);
const THUNDER_DURATION: (i32, i32) = (3_600, 15_600);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Clear,
    Rain,
    Thunder,
}

/// 官方 `MinecraftServer.setWeatherParameters(clearTime, rainTime, raining, thundering)`。
pub(crate) fn weather(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let kind = parsed.nodes.iter().find_map(|&node| {
        let NodeKind::Literal(name) = &crate::command::commands().graph.node(node).kind else {
            return None;
        };
        match name.as_str() {
            "clear" => Some(Kind::Clear),
            "rain" => Some(Kind::Rain),
            "thunder" => Some(Kind::Thunder),
            _ => None,
        }
    });
    let Some(kind) = kind else {
        return Err(fail("command.failed"));
    };
    let duration = parsed
        .get("duration")
        .and_then(Value::as_time)
        .unwrap_or(-1);
    let duration = if duration == -1 {
        let (min, max) = match kind {
            Kind::Clear => RAIN_DELAY,
            Kind::Rain => RAIN_DURATION,
            Kind::Thunder => THUNDER_DURATION,
        };
        RandomSource::next_int_between_inclusive(region.random.source(), min, max)
    } else {
        duration
    };
    let mut state = global.state().weather;
    let was_raining = state.raining;
    match kind {
        Kind::Clear => {
            state.clear_time = duration;
            state.rain_time = 0;
            state.thunder_time = 0;
            state.raining = false;
            state.thundering = false;
        }
        Kind::Rain => {
            state.clear_time = 0;
            state.rain_time = duration;
            state.thunder_time = duration;
            state.raining = true;
            state.thundering = false;
        }
        Kind::Thunder => {
            state.clear_time = 0;
            state.rain_time = duration;
            state.thunder_time = duration;
            state.raining = true;
            state.thundering = true;
        }
    }
    global.state_mut().weather = state;
    broadcast(region, was_raining, state);
    let key = match kind {
        Kind::Clear => "commands.weather.set.clear",
        Kind::Rain => "commands.weather.set.rain",
        Kind::Thunder => "commands.weather.set.thunder",
    };
    feedback::success(region, id, Component::translate(key, []), true);
    Ok(())
}

/// 官方 `ServerLevel.updateWeather` 在状态变化时广播的四个包。
fn broadcast(region: &mut Region, was_raining: bool, state: crate::world_state::Weather) {
    let mut events = Vec::new();
    if was_raining != state.raining {
        events.push(GameEvent {
            event: if was_raining {
                GameEvent::STOP_RAINING
            } else {
                GameEvent::START_RAINING
            },
            param: 0.0,
        });
        events.push(GameEvent {
            event: GameEvent::RAIN_LEVEL_CHANGE,
            param: f32::from(u8::from(state.raining)),
        });
        events.push(GameEvent {
            event: GameEvent::THUNDER_LEVEL_CHANGE,
            param: f32::from(u8::from(state.thundering)),
        });
    } else if state.raining {
        events.push(GameEvent {
            event: GameEvent::THUNDER_LEVEL_CHANGE,
            param: f32::from(u8::from(state.thundering)),
        });
    }
    let mut frames = Vec::new();
    for event in &events {
        frames.extend(encode_frames(event));
    }
    region.links.broadcast(&frames);
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use mc_bamboo_protocol::{
        codec::Reader,
        frame::{MAX_FRAME_LEN, find_frame},
        packet::play::{ClientInformation, GameMode},
    };
    use uuid::Uuid;

    use crate::{
        command::{self, commands},
        global::Global,
        inbound::{PlayerId, PlayerSink},
        player::Player,
        region::Region,
    };

    #[derive(Default, Clone)]
    struct Sink(Arc<Mutex<Vec<u8>>>);

    impl PlayerSink for Sink {
        fn send(&mut self, frames: Vec<u8>) -> bool {
            self.0.lock().unwrap().extend(frames);
            true
        }

        fn close(&mut self) {}
    }

    fn world() -> (Global, Region, PlayerId, Sink) {
        let mut region = Region::new(2, 20, false);
        let id = PlayerId(1);
        let sink = Sink::default();
        let mut player = Player::new(
            id,
            Uuid::from_u128(1),
            "Kitten".to_owned(),
            GameMode::Creative,
            ClientInformation::default(),
            mc_bamboo_core::Vec3::new(8.5, -59.0, 8.5),
        );
        player.permission_level = 2;
        player.is_op = true;
        region.links.add(id, Box::new(sink.clone()));
        region.insert_player(player);
        (Global::default(), region, id, sink)
    }

    fn events(bytes: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut rest = bytes;
        while !rest.is_empty() {
            let bounds = find_frame(rest, MAX_FRAME_LEN).unwrap().unwrap();
            let end = bounds.header_len + bounds.payload_len;
            let mut reader = Reader::new(&rest[bounds.header_len..end]);
            let _id = reader.varint().unwrap();
            if let Ok(event) = reader.u8() {
                out.push(event);
            }
            rest = &rest[end..];
        }
        out
    }

    #[test]
    fn rain_then_clear_sends_the_official_game_events() {
        let (mut global, mut region, id, sink) = world();
        command::on_command(&mut global, &mut region, id, "weather rain 100", None);
        assert!(global.state().weather.raining);
        assert_eq!(global.state().weather.rain_time, 100);
        region.links.flush();
        assert!(
            events(&sink.0.lock().unwrap())
                .into_iter()
                .filter(|&event| event != 0)
                .count()
                >= 3
        );
        sink.0.lock().unwrap().clear();
        command::on_command(&mut global, &mut region, id, "weather clear 50", None);
        assert!(!global.state().weather.raining);
        assert_eq!(global.state().weather.clear_time, 50);
        region.links.flush();
        let events = events(&sink.0.lock().unwrap());
        assert!(events.contains(&mc_bamboo_protocol::packet::play::GameEvent::STOP_RAINING));
    }

    #[test]
    fn thunder_and_tree_shapes() {
        let (mut global, mut region, id, _sink) = world();
        command::on_command(&mut global, &mut region, id, "weather thunder", None);
        assert!(global.state().weather.thundering);
        assert!(commands().graph.parse("weather rain 1", 2).is_ok());
        assert!(commands().graph.parse("weather rain 0", 2).is_err());
        assert!(commands().graph.parse("weather clear", 0).is_err());
    }
}
