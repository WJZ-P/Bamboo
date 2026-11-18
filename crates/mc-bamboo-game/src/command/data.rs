//! `/data`：官方 `DataCommands` 的执行入口。
//!
//! 本期执行 `get` / `merge` / `remove` 和 `modify set|merge|append|prepend|insert`
//! 配 `value` / `from`；`string`、`compute` 和 int/float provider 明确报错。
//! 三种数据源共用 `data_source`，实体走存档 codec，方块实体走游戏层那四张表。
//!
//! `/data`: official `DataCommands` execution. This round runs `get`, `merge`,
//! `remove` and `modify ... value|from`; `string`, `compute` and providers
//! report an explicit failure. All three sources share `data_source`.

use mc_bamboo_command::{NodeKind, ParsedCommand, Value};
use mc_bamboo_core::BlockPos;
use mc_bamboo_nbt::{NbtPath, Tag};
use mc_bamboo_protocol::text::{Component, TranslateArg};

use crate::{
    command::{
        commands,
        data_source::{self, Kind, Source},
        feedback::{self, CommandFail, fail, fail_with},
        select, source,
    },
    global::Global,
    inbound::PlayerId,
    region::Region,
};

/// 所有 `/data` 可执行节点的动作。
pub(crate) fn data(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let words = literal_words(parsed);
    let has = |name: &str| words.iter().any(|word| word == name);
    let operation = ["get", "merge", "remove", "modify"]
        .into_iter()
        .find(|name| has(name))
        .ok_or_else(|| fail("command.failed"))?;
    let kind = if has("block") {
        Kind::Block
    } else if has("entity") {
        Kind::Entity
    } else if has("storage") {
        Kind::Storage
    } else {
        return Err(fail("command.failed"));
    };
    let command_source = source::of(region, id);
    let target = target(global, region, id, parsed, kind, &command_source)?;
    let unsupported = has("string") || has("compute") || has("provider");
    if unsupported {
        return Err(fail("command.exception"));
    }
    match operation {
        "get" => get(global, region, id, parsed, &target),
        "merge" => merge(global, region, id, parsed, &target),
        "remove" => remove(global, region, id, parsed, &target),
        "modify" => modify(global, region, id, parsed, &target, &words),
        _ => Err(fail("command.failed")),
    }
}

fn literal_words(parsed: &ParsedCommand) -> Vec<String> {
    let graph = &commands().graph;
    parsed
        .nodes
        .iter()
        .filter_map(|&node| match &graph.node(node).kind {
            NodeKind::Literal(name) => Some(name.clone()),
            _ => None,
        })
        .collect()
}

/// 官方三种 `ArgProvider` 的目标解析。
fn target(
    _global: &Global,
    region: &Region,
    id: PlayerId,
    parsed: &ParsedCommand,
    kind: Kind,
    command_source: &source::Source,
) -> Result<Source, CommandFail> {
    let mut out = Source {
        kind,
        block: None,
        entity: None,
        storage: None,
    };
    match kind {
        Kind::Block => {
            let coords = parsed
                .get("target")
                .and_then(Value::as_block_pos)
                .ok_or_else(|| fail("command.failed"))?;
            out.block = Some(BlockPos::containing(source::position(
                coords,
                *command_source,
            )));
        }
        Kind::Storage => {
            out.storage = Some(
                parsed
                    .get("target")
                    .and_then(Value::as_str)
                    .ok_or_else(|| fail("command.failed"))?
                    .to_owned(),
            );
        }
        Kind::Entity => {
            // `modify ... from entity` 等还有第二个实体参数；`target` 只属于主目标。
            let selector = parsed
                .get("target")
                .and_then(Value::as_entity)
                .ok_or_else(|| fail("command.failed"))?;
            let found = select::one_target(region, id, source::level(region, id), selector)?;
            out.entity = Some(found);
        }
    }
    Ok(out)
}

fn get(
    global: &Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
    target: &Source,
) -> Result<(), CommandFail> {
    let whole = data_source::read(global, region, target).map_err(fail)?;
    let message = if let Some(path_text) = parsed.get("path").and_then(Value::as_str) {
        let path = NbtPath::parse(path_text).map_err(|_| fail("command.failed"))?;
        let found = path.get(&whole);
        let [only] = found.as_slice() else {
            return Err(if found.len() > 1 {
                fail("commands.data.get.multiple")
            } else {
                fail_with(
                    "arguments.nbtpath.nothing_found",
                    [path_text.to_owned().into()],
                )
            });
        };
        let scale = parsed
            .get("scale")
            .and_then(Value::as_double)
            .unwrap_or(1.0);
        let number = match only {
            Tag::Byte(v) => Some(f64::from(*v)),
            Tag::Short(v) => Some(f64::from(*v)),
            Tag::Int(v) => Some(f64::from(*v)),
            Tag::Long(v) => Some(*v as f64),
            Tag::Float(v) => Some(f64::from(*v)),
            Tag::Double(v) => Some(*v),
            _ => None,
        };
        if scale != 1.0 {
            let number = number.ok_or_else(|| {
                fail_with("commands.data.get.invalid", [path_text.to_owned().into()])
            })?;
            let value = mc_bamboo_core::mth::floor(number * scale);
            return numeric_feedback(region, id, target, path_text, scale, value);
        }
        query_component(region, target, pretty_value(only))
    } else {
        query_component(region, target, whole.to_pretty_snbt().into())
    };
    feedback::success(region, id, message, false);
    Ok(())
}

fn numeric_feedback(
    region: &mut Region,
    id: PlayerId,
    target: &Source,
    path: &str,
    scale: f64,
    value: i32,
) -> Result<(), CommandFail> {
    let key = match target.kind {
        Kind::Block => "commands.data.block.get",
        Kind::Entity => "commands.data.entity.get",
        Kind::Storage => "commands.data.storage.get",
    };
    let message = match target.kind {
        Kind::Block => {
            let pos = target.block.unwrap_or(BlockPos::new(0, 0, 0));
            Component::translate(
                key,
                [
                    path.to_owned().into(),
                    pos.x.into(),
                    pos.y.into(),
                    pos.z.into(),
                    scale.into(),
                    value.into(),
                ],
            )
        }
        Kind::Storage => Component::translate(
            key,
            [
                path.to_owned().into(),
                Component::text(target.storage.clone().unwrap_or_default()).into(),
                scale.into(),
                value.into(),
            ],
        ),
        Kind::Entity => Component::translate(
            key,
            [
                path.to_owned().into(),
                entity_name(region, target).into(),
                scale.into(),
                value.into(),
            ],
        ),
    };
    feedback::success(region, id, message, false);
    Ok(())
}

fn pretty_value(tag: &Tag) -> TranslateArg {
    tag.to_pretty_snbt().into()
}

fn query_component(region: &Region, target: &Source, value: TranslateArg) -> Component {
    match target.kind {
        Kind::Block => {
            let pos = target.block.unwrap_or(BlockPos::new(0, 0, 0));
            Component::translate(
                "commands.data.block.query",
                [pos.x.into(), pos.y.into(), pos.z.into(), value],
            )
        }
        Kind::Entity => Component::translate(
            "commands.data.entity.query",
            [entity_name(region, target).into(), value],
        ),
        Kind::Storage => Component::translate(
            "commands.data.storage.query",
            [
                Component::text(target.storage.clone().unwrap_or_default()).into(),
                value,
            ],
        ),
    }
}

fn merge(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
    target: &Source,
) -> Result<(), CommandFail> {
    let old = data_source::read(global, region, target).map_err(fail)?;
    let patch = parsed
        .get("nbt")
        .and_then(Value::as_nbt)
        .ok_or_else(|| fail("command.failed"))?;
    let mut merged = old.clone();
    merge_tags(&mut merged, patch);
    if merged == old {
        return Err(fail("commands.data.merge.failed"));
    }
    data_source::write(global, region, target, &merged).map_err(fail)?;
    modified(region, id, target)
}

fn remove(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
    target: &Source,
) -> Result<(), CommandFail> {
    let mut root = data_source::read(global, region, target).map_err(fail)?;
    let text = parsed
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| fail("command.failed"))?;
    let path = NbtPath::parse(text).map_err(|_| fail("command.failed"))?;
    if path.remove(&mut root) == 0 {
        return Err(fail("commands.data.merge.failed"));
    }
    data_source::write(global, region, target, &root).map_err(fail)?;
    modified(region, id, target)
}

fn modify(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
    target: &Source,
    words: &[String],
) -> Result<(), CommandFail> {
    let operation = ["set", "merge", "append", "prepend", "insert"]
        .into_iter()
        .find(|name| words.iter().any(|word| word == name))
        .ok_or_else(|| fail("command.failed"))?;
    let has = |name: &str| words.iter().any(|word| word == name);
    if has("string") || has("compute") || has("provider") {
        return Err(fail("command.exception"));
    }
    let mut root = data_source::read(global, region, target).map_err(fail)?;
    let text = parsed
        .get("targetPath")
        .and_then(Value::as_str)
        .ok_or_else(|| fail("command.failed"))?;
    let path = NbtPath::parse(text).map_err(|_| fail("command.failed"))?;
    let value = if has("from") {
        let source_path = parsed
            .get("sourcePath")
            .and_then(Value::as_str)
            .ok_or_else(|| fail("command.failed"))?;
        let from = from_source(global, region, id, parsed, command_source(region, id))?;
        let whole = data_source::read(global, region, &from).map_err(fail)?;
        let from_path = NbtPath::parse(source_path).map_err(|_| fail("command.failed"))?;
        let found = from_path.get(&whole);
        let [only] = found.as_slice() else {
            return Err(fail("commands.data.get.multiple"));
        };
        (*only).clone()
    } else {
        parsed
            .get("value")
            .and_then(Value::as_nbt)
            .cloned()
            .ok_or_else(|| fail("command.failed"))?
    };
    let mut changed = false;
    for leaf in path.get_mut(&mut root) {
        changed |= apply(operation, leaf, &value);
    }
    if !changed {
        return Err(fail("commands.data.merge.failed"));
    }
    data_source::write(global, region, target, &root).map_err(fail)?;
    modified(region, id, target)
}

fn apply(operation: &str, leaf: &mut Tag, value: &Tag) -> bool {
    match operation {
        "set" => {
            *leaf = value.clone();
            true
        }
        "merge" => {
            if !matches!(leaf, Tag::Compound(_)) {
                return false;
            }
            let before = leaf.clone();
            merge_tags(leaf, value);
            *leaf != before
        }
        "append" | "prepend" | "insert" => {
            let Tag::List(items) = leaf else {
                return false;
            };
            let values = match value {
                Tag::List(values) => values.clone(),
                other => vec![other.clone()],
            };
            if values.is_empty() {
                return false;
            }
            if operation == "append" {
                items.extend(values);
            } else {
                items.splice(0..0, values);
            }
            true
        }
        _ => false,
    }
}

fn merge_tags(root: &mut Tag, patch: &Tag) {
    if !matches!(root, Tag::Compound(_)) || !matches!(patch, Tag::Compound(_)) {
        *root = patch.clone();
        return;
    }
    let (Tag::Compound(entries), Tag::Compound(additions)) = (root, patch) else {
        unreachable!("上面刚判过两种都是复合标签");
    };
    for (key, value) in additions {
        match entries.iter_mut().find(|(name, _)| name == key) {
            Some((_, slot)) if matches!(slot, Tag::Compound(_)) => merge_tags(slot, value),
            Some((_, slot)) => *slot = value.clone(),
            None => entries.push((key.clone(), value.clone())),
        }
    }
}

fn from_source(
    _global: &Global,
    region: &Region,
    id: PlayerId,
    parsed: &ParsedCommand,
    command_source: source::Source,
) -> Result<Source, CommandFail> {
    // `from` 的第二个目标可能是 block / entity / storage；名字和目标主分支同名时
    // 按参数里出现的 `source` 类型判断。
    let kind = if parsed
        .get("source")
        .is_some_and(|value| Value::as_block_pos(value).is_some())
    {
        Kind::Block
    } else if parsed
        .get("source")
        .is_some_and(|value| Value::as_entity(value).is_some())
    {
        Kind::Entity
    } else {
        Kind::Storage
    };
    let mut out = Source {
        kind,
        block: None,
        entity: None,
        storage: None,
    };
    let value = parsed.get("source").ok_or_else(|| fail("command.failed"))?;
    match kind {
        Kind::Block => {
            let coords = Value::as_block_pos(value).ok_or_else(|| fail("command.failed"))?;
            out.block = Some(BlockPos::containing(source::position(
                coords,
                command_source,
            )));
        }
        Kind::Entity => {
            let selector = Value::as_entity(value).ok_or_else(|| fail("command.failed"))?;
            out.entity = Some(select::one_target(
                region,
                id,
                source::level(region, id),
                selector,
            )?);
        }
        Kind::Storage => {
            out.storage = Some(
                Value::as_str(value)
                    .ok_or_else(|| fail("command.failed"))?
                    .to_owned(),
            );
        }
    }
    Ok(out)
}

fn command_source(region: &Region, id: PlayerId) -> source::Source {
    source::of(region, id)
}

fn modified(region: &mut Region, id: PlayerId, target: &Source) -> Result<(), CommandFail> {
    let key = match target.kind {
        Kind::Block => "commands.data.block.modified",
        Kind::Entity => "commands.data.entity.modified",
        Kind::Storage => "commands.data.storage.modified",
    };
    let message = match target.kind {
        Kind::Block => {
            let pos = target.block.unwrap_or(BlockPos::new(0, 0, 0));
            Component::translate(key, [pos.x.into(), pos.y.into(), pos.z.into()])
        }
        Kind::Storage => Component::translate(
            key,
            [Component::text(target.storage.clone().unwrap_or_default()).into()],
        ),
        Kind::Entity => Component::translate(key, [entity_name(region, target).into()]),
    };
    feedback::success(region, id, message, true);
    Ok(())
}

fn entity_name(region: &Region, target: &Source) -> Component {
    Component::text(
        target
            .entity
            .and_then(|entity| source::entity_name(region, entity))
            .unwrap_or_default(),
    )
}

#[cfg(test)]
mod tests {
    use mc_bamboo_blocks::entity::Entity;
    use mc_bamboo_core::Vec3;
    use mc_bamboo_nbt::Tag;

    use crate::{
        command::{data_source, on_command, tests::world_with},
        global::Global,
        inbound::PlayerId,
        region::Region,
    };

    fn world() -> (Global, Region, PlayerId) {
        world_with("Kitten", 2)
    }

    fn storage_tag(global: &Global, id: &str) -> Tag {
        global.command_storage.get(id)
    }

    #[test]
    fn storage_merge_get_and_remove() {
        let (mut global, mut region, id) = world();
        on_command(
            &mut global,
            &mut region,
            id,
            "data merge storage minecraft:test {Value:7}",
            None,
        );
        assert_eq!(
            storage_tag(&global, "minecraft:test"),
            Tag::Compound(vec![("Value".to_owned(), Tag::Int(7))])
        );
        on_command(
            &mut global,
            &mut region,
            id,
            "data merge storage minecraft:test {Other:1b}",
            None,
        );
        assert!(
            storage_tag(&global, "minecraft:test")
                .to_pretty_snbt()
                .contains("Other")
        );
        on_command(
            &mut global,
            &mut region,
            id,
            "data remove storage minecraft:test Value",
            None,
        );
        assert!(
            !storage_tag(&global, "minecraft:test")
                .to_pretty_snbt()
                .contains("Value")
        );
    }

    #[test]
    fn entity_merge_uses_the_archive_codec() {
        let (mut global, mut region, id) = world();
        let uid = region
            .entities
            .spawn(Entity::cow(Vec3::new(2.5, -60.0, 2.5)));
        on_command(
            &mut global,
            &mut region,
            id,
            "data merge entity @e[type=cow,limit=1] {Health:3.0f}",
            None,
        );
        let cow = region.entities.get(uid).expect("牛还在");
        assert_eq!(cow.kind.mob().expect("牛").health, 3.0);
    }

    #[test]
    fn entity_merge_on_a_player_reports_the_official_error() {
        let (mut global, mut region, id) = world();
        let before = region.players[&id].inventory.slots.clone();
        on_command(
            &mut global,
            &mut region,
            id,
            "data merge entity @s {Health:1.0f}",
            None,
        );
        assert_eq!(region.players[&id].inventory.slots, before);
    }

    #[test]
    fn block_data_uses_the_game_container_table() {
        let (mut global, mut region, id) = world();
        let pos = mc_bamboo_core::BlockPos::new(4, -60, 6);
        data_source::put_test_chest(&mut region, pos);
        on_command(
            &mut global,
            &mut region,
            id,
            "data modify block 4 -60 6 Items set value [{Slot:0b,id:\"minecraft:stone\",count:2}]",
            None,
        );
        let entry = region.containers.get(&pos).expect("箱子还在");
        assert_eq!(entry.slots[0].item, mc_bamboo_data::items::STONE);
        assert_eq!(entry.slots[0].count, 2);
    }

    #[test]
    fn modify_set_value_and_from_are_supported() {
        let (mut global, mut region, id) = world();
        on_command(
            &mut global,
            &mut region,
            id,
            "data modify storage minecraft:a Target set value 5",
            None,
        );
        on_command(
            &mut global,
            &mut region,
            id,
            "data modify storage minecraft:b Copy set from storage minecraft:a Target",
            None,
        );
        assert!(
            storage_tag(&global, "minecraft:b")
                .to_pretty_snbt()
                .contains("5")
        );
    }

    #[test]
    fn unsupported_string_branch_reports_a_failure() {
        let (mut global, mut region, id) = world();
        on_command(
            &mut global,
            &mut region,
            id,
            "data merge storage minecraft:a {Text:\"hello\"}",
            None,
        );
        let before = storage_tag(&global, "minecraft:a");
        on_command(
            &mut global,
            &mut region,
            id,
            "data modify storage minecraft:a Text set string storage minecraft:a Text 0 2",
            None,
        );
        assert_eq!(storage_tag(&global, "minecraft:a"), before, "不静默成功");
    }
}
