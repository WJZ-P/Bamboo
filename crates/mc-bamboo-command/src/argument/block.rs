//! 方块状态和方块谓词参数。官方 `BlockStateParser` 的移植。
//!
//! 方块状态写成 `stone`、`minecraft:oak_stairs[facing=east,half=top]`，
//! 方块谓词额外允许 `#minecraft:logs[axis=y]` 这种标签形式。
//! Bamboo 没有 NBT，遇到 `{...}` 时会明确报不支持。

use mc_bamboo_blocks::BlockState;
use mc_bamboo_data::{block_by_name, block_tag};

use crate::{
    ErrorArg, ParseError, Value,
    identifier::Identifier,
    reader::StringReader,
    value::{BlockPredicateInput, BlockStateInput},
};

const SYNTAX_START_PROPERTIES: char = '[';
const SYNTAX_START_NBT: char = '{';
const SYNTAX_END_PROPERTIES: char = ']';
const SYNTAX_EQUALS: char = '=';
const SYNTAX_PROPERTY_SEPARATOR: char = ',';
const SYNTAX_TAG: char = '#';

/// 解析到一半的结果，标签和方块两条路共用外层的错误回滚。
struct BlockResult {
    input: BlockStateInput,
    tag: Option<(String, Vec<i32>)>,
}

/// `BlockStateArgument.block()`，不允许标签，允许 NBT（Bamboo 会拒绝）。
pub fn parse_block_state(reader: &mut StringReader) -> Result<Value, ParseError> {
    let result = parse_for_block(reader, false)?;
    Ok(Value::BlockState(result.input))
}

/// `BlockPredicateArgument.blockPredicate()`，允许标签和模糊属性。
pub fn parse_block_predicate(reader: &mut StringReader) -> Result<Value, ParseError> {
    let result = parse_for_block(reader, true)?;
    match result.tag {
        Some((tag, block_ids)) => Ok(Value::BlockPredicate(BlockPredicateInput::Tag {
            tag,
            block_ids,
            properties: result.input.properties,
        })),
        None => Ok(Value::BlockPredicate(BlockPredicateInput::State(
            result.input,
        ))),
    }
}

fn parse_for_block(
    reader: &mut StringReader,
    for_testing: bool,
) -> Result<BlockResult, ParseError> {
    let start = reader.cursor();
    match parse_block(reader, for_testing) {
        Ok(result) => Ok(result),
        Err(error) => {
            // 官方 `parseForBlock`/`parseForTesting` 出错时都把光标退回参数开头。
            reader.set_cursor(start);
            Err(error)
        }
    }
}

fn parse_block(reader: &mut StringReader, for_testing: bool) -> Result<BlockResult, ParseError> {
    if for_testing && reader.can_read() && reader.peek() == SYNTAX_TAG {
        let (tag, block_ids) = read_tag(reader)?;
        let mut properties = Vec::new();
        if reader.can_read() && reader.peek() == SYNTAX_START_PROPERTIES {
            read_vague_properties(reader, &tag, &mut properties)?;
        }
        return finish(reader, BlockState(0), properties, Some((tag, block_ids)));
    }

    let (block_name, mut state) = read_block(reader)?;
    let mut properties = Vec::new();
    if reader.can_read() && reader.peek() == SYNTAX_START_PROPERTIES {
        read_properties(reader, &mut state, &block_name, &mut properties)?;
    }
    finish(reader, state, properties, None)
}

/// 检查 NBT 后缀并组装结果。官方允许 NBT，Bamboo 没有 SNBT，只能明确拒绝。
fn finish(
    reader: &mut StringReader,
    state: BlockState,
    properties: Vec<(String, String)>,
    tag: Option<(String, Vec<i32>)>,
) -> Result<BlockResult, ParseError> {
    if reader.can_read() && reader.peek() == SYNTAX_START_NBT {
        return Err(ParseError::text(
            "command.exception",
            "block entity data (SNBT) is not supported by Bamboo",
        ));
    }
    Ok(BlockResult {
        input: BlockStateInput { state, properties },
        tag,
    })
}

/// 官方 `readBlock`。
fn read_block(reader: &mut StringReader) -> Result<(String, BlockState), ParseError> {
    let start = reader.cursor();
    let id = Identifier::read(reader)?;
    let name = id.to_registry_name();
    let Some(block) = block_by_name(&name) else {
        reader.set_cursor(start);
        return Err(ParseError::text("argument.block.id.invalid", name));
    };
    Ok((name, BlockState(block.default_state)))
}

/// 官方 `readTag`，只在测试形式（谓词）里可用。
fn read_tag(reader: &mut StringReader) -> Result<(String, Vec<i32>), ParseError> {
    let start = reader.cursor();
    reader.skip();
    let id = Identifier::read(reader)?;
    let name = id.to_registry_name();
    let Some(ids) = block_tag(&name) else {
        reader.set_cursor(start);
        return Err(ParseError::text("arguments.block.tag.unknown", name));
    };
    Ok((name, ids.to_vec()))
}

/// 官方 `readProperties`。状态本身不在这里改，返回值由调用方按顺序应用。
fn read_properties(
    reader: &mut StringReader,
    state: &mut BlockState,
    block_name: &str,
    properties: &mut Vec<(String, String)>,
) -> Result<(), ParseError> {
    reader.skip();
    reader.skip_whitespace();
    while reader.can_read() && reader.peek() != SYNTAX_END_PROPERTIES {
        reader.skip_whitespace();
        let key_start = reader.cursor();
        let key = reader.read_string()?;
        if state.property_text(&key).is_none() {
            reader.set_cursor(key_start);
            return Err(ParseError::with_args(
                "argument.block.property.unknown",
                [ErrorArg::Text(block_name.into()), ErrorArg::Text(key)],
            ));
        }
        if properties.iter().any(|(name, _)| name == &key) {
            reader.set_cursor(key_start);
            return Err(ParseError::with_args(
                "argument.block.property.duplicate",
                [ErrorArg::Text(key), ErrorArg::Text(block_name.into())],
            ));
        }
        reader.skip_whitespace();
        if !reader.can_read() || reader.peek() != SYNTAX_EQUALS {
            return Err(ParseError::with_args(
                "argument.block.property.novalue",
                [ErrorArg::Text(key), ErrorArg::Text(block_name.into())],
            ));
        }
        reader.skip();
        reader.skip_whitespace();
        let value_start = reader.cursor();
        let value = reader.read_string()?;
        let Some(updated) = state.try_with_text(&key, &value) else {
            reader.set_cursor(value_start);
            return Err(ParseError::with_args(
                "argument.block.property.invalid",
                [
                    ErrorArg::Text(block_name.into()),
                    ErrorArg::Text(value),
                    ErrorArg::Text(key),
                ],
            ));
        };
        *state = updated;
        properties.push((key, value));
        reader.skip_whitespace();
        if reader.can_read() {
            if reader.peek() != SYNTAX_PROPERTY_SEPARATOR {
                if reader.peek() != SYNTAX_END_PROPERTIES {
                    return Err(ParseError::new("argument.block.property.unclosed"));
                }
                break;
            }
            reader.skip();
        }
    }
    if reader.can_read() {
        reader.skip();
        Ok(())
    } else {
        Err(ParseError::new("argument.block.property.unclosed"))
    }
}

/// 官方 `readVagueProperties`。标签的属性不去方块定义里校验，留给执行时判断。
fn read_vague_properties(
    reader: &mut StringReader,
    tag_name: &str,
    properties: &mut Vec<(String, String)>,
) -> Result<(), ParseError> {
    reader.skip();
    let mut value_start = None;
    reader.skip_whitespace();
    while reader.can_read() && reader.peek() != SYNTAX_END_PROPERTIES {
        reader.skip_whitespace();
        let key_start = reader.cursor();
        let key = reader.read_string()?;
        if properties.iter().any(|(name, _)| name == &key) {
            reader.set_cursor(key_start);
            return Err(ParseError::with_args(
                "argument.block.property.duplicate",
                [ErrorArg::Text(key), ErrorArg::Text(tag_name.into())],
            ));
        }
        reader.skip_whitespace();
        if !reader.can_read() || reader.peek() != SYNTAX_EQUALS {
            reader.set_cursor(key_start);
            return Err(ParseError::with_args(
                "argument.block.property.novalue",
                [ErrorArg::Text(key), ErrorArg::Text(tag_name.into())],
            ));
        }
        reader.skip();
        reader.skip_whitespace();
        value_start = Some(reader.cursor());
        let value = reader.read_string()?;
        properties.push((key, value));
        reader.skip_whitespace();
        if reader.can_read() {
            value_start = None;
            if reader.peek() != SYNTAX_PROPERTY_SEPARATOR {
                if reader.peek() != SYNTAX_END_PROPERTIES {
                    return Err(ParseError::new("argument.block.property.unclosed"));
                }
                break;
            }
            reader.skip();
        }
    }
    if reader.can_read() {
        reader.skip();
        Ok(())
    } else {
        if let Some(start) = value_start {
            reader.set_cursor(start);
        }
        Err(ParseError::new("argument.block.property.unclosed"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(input: &str) -> BlockStateInput {
        match parse_block_state(&mut StringReader::new(input)).unwrap() {
            Value::BlockState(input) => input,
            other => panic!("不是方块状态：{other:?}"),
        }
    }

    fn predicate(input: &str) -> BlockPredicateInput {
        match parse_block_predicate(&mut StringReader::new(input)).unwrap() {
            Value::BlockPredicate(input) => input,
            other => panic!("不是方块谓词：{other:?}"),
        }
    }

    fn block_error(input: &str) -> ParseError {
        match parse_block_state(&mut StringReader::new(input)) {
            Ok(value) => panic!("不该成功：{value:?}"),
            Err(error) => error,
        }
    }

    #[test]
    fn blocks_default_to_their_first_state() {
        let stone = state("stone");
        assert_eq!(
            stone.state,
            BlockState(block_by_name("minecraft:stone").unwrap().default_state)
        );
        assert!(stone.properties.is_empty());
        assert_eq!(state("minecraft:stone").properties.len(), 0);
    }

    #[test]
    fn properties_are_applied_in_order() {
        let input = state("oak_stairs[facing=east,half=top,waterlogged=true]");
        assert_eq!(
            input.properties,
            [
                ("facing".to_owned(), "east".to_owned()),
                ("half".to_owned(), "top".to_owned()),
                ("waterlogged".to_owned(), "true".to_owned()),
            ]
        );
        assert_eq!(input.state.property_text("facing"), Some("east"));
        assert_eq!(input.state.property_text("half"), Some("top"));
        assert_eq!(input.state.property_text("waterlogged"), Some("true"));
    }

    #[test]
    fn unknown_blocks_and_properties_use_the_official_keys() {
        let error = block_error("not_a_block");
        assert_eq!(error.key, "argument.block.id.invalid");
        assert_eq!(error.args, [ErrorArg::Text("minecraft:not_a_block".into())]);

        let error = block_error("stone[wat=true]");
        assert_eq!(error.key, "argument.block.property.unknown");
        assert_eq!(
            error.args,
            [
                ErrorArg::Text("minecraft:stone".into()),
                ErrorArg::Text("wat".into())
            ]
        );

        let error = block_error("oak_stairs[facing=east,facing=west]");
        assert_eq!(error.key, "argument.block.property.duplicate");
        assert_eq!(
            error.args,
            [
                ErrorArg::Text("facing".into()),
                ErrorArg::Text("minecraft:oak_stairs".into())
            ]
        );

        let error = block_error("oak_stairs[facing]");
        assert_eq!(error.key, "argument.block.property.novalue");
        assert_eq!(
            error.args,
            [
                ErrorArg::Text("facing".into()),
                ErrorArg::Text("minecraft:oak_stairs".into())
            ]
        );

        let error = block_error("oak_stairs[facing=sideways]");
        assert_eq!(error.key, "argument.block.property.invalid");
        assert_eq!(
            error.args,
            [
                ErrorArg::Text("minecraft:oak_stairs".into()),
                ErrorArg::Text("sideways".into()),
                ErrorArg::Text("facing".into())
            ]
        );

        let error = block_error("oak_stairs[facing=east");
        assert_eq!(error.key, "argument.block.property.unclosed");
    }

    #[test]
    fn errors_rewind_to_the_argument_start() {
        let mut reader = StringReader::new("stone[wat=true]");
        let _ = parse_block_state(&mut reader).unwrap_err();
        assert_eq!(reader.cursor(), 0);
    }

    #[test]
    fn tags_are_only_allowed_in_predicates() {
        let predicate = predicate("#minecraft:logs[axis=y]");
        let BlockPredicateInput::Tag {
            tag,
            block_ids,
            properties,
        } = predicate
        else {
            panic!("不是标签谓词");
        };
        assert_eq!(tag, "minecraft:logs");
        assert!(!block_ids.is_empty());
        assert_eq!(properties, [("axis".to_owned(), "y".to_owned())]);

        assert_eq!(
            block_error("#minecraft:logs").key,
            "argument.block.id.invalid"
        );
        assert_eq!(
            parse_block_predicate(&mut StringReader::new("#minecraft:not_a_tag"))
                .unwrap_err()
                .key,
            "arguments.block.tag.unknown"
        );
    }

    #[test]
    fn predicates_only_compare_the_properties_that_were_written() {
        let predicate = predicate("oak_stairs[facing=east]");
        let east = state("oak_stairs[facing=east,half=top]").state;
        let west = state("oak_stairs[facing=west]").state;
        assert!(predicate.test(east));
        assert!(!predicate.test(west));
    }

    #[test]
    fn nbt_suffixes_fail_transparently() {
        let error = block_error("stone{foo:1}");
        assert_eq!(error.key, "command.exception");
        assert!(matches!(&error.args[0], ErrorArg::Text(text) if text.contains("SNBT")));
    }
}
