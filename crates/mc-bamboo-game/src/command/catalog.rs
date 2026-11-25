//! 第一批命令的语法。根节点的顺序跟官方 `Commands` 构造函数里出现的顺序一致。
//!
//! Grammar of the first commands. Root order follows the official `Commands` constructor.

use mc_bamboo_command::{ArgumentType, CommandGraph, StringKind, permission};

use mc_bamboo_data::gamerules::{GameRuleDef, GameRuleKind, RULES};

use crate::command::{
    Action, CommandFn, Commands, IslandFn, admin, advancement, blocks, bossbar, clone, damage,
    data, effect, enchant, execute, experience, gamerule, items, kill, loot, particle, players,
    playsound, random, ride, save, scoreboard, stopsound, summon, team, teleport, text, time,
    title, weather, world_border, world_settings,
};

struct Builder {
    graph: CommandGraph,
    actions: Vec<Option<Action>>,
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

    /// 全局类命令（岛的实施计划 4.4）。
    ///
    /// A global command (island plan 4.4).
    fn run(&mut self, node: usize, action: CommandFn) -> usize {
        self.set_action(node, Action::Global(action))
    }

    /// 只动本岛或可能跨岛的命令（4.4）。
    ///
    /// An island-local or possibly cross-island command (4.4).
    fn run_island(&mut self, node: usize, action: IslandFn) -> usize {
        self.set_action(node, Action::Island(action))
    }

    fn set_action(&mut self, node: usize, action: Action) -> usize {
        self.graph.executable(node);
        self.ensure(node);
        self.actions[node] = Some(action);
        node
    }

    fn bind_literal(&mut self, parent: usize, name: &str, action: CommandFn) {
        let node = self.literal(parent, name, 0);
        self.run(node, action);
    }

    fn bind_island(&mut self, parent: usize, name: &str, action: IslandFn) {
        let node = self.literal(parent, name, 0);
        self.run_island(node, action);
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
    register_save(&mut builder, root);
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
    register_kill(&mut builder, root);
    register_list(&mut builder, root);
    register_playsound(&mut builder, root);
    register_msg(&mut builder, root);
    register_particle(&mut builder, root);
    register_say(&mut builder, root);
    register_scoreboard(&mut builder, root);
    register_setblock(&mut builder, root);
    register_summon(&mut builder, root);
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
    register_damage(&mut builder, root);
    register_effect(&mut builder, root);
    register_stopsound(&mut builder, root);
    register_random(&mut builder, root);
    register_enchant(&mut builder, root);
    register_execute(&mut builder, root);
    register_data(&mut builder, root);
    register_experience(&mut builder, root);
    register_loot(&mut builder, root);
    register_weather(&mut builder, root);
    register_advancement(&mut builder, root);
    register_worldborder(&mut builder, root);
    register_ride(&mut builder, root);
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
    builder.run_island(clear, items::clear);
    let targets = builder.argument(clear, "targets", players_argument(), 0);
    builder.run_island(targets, items::clear);
    let item = builder.argument(targets, "item", ArgumentType::ItemPredicate, 0);
    builder.run_island(item, items::clear);
    let count = builder.argument(
        item,
        "maxCount",
        ArgumentType::Integer {
            min: 0,
            max: i32::MAX,
        },
        0,
    );
    builder.run_island(count, items::clear);
}

fn register_me(builder: &mut Builder, root: usize) {
    let me = builder.literal(root, "me", permission::ALL);
    let action = builder.argument(me, "action", ArgumentType::Message, 0);
    builder.run_island(action, players::me);
}

fn register_fill(builder: &mut Builder, root: usize) {
    let fill = builder.literal(root, "fill", permission::GAMEMASTERS);
    let from = builder.argument(fill, "from", ArgumentType::BlockPos, 0);
    let to = builder.argument(from, "to", ArgumentType::BlockPos, 0);
    let block = builder.argument(to, "block", ArgumentType::BlockState, 0);
    wrap_fill_modes(builder, block);
    let replace = builder.literal(block, "replace", 0);
    builder.run_island(replace, blocks::fill_replace);
    let filter = builder.argument(replace, "filter", ArgumentType::BlockPredicate, 0);
    wrap_fill_modes(builder, filter);
    builder.bind_island(block, "keep", blocks::fill_keep);
}

fn wrap_fill_modes(builder: &mut Builder, parent: usize) {
    builder.run_island(parent, blocks::fill_replace);
    builder.bind_island(parent, "outline", blocks::fill_outline);
    builder.bind_island(parent, "hollow", blocks::fill_hollow);
    builder.bind_island(parent, "destroy", blocks::fill_destroy);
    builder.bind_island(parent, "strict", blocks::fill_strict);
}

fn register_gamemode(builder: &mut Builder, root: usize) {
    let gamemode = builder.literal(root, "gamemode", permission::GAMEMASTERS);
    let mode = builder.argument(gamemode, "gamemode", ArgumentType::GameMode, 0);
    builder.run_island(mode, players::gamemode);
    let target = builder.argument(mode, "target", players_argument(), 0);
    builder.run_island(target, players::gamemode);
}

fn register_give(builder: &mut Builder, root: usize) {
    let give = builder.literal(root, "give", permission::GAMEMASTERS);
    let targets = builder.argument(give, "targets", players_argument(), 0);
    let item = builder.argument(targets, "item", ArgumentType::Item, 0);
    builder.run_island(item, items::give);
    let count = builder.argument(
        item,
        "count",
        ArgumentType::Integer {
            min: 1,
            max: i32::MAX,
        },
        0,
    );
    builder.run_island(count, items::give);
}

fn register_help(builder: &mut Builder, root: usize) {
    let help = builder.literal(root, "help", permission::ALL);
    builder.run(help, players::help);
    let command = builder.argument(help, "command", ArgumentType::String(StringKind::Greedy), 0);
    builder.run(command, players::help);
}

/// `KillCommand.register`：权限 2；根节点本身就是 `kill @s`。
///
/// `KillCommand.register`: level 2; the root node is `kill @s` itself.
fn register_kill(builder: &mut Builder, root: usize) {
    let command = builder.literal(root, "kill", permission::GAMEMASTERS);
    builder.run_island(command, kill::kill);
    let targets = builder.argument(command, "targets", entities_argument(), 0);
    builder.run_island(targets, kill::kill);
}
/// `DamageCommand.register`: `<target> <amount> [damageType] [at location | by entity [from cause]]`。
fn register_damage(builder: &mut Builder, root: usize) {
    let command = builder.literal(root, "damage", permission::GAMEMASTERS);
    let target = builder.argument(command, "target", entity_argument(), 0);
    let amount = builder.argument(
        target,
        "amount",
        ArgumentType::Float {
            min: 0.0,
            max: f32::MAX,
        },
        0,
    );
    builder.run_island(amount, damage::damage);
    let kind = builder.argument(
        amount,
        "damageType",
        ArgumentType::Resource {
            registry: "minecraft:damage_type",
        },
        0,
    );
    builder.run_island(kind, damage::damage);
    let at = builder.literal(kind, "at", 0);
    let location = builder.argument(at, "location", ArgumentType::Vec3, 0);
    builder.run_island(location, damage::damage);
    let by = builder.literal(kind, "by", 0);
    let entity = builder.argument(by, "entity", entity_argument(), 0);
    builder.run_island(entity, damage::damage);
    let from = builder.literal(entity, "from", 0);
    let cause = builder.argument(from, "cause", entity_argument(), 0);
    builder.run_island(cause, damage::damage);
}

/// `EffectCommands.register`：权限 2；`clear [targets] [effect]` 和
/// `give <targets> <effect> [seconds | infinite] [amplifier] [hideParticles]`。
/// 秒数的范围、等级上限和 `infinite` 的位置都照官方命令树。
fn register_effect(builder: &mut Builder, root: usize) {
    let command = builder.literal(root, "effect", permission::GAMEMASTERS);
    let effect_argument = || ArgumentType::Resource {
        registry: "minecraft:mob_effect",
    };
    let amplifier = || ArgumentType::Integer { min: 0, max: 255 };
    // ---- clear ----
    let clear = builder.literal(command, "clear", 0);
    builder.run_island(clear, effect::clear);
    let targets = builder.argument(clear, "targets", entities_argument(), 0);
    builder.run_island(targets, effect::clear);
    let only = builder.argument(targets, "effect", effect_argument(), 0);
    builder.run_island(only, effect::clear);
    // ---- give ----
    let give = builder.literal(command, "give", 0);
    let targets = builder.argument(give, "targets", entities_argument(), 0);
    let effect = builder.argument(targets, "effect", effect_argument(), 0);
    builder.run_island(effect, effect::give);
    let seconds = builder.argument(
        effect,
        "seconds",
        ArgumentType::Integer {
            min: 1,
            max: 1_000_000,
        },
        0,
    );
    builder.run_island(seconds, effect::give);
    let amplifier_node = builder.argument(seconds, "amplifier", amplifier(), 0);
    builder.run_island(amplifier_node, effect::give);
    let hide = builder.argument(amplifier_node, "hideParticles", ArgumentType::Bool, 0);
    builder.run_island(hide, effect::give);
    // `infinite` 是和 `seconds` 并列的字面量，后面同样可以接等级和隐藏粒子。
    let infinite = builder.literal(effect, "infinite", 0);
    builder.run_island(infinite, effect::give_infinite);
    let infinite_amplifier = builder.argument(infinite, "amplifier", amplifier(), 0);
    builder.run_island(infinite_amplifier, effect::give_infinite);
    let infinite_hide =
        builder.argument(infinite_amplifier, "hideParticles", ArgumentType::Bool, 0);
    builder.run_island(infinite_hide, effect::give_infinite);
    // 官方 `give <effect> infinite` 分支的 `hideParticles` 也可以不带等级。
    let infinite_only_hide = builder.argument(infinite, "hideParticles", ArgumentType::Bool, 0);
    builder.run_island(infinite_only_hide, effect::give_infinite);
}

fn register_kick(builder: &mut Builder, root: usize) {
    let kick = builder.literal(root, "kick", permission::ADMINS);
    let targets = builder.argument(kick, "targets", players_argument(), 0);
    builder.run_island(targets, players::kick);
    let reason = builder.argument(targets, "reason", ArgumentType::Message, 0);
    builder.run_island(reason, players::kick);
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
    builder.run_island(message, players::msg);
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
    builder.run_island(name, particle::normal);
    let pos = builder.argument(name, "pos", ArgumentType::Vec3, 0);
    builder.run_island(pos, particle::normal);
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
    builder.run_island(count, particle::normal);
    let force_action: IslandFn = particle::force;
    let normal_action: IslandFn = particle::normal;
    for (mode, action) in [("force", force_action), ("normal", normal_action)] {
        let branch = builder.literal(count, mode, 0);
        builder.run_island(branch, action);
        let viewers = builder.argument(branch, "viewers", players_argument(), 0);
        builder.run_island(viewers, action);
    }
}

fn register_playsound(builder: &mut Builder, root: usize) {
    let command = builder.literal(root, "playsound", permission::GAMEMASTERS);
    let sound = builder.argument(command, "sound", ArgumentType::ResourceLocation, 0);
    builder
        .graph
        .suggest_with(sound, playsound::SUGGESTION, playsound::suggest_sounds);
    builder.run_island(sound, playsound::sound_only);
    for (name, action) in playsound::SOURCES
        .iter()
        .map(|(name, _)| *name)
        .zip(playsound::ACTIONS)
    {
        let branch = builder.literal(sound, name, 0);
        builder.run_island(branch, action);
        let targets = builder.argument(branch, "targets", players_argument(), 0);
        builder.run_island(targets, action);
        let pos = builder.argument(targets, "pos", ArgumentType::Vec3, 0);
        builder.run_island(pos, action);
        let volume = builder.argument(
            pos,
            "volume",
            ArgumentType::Float {
                min: 0.0,
                max: f32::MAX,
            },
            0,
        );
        builder.run_island(volume, action);
        let pitch = builder.argument(
            volume,
            "pitch",
            ArgumentType::Float { min: 0.0, max: 2.0 },
            0,
        );
        builder.run_island(pitch, action);
        let min_volume = builder.argument(
            pitch,
            "minVolume",
            ArgumentType::Float { min: 0.0, max: 1.0 },
            0,
        );
        builder.run_island(min_volume, action);
    }
}

fn register_say(builder: &mut Builder, root: usize) {
    let say = builder.literal(root, "say", permission::GAMEMASTERS);
    let message = builder.argument(say, "message", ArgumentType::Message, 0);
    builder.run_island(message, players::say);
}

fn register_setblock(builder: &mut Builder, root: usize) {
    let setblock = builder.literal(root, "setblock", permission::GAMEMASTERS);
    let pos = builder.argument(setblock, "pos", ArgumentType::BlockPos, 0);
    let block = builder.argument(pos, "block", ArgumentType::BlockState, 0);
    builder.run_island(block, blocks::setblock_replace);
    builder.bind_island(block, "destroy", blocks::setblock_destroy);
    builder.bind_island(block, "keep", blocks::setblock_keep);
    builder.bind_island(block, "replace", blocks::setblock_replace);
    builder.bind_island(block, "strict", blocks::setblock_strict);
}

/// `SummonCommand.register`：权限 2；`<entity>`、`<entity> <pos>` 跑 `finalizeSpawn`，
/// 带 `<nbt>` 的不跑。
fn register_summon(builder: &mut Builder, root: usize) {
    let command = builder.literal(root, "summon", permission::GAMEMASTERS);
    let entity = builder.argument(
        command,
        "entity",
        ArgumentType::Resource {
            registry: "minecraft:entity_type",
        },
        0,
    );
    builder
        .graph
        .suggest_with(entity, summon::SUGGESTION, summon::suggest_entities);
    builder.run_island(entity, summon::summon_here);
    let pos = builder.argument(entity, "pos", ArgumentType::Vec3, 0);
    builder.run_island(pos, summon::summon_at);
    let nbt = builder.argument(pos, "nbt", ArgumentType::CompoundTag, 0);
    builder.run_island(nbt, summon::summon_with_nbt);
}

/// `TellRawCommand.register`：权限 2，只给每个目标发系统消息，没有成功反馈。
fn register_tellraw(builder: &mut Builder, root: usize) {
    let tellraw = builder.literal(root, "tellraw", permission::GAMEMASTERS);
    let targets = builder.argument(tellraw, "targets", players_argument(), 0);
    let message = builder.argument(targets, "message", ArgumentType::Component, 0);
    builder.run_island(message, text::tellraw);
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
        builder.run_island(node, action_fn(action));
    }
    for (name, action) in [
        ("title", title::Action::Title),
        ("subtitle", title::Action::Subtitle),
        ("actionbar", title::Action::Actionbar),
    ] {
        let branch = builder.literal(targets, name, 0);
        let text = builder.argument(branch, "title", ArgumentType::Component, 0);
        builder.run_island(text, action_fn(action));
    }
    let times = builder.literal(targets, "times", 0);
    let fade_in = builder.argument(times, "fadeIn", ArgumentType::Time { minimum: 0 }, 0);
    let stay = builder.argument(fade_in, "stay", ArgumentType::Time { minimum: 0 }, 0);
    let fade_out = builder.argument(stay, "fadeOut", ArgumentType::Time { minimum: 0 }, 0);
    builder.run_island(fade_out, title::run_times);
}

fn action_fn(action: title::Action) -> IslandFn {
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
    builder.run_island(location, teleport::teleport);
    let destination = builder.argument(teleport, "destination", entity_argument(), 0);
    builder.run_island(destination, teleport::teleport);
    let targets = builder.argument(teleport, "targets", entities_argument(), 0);
    let targets_location = builder.argument(targets, "location", ArgumentType::Vec3, 0);
    builder.run_island(targets_location, teleport::teleport);
    let rotation = builder.argument(targets_location, "rotation", ArgumentType::Rotation, 0);
    builder.run_island(rotation, teleport::teleport);
    let facing = builder.literal(targets_location, "facing", 0);
    let entity = builder.literal(facing, "entity", 0);
    let facing_entity = builder.argument(entity, "facingEntity", entity_argument(), 0);
    builder.run_island(facing_entity, teleport::teleport);
    let anchor = builder.argument(facing_entity, "facingAnchor", ArgumentType::EntityAnchor, 0);
    builder.run_island(anchor, teleport::teleport);
    let facing_location = builder.argument(facing, "facingLocation", ArgumentType::Vec3, 0);
    builder.run_island(facing_location, teleport::teleport);
    let targets_destination = builder.argument(targets, "destination", entity_argument(), 0);
    builder.run_island(targets_destination, teleport::teleport);
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
    builder.run_island(strict, clone::strict_replace_normal);
    clone_filter_modes(builder, strict, true);
}

fn clone_modes(builder: &mut Builder, parent: usize, strict: bool) {
    let normal = if strict {
        clone::strict_replace_normal
    } else {
        clone::replace_normal
    };
    builder.run_island(parent, normal);
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
    let actions: [IslandFn; 3] = match (filter, strict) {
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
    builder.run_island(parent, actions[0]);
    builder.bind_island(parent, "force", actions[1]);
    builder.bind_island(parent, "move", actions[2]);
    builder.bind_island(parent, "normal", actions[0]);
}

fn register_stopsound(builder: &mut Builder, root: usize) {
    let command = builder.literal(root, "stopsound", permission::GAMEMASTERS);
    let targets = builder.argument(command, "targets", players_argument(), 0);
    builder.run_island(targets, stopsound::stop_sound);
    let all = builder.literal(targets, "*", 0);
    add_sound_name(builder, all);
    for name in [
        "master", "music", "record", "weather", "block", "hostile", "neutral", "player", "ambient",
        "voice", "ui",
    ] {
        let source = builder.literal(targets, name, 0);
        builder.run_island(source, stopsound::stop_sound);
        add_sound_name(builder, source);
    }
}

fn add_sound_name(builder: &mut Builder, parent: usize) {
    let sound = builder.argument(parent, "sound", ArgumentType::ResourceLocation, 0);
    builder.run_island(sound, stopsound::stop_sound);
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

/// `ExecuteCommand.register`：权限 2。链上每一步都重定向回 `execute` 根，
/// `run` 重定向到整棵命令树的根；只有 `run` 后面的命令真正执行。
///
/// `ExecuteCommand.register`: level 2. Every modifier redirects to the
/// `execute` root; `run` redirects to the command-tree root.
fn register_execute(builder: &mut Builder, root: usize) {
    let execute = builder.literal(root, "execute", permission::GAMEMASTERS);
    let back = |builder: &mut Builder, node: usize| builder.graph.redirect(node, execute);

    let run = builder.literal(execute, "run", 0);
    builder.graph.redirect(run, root);
    builder.run(run, execute::execute_global);

    for name in ["as", "at"] {
        let branch = builder.literal(execute, name, 0);
        let targets = builder.argument(branch, "targets", entities_argument(), 0);
        back(builder, targets);
    }

    let positioned = builder.literal(execute, "positioned", 0);
    let pos = builder.argument(positioned, "pos", ArgumentType::Vec3, 0);
    back(builder, pos);
    let as_branch = builder.literal(positioned, "as", 0);
    let targets = builder.argument(as_branch, "targets", entities_argument(), 0);
    back(builder, targets);
    let over = builder.literal(positioned, "over", 0);
    let heightmap = builder.argument(over, "heightmap", ArgumentType::Heightmap, 0);
    back(builder, heightmap);

    let rotated = builder.literal(execute, "rotated", 0);
    let rot = builder.argument(rotated, "rot", ArgumentType::Rotation, 0);
    back(builder, rot);
    let as_branch = builder.literal(rotated, "as", 0);
    let targets = builder.argument(as_branch, "targets", entities_argument(), 0);
    back(builder, targets);

    let facing = builder.literal(execute, "facing", 0);
    let pos = builder.argument(facing, "pos", ArgumentType::Vec3, 0);
    back(builder, pos);
    let entity = builder.literal(facing, "entity", 0);
    let targets = builder.argument(entity, "targets", entities_argument(), 0);
    let anchor = builder.argument(targets, "anchor", ArgumentType::EntityAnchor, 0);
    back(builder, anchor);

    let align = builder.literal(execute, "align", 0);
    let axes = builder.argument(align, "axes", ArgumentType::Swizzle, 0);
    back(builder, axes);
    let anchored = builder.literal(execute, "anchored", 0);
    let anchor = builder.argument(anchored, "anchor", ArgumentType::EntityAnchor, 0);
    back(builder, anchor);
    let in_branch = builder.literal(execute, "in", 0);
    let dimension = builder.argument(in_branch, "dimension", ArgumentType::Dimension, 0);
    back(builder, dimension);

    for name in ["if", "unless"] {
        let branch = builder.literal(execute, name, 0);
        let block = builder.literal(branch, "block", 0);
        let pos = builder.argument(block, "pos", ArgumentType::BlockPos, 0);
        let predicate = builder.argument(pos, "block", ArgumentType::BlockPredicate, 0);
        builder.run(predicate, execute::execute_global);
        back(builder, predicate);

        let blocks = builder.literal(branch, "blocks", 0);
        let start = builder.argument(blocks, "start", ArgumentType::BlockPos, 0);
        let end = builder.argument(start, "end", ArgumentType::BlockPos, 0);
        let destination = builder.argument(end, "destination", ArgumentType::BlockPos, 0);
        for mode in ["all", "masked"] {
            let node = builder.literal(destination, mode, 0);
            builder.run(node, execute::execute_global);
            back(builder, node);
        }

        let entity = builder.literal(branch, "entity", 0);
        let entities = builder.argument(entity, "entities", entities_argument(), 0);
        builder.run(entities, execute::execute_global);
        back(builder, entities);

        let score = builder.literal(branch, "score", 0);
        let target = builder.argument(
            score,
            "target",
            ArgumentType::ScoreHolder { multiple: false },
            0,
        );
        let objective = builder.argument(target, "targetObjective", ArgumentType::Objective, 0);
        let matches = builder.literal(objective, "matches", 0);
        let range = builder.argument(matches, "range", ArgumentType::IntRange, 0);
        builder.run(range, execute::execute_global);
        back(builder, range);
        for op in ["<", "<=", "=", ">=", ">"] {
            let cmp = builder.literal(objective, op, 0);
            let source = builder.argument(
                cmp,
                "source",
                ArgumentType::ScoreHolder { multiple: false },
                0,
            );
            let source_objective =
                builder.argument(source, "sourceObjective", ArgumentType::Objective, 0);
            builder.run(source_objective, execute::execute_global);
            back(builder, source_objective);
        }

        let data = builder.literal(branch, "data", 0);
        for data_kind in ["block", "entity", "storage"] {
            let node = builder.literal(data, data_kind, 0);
            let source = builder.argument(
                node,
                "source",
                if data_kind == "block" {
                    ArgumentType::BlockPos
                } else if data_kind == "entity" {
                    entity_argument()
                } else {
                    ArgumentType::ResourceLocation
                },
                0,
            );
            let path = builder.argument(source, "path", ArgumentType::NbtPath, 0);
            builder.run(path, execute::execute_global);
            back(builder, path);
        }

        let predicate = builder.literal(branch, "predicate", 0);
        let id = builder.argument(predicate, "predicate", ArgumentType::LootPredicate, 0);
        builder.run(id, execute::execute_global);
        back(builder, id);
    }

    for name in ["result", "success"] {
        let store = builder.literal(execute, "store", 0);
        let kind = builder.literal(store, name, 0);
        let score = builder.literal(kind, "score", 0);
        let targets = builder.argument(
            score,
            "targets",
            ArgumentType::ScoreHolder { multiple: true },
            0,
        );
        let objective = builder.argument(targets, "objective", ArgumentType::Objective, 0);
        back(builder, objective);
        let bossbar = builder.literal(kind, "bossbar", 0);
        let id = builder.argument(bossbar, "id", ArgumentType::ResourceLocation, 0);
        for field in ["value", "max"] {
            let node = builder.literal(id, field, 0);
            back(builder, node);
        }
        for target in ["block", "entity", "storage"] {
            let kind_node = builder.literal(kind, target, 0);
            let source = builder.argument(
                kind_node,
                "target",
                if target == "block" {
                    ArgumentType::BlockPos
                } else if target == "entity" {
                    ArgumentType::Entity {
                        single: true,
                        players_only: false,
                    }
                } else {
                    ArgumentType::ResourceLocation
                },
                0,
            );
            let path = builder.argument(source, "path", ArgumentType::NbtPath, 0);
            for ty in ["byte", "short", "int", "long", "float", "double"] {
                let node = builder.literal(path, ty, 0);
                let scale = builder.argument(
                    node,
                    "scale",
                    ArgumentType::Double {
                        min: f64::MIN,
                        max: f64::MAX,
                    },
                    0,
                );
                back(builder, scale);
            }
        }
    }
    // 整棵 `/execute` 的动作都绑在这一个入口：执行器从 `ParsedCommand.chain` 读链。
    // Every `/execute` node binds this one entry; the runner reads `chain`.
    builder.run(execute, execute::execute_global);
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
    builder.run_island(enchantment, enchant::enchant);
    let level = builder.argument(
        enchantment,
        "level",
        ArgumentType::Integer {
            min: 0,
            max: i32::MAX,
        },
        0,
    );
    builder.run_island(level, enchant::enchant);
}

/// 官方 `ExperienceCommand.register`：`experience` 是主名，`xp` 重定向到它。
/// `add` / `set` 的 `amount` 节点本身可执行（默认 `points`），`query` 必须带后缀。
///
/// Official `ExperienceCommand.register`: `experience` is the main name, `xp`
/// redirects to it. The `amount` node of `add` / `set` is executable by itself
/// (defaulting to `points`); `query` requires a suffix.
/// `DataCommands.register`：官方树 751 个节点，按每个数据源一套模板展开。
/// 所有可执行节点都绑到同一个动作，执行器从 `ParsedCommand.nodes` 认出分支。
///
/// `DataCommands.register`: 751 official nodes, built from one template per
/// target kind. Every executable node binds the one action; the runner reads
/// the branch names from `ParsedCommand.nodes`.
fn register_data(builder: &mut Builder, root: usize) {
    let command = builder.literal(root, "data", permission::GAMEMASTERS);
    for kind in ["block", "entity", "storage"] {
        let target_kind = |builder: &mut Builder, parent: usize, name: &str| -> usize {
            match kind {
                "block" => builder.argument(parent, name, ArgumentType::BlockPos, 0),
                "entity" => builder.argument(parent, name, entity_argument(), 0),
                _ => builder.argument(parent, name, ArgumentType::ResourceLocation, 0),
            }
        };

        let get = builder.literal(command, "get", 0);
        let get_kind = builder.literal(get, kind, 0);
        let target = target_kind(builder, get_kind, "target");
        bind(builder, target);
        let path = builder.argument(target, "path", ArgumentType::NbtPath, 0);
        bind(builder, path);
        let scale = builder.argument(
            path,
            "scale",
            ArgumentType::Double {
                min: f64::MIN,
                max: f64::MAX,
            },
            0,
        );
        bind(builder, scale);

        let merge = builder.literal(command, "merge", 0);
        let merge_kind = builder.literal(merge, kind, 0);
        let target = target_kind(builder, merge_kind, "target");
        let nbt = builder.argument(target, "nbt", ArgumentType::CompoundTag, 0);
        bind(builder, nbt);

        let remove = builder.literal(command, "remove", 0);
        let remove_kind = builder.literal(remove, kind, 0);
        let target = target_kind(builder, remove_kind, "target");
        let path = builder.argument(target, "path", ArgumentType::NbtPath, 0);
        bind(builder, path);

        let modify = builder.literal(command, "modify", 0);
        let modify_kind = builder.literal(modify, kind, 0);
        let target = target_kind(builder, modify_kind, "target");
        let target_path = builder.argument(target, "targetPath", ArgumentType::NbtPath, 0);
        for op in ["append", "merge", "prepend", "set"] {
            let branch = builder.literal(target_path, op, 0);
            add_data_sources(builder, branch);
        }
        let insert = builder.literal(target_path, "insert", 0);
        let index = builder.argument(
            insert,
            "index",
            ArgumentType::Integer {
                min: i32::MIN,
                max: i32::MAX,
            },
            0,
        );
        add_data_sources(builder, index);
    }
}

/// `/data modify` 的四种来源后缀：`from`、`value`、`string`、`compute`。
/// 三种目标共用一个模板；每个可执行节点都绑同一个动作。
fn add_data_sources(builder: &mut Builder, parent: usize) {
    let source_kind = |builder: &mut Builder, parent: usize, kind: &str, name: &str| -> usize {
        match kind {
            "block" => builder.argument(parent, name, ArgumentType::BlockPos, 0),
            "entity" => builder.argument(parent, name, entity_argument(), 0),
            _ => builder.argument(parent, name, ArgumentType::ResourceLocation, 0),
        }
    };
    let integer = || ArgumentType::Integer {
        min: i32::MIN,
        max: i32::MAX,
    };

    let value = builder.literal(parent, "value", 0);
    let tag = builder.argument(value, "value", ArgumentType::NbtTag, 0);
    bind(builder, tag);

    let from = builder.literal(parent, "from", 0);
    let string = builder.literal(parent, "string", 0);
    let compute = builder.literal(parent, "compute", 0);
    for kind in ["block", "entity", "storage"] {
        let from_kind = builder.literal(from, kind, 0);
        let source = source_kind(builder, from_kind, kind, "source");
        bind(builder, source);
        let source_path = builder.argument(source, "sourcePath", ArgumentType::NbtPath, 0);
        bind(builder, source_path);

        let string_kind = builder.literal(string, kind, 0);
        let source = source_kind(builder, string_kind, kind, "source");
        bind(builder, source);
        let source_path = builder.argument(source, "sourcePath", ArgumentType::NbtPath, 0);
        bind(builder, source_path);
        let start = builder.argument(source_path, "start", integer(), 0);
        bind(builder, start);
        let end = builder.argument(start, "end", integer(), 0);
        bind(builder, end);

        if kind == "storage" {
            // 官方 `compute` 只有 block / default / entity 三种来源。
            continue;
        }
        let compute_kind = builder.literal(compute, kind, 0);
        let provider_parent = if kind == "block" {
            builder.argument(compute_kind, "computePos", ArgumentType::BlockPos, 0)
        } else {
            builder.argument(compute_kind, "computeTarget", entity_argument(), 0)
        };
        for ty in ["float", "integer"] {
            let branch = builder.literal(provider_parent, ty, 0);
            let provider = builder.argument(
                branch,
                "provider",
                if ty == "float" {
                    ArgumentType::ContextFloatProvider
                } else {
                    ArgumentType::ContextIntProvider
                },
                0,
            );
            bind(builder, provider);
        }
    }
    let default = builder.literal(compute, "default", 0);
    for ty in ["float", "integer"] {
        let branch = builder.literal(default, ty, 0);
        let provider = builder.argument(
            branch,
            "provider",
            if ty == "float" {
                ArgumentType::ContextFloatProvider
            } else {
                ArgumentType::ContextIntProvider
            },
            0,
        );
        bind(builder, provider);
    }
}

fn bind(builder: &mut Builder, node: usize) {
    builder.run(node, data::data);
}

fn register_experience(builder: &mut Builder, root: usize) {
    let command = builder.literal(root, "experience", permission::GAMEMASTERS);

    let add = builder.literal(command, "add", 0);
    let target = builder.argument(add, "target", players_argument(), 0);
    let amount = builder.argument(
        target,
        "amount",
        ArgumentType::Integer {
            min: i32::MIN,
            max: i32::MAX,
        },
        0,
    );
    builder.run_island(amount, experience::add_points);
    builder.bind_island(amount, "points", experience::add_points);
    builder.bind_island(amount, "levels", experience::add_levels);

    let set = builder.literal(command, "set", 0);
    let target = builder.argument(set, "target", players_argument(), 0);
    let amount = builder.argument(
        target,
        "amount",
        ArgumentType::Integer {
            min: 0,
            max: i32::MAX,
        },
        0,
    );
    builder.run_island(amount, experience::set_points);
    builder.bind_island(amount, "points", experience::set_points);
    builder.bind_island(amount, "levels", experience::set_levels);

    let query = builder.literal(command, "query", 0);
    let target = builder.argument(
        query,
        "target",
        ArgumentType::Entity {
            single: true,
            players_only: true,
        },
        0,
    );
    builder.bind_island(target, "points", experience::query_points);
    builder.bind_island(target, "levels", experience::query_levels);

    let alias = builder.literal(root, "xp", permission::GAMEMASTERS);
    builder.graph.redirect(alias, command);
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

/// `WorldBorderCommand.register`：整棵树要等级 2。距离是 `doubleArg(-MAX, MAX)`，
/// 时间是 `TimeArgument.time(0)`，警告距离是 `integer(0)`，伤害两项是 `floatArg(0)`。
///
/// `WorldBorderCommand.register`: level 2 for the whole tree. Distances use
/// `doubleArg(-MAX, MAX)`, times `TimeArgument.time(0)`, the warning distance
/// `integer(0)`, and the two damage values `floatArg(0)`.
fn register_worldborder(builder: &mut Builder, root: usize) {
    let command = builder.literal(root, "worldborder", permission::GAMEMASTERS);
    let distance = || ArgumentType::Double {
        min: -mc_bamboo_blocks::MAX_SIZE,
        max: mc_bamboo_blocks::MAX_SIZE,
    };
    let time = || ArgumentType::Time { minimum: 0 };
    let float = || ArgumentType::Float {
        min: 0.0,
        max: f32::MAX,
    };

    let add = builder.literal(command, "add", 0);
    let add_distance = builder.argument(add, "distance", distance(), 0);
    builder.run(add_distance, world_border::add);
    let add_time = builder.argument(add_distance, "time", time(), 0);
    builder.run(add_time, world_border::add_time);

    let set = builder.literal(command, "set", 0);
    let set_distance = builder.argument(set, "distance", distance(), 0);
    builder.run(set_distance, world_border::set);
    let set_time = builder.argument(set_distance, "time", time(), 0);
    builder.run(set_time, world_border::set_time);

    let center = builder.literal(command, "center", 0);
    let pos = builder.argument(center, "pos", ArgumentType::Vec2, 0);
    builder.run(pos, world_border::center);

    let damage = builder.literal(command, "damage", 0);
    let amount = builder.literal(damage, "amount", 0);
    let per_block = builder.argument(amount, "damagePerBlock", float(), 0);
    builder.run(per_block, world_border::damage_amount);
    let buffer = builder.literal(damage, "buffer", 0);
    let buffer_distance = builder.argument(buffer, "distance", float(), 0);
    builder.run(buffer_distance, world_border::damage_buffer);

    builder.bind_literal(command, "get", world_border::get);

    let warning = builder.literal(command, "warning", 0);
    let warning_distance = builder.literal(warning, "distance", 0);
    let blocks = builder.argument(
        warning_distance,
        "distance",
        ArgumentType::Integer {
            min: 0,
            max: i32::MAX,
        },
        0,
    );
    builder.run(blocks, world_border::warning_distance);
    let warning_time = builder.literal(warning, "time", 0);
    let ticks = builder.argument(warning_time, "time", time(), 0);
    builder.run(ticks, world_border::warning_time);
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
    builder.run_island(spawnpoint, world_settings::spawnpoint);
    let targets = builder.argument(spawnpoint, "targets", players_argument(), 0);
    builder.run_island(targets, world_settings::spawnpoint);
    let pos = builder.argument(targets, "pos", ArgumentType::BlockPos, 0);
    builder.run_island(pos, world_settings::spawnpoint);
    let rotation = builder.argument(pos, "rotation", ArgumentType::Rotation, 0);
    builder.run_island(rotation, world_settings::spawnpoint);
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

fn register_save(builder: &mut Builder, root: usize) {
    // 官方 `SaveAllCommand.register`：save-all 可执行，另有一个 flush 分支。
    let save_all = builder.literal(root, "save-all", permission::OWNERS);
    builder.run(save_all, save::save_all);
    let flush = builder.literal(save_all, "flush", 0);
    builder.run(flush, save::save_all_flush);
    let save_on = builder.literal(root, "save-on", permission::OWNERS);
    builder.run(save_on, save::save_on);
    let save_off = builder.literal(root, "save-off", permission::OWNERS);
    builder.run(save_off, save::save_off);
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
    builder.run_island(hostname, admin::transfer);
    let port = builder.argument(
        hostname,
        "port",
        ArgumentType::Integer { min: 1, max: 65535 },
        0,
    );
    builder.run_island(port, admin::transfer);
    let players = builder.argument(port, "players", players_argument(), 0);
    builder.run_island(players, admin::transfer);
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

/// `RideCommand.register`：`<target> mount <vehicle>` / `<target> dismount`。
fn register_ride(builder: &mut Builder, root: usize) {
    let command = builder.literal(root, "ride", permission::GAMEMASTERS);
    let target = builder.argument(command, "target", entity_argument(), 0);
    let dismount = builder.literal(target, "dismount", 0);
    builder.run_island(dismount, ride::ride);
    let mount = builder.literal(target, "mount", 0);
    let vehicle = builder.argument(mount, "vehicle", entity_argument(), 0);
    builder.run_island(vehicle, ride::ride);
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

/// `/advancement grant|revoke <目标> only|from|through|until|everything [进度] [判据]`。
/// 官方 `AdvancementCommands.register` 的语法。两条动作共用同一个执行函数，
/// 靠 `action` 参数区分；`only` / `from` / `through` / `until` 后面都跟一个进度 id，
/// `everything` 不带进度参数。判据参数是可选的单词。
fn register_advancement(builder: &mut Builder, root: usize) {
    use mc_bamboo_command::ArgumentType;
    let command = builder.literal(root, "advancement", permission::GAMEMASTERS);
    for (action, function) in [
        ("grant", advancement::apply as IslandFn),
        ("revoke", advancement::revoke as IslandFn),
    ] {
        let node = builder.literal(command, action, 0);
        let targets = builder.argument(node, "target", players_argument(), 0);
        let everything = builder.literal(targets, "everything", 0);
        builder.run_island(everything, function);
        for mode in ["only", "from", "through", "until"] {
            let mode_node = builder.literal(targets, mode, 0);
            let id = builder.argument(mode_node, "advancement", ArgumentType::ResourceLocation, 0);
            builder.run_island(id, function);
            let with_criterion = builder.argument(
                id,
                "criterion",
                ArgumentType::String(mc_bamboo_command::StringKind::SingleWord),
                0,
            );
            builder.run_island(with_criterion, function);
        }
    }
}
