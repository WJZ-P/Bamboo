//! 第一批命令的语法。根节点的顺序跟官方 `Commands` 构造函数里出现的顺序一致。
//!
//! Grammar of the first commands. Root order follows the official `Commands` constructor.

use mc_bamboo_command::{ArgumentType, CommandGraph, StringKind, permission};

use mc_bamboo_data::gamerules::{GameRuleDef, GameRuleKind, RULES};

use crate::command::{
    CommandFn, Commands, admin, blocks, bossbar, clone, enchant, gamerule, items, loot, particle,
    players, playsound, random, scoreboard, stopsound, team, teleport, text, time, title, weather,
    world_settings,
};

struct Builder {
    graph: CommandGraph,
    actions: Vec<Option<CommandFn>>,
    rules: Vec<Option<u16>>,
}

impl Builder {
    fn new() -> Self {
        Self {
            graph: CommandGraph::new(),
            actions: Vec::new(),
            rules: Vec::new(),
        }
    }

    fn literal(&mut self, parent: usize, name: &str, requires: u8) -> usize {
        let id = self.graph.literal(parent, name, requires);
        self.ensure(id);
        id
    }

    fn argument(&mut self, parent: usize, name: &str, kind: ArgumentType, requires: u8) -> usize {
        let id = self.graph.argument(parent, name, kind, requires);
        self.ensure(id);
        id
    }

    fn run(&mut self, node: usize, action: CommandFn) -> usize {
        self.graph.executable(node);
        self.ensure(node);
        self.actions[node] = Some(action);
        node
    }

    fn bind_literal(&mut self, parent: usize, name: &str, action: CommandFn) {
        let node = self.literal(parent, name, 0);
        self.run(node, action);
    }

    fn run_rule(&mut self, node: usize, rule: usize, action: CommandFn) {
        self.run(node, action);
        self.rules[node] = u16::try_from(rule).ok();
    }

    fn ensure(&mut self, id: usize) {
        if self.actions.len() <= id {
            self.actions.resize(id + 1, None);
            self.rules.resize(id + 1, None);
        }
    }
}

pub(crate) fn build() -> Commands {
    let mut builder = Builder::new();
    let root = builder.graph.root();
    register_bossbar(&mut builder, root);
    register_ban(&mut builder, root);
    register_ban_ip(&mut builder, root);
    register_banlist(&mut builder, root);
    register_deop(&mut builder, root);
    register_op(&mut builder, root);
    register_pardon(&mut builder, root);
    register_pardon_ip(&mut builder, root);
    register_set_idle_timeout(&mut builder, root);
    register_stop(&mut builder, root);
    register_transfer(&mut builder, root);
    register_version(&mut builder, root);
    register_whitelist(&mut builder, root);
    register_clear(&mut builder, root);
    register_me(&mut builder, root);
    register_fill(&mut builder, root);
    register_gamemode(&mut builder, root);
    register_gamerule(&mut builder, root);
    register_give(&mut builder, root);
    register_help(&mut builder, root);
    register_kick(&mut builder, root);
    register_list(&mut builder, root);
    register_playsound(&mut builder, root);
    register_msg(&mut builder, root);
    register_particle(&mut builder, root);
    register_say(&mut builder, root);
    register_scoreboard(&mut builder, root);
    register_setblock(&mut builder, root);
    register_tellraw(&mut builder, root);
    register_team(&mut builder, root);
    register_teleport(&mut builder, root);
    register_title(&mut builder, root);
    register_time(&mut builder, root);
    register_difficulty(&mut builder, root);
    register_default_gamemode(&mut builder, root);
    register_seed(&mut builder, root);
    register_spawns(&mut builder, root);
    register_clone(&mut builder, root);
    register_stopsound(&mut builder, root);
    register_random(&mut builder, root);
    register_enchant(&mut builder, root);
    register_loot(&mut builder, root);
    register_weather(&mut builder, root);
    Commands {
        graph: builder.graph,
        actions: builder.actions,
        rules: builder.rules,
    }
}

/// `GameRuleCommand.register`：每个启用的规则有短名和 `minecraft:` 全名，都能查询和设置。
///
/// 子节点按注册表顺序。官方 `GameRuleMap` 是 identity hash，顺序不稳定。
///
/// `GameRuleCommand.register`: each enabled rule has a short name and a `minecraft:` name,
/// and both can query and set.
///
/// Children follow registry order. The official `GameRuleMap` is an identity hash map,
/// so its order is not stable.
fn register_gamerule(builder: &mut Builder, root: usize) {
    let gamerule = builder.literal(root, "gamerule", permission::GAMEMASTERS);
    for (index, rule) in RULES.iter().enumerate() {
        if rule.enabled {
            register_one_rule(builder, gamerule, rule.name, index, rule);
            register_one_rule(builder, gamerule, rule.identifier, index, rule);
        }
    }
}

fn register_one_rule(
    builder: &mut Builder,
    parent: usize,
    name: &str,
    index: usize,
    rule: &GameRuleDef,
) {
    let node = builder.literal(parent, name, 0);
    builder.run_rule(node, index, gamerule::query);
    let value = builder.argument(node, "value", argument_of(rule), 0);
    builder.run_rule(value, index, gamerule::set);
}

fn argument_of(rule: &GameRuleDef) -> ArgumentType {
    match rule.kind {
        GameRuleKind::Bool => ArgumentType::Bool,
        GameRuleKind::Int => ArgumentType::Integer {
            min: rule.min,
            max: rule.max,
        },
    }
}

fn register_bossbar(builder: &mut Builder, root: usize) {
    let command = builder.literal(root, "bossbar", permission::GAMEMASTERS);
    let add = builder.literal(command, "add", 0);
    let id = builder.argument(add, "id", ArgumentType::ResourceLocation, 0);
    let name = builder.argument(id, "name", ArgumentType::Component, 0);
    builder.run(name, bossbar::add);

    let remove = builder.literal(command, "remove", 0);
    let id = builder.argument(remove, "id", ArgumentType::ResourceLocation, 0);
    builder.run(id, bossbar::remove);
    builder.bind_literal(command, "list", bossbar::list);

    let get = builder.literal(command, "get", 0);
    let id = builder.argument(get, "id", ArgumentType::ResourceLocation, 0);
    builder.bind_literal(id, "value", bossbar::get_value);
    builder.bind_literal(id, "max", bossbar::get_max);
    builder.bind_literal(id, "visible", bossbar::get_visible);
    builder.bind_literal(id, "players", bossbar::get_players);

    let set = builder.literal(command, "set", 0);
    let id = builder.argument(set, "id", ArgumentType::ResourceLocation, 0);
    let name = builder.literal(id, "name", 0);
    let contents = builder.argument(name, "name", ArgumentType::Component, 0);
    builder.run(contents, bossbar::set_name);
    let color = builder.literal(id, "color", 0);
    for (literal, action) in [
        ("pink", bossbar::color_pink as CommandFn),
        ("blue", bossbar::color_blue),
        ("red", bossbar::color_red),
        ("green", bossbar::color_green),
        ("yellow", bossbar::color_yellow),
        ("purple", bossbar::color_purple),
        ("white", bossbar::color_white),
    ] {
        builder.bind_literal(color, literal, action);
    }
    let style = builder.literal(id, "style", 0);
    for (literal, action) in [
        ("progress", bossbar::style_progress as CommandFn),
        ("notched_6", bossbar::style_notched_6),
        ("notched_10", bossbar::style_notched_10),
        ("notched_12", bossbar::style_notched_12),
        ("notched_20", bossbar::style_notched_20),
    ] {
        builder.bind_literal(style, literal, action);
    }
    let value = builder.literal(id, "value", 0);
    let number = builder.argument(
        value,
        "value",
        ArgumentType::Integer {
            min: 0,
            max: i32::MAX,
        },
        0,
    );
    builder.run(number, bossbar::set_value);
    let max = builder.literal(id, "max", 0);
    let number = builder.argument(
        max,
        "max",
        ArgumentType::Integer {
            min: 1,
            max: i32::MAX,
        },
        0,
    );
    builder.run(number, bossbar::set_max);
    let visible = builder.literal(id, "visible", 0);
    let flag = builder.argument(visible, "visible", ArgumentType::Bool, 0);
    builder.run(flag, bossbar::set_visible);
    let players = builder.literal(id, "players", 0);
    builder.run(players, bossbar::set_players);
    let targets = builder.argument(
        players,
        "targets",
        ArgumentType::Entity {
            single: false,
            players_only: true,
        },
        0,
    );
    builder.run(targets, bossbar::set_players);
}

fn register_clear(builder: &mut Builder, root: usize) {
    let clear = builder.literal(root, "clear", permission::GAMEMASTERS);
    builder.run(clear, items::clear);
    let targets = builder.argument(clear, "targets", players_argument(), 0);
    builder.run(targets, items::clear);
    let item = builder.argument(targets, "item", ArgumentType::ItemPredicate, 0);
    builder.run(item, items::clear);
    let count = builder.argument(
        item,
        "maxCount",
        ArgumentType::Integer {
            min: 0,
            max: i32::MAX,
        },
        0,
    );
    builder.run(count, items::clear);
}

fn register_me(builder: &mut Builder, root: usize) {
    let me = builder.literal(root, "me", permission::ALL);
    let action = builder.argument(me, "action", ArgumentType::Message, 0);
    builder.run(action, players::me);
}

fn register_fill(builder: &mut Builder, root: usize) {
    let fill = builder.literal(root, "fill", permission::GAMEMASTERS);
    let from = builder.argument(fill, "from", ArgumentType::BlockPos, 0);
    let to = builder.argument(from, "to", ArgumentType::BlockPos, 0);
    let block = builder.argument(to, "block", ArgumentType::BlockState, 0);
    wrap_fill_modes(builder, block);
    let replace = builder.literal(block, "replace", 0);
    builder.run(replace, blocks::fill_replace);
    let filter = builder.argument(replace, "filter", ArgumentType::BlockPredicate, 0);
    wrap_fill_modes(builder, filter);
    builder.bind_literal(block, "keep", blocks::fill_keep);
}

fn wrap_fill_modes(builder: &mut Builder, parent: usize) {
    builder.run(parent, blocks::fill_replace);
    builder.bind_literal(parent, "outline", blocks::fill_outline);
    builder.bind_literal(parent, "hollow", blocks::fill_hollow);
    builder.bind_literal(parent, "destroy", blocks::fill_destroy);
    builder.bind_literal(parent, "strict", blocks::fill_strict);
}

fn register_gamemode(builder: &mut Builder, root: usize) {
    let gamemode = builder.literal(root, "gamemode", permission::GAMEMASTERS);
    let mode = builder.argument(gamemode, "gamemode", ArgumentType::GameMode, 0);
    builder.run(mode, players::gamemode);
    let target = builder.argument(mode, "target", players_argument(), 0);
    builder.run(target, players::gamemode);
}

fn register_give(builder: &mut Builder, root: usize) {
    let give = builder.literal(root, "give", permission::GAMEMASTERS);
    let targets = builder.argument(give, "targets", players_argument(), 0);
    let item = builder.argument(targets, "item", ArgumentType::Item, 0);
    builder.run(item, items::give);
    let count = builder.argument(
        item,
        "count",
        ArgumentType::Integer {
            min: 1,
            max: i32::MAX,
        },
        0,
    );
    builder.run(count, items::give);
}

fn register_help(builder: &mut Builder, root: usize) {
    let help = builder.literal(root, "help", permission::ALL);
    builder.run(help, players::help);
    let command = builder.argument(help, "command", ArgumentType::String(StringKind::Greedy), 0);
    builder.run(command, players::help);
}

fn register_kick(builder: &mut Builder, root: usize) {
    let kick = builder.literal(root, "kick", permission::ADMINS);
    let targets = builder.argument(kick, "targets", players_argument(), 0);
    builder.run(targets, players::kick);
    let reason = builder.argument(targets, "reason", ArgumentType::Message, 0);
    builder.run(reason, players::kick);
}

fn register_list(builder: &mut Builder, root: usize) {
    let list = builder.literal(root, "list", permission::ALL);
    builder.run(list, players::list);
    builder.bind_literal(list, "uuids", players::list);
}

fn register_msg(builder: &mut Builder, root: usize) {
    let msg = builder.literal(root, "msg", permission::ALL);
    let targets = builder.argument(msg, "targets", players_argument(), 0);
    let message = builder.argument(targets, "message", ArgumentType::Message, 0);
    builder.run(message, players::msg);
    let tell = builder.literal(root, "tell", permission::ALL);
    builder.graph.redirect(tell, msg);
    let whisper = builder.literal(root, "w", permission::ALL);
    builder.graph.redirect(whisper, msg);
}

/// `PlaySoundCommand.register`：`sound` 用 `ResourceLocationArgument` 加
/// `minecraft:available_sounds` 自定义补全；11 个来源各挂同一条参数链。
/// `ParticleCommand.register`：`name` 后面 pos/delta/speed/count/force|normal/viewers。
fn register_particle(builder: &mut Builder, root: usize) {
    let command = builder.literal(root, "particle", permission::GAMEMASTERS);
    let name = builder.argument(command, "name", ArgumentType::Particle, 0);
    builder.run(name, particle::normal);
    let pos = builder.argument(name, "pos", ArgumentType::Vec3, 0);
    builder.run(pos, particle::normal);
    let spread = builder.argument(pos, "delta", ArgumentType::Vec3Uncentered, 0);
    let speed = builder.argument(
        spread,
        "speed",
        ArgumentType::Float {
            min: 0.0,
            max: f32::MAX,
        },
        0,
    );
    let count = builder.argument(
        speed,
        "count",
        ArgumentType::Integer {
            min: 0,
            max: i32::MAX,
        },
        0,
    );
    builder.run(count, particle::normal);
    let force_action: CommandFn = particle::force;
    let normal_action: CommandFn = particle::normal;
    for (mode, action) in [("force", force_action), ("normal", normal_action)] {
        let branch = builder.literal(count, mode, 0);
        builder.run(branch, action);
        let viewers = builder.argument(branch, "viewers", players_argument(), 0);
        builder.run(viewers, action);
    }
}

fn register_playsound(builder: &mut Builder, root: usize) {
    let command = builder.literal(root, "playsound", permission::GAMEMASTERS);
    let sound = builder.argument(command, "sound", ArgumentType::ResourceLocation, 0);
    builder
        .graph
        .suggest_with(sound, playsound::SUGGESTION, playsound::suggest_sounds);
    builder.run(sound, playsound::sound_only);
    for (name, action) in playsound::SOURCES
        .iter()
        .map(|(name, _)| *name)
        .zip(playsound::ACTIONS)
    {
        let branch = builder.literal(sound, name, 0);
        builder.run(branch, action);
        let targets = builder.argument(branch, "targets", players_argument(), 0);
        builder.run(targets, action);
        let pos = builder.argument(targets, "pos", ArgumentType::Vec3, 0);
        builder.run(pos, action);
        let volume = builder.argument(
            pos,
            "volume",
            ArgumentType::Float {
                min: 0.0,
                max: f32::MAX,
            },
            0,
        );
        builder.run(volume, action);
        let pitch = builder.argument(
            volume,
            "pitch",
            ArgumentType::Float { min: 0.0, max: 2.0 },
            0,
        );
        builder.run(pitch, action);
        let min_volume = builder.argument(
            pitch,
            "minVolume",
            ArgumentType::Float { min: 0.0, max: 1.0 },
            0,
        );
        builder.run(min_volume, action);
    }
}

fn register_say(builder: &mut Builder, root: usize) {
    let say = builder.literal(root, "say", permission::GAMEMASTERS);
    let message = builder.argument(say, "message", ArgumentType::Message, 0);
    builder.run(message, players::say);
}

fn register_setblock(builder: &mut Builder, root: usize) {
    let setblock = builder.literal(root, "setblock", permission::GAMEMASTERS);
    let pos = builder.argument(setblock, "pos", ArgumentType::BlockPos, 0);
    let block = builder.argument(pos, "block", ArgumentType::BlockState, 0);
    builder.run(block, blocks::setblock_replace);
    builder.bind_literal(block, "destroy", blocks::setblock_destroy);
    builder.bind_literal(block, "keep", blocks::setblock_keep);
    builder.bind_literal(block, "replace", blocks::setblock_replace);
    builder.bind_literal(block, "strict", blocks::setblock_strict);
}

/// `TellRawCommand.register`：权限 2，只给每个目标发系统消息，没有成功反馈。
fn register_tellraw(builder: &mut Builder, root: usize) {
    let tellraw = builder.literal(root, "tellraw", permission::GAMEMASTERS);
    let targets = builder.argument(tellraw, "targets", players_argument(), 0);
    let message = builder.argument(targets, "message", ArgumentType::Component, 0);
    builder.run(message, text::tellraw);
}

/// `TitleCommand.register`：权限 2，六种动作；`times` 用 `TimeArgument`（min 0）。
fn register_title(builder: &mut Builder, root: usize) {
    let command = builder.literal(root, "title", permission::GAMEMASTERS);
    let targets = builder.argument(command, "targets", players_argument(), 0);
    for (name, action) in [
        ("clear", title::Action::Clear),
        ("reset", title::Action::Reset),
    ] {
        let node = builder.literal(targets, name, 0);
        builder.run(node, action_fn(action));
    }
    for (name, action) in [
        ("title", title::Action::Title),
        ("subtitle", title::Action::Subtitle),
        ("actionbar", title::Action::Actionbar),
    ] {
        let branch = builder.literal(targets, name, 0);
        let text = builder.argument(branch, "title", ArgumentType::Component, 0);
        builder.run(text, action_fn(action));
    }
    let times = builder.literal(targets, "times", 0);
    let fade_in = builder.argument(times, "fadeIn", ArgumentType::Time { minimum: 0 }, 0);
    let stay = builder.argument(fade_in, "stay", ArgumentType::Time { minimum: 0 }, 0);
    let fade_out = builder.argument(stay, "fadeOut", ArgumentType::Time { minimum: 0 }, 0);
    builder.run(fade_out, title::run_times);
}

fn action_fn(action: title::Action) -> CommandFn {
    match action {
        title::Action::Clear => title::clear,
        title::Action::Reset => title::reset,
        title::Action::Title => title::show_title,
        title::Action::Subtitle => title::show_subtitle,
        title::Action::Actionbar => title::show_actionbar,
        title::Action::Times => title::run_times,
    }
}

fn register_teleport(builder: &mut Builder, root: usize) {
    let teleport = builder.literal(root, "teleport", permission::GAMEMASTERS);
    let location = builder.argument(teleport, "location", ArgumentType::Vec3, 0);
    builder.run(location, teleport::teleport);
    let destination = builder.argument(teleport, "destination", entity_argument(), 0);
    builder.run(destination, teleport::teleport);
    let targets = builder.argument(teleport, "targets", entities_argument(), 0);
    let targets_location = builder.argument(targets, "location", ArgumentType::Vec3, 0);
    builder.run(targets_location, teleport::teleport);
    let rotation = builder.argument(targets_location, "rotation", ArgumentType::Rotation, 0);
    builder.run(rotation, teleport::teleport);
    let facing = builder.literal(targets_location, "facing", 0);
    let entity = builder.literal(facing, "entity", 0);
    let facing_entity = builder.argument(entity, "facingEntity", entity_argument(), 0);
    builder.run(facing_entity, teleport::teleport);
    let anchor = builder.argument(facing_entity, "facingAnchor", ArgumentType::EntityAnchor, 0);
    builder.run(anchor, teleport::teleport);
    let facing_location = builder.argument(facing, "facingLocation", ArgumentType::Vec3, 0);
    builder.run(facing_location, teleport::teleport);
    let targets_destination = builder.argument(targets, "destination", entity_argument(), 0);
    builder.run(targets_destination, teleport::teleport);
    let alias = builder.literal(root, "tp", permission::GAMEMASTERS);
    builder.graph.redirect(alias, teleport);
}

fn register_time(builder: &mut Builder, root: usize) {
    let time_node = builder.literal(root, "time", permission::GAMEMASTERS);
    add_clock_nodes(builder, time_node, true);
    let of = builder.literal(time_node, "of", 0);
    let clock = builder.argument(
        of,
        "clock",
        ArgumentType::Resource {
            registry: "minecraft:world_clock",
        },
        0,
    );
    add_clock_nodes(builder, clock, false);
}

fn add_clock_nodes(builder: &mut Builder, parent: usize, gametime: bool) {
    let set = builder.literal(parent, "set", 0);
    let time_arg = builder.argument(set, "time", ArgumentType::Time { minimum: 0 }, 0);
    builder.run(time_arg, time::set_time);
    let marker = builder.argument(set, "timemarker", ArgumentType::ResourceLocation, 0);
    builder
        .graph
        .suggest_with(marker, time::SUGGESTION, time::suggest_markers);
    builder.run(marker, time::set_marker);

    let add = builder.literal(parent, "add", 0);
    let add_time = builder.argument(add, "time", ArgumentType::Time { minimum: i32::MIN }, 0);
    builder.run(add_time, time::add_time);

    builder.bind_literal(parent, "pause", time::pause);
    builder.bind_literal(parent, "resume", time::resume);

    let rate = builder.literal(parent, "rate", 0);
    let rate_arg = builder.argument(
        rate,
        "rate",
        ArgumentType::Float {
            min: 1.0e-5,
            max: 1000.0,
        },
        0,
    );
    builder.run(rate_arg, time::set_rate);

    let query = builder.literal(parent, "query", 0);
    builder.bind_literal(query, "time", time::query_time);
    let timeline = builder.argument(
        query,
        "timeline",
        ArgumentType::Resource {
            registry: "minecraft:timeline",
        },
        0,
    );
    builder
        .graph
        .suggest_with(timeline, time::SUGGESTION, time::suggest_timelines);
    builder.run(timeline, time::query_timeline);
    builder.bind_literal(timeline, "repetition", time::query_repetition);
    if gametime {
        builder.bind_literal(query, "gametime", time::query_gametime);
    }
}

fn register_scoreboard(builder: &mut Builder, root: usize) {
    let command = builder.literal(root, "scoreboard", permission::GAMEMASTERS);
    let objectives = builder.literal(command, "objectives", 0);
    let players = builder.literal(command, "players", 0);

    // objectives list|add|remove|setdisplay|modify
    builder.bind_literal(objectives, "list", scoreboard::objectives_list);
    let add = builder.literal(objectives, "add", 0);
    let objective = builder.argument(
        add,
        "objective",
        ArgumentType::String(StringKind::SingleWord),
        0,
    );
    let criteria = builder.argument(objective, "criteria", ArgumentType::Criteria, 0);
    builder.run(criteria, scoreboard::objectives_add);
    let display_name = builder.argument(criteria, "displayName", ArgumentType::Component, 0);
    builder.run(display_name, scoreboard::objectives_add);

    let remove = builder.literal(objectives, "remove", 0);
    let target = builder.argument(remove, "objective", ArgumentType::Objective, 0);
    builder.run(target, scoreboard::objectives_remove);

    let setdisplay = builder.literal(objectives, "setdisplay", 0);
    let slot = builder.argument(setdisplay, "slot", ArgumentType::DisplaySlot, 0);
    builder.run(slot, scoreboard::objectives_setdisplay);
    let shown = builder.argument(slot, "objective", ArgumentType::Objective, 0);
    builder.run(shown, scoreboard::objectives_setdisplay);

    let modify = builder.literal(objectives, "modify", 0);
    let objective = builder.argument(modify, "objective", ArgumentType::Objective, 0);
    let displayname = builder.literal(objective, "displayname", 0);
    let name = builder.argument(displayname, "displayName", ArgumentType::Component, 0);
    builder.run(name, scoreboard::objectives_displayname);
    let rendertype = builder.literal(objective, "rendertype", 0);
    builder.bind_literal(rendertype, "hearts", scoreboard::objectives_render_hearts);
    builder.bind_literal(rendertype, "integer", scoreboard::objectives_render_integer);
    let autoupdate = builder.literal(objective, "displayautoupdate", 0);
    let value = builder.argument(autoupdate, "value", ArgumentType::Bool, 0);
    builder.run(value, scoreboard::objectives_autoupdate);
    add_objective_number_formats(builder, objective);

    // players list|get|set|add|remove|reset|enable|operation|display
    builder.bind_literal(players, "list", scoreboard::players_list);
    let list = builder.literal(players, "list", 0);
    let target = builder.argument(
        list,
        "target",
        ArgumentType::ScoreHolder { multiple: false },
        0,
    );
    builder.run(target, scoreboard::players_list_scores);
    let get = builder.literal(players, "get", 0);
    let target = builder.argument(
        get,
        "target",
        ArgumentType::ScoreHolder { multiple: false },
        0,
    );
    let objective = builder.argument(target, "objective", ArgumentType::Objective, 0);
    builder.run(objective, scoreboard::players_get);

    let set = builder.literal(players, "set", 0);
    let targets = builder.argument(
        set,
        "targets",
        ArgumentType::ScoreHolder { multiple: true },
        0,
    );
    let objective = builder.argument(targets, "objective", ArgumentType::Objective, 0);
    let score = builder.argument(
        objective,
        "score",
        ArgumentType::Integer {
            min: i32::MIN,
            max: i32::MAX,
        },
        0,
    );
    builder.run(score, scoreboard::players_set);
    let add = builder.literal(players, "add", 0);
    let targets = builder.argument(
        add,
        "targets",
        ArgumentType::ScoreHolder { multiple: true },
        0,
    );
    let objective = builder.argument(targets, "objective", ArgumentType::Objective, 0);
    let score = builder.argument(
        objective,
        "score",
        ArgumentType::Integer {
            min: 0,
            max: i32::MAX,
        },
        0,
    );
    builder.run(score, scoreboard::players_add);
    let remove = builder.literal(players, "remove", 0);
    let targets = builder.argument(
        remove,
        "targets",
        ArgumentType::ScoreHolder { multiple: true },
        0,
    );
    let objective = builder.argument(targets, "objective", ArgumentType::Objective, 0);
    let score = builder.argument(
        objective,
        "score",
        ArgumentType::Integer {
            min: 0,
            max: i32::MAX,
        },
        0,
    );
    builder.run(score, scoreboard::players_remove);

    let reset = builder.literal(players, "reset", 0);
    let targets = builder.argument(
        reset,
        "targets",
        ArgumentType::ScoreHolder { multiple: true },
        0,
    );
    builder.run(targets, scoreboard::players_reset);
    let objective = builder.argument(targets, "objective", ArgumentType::Objective, 0);
    builder.run(objective, scoreboard::players_reset);

    let enable = builder.literal(players, "enable", 0);
    let targets = builder.argument(
        enable,
        "targets",
        ArgumentType::ScoreHolder { multiple: true },
        0,
    );
    let objective = builder.argument(targets, "objective", ArgumentType::Objective, 0);
    builder.run(objective, scoreboard::players_enable);

    let operation = builder.literal(players, "operation", 0);
    let targets = builder.argument(
        operation,
        "targets",
        ArgumentType::ScoreHolder { multiple: true },
        0,
    );
    let target_objective = builder.argument(targets, "targetObjective", ArgumentType::Objective, 0);
    let op = builder.argument(target_objective, "operation", ArgumentType::Operation, 0);
    let source = builder.argument(
        op,
        "source",
        ArgumentType::ScoreHolder { multiple: true },
        0,
    );
    let source_objective = builder.argument(source, "sourceObjective", ArgumentType::Objective, 0);
    builder.run(source_objective, scoreboard::players_operation);

    let display = builder.literal(players, "display", 0);
    let name = builder.literal(display, "name", 0);
    let targets = builder.argument(
        name,
        "targets",
        ArgumentType::ScoreHolder { multiple: true },
        0,
    );
    let objective = builder.argument(targets, "objective", ArgumentType::Objective, 0);
    builder.run(objective, scoreboard::players_display_clear);
    let contents = builder.argument(objective, "name", ArgumentType::Component, 0);
    builder.run(contents, scoreboard::players_display_name);
    let format = builder.literal(display, "numberformat", 0);
    let targets = builder.argument(
        format,
        "targets",
        ArgumentType::ScoreHolder { multiple: true },
        0,
    );
    let objective = builder.argument(targets, "objective", ArgumentType::Objective, 0);
    builder.run(objective, scoreboard::players_format_clear);
    add_player_number_formats(builder, objective);
}

fn add_objective_number_formats(builder: &mut Builder, objective: usize) {
    let format = builder.literal(objective, "numberformat", 0);
    builder.run(format, scoreboard::objectives_format_default);
    builder.bind_literal(format, "blank", scoreboard::objectives_format_blank);
    let styled = builder.literal(format, "styled", 0);
    let style = builder.argument(styled, "style", ArgumentType::Style, 0);
    builder.run(style, scoreboard::objectives_format_styled);
    let fixed = builder.literal(format, "fixed", 0);
    let contents = builder.argument(fixed, "contents", ArgumentType::Component, 0);
    builder.run(contents, scoreboard::objectives_format_fixed);
}

fn add_player_number_formats(builder: &mut Builder, objective: usize) {
    builder.bind_literal(objective, "blank", scoreboard::players_format_blank);
    let styled = builder.literal(objective, "styled", 0);
    let style = builder.argument(styled, "style", ArgumentType::Style, 0);
    builder.run(style, scoreboard::players_format_styled);
    let fixed = builder.literal(objective, "fixed", 0);
    let contents = builder.argument(fixed, "contents", ArgumentType::Component, 0);
    builder.run(contents, scoreboard::players_format_fixed);
}

fn register_team(builder: &mut Builder, root: usize) {
    let command = builder.literal(root, "team", permission::GAMEMASTERS);
    builder.bind_literal(command, "list", team::list);
    let list = builder.literal(command, "list", 0);
    let name = builder.argument(list, "team", ArgumentType::Team, 0);
    builder.run(name, team::list_members);

    let add = builder.literal(command, "add", 0);
    let name = builder.argument(add, "team", ArgumentType::String(StringKind::SingleWord), 0);
    builder.run(name, team::add);
    let display = builder.argument(name, "displayName", ArgumentType::Component, 0);
    builder.run(display, team::add);

    let remove = builder.literal(command, "remove", 0);
    let name = builder.argument(remove, "team", ArgumentType::Team, 0);
    builder.run(name, team::remove);

    let empty = builder.literal(command, "empty", 0);
    let name = builder.argument(empty, "team", ArgumentType::Team, 0);
    builder.run(name, team::empty);

    let join = builder.literal(command, "join", 0);
    let name = builder.argument(join, "team", ArgumentType::Team, 0);
    builder.run(name, team::join);
    let members = builder.argument(
        name,
        "members",
        ArgumentType::ScoreHolder { multiple: true },
        0,
    );
    builder.run(members, team::join);

    let leave = builder.literal(command, "leave", 0);
    let members = builder.argument(
        leave,
        "members",
        ArgumentType::ScoreHolder { multiple: true },
        0,
    );
    builder.run(members, team::leave);

    let modify = builder.literal(command, "modify", 0);
    let name = builder.argument(modify, "team", ArgumentType::Team, 0);
    let display_name = builder.literal(name, "displayName", 0);
    let display = builder.argument(display_name, "displayName", ArgumentType::Component, 0);
    builder.run(display, team::display_name);
    let prefix = builder.literal(name, "prefix", 0);
    let text = builder.argument(prefix, "prefix", ArgumentType::Component, 0);
    builder.run(text, team::prefix);
    let suffix = builder.literal(name, "suffix", 0);
    let text = builder.argument(suffix, "suffix", ArgumentType::Component, 0);
    builder.run(text, team::suffix);

    let color = builder.literal(name, "color", 0);
    let value = builder.argument(color, "value", ArgumentType::TeamColor, 0);
    builder.run(value, team::color);
    builder.bind_literal(color, "reset", team::clear_color);

    let friendly = builder.literal(name, "friendlyFire", 0);
    let allowed = builder.argument(friendly, "allowed", ArgumentType::Bool, 0);
    builder.run(allowed, team::friendly_fire);
    let invisibles = builder.literal(name, "seeFriendlyInvisibles", 0);
    let allowed = builder.argument(invisibles, "allowed", ArgumentType::Bool, 0);
    builder.run(allowed, team::see_invisibles);

    let nametag = builder.literal(name, "nametagVisibility", 0);
    builder.bind_literal(nametag, "always", team::nametag_always);
    builder.bind_literal(nametag, "never", team::nametag_never);
    builder.bind_literal(nametag, "hideForOtherTeams", team::nametag_hide_other);
    builder.bind_literal(nametag, "hideForOwnTeam", team::nametag_hide_own);
    let death = builder.literal(name, "deathMessageVisibility", 0);
    builder.bind_literal(death, "always", team::death_always);
    builder.bind_literal(death, "never", team::death_never);
    builder.bind_literal(death, "hideForOtherTeams", team::death_hide_other);
    builder.bind_literal(death, "hideForOwnTeam", team::death_hide_own);
    let collision = builder.literal(name, "collisionRule", 0);
    builder.bind_literal(collision, "always", team::collision_always);
    builder.bind_literal(collision, "never", team::collision_never);
    builder.bind_literal(collision, "pushOtherTeams", team::collision_other);
    builder.bind_literal(collision, "pushOwnTeam", team::collision_own);
}

fn register_difficulty(builder: &mut Builder, root: usize) {
    let difficulty = builder.literal(root, "difficulty", permission::GAMEMASTERS);
    builder.run(difficulty, world_settings::query_difficulty);
    // 官方按 `Difficulty.values()` 的顺序枚举，报告里的顺序是字母序，比较用名字查。
    for name in ["peaceful", "easy", "normal", "hard"] {
        builder.bind_literal(difficulty, name, world_settings::difficulty_literal);
    }
}

/// 官方 `CloneCommands.register`：`begin`/`end`，两层 `to <dimension>` 分支，
/// 以及 `destination` 后面的 `strict` 和三种过滤方式。所有分支的子树形状相同。
fn register_clone(builder: &mut Builder, root: usize) {
    let command = builder.literal(root, "clone", permission::GAMEMASTERS);
    clone_begin_end(builder, command);
    let from = builder.literal(command, "from", 0);
    let source = builder.argument(from, "sourceDimension", ArgumentType::Dimension, 0);
    clone_begin_end(builder, source);
}

fn clone_begin_end(builder: &mut Builder, parent: usize) {
    let begin = builder.argument(parent, "begin", ArgumentType::BlockPos, 0);
    let end = builder.argument(begin, "end", ArgumentType::BlockPos, 0);
    clone_destination(builder, end);
    let to = builder.literal(end, "to", 0);
    let target = builder.argument(to, "targetDimension", ArgumentType::Dimension, 0);
    clone_destination(builder, target);
}

fn clone_destination(builder: &mut Builder, parent: usize) {
    let destination = builder.argument(parent, "destination", ArgumentType::BlockPos, 0);
    clone_modes(builder, destination, false);
    let strict = builder.literal(destination, "strict", 0);
    builder.run(strict, clone::strict_replace_normal);
    clone_filter_modes(builder, strict, true);
}

fn clone_modes(builder: &mut Builder, parent: usize, strict: bool) {
    let normal = if strict {
        clone::strict_replace_normal
    } else {
        clone::replace_normal
    };
    builder.run(parent, normal);
    clone_filter_modes(builder, parent, strict);
}

fn clone_filter_modes(builder: &mut Builder, parent: usize, strict: bool) {
    let replace = builder.literal(parent, "replace", 0);
    bind_clone_modes(builder, replace, FilterKind::Replace, strict);
    let masked = builder.literal(parent, "masked", 0);
    bind_clone_modes(builder, masked, FilterKind::Masked, strict);
    let filtered = builder.literal(parent, "filtered", 0);
    let filter = builder.argument(filtered, "filter", ArgumentType::BlockPredicate, 0);
    bind_clone_modes(builder, filter, FilterKind::Filtered, strict);
}

enum FilterKind {
    Replace,
    Masked,
    Filtered,
}

fn bind_clone_modes(builder: &mut Builder, parent: usize, filter: FilterKind, strict: bool) {
    let actions: [CommandFn; 3] = match (filter, strict) {
        (FilterKind::Replace, false) => [
            clone::replace_normal,
            clone::replace_force,
            clone::replace_move,
        ],
        (FilterKind::Masked, false) => [
            clone::masked_normal,
            clone::masked_force,
            clone::masked_move,
        ],
        (FilterKind::Filtered, false) => [
            clone::filtered_normal,
            clone::filtered_force,
            clone::filtered_move,
        ],
        (FilterKind::Replace, true) => [
            clone::strict_replace_normal,
            clone::strict_replace_force,
            clone::strict_replace_move,
        ],
        (FilterKind::Masked, true) => [
            clone::strict_masked_normal,
            clone::strict_masked_force,
            clone::strict_masked_move,
        ],
        (FilterKind::Filtered, true) => [
            clone::strict_filtered_normal,
            clone::strict_filtered_force,
            clone::strict_filtered_move,
        ],
    };
    builder.run(parent, actions[0]);
    builder.bind_literal(parent, "force", actions[1]);
    builder.bind_literal(parent, "move", actions[2]);
    builder.bind_literal(parent, "normal", actions[0]);
}

fn register_stopsound(builder: &mut Builder, root: usize) {
    let command = builder.literal(root, "stopsound", permission::GAMEMASTERS);
    let targets = builder.argument(command, "targets", players_argument(), 0);
    builder.run(targets, stopsound::stop_sound);
    let all = builder.literal(targets, "*", 0);
    add_sound_name(builder, all);
    for name in [
        "master", "music", "record", "weather", "block", "hostile", "neutral", "player", "ambient",
        "voice", "ui",
    ] {
        let source = builder.literal(targets, name, 0);
        builder.run(source, stopsound::stop_sound);
        add_sound_name(builder, source);
    }
}

fn add_sound_name(builder: &mut Builder, parent: usize) {
    let sound = builder.argument(parent, "sound", ArgumentType::ResourceLocation, 0);
    builder.run(sound, stopsound::stop_sound);
}

fn register_random(builder: &mut Builder, root: usize) {
    let command = builder.literal(root, "random", 0);
    let actions: [(&str, CommandFn); 2] = [("value", random::value), ("roll", random::roll)];
    for (name, action) in actions {
        let branch = builder.literal(command, name, 0);
        let range = builder.argument(branch, "range", ArgumentType::IntRange, 0);
        builder.run(range, action);
        let sequence = builder.argument(
            range,
            "sequence",
            ArgumentType::ResourceLocation,
            permission::GAMEMASTERS,
        );
        builder.run(sequence, action);
    }
    let reset = builder.literal(command, "reset", permission::GAMEMASTERS);
    let all = builder.literal(reset, "*", 0);
    builder.run(all, random::reset);
    add_reset_seed(builder, all);
    let sequence = builder.argument(reset, "sequence", ArgumentType::ResourceLocation, 0);
    builder.run(sequence, random::reset);
    add_reset_seed(builder, sequence);
}

fn add_reset_seed(builder: &mut Builder, parent: usize) {
    let seed = builder.argument(
        parent,
        "seed",
        ArgumentType::Integer {
            min: i32::MIN,
            max: i32::MAX,
        },
        0,
    );
    builder.run(seed, random::reset);
    let world = builder.argument(seed, "includeWorldSeed", ArgumentType::Bool, 0);
    builder.run(world, random::reset);
    let sequence_id = builder.argument(world, "includeSequenceId", ArgumentType::Bool, 0);
    builder.run(sequence_id, random::reset);
}

fn register_enchant(builder: &mut Builder, root: usize) {
    let command = builder.literal(root, "enchant", permission::GAMEMASTERS);
    let targets = builder.argument(command, "targets", entities_argument(), 0);
    let enchantment = builder.argument(
        targets,
        "enchantment",
        ArgumentType::Resource {
            registry: "minecraft:enchantment",
        },
        0,
    );
    builder.run(enchantment, enchant::enchant);
    let level = builder.argument(
        enchantment,
        "level",
        ArgumentType::Integer {
            min: 0,
            max: i32::MAX,
        },
        0,
    );
    builder.run(level, enchant::enchant);
}

/// 官方 `LootCommand.addTargets`：四种目标，每个目标后面是同一组来源后缀。
fn register_loot(builder: &mut Builder, root: usize) {
    let command = builder.literal(root, "loot", permission::GAMEMASTERS);

    let give = builder.literal(command, "give", 0);
    let players = builder.argument(give, "players", players_argument(), 0);
    add_loot_sources(builder, players, LootTarget::Give);

    let insert = builder.literal(command, "insert", 0);
    let target = builder.argument(insert, "targetPos", ArgumentType::BlockPos, 0);
    add_loot_sources(builder, target, LootTarget::Insert);

    let spawn = builder.literal(command, "spawn", 0);
    let target = builder.argument(spawn, "targetPos", ArgumentType::Vec3, 0);
    add_loot_sources(builder, target, LootTarget::Spawn);

    let replace = builder.literal(command, "replace", 0);
    let entity = builder.literal(replace, "entity", 0);
    let targets = builder.argument(entity, "entities", entities_argument(), 0);
    add_replace_suffix(builder, targets, LootTarget::ReplaceEntity);
    let block = builder.literal(replace, "block", 0);
    let target = builder.argument(block, "targetPos", ArgumentType::BlockPos, 0);
    add_replace_suffix(builder, target, LootTarget::ReplaceBlock);
}

fn add_replace_suffix(builder: &mut Builder, parent: usize, target: LootTarget) {
    let slot = builder.argument(parent, "slot", ArgumentType::ItemSlot, 0);
    add_loot_sources(builder, slot, target);
    let count = builder.argument(
        slot,
        "count",
        ArgumentType::Integer {
            min: 0,
            max: i32::MAX,
        },
        0,
    );
    add_loot_sources(builder, count, target);
}

#[derive(Clone, Copy)]
enum LootTarget {
    Give,
    Insert,
    ReplaceBlock,
    ReplaceEntity,
    Spawn,
}

/// `loot <table>`、`mine [pos [mainhand|offhand|tool]]`、`kill <target>`、
/// `fish <table> <pos> [mainhand|offhand|tool]`。
fn add_loot_sources(builder: &mut Builder, parent: usize, target: LootTarget) {
    let table = builder.literal(parent, "loot", 0);
    let name = builder.argument(table, "loot_table", ArgumentType::LootTable, 0);
    bind_loot(builder, name, target, LootSource::Loot);

    let mine = builder.literal(parent, "mine", 0);
    let pos = builder.argument(mine, "pos", ArgumentType::BlockPos, 0);
    bind_loot(builder, pos, target, LootSource::Mine);
    add_tool_suffixes(builder, pos, target, LootSource::Mine);

    let kill = builder.literal(parent, "kill", 0);
    let victim = builder.argument(kill, "target", entity_argument(), 0);
    bind_loot(builder, victim, target, LootSource::Kill);

    let fish = builder.literal(parent, "fish", 0);
    let name = builder.argument(fish, "loot_table", ArgumentType::LootTable, 0);
    let pos = builder.argument(name, "pos", ArgumentType::BlockPos, 0);
    bind_loot(builder, pos, target, LootSource::Fish);
    add_tool_suffixes(builder, pos, target, LootSource::Fish);
}

fn add_tool_suffixes(builder: &mut Builder, parent: usize, target: LootTarget, source: LootSource) {
    builder.bind_literal(parent, "mainhand", loot_action(target, source));
    builder.bind_literal(parent, "offhand", loot_action(target, source));
    let tool = builder.argument(parent, "tool", ArgumentType::Item, 0);
    builder.run(tool, loot_action(target, source));
}

#[derive(Clone, Copy)]
enum LootSource {
    Loot,
    Mine,
    Kill,
    Fish,
}

fn loot_action(target: LootTarget, source: LootSource) -> CommandFn {
    match (target, source) {
        (LootTarget::Give, LootSource::Loot) => loot::give_loot,
        (LootTarget::Give, LootSource::Mine) => loot::give_mine,
        (LootTarget::Give, LootSource::Kill) => loot::give_kill,
        (LootTarget::Give, LootSource::Fish) => loot::give_fish,
        (LootTarget::Insert, LootSource::Loot) => loot::insert_loot,
        (LootTarget::Insert, LootSource::Mine) => loot::insert_mine,
        (LootTarget::Insert, LootSource::Kill) => loot::insert_kill,
        (LootTarget::Insert, LootSource::Fish) => loot::insert_fish,
        (LootTarget::Spawn, LootSource::Loot) => loot::spawn_loot,
        (LootTarget::Spawn, LootSource::Mine) => loot::spawn_mine,
        (LootTarget::Spawn, LootSource::Kill) => loot::spawn_kill,
        (LootTarget::Spawn, LootSource::Fish) => loot::spawn_fish,
        (LootTarget::ReplaceBlock, LootSource::Loot) => loot::replace_block_loot,
        (LootTarget::ReplaceBlock, LootSource::Mine) => loot::replace_block_mine,
        (LootTarget::ReplaceBlock, LootSource::Kill) => loot::replace_block_kill,
        (LootTarget::ReplaceBlock, LootSource::Fish) => loot::replace_block_fish,
        (LootTarget::ReplaceEntity, LootSource::Loot) => loot::replace_entity_loot,
        (LootTarget::ReplaceEntity, LootSource::Mine) => loot::replace_entity_mine,
        (LootTarget::ReplaceEntity, LootSource::Kill) => loot::replace_entity_kill,
        (LootTarget::ReplaceEntity, LootSource::Fish) => loot::replace_entity_fish,
    }
}

fn bind_loot(builder: &mut Builder, parent: usize, target: LootTarget, source: LootSource) {
    builder.run(parent, loot_action(target, source));
}

fn register_weather(builder: &mut Builder, root: usize) {
    let command = builder.literal(root, "weather", permission::GAMEMASTERS);
    for name in ["clear", "rain", "thunder"] {
        let branch = builder.literal(command, name, 0);
        builder.run(branch, weather::weather);
        let duration = builder.argument(branch, "duration", ArgumentType::Time { minimum: 1 }, 0);
        builder.run(duration, weather::weather);
    }
}

fn register_seed(builder: &mut Builder, root: usize) {
    let seed = builder.literal(root, "seed", permission::GAMEMASTERS);
    builder.run(seed, world_settings::seed);
}

fn register_spawns(builder: &mut Builder, root: usize) {
    let world_spawn = builder.literal(root, "setworldspawn", permission::GAMEMASTERS);
    builder.run(world_spawn, world_settings::set_world_spawn);
    let pos = builder.argument(world_spawn, "pos", ArgumentType::BlockPos, 0);
    builder.run(pos, world_settings::set_world_spawn);
    let rotation = builder.argument(pos, "rotation", ArgumentType::Rotation, 0);
    builder.run(rotation, world_settings::set_world_spawn);

    let spawnpoint = builder.literal(root, "spawnpoint", permission::GAMEMASTERS);
    builder.run(spawnpoint, world_settings::spawnpoint);
    let targets = builder.argument(spawnpoint, "targets", players_argument(), 0);
    builder.run(targets, world_settings::spawnpoint);
    let pos = builder.argument(targets, "pos", ArgumentType::BlockPos, 0);
    builder.run(pos, world_settings::spawnpoint);
    let rotation = builder.argument(pos, "rotation", ArgumentType::Rotation, 0);
    builder.run(rotation, world_settings::spawnpoint);
}

fn register_default_gamemode(builder: &mut Builder, root: usize) {
    let command = builder.literal(root, "defaultgamemode", permission::GAMEMASTERS);
    let mode = builder.argument(command, "gamemode", ArgumentType::GameMode, 0);
    builder.run(mode, world_settings::default_gamemode);
}

fn register_op(builder: &mut Builder, root: usize) {
    let op = builder.literal(root, "op", permission::ADMINS);
    let targets = builder.argument(op, "targets", ArgumentType::GameProfile, 0);
    builder.run(targets, admin::op);
}

fn register_deop(builder: &mut Builder, root: usize) {
    let deop = builder.literal(root, "deop", permission::ADMINS);
    let targets = builder.argument(deop, "targets", ArgumentType::GameProfile, 0);
    builder.run(targets, admin::deop);
}

fn register_ban(builder: &mut Builder, root: usize) {
    let ban = builder.literal(root, "ban", permission::ADMINS);
    let targets = builder.argument(ban, "targets", ArgumentType::GameProfile, 0);
    builder.run(targets, admin::ban);
    let reason = builder.argument(targets, "reason", ArgumentType::Message, 0);
    builder.run(reason, admin::ban);
}

fn register_ban_ip(builder: &mut Builder, root: usize) {
    let ban = builder.literal(root, "ban-ip", permission::ADMINS);
    let target = builder.argument(
        ban,
        "target",
        ArgumentType::String(StringKind::SingleWord),
        0,
    );
    builder.run(target, admin::ban_ip);
    let reason = builder.argument(target, "reason", ArgumentType::Message, 0);
    builder.run(reason, admin::ban_ip);
}

fn register_banlist(builder: &mut Builder, root: usize) {
    let list = builder.literal(root, "banlist", permission::ADMINS);
    builder.run(list, admin::banlist_all);
    builder.bind_literal(list, "ips", admin::banlist_ips);
    builder.bind_literal(list, "players", admin::banlist_players);
}

fn register_pardon(builder: &mut Builder, root: usize) {
    let pardon = builder.literal(root, "pardon", permission::ADMINS);
    let targets = builder.argument(pardon, "targets", ArgumentType::GameProfile, 0);
    builder.run(targets, admin::pardon);
}

fn register_pardon_ip(builder: &mut Builder, root: usize) {
    let pardon = builder.literal(root, "pardon-ip", permission::ADMINS);
    let target = builder.argument(
        pardon,
        "target",
        ArgumentType::String(StringKind::SingleWord),
        0,
    );
    builder.run(target, admin::pardon_ip);
}

fn register_whitelist(builder: &mut Builder, root: usize) {
    let list = builder.literal(root, "whitelist", permission::ADMINS);
    builder.bind_literal(list, "on", admin::whitelist_enable);
    builder.bind_literal(list, "off", admin::whitelist_disable);
    builder.bind_literal(list, "list", admin::whitelist_list);
    builder.bind_literal(list, "reload", admin::whitelist_reload);
    let add = builder.literal(list, "add", 0);
    let added = builder.argument(add, "targets", ArgumentType::GameProfile, 0);
    builder.run(added, admin::whitelist_add);
    let remove = builder.literal(list, "remove", 0);
    let removed = builder.argument(remove, "targets", ArgumentType::GameProfile, 0);
    builder.run(removed, admin::whitelist_remove);
}

fn register_stop(builder: &mut Builder, root: usize) {
    let stop = builder.literal(root, "stop", permission::OWNERS);
    builder.run(stop, admin::stop);
}

fn register_set_idle_timeout(builder: &mut Builder, root: usize) {
    let command = builder.literal(root, "setidletimeout", permission::ADMINS);
    let minutes = builder.argument(
        command,
        "minutes",
        ArgumentType::Integer {
            min: 0,
            max: i32::MAX,
        },
        0,
    );
    builder.run(minutes, admin::set_idle_timeout);
}

fn register_transfer(builder: &mut Builder, root: usize) {
    let command = builder.literal(root, "transfer", permission::ADMINS);
    let hostname = builder.argument(
        command,
        "hostname",
        ArgumentType::String(StringKind::Quotable),
        0,
    );
    builder.run(hostname, admin::transfer);
    let port = builder.argument(
        hostname,
        "port",
        ArgumentType::Integer { min: 1, max: 65535 },
        0,
    );
    builder.run(port, admin::transfer);
    let players = builder.argument(port, "players", players_argument(), 0);
    builder.run(players, admin::transfer);
}

fn register_version(builder: &mut Builder, root: usize) {
    let version = builder.literal(root, "version", permission::GAMEMASTERS);
    builder.run(version, admin::version);
}

fn players_argument() -> ArgumentType {
    ArgumentType::Entity {
        single: false,
        players_only: true,
    }
}

fn entity_argument() -> ArgumentType {
    ArgumentType::Entity {
        single: true,
        players_only: false,
    }
}

fn entities_argument() -> ArgumentType {
    ArgumentType::Entity {
        single: false,
        players_only: false,
    }
}
