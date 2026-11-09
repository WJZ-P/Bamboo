//! Brigadier 的解析流程。这里逐行对照官方 `CommandDispatcher.parseNodes` 实现。

use std::cmp::Ordering;

use crate::{
    ParseError, Value,
    graph::{CommandGraph, NodeKind},
    reader::StringReader,
};

/// 解析成功的结果。
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedCommand {
    /// 真正执行的节点。官方 `ContextChain` 里最深一层的命令节点。
    pub action: usize,
    /// 从根到可执行节点的节点 ID。
    pub nodes: Vec<usize>,
    /// 可执行节点所在上下文的参数。
    pub arguments: Vec<(String, Value)>,
    /// 已读到的字符位置。
    pub cursor: usize,
}

impl ParsedCommand {
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.arguments
            .iter()
            .find(|(argument, _)| argument == name)
            .map(|(_, value)| value)
    }
}

/// 解析失败。`cursor` 是官方异常发生（或命令无法继续）的位置。
#[derive(Debug, Clone, PartialEq)]
pub struct ParseFailure {
    pub error: ParseError,
    pub cursor: usize,
}

/// 一条补全结果。客户端会把 `[start, start + length)` 换成选中的文本。
#[derive(Debug, Clone, PartialEq)]
pub struct SuggestionList {
    /// 起点的字符位置。官方 `Suggestions` 里 range 的开头。
    pub start: usize,
    /// 被替换的字符数，等于光标减去起点。
    pub length: usize,
    pub values: Vec<String>,
}

/// Brigadier `CommandContextBuilder` 里的一条已解析节点。
#[derive(Debug, Clone, Copy)]
struct ParsedNode {
    node: usize,
    start: usize,
    end: usize,
}

/// Brigadier `CommandContextBuilder`。重定向时通过 `child` 串起来。
#[derive(Debug, Clone)]
struct Context {
    nodes: Vec<ParsedNode>,
    arguments: Vec<(String, Value)>,
    command: Option<usize>,
    range: (usize, usize),
    child: Option<Box<Self>>,
}

fn collect_signable(context: &Context, out: &mut Vec<(String, String)>) {
    for (name, value) in &context.arguments {
        if let Some(message) = value.as_message() {
            out.push((name.clone(), message.text.clone()));
        }
    }
    if let Some(child) = &context.child {
        collect_signable(child, out);
    }
}

impl Context {
    fn new(start: usize) -> Self {
        Self {
            nodes: Vec::new(),
            arguments: Vec::new(),
            command: None,
            range: (start, start),
            child: None,
        }
    }

    fn with_node(&mut self, node: usize, start: usize, end: usize) {
        self.nodes.push(ParsedNode { node, start, end });
        self.range = (self.range.0.min(start), self.range.1.max(end));
    }

    /// 官方 `ContextChain.tryFlatten` 取到的最深一层上下文。
    fn deepest(&self) -> &Self {
        let mut current = self;
        while let Some(child) = &current.child {
            current = child;
        }
        current
    }
}

#[derive(Debug, Clone)]
struct LocatedError {
    error: ParseError,
    cursor: usize,
}

/// Brigadier `ParseResults`。
#[derive(Debug, Clone)]
struct ParseResults {
    context: Context,
    cursor: usize,
    can_read: bool,
    exceptions: Vec<LocatedError>,
}

impl ParseResults {
    fn compare(&self, other: &Self) -> Ordering {
        // 读完的优先，异常少的优先，其余保持原顺序（稳定排序）。
        match (self.can_read, other.can_read) {
            (false, true) => Ordering::Less,
            (true, false) => Ordering::Greater,
            _ => match (self.exceptions.is_empty(), other.exceptions.is_empty()) {
                (true, false) => Ordering::Less,
                (false, true) => Ordering::Greater,
                _ => Ordering::Equal,
            },
        }
    }
}

impl CommandGraph {
    /// 解析一条命令（不带开头的 `/`）。
    pub fn parse(&self, input: &str, permission_level: u8) -> Result<ParsedCommand, ParseFailure> {
        let raw = self.parse_raw(input, permission_level);
        if raw.can_read {
            if raw.exceptions.len() == 1 {
                let exception = &raw.exceptions[0];
                return Err(ParseFailure {
                    error: exception.error.clone(),
                    cursor: exception.cursor,
                });
            }
            let key = if raw.context.range.0 == raw.context.range.1 {
                "command.unknown.command"
            } else {
                "command.unknown.argument"
            };
            return Err(ParseFailure {
                error: ParseError::new(key),
                cursor: raw.cursor,
            });
        }

        let deepest = raw.context.deepest();
        let Some(action) = deepest.command else {
            return Err(ParseFailure {
                error: ParseError::new("command.unknown.command"),
                cursor: raw.cursor,
            });
        };
        Ok(ParsedCommand {
            action,
            nodes: deepest.nodes.iter().map(|node| node.node).collect(),
            arguments: deepest.arguments.clone(),
            cursor: raw.cursor,
        })
    }

    /// 最外层上下文最后一个已经解析成功的节点。
    ///
    /// `/help <command>` 用它找要显示用法的节点。一个词都对不上时返回 `None`，
    /// 和官方 `getContext().getNodes().isEmpty()` 一致。命令没写完但前缀对上时仍然返回那个节点。
    ///
    /// The last successfully parsed node of the outer context. `/help <command>` uses it
    /// to choose a usage node. `None` when no word matched, matching
    /// `getContext().getNodes().isEmpty()`. An incomplete command still returns the node
    /// whose prefix matched.
    pub fn parse_last_node(&self, input: &str, permission_level: u8) -> Option<usize> {
        let raw = self.parse_raw(input, permission_level);
        raw.context.nodes.last().map(|node| node.node)
    }

    /// 这次解析里已经吃进去的 `message` 参数，命令整体失败时也算。
    ///
    /// 官方 `SignableCommand.of` 看的是 `ParseResults` 里解析成功的参数，不要求整条命令能执行。
    /// 返回 `(参数名, 参数原文)`。
    ///
    /// Message arguments consumed by this parse, including when the command as a whole fails.
    ///
    /// Official `SignableCommand.of` reads arguments that parsed inside `ParseResults`.
    /// It does not require the command to be executable. Each entry is `(name, raw text)`.
    pub fn signable_messages(&self, input: &str, permission_level: u8) -> Vec<(String, String)> {
        let raw = self.parse_raw(input, permission_level);
        let mut messages = Vec::new();
        collect_signable(&raw.context, &mut messages);
        messages
    }

    fn parse_raw(&self, input: &str, permission_level: u8) -> ParseResults {
        let mut reader = StringReader::new(input);
        let context = Context::new(reader.cursor());
        self.parse_nodes(self.root(), &mut reader, &context, permission_level)
    }

    /// 官方 `CommandDispatcher.parseNodes`。
    fn parse_nodes(
        &self,
        node: usize,
        reader: &mut StringReader,
        context: &Context,
        permission_level: u8,
    ) -> ParseResults {
        let start = reader.cursor();
        let mut exceptions = Vec::new();
        let mut potentials: Vec<ParseResults> = Vec::new();

        for child in self.relevant_nodes(node, reader) {
            if !self.can_use(child, permission_level) {
                continue;
            }
            let mut child_reader = reader.clone();
            let mut child_context = context.clone();
            if let Err(error) = self.parse_node(
                child,
                &mut child_reader,
                &mut child_context,
                permission_level,
            ) {
                exceptions.push(LocatedError {
                    error,
                    cursor: child_reader.cursor(),
                });
                continue;
            }
            if child_reader.can_read() && child_reader.peek() != ' ' {
                exceptions.push(LocatedError {
                    error: ParseError::new("command.expected.separator"),
                    cursor: child_reader.cursor(),
                });
                continue;
            }
            child_context.command = self.node(child).executable.then_some(child);

            let redirect = self.node(child).redirect;
            let needed = if redirect.is_some() { 1 } else { 2 };
            if child_reader.can_read_n(needed) {
                child_reader.skip();
                if let Some(target) = redirect {
                    let redirected = Context::new(child_reader.cursor());
                    let parsed =
                        self.parse_nodes(target, &mut child_reader, &redirected, permission_level);
                    child_context.child = Some(Box::new(parsed.context));
                    return ParseResults {
                        context: child_context,
                        cursor: parsed.cursor,
                        can_read: parsed.can_read,
                        exceptions: parsed.exceptions,
                    };
                }
                potentials.push(self.parse_nodes(
                    child,
                    &mut child_reader,
                    &child_context,
                    permission_level,
                ));
            } else {
                potentials.push(ParseResults {
                    context: child_context,
                    cursor: child_reader.cursor(),
                    can_read: child_reader.can_read(),
                    exceptions: Vec::new(),
                });
            }
        }

        if !potentials.is_empty() {
            potentials.sort_by(ParseResults::compare);
            return potentials.remove(0);
        }
        ParseResults {
            context: context.clone(),
            cursor: start,
            can_read: reader.can_read(),
            exceptions,
        }
    }

    fn parse_node(
        &self,
        node: usize,
        reader: &mut StringReader,
        context: &mut Context,
        permission_level: u8,
    ) -> Result<(), ParseError> {
        match &self.node(node).kind {
            NodeKind::Root => Ok(()),
            NodeKind::Literal(literal) => {
                let start = reader.cursor();
                let length = literal.chars().count();
                if reader.can_read_n(length)
                    && reader.slice(start, start + length) == literal.as_str()
                {
                    reader.set_cursor(start + length);
                    context.with_node(node, start, start + length);
                    Ok(())
                } else {
                    Err(ParseError::text("argument.literal.incorrect", literal))
                }
            }
            NodeKind::Argument { name, kind } => {
                let start = reader.cursor();
                // 实体选择器权限是游戏管理员（2）。官方 `Permissions.COMMANDS_ENTITY_SELECTORS`。
                //
                // Entity-selector permission is gamemaster (2). Official
                // `Permissions.COMMANDS_ENTITY_SELECTORS`.
                let allow_selectors = permission_level >= crate::permission::GAMEMASTERS;
                let value = kind.parse_with(reader, allow_selectors)?;
                context.arguments.push((name.clone(), value));
                context.with_node(node, start, reader.cursor());
                Ok(())
            }
        }
    }

    /// 官方 `CommandNode.getRelevantNodes`。
    fn relevant_nodes(&self, node: usize, reader: &mut StringReader) -> Vec<usize> {
        let children = &self.node(node).children;
        if children.is_empty() {
            return Vec::new();
        }
        let start = reader.cursor();
        while reader.can_read() && reader.peek() != ' ' {
            reader.skip();
        }
        let text = reader.slice(start, reader.cursor());
        reader.set_cursor(start);

        if let Some(literal) = children.iter().copied().find(
            |&child| matches!(&self.node(child).kind, NodeKind::Literal(name) if name == &text),
        ) {
            return vec![literal];
        }
        children
            .iter()
            .copied()
            .filter(|&child| matches!(self.node(child).kind, NodeKind::Argument { .. }))
            .collect()
    }

    /// 官方 `CommandNode.getUsageText`。
    pub fn usage_text(&self, node: usize) -> String {
        match &self.node(node).kind {
            NodeKind::Root => String::new(),
            NodeKind::Literal(literal) => literal.clone(),
            NodeKind::Argument { name, .. } => format!("<{name}>"),
        }
    }

    /// 官方 `CommandNode.getSmartUsage`：这个节点的每个可执行子节点各一行用法。
    ///
    /// `/help` 直接把这个列表发给玩家，每行前面加 `/`。
    pub fn smart_usage(&self, node: usize, permission_level: u8) -> Vec<String> {
        let optional = self.node(node).executable;
        self.node(node)
            .children
            .iter()
            .filter_map(|&child| self.smart_usage_of(child, permission_level, optional, false))
            .collect()
    }

    /// 官方 `getSmartUsage(node, source, optional, deep)` 的递归部分。
    ///
    /// 叶子节点返回自己的用法文本；只有一个可执行子节点时把子节点的用法接在后面，
    /// 多个子节点时用 `|` 分开。
    fn smart_usage_of(
        &self,
        node: usize,
        permission_level: u8,
        optional: bool,
        deep: bool,
    ) -> Option<String> {
        if !self.can_use(node, permission_level) {
            return None;
        }
        let usage = self.usage_text(node);
        let this = if optional {
            format!("[{usage}]")
        } else {
            usage
        };
        let child_optional = self.node(node).executable;
        let (open, close) = if child_optional {
            ("[", "]")
        } else {
            ("(", ")")
        };
        if !deep {
            if let Some(redirect) = self.node(node).redirect {
                let target = if redirect == self.root() {
                    "...".to_owned()
                } else {
                    format!("-> {}", self.usage_text(redirect))
                };
                return Some(format!("{this} {target}"));
            }
            let children: Vec<usize> = self
                .node(node)
                .children
                .iter()
                .copied()
                .filter(|&child| self.can_use(child, permission_level))
                .collect();
            if children.len() == 1 {
                if let Some(child_usage) = self.smart_usage_of(
                    children[0],
                    permission_level,
                    child_optional,
                    child_optional,
                ) {
                    return Some(format!("{this} {child_usage}"));
                }
            } else if children.len() > 1 {
                let mut alternatives: Vec<String> = Vec::new();
                for &child in &children {
                    if let Some(child_usage) =
                        self.smart_usage_of(child, permission_level, child_optional, true)
                        && !alternatives.contains(&child_usage)
                    {
                        alternatives.push(child_usage);
                    }
                }
                match alternatives.len() {
                    1 => {
                        let usage = &alternatives[0];
                        let body = if child_optional {
                            format!("[{usage}]")
                        } else {
                            usage.clone()
                        };
                        return Some(format!("{this} {body}"));
                    }
                    size if size > 1 => {
                        let mut builder = String::from(open);
                        for (index, &child) in children.iter().enumerate() {
                            if index > 0 {
                                builder.push('|');
                            }
                            builder.push_str(&self.usage_text(child));
                        }
                        builder.push_str(close);
                        return Some(format!("{this} {builder}"));
                    }
                    _ => {}
                }
            }
        }
        Some(this)
    }

    /// 官方 `CommandDispatcher.getCompletionSuggestions`。
    ///
    /// `cursor` 是光标所在的字符位置。补全覆盖字面量、玩家名、游戏模式，以及节点上挂的自定义建议。
    ///
    /// Official `CommandDispatcher.getCompletionSuggestions`. `cursor` is a character offset.
    /// Suggestions cover literals, player names, game modes, and a node's custom provider.
    pub fn suggestions(
        &self,
        input: &str,
        cursor: usize,
        permission_level: u8,
        player_names: &[&str],
    ) -> SuggestionList {
        let raw = self.parse_raw(input, permission_level);
        let chars: Vec<char> = input.chars().collect();
        let cursor = cursor.min(chars.len());
        let suggestion = self.find_suggestion_context(&raw.context, cursor);
        let start = suggestion.start.min(cursor);
        let remaining: String = chars[start..cursor].iter().collect();
        let lower = remaining.to_lowercase();

        let mut values = Vec::new();
        for &child in &self.node(suggestion.parent).children {
            if !self.can_use(child, permission_level) {
                continue;
            }
            let suggest = self.node(child).suggest;
            match &self.node(child).kind {
                NodeKind::Literal(literal) => {
                    // 官方比较的是小写：`/Gamemode` 也补全成 `gamemode`。
                    if literal.to_lowercase().starts_with(&lower) {
                        values.push(literal.clone());
                    }
                }
                NodeKind::Argument { kind, .. } => {
                    if let Some(suggest) = suggest {
                        suggest(&suggestion.arguments, &lower, &mut values);
                    } else {
                        kind.suggest(&lower, player_names, &mut values);
                    }
                }
                NodeKind::Root => {}
            }
        }
        SuggestionList {
            start,
            length: cursor - start,
            values,
        }
    }

    fn find_suggestion_context(&self, context: &Context, cursor: usize) -> SuggestionTarget {
        let arguments = context.arguments.clone();
        if context.range.0 > cursor {
            return SuggestionTarget {
                parent: context.nodes.last().map_or(self.root(), |node| node.node),
                start: cursor,
                arguments,
            };
        }
        if context.range.1 < cursor {
            if let Some(child) = &context.child {
                return self.find_suggestion_context(child, cursor);
            }
            if let Some(last) = context.nodes.last() {
                return SuggestionTarget {
                    parent: last.node,
                    start: last.end + 1,
                    arguments,
                };
            }
            return SuggestionTarget {
                parent: self.root(),
                start: context.range.0,
                arguments,
            };
        }

        let mut parent = self.root();
        for parsed in &context.nodes {
            if parsed.start <= cursor && cursor <= parsed.end {
                return SuggestionTarget {
                    parent,
                    start: parsed.start,
                    arguments,
                };
            }
            parent = parsed.node;
        }
        SuggestionTarget {
            parent,
            start: context.range.0,
            arguments,
        }
    }
}

/// 官方 `SuggestionContext` 里用到的两个字段。
struct SuggestionTarget {
    parent: usize,
    start: usize,
    arguments: Vec<(String, Value)>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::argument::{ArgumentType, StringKind};
    use crate::value::Coordinates;

    fn integer(min: i32, max: i32) -> ArgumentType {
        ArgumentType::Integer { min, max }
    }

    /// 建一棵 `gamemode <mode>` 的小树。
    fn game_mode_graph() -> (CommandGraph, usize) {
        let mut graph = CommandGraph::new();
        let root = graph.root();
        let gamemode = graph.literal(root, "gamemode", 2);
        let mode = graph.argument(gamemode, "mode", ArgumentType::GameMode, 0);
        graph.executable(mode);
        (graph, mode)
    }

    #[test]
    fn parses_literals_and_arguments() {
        let (graph, action) = game_mode_graph();
        let parsed = graph.parse("gamemode creative", 2).unwrap();
        assert_eq!(parsed.action, action);
        assert_eq!(
            parsed.get("mode").and_then(Value::as_game_mode),
            Some(crate::GameMode::Creative)
        );
        assert_eq!(parsed.cursor, 17);
    }

    #[test]
    fn permissions_hide_whole_subtrees() {
        let (graph, _) = game_mode_graph();
        let failure = graph.parse("gamemode creative", 1).unwrap_err();
        assert_eq!(failure.error.key, "command.unknown.command");
        assert_eq!(failure.cursor, 0);
    }

    #[test]
    fn unknown_commands_and_arguments_use_different_keys() {
        let (graph, _) = game_mode_graph();
        assert_eq!(
            graph.parse("nope", 2).unwrap_err().error.key,
            "command.unknown.command"
        );
        assert_eq!(
            graph.parse("gamemode nope", 2).unwrap_err().error.key,
            "argument.gamemode.invalid"
        );
        let mut graph2 = CommandGraph::new();
        let root = graph2.root();
        let command = graph2.literal(root, "count", 0);
        let number = graph2.argument(command, "value", integer(i32::MIN, i32::MAX), 0);
        graph2.executable(number);
        assert_eq!(
            graph2.parse("count nope", 0).unwrap_err().error.key,
            "parsing.int.expected"
        );
        assert_eq!(
            graph2.parse("count 1x", 0).unwrap_err().error.key,
            "command.expected.separator"
        );
    }

    #[test]
    fn trailing_input_reports_where_parsing_stopped() {
        let mut graph = CommandGraph::new();
        let root = graph.root();
        let command = graph.literal(root, "count", 0);
        let number = graph.argument(command, "value", integer(i32::MIN, i32::MAX), 0);
        graph.executable(number);
        let failure = graph.parse("count 1x", 0).unwrap_err();
        assert_eq!(failure.cursor, 7);
    }

    #[test]
    fn literals_take_precedence_over_arguments() {
        let mut graph = CommandGraph::new();
        let root = graph.root();
        let command = graph.literal(root, "set", 0);
        let literal = graph.literal(command, "day", 0);
        graph.executable(literal);
        let argument = graph.argument(command, "value", integer(i32::MIN, i32::MAX), 0);
        graph.executable(argument);

        let parsed = graph.parse("set day", 0).unwrap();
        assert_eq!(parsed.action, literal);
        let parsed = graph.parse("set 1", 0).unwrap();
        assert_eq!(parsed.action, argument);
    }

    #[test]
    fn redirects_parse_the_target_subtree() {
        let mut graph = CommandGraph::new();
        let root = graph.root();
        let teleport = graph.literal(root, "teleport", 2);
        let location = graph.argument(teleport, "location", ArgumentType::Vec3, 0);
        graph.executable(location);
        let tp = graph.literal(root, "tp", 2);
        graph.redirect(tp, teleport);

        let parsed = graph.parse("tp 1 2 3", 2).unwrap();
        assert_eq!(parsed.action, location);
        assert_eq!(
            parsed
                .get("location")
                .and_then(Value::as_vec3)
                .map(|position| { position.position(mc_bamboo_core::Vec3::default(), 0.0, 0.0) }),
            Some(mc_bamboo_core::Vec3::new(1.5, 2.0, 3.5))
        );
        // 只有别名、没有参数时不能执行。
        assert_eq!(
            graph.parse("tp", 2).unwrap_err().error.key,
            "command.unknown.command"
        );
    }

    #[test]
    fn a_failed_parse_keeps_message_arguments_that_were_consumed() {
        let mut graph = CommandGraph::new();
        let root = graph.root();
        let say = graph.literal(root, "say", 0);
        let message = graph.argument(say, "message", ArgumentType::Message, 0);
        graph.executable(message);
        let echo = graph.literal(root, "echo", 0);
        let text = graph.argument(echo, "message", ArgumentType::Message, 0);
        let extra = graph.argument(text, "count", ArgumentType::Integer { min: 0, max: 1 }, 0);
        graph.executable(extra);

        assert_eq!(
            graph.signable_messages("say hello", 0),
            [("message".to_owned(), "hello".to_owned())]
        );
        assert!(graph.parse("say", 0).is_err());
        assert!(graph.signable_messages("say", 0).is_empty());
        assert!(graph.parse("nope", 0).is_err());
        assert!(graph.signable_messages("nope", 0).is_empty());
        // `message` 把剩下的字都吃掉了，后面的参数对不上，整条命令失败，但原文还在。
        //
        // `message` consumes the rest, so the following argument fails and the command
        // does not run, but the consumed text is still available.
        assert!(graph.parse("echo hello", 0).is_err());
        assert_eq!(
            graph.signable_messages("echo hello", 0),
            [("message".to_owned(), "hello".to_owned())]
        );
    }

    #[test]
    fn suggestions_cover_literals_and_player_names() {
        let mut graph = CommandGraph::new();
        let root = graph.root();
        let teleport = graph.literal(root, "teleport", 0);
        let targets = graph.argument(
            teleport,
            "targets",
            ArgumentType::Entity {
                single: false,
                players_only: true,
            },
            0,
        );
        graph.executable(targets);
        let say = graph.literal(root, "say", 0);
        let message = graph.argument(say, "message", ArgumentType::Message, 0);
        graph.executable(message);

        assert_eq!(
            graph.suggestions("", 0, 0, &["Steve"]),
            SuggestionList {
                start: 0,
                length: 0,
                values: vec!["teleport".to_owned(), "say".to_owned()],
            }
        );
        assert_eq!(
            graph.suggestions("te", 2, 0, &[]),
            SuggestionList {
                start: 0,
                length: 2,
                values: vec!["teleport".to_owned()],
            }
        );
        assert_eq!(
            graph.suggestions("teleport ", 9, 0, &["Steve", "Alex"]),
            SuggestionList {
                start: 9,
                length: 0,
                values: vec!["Steve".to_owned(), "Alex".to_owned()],
            }
        );
        assert_eq!(
            graph.suggestions("teleport St", 11, 0, &["Steve", "Alex"]),
            SuggestionList {
                start: 9,
                length: 2,
                values: vec!["Steve".to_owned()],
            }
        );
    }

    #[test]
    fn literal_suggestions_ignore_case() {
        let (graph, _) = game_mode_graph();
        assert_eq!(
            graph.suggestions("Game", 4, 2, &[]),
            SuggestionList {
                start: 0,
                length: 4,
                values: vec!["gamemode".to_owned()],
            }
        );
        assert!(graph.suggestions("game", 4, 1, &[]).values.is_empty());
    }

    #[test]
    fn smart_usage_matches_the_official_shapes() {
        let mut graph = CommandGraph::new();
        let root = graph.root();
        let gamemode = graph.literal(root, "gamemode", 2);
        let mode = graph.argument(gamemode, "gamemode", ArgumentType::GameMode, 0);
        graph.executable(mode);
        let target = graph.argument(
            mode,
            "target",
            ArgumentType::Entity {
                single: false,
                players_only: true,
            },
            0,
        );
        graph.executable(target);

        let say = graph.literal(root, "say", 2);
        let message = graph.argument(say, "message", ArgumentType::Message, 0);
        graph.executable(message);

        let teleport = graph.literal(root, "teleport", 2);
        let location = graph.argument(teleport, "location", ArgumentType::Vec3, 0);
        graph.executable(location);
        let tp = graph.literal(root, "tp", 2);
        graph.redirect(tp, teleport);

        assert_eq!(
            graph.smart_usage(root, 2),
            [
                "gamemode <gamemode> [<target>]",
                "say <message>",
                "teleport <location>",
                "tp -> teleport",
            ]
        );
        // 权限不够的节点整棵消失。
        assert_eq!(graph.smart_usage(root, 1), [] as [&str; 0]);
        // `/help gamemode`：最后一个节点的子节点用法不带自己的名字。
        assert_eq!(graph.smart_usage(gamemode, 2), ["<gamemode> [<target>]"]);
    }

    #[test]
    fn parse_last_node_follows_the_parsed_prefix() {
        let (graph, mode) = game_mode_graph();
        assert_eq!(graph.parse_last_node("gamemode creative", 2), Some(mode));
        assert_eq!(graph.parse_last_node("gamemode", 2), Some(graph.root() + 1));
        assert_eq!(graph.parse_last_node("nope", 2), None);
    }

    #[test]
    fn block_and_item_arguments_parse_through_the_graph() {
        let mut graph = CommandGraph::new();
        let root = graph.root();
        let setblock = graph.literal(root, "setblock", 2);
        let pos = graph.argument(setblock, "pos", ArgumentType::BlockPos, 0);
        let block = graph.argument(pos, "block", ArgumentType::BlockState, 0);
        graph.executable(block);

        let parsed = graph.parse("setblock 1 2 3 stone", 2).unwrap();
        let state = parsed.get("block").and_then(Value::as_block_state).unwrap();
        assert_eq!(
            state.state,
            mc_bamboo_blocks::BlockState(
                mc_bamboo_data::block_by_name("minecraft:stone")
                    .unwrap()
                    .default_state
            )
        );
        assert!(matches!(
            parsed.get("pos"),
            Some(Value::BlockPos(Coordinates::World { .. }))
        ));
    }

    #[test]
    fn string_arguments_stop_at_spaces() {
        let mut graph = CommandGraph::new();
        let root = graph.root();
        let command = graph.literal(root, "echo", 0);
        let word = graph.argument(
            command,
            "word",
            ArgumentType::String(StringKind::Quotable),
            0,
        );
        graph.executable(word);
        let parsed = graph.parse("echo \"hello world\"", 0).unwrap();
        assert_eq!(
            parsed.get("word").and_then(Value::as_str),
            Some("hello world")
        );
    }
}
