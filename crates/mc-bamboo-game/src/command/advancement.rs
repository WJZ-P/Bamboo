//! `/advancement grant|revoke <玩家> only|from|through|until|everything [判据]`。
//!
//! 语法、反馈和返回值照官方 `AdvancementCommands`。范围是 `Players`：只读改玩家的
//! 进度状态和出站链路，不碰方块或实体，借岛规划器按实际读写把它归到玩家类。

use mc_bamboo_advancements::AdvancementTree;
use mc_bamboo_command::{ParsedCommand, Value};
use mc_bamboo_protocol::text::{Component, TranslateArg};

use crate::{
    command::{
        feedback::{self, CommandFail, fail},
        players, select,
    },
    inbound::PlayerId,
    region::Region,
};

/// 官方 `AdvancementCommands` 的五种范围。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Only,
    From,
    Through,
    Until,
    Everything,
}

impl Mode {}

/// 官方 26.3 `AdvancementCommands$Action` 拼出来的 24 个反馈键，按
/// `(grant/revoke, 一名/多名玩家, criterion/one/many, success/failure)` 索引。
fn feedback_key(
    grant: bool,
    one_player: bool,
    amount: FeedbackAmount,
    success: bool,
) -> &'static str {
    const KEYS: [[[[&str; 2]; 3]; 2]; 2] = [
        // grant
        [
            // one player
            [
                [
                    "commands.advancement.grant.one.to.criterion.failure",
                    "commands.advancement.grant.one.to.criterion.success",
                ],
                [
                    "commands.advancement.grant.one.to.one.failure",
                    "commands.advancement.grant.one.to.one.success",
                ],
                [
                    "commands.advancement.grant.one.to.many.failure",
                    "commands.advancement.grant.one.to.many.success",
                ],
            ],
            // many players
            [
                [
                    "commands.advancement.grant.many.to.criterion.failure",
                    "commands.advancement.grant.many.to.criterion.success",
                ],
                [
                    "commands.advancement.grant.many.to.one.failure",
                    "commands.advancement.grant.many.to.one.success",
                ],
                [
                    "commands.advancement.grant.many.to.many.failure",
                    "commands.advancement.grant.many.to.many.success",
                ],
            ],
        ],
        // revoke
        [
            [
                [
                    "commands.advancement.revoke.one.to.criterion.failure",
                    "commands.advancement.revoke.one.to.criterion.success",
                ],
                [
                    "commands.advancement.revoke.one.to.one.failure",
                    "commands.advancement.revoke.one.to.one.success",
                ],
                [
                    "commands.advancement.revoke.one.to.many.failure",
                    "commands.advancement.revoke.one.to.many.success",
                ],
            ],
            [
                [
                    "commands.advancement.revoke.many.to.criterion.failure",
                    "commands.advancement.revoke.many.to.criterion.success",
                ],
                [
                    "commands.advancement.revoke.many.to.one.failure",
                    "commands.advancement.revoke.many.to.one.success",
                ],
                [
                    "commands.advancement.revoke.many.to.many.failure",
                    "commands.advancement.revoke.many.to.many.success",
                ],
            ],
        ],
    ];
    KEYS[usize::from(!grant)][usize::from(!one_player)][amount as usize][usize::from(success)]
}

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
enum FeedbackAmount {
    Criterion = 0,
    One = 1,
    Many = 2,
}

/// 选出这次命令要动的进度下标。
fn selected(tree: &AdvancementTree, mode: Mode, target: usize) -> Vec<usize> {
    let mut ancestors = Vec::new();
    let mut current = Some(target);
    while let Some(index) = current {
        ancestors.push(index);
        current = tree.nodes[index].parent;
    }
    let descendants = |root: usize| -> Vec<usize> {
        let mut result = Vec::new();
        let mut stack = vec![root];
        while let Some(index) = stack.pop() {
            result.push(index);
            stack.extend(tree.nodes[index].children.iter().copied());
        }
        result
    };
    match mode {
        Mode::Only => vec![target],
        Mode::From => descendants(target),
        Mode::Through => {
            let mut result = descendants(target);
            result.extend(ancestors.iter().skip(1).copied());
            result
        }
        Mode::Until => ancestors,
        Mode::Everything => (0..tree.nodes.len()).collect(),
    }
}

/// 解析 `advancement` 参数：完整标识符字符串。
fn advancement_id(parsed: &ParsedCommand) -> Result<String, CommandFail> {
    parsed
        .get("advancement")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| fail("command.failed"))
}

/// 官方 `AdvancementCommands.grant/revoke`：返回改动的判据数量。
pub(crate) fn apply(
    port: &mut crate::global_port::GlobalPort<'_>,
    region: &mut Region,
    source: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    apply_inner(port, region, source, parsed, true)
}

fn apply_inner(
    port: &mut crate::global_port::GlobalPort<'_>,
    region: &mut Region,
    source: PlayerId,
    parsed: &ParsedCommand,
    grant: bool,
) -> Result<(), CommandFail> {
    let _ = port;
    let tree = AdvancementTree::global();
    let selector = parsed
        .get("target")
        .and_then(Value::as_entity)
        .ok_or_else(|| fail("command.failed"))?;
    let targets = select::players(region, source, players::level_of(region, source), selector)?;
    // 字面量节点不进 `parsed.arguments`，只能从解析出的节点链里找。
    let nodes = crate::command::commands().graph.clone();
    let literals_on_path: Vec<&str> = parsed
        .nodes
        .iter()
        .filter_map(|&node| match &nodes.node(node).kind {
            mc_bamboo_command::NodeKind::Literal(name) => Some(name.as_str()),
            _ => None,
        })
        .collect();
    let mode = ["only", "from", "through", "until", "everything"]
        .iter()
        .find(|name| literals_on_path.contains(name))
        .and_then(|name| match *name {
            "only" => Some(Mode::Only),
            "from" => Some(Mode::From),
            "through" => Some(Mode::Through),
            "until" => Some(Mode::Until),
            "everything" => Some(Mode::Everything),
            _ => None,
        })
        .unwrap_or(Mode::Everything);
    let criterion = parsed
        .get("criterion")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let root = match mode {
        Mode::Everything => None,
        _ => {
            let id = advancement_id(parsed)?;
            let index = tree
                .index(&id)
                .ok_or_else(|| fail("commands.advancement.advancement_not_found"))?;
            Some(index)
        }
    };
    let indices: Vec<usize> = match root {
        Some(index) => selected(tree, mode, index),
        None => (0..tree.nodes.len()).collect(),
    };
    let mut count = 0usize;
    for target in &targets {
        let Some(player) = region.players.get_mut(target) else {
            continue;
        };
        let mut state = std::mem::take(&mut player.advancements);
        for &index in &indices {
            let names: Vec<String> = tree.nodes[index]
                .advancement
                .criteria
                .iter()
                .map(|(name, _)| name.clone())
                .filter(|name| criterion.as_ref().is_none_or(|only| only == name))
                .collect();
            for name in names {
                if grant {
                    if state.award(tree, index, &name) {
                        count += 1;
                    }
                } else if state.revoke(tree, index, &name) {
                    count += 1;
                }
            }
        }
        player.advancements = state;
    }
    let one_player = targets.len() == 1;
    let amount = if criterion.is_some() {
        FeedbackAmount::Criterion
    } else if indices.len() == 1 {
        FeedbackAmount::One
    } else {
        FeedbackAmount::Many
    };
    if count == 0 {
        return Err(fail(feedback_key(grant, one_player, amount, false)));
    }
    let key = feedback_key(grant, one_player, amount, true);
    let action = if grant { "grant" } else { "revoke" };
    let message = Component::translate(
        key,
        [
            TranslateArg::from(targets.len().to_string()),
            TranslateArg::from(action.to_owned()),
        ],
    );
    feedback::success(region, source, message, true);
    Ok(())
}

/// `/advancement revoke` 的入口：和 grant 共用逻辑，只把 grant 标记翻过来。
pub(crate) fn revoke(
    port: &mut crate::global_port::GlobalPort<'_>,
    region: &mut Region,
    source: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    apply_inner(port, region, source, parsed, false)
}
