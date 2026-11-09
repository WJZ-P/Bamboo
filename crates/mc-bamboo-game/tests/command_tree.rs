//! 把这次加的根节点和官方数据生成器导出的 `commands.json` 逐节点对照。
//!
//! 命令树是 `mc-bamboo-game` 私有的，所以测试放在 crate 里，通过公开的
//! [`World`] 拿不到树；这里直接走同 crate 的 `#[cfg(test)]` 桥接函数。

use std::{fs, path::PathBuf};

use serde_json::Value;

/// 官方报告里这 12 个根节点的子树，序列化成和报告同形的 JSON。
#[test]
fn admin_roots_match_the_official_report() {
    let official = official_report();
    let ours = crate_tree();
    for root in ROOTS {
        let expected = official
            .get(*root)
            .unwrap_or_else(|| panic!("官方报告里没有 /{root}"));
        let actual = ours
            .get(*root)
            .unwrap_or_else(|| panic!("Bamboo 的命令树里没有 /{root}"));
        assert_eq!(actual, expected, "/{root} 的命令树和官方不一致");
    }
}

const ROOTS: &[&str] = &[
    "op",
    "deop",
    "ban",
    "ban-ip",
    "banlist",
    "pardon",
    "pardon-ip",
    "whitelist",
    "stop",
    "setidletimeout",
    "transfer",
    "version",
];

fn official_report() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/vanilla/generated/reports/commands.json");
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("官方命令报告应该已经生成：{}：{error}", path.display()));
    let report: Value = serde_json::from_str(&text).expect("官方命令报告应该是合法 JSON");
    let roots = report["children"]
        .as_object()
        .expect("报告的根节点应该是对象");
    Value::Object(
        roots
            .iter()
            .filter(|(name, _)| ROOTS.contains(&name.as_str()))
            .map(|(name, node)| (name.clone(), node.clone()))
            .collect(),
    )
}

/// 和 `commands.json` 一样的形状：Bamboo 树里每个节点转成
/// `{type, parser, properties, executable, permissions, redirect, children}`。
fn crate_tree() -> Value {
    let json = mc_bamboo_game::admin::tests::command_tree_json();
    let roots: Value = serde_json::from_str(&json).expect("树应该是合法 JSON");
    let roots = roots.as_object().expect("树的根节点应该是对象");
    Value::Object(
        roots
            .iter()
            .filter(|(name, _)| ROOTS.contains(&name.as_str()))
            .map(|(name, node)| (name.clone(), node.clone()))
            .collect(),
    )
}
