//! 消息参数。整段文字原样保存，选择器单独记下位置，执行时按权限决定要不要解析。
//!
//! 官方用 UTF-16 下标记录选择器位置，这里用字符下标。只有非基本平面的字符会差一两个下标，
//! 解析本身不受影响，执行时也用同一套下标切分文字。

use crate::{
    ErrorArg, ParseError, Value,
    argument::entity::parse_entity,
    reader::StringReader,
    value::{Message, MessagePart},
};

/// 官方 `MessageArgument.TOO_LONG` 的上限。
const MAX_MESSAGE_LENGTH: usize = 256;

/// 官方 `MessageArgument.Message.parseText`。
///
/// `allow_selectors` 为假时整段当文字，不记选择器，对应没有实体选择器权限的来源。
///
/// Official `MessageArgument.Message.parseText`. When `allow_selectors` is false the
/// whole span stays text and no selectors are recorded, matching a source without
/// entity-selector permission.
pub fn parse_message(
    reader: &mut StringReader,
    allow_selectors: bool,
) -> Result<Value, ParseError> {
    let remaining = reader.remaining_length();
    if remaining > MAX_MESSAGE_LENGTH {
        return Err(ParseError::with_args(
            "argument.message.too_long",
            [
                ErrorArg::Int(remaining as i64),
                ErrorArg::Int(MAX_MESSAGE_LENGTH as i64),
            ],
        ));
    }
    let text = reader.remaining();
    // 不允许选择器时官方不扫描 `@`，坏选择器也不会让解析失败。
    //
    // Without selector permission the official parser does not scan for `@`, so a broken
    // selector does not fail parsing.
    if !allow_selectors {
        reader.set_cursor(reader.total_length());
        return Ok(Value::Message(Message {
            text,
            parts: Vec::new(),
        }));
    }
    let offset = reader.cursor();
    let mut parts = Vec::new();

    loop {
        if !reader.can_read() {
            return Ok(Value::Message(Message { text, parts }));
        }
        if reader.peek() != '@' {
            reader.skip();
            continue;
        }

        let start = reader.cursor();
        match parse_entity(reader, false, false) {
            Ok(Value::Entity(selector)) => parts.push(MessagePart {
                start: start - offset,
                end: reader.cursor() - offset,
                selector: *selector,
            }),
            Ok(_) => unreachable!("实体参数只会返回 Value::Entity"),
            // 官方只吞掉“@ 后面不是合法选择器类型”的两种情况，其它错误照常抛出。
            Err(error)
                if error.key == "argument.entity.selector.missing"
                    || error.key == "argument.entity.selector.unknown" =>
            {
                reader.set_cursor(start + 1);
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(input: &str) -> Message {
        match parse_message(&mut StringReader::new(input), true).unwrap() {
            Value::Message(message) => message,
            other => panic!("不是消息：{other:?}"),
        }
    }

    #[test]
    fn plain_text_is_kept_whole() {
        let message = message("hello world");
        assert_eq!(message.text, "hello world");
        assert!(message.parts.is_empty());
    }

    #[test]
    fn selectors_are_recorded_with_their_offsets() {
        let message = message("hi @a and @p[x=1]");
        assert_eq!(message.text, "hi @a and @p[x=1]");
        assert_eq!(message.parts.len(), 2);
        assert_eq!((message.parts[0].start, message.parts[0].end), (3, 5));
        assert_eq!((message.parts[1].start, message.parts[1].end), (10, 17));
        assert_eq!(message.parts[0].selector.max_results, i32::MAX);
        assert_eq!(message.parts[1].selector.x, Some(1.0));
    }

    #[test]
    fn at_signs_that_are_not_selectors_are_text() {
        // `@e` 其实是合法的选择器；这里用 `@f` 和 `@x` 这两个不存在的类型。
        let message = message("mail me@foo.com @x");
        assert!(message.parts.is_empty());
        assert_eq!(message.text, "mail me@foo.com @x");
    }

    #[test]
    fn messages_longer_than_256_are_rejected() {
        let input = "a".repeat(257);
        let error = parse_message(&mut StringReader::new(&input), true).unwrap_err();
        assert_eq!(error.key, "argument.message.too_long");
        assert_eq!(error.args, [ErrorArg::Int(257), ErrorArg::Int(256)]);
    }
}
