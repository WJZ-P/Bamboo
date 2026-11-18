//! 官方 `NbtPathArgument.NbtPath` 的子集：`a.b[2]`、`"带点"` 这类路径。
//!
//! 路径的节点只有「复合标签的键」和「列表的下标 / 全匹配 / 带筛选的复合标签」。
//! 求值返回所有匹配到的标签；写入时只改最后一个节点。官方在路径不存在时
//! 按 `get` / `remove` / `modify` 分别报不同的错，由调用方决定。
//!
//! A subset of official `NbtPathArgument.NbtPath`: `a.b[2]`, quoted keys,
//! list indices and matchers. Evaluation returns all matching tags; writes
//! only touch the last node.

use crate::Tag;

/// 路径的一个节点。
#[derive(Debug, Clone, PartialEq)]
enum Node {
    /// 复合标签里的一个键。
    Key(String),
    /// 列表里的一个下标（负数从末尾数）。
    Index(i32),
    /// 列表里的全部元素。
    All,
    /// 列表里满足 `{键=值}` 的元素；本期只解析，不匹配时返回空。
    Match(Tag),
}

/// 一条 NBT 路径。
#[derive(Debug, Clone, PartialEq)]
pub struct NbtPath {
    text: String,
    nodes: Vec<Node>,
}

/// 路径解析失败。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathError;

impl NbtPath {
    /// 解析一条路径文本。空路径是合法的（指向整条复合标签）。
    pub fn parse(text: &str) -> Result<Self, PathError> {
        let mut nodes = Vec::new();
        let chars: Vec<char> = text.chars().collect();
        let mut at = 0;
        while at < chars.len() {
            if chars[at] == '.' {
                at += 1;
                continue;
            }
            if chars[at] == '[' {
                let (node, next) = parse_bracket(&chars, at)?;
                nodes.push(node);
                at = next;
                continue;
            }
            let (key, next) = parse_key(&chars, at)?;
            nodes.push(Node::Key(key));
            at = next;
        }
        Ok(Self {
            text: text.to_owned(),
            nodes,
        })
    }

    /// 官方 `NbtPath.toString()`：路径原文。
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// 官方 `NbtPath.get(tag)`：所有匹配到的标签（按出现顺序）。
    pub fn get<'a>(&self, root: &'a Tag) -> Vec<&'a Tag> {
        let mut current = vec![root];
        for node in &self.nodes {
            let mut next = Vec::new();
            for tag in current {
                match (node, tag) {
                    (Node::Key(key), Tag::Compound(entries)) => {
                        if let Some((_, value)) = entries.iter().find(|(name, _)| name == key) {
                            next.push(value);
                        }
                    }
                    (Node::Index(index), Tag::List(items)) => {
                        if let Some(value) = at_index(items, *index) {
                            next.push(value);
                        }
                    }
                    (Node::All, Tag::List(items)) => next.extend(items.iter()),
                    (Node::Match(pattern), Tag::List(items)) => {
                        for item in items {
                            if matches_pattern(item, pattern) {
                                next.push(item);
                            }
                        }
                    }
                    _ => {}
                }
            }
            current = next;
        }
        current
    }

    /// 官方 `NbtPath.getOrCreate(tag)`：为写入建出路径；只支持键和整数下标。
    /// 返回叶子节点的可变引用列表。
    pub fn get_mut<'a>(&self, root: &'a mut Tag) -> Vec<&'a mut Tag> {
        let mut current: Vec<&mut Tag> = vec![root];
        for node in &self.nodes {
            let mut next: Vec<&mut Tag> = Vec::new();
            for tag in current {
                match (node, tag) {
                    (Node::Key(key), Tag::Compound(entries)) => {
                        let index = match entries.iter().position(|(name, _)| name == key) {
                            Some(index) => index,
                            None => {
                                entries.push((key.clone(), Tag::Compound(Vec::new())));
                                entries.len() - 1
                            }
                        };
                        next.push(&mut entries[index].1);
                    }
                    (Node::Index(index), Tag::List(items)) => {
                        let position = resolve_index(items.len(), *index);
                        if let Some(position) = position
                            && let Some(value) = items.get_mut(position)
                        {
                            next.push(value);
                        }
                    }
                    (Node::All, Tag::List(items)) => next.extend(items.iter_mut()),
                    _ => {}
                }
            }
            current = next;
        }
        current
    }

    /// 官方 `NbtPath.remove(tag)`：删掉路径末端，返回删掉的条数。
    pub fn remove(&self, root: &mut Tag) -> usize {
        if self.nodes.is_empty() {
            return 0;
        }
        let parents = NbtPath {
            text: self.text.clone(),
            nodes: self.nodes[..self.nodes.len() - 1].to_vec(),
        };
        let last = &self.nodes[self.nodes.len() - 1];
        let mut removed = 0;
        for parent in parents.get_mut(root) {
            match (last, parent) {
                (Node::Key(key), Tag::Compound(entries)) => {
                    let before = entries.len();
                    entries.retain(|(name, _)| name != key);
                    removed += before - entries.len();
                }
                (Node::Index(index), Tag::List(items)) => {
                    if let Some(position) = resolve_index(items.len(), *index) {
                        items.remove(position);
                        removed += 1;
                    }
                }
                (Node::All, Tag::List(items)) => {
                    removed += items.len();
                    items.clear();
                }
                _ => {}
            }
        }
        removed
    }
}

fn at_index(items: &[Tag], index: i32) -> Option<&Tag> {
    resolve_index(items.len(), index).and_then(|position| items.get(position))
}

fn resolve_index(len: usize, index: i32) -> Option<usize> {
    let at = if index < 0 {
        len as i64 + i64::from(index)
    } else {
        i64::from(index)
    };
    (at >= 0 && at < len as i64).then_some(at as usize)
}

fn matches_pattern(tag: &Tag, pattern: &Tag) -> bool {
    let Tag::Compound(entries) = pattern else {
        return false;
    };
    let Tag::Compound(values) = tag else {
        return false;
    };
    entries
        .iter()
        .all(|(key, want)| values.iter().any(|(name, got)| name == key && got == want))
}

fn parse_key(chars: &[char], start: usize) -> Result<(String, usize), PathError> {
    let quoted = matches!(chars.get(start), Some('"' | '\''));
    let quote = if quoted { Some(chars[start]) } else { None };
    let mut at = start + usize::from(quoted);
    let mut out = String::new();
    while at < chars.len() {
        let c = chars[at];
        if let Some(end) = quote {
            if c == end {
                return Ok((out, at + 1));
            }
            if c == '\\' && at + 1 < chars.len() {
                out.push(chars[at + 1]);
                at += 2;
                continue;
            }
            out.push(c);
            at += 1;
        } else if c == '.' || c == '[' {
            break;
        } else {
            out.push(c);
            at += 1;
        }
    }
    if quoted || out.is_empty() {
        return Err(PathError);
    }
    Ok((out, at))
}

fn parse_bracket(chars: &[char], start: usize) -> Result<(Node, usize), PathError> {
    let mut at = start + 1;
    while at < chars.len() && chars[at].is_ascii_whitespace() {
        at += 1;
    }
    if at >= chars.len() {
        return Err(PathError);
    }
    if chars[at] == '{' {
        // 复合标签筛选器：交给 SNBT 解析器读一个值，再原样转成 Tag。
        let text: String = chars[at..].iter().collect();
        let (tag, used) = crate::parse_snbt_prefix(&text).map_err(|_| PathError)?;
        let consumed: usize = text[..used].chars().count();
        at += consumed;
        while at < chars.len() && chars[at].is_ascii_whitespace() {
            at += 1;
        }
        if chars.get(at) != Some(&']') {
            return Err(PathError);
        }
        return Ok((Node::Match(tag), at + 1));
    }
    if chars[at] == ']' {
        return Ok((Node::All, at + 1));
    }
    let mut number = String::new();
    while at < chars.len() && (chars[at].is_ascii_digit() || chars[at] == '-') {
        number.push(chars[at]);
        at += 1;
    }
    while at < chars.len() && chars[at].is_ascii_whitespace() {
        at += 1;
    }
    if chars.get(at) != Some(&']') {
        return Err(PathError);
    }
    let index = number.parse::<i32>().map_err(|_| PathError)?;
    Ok((Node::Index(index), at + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compound(entries: Vec<(&str, Tag)>) -> Tag {
        Tag::Compound(
            entries
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value))
                .collect(),
        )
    }

    #[test]
    fn keys_indices_and_removal() {
        let mut root = compound(vec![("Items", Tag::List(vec![Tag::Int(7), Tag::Int(9)]))]);
        let path = NbtPath::parse("Items[1]").unwrap();
        assert_eq!(path.get(&root), vec![&Tag::Int(9)]);
        assert_eq!(path.remove(&mut root), 1);
        assert_eq!(path.get(&root), Vec::<&Tag>::new());
        assert_eq!(NbtPath::parse("").unwrap().get(&root), vec![&root]);
    }

    #[test]
    fn missing_intermediate_keys_are_created_for_writes() {
        let mut root = compound(vec![]);
        let path = NbtPath::parse("a.b").unwrap();
        for leaf in path.get_mut(&mut root) {
            *leaf = Tag::Int(3);
        }
        assert_eq!(path.get(&root), vec![&Tag::Int(3)]);
    }
}
