//! `/trigger`。
//!
//! 语法和反馈按官方 `TriggerCommand`：不要权限，只有玩家能用；目标必须是 `trigger`
//! 准则，并且这名玩家的分数已经被 `/scoreboard players enable` 解锁。每次使用先重新
//! 上锁，再加分或设分。
//!
//! `/trigger`, following official `TriggerCommand`: no permission level, players only.
//! The objective must use the `trigger` criterion and the player's score must have been
//! unlocked with `/scoreboard players enable`. Each use locks the score again and then
//! adds to it or sets it.

use std::{cell::RefCell, sync::Arc};

use mc_bamboo_command::{ParsedCommand, Value, argument::matches_sub_str};
use mc_bamboo_protocol::text::{Component, TranslateArg};

use crate::{
    command::{
        feedback::{self, CommandFail, fail, fail_with},
        source,
    },
    global::Global,
    inbound::PlayerId,
    region::Region,
    scoreboard::{Holder, Scoreboard},
};

/// 客户端收到的补全类型：官方 `.suggests(...)` 的节点都换成 `ask_server`。
///
/// The suggestion type sent to clients: official `.suggests(...)` nodes become `ask_server`.
pub(crate) const SUGGESTION: &str = "minecraft:ask_server";

thread_local! {
    /// 正在算补全的来源：计分板快照和来源玩家的名字。补全函数拿不到世界，
    /// 所以 `on_suggestion` 在调命令树之前把它放在这里。
    ///
    /// The source whose suggestions are being computed: the scoreboard snapshot and the
    /// source player's name. Suggestion functions cannot reach the world, so
    /// `on_suggestion` stores this before it asks the command tree.
    static ASKING: RefCell<Option<(Arc<Scoreboard>, String)>> = const { RefCell::new(None) };
}

/// 在一次补全计算期间记下来源。
///
/// Remember the source for the duration of one suggestion computation.
pub(crate) fn while_suggesting<T>(
    board: &Arc<Scoreboard>,
    player: &str,
    compute: impl FnOnce() -> T,
) -> T {
    ASKING.with(|asking| *asking.borrow_mut() = Some((Arc::clone(board), player.to_owned())));
    let result = compute();
    ASKING.with(|asking| *asking.borrow_mut() = None);
    result
}

/// 官方 `TriggerCommand.suggestObjectives`：只列出来源已解锁的 `trigger` 目标。
///
/// Official `TriggerCommand.suggestObjectives`: only the `trigger` objectives the source
/// has unlocked.
pub(crate) fn suggest_objectives(
    _arguments: &[(String, Value)],
    lower: &str,
    out: &mut Vec<String>,
) {
    ASKING.with(|asking| {
        let asking = asking.borrow();
        let Some((board, player)) = asking.as_ref() else {
            return;
        };
        for name in board.primed_triggers(player) {
            if matches_sub_str(lower, &name.to_lowercase()) {
                out.push(name.to_owned());
            }
        }
    });
}

/// 这次触发要做的事。
///
/// What one use of the trigger does.
#[derive(Clone, Copy)]
enum Action {
    Simple,
    Add(i32),
    Set(i32),
}

pub(crate) fn simple(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    trigger(global, region, id, parsed, Action::Simple)
}

pub(crate) fn add(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let amount = value(parsed)?;
    trigger(global, region, id, parsed, Action::Add(amount))
}

pub(crate) fn set(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let amount = value(parsed)?;
    trigger(global, region, id, parsed, Action::Set(amount))
}

fn value(parsed: &ParsedCommand) -> Result<i32, CommandFail> {
    parsed
        .get("value")
        .and_then(Value::as_integer)
        .ok_or_else(|| fail("command.failed"))
}

/// 官方 `CommandSourceStack.getPlayerOrException`：来源不是玩家就报
/// `permissions.requires.player`。
///
/// Official `CommandSourceStack.getPlayerOrException`: `permissions.requires.player`
/// unless the source is a player.
fn source_player(region: &Region, id: PlayerId) -> Result<String, CommandFail> {
    let player = match source::of(region, id).entity {
        Some(source::Entity::Player(player)) => region.players.get(&player),
        _ => None,
    };
    player
        .map(|player| player.name.clone())
        .ok_or_else(|| fail("permissions.requires.player"))
}

/// 官方 `ObjectiveArgument.getObjective`。
///
/// Official `ObjectiveArgument.getObjective`.
fn objective(global: &Global, parsed: &ParsedCommand) -> Result<String, CommandFail> {
    let name = parsed
        .get("objective")
        .and_then(Value::as_str)
        .ok_or_else(|| fail("command.failed"))?;
    if !global.scoreboard().objective_exists(name) {
        return Err(fail_with(
            "arguments.objective.notFound",
            [TranslateArg::from(name)],
        ));
    }
    Ok(name.to_owned())
}

/// 官方 `TriggerCommand.getScore`：检查准则和解锁状态，然后重新上锁。
///
/// Official `TriggerCommand.getScore`: check the criterion and the unlocked state, then
/// lock the score again.
fn take_score(global: &mut Global, player: &str, objective: &str) -> Result<(), CommandFail> {
    if !global.scoreboard().is_trigger(objective) {
        return Err(fail("commands.trigger.failed.invalid"));
    }
    if !global.scoreboard().is_primed(player, objective) {
        return Err(fail("commands.trigger.failed.unprimed"));
    }
    global.scoreboard_mut().lock(player, objective);
    Ok(())
}

fn trigger(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
    action: Action,
) -> Result<(), CommandFail> {
    let player = source_player(region, id)?;
    let objective = objective(global, parsed)?;
    take_score(global, &player, &objective)?;
    // 官方 `ScoreAccess.set` 在「显示自动更新」打开时把分数的显示名换成玩家的显示名。
    //
    // Official `ScoreAccess.set` replaces the score's display with the player's display
    // name while display auto-update is on.
    let holder = Holder {
        display: Some(global.scoreboard().format_player_name(&player)),
        name: player,
    };
    let formatted = global
        .scoreboard()
        .formatted_objective(&objective)
        .ok_or_else(|| fail("command.failed"))?;
    let board = global.scoreboard_mut();
    let message = match action {
        Action::Simple => {
            board.add_score(&holder, &objective, 1, region);
            Component::translate("commands.trigger.simple.success", [formatted.into()])
        }
        Action::Add(amount) => {
            board.add_score(&holder, &objective, amount, region);
            Component::translate(
                "commands.trigger.add.success",
                [formatted.into(), amount.into()],
            )
        }
        Action::Set(amount) => {
            board.set_score(&holder, &objective, amount, region);
            Component::translate(
                "commands.trigger.set.success",
                [formatted.into(), amount.into()],
            )
        }
    };
    feedback::success(region, id, message, true);
    Ok(())
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::packets::play::clientbound;

    use crate::{
        command::commands,
        world::tests::{Client, Harness, frames_with_id},
    };

    fn run(server: &mut Harness, client: &Client, text: &str) {
        server.request(
            client,
            mc_bamboo_protocol::packet::play::PlayRequest::ChatCommand {
                command: text.to_owned(),
                signed: None,
            },
        );
        server.tick();
    }

    fn joined(name: &str, level: u8) -> (Harness, Client) {
        let mut server = Harness::new(2);
        let client = server.join_and_settle(name);
        set_level(&mut server, &client, level);
        client.take_frames();
        (server, client)
    }

    fn set_level(server: &mut Harness, client: &Client, level: u8) {
        server
            .region()
            .players
            .get_mut(&client.id)
            .expect("the player is in the world")
            .permission_level = level;
    }

    fn has_key(client: &Client, key: &str) -> bool {
        frames_with_id(&client.take_frames(), clientbound::SYSTEM_CHAT)
            .iter()
            .any(|frame| {
                frame
                    .body
                    .windows(key.len())
                    .any(|window| window == key.as_bytes())
            })
    }

    fn score(server: &mut Harness, player: &str, objective: &str) -> Option<i32> {
        server.global().scoreboard().score_value(player, objective)
    }

    #[test]
    fn the_tree_needs_no_permission_level() {
        let graph = &commands().graph;
        for text in ["trigger t", "trigger t add 3", "trigger t set -2"] {
            assert!(graph.parse(text, 0).is_ok(), "{text}");
        }
        assert!(graph.parse("trigger", 0).is_err());
        assert!(graph.parse("trigger t add", 0).is_err());
    }

    #[test]
    fn a_trigger_needs_enabling_and_locks_itself_again() {
        let (mut server, client) = joined("Kitten", 2);
        run(&mut server, &client, "scoreboard objectives add t trigger");
        run(&mut server, &client, "scoreboard objectives add d dummy");
        set_level(&mut server, &client, 0);
        client.take_frames();

        run(&mut server, &client, "trigger t");
        assert!(has_key(&client, "commands.trigger.failed.unprimed"));
        run(&mut server, &client, "trigger d");
        assert!(has_key(&client, "commands.trigger.failed.invalid"));
        run(&mut server, &client, "trigger missing");
        assert!(has_key(&client, "arguments.objective.notFound"));

        set_level(&mut server, &client, 2);
        run(&mut server, &client, "scoreboard players enable Kitten t");
        set_level(&mut server, &client, 0);
        client.take_frames();
        run(&mut server, &client, "trigger t");
        assert!(has_key(&client, "commands.trigger.simple.success"));
        assert_eq!(score(&mut server, "Kitten", "t"), Some(1));
        run(&mut server, &client, "trigger t");
        assert!(has_key(&client, "commands.trigger.failed.unprimed"));
        assert_eq!(score(&mut server, "Kitten", "t"), Some(1));
    }

    #[test]
    fn add_and_set_write_the_value() {
        let (mut server, client) = joined("Kitten", 2);
        run(&mut server, &client, "scoreboard objectives add t trigger");
        run(&mut server, &client, "scoreboard players enable Kitten t");
        run(&mut server, &client, "trigger t add 5");
        assert!(has_key(&client, "commands.trigger.add.success"));
        assert_eq!(score(&mut server, "Kitten", "t"), Some(5));
        run(&mut server, &client, "scoreboard players enable Kitten t");
        run(&mut server, &client, "trigger t set -7");
        assert!(has_key(&client, "commands.trigger.set.success"));
        assert_eq!(score(&mut server, "Kitten", "t"), Some(-7));
    }

    #[test]
    fn suggestions_list_only_unlocked_triggers() {
        let (mut server, client) = joined("Kitten", 2);
        for text in [
            "scoreboard objectives add alpha trigger",
            "scoreboard objectives add beta trigger",
            "scoreboard objectives add gamma dummy",
            "scoreboard players enable Kitten beta",
            "scoreboard players set Kitten gamma 1",
        ] {
            run(&mut server, &client, text);
        }
        let region = server.region();
        let board = std::sync::Arc::clone(&region.global.scoreboard);
        let list = super::while_suggesting(&board, "Kitten", || {
            commands().graph.suggestions("trigger ", 8, 0, &[])
        });
        assert_eq!(list.values, ["beta"]);
        let list = super::while_suggesting(&board, "Other", || {
            commands().graph.suggestions("trigger ", 8, 0, &[])
        });
        assert!(list.values.is_empty());
    }
}
