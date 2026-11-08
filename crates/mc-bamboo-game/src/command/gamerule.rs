//! `/gamerule`。
//!
//! 语法、权限和反馈跟官方 `GameRuleCommand` 一样：等级 2，查询不广播，
//! 设置广播给其他 OP。值和现在一样时报 `commands.gamerule.not_set`。
//!
//! `/gamerule`.
//!
//! Grammar, permission and feedback follow official `GameRuleCommand`: level 2, a query
//! is not broadcast, and a change is broadcast to other ops. An unchanged value reports
//! `commands.gamerule.not_set`.

#[cfg(test)]
use crate::{command::Commands, gamerules::GameRuleSet};
#[cfg(test)]
use mc_bamboo_command::NodeKind;
use mc_bamboo_command::{ParsedCommand, Value};
use mc_bamboo_data::gamerules::RULES;
use mc_bamboo_protocol::text::Component;

use crate::{
    command::feedback::{self, CommandFail, fail, fail_with},
    gamerules::Assignment,
    global::Global,
    inbound::PlayerId,
    region::Region,
};

pub(crate) fn query(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let index = rule_index(parsed)?;
    let name = RULES[index].name;
    let value = global.rules().serialize(index);
    // `queryRule` 的 `sendSuccess(..., false)`：只给自己，不广播，不写日志。
    //
    // `queryRule`'s `sendSuccess(..., false)`: only the caller, no broadcast and no log.
    feedback::success(
        region,
        id,
        Component::translate("commands.gamerule.query", [name.into(), value.into()]),
        false,
    );
    Ok(())
}

pub(crate) fn set(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let index = rule_index(parsed)?;
    let Some(text) = parsed.get("value").and_then(canonical) else {
        return Err(fail("command.failed"));
    };
    let name = RULES[index].name;
    match global.rules_mut().assign(index, &text) {
        Assignment::Updated(value) => {
            // 先同步快照。`sendSuccess` 要读到刚写进去的 `send_command_feedback`。
            //
            // Publish first. `sendSuccess` must see the `send_command_feedback` just written.
            global.rules().publish(region);
            feedback::success(
                region,
                id,
                Component::translate("commands.gamerule.set", [name.into(), value.into()]),
                true,
            );
            Ok(())
        }
        Assignment::Unchanged(value) => Err(fail_with(
            "commands.gamerule.not_set",
            [name.into(), value.into()],
        )),
        Assignment::Invalid | Assignment::Disabled(_) => Err(fail("command.failed")),
    }
}

fn canonical(value: &Value) -> Option<String> {
    match value {
        Value::Bool(value) => Some(value.to_string()),
        Value::Integer(value) => Some(value.to_string()),
        _ => None,
    }
}

fn rule_index(parsed: &ParsedCommand) -> Result<usize, CommandFail> {
    commands_rule(parsed.action).ok_or_else(|| fail("command.failed"))
}

fn commands_rule(action: usize) -> Option<usize> {
    crate::command::commands()
        .rules
        .get(action)
        .copied()
        .flatten()
        .map(usize::from)
}

/// 测试用：这棵树里某个可执行节点对应的规则。
#[cfg(test)]
pub(crate) fn rule_of(commands: &Commands, action: usize) -> Option<u16> {
    commands.rules.get(action).copied().flatten()
}

/// 测试确认默认集合不含未启用的规则。
#[cfg(test)]
pub(crate) fn vanilla_has(name: &str) -> bool {
    GameRuleSet::vanilla()
        .enabled_values()
        .iter()
        .any(|(identifier, _)| {
            *identifier == name || identifier.strip_prefix("minecraft:") == Some(name)
        })
}

/// 根节点的子字面量，测试用来核对官方的注册顺序。
#[cfg(test)]
pub(crate) fn root_literals(commands: &Commands) -> Vec<String> {
    commands
        .graph
        .node(commands.graph.root())
        .children
        .iter()
        .filter_map(|&child| match &commands.graph.node(child).kind {
            NodeKind::Literal(name) => Some(name.clone()),
            NodeKind::Root | NodeKind::Argument { .. } => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::Vec3;
    use mc_bamboo_data::gamerules;
    use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode};
    use uuid::Uuid;

    use crate::{
        command::{self, commands},
        global::Global,
        inbound::PlayerId,
        player::Player,
        region::Region,
    };

    use super::{root_literals, rule_of, vanilla_has};

    #[test]
    fn the_command_sits_between_gamemode_and_give() {
        let commands = commands();
        let names = root_literals(commands);
        let gamemode = names.iter().position(|name| name == "gamemode").unwrap();
        let gamerule = names.iter().position(|name| name == "gamerule").unwrap();
        let give = names.iter().position(|name| name == "give").unwrap();
        assert_eq!(gamerule, gamemode + 1);
        assert_eq!(give, gamerule + 1);
        assert!(
            commands
                .graph
                .parse("gamerule random_tick_speed", 2)
                .is_ok()
        );
        assert!(
            commands
                .graph
                .parse("gamerule random_tick_speed", 1)
                .is_err()
        );
        assert!(
            commands
                .graph
                .parse("gamerule max_minecart_speed", 4)
                .is_err()
        );
        let parsed = commands
            .graph
            .parse("gamerule minecraft:random_tick_speed 4", 2)
            .unwrap();
        assert_eq!(
            rule_of(commands, parsed.action),
            Some(u16::try_from(gamerules::RANDOM_TICK_SPEED).unwrap())
        );
        assert!(vanilla_has("random_tick_speed"));
        assert!(!vanilla_has("max_minecart_speed"));
    }

    #[test]
    fn setting_a_rule_updates_the_region_snapshot() {
        let mut global = Global::default();
        let mut region = Region::new(8, 20, false);
        let mut player = Player::new(
            PlayerId(1),
            Uuid::nil(),
            "Op".to_owned(),
            GameMode::Creative,
            ClientInformation::default(),
            Vec3::new(0.0, 64.0, 0.0),
        );
        player.permission_level = 2;
        player.is_op = true;
        region.insert_player(player);

        command::on_command(
            &mut global,
            &mut region,
            PlayerId(1),
            "gamerule random_tick_speed 7",
        );
        assert_eq!(global.rules().int(gamerules::RANDOM_TICK_SPEED), 7);
        assert_eq!(region.block_rules.random_tick_speed, 7);
        command::on_command(
            &mut global,
            &mut region,
            PlayerId(1),
            "gamerule random_tick_speed 7",
        );
        assert_eq!(global.rules().int(gamerules::RANDOM_TICK_SPEED), 7);
        command::on_command(
            &mut global,
            &mut region,
            PlayerId(1),
            "gamerule tnt_explodes false",
        );
        assert!(!region.block_rules.tnt_explodes);
        command::on_command(
            &mut global,
            &mut region,
            PlayerId(1),
            "gamerule send_command_feedback false",
        );
        assert!(!region.send_command_feedback);
    }
}
