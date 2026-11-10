//! 世界设置类命令：难度、默认模式、种子、世界出生点和玩家重生点。
//!
//! 状态写在 [`Global`]，难度和世界出生点改动后按官方广播对应的包。

use mc_bamboo_command::{Coordinates, NodeKind, ParsedCommand, Value};
use mc_bamboo_core::BlockPos;
use mc_bamboo_protocol::text::{Component, TranslateArg};

use crate::{
    command::{
        feedback::{self, CommandFail, fail, fail_with},
        players, select,
    },
    global::Global,
    inbound::PlayerId,
    player::Player,
    region::{DIMENSION, Region},
    world_state::{Difficulty, FORCE_GAME_MODE, RespawnData},
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

/// 反馈世界种子。官方 `SeedCommand` 把 `String.valueOf(seed)` 做成可点击复制的
/// 组件，不广播给其他 OP。
pub(crate) fn seed(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = parsed;
    let text = global.state().seed.to_string();
    feedback::success(
        region,
        id,
        Component::translate(
            "commands.seed.success",
            [Component::copy_on_click(&text).into()],
        ),
        false,
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

/// 官方 `BlockPosArgument.getSpawnablePos`：超出世界边界时报 `argument.pos.outofbounds`。
fn spawnable_pos(pos: BlockPos) -> Result<BlockPos, CommandFail> {
    // `Level.isInSpawnableBounds`：水平坐标在 [-30000000, 30000000)，y 在世界高度内。
    const HORIZONTAL: i32 = 30_000_000;
    if pos.x < -HORIZONTAL
        || pos.x >= HORIZONTAL
        || pos.z < -HORIZONTAL
        || pos.z >= HORIZONTAL
        || pos.y < mc_bamboo_world::MIN_Y
        || pos.y >= mc_bamboo_world::MIN_Y + mc_bamboo_world::WORLD_HEIGHT
    {
        return Err(fail("argument.pos.outofbounds"));
    }
    Ok(pos)
}

/// 解析两个命令共用的 pos / rotation。不写坐标时用来源脚底位置，
/// 不写旋转时用官方的 `ZERO_ROTATION`。
fn target_spawn(
    region: &Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<RespawnData, CommandFail> {
    let player = region
        .players
        .get(&id)
        .ok_or_else(|| fail("permissions.requires.player"))?;
    let pos = if let Some(coords) = parsed.get("pos").and_then(Value::as_block_pos) {
        block_pos(coords, player)
    } else {
        BlockPos::containing(player.position())
    };
    let pos = spawnable_pos(pos)?;
    let (yaw, pitch) = parsed
        .get("rotation")
        .and_then(Value::as_rotation)
        .map(|rotation| rotation.resolve(player.yaw, player.pitch))
        .unwrap_or((0.0, 0.0));
    Ok(RespawnData::of(pos, yaw, pitch))
}

fn block_pos(coords: Coordinates, player: &Player) -> BlockPos {
    BlockPos::containing(coords.position(player.position(), player.yaw, player.pitch))
}

/// `/setworldspawn`：世界出生点，改动时按官方广播 `set_default_spawn_position`。
pub(crate) fn set_world_spawn(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let spawn = target_spawn(region, id, parsed)?;
    let frames = global.set_spawn(spawn);
    if !frames.is_empty() {
        region.links.broadcast(&frames);
    }
    feedback::success(
        region,
        id,
        Component::translate("commands.setworldspawn.success", spawn_args(spawn)),
        true,
    );
    Ok(())
}

/// `/spawnpoint`：只把 `RespawnData` 存到玩家状态，没有死亡重生。
///
/// 官方 `CommandResponseTracker` 按目标人数选 single/multiple 的反馈。
pub(crate) fn spawnpoint(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = global;
    let spawn = target_spawn(region, id, parsed)?;
    let targets = if let Some(selector) = parsed.get("targets").and_then(Value::as_entity) {
        select::players(region, id, players::level_of(region, id), selector)?
    } else {
        vec![id]
    };
    for target in &targets {
        if let Some(player) = region.players.get_mut(target) {
            player.respawn = Some(spawn);
        }
    }
    let args = spawn_args(spawn);
    let (key, last): (&str, TranslateArg) = match targets.as_slice() {
        [only] => (
            "commands.spawnpoint.success.single",
            region
                .players
                .get(only)
                .map(|player| Component::text(player.name.clone()).into())
                .unwrap_or_else(|| "".into()),
        ),
        _ => (
            "commands.spawnpoint.success.multiple",
            TranslateArg::from(targets.len() as i32),
        ),
    };
    let mut all = args.to_vec();
    all.push(last);
    feedback::success(region, id, Component::translate(key, all), true);
    Ok(())
}

/// 官方 `lambda$setSpawn$0` / `lambda$static$0` 里的六个参数：
/// x、y、z、yaw、pitch、维度。翻译键没用到的参数客户端自己忽略。
fn spawn_args(spawn: RespawnData) -> [TranslateArg; 6] {
    [
        spawn.pos.x.into(),
        spawn.pos.y.into(),
        spawn.pos.z.into(),
        TranslateArg::from(spawn.yaw),
        TranslateArg::from(spawn.pitch),
        DIMENSION.into(),
    ]
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
            play::{ChangeDifficulty, GameMode, SetDefaultSpawnPosition, SystemChat},
        },
    };
    use uuid::Uuid;

    use crate::{
        command::{self, commands},
        global::Global,
        inbound::{PlayerId, PlayerSink},
        outgoing::Outgoing,
        player::Player,
        region::Region,
        world_state::Difficulty,
    };

    #[derive(Default, Clone)]
    struct Sink(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

    impl PlayerSink for Sink {
        fn send(&mut self, frames: Outgoing) -> bool {
            self.0.lock().unwrap().extend(frames.to_raw_bytes());
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
    fn seed_prints_a_copyable_zero_and_needs_level_two() {
        let (mut global, mut region, id, sink) = world();
        command::on_command(&mut global, &mut region, id, "seed", None);
        region.links.flush();
        let sent = packets(&sink.0.lock().unwrap());
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].0, SystemChat::ID);
        let content = mc_bamboo_protocol::components::text::read(
            &mut mc_bamboo_protocol::components::cursor::Cursor::new(&sent[0].1),
        )
        .unwrap();
        let text = format!("{:?}", content.tag());
        assert!(text.contains("commands.seed.success"), "{text}");
        assert!(text.contains("copy_to_clipboard"), "{text}");
        assert!(text.contains("chat.square_brackets"), "{text}");
        assert!(text.contains("String(\"0\")"), "{text}");
        assert!(commands().graph.parse("seed", 0).is_err());
        assert!(commands().graph.parse("seed", 2).is_ok());
    }

    #[test]
    fn setworldspawn_broadcasts_and_updates_the_join_spawn() {
        let (mut global, mut region, id, sink) = world();
        command::on_command(
            &mut global,
            &mut region,
            id,
            "setworldspawn 10 -60 12 90 45",
            None,
        );
        let spawn = global.state().spawn;
        assert_eq!(spawn.pos, mc_bamboo_core::BlockPos::new(10, -60, 12));
        assert_eq!(spawn.yaw, 90.0);
        assert_eq!(spawn.pitch, 45.0);
        region.links.flush();
        let sent = packets(&sink.0.lock().unwrap());
        let mut body = Reader::new(&sent[0].1);
        assert_eq!(sent[0].0, SetDefaultSpawnPosition::ID);
        assert_eq!(body.string(32767).unwrap(), "minecraft:overworld");
        assert_eq!(body.i64().unwrap(), spawn.pos.pack());
        assert_eq!(body.f32().unwrap(), spawn.yaw);
        assert_eq!(body.f32().unwrap(), spawn.pitch);
        body.finish().unwrap();
        assert_eq!(sent[1].0, SystemChat::ID);

        // 同样的位置再执行一次不广播。
        sink.0.lock().unwrap().clear();
        command::on_command(
            &mut global,
            &mut region,
            id,
            "setworldspawn 10 -60 12 90 45",
            None,
        );
        region.links.flush();
        assert_eq!(packets(&sink.0.lock().unwrap()).len(), 1);
    }

    #[test]
    fn spawnpoint_stores_the_per_player_position() {
        let (mut global, mut region, id, _sink) = world();
        command::on_command(&mut global, &mut region, id, "spawnpoint", None);
        assert_eq!(
            region.players[&id].respawn.unwrap().pos,
            mc_bamboo_core::BlockPos::new(8, -59, 8)
        );
        command::on_command(
            &mut global,
            &mut region,
            id,
            "spawnpoint Kitten 3 -60 4",
            None,
        );
        assert_eq!(
            region.players[&id].respawn.unwrap().pos,
            mc_bamboo_core::BlockPos::new(3, -60, 4)
        );
        // 世界出生点不受玩家重生点影响。
        assert_eq!(global.state().spawn.pos.x, 8);
        assert!(commands().graph.parse("spawnpoint @a", 0).is_err());
    }

    #[test]
    fn permission_and_literals_follow_the_report() {
        assert!(commands().graph.parse("difficulty", 0).is_err());
        assert!(commands().graph.parse("difficulty peaceful", 2).is_ok());
        assert!(commands().graph.parse("difficulty hard", 2).is_ok());
        assert!(commands().graph.parse("difficulty unknown", 2).is_err());
    }
}
