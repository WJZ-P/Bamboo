//! 第一批命令的语法。根节点的顺序跟官方 `Commands` 构造函数里出现的顺序一致。
//!
//! Grammar of the first commands. Root order follows the official `Commands` constructor.

use mc_bamboo_command::{ArgumentType, CommandGraph, StringKind, permission};

use mc_bamboo_data::gamerules::{GameRuleDef, GameRuleKind, RULES};

use crate::command::{
    CommandFn, Commands, blocks, clone, enchant, gamerule, items, loot, players, random, stopsound,
    teleport, time, weather, world_settings,
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
