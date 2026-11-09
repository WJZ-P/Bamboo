//! 世界设置类命令：`/difficulty`。
//!
//! 语法、权限和反馈跟官方 `DifficultyCommand` 一致；状态写在 [`Global`]，
//! 改动后按官方 `MinecraftServer.setDifficulty` 给所有人发 `change_difficulty`。

use mc_bamboo_command::{NodeKind, ParsedCommand, Value};
use mc_bamboo_protocol::text::Component;

use crate::{
    command::{
        feedback::{self, CommandFail, fail, fail_with},
        players,
    },
    global::Global,
    inbound::PlayerId,
    region::Region,
    world_state::{Difficulty, FORCE_GAME_MODE},
};

/// 查询当前难度。根节点可执行，官方 `sendSuccess(..., false)`。
pub(crate) fn query_difficulty(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = parsed;
    let name = global.difficulty_name();
    feedback::success(
        region,
        id,
        Component::translate(
            "commands.difficulty.query",
            [Component::translate(name, []).into()],
        ),
        false,
    );
    Ok(())
}

/// 设置难度。当前难度相同时报 `commands.difficulty.failure`。
///
/// 官方 `setDifficulty` 用 `force=true` 调 `MinecraftServer.setDifficulty`，所以
/// `difficulty_locked` 不阻止这条命令；改动后无条件广播难度包。
pub(crate) fn set_difficulty(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
    difficulty: Difficulty,
) -> Result<(), CommandFail> {
    let _ = parsed;
    if global.state().difficulty == difficulty {
        return Err(fail_with(
            "commands.difficulty.failure",
            [Component::translate(difficulty.display_name(), []).into()],
        ));
    }
    global.set_difficulty(difficulty);
    region.links.broadcast(&global.difficulty_frames());
    feedback::success(
        region,
        id,
        Component::translate(
            "commands.difficulty.success",
            [Component::translate(difficulty.display_name(), []).into()],
        ),
        true,
    );
    Ok(())
}

/// 修改默认游戏模式。官方 `setMode` 先写默认值，再在 `force-gamemode` 为真时
/// 把在线玩家切成新模式；专用服务器默认不强制，所以这里只在那个常量为真时改人。
pub(crate) fn default_gamemode(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let Some(mode) = parsed.get("gamemode").and_then(Value::as_game_mode) else {
        return Err(fail("command.failed"));
    };
    let protocol = players::protocol_mode(mode);
    global.state_mut().default_game_mode = protocol;
    if FORCE_GAME_MODE {
        let online: Vec<PlayerId> = region.players.keys().copied().collect();
        for target in online {
            players::apply_mode(global, region, target, target, mode, protocol);
        }
    }
    let name = Component::translate(format!("gameMode.{}", mode.name()), []);
    feedback::success(
        region,
        id,
        Component::translate("commands.defaultgamemode.success", [name.into()]),
        true,
    );
    Ok(())
}

/// 四个难度字面量共用的入口，难度从命令树的字面量名字查出来。
pub(crate) fn difficulty_literal(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let name = match &crate::command::commands().graph.node(parsed.action).kind {
        NodeKind::Literal(name) => name.as_str(),
        _ => return Err(fail("command.failed")),
    };
    let Some(difficulty) = Difficulty::ALL
        .into_iter()
        .find(|difficulty| difficulty.name() == name)
    else {
        return Err(fail("command.failed"));
    };
    set_difficulty(global, region, id, parsed, difficulty)
}

#[cfg(test)]
mod tests {
    use mc_bamboo_protocol::{
        codec::Reader,
        frame::{MAX_FRAME_LEN, find_frame},
        packet::{
            ClientboundPacket,
            play::{ChangeDifficulty, GameMode, SystemChat},
        },
    };
    use uuid::Uuid;

    use crate::{
        command::{self, commands},
        global::Global,
        inbound::{PlayerId, PlayerSink},
        player::Player,
        region::Region,
        world_state::Difficulty,
    };

    #[derive(Default, Clone)]
    struct Sink(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

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
            mc_bamboo_protocol::packet::play::ClientInformation::default(),
            mc_bamboo_core::Vec3::new(8.5, -59.0, 8.5),
        );
        player.permission_level = 2;
        player.is_op = true;
        region.links.add(id, Box::new(sink.clone()));
        region.insert_player(player);
        (Global::default(), region, id, sink)
    }

    /// 把假客户端收到的帧拆成 (包 ID, body)。
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
    fn querying_reports_easy_and_setting_broadcasts_the_new_difficulty() {
        let (mut global, mut region, id, sink) = world();
        assert_eq!(global.state().difficulty, Difficulty::Easy);

        command::on_command(&mut global, &mut region, id, "difficulty", None);
        region.links.flush();
        let sent = packets(&sink.0.lock().unwrap());
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].0, SystemChat::ID);
        let content = mc_bamboo_protocol::components::text::read(
            &mut mc_bamboo_protocol::components::cursor::Cursor::new(&sent[0].1),
        )
        .unwrap();
        assert!(format!("{:?}", content.tag()).contains("commands.difficulty.query"));

        sink.0.lock().unwrap().clear();
        command::on_command(&mut global, &mut region, id, "difficulty hard", None);
        assert_eq!(global.state().difficulty, Difficulty::Hard);
        region.links.flush();
        let sent = packets(&sink.0.lock().unwrap());
        assert_eq!(
            sent[0],
            (ChangeDifficulty::ID, vec![Difficulty::Hard as u8, 0])
        );
        assert_eq!(sent[1].0, SystemChat::ID);
    }

    #[test]
    fn unchanged_difficulty_fails_without_a_broadcast() {
        let (mut global, mut region, id, sink) = world();
        command::on_command(&mut global, &mut region, id, "difficulty easy", None);
        region.links.flush();
        let sent = packets(&sink.0.lock().unwrap());
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].0, SystemChat::ID);
        assert_eq!(global.state().difficulty, Difficulty::Easy);
    }

    #[test]
    fn default_gamemode_changes_the_state_without_touching_online_players() {
        let (mut global, mut region, id, _sink) = world();
        assert_eq!(
            global.state().default_game_mode,
            mc_bamboo_protocol::packet::play::GameMode::Creative
        );
        command::on_command(
            &mut global,
            &mut region,
            id,
            "defaultgamemode survival",
            None,
        );
        assert_eq!(
            global.state().default_game_mode,
            mc_bamboo_protocol::packet::play::GameMode::Survival
        );
        // `force-gamemode` 默认关闭，在线玩家的模式不变。
        assert_eq!(
            region.players[&id].game_mode,
            mc_bamboo_protocol::packet::play::GameMode::Creative
        );
        assert!(
            commands()
                .graph
                .parse("defaultgamemode creative", 0)
                .is_err()
        );
        assert!(commands().graph.parse("defaultgamemode nope", 2).is_err());
    }

    #[test]
    fn permission_and_literals_follow_the_report() {
        assert!(commands().graph.parse("difficulty", 0).is_err());
        assert!(commands().graph.parse("difficulty peaceful", 2).is_ok());
        assert!(commands().graph.parse("difficulty hard", 2).is_ok());
        assert!(commands().graph.parse("difficulty unknown", 2).is_err());
    }
}
