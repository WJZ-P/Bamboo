//! 文本组件参数。官方 `ComponentArgument` 用 SNBT 语法读一个 NBT 字面量，
//! 再交给 `ComponentSerialization.CODEC` 校验。
//!
//! 校验和重编码在 `mc-bamboo-protocol` 里做，结果存成标签给所有命令共用。
//! 出错时游标退回参数开头，和官方 `CommandArgumentParser.withCodec` 一致。
//!
//! The component argument. Official `ComponentArgument` reads one SNBT literal and
//! validates it with `ComponentSerialization.CODEC`. The protocol crate does the
//! validation; all commands share the resulting tag.

use mc_bamboo_nbt::{Tag, parse_snbt_prefix};
use mc_bamboo_protocol::components::text::checked_component;

use crate::{ParseError, Value, reader::StringReader};

/// `ComponentArgument.parse`：读一个 SNBT 字面量，再过官方文本 `Codec`。
///
/// 校验和重编码在这里一次做完，计分板、队伍、Boss 栏的命令直接拿结果用。
/// 失败时按官方 `CommandArgumentParser.withCodec` 把游标退回参数开头。
///
/// Parses one SNBT literal and validates it with the official text codec, so
/// scoreboard, team and boss bar commands can use the result directly.
pub fn parse_component(reader: &mut StringReader) -> Result<Value, ParseError> {
    let start = reader.cursor();
    let rest = reader.remaining();
    let (tag, used) = match parse_snbt_prefix(&rest) {
        Ok(parsed) => parsed,
        Err(error) => {
            return Err(ParseError::text("argument.component.invalid", error.message));
        }
    };
    let Some(checked) = checked_component(&tag) else {
        return Err(ParseError::text(
            "argument.component.invalid",
            "invalid component",
        ));
    };
    reader.set_cursor(start + used);
    Ok(Value::Component(checked.tag().clone()))
}

/// 从已经校验过的标签构造组件，供命令执行时用。
pub fn component_tag(tag: Tag) -> mc_bamboo_protocol::text::Component {
    mc_bamboo_protocol::components::text::component_from_checked_tag(tag)
}

#[cfg(test)]
mod tests {
    use mc_bamboo_nbt::Tag;

    use super::*;

    #[test]
    fn components_are_snbt_literals_and_stop_at_the_next_argument() {
        let mut reader = StringReader::new("{text:\"hi\"} 3");
        let value = parse_component(&mut reader).unwrap();
        assert_eq!(
            value.as_component(),
            Some(&Tag::String("hi".into()))
        );
        assert_eq!(reader.remaining(), " 3");
    }

    #[test]
    fn bare_words_and_quoted_strings_are_literals() {
        for input in ["hi", "\"hello world\"", "'hi'", "[\"a\",\"b\"]"] {
            let mut reader = StringReader::new(input);
            assert!(parse_component(&mut reader).is_ok(), "{input}");
            assert_eq!(reader.remaining(), "");
        }
    }

    #[test]
    fn broken_components_report_the_official_key_without_consuming_input() {
        let mut reader = StringReader::new("{text:\"hi\"");
        let error = parse_component(&mut reader).unwrap_err();
        assert_eq!(error.key, "argument.component.invalid");
        assert_eq!(reader.cursor(), 0);
    }
}
