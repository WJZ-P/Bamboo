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

impl Mode {
    fn name(self) -> &'static str {
        match self {
            Self::Only => "only",
            Self::From => "from",
            Self::Through => "through",
            Self::Until => "until",
            Self::Everything => "everything",
        }
    }
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
    let mode = ["only", "from", "through", "until", "everything"]
        .iter()
        .find(|name| parsed.get(name).is_some())
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
    if count == 0 {
        return Err(fail(if grant {
            "commands.advancement.grant.criterion_not_found"
        } else {
            "commands.advancement.revoke.criterion_not_found"
        }));
    }
    let key = if grant {
        "commands.advancement.grant.success"
    } else {
        "commands.advancement.revoke.success"
    };
    let message = Component::translate(
        key,
        [
            TranslateArg::from(targets.len().to_string()),
            TranslateArg::from(mode.name().to_owned()),
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
