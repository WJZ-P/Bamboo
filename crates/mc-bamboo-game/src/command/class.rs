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

/// 4.4 的表：51 个顶层字面量各属哪一类（别名单列），加上后来接入的 `/summon`。
/// `/summon` 只动本岛的实体表，归本岛类。
///
/// The 4.4 table: the class of each of the 51 root literals (aliases listed),
/// plus `/summon`, which only touches the island's own entity table.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) const CLASSES: &[(&str, CommandClass)] = &[
    ("setblock", CommandClass::Island),
    ("summon", CommandClass::Island),
    ("fill", CommandClass::Island),
    ("clone", CommandClass::Island),
    ("particle", CommandClass::Island),
    ("playsound", CommandClass::Island),
    ("loot", CommandClass::Island),
    ("random", CommandClass::Island),
    ("enchant", CommandClass::Island),
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
];

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
        // 4.4 的 51 个加上 `/summon`。
        // The 51 of 4.4 plus `/summon`.
        assert_eq!(roots.len(), 52);
        assert_eq!(CLASSES.len(), 52);
        let names: BTreeSet<&str> = CLASSES.iter().map(|(name, _)| *name).collect();
        assert_eq!(names.len(), CLASSES.len(), "表里不该有重复");
        for (name, root) in roots {
            let class = CLASSES
                .iter()
                .find(|(listed, _)| *listed == name)
                .map(|(_, class)| *class)
                .unwrap_or_else(|| panic!("{name} 没有分类"));
            let kinds = action_kinds(root);
            let expected = BTreeSet::from([class == CommandClass::Global]);
            assert_eq!(
                kinds, expected,
                "{name} 的动作和 4.4 的类别 {class:?} 不一致"
            );
        }
    }
}
