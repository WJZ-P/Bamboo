//! `/team`。
//!
//! 语法、权限和反馈按官方 `TeamCommand`。队伍包的时机由 `ServerScoreboard` 决定：
//! 创建、删除、改属性和成员变化都广播 `set_player_team`。

use mc_bamboo_command::{ParsedCommand, ScoreHolderSpec, TeamColorId, Value, permission};
use mc_bamboo_protocol::{
    components::text::component_from_checked_tag,
    text::{Component, TranslateArg},
};

use crate::{
    command::{
        feedback::{self, CommandFail, fail, fail_with},
        select,
    },
    global::Global,
    inbound::PlayerId,
    region::Region,
};

fn args<const N: usize>(values: [TranslateArg; N]) -> Vec<TranslateArg> {
    values.into()
}

fn ok(
    region: &mut Region,
    id: PlayerId,
    key: &'static str,
    values: impl IntoIterator<Item = TranslateArg>,
    broadcast: bool,
) {
    feedback::success(region, id, Component::translate(key, values), broadcast);
}

fn team(global: &Global, parsed: &ParsedCommand) -> Result<String, CommandFail> {
    let name = parsed
        .get("team")
        .and_then(Value::as_str)
        .ok_or_else(|| fail("command.failed"))?;
    if !global.scoreboard().team_exists(name) {
        return Err(fail_with("team.notFound", args([name.into()])));
    }
    Ok(name.to_owned())
}

fn formatted(global: &Global, name: &str) -> Result<Component, CommandFail> {
    global
        .scoreboard()
        .formatted_team(name)
        .ok_or_else(|| fail("command.failed"))
}

fn component(parsed: &ParsedCommand, name: &str) -> Result<Component, CommandFail> {
    parsed
        .get(name)
        .and_then(Value::as_component_tag)
        .cloned()
        .map(component_from_checked_tag)
        .ok_or_else(|| fail("command.failed"))
}

/// 队伍成员也是 `score_holder`。`*` 是全部有分数的名字。
///
/// Team members are score holders too. `*` means every tracked score holder.
fn members(
    global: &Global,
    region: &Region,
    id: PlayerId,
    parsed: &ParsedCommand,
    name: &str,
) -> Result<Vec<String>, CommandFail> {
    let spec = parsed
        .get(name)
        .and_then(Value::as_score_holder)
        .ok_or_else(|| fail("command.failed"))?;
    let level = region
        .players
        .get(&id)
        .map(|player| player.permission_level)
        .unwrap_or(permission::ALL);
    let names = match spec {
        ScoreHolderSpec::Wildcard => global.scoreboard().tracked_players(),
        ScoreHolderSpec::Text(text) => vec![text.clone()],
        ScoreHolderSpec::Selector(selector) => {
            let ids = select::with_scoreboard(region, global.scoreboard(), id, level, selector)?;
            ids.iter()
                .filter_map(|player| region.players.get(player).map(|entry| entry.name.clone()))
                .collect()
        }
    };
    if names.is_empty() {
        return Err(fail("argument.scoreHolder.empty"));
    }
    Ok(names)
}

fn display_of(global: &Global, name: &str) -> Component {
    global.scoreboard().format_player_name(name)
}

pub(crate) fn list(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    _parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let board = global.scoreboard();
    let count = board.team_count();
    if count == 0 {
        ok(region, id, "commands.team.list.teams.empty", [], false);
    } else {
        let entries = crate::scoreboard::format_list(&board.team_entries());
        ok(
            region,
            id,
            "commands.team.list.teams.success",
            args([(count as i32).into(), entries.into()]),
            false,
        );
    }
    Ok(())
}

pub(crate) fn list_members(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let name = team(global, parsed)?;
    let shown = formatted(global, &name)?;
    let members = global
        .scoreboard()
        .team_members(&name)
        .unwrap_or_default()
        .to_vec();
    if members.is_empty() {
        ok(
            region,
            id,
            "commands.team.list.members.empty",
            args([shown.into()]),
            false,
        );
    } else {
        let list = crate::scoreboard::format_green_names(&members);
        ok(
            region,
            id,
            "commands.team.list.members.success",
            args([shown.into(), (members.len() as i32).into(), list.into()]),
            false,
        );
    }
    Ok(())
}

pub(crate) fn add(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let name = parsed
        .get("team")
        .and_then(Value::as_str)
        .ok_or_else(|| fail("command.failed"))?;
    if global.scoreboard().team_exists(name) {
        return Err(fail("commands.team.add.duplicate"));
    }
    let display = match parsed.get("displayName") {
        Some(_) => component(parsed, "displayName")?,
        None => Component::text(name),
    };
    global.scoreboard_mut().create_team(name, display, region);
    let shown = formatted(global, name)?;
    ok(
        region,
        id,
        "commands.team.add.success",
        args([shown.into()]),
        true,
    );
    Ok(())
}

pub(crate) fn remove(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let name = team(global, parsed)?;
    let shown = formatted(global, &name)?;
    global.scoreboard_mut().delete_team(&name, region);
    ok(
        region,
        id,
        "commands.team.remove.success",
        args([shown.into()]),
        true,
    );
    Ok(())
}

pub(crate) fn empty(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let name = team(global, parsed)?;
    let shown = formatted(global, &name)?;
    let removed = global
        .scoreboard_mut()
        .empty_team(&name, region)
        .ok_or_else(|| fail("command.failed"))?;
    if removed.is_empty() {
        return Err(fail("commands.team.empty.unchanged"));
    }
    ok(
        region,
        id,
        "commands.team.empty.success",
        args([(removed.len() as i32).into(), shown.into()]),
        true,
    );
    Ok(())
}

pub(crate) fn join(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let name = team(global, parsed)?;
    let shown = formatted(global, &name)?;
    let names = members(global, region, id, parsed, "members")?;
    let mut changed: i32 = 0;
    let mut only = String::new();
    for member in &names {
        if global.scoreboard_mut().join(member, &name, region) {
            changed += 1;
            only = member.clone();
        }
    }
    // 官方 `CommandResponseTracker` 没有 onZero，0 个变化时也发 multiple 消息。
    //
    // Official `CommandResponseTracker` has no onZero handler, so zero changes
    // still produce the multiple message.
    if changed == 1 {
        ok(
            region,
            id,
            "commands.team.join.success.single",
            args([display_of(global, &only).into(), shown.into()]),
            true,
        );
    } else {
        ok(
            region,
            id,
            "commands.team.join.success.multiple",
            args([changed.into(), shown.into()]),
            true,
        );
    }
    Ok(())
}

pub(crate) fn leave(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let names = members(global, region, id, parsed, "members")?;
    let mut changed: i32 = 0;
    let mut only = String::new();
    for member in &names {
        if global.scoreboard_mut().leave(member, region) {
            changed += 1;
            only = member.clone();
        }
    }
    // 同上：0 个变化时也发 multiple 消息。
    //
    // Same as above: zero changes still produce the multiple message.
    if changed == 1 {
        ok(
            region,
            id,
            "commands.team.leave.success.single",
            args([display_of(global, &only).into()]),
            true,
        );
    } else {
        ok(
            region,
            id,
            "commands.team.leave.success.multiple",
            args([changed.into()]),
            true,
        );
    }
    Ok(())
}

pub(crate) fn display_name(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let name = team(global, parsed)?;
    let display = component(parsed, "displayName")?;
    if global
        .scoreboard()
        .team_display(&name)
        .is_some_and(|current| current == &display)
    {
        return Err(fail("commands.team.option.name.unchanged"));
    }
    global
        .scoreboard_mut()
        .set_team_display(&name, display, region);
    let shown = formatted(global, &name)?;
    ok(
        region,
        id,
        "commands.team.option.name.success",
        args([shown.into()]),
        true,
    );
    Ok(())
}

pub(crate) fn prefix(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let name = team(global, parsed)?;
    let text = component(parsed, "prefix")?;
    global
        .scoreboard_mut()
        .set_team_prefix(&name, text.clone(), region);
    ok(
        region,
        id,
        "commands.team.option.prefix.success",
        args([text.into()]),
        false,
    );
    Ok(())
}

pub(crate) fn suffix(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let name = team(global, parsed)?;
    let text = component(parsed, "suffix")?;
    global
        .scoreboard_mut()
        .set_team_suffix(&name, text.clone(), region);
    ok(
        region,
        id,
        "commands.team.option.suffix.success",
        args([text.into()]),
        false,
    );
    Ok(())
}

pub(crate) fn color(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let name = team(global, parsed)?;
    let shown = formatted(global, &name)?;
    let color = parsed
        .get("value")
        .and_then(Value::as_team_color)
        .map(|color| color.0);
    let current = global.scoreboard().team_color(&name);
    if current == Some(color) {
        return Err(fail("commands.team.option.color.unchanged"));
    }
    global.scoreboard_mut().set_team_color(&name, color, region);
    let shown = global.scoreboard().formatted_team(&name).unwrap_or(shown);
    match color {
        Some(color) => ok(
            region,
            id,
            "commands.team.option.color.success",
            args([shown.into(), TeamColorId(color).name().into()]),
            true,
        ),
        None => ok(
            region,
            id,
            "commands.team.option.color.clear.success",
            args([shown.into()]),
            true,
        ),
    }
    Ok(())
}

pub(crate) fn clear_color(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    color(global, region, id, parsed)
}

fn flag(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
    friendly_fire: bool,
) -> Result<(), CommandFail> {
    let name = team(global, parsed)?;
    let shown = formatted(global, &name)?;
    let allowed = parsed
        .get("allowed")
        .and_then(Value::as_bool)
        .ok_or_else(|| fail("command.failed"))?;
    let current = global
        .scoreboard()
        .team_flag(&name, friendly_fire)
        .ok_or_else(|| fail("command.failed"))?;
    if current == allowed {
        let key = if friendly_fire {
            if allowed {
                "commands.team.option.friendlyfire.alreadyEnabled"
            } else {
                "commands.team.option.friendlyfire.alreadyDisabled"
            }
        } else if allowed {
            "commands.team.option.seeFriendlyInvisibles.alreadyEnabled"
        } else {
            "commands.team.option.seeFriendlyInvisibles.alreadyDisabled"
        };
        return Err(fail(key));
    }
    if friendly_fire {
        global
            .scoreboard_mut()
            .set_team_friendly_fire(&name, allowed, region);
    } else {
        global
            .scoreboard_mut()
            .set_team_see_invisibles(&name, allowed, region);
    }
    let key = if friendly_fire {
        if allowed {
            "commands.team.option.friendlyfire.enabled"
        } else {
            "commands.team.option.friendlyfire.disabled"
        }
    } else if allowed {
        "commands.team.option.seeFriendlyInvisibles.enabled"
    } else {
        "commands.team.option.seeFriendlyInvisibles.disabled"
    };
    ok(region, id, key, args([shown.into()]), true);
    Ok(())
}

pub(crate) fn friendly_fire(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    flag(global, region, id, parsed, true)
}

pub(crate) fn see_invisibles(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    flag(global, region, id, parsed, false)
}

/// 官方 `Team.Visibility` 和 `CollisionRule` 的四个值，id 与网络包一致。
///
/// The four official `Team.Visibility` / `CollisionRule` values, with the packet ids.
fn visibility(name: &str) -> Option<(i32, &'static str)> {
    Some(match name {
        "always" => (0, "team.visibility.always"),
        "never" => (1, "team.visibility.never"),
        "hideForOtherTeams" => (2, "team.visibility.hideForOtherTeams"),
        "hideForOwnTeam" => (3, "team.visibility.hideForOwnTeam"),
        _ => return None,
    })
}

fn collision(name: &str) -> Option<(i32, &'static str)> {
    Some(match name {
        "always" => (0, "team.collision.always"),
        "never" => (1, "team.collision.never"),
        "pushOtherTeams" => (2, "team.collision.pushOtherTeams"),
        "pushOwnTeam" => (3, "team.collision.pushOwnTeam"),
        _ => return None,
    })
}

fn visibility_change(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
    death: bool,
    name: &str,
) -> Result<(), CommandFail> {
    let team_name = team(global, parsed)?;
    let shown = formatted(global, &team_name)?;
    let (value, label) = visibility(name).ok_or_else(|| fail("command.failed"))?;
    let current = global
        .scoreboard()
        .team_visibility(&team_name, death)
        .ok_or_else(|| fail("command.failed"))?;
    if current == value {
        let key = if death {
            "commands.team.option.deathMessageVisibility.unchanged"
        } else {
            "commands.team.option.nametagVisibility.unchanged"
        };
        return Err(fail(key));
    }
    if !death {
        global
            .scoreboard_mut()
            .set_team_visibility(&team_name, value, region);
    } else {
        global
            .scoreboard_mut()
            .set_team_death(&team_name, value, region);
    }
    let key = if death {
        "commands.team.option.deathMessageVisibility.success"
    } else {
        "commands.team.option.nametagVisibility.success"
    };
    ok(
        region,
        id,
        key,
        args([shown.into(), Component::translate(label, []).into()]),
        true,
    );
    Ok(())
}

pub(crate) fn nametag_always(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    visibility_change(global, region, id, parsed, false, "always")
}

pub(crate) fn nametag_never(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    visibility_change(global, region, id, parsed, false, "never")
}

pub(crate) fn nametag_hide_other(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    visibility_change(global, region, id, parsed, false, "hideForOtherTeams")
}

pub(crate) fn nametag_hide_own(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    visibility_change(global, region, id, parsed, false, "hideForOwnTeam")
}

pub(crate) fn death_always(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    visibility_change(global, region, id, parsed, true, "always")
}

pub(crate) fn death_never(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    visibility_change(global, region, id, parsed, true, "never")
}

pub(crate) fn death_hide_other(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    visibility_change(global, region, id, parsed, true, "hideForOtherTeams")
}

pub(crate) fn death_hide_own(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    visibility_change(global, region, id, parsed, true, "hideForOwnTeam")
}

fn collision_change(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
    name: &str,
) -> Result<(), CommandFail> {
    let team_name = team(global, parsed)?;
    let shown = formatted(global, &team_name)?;
    let (value, label) = collision(name).ok_or_else(|| fail("command.failed"))?;
    if global
        .scoreboard()
        .team_collision(&team_name)
        .ok_or_else(|| fail("command.failed"))?
        == value
    {
        return Err(fail("commands.team.option.collisionRule.unchanged"));
    }
    global
        .scoreboard_mut()
        .set_team_collision(&team_name, value, region);
    ok(
        region,
        id,
        "commands.team.option.collisionRule.success",
        args([shown.into(), Component::translate(label, []).into()]),
        true,
    );
    Ok(())
}

pub(crate) fn collision_always(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    collision_change(global, region, id, parsed, "always")
}

pub(crate) fn collision_never(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    collision_change(global, region, id, parsed, "never")
}

pub(crate) fn collision_other(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    collision_change(global, region, id, parsed, "pushOtherTeams")
}

pub(crate) fn collision_own(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    collision_change(global, region, id, parsed, "pushOwnTeam")
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::packets::play::clientbound;

    use crate::{
        command::commands,
        world::tests::{Harness, frames_with_id},
    };

    fn run(server: &mut Harness, client: &crate::world::tests::Client, text: &str) {
        server.request(
            client,
            mc_bamboo_protocol::packet::play::PlayRequest::ChatCommand {
                command: text.to_owned(),
                signed: None,
            },
        );
        server.tick();
    }

    fn operator(name: &str) -> (Harness, crate::world::tests::Client) {
        let mut server = Harness::new(2);
        let client = server.join_and_settle(name);
        server
            .region()
            .players
            .get_mut(&client.id)
            .expect("玩家应该在世界里")
            .permission_level = 2;
        client.take_frames();
        (server, client)
    }

    fn system_text(client: &crate::world::tests::Client) -> Vec<Vec<u8>> {
        frames_with_id(&client.take_frames(), clientbound::SYSTEM_CHAT)
            .into_iter()
            .map(|frame| frame.body.clone())
            .collect()
    }

    fn has_key(bodies: &[Vec<u8>], key: &str) -> bool {
        bodies.iter().any(|body| {
            body.windows(key.len())
                .any(|window| window == key.as_bytes())
        })
    }

    #[test]
    fn command_tree_matches_the_official_shapes() {
        let graph = &commands().graph;
        for text in [
            "team list",
            "team list red",
            "team add red",
            r#"team add red {text:"Red"}"#,
            "team remove red",
            "team empty red",
            "team join red",
            "team join red Kitten",
            "team leave Kitten",
            r#"team modify red displayName {text:"R"}"#,
            r#"team modify red prefix {text:"["}"#,
            r#"team modify red suffix {text:"]"}"#,
            "team modify red color red",
            "team modify red color reset",
            "team modify red friendlyFire true",
            "team modify red seeFriendlyInvisibles false",
            "team modify red nametagVisibility never",
            "team modify red deathMessageVisibility hideForOwnTeam",
            "team modify red collisionRule pushOwnTeam",
        ] {
            assert!(graph.parse(text, 2).is_ok(), "{text}");
            assert!(graph.parse(text, 1).is_err(), "{text} at level 1");
        }
    }

    #[test]
    fn add_join_list_and_leave_report_the_official_keys() {
        let (mut server, client) = operator("Op");
        run(&mut server, &client, "team add red");
        assert!(has_key(&system_text(&client), "commands.team.add.success"));
        assert_eq!(server.global().scoreboard().team_count(), 1);
        run(&mut server, &client, "team add red");
        assert!(has_key(
            &system_text(&client),
            "commands.team.add.duplicate"
        ));

        run(&mut server, &client, "team join red Kitten");
        assert!(has_key(
            &system_text(&client),
            "commands.team.join.success.single"
        ));
        assert_eq!(server.global().scoreboard().team_of("Kitten"), Some("red"));
        run(&mut server, &client, "team list red");
        assert!(has_key(
            &system_text(&client),
            "commands.team.list.members.success"
        ));
        run(&mut server, &client, "team leave Kitten");
        assert!(has_key(
            &system_text(&client),
            "commands.team.leave.success.single"
        ));
        assert_eq!(server.global().scoreboard().team_of("Kitten"), None);
    }

    #[test]
    fn team_add_broadcasts_a_team_packet_and_join_replays_it() {
        let (mut server, client) = operator("Op");
        run(&mut server, &client, "team add red");
        run(&mut server, &client, "team join red Kitten");
        client.take_frames();
        run(&mut server, &client, "team modify red color blue");
        let frames = client.take_frames();
        assert!(
            frames
                .iter()
                .any(|frame| frame.id == clientbound::SET_PLAYER_TEAM)
        );

        let newcomer = server.join("Alex");
        server.tick();
        let frames = newcomer.take_frames();
        assert!(
            frames
                .iter()
                .any(|frame| frame.id == clientbound::SET_PLAYER_TEAM)
        );
    }

    #[test]
    fn options_reject_unchanged_values() {
        let (mut server, client) = operator("Op");
        run(&mut server, &client, "team add red");
        run(&mut server, &client, "team modify red color reset");
        assert!(has_key(
            &system_text(&client),
            "commands.team.option.color.unchanged"
        ));
        run(&mut server, &client, "team modify red color blue");
        assert!(has_key(
            &system_text(&client),
            "commands.team.option.color.success"
        ));
        run(
            &mut server,
            &client,
            "team modify red nametagVisibility always",
        );
        assert!(has_key(
            &system_text(&client),
            "commands.team.option.nametagVisibility.unchanged"
        ));
        run(&mut server, &client, "team modify red friendlyFire true");
        assert!(has_key(
            &system_text(&client),
            "commands.team.option.friendlyfire.alreadyEnabled"
        ));
    }

    #[test]
    fn unknown_team_reports_the_official_key() {
        let (mut server, client) = operator("Op");
        run(&mut server, &client, "team remove ghost");
        assert!(has_key(&system_text(&client), "team.notFound"));
    }
}
