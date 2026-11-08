//! 官方 `NbtPathArgument.parse`。路径在空格处停，调用方再要求整段字符串都被吃掉。
//!
//! `NbtPathArgument.parse`. The path stops at a space; the caller then requires
//! the whole string to have been consumed.

use super::syntax::{Reader, take_compound};

/// 整段文本是不是一条合法的 NBT 路径。空字符串官方也收下。
///
/// Whether the whole text is an NBT path. The empty string is accepted.
pub(crate) fn parse(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    let mut reader = Reader::new(&chars);
    let mut first = true;
    while reader.can_read() && reader.peek() != Some(' ') {
        if !node(&mut reader, first) {
            return false;
        }
        first = false;
        if !reader.can_read() {
            break;
        }
        match reader.peek() {
            Some(' ' | '[' | '{') => {}
            _ => {
                if reader.expect('.').is_err() {
                    return false;
                }
            }
        }
    }
    !reader.can_read()
}

fn node(reader: &mut Reader<'_>, first: bool) -> bool {
    match reader.peek() {
        Some('"' | '\'') => {
            let Ok(name) = reader.read_string() else {
                return false;
            };
            object(reader, &name)
        }
        Some('[') => index(reader),
        Some('{') if first => take_compound(reader),
        Some('{') => false,
        _ => {
            let name = unquoted_name(reader);
            object(reader, &name)
        }
    }
}

fn object(reader: &mut Reader<'_>, name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    if reader.peek() == Some('{') {
        take_compound(reader)
    } else {
        true
    }
}

fn index(reader: &mut Reader<'_>) -> bool {
    if reader.expect('[').is_err() {
        return false;
    }
    if reader.peek() == Some('{') {
        return take_compound(reader) && reader.expect(']').is_ok();
    }
    if reader.peek() == Some(']') {
        reader.skip();
        return true;
    }
    reader.read_int().is_ok() && reader.expect(']').is_ok()
}

fn unquoted_name(reader: &mut Reader<'_>) -> String {
    let start = reader.pos();
    while reader
        .peek()
        .is_some_and(|c| !matches!(c, ' ' | '"' | '\'' | '[' | ']' | '.' | '{' | '}'))
    {
        reader.skip();
    }
    let end = reader.pos();
    reader.slice_from(start)[..end - start].iter().collect()
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn paths_match_the_official_grammar() {
        assert!(parse(""));
        assert!(parse("Items"));
        assert!(parse("foo.bar"));
        assert!(parse("foo[0]"));
        assert!(parse("[]"));
        assert!(parse("{foo:1}"));
        assert!(!parse("foo "));
        assert!(!parse("."));
        assert!(!parse("foo{"));
    }
}
