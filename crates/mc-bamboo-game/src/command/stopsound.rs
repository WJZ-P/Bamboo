//! `/stopsound`：给选中的玩家发 `ClientboundStopSoundPacket`。

use mc_bamboo_data::sounds::SoundSource;
use mc_bamboo_protocol::{packet::play::StopSound, text::Component};

use mc_bamboo_command::{NodeKind, ParsedCommand, Value};

use crate::{
    command::{
        feedback::{self, CommandFail, fail},
        players, select,
    },
    global::Global,
    inbound::PlayerId,
    links::encode_frames,
    region::Region,
};

/// 官方 `SoundSource.values()` 的顺序和名字。
const SOURCES: [(&str, SoundSource); 11] = [
    ("master", mc_bamboo_data::sounds::MASTER),
    ("music", mc_bamboo_data::sounds::MUSIC),
    ("record", mc_bamboo_data::sounds::RECORDS),
    ("weather", mc_bamboo_data::sounds::WEATHER),
    ("block", mc_bamboo_data::sounds::BLOCKS),
    ("hostile", mc_bamboo_data::sounds::HOSTILE),
    ("neutral", mc_bamboo_data::sounds::NEUTRAL),
    ("player", mc_bamboo_data::sounds::PLAYERS),
    ("ambient", mc_bamboo_data::sounds::AMBIENT),
    ("voice", mc_bamboo_data::sounds::VOICE),
    ("ui", mc_bamboo_data::sounds::UI),
];

pub(crate) fn stop_sound(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = global;
    let selector = parsed
        .get("targets")
        .and_then(Value::as_entity)
        .ok_or_else(|| fail("command.failed"))?;
    let targets = select::players(region, id, players::level_of(region, id), selector)?;
    // 命令树里的来源字面量（`*` 表示不带来源），以及可选的 `sound` 参数。
    let source = parsed.nodes.iter().rev().find_map(|&node| {
        let NodeKind::Literal(name) = &crate::command::commands().graph.node(node).kind else {
            return None;
        };
        if name == "*" {
            return Some(None);
        }
        SOURCES
            .iter()
            .find(|(text, _)| *text == name)
            .map(|(_, source)| Some(*source))
    });
    let sound = parsed.get("sound").and_then(Value::as_str);
    let frames = encode_frames(&StopSound {
        source: source.flatten(),
        sound,
    });
    for target in &targets {
        region.links.send_frames(*target, &frames);
    }
    let name = source.flatten().and_then(|source| {
        SOURCES
            .iter()
            .find(|(_, candidate)| *candidate == source)
            .map(|(name, _)| *name)
    });
    let message = match (name, sound) {
        (Some(name), Some(sound)) => Component::translate(
            "commands.stopsound.success.source.sound",
            [Component::text(sound).into(), name.into()],
        ),
        (Some(name), None) => {
            Component::translate("commands.stopsound.success.source.any", [name.into()])
        }
        (None, Some(sound)) => Component::translate(
            "commands.stopsound.success.sourceless.sound",
            [Component::text(sound).into()],
        ),
        (None, None) => Component::translate("commands.stopsound.success.sourceless.any", []),
    };
    feedback::success(region, id, message, true);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use mc_bamboo_protocol::{
        codec::Reader,
        frame::{MAX_FRAME_LEN, find_frame},
        packet::{
            ClientboundPacket,
            play::{ClientInformation, GameMode, StopSound, SystemChat},
        },
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

    fn packets(bytes: &[u8]) -> Vec<(i32, Vec<u8>)> {
        let mut out = Vec::new();
        let mut rest = bytes;
        while !rest.is_empty() {
            let bounds = find_frame(rest, MAX_FRAME_LEN).unwrap().unwrap();
            let end = bounds.header_len + bounds.payload_len;
            let mut reader = Reader::new(&rest[bounds.header_len..end]);
            let id = reader.varint().unwrap();
            out.push((id, reader.rest().to_vec()));
            rest = &rest[end..];
        }
        out
    }

    #[test]
    fn source_and_sound_are_encoded_in_the_packet() {
        let (mut global, mut region, id, sink) = world();
        command::on_command(
            &mut global,
            &mut region,
            id,
            "stopsound Kitten block minecraft:block.stone.break",
            None,
        );
        region.links.flush();
        let sent = packets(&sink.0.lock().unwrap());
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0].0, StopSound::ID);
        assert_eq!(sent[0].1[0], 3);
        assert_eq!(sent[0].1[1], 4);
        assert_eq!(&sent[0].1[3..], b"minecraft:block.stone.break");
        assert_eq!(sent[1].0, SystemChat::ID);
    }

    #[test]
    fn wildcard_and_bare_target_send_no_optional_fields() {
        let (mut global, mut region, id, sink) = world();
        command::on_command(&mut global, &mut region, id, "stopsound Kitten", None);
        region.links.flush();
        let sent = packets(&sink.0.lock().unwrap());
        assert_eq!(sent[0], (StopSound::ID, vec![0]));
        assert!(commands().graph.parse("stopsound Kitten", 0).is_err());
    }
}
