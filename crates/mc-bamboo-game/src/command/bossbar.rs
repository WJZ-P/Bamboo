//! `/bossbar`。
//!
//! 语法、权限和反馈按官方 `BossBarCommands`。包由 `ServerBossEvent` 的时机发：
//! 加人发 add，移除发 remove，改属性按动作发 update，隐藏时整体切换 add/remove。

use mc_bamboo_command::{ParsedCommand, Value};
use mc_bamboo_protocol::{
    components::text::component_from_checked_tag,
    text::{Component, TranslateArg},
};
use uuid::Uuid;

use crate::{
    command::{
        feedback::{self, CommandFail, fail, fail_with},
        select,
    },
    global::Global,
    inbound::PlayerId,
    region::Region,
    scoreboard::{BossColor, BossStyle},
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

fn identifier(parsed: &ParsedCommand) -> Result<String, CommandFail> {
    parsed
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_owned)
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

fn amount(parsed: &ParsedCommand, name: &str) -> Result<i32, CommandFail> {
    parsed
        .get(name)
        .and_then(Value::as_integer)
        .ok_or_else(|| fail("command.failed"))
}

/// 官方 `BossBarCommands.getBossBar`：不存在时是 `commands.bossbar.unknown`。
///
/// Official `BossBarCommands.getBossBar`: a missing bar reports `commands.bossbar.unknown`.
fn bar(global: &Global, parsed: &ParsedCommand) -> Result<String, CommandFail> {
    let id = identifier(parsed)?;
    if global.boss_bars().bar(&id).is_none() {
        return Err(fail_with("commands.bossbar.unknown", args([id.into()])));
    }
    Ok(id)
}

fn shown(global: &Global, id: &str) -> Result<Component, CommandFail> {
    global
        .boss_bars()
        .display_name(id)
        .ok_or_else(|| fail("command.failed"))
}

pub(crate) fn add(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let name = identifier(parsed)?;
    let display = component(parsed, "name")?;
    if !global.boss_bars_mut().create(name.clone(), display) {
        return Err(fail_with(
            "commands.bossbar.create.failed",
            args([name.into()]),
        ));
    }
    let shown = shown(global, &name)?;
    ok(
        region,
        id,
        "commands.bossbar.create.success",
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
    let name = bar(global, parsed)?;
    let shown = shown(global, &name)?;
    global.boss_bars_mut().remove(&name, region);
    ok(
        region,
        id,
        "commands.bossbar.remove.success",
        args([shown.into()]),
        true,
    );
    Ok(())
}

pub(crate) fn list(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    _parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let bars = global.boss_bars();
    let count = bars.ids().len();
    if count == 0 {
        ok(region, id, "commands.bossbar.list.bars.none", [], false);
    } else {
        let entries = crate::scoreboard::format_list(&bars.entries());
        ok(
            region,
            id,
            "commands.bossbar.list.bars.some",
            args([(count as i32).into(), entries.into()]),
            false,
        );
    }
    Ok(())
}

pub(crate) fn get_value(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let name = bar(global, parsed)?;
    let shown = shown(global, &name)?;
    let value = global
        .boss_bars()
        .value(&name)
        .ok_or_else(|| fail("command.failed"))?;
    ok(
        region,
        id,
        "commands.bossbar.get.value",
        args([shown.into(), value.into()]),
        true,
    );
    Ok(())
}

pub(crate) fn get_max(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let name = bar(global, parsed)?;
    let shown = shown(global, &name)?;
    let max = global
        .boss_bars()
        .max(&name)
        .ok_or_else(|| fail("command.failed"))?;
    ok(
        region,
        id,
        "commands.bossbar.get.max",
        args([shown.into(), max.into()]),
        true,
    );
    Ok(())
}

pub(crate) fn get_visible(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let name = bar(global, parsed)?;
    let shown = shown(global, &name)?;
    let visible = global
        .boss_bars()
        .visible(&name)
        .ok_or_else(|| fail("command.failed"))?;
    let key = if visible {
        "commands.bossbar.get.visible.visible"
    } else {
        "commands.bossbar.get.visible.hidden"
    };
    ok(region, id, key, args([shown.into()]), true);
    Ok(())
}

pub(crate) fn get_players(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let name = bar(global, parsed)?;
    let shown = shown(global, &name)?;
    let names = global
        .boss_bars()
        .online_names(&name, region)
        .ok_or_else(|| fail("command.failed"))?;
    if names.is_empty() {
        ok(
            region,
            id,
            "commands.bossbar.get.players.none",
            args([shown.into()]),
            true,
        );
    } else {
        let list = crate::scoreboard::format_green_names(&names);
        ok(
            region,
            id,
            "commands.bossbar.get.players.some",
            args([shown.into(), (names.len() as i32).into(), list.into()]),
            true,
        );
    }
    Ok(())
}

pub(crate) fn set_name(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let name = bar(global, parsed)?;
    let display = component(parsed, "name")?;
    if global.boss_bars_mut().set_name(&name, display, region) == Some(false) {
        return Err(fail("commands.bossbar.set.name.unchanged"));
    }
    let shown = shown(global, &name)?;
    ok(
        region,
        id,
        "commands.bossbar.set.name.success",
        args([shown.into()]),
        true,
    );
    Ok(())
}

pub(crate) fn set_color(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
    color: BossColor,
) -> Result<(), CommandFail> {
    let name = bar(global, parsed)?;
    if global.boss_bars_mut().set_color(&name, color, region) == Some(false) {
        return Err(fail("commands.bossbar.set.color.unchanged"));
    }
    let shown = shown(global, &name)?;
    ok(
        region,
        id,
        "commands.bossbar.set.color.success",
        args([shown.into()]),
        true,
    );
    Ok(())
}

pub(crate) fn set_style(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
    style: BossStyle,
) -> Result<(), CommandFail> {
    let name = bar(global, parsed)?;
    if global.boss_bars_mut().set_style(&name, style, region) == Some(false) {
        return Err(fail("commands.bossbar.set.style.unchanged"));
    }
    let shown = shown(global, &name)?;
    ok(
        region,
        id,
        "commands.bossbar.set.style.success",
        args([shown.into()]),
        true,
    );
    Ok(())
}

pub(crate) fn set_value(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let name = bar(global, parsed)?;
    let value = amount(parsed, "value")?;
    if global.boss_bars_mut().set_value(&name, value, region) == Some(false) {
        return Err(fail("commands.bossbar.set.value.unchanged"));
    }
    let shown = shown(global, &name)?;
    ok(
        region,
        id,
        "commands.bossbar.set.value.success",
        args([shown.into(), value.into()]),
        true,
    );
    Ok(())
}

pub(crate) fn set_max(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let name = bar(global, parsed)?;
    let max = amount(parsed, "max")?;
    if global.boss_bars_mut().set_max(&name, max, region) == Some(false) {
        return Err(fail("commands.bossbar.set.max.unchanged"));
    }
    let shown = shown(global, &name)?;
    ok(
        region,
        id,
        "commands.bossbar.set.max.success",
        args([shown.into(), max.into()]),
        true,
    );
    Ok(())
}

pub(crate) fn set_visible(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let name = bar(global, parsed)?;
    let visible = parsed
        .get("visible")
        .and_then(Value::as_bool)
        .ok_or_else(|| fail("command.failed"))?;
    if global.boss_bars_mut().set_visible(&name, visible, region) == Some(false) {
        let key = if visible {
            "commands.bossbar.set.visibility.unchanged.visible"
        } else {
            "commands.bossbar.set.visibility.unchanged.hidden"
        };
        return Err(fail(key));
    }
    let shown = shown(global, &name)?;
    let key = if visible {
        "commands.bossbar.set.visible.success.visible"
    } else {
        "commands.bossbar.set.visible.success.hidden"
    };
    ok(region, id, key, args([shown.into()]), true);
    Ok(())
}

/// 官方 `setPlayers`：`@a` 之类实体选择器解析成在线玩家，再按 UUID 比对。
///
/// Official `setPlayers`: entity selectors resolve to online players, compared by UUID.
fn targets(
    region: &Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<Vec<(PlayerId, Uuid)>, CommandFail> {
    let selector = parsed
        .get("targets")
        .and_then(Value::as_entity)
        .ok_or_else(|| fail("command.failed"))?;
    let level = region
        .players
        .get(&id)
        .map(|player| player.permission_level)
        .unwrap_or(mc_bamboo_command::permission::ALL);
    let ids = select::players(region, id, level, selector)?;
    Ok(ids
        .iter()
        .filter_map(|player| {
            region
                .players
                .get(player)
                .map(|entry| (*player, entry.uuid))
        })
        .collect())
}

pub(crate) fn set_players(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let name = bar(global, parsed)?;
    let shown = shown(global, &name)?;
    let targets = match parsed.get("targets") {
        Some(_) => targets(region, id, parsed)?,
        None => Vec::new(),
    };
    if global.boss_bars_mut().set_players(&name, &targets, region) == Some(false) {
        return Err(fail("commands.bossbar.set.players.unchanged"));
    }
    let names = global
        .boss_bars()
        .online_names(&name, region)
        .ok_or_else(|| fail("command.failed"))?;
    if names.is_empty() {
        ok(
            region,
            id,
            "commands.bossbar.set.players.success.none",
            args([shown.into()]),
            true,
        );
    } else {
        let list = crate::scoreboard::format_green_names(&names);
        ok(
            region,
            id,
            "commands.bossbar.set.players.success.some",
            args([shown.into(), (names.len() as i32).into(), list.into()]),
            true,
        );
    }
    Ok(())
}

macro_rules! color_actions {
    ($($name:ident => $color:ident),* $(,)?) => {
        $(
            pub(crate) fn $name(
                global: &mut Global,
                region: &mut Region,
                id: PlayerId,
                parsed: &ParsedCommand,
            ) -> Result<(), CommandFail> {
                set_color(global, region, id, parsed, BossColor::$color)
            }
        )*
    };
}

color_actions! {
    color_pink => Pink,
    color_blue => Blue,
    color_red => Red,
    color_green => Green,
    color_yellow => Yellow,
    color_purple => Purple,
    color_white => White,
}

macro_rules! style_actions {
    ($($name:ident => $style:ident),* $(,)?) => {
        $(
            pub(crate) fn $name(
                global: &mut Global,
                region: &mut Region,
                id: PlayerId,
                parsed: &ParsedCommand,
            ) -> Result<(), CommandFail> {
                set_style(global, region, id, parsed, BossStyle::$style)
            }
        )*
    };
}

style_actions! {
    style_progress => Progress,
    style_notched_6 => Notched6,
    style_notched_10 => Notched10,
    style_notched_12 => Notched12,
    style_notched_20 => Notched20,
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
            r#"bossbar add test:one {text:"One"}"#,
            "bossbar remove test:one",
            "bossbar list",
            "bossbar get test:one value",
            "bossbar get test:one max",
            "bossbar get test:one visible",
            "bossbar get test:one players",
            "bossbar set test:one name {text:\"Two\"}",
            "bossbar set test:one color blue",
            "bossbar set test:one style notched_10",
            "bossbar set test:one value 5",
            "bossbar set test:one max 200",
            "bossbar set test:one visible false",
            "bossbar set test:one players",
            "bossbar set test:one players @a",
        ] {
            assert!(graph.parse(text, 2).is_ok(), "{text}");
            assert!(graph.parse(text, 1).is_err(), "{text} at level 1");
        }
    }

    #[test]
    fn add_list_get_and_remove_report_the_official_keys() {
        let (mut server, client) = operator("Op");
        run(&mut server, &client, r#"bossbar add test:one {text:"One"}"#);
        assert!(has_key(
            &system_text(&client),
            "commands.bossbar.create.success"
        ));
        assert!(server.global().boss_bars().bar("test:one").is_some());
        run(&mut server, &client, r#"bossbar add test:one {text:"One"}"#);
        assert!(has_key(
            &system_text(&client),
            "commands.bossbar.create.failed"
        ));
        run(&mut server, &client, "bossbar list");
        assert!(has_key(
            &system_text(&client),
            "commands.bossbar.list.bars.some"
        ));
        run(&mut server, &client, "bossbar get test:one max");
        assert!(has_key(&system_text(&client), "commands.bossbar.get.max"));
        run(&mut server, &client, "bossbar get missing value");
        assert!(has_key(&system_text(&client), "commands.bossbar.unknown"));
        run(&mut server, &client, "bossbar remove test:one");
        assert!(has_key(
            &system_text(&client),
            "commands.bossbar.remove.success"
        ));
    }

    #[test]
    fn set_players_sends_add_and_remove_packets() {
        let (mut server, client) = operator("Op");
        run(&mut server, &client, r#"bossbar add test:one {text:"One"}"#);
        client.take_frames();
        run(&mut server, &client, "bossbar set test:one players @a");
        let frames = client.take_frames();
        assert!(
            frames
                .iter()
                .any(|frame| frame.id == clientbound::BOSS_EVENT)
        );
        assert!(has_key(
            &frames_with_id(&frames, clientbound::SYSTEM_CHAT)
                .iter()
                .map(|frame| frame.body.clone())
                .collect::<Vec<_>>(),
            "commands.bossbar.set.players.success.some"
        ));
        run(&mut server, &client, "bossbar set test:one players");
        let frames = client.take_frames();
        assert!(
            frames
                .iter()
                .any(|frame| frame.id == clientbound::BOSS_EVENT)
        );
        assert!(has_key(
            &frames_with_id(&frames, clientbound::SYSTEM_CHAT)
                .iter()
                .map(|frame| frame.body.clone())
                .collect::<Vec<_>>(),
            "commands.bossbar.set.players.success.none"
        ));
    }

    #[test]
    fn visible_toggles_add_and_remove_and_updates_only_go_to_players() {
        let (mut server, client) = operator("Op");
        run(&mut server, &client, r#"bossbar add test:one {text:"One"}"#);
        run(&mut server, &client, "bossbar set test:one players @a");
        client.take_frames();
        run(&mut server, &client, "bossbar set test:one value 40");
        let frames = client.take_frames();
        assert!(
            frames
                .iter()
                .any(|frame| frame.id == clientbound::BOSS_EVENT)
        );
        run(&mut server, &client, "bossbar set test:one visible false");
        assert!(has_key(
            &system_text(&client),
            "commands.bossbar.set.visible.success.hidden"
        ));
        run(&mut server, &client, "bossbar set test:one visible false");
        assert!(has_key(
            &system_text(&client),
            "commands.bossbar.set.visibility.unchanged.hidden"
        ));
    }

    #[test]
    fn rejoining_player_gets_the_add_packet_again() {
        let (mut server, client) = operator("Op");
        run(&mut server, &client, r#"bossbar add test:one {text:"One"}"#);
        run(&mut server, &client, "bossbar set test:one players @a");
        client.take_frames();
        let name = server
            .region()
            .players
            .get(&client.id)
            .expect("玩家应该在世界里")
            .name
            .clone();
        server.leave(&client);
        server.tick();
        let rejoin = server.join_uuid(&name, client.uuid);
        server.tick();
        let frames = rejoin.take_frames();
        assert!(
            frames
                .iter()
                .any(|frame| frame.id == clientbound::BOSS_EVENT)
        );
    }
}
