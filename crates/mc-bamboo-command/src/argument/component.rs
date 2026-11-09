//! 文本组件参数。官方 `ComponentArgument` 用 SNBT 语法读一个 NBT 字面量，
//! 再交给 `ComponentSerialization.CODEC` 校验。
//!
//! 这里只把字面量解析成标签；校验和重编码在 `mc-bamboo-protocol` 里做，
//! 因为这个 crate 不依赖网络层。出错时游标退回参数开头，和官方
//! `CommandArgumentParser.withCodec` 一致。
//!
//! The component argument. Official `ComponentArgument` reads one SNBT literal and
//! validates it with `ComponentSerialization.CODEC`. This crate only parses the
//! literal; the protocol crate validates and re-encodes it. On failure the cursor
//! returns to the start of the argument.

use mc_bamboo_nbt::parse_snbt_argument;

use crate::{ParseError, Value, reader::StringReader};

/// `ComponentArgument.parse`：读一个 SNBT 字面量。
pub fn parse_component(reader: &mut StringReader) -> Result<Value, ParseError> {
    let start = reader.cursor();
    match parse_snbt_argument(&reader.remaining()) {
        Ok((tag, used)) => {
            reader.set_cursor(start + used);
            Ok(Value::Component(tag))
        }
        Err(error) => Err(ParseError::text(
            "argument.component.invalid",
            error.message,
        )),
    }
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
            Some(&Tag::Compound(vec![(
                "text".to_owned(),
                Tag::String("hi".into())
            )]))
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
