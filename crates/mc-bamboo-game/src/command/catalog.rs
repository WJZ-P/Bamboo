//! 第一批命令的语法。根节点的顺序跟官方 `Commands` 构造函数里出现的顺序一致。
//!
//! Grammar of the first commands. Root order follows the official `Commands` constructor.

use mc_bamboo_command::{ArgumentType, CommandGraph, StringKind, permission};

use mc_bamboo_data::gamerules::{GameRuleDef, GameRuleKind, RULES};

use crate::command::{
    CommandFn, Commands, blocks, gamerule, items, players, teleport, time, world_settings,
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
    register_clear(&mut builder, root);
    register_me(&mut builder, root);
    register_fill(&mut builder, root);
    register_gamemode(&mut builder, root);
    register_gamerule(&mut builder, root);
    register_give(&mut builder, root);
    register_help(&mut builder, root);
    register_kick(&mut builder, root);
    register_list(&mut builder, root);
    register_msg(&mut builder, root);
    register_say(&mut builder, root);
    register_setblock(&mut builder, root);
    register_teleport(&mut builder, root);
    register_time(&mut builder, root);
    register_difficulty(&mut builder, root);
    register_default_gamemode(&mut builder, root);
    register_seed(&mut builder, root);
    register_spawns(&mut builder, root);
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

fn register_difficulty(builder: &mut Builder, root: usize) {
    let difficulty = builder.literal(root, "difficulty", permission::GAMEMASTERS);
    builder.run(difficulty, world_settings::query_difficulty);
    // 官方按 `Difficulty.values()` 的顺序枚举，报告里的顺序是字母序，比较用名字查。
    for name in ["peaceful", "easy", "normal", "hard"] {
        builder.bind_literal(difficulty, name, world_settings::difficulty_literal);
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
