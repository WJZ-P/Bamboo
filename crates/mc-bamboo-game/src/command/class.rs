//! 命令按岛的实施计划 4.4 分三类。
//!
//! 类别由注册时用的动作决定：全局类注册 [`CommandFn`](super::CommandFn)（拿权威的
//! `Global`，经 `GlobalPort` 交给主控执行），另外两类注册 [`IslandFn`](super::IslandFn)
//! （在岛上当场执行）。这张表是 4.4 的原文，测试核对每个顶层字面量的全部动作都和它一致。
//!
//! Commands fall into the three classes of island plan 4.4. The class follows
//! from the registered action: global commands register a
//! [`CommandFn`](super::CommandFn) (takes the authoritative `Global`, runs on the
//! master through `GlobalPort`); the other two register an
//! [`IslandFn`](super::IslandFn) (runs on the island at once). This table is
//! 4.4 verbatim; a test checks every action under every root literal against it.

/// 岛的实施计划 4.4 的三类命令。
///
/// The three command classes of island plan 4.4.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum CommandClass {
    /// 只动本岛：当场执行。
    ///
    /// Island-local: runs at once.
    Island,
    /// 只动全局：岛把（来源、命令）交给主控执行，反馈晚到。
    ///
    /// Global: the island hands (source, command) to the master; feedback arrives later.
    Global,
    /// 可能跨岛：选择器先在本岛解析；只有一个岛时和本岛类一样当场执行。
    ///
    /// Possibly cross-island: selectors resolve on the island first; with one
    /// island it runs at once like an island command.
    CrossIsland,
}

/// 4.4 的表：51 个顶层字面量各属哪一类（别名单列），加上后来接入的
/// `/summon`、`/kill`、`/execute`、`/data`、`/experience` 和 `/xp`。
///
/// The 4.4 table: the class of each of the 51 root literals (aliases listed),
/// plus `/summon`, `/kill`, `/execute`, `/data`, `/experience` and `/xp`.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) const CLASSES: &[(&str, CommandClass)] = &[
    ("setblock", CommandClass::Island),
    ("summon", CommandClass::Island),
    ("fill", CommandClass::Island),
    ("clone", CommandClass::Island),
    ("particle", CommandClass::Island),
    ("playsound", CommandClass::Island),
    ("enchant", CommandClass::Island),
    ("data", CommandClass::Global),
    ("execute", CommandClass::CrossIsland),
    ("experience", CommandClass::Island),
    ("xp", CommandClass::Island),
    ("stopsound", CommandClass::Island),
    ("spawnpoint", CommandClass::Island),
    ("transfer", CommandClass::Island),
    ("time", CommandClass::Global),
    ("weather", CommandClass::Global),
    ("gamerule", CommandClass::Global),
    ("difficulty", CommandClass::Global),
    ("worldborder", CommandClass::Global),
    ("seed", CommandClass::Global),
    ("setworldspawn", CommandClass::Global),
    ("defaultgamemode", CommandClass::Global),
    ("op", CommandClass::Global),
    ("deop", CommandClass::Global),
    ("ban", CommandClass::Global),
    ("ban-ip", CommandClass::Global),
    ("banlist", CommandClass::Global),
    ("pardon", CommandClass::Global),
    ("pardon-ip", CommandClass::Global),
    ("whitelist", CommandClass::Global),
    ("save-all", CommandClass::Global),
    ("save-on", CommandClass::Global),
    ("save-off", CommandClass::Global),
    ("stop", CommandClass::Global),
    ("setidletimeout", CommandClass::Global),
    ("version", CommandClass::Global),
    ("help", CommandClass::Global),
    ("list", CommandClass::Global),
    ("scoreboard", CommandClass::Global),
    ("team", CommandClass::Global),
    ("bossbar", CommandClass::Global),
    // 按名随机序列全世界一份、在主控上（计划 3.3「岛的随机源」第 2 条）。
    // The named random sequences are one per world, on the master (plan 3.3).
    ("loot", CommandClass::Global),
    ("random", CommandClass::Global),
    ("kill", CommandClass::Island),
    ("damage", CommandClass::Island),
    ("effect", CommandClass::Island),
    ("teleport", CommandClass::CrossIsland),
    ("tp", CommandClass::CrossIsland),
    ("give", CommandClass::CrossIsland),
    ("clear", CommandClass::CrossIsland),
    ("gamemode", CommandClass::CrossIsland),
    ("kick", CommandClass::CrossIsland),
    ("title", CommandClass::CrossIsland),
    ("tellraw", CommandClass::CrossIsland),
    ("msg", CommandClass::CrossIsland),
    ("tell", CommandClass::CrossIsland),
    ("w", CommandClass::CrossIsland),
    ("me", CommandClass::CrossIsland),
    ("say", CommandClass::CrossIsland),
    ("advancement", CommandClass::CrossIsland),
];

/// 一条命令执行时要看得见多大的世界（岛的实施计划 4.5）。多岛时主控按它决定怎么借。
///
/// How much of the world a command must see while it runs (island plan 4.5);
/// with several islands the master borrows accordingly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CommandScope {
    /// 只读写玩家身上的状态、出站链路和全局状态：把别的岛上的玩家借过来就够。
    ///
    /// Only players' own state, their links and global state: borrowing the
    /// other islands' players is enough.
    Players,
    /// 可能读写方块、方块实体、容器或者非玩家实体（含读目标玩家脚下的方块）：全部岛
    /// 临时拼成一个再执行。
    ///
    /// May read or write blocks, block entities, containers or non-player
    /// entities (including the block under a target player): every island is
    /// temporarily joined into one first.
    World,
}

/// 4.5 的表：每个顶层字面量的范围。按代码核过：`World` 的几条用 `Region::block_level`、
/// 容器、实体表，或者经 `players::apply_mode` 读目标玩家脚下的方块；其余只碰 `players` /
/// `links` / `global` / 管理名单 / 发起人岛的随机源。新命令不在表里时测试失败；拿不准的
/// 归 `World`。
///
/// The 4.5 table: the scope of every root literal, checked against the code.
/// The `World` ones use `Region::block_level`, containers or the entity table,
/// or read the block under a target player through `players::apply_mode`; the
/// rest touch only `players`, `links`, `global`, the admin lists and the
/// sender island's random source. A new command missing here fails the test;
/// when in doubt it is `World`.
pub(crate) const SCOPES: &[(&str, CommandScope)] = &[
    ("setblock", CommandScope::World),
    ("fill", CommandScope::World),
    ("clone", CommandScope::World),
    ("summon", CommandScope::World),
    ("loot", CommandScope::World),
    ("gamemode", CommandScope::World),
    ("defaultgamemode", CommandScope::World),
    ("particle", CommandScope::Players),
    ("playsound", CommandScope::Players),
    ("enchant", CommandScope::World),
    ("data", CommandScope::World),
    ("execute", CommandScope::World),
    ("experience", CommandScope::Players),
    ("xp", CommandScope::Players),
    ("stopsound", CommandScope::Players),
    ("spawnpoint", CommandScope::Players),
    ("transfer", CommandScope::Players),
    ("time", CommandScope::Players),
    ("weather", CommandScope::Players),
    ("gamerule", CommandScope::Players),
    ("difficulty", CommandScope::Players),
    ("worldborder", CommandScope::Players),
    ("seed", CommandScope::Players),
    ("setworldspawn", CommandScope::Players),
    ("op", CommandScope::Players),
    ("deop", CommandScope::Players),
    ("ban", CommandScope::Players),
    ("ban-ip", CommandScope::Players),
    ("banlist", CommandScope::Players),
    ("pardon", CommandScope::Players),
    ("pardon-ip", CommandScope::Players),
    ("whitelist", CommandScope::Players),
    ("save-all", CommandScope::Players),
    ("save-on", CommandScope::Players),
    ("save-off", CommandScope::Players),
    ("stop", CommandScope::Players),
    ("setidletimeout", CommandScope::Players),
    ("version", CommandScope::Players),
    ("help", CommandScope::Players),
    ("list", CommandScope::Players),
    ("scoreboard", CommandScope::Players),
    ("team", CommandScope::Players),
    ("bossbar", CommandScope::Players),
    ("random", CommandScope::Players),
    ("kill", CommandScope::World),
    ("damage", CommandScope::World),
    ("effect", CommandScope::World),
    ("teleport", CommandScope::World),
    ("tp", CommandScope::World),
    ("give", CommandScope::Players),
    ("clear", CommandScope::Players),
    ("kick", CommandScope::Players),
    ("title", CommandScope::Players),
    ("tellraw", CommandScope::Players),
    ("msg", CommandScope::Players),
    ("tell", CommandScope::Players),
    ("w", CommandScope::Players),
    ("me", CommandScope::Players),
    ("say", CommandScope::Players),
    ("advancement", CommandScope::Players),
];

/// 一行命令（可以带开头的 `/`）的范围，按它的第一个词查 [`SCOPES`]。认不出来的词解析
/// 不过，只会发一条反馈，按 `Players` 算。
///
/// The scope of a command line (a leading `/` allowed), by its first word in
/// [`SCOPES`]. An unknown word does not parse and only produces feedback, so
/// it counts as `Players`.
pub(crate) fn scope_of(line: &str) -> CommandScope {
    let line = line.strip_prefix('/').unwrap_or(line);
    let root = line.split_whitespace().next().unwrap_or("");
    SCOPES
        .iter()
        .find(|(name, _)| *name == root)
        .map_or(CommandScope::Players, |(_, scope)| *scope)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use mc_bamboo_command::NodeKind;

    use super::*;
    use crate::command::{Action, commands};

    /// 顺着子节点和重定向走遍一个顶层字面量下的全部节点，收集动作的种类。
    fn action_kinds(root: usize) -> BTreeSet<bool> {
        let commands = commands();
        let mut seen = BTreeSet::new();
        let mut stack = vec![root];
        let mut kinds = BTreeSet::new();
        while let Some(node) = stack.pop() {
            if !seen.insert(node) {
                continue;
            }
            if let Some(Some(action)) = commands.actions.get(node) {
                kinds.insert(matches!(action, Action::Global(_)));
            }
            let node = commands.graph.node(node);
            stack.extend(node.children.iter().copied());
            stack.extend(node.redirect);
        }
        kinds
    }

    #[test]
    fn every_root_literal_is_classified_and_registers_matching_actions() {
        let commands = commands();
        let graph = &commands.graph;
        let mut roots = Vec::new();
        for &child in &graph.node(graph.root()).children {
            let NodeKind::Literal(name) = &graph.node(child).kind else {
                panic!("根下只应该有字面量");
            };
            roots.push((name.clone(), child));
        }
        // 4.4 的 51 个加上 `/summon`、`/kill`、`/damage`、`/effect`、`/execute`、
        // `/data`、`/experience`、`/xp` 和 `/advancement`。
        assert_eq!(roots.len(), 60);
        assert_eq!(CLASSES.len(), 60);
        // 每个顶层字面量都定了范围（计划 4.5），不多不少。
        // Every root literal has a scope (plan 4.5), no more and no fewer.
        let scoped: BTreeSet<&str> = SCOPES.iter().map(|(name, _)| *name).collect();
        assert_eq!(scoped.len(), SCOPES.len(), "范围表里不该有重复");
        let registered: BTreeSet<&str> = roots.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(scoped, registered, "范围表和注册的顶层字面量不一致");
        let names: BTreeSet<&str> = CLASSES.iter().map(|(name, _)| *name).collect();
        assert_eq!(names.len(), CLASSES.len(), "表里不该有重复");
        for (name, root) in roots {
            let class = CLASSES
                .iter()
                .find(|(listed, _)| *listed == name)
                .map(|(_, class)| *class)
                .unwrap_or_else(|| panic!("{name} 没有分类"));
            let kinds = action_kinds(root);
            let expected = if name == "execute" {
                // `/execute run` 重定向到整棵树，动作表里自然同时有全局和岛上两类；
                // 这个根本身按 4.4 归可能跨岛类。
                // `/execute run` redirects to the whole tree, so both global and
                // island actions are reachable; the root itself is cross-island.
                BTreeSet::from([false, true])
            } else {
                BTreeSet::from([class == CommandClass::Global])
            };
            assert_eq!(
                kinds, expected,
                "{name} 的动作和 4.4 的类别 {class:?} 不一致"
            );
        }
    }

    #[test]
    fn the_scope_of_a_line_follows_its_first_word() {
        assert_eq!(scope_of("setblock 1 2 3 stone"), CommandScope::World);
        assert_eq!(scope_of("/fill 1 2 3 4 5 6 stone"), CommandScope::World);
        assert_eq!(scope_of("gamemode creative @a"), CommandScope::World);
        assert_eq!(scope_of("msg someone hi"), CommandScope::Players);
        assert_eq!(scope_of("  "), CommandScope::Players);
        assert_eq!(scope_of("nosuchcommand 1"), CommandScope::Players);
    }

    /// 要整个世界的命令只在发起人的权限够用它时才借整个世界：不够的解析一定失败，不拼。
    ///
    /// A whole-world command borrows the whole world only when the sender may
    /// use it; otherwise it cannot parse and nothing is joined.
    #[test]
    fn a_command_the_sender_may_not_use_borrows_only_players() {
        use mc_bamboo_command::permission;
        // 权限门：不够用这条命令的顶层字面量时不能因为一条命令让全服顿一下。
        // Permission gate: a level that may not use the root must not be able
        // to stall the whole server with one command.
        assert!(crate::command::may_use_root(
            "setblock 1 2 3 stone",
            permission::GAMEMASTERS
        ));
        assert!(!crate::command::may_use_root(
            "setblock 1 2 3 stone",
            permission::ALL
        ));
        assert!(!crate::command::may_use_root(
            "/gamemode creative",
            permission::ALL
        ));
        assert!(crate::command::may_use_root("msg a hi", permission::OWNERS));
        assert!(crate::command::may_use_root(
            "fill 0 0 0 1 1 1 stone",
            permission::OWNERS
        ));
    }
}
