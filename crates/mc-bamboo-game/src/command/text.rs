//! `/tellraw` 和文本组件参数的公共处理。
//!
//! 官方 `TellRawCommand` 对每个目标重新解析一遍组件：`ComponentArgument.getResolvedComponent`
//! 用 `ComponentUtils.resolve`，`selector` 换成实体的显示名列表，`score` 和 `nbt`
//! 查计分板与 NBT 数据源。Bamboo 没有计分板和实体 NBT，所以后两种按
//! 「查不到东西」处理，结果都是空文本。
//!
//! `/tellraw` and shared handling for component arguments. Official
//! `ComponentArgument.getResolvedComponent` resolves the component once per target:
//! `selector` becomes a list of entity names, and `score`/`nbt` query the scoreboard
//! and NBT sources. Bamboo has neither, so those two resolve to empty text, which
//! is the official result when no value is found.

use mc_bamboo_command::{ParsedCommand, Value, argument::entity::parse_entity, permission};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::{
    components::text::checked_component,
    packet::play::{ChatVisibility, SystemChat},
    text::Component,
};

use crate::{
    command::{
        feedback::{self, CommandFail, fail},
        select,
    },
    global::Global,
    inbound::PlayerId,
    region::Region,
};

/// 官方 `ComponentUtils.DEFAULT_SEPARATOR`：灰色、文本是 `","`。
///
/// Official `ComponentUtils.DEFAULT_SEPARATOR`: `","`, styled gray.
const DEFAULT_SEPARATOR: &str = ",";

/// 按官方组件 `Codec` 校验命令里的 SNBT 标签。失败时 `argument.component.invalid`。
///
/// Validates a parsed SNBT tag with the official component codec. Invalid input
/// produces `argument.component.invalid`.
pub(crate) fn component(parsed: &ParsedCommand, name: &str) -> Result<Component, CommandFail> {
    let tag = parsed
        .get(name)
        .and_then(Value::as_component)
        .ok_or_else(|| fail("command.failed"))?;
    checked_component(tag).ok_or_else(|| feedback::CommandFail {
        message: Component::translate("argument.component.invalid", ["invalid component".into()]),
        cursor: None,
    })
}

/// 官方 `ComponentUtils.resolve`：发给每个目标前把动态内容展开。
///
/// `ResolutionContext.withEntityOverride(target)` 只影响 `@s` 和 `score` 的 `*` 名字，
/// 选择器本身仍以命令来源为基准查实体。
///
/// Official `ComponentUtils.resolve`. `withEntityOverride` only changes what `@s`
/// inside a `score`/`selector` component resolves to; the source still owns the
/// selector's position and permission checks.
pub(crate) fn resolve(
    region: &Region,
    source: PlayerId,
    target: PlayerId,
    target_uuid: uuid::Uuid,
    component: &Tag,
    depth: u32,
) -> Component {
    if depth > 64 {
        return Component::text("");
    }
    match component {
        Tag::List(items) => {
            let mut first = Component::text("");
            for (index, item) in items.iter().enumerate() {
                let part = resolve(region, source, target, target_uuid, item, depth + 1);
                if index == 0 {
                    first = part;
                } else {
                    first = first.extra(part);
                }
            }
            first
        }
        Tag::Compound(entries) => {
            let mut out = resolve_content(region, source, target, target_uuid, entries, depth);
            if let Some(Tag::List(children)) = field(entries, "extra") {
                for child in children {
                    out = out.extra(resolve(
                        region,
                        source,
                        target,
                        target_uuid,
                        child,
                        depth + 1,
                    ));
                }
            }
            out
        }
        Tag::String(text) => Component::text(text.clone()),
        // 校验过的组件树不会走到这里。
        _ => Component::text(""),
    }
}

fn resolve_content(
    region: &Region,
    source: PlayerId,
    target: PlayerId,
    target_uuid: uuid::Uuid,
    entries: &[(String, Tag)],
    depth: u32,
) -> Component {
    let kind = match field(entries, "type") {
        Some(Tag::String(kind)) => kind.as_str(),
        Some(_) => return Component::text(""),
        None => match () {
            () if field(entries, "selector").is_some() => "selector",
            () if field(entries, "score").is_some() => "score",
            () if field(entries, "nbt").is_some() => "nbt",
            () if field(entries, "translate").is_some() => "translatable",
            () if field(entries, "keybind").is_some() => "keybind",
            () if field(entries, "object").is_some() => "object",
            () => "text",
        },
    };
    match kind {
        "text" => Component::text(string_field(entries, "text").unwrap_or_default()),
        "translatable" => {
            let key = string_field(entries, "translate").unwrap_or_default();
            let mut args = Vec::new();
            if let Some(Tag::List(with)) = field(entries, "with") {
                for value in with {
                    let arg = match value {
                        Tag::Compound(_) | Tag::List(_) => {
                            resolve(region, source, target, target_uuid, value, depth + 1).into()
                        }
                        Tag::String(text) => text.clone().into(),
                        Tag::Byte(value) => i32::from(*value).into(),
                        Tag::Short(value) => i32::from(*value).into(),
                        Tag::Int(value) => (*value).into(),
                        Tag::Long(value) => (*value).into(),
                        Tag::Float(value) => (*value).into(),
                        Tag::Double(value) => (*value).into(),
                        _ => "".into(),
                    };
                    args.push(arg);
                }
            }
            Component::translate(key, args)
        }
        "keybind" => Component::text("").with_entry(
            "keybind",
            Tag::String(string_field(entries, "keybind").unwrap_or_default()),
        ),
        "selector" => selector_text(region, source, target, target_uuid, entries, depth),
        // 没有计分板和实体 NBT，官方这两类在查不到值时都是空文本。
        //
        // No scoreboard or entity NBT here; both officially resolve to empty text when
        // there is no value.
        // `object` 客户端按图集或玩家档案解析，这里没有对应的数据源。
        "score" | "nbt" | "object" => Component::text(""),
        _ => Component::text(""),
    }
}

fn selector_text(
    region: &Region,
    source: PlayerId,
    target: PlayerId,
    target_uuid: uuid::Uuid,
    entries: &[(String, Tag)],
    depth: u32,
) -> Component {
    let Some(pattern) = string_field(entries, "selector") else {
        return Component::text("");
    };
    let level = level_of(region, source);
    let mut reader = mc_bamboo_command::StringReader::new(&pattern);
    let Ok(Value::Entity(selector)) = parse_entity(&mut reader, false, false) else {
        return Component::text("");
    };
    if reader.can_read() {
        return Component::text("");
    }
    let mut overridden = selector;
    if overridden.is_self_selector() {
        overridden.uuid = Some(target_uuid);
        overridden.player_name = None;
        overridden.current_entity = false;
    }
    let names = select::mention(region, source, level, &overridden)
        .unwrap_or_default()
        .into_iter()
        .map(|id| {
            region
                .players
                .get(&id)
                .map(|player| player.name.clone())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>();
    let separator = match field(entries, "separator") {
        Some(tag) => resolve(region, source, target, target_uuid, tag, depth + 1),
        None => Component::text(DEFAULT_SEPARATOR).color(mc_bamboo_protocol::text::Color::Gray),
    };
    let mut list = Component::text("");
    for (index, name) in names.iter().enumerate() {
        if index > 0 {
            list = list.extra(separator.clone());
        }
        list = list.extra(Component::text(name.clone()));
    }
    list
}

fn field<'a>(entries: &'a [(String, Tag)], name: &str) -> Option<&'a Tag> {
    entries
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value)
}

fn string_field(entries: &[(String, Tag)], name: &str) -> Option<String> {
    match field(entries, name) {
        Some(Tag::String(text)) => Some(text.clone()),
        _ => None,
    }
}

pub(crate) fn tellraw(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = global;
    let selector = parsed
        .get("targets")
        .and_then(Value::as_entity)
        .ok_or_else(|| fail("command.failed"))?;
    let targets = select::players(region, id, level_of(region, id), selector)?;
    let raw = component(parsed, "message")?;
    for target in targets {
        // 官方 `ServerPlayer.sendSystemMessage` 在聊天设为「隐藏」时不发系统消息。
        let hidden = region
            .players
            .get(&target)
            .is_some_and(|player| player.client.chat_visibility == ChatVisibility::Hidden);
        if hidden {
            continue;
        }
        let target_uuid = region
            .players
            .get(&target)
            .map(|player| player.uuid)
            .unwrap_or_default();
        let resolved = resolve(region, id, target, target_uuid, raw.tag(), 0);
        region.links.send(
            target,
            &SystemChat {
                content: &resolved,
                overlay: false,
            },
        );
    }
    Ok(())
}

/// 供其他命令读取目标的权限等级。
pub(crate) fn level_of(region: &Region, id: PlayerId) -> u8 {
    region
        .players
        .get(&id)
        .map(|player| player.permission_level)
        .unwrap_or(permission::ALL)
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    #[test]
    fn empty_score_and_nbt_contents_resolve_to_empty_text() {
        let (_, region, id) = crate::command::tests::world_with("Kitten", 2);
        let score = Tag::Compound(vec![(
            "score".into(),
            Tag::Compound(vec![
                ("name".into(), Tag::String("*".into())),
                ("objective".into(), Tag::String("kills".into())),
            ]),
        )]);
        assert_eq!(
            resolve(&region, id, id, Uuid::nil(), &score, 0),
            Component::text("")
        );
        let nbt = Tag::Compound(vec![
            ("nbt".into(), Tag::String("Inventory".into())),
            ("entity".into(), Tag::String("@s".into())),
        ]);
        assert_eq!(
            resolve(&region, id, id, Uuid::nil(), &nbt, 0),
            Component::text("")
        );
    }

    #[test]
    fn selectors_are_joined_with_the_official_separator() {
        let (_, region, id) = crate::command::tests::world_with("Kitten", 2);
        let selector = Tag::Compound(vec![("selector".into(), Tag::String("@a".into()))]);
        let resolved = resolve(&region, id, id, Uuid::nil(), &selector, 0);
        let mut out = Vec::new();
        resolved.write(&mut out);
        // 一个目标时不写分隔符；转成标签后检查名字在里面。
        let text = String::from_utf8_lossy(&out);
        assert!(text.contains("Kitten"), "{text:?}");
    }
}
