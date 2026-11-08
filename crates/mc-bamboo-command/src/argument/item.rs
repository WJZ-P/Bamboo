//! 物品和物品谓词参数。Bamboo 没有数据组件，`[...]` 后缀会明确报不支持。

use mc_bamboo_data::{item_by_name, tags, tags::TAG_GROUPS};

use crate::{
    ParseError, Value,
    identifier::Identifier,
    reader::StringReader,
    value::{ItemInput, ItemPredicateInput},
};

const SYNTAX_START_COMPONENTS: char = '[';
const SYNTAX_START_TAG: char = '#';

/// 官方 `ItemArgument.item()`。
pub fn parse_item(reader: &mut StringReader) -> Result<Value, ParseError> {
    let start = reader.cursor();
    match parse_item_inner(reader) {
        Ok(item) => Ok(Value::Item(item)),
        Err(error) => {
            // 官方 `ItemParser.parse` 出错时把光标退回参数开头。
            reader.set_cursor(start);
            Err(error)
        }
    }
}

fn parse_item_inner(reader: &mut StringReader) -> Result<ItemInput, ParseError> {
    let start = reader.cursor();
    let id = Identifier::read(reader)?;
    let name = id.to_registry_name();
    let Some(item) = item_by_name(&name) else {
        reader.set_cursor(start);
        return Err(ParseError::text("argument.item.id.invalid", name));
    };
    if reader.can_read() && reader.peek() == SYNTAX_START_COMPONENTS {
        reader.skip();
        return Err(ParseError::text(
            "command.exception",
            "item data components are not supported by Bamboo",
        ));
    }
    Ok(ItemInput { item })
}

/// 官方 `ItemPredicateArgument.itemPredicate()`，Bamboo 只支持物品和物品标签。
pub fn parse_item_predicate(reader: &mut StringReader) -> Result<Value, ParseError> {
    let start = reader.cursor();
    match parse_item_predicate_inner(reader) {
        Ok(predicate) => Ok(Value::ItemPredicate(predicate)),
        Err(error) => {
            reader.set_cursor(start);
            Err(error)
        }
    }
}

fn parse_item_predicate_inner(reader: &mut StringReader) -> Result<ItemPredicateInput, ParseError> {
    if reader.can_read() && reader.peek() == SYNTAX_START_TAG {
        reader.skip();
        let id = Identifier::read(reader)?;
        let name = id.to_registry_name();
        let Some(ids) = item_tag(&name) else {
            return Err(ParseError::text("arguments.item.tag.unknown", name));
        };
        reject_components(reader)?;
        return Ok(ItemPredicateInput::Tag {
            tag: name,
            item_ids: ids.to_vec(),
        });
    }
    let start = reader.cursor();
    let id = Identifier::read(reader)?;
    let name = id.to_registry_name();
    let Some(item) = item_by_name(&name) else {
        reader.set_cursor(start);
        return Err(ParseError::text("argument.item.id.invalid", name));
    };
    reject_components(reader)?;
    Ok(ItemPredicateInput::Item(item))
}

fn reject_components(reader: &mut StringReader) -> Result<(), ParseError> {
    if reader.can_read() && reader.peek() == SYNTAX_START_COMPONENTS {
        reader.skip();
        return Err(ParseError::text(
            "command.exception",
            "item data components are not supported by Bamboo",
        ));
    }
    Ok(())
}

/// 官方物品标签的条目 ID。`name` 要带命名空间。
fn item_tag(name: &str) -> Option<&'static [i32]> {
    let group = TAG_GROUPS.get(tags::item_tags::GROUP)?;
    let tag = group.tags.iter().find(|tag| tag.name == name)?;
    Some(tag.ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ErrorArg;

    fn item(input: &str) -> ItemInput {
        match parse_item(&mut StringReader::new(input)).unwrap() {
            Value::Item(item) => item,
            other => panic!("不是物品：{other:?}"),
        }
    }

    fn predicate(input: &str) -> ItemPredicateInput {
        match parse_item_predicate(&mut StringReader::new(input)).unwrap() {
            Value::ItemPredicate(predicate) => predicate,
            other => panic!("不是物品谓词：{other:?}"),
        }
    }

    #[test]
    fn items_use_the_default_namespace() {
        assert_eq!(item("diamond").item.name, "minecraft:diamond");
        assert_eq!(item("minecraft:diamond").item.name, "minecraft:diamond");
    }

    #[test]
    fn unknown_items_use_the_official_key() {
        let mut reader = StringReader::new("not_an_item");
        let error = parse_item(&mut reader).unwrap_err();
        assert_eq!(error.key, "argument.item.id.invalid");
        assert_eq!(error.args, [ErrorArg::Text("minecraft:not_an_item".into())]);
        assert_eq!(reader.cursor(), 0);
    }

    #[test]
    fn predicates_accept_items_and_tags() {
        assert_eq!(
            predicate("stick"),
            ItemPredicateInput::Item(item_by_name("minecraft:stick").unwrap())
        );
        let ItemPredicateInput::Tag { tag, item_ids } = predicate("#minecraft:axes") else {
            panic!("不是标签谓词");
        };
        assert_eq!(tag, "minecraft:axes");
        assert!(!item_ids.is_empty());
        assert_eq!(
            parse_item_predicate(&mut StringReader::new("#minecraft:not_a_tag"))
                .unwrap_err()
                .key,
            "arguments.item.tag.unknown"
        );
    }

    #[test]
    fn components_are_rejected_transparently() {
        let error = parse_item(&mut StringReader::new("stick[damage=1]")).unwrap_err();
        assert_eq!(error.key, "command.exception");
        assert!(matches!(&error.args[0], ErrorArg::Text(text) if text.contains("components")));
    }
}
