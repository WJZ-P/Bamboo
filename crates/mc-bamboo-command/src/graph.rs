//! 命令树的节点。建树只负责结构，解析和执行在 [`crate::dispatcher`]。

use crate::{Value, argument::ArgumentType};

/// 一个节点的类型。根节点不属于任何命令。
#[derive(Debug, Clone, PartialEq)]
pub enum NodeKind {
    Root,
    Literal(String),
    Argument { name: String, kind: ArgumentType },
}

/// 命令树里的一个节点。下标就是它的 ID。
#[derive(Debug, Clone)]
pub struct Node {
    pub kind: NodeKind,
    /// 使用这个节点需要的权限等级。
    pub requires: u8,
    /// 节点本身是不是可以直接执行，对应 Brigadier 的 `Command` 不为空。
    pub executable: bool,
    /// 重定向目标的节点 ID。有重定向时解析会跳到目标节点。
    pub redirect: Option<usize>,
    /// 自定义补全的标识符。非空时命令树带上官方的自定义补全标志。
    ///
    /// Custom suggestion id. Non-empty values set the official custom-suggestions flag.
    pub suggestion: Option<&'static str>,
    /// 服务端补全。参数是已经解析的参数，以及光标前这段的小写文本。
    ///
    /// Server-side suggestions. The arguments are the values parsed so far and the
    /// lowercased text before the cursor.
    pub suggest: Option<SuggestFn>,
    pub children: Vec<usize>,
}

/// 服务端补全函数。参数是已经解析的参数，以及光标前这段的小写文本。
///
/// A server-side suggestion function. The arguments are the values parsed so far and the
/// lowercased text before the cursor.
pub type SuggestFn = fn(&[(String, Value)], &str, &mut Vec<String>);

/// 一棵命令树。0 号永远是根节点。
#[derive(Debug, Clone)]
pub struct CommandGraph {
    nodes: Vec<Node>,
}

impl Default for CommandGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandGraph {
    pub fn new() -> Self {
        Self {
            nodes: vec![Node {
                kind: NodeKind::Root,
                requires: 0,
                executable: false,
                redirect: None,
                suggestion: None,
                suggest: None,
                children: Vec::new(),
            }],
        }
    }

    pub const fn root(&self) -> usize {
        0
    }

    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    pub fn node(&self, id: usize) -> &Node {
        &self.nodes[id]
    }

    /// 取或新建一个子节点。同名节点会合并，权限取较高的那个。
    pub fn literal(&mut self, parent: usize, name: &str, requires: u8) -> usize {
        if let Some(&existing) = self.nodes[parent].children.iter().find(
            |&&child| matches!(&self.nodes[child].kind, NodeKind::Literal(text) if text == name),
        ) {
            self.nodes[existing].requires = self.nodes[existing].requires.max(requires);
            return existing;
        }
        self.add(parent, NodeKind::Literal(name.to_owned()), requires)
    }

    pub fn argument(
        &mut self,
        parent: usize,
        name: &str,
        kind: ArgumentType,
        requires: u8,
    ) -> usize {
        if let Some(&existing) = self.nodes[parent].children.iter().find(|&&child| {
            matches!(
                &self.nodes[child].kind,
                NodeKind::Argument { name: child_name, kind: child_kind }
                    if child_name == name && child_kind == &kind
            )
        }) {
            self.nodes[existing].requires = self.nodes[existing].requires.max(requires);
            return existing;
        }
        self.add(
            parent,
            NodeKind::Argument {
                name: name.to_owned(),
                kind,
            },
            requires,
        )
    }

    fn add(&mut self, parent: usize, kind: NodeKind, requires: u8) -> usize {
        let id = self.nodes.len();
        self.nodes.push(Node {
            kind,
            requires,
            executable: false,
            redirect: None,
            suggestion: None,
            suggest: None,
            children: Vec::new(),
        });
        self.nodes[parent].children.push(id);
        id
    }

    /// 把节点标记成可执行。
    pub fn executable(&mut self, node: usize) -> usize {
        self.nodes[node].executable = true;
        node
    }

    /// 让一个节点重定向到另一个节点。官方用它做命令别名。
    pub fn redirect(&mut self, node: usize, target: usize) {
        self.nodes[node].redirect = Some(target);
    }

    /// 给参数节点挂上官方的自定义补全。`id` 发给客户端，`suggest` 在服务端产生建议。
    ///
    /// Attaches an official custom suggestion provider. `id` is sent to the client and
    /// `suggest` produces the suggestions on the server.
    pub fn suggest_with(&mut self, node: usize, id: &'static str, suggest: SuggestFn) {
        self.nodes[node].suggestion = Some(id);
        self.nodes[node].suggest = Some(suggest);
    }

    /// 官方 `CommandNode.canUse`。
    pub fn can_use(&self, node: usize, permission_level: u8) -> bool {
        permission_level >= self.nodes[node].requires
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals_merge_and_keep_the_higher_permission() {
        let mut graph = CommandGraph::new();
        let first = graph.literal(graph.root(), "gamemode", 2);
        let second = graph.literal(graph.root(), "gamemode", 3);
        assert_eq!(first, second);
        assert_eq!(graph.node(first).requires, 3);
        assert_eq!(graph.node(graph.root()).children, [first]);
    }

    #[test]
    fn arguments_merge_only_when_name_and_type_match() {
        let mut graph = CommandGraph::new();
        let command = graph.literal(graph.root(), "time", 0);
        let first = graph.argument(
            command,
            "value",
            ArgumentType::Integer { min: 0, max: 10 },
            0,
        );
        let same = graph.argument(
            command,
            "value",
            ArgumentType::Integer { min: 0, max: 10 },
            0,
        );
        let other = graph.argument(command, "value", ArgumentType::Time { minimum: 0 }, 0);
        assert_eq!(first, same);
        assert_ne!(first, other);
    }

    #[test]
    fn permission_levels_gate_nodes() {
        let mut graph = CommandGraph::new();
        let node = graph.literal(graph.root(), "op", 2);
        assert!(!graph.can_use(node, 0));
        assert!(!graph.can_use(node, 1));
        assert!(graph.can_use(node, 2));
        assert!(graph.can_use(node, 4));
    }
}
