//! 新命令根节点和官方 `commands.json` 逐节点对比。
//!
//! 只比较本分支注册的根，别的命令由各自的分支负责。

use std::{collections::BTreeMap, fs, path::Path};

use mc_bamboo_command::{ArgumentType, CommandGraph, NodeKind, StringKind, permission};
use serde_json::Value;

/// 本分支新增的根节点，按名字排列。
const ROOTS: &[&str] = &[
    "clone",
    "defaultgamemode",
    "enchant",
    "random",
    "difficulty",
    "seed",
    "setworldspawn",
    "spawnpoint",
    "stopsound",
];

/// 从官方报告里取一个根节点。
fn official(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/vanilla/generated/reports/commands.json");
    let report: Value = serde_json::from_str(
        &fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display())),
    )
    .expect("commands.json is valid JSON");
    report["children"][name].clone()
}

/// 把一棵树的一个节点转成官方报告的形状。
fn describe(graph: &CommandGraph, id: usize) -> Value {
    let node = graph.node(id);
    let mut object = BTreeMap::new();
    match &node.kind {
        NodeKind::Root => {
            object.insert("type", "root".into());
        }
        NodeKind::Literal(_) => {
            object.insert("type", "literal".into());
        }
        NodeKind::Argument { name: _, kind } => {
            object.insert("type", "argument".into());
            // 官方报告里节点名就是它在 children 里的键，不在节点里重复写。
            object.insert("parser", kind.parser_name().into());
            if let Some(properties) = argument_properties(kind) {
                object.insert("properties", properties);
            }
        }
    }
    if node.executable {
        object.insert("executable", true.into());
    }
    if let Some(redirect) = node.redirect {
        let target = graph.node(redirect);
        let name = match &target.kind {
            NodeKind::Literal(name) => name.clone(),
            _ => panic!("redirect to a non-literal"),
        };
        object.insert("redirect", name.into());
    }
    if node.requires > 0 {
        object.insert("permissions", permission_object(node.requires));
    }
    let children: BTreeMap<String, Value> = node
        .children
        .iter()
        .map(|&child| (child_key(graph, child), describe(graph, child)))
        .collect();
    if !children.is_empty() {
        object.insert("children", serde_json::to_value(children).unwrap());
    }
    serde_json::to_value(object).unwrap()
}

/// 官方报告用子节点名做键。
fn child_key(graph: &CommandGraph, id: usize) -> String {
    match &graph.node(id).kind {
        NodeKind::Literal(name) => name.clone(),
        NodeKind::Argument { name, .. } => name.clone(),
        NodeKind::Root => "root".to_owned(),
    }
}

fn permission_object(level: u8) -> Value {
    let level = match level {
        permission::MODERATORS => "moderators",
        permission::GAMEMASTERS => "gamemasters",
        permission::ADMINS => "admins",
        permission::OWNERS => "owners",
        _ => panic!("unexpected permission level {level}"),
    };
    serde_json::json!({
        "type": "minecraft:require",
        "permission": {
            "type": "minecraft:command_level",
            "level": level,
        }
    })
}

/// 参数类型和属性。没有属性时返回 `None`，和报告里省略 `properties` 一致。
fn argument_properties(kind: &ArgumentType) -> Option<Value> {
    Some(match *kind {
        ArgumentType::Integer {
            min: i32::MIN,
            max: i32::MAX,
        } => return None,
        ArgumentType::Integer { min, max: i32::MAX } => serde_json::json!({"min": min}),
        ArgumentType::Integer { min, max } => serde_json::json!({"min": min, "max": max}),
        ArgumentType::Long {
            min: i64::MIN,
            max: i64::MAX,
        } => return None,
        ArgumentType::Long { min, max } => serde_json::json!({"min": min, "max": max}),
        ArgumentType::Float { min, max } => serde_json::json!({"min": min, "max": max}),
        ArgumentType::Double { min, max } => serde_json::json!({"min": min, "max": max}),
        ArgumentType::String(kind) => {
            let kind = match kind {
                StringKind::SingleWord => "word",
                StringKind::Quotable => "quotable",
                StringKind::Greedy => "greedy",
            };
            serde_json::json!({"type": kind})
        }
        ArgumentType::Entity {
            single,
            players_only,
        } => serde_json::json!({
            "type": if players_only { "players" } else { "entities" },
            "amount": if single { "single" } else { "multiple" },
        }),
        ArgumentType::Time { minimum } => serde_json::json!({"min": minimum}),
        ArgumentType::Resource { registry } => serde_json::json!({"registry": registry}),
        _ => return None,
    })
}

/// 去掉报告里 Bamboo 还没实现的自定义补全字段；注册时按官方补上即可。
fn normalize(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(key, _)| key.as_str() != "suggestions")
                .map(|(key, value)| (key.clone(), normalize(value)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(normalize).collect()),
        _ => value.clone(),
    }
}

#[test]
fn new_roots_match_the_official_report() {
    let (graph, actions) = mc_bamboo_game::test_commands();
    let root = graph.root();
    for &name in ROOTS {
        let child =
            graph.node(root).children.iter().copied().find(
                |&id| matches!(&graph.node(id).kind, NodeKind::Literal(text) if text == name),
            );
        let child = child.unwrap_or_else(|| panic!("missing root command {name}"));
        let ours = describe(&graph, child);
        let expected = normalize(&official(name));
        assert_eq!(ours, expected, "command tree of /{name}");
        // 官方报告写 `executable` 的节点必须有动作，其余节点可以没有。
        if expected.get("executable") == Some(&Value::Bool(true)) {
            assert!(actions[child], "/{name} root must be executable");
        }
    }
}
