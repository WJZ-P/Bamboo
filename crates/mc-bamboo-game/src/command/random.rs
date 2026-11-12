//! `/random`：`value`、`roll`、`reset`。
//!
//! 随机流复用 `mc-bamboo-loot` 的 `RandomSequences`，种子规则按官方
//! `RandomSequences.createSequence`。`/random value|roll <range>` 不带序列名时
//! 用世界的 `Level.random`（Bamboo 的 `Region.random`）。

use mc_bamboo_blocks::random::RandomSource;
use mc_bamboo_command::{ParsedCommand, Value};
use mc_bamboo_protocol::{
    packet::play::SystemChat,
    text::{Color, Component},
};

use crate::{
    command::feedback::{self, CommandFail, fail},
    global::Global,
    inbound::PlayerId,
    links::encode_frames,
    region::Region,
};

/// 官方 `randomSample`：不带序列名时从 `Level.random` 取数，带序列名时从
/// 服务器的 `RandomSequences` 取。范围按 `MinMaxBounds.Ints` 解析。
pub(crate) fn sample(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
    roll: bool,
) -> Result<(), CommandFail> {
    let (min, max) = parsed
        .get("range")
        .and_then(Value::as_int_bounds)
        .ok_or_else(|| fail("command.failed"))?;
    let min = min.unwrap_or(i32::MIN);
    let max = max.unwrap_or(i32::MAX);
    let span = i64::from(max) - i64::from(min);
    if span <= 0 {
        return Err(fail("commands.random.error.range_too_small"));
    }
    if span >= i64::from(i32::MAX) {
        return Err(fail("commands.random.error.range_too_large"));
    }
    let value = match parsed.get("sequence").and_then(Value::as_str) {
        Some(name) => {
            let source = global.state().sequences.get(name);
            source.next_int(max - min + 1) + min
        }
        None => RandomSource::next_int_between_inclusive(region.random.source(), min, max),
    };
    if roll {
        let name = region
            .players
            .get(&id)
            .map(|player| Component::text(player.name.clone()))
            .unwrap_or_else(|| Component::text(""));
        let content = Component::translate(
            "commands.random.roll",
            [name.into(), value.into(), min.into(), max.into()],
        )
        .color(Color::Gray)
        .italic();
        // 官方 `RandomCommand` 走 `PlayerList.broadcastSystemMessage`，逐玩家看可见性。
        //
        // Official `RandomCommand` uses `PlayerList.broadcastSystemMessage`, which checks
        // visibility per player.
        let frames = encode_frames(&SystemChat {
            content: &content,
            overlay: false,
        });
        crate::chat::broadcast_system_message(region, &frames, None);
    } else {
        feedback::success(
            region,
            id,
            Component::translate("commands.random.sample.success", [value.into()]),
            false,
        );
    }
    Ok(())
}

pub(crate) fn value(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    sample(global, region, id, parsed, false)
}

pub(crate) fn roll(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    sample(global, region, id, parsed, true)
}

/// 官方 `resetSequence` / `resetAllSequences` / `resetAllSequencesAndSetNewDefaults`。
pub(crate) fn reset(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let sequences = &mut global.state_mut().sequences;
    let sequence_name = parsed.get("sequence").and_then(Value::as_str);
    let count = if let Some(salt) = parsed.get("seed").and_then(Value::as_integer) {
        // 官方的两条 reset 分支：`*` 改全局默认后清空；带名字的只替换这一条。
        let world = parsed
            .get("includeWorldSeed")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let sequence_id = parsed
            .get("includeSequenceId")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        if let Some(name) = sequence_name {
            sequences.reset_with(name, salt, world, sequence_id);
            feedback::success(
                region,
                id,
                Component::translate("commands.random.reset.success", [name.into()]),
                false,
            );
            return Ok(());
        }
        sequences.set_seed_defaults(salt, world, sequence_id);
        sequences.clear()
    } else {
        match sequence_name {
            Some(name) => {
                sequences.reset(name);
                feedback::success(
                    region,
                    id,
                    Component::translate("commands.random.reset.success", [name.into()]),
                    false,
                );
                return Ok(());
            }
            None => sequences.clear(),
        }
    };
    feedback::success(
        region,
        id,
        Component::translate("commands.random.reset.all.success", [(count as i32).into()]),
        false,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use mc_bamboo_blocks::random::RandomSource;
    use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode};
    use uuid::Uuid;

    use crate::{
        command::{self, commands},
        global::Global,
        inbound::PlayerId,
        player::Player,
        region::Region,
    };

    fn world() -> (Global, Region, PlayerId) {
        let mut region = Region::new(2, 20, false);
        let id = PlayerId(1);
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
        region.insert_player(player);
        (Global::default(), region, id)
    }

    #[test]
    fn named_sequences_share_a_stream_and_reset_returns_to_the_first_value() {
        let (mut global, mut region, id) = world();
        let expected = {
            let source = global.state().sequences.get("minecraft:test");
            let first = source.next_int(10);
            let second = source.next_int(10);
            (first, second)
        };
        global.state_mut().sequences.reset("minecraft:test");
        let source = global.state().sequences.get("minecraft:test");
        assert_eq!(source.next_int(10), expected.0);
        // 命令树里带名字的 value 也能解析。
        assert!(
            commands()
                .graph
                .parse("random value 1..10 minecraft:test", 2)
                .is_ok()
        );
        command::on_command(
            &mut global,
            &mut region,
            id,
            "random value 1..10 minecraft:test",
            None,
        );
        assert_eq!(global.state().sequences.len(), 1);
    }

    #[test]
    fn roll_broadcasts_and_reset_accepts_all_suffixes() {
        let (mut global, mut region, id) = world();
        command::on_command(&mut global, &mut region, id, "random roll 1..2", None);
        assert!(commands().graph.parse("random reset *", 2).is_ok());
        assert!(commands().graph.parse("random reset * 7", 2).is_ok());
        assert!(
            commands()
                .graph
                .parse("random reset * 7 false true", 2)
                .is_ok()
        );
        assert!(
            commands()
                .graph
                .parse("random reset minecraft:test 7 false true", 2)
                .is_ok()
        );
        assert!(commands().graph.parse("random value 1..", 2).is_ok());
        assert!(commands().graph.parse("random value ..5", 2).is_ok());
        assert!(commands().graph.parse("random value 5", 2).is_ok());
        assert!(commands().graph.parse("random value 5..3", 2).is_err());
    }
}
