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
use mc_bamboo_protocol::{components::text::checked_component, text::Component};

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

/// 官方 `ComponentArgument.getResolvedComponent(context, name)`：按命令来源解析一次。
///
/// `ResolutionContext.create(source)` 没有 entity override，所以 `score` 里的 `*`
/// 指命令来源自己；`/tellraw` 和 `/title` 才用带接收者覆盖的 [`resolve`]。
///
/// Official `ComponentArgument.getResolvedComponent(context, name)`: resolve once
/// against the command source. `ResolutionContext.create(source)` has no entity
/// override, so `*` in a score component means the source itself.
pub(crate) fn resolved_component(
    global: &Global,
    region: &Region,
    source: PlayerId,
    parsed: &ParsedCommand,
    name: &str,
) -> Result<Component, CommandFail> {
    let tag = parsed
        .get(name)
        .and_then(Value::as_component)
        .ok_or_else(|| fail("command.failed"))?;
    let uuid = region
        .players
        .get(&source)
        .map(|player| player.uuid)
        .unwrap_or_default();
    Ok(resolve(global, region, source, source, uuid, tag, 0))
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
    global: &Global,
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
                let part = resolve(global, region, source, target, target_uuid, item, depth + 1);
                if index == 0 {
                    first = part;
                } else {
                    first = first.extra(part);
                }
            }
            first
        }
        Tag::Compound(entries) => {
            let mut out =
                resolve_content(global, region, source, target, target_uuid, entries, depth);
            if let Some(Tag::List(children)) = field(entries, "extra") {
                for child in children {
                    out = out.extra(resolve(
                        global,
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
    global: &Global,
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
            () if field(entries, "sprite").is_some() || field(entries, "player").is_some() => {
                "object"
            }
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
                        Tag::Compound(_) | Tag::List(_) => resolve(
                            global,
                            region,
                            source,
                            target,
                            target_uuid,
                            value,
                            depth + 1,
                        )
                        .into(),
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
        "selector" => selector_text(global, region, source, target, target_uuid, entries, depth),
        "score" => score_text(global, region, source, target, target_uuid, entries),
        "object" => object_text(global, region, source, target, target_uuid, entries, depth),
        // 没有实体和方块实体的 NBT 数据源：官方 `NbtContents.resolve` 在取不到数据时
        // 把流 reduce 成空，结果就是空文本。
        "nbt" => Component::text(""),
        _ => Component::text(""),
    }
}

/// 官方 `ScoreContents.resolve`：`*` 换成接收者，选择器要求正好一个实体，
/// 然后按 `ScoreContents.getScore` 到计分板取分数并套用数字格式。
fn score_text(
    global: &Global,
    region: &Region,
    source: PlayerId,
    target: PlayerId,
    target_uuid: uuid::Uuid,
    entries: &[(String, Tag)],
) -> Component {
    let Some(Tag::Compound(score)) = field(entries, "score") else {
        return Component::text("");
    };
    let objective = string_field(score, "objective").unwrap_or_default();
    let name = string_field(score, "name").unwrap_or_default();
    let holder = if name == "*" {
        scoreboard_name(region, target)
    } else {
        // `ScoreContents.INNER_CODEC` 把 name 编成 `Either<选择器, 字符串>`：
        // 所有非空名字都先当选择器编译（玩家名也是合法选择器），只有 `*` 走上面的分支。
        let mut reader = mc_bamboo_command::StringReader::new(&name);
        let Ok(Value::Entity(selector)) = parse_entity(&mut reader, false, false) else {
            return Component::text("");
        };
        if reader.can_read() {
            return Component::text("");
        }
        let found = select::mention(region, source, level_of(region, source), &selector)
            .unwrap_or_default();
        let [only] = found.as_slice() else {
            // 官方 `findTargetName`：零个结果退回原文，多个结果报错（这里按空文本）。
            return if found.is_empty() {
                global
                    .scoreboard()
                    .formatted_score(&name, &objective)
                    .unwrap_or_else(|| Component::text(""))
            } else {
                Component::text("")
            };
        };
        scoreboard_name(region, *only)
    };
    let _ = target_uuid;
    global
        .scoreboard()
        .formatted_score(&holder, &objective)
        .unwrap_or_else(|| Component::text(""))
}

fn scoreboard_name(region: &Region, id: PlayerId) -> String {
    region
        .players
        .get(&id)
        .map(|player| player.name.clone())
        .unwrap_or_default()
}

/// 官方 `ObjectContents.resolve`：默认的对象校验器恒为真，所以图集精灵和玩家头像
/// 都原样保留；`fallback` 先按 `ComponentUtils.resolve` 展开，再一起写回去。
fn object_text(
    global: &Global,
    region: &Region,
    source: PlayerId,
    target: PlayerId,
    target_uuid: uuid::Uuid,
    entries: &[(String, Tag)],
    depth: u32,
) -> Component {
    let mut out = Component::text("");
    for (key, value) in entries {
        if matches!(
            key.as_str(),
            "object" | "sprite" | "atlas" | "player" | "hat"
        ) {
            out = out.with_entry(key, value.clone());
        }
    }
    if let Some(fallback) = field(entries, "fallback") {
        let resolved = resolve(
            global,
            region,
            source,
            target,
            target_uuid,
            fallback,
            depth + 1,
        );
        out = out.with_entry("fallback", resolved.tag().clone());
    }
    out
}

fn selector_text(
    global: &Global,
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
        Some(tag) => resolve(global, region, source, target, target_uuid, tag, depth + 1),
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
    let selector = parsed
        .get("targets")
        .and_then(Value::as_entity)
        .ok_or_else(|| fail("command.failed"))?;
    let targets = select::players(region, id, level_of(region, id), selector)?;
    let raw = component(parsed, "message")?;
    for target in targets {
        let target_uuid = region
            .players
            .get(&target)
            .map(|player| player.uuid)
            .unwrap_or_default();
        let resolved = resolve(global, region, id, target, target_uuid, raw.tag(), 0);
        // 官方 `TellRawCommand` 对每个目标调 `player.sendSystemMessage`，隐藏聊天的不发。
        //
        // Official `TellRawCommand` calls `player.sendSystemMessage` for each target;
        // hidden chat is skipped.
        crate::chat::send_system_message(region, target, &resolved, false);
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
    fn score_components_read_the_scoreboard_and_nbt_stays_empty() {
        let (mut global, mut region, id) = crate::command::tests::world_with("Kitten", 2);
        global.scoreboard_mut().add_objective(
            "kills",
            "dummy",
            false,
            false,
            Component::text("kills"),
        );
        global.scoreboard_mut().set_score(
            &crate::scoreboard::Holder {
                name: "Kitten".into(),
                display: None,
            },
            "kills",
            7,
            &mut region,
        );
        let score = Tag::Compound(vec![(
            "score".into(),
            Tag::Compound(vec![
                ("name".into(), Tag::String("*".into())),
                ("objective".into(), Tag::String("kills".into())),
            ]),
        )]);
        assert_eq!(
            resolve(&global, &region, id, id, Uuid::nil(), &score, 0),
            Component::text("7")
        );
        let nbt = Tag::Compound(vec![
            ("nbt".into(), Tag::String("Inventory".into())),
            ("entity".into(), Tag::String("@s".into())),
        ]);
        assert_eq!(
            resolve(&global, &region, id, id, Uuid::nil(), &nbt, 0),
            Component::text("")
        );
    }

    #[test]
    fn selectors_are_joined_with_the_official_separator() {
        let (global, region, id) = crate::command::tests::world_with("Kitten", 2);
        let selector = Tag::Compound(vec![("selector".into(), Tag::String("@a".into()))]);
        let resolved = resolve(&global, &region, id, id, Uuid::nil(), &selector, 0);
        let mut out = Vec::new();
        resolved.write(&mut out);
        // 一个目标时不写分隔符；转成标签后检查名字在里面。
        let text = String::from_utf8_lossy(&out);
        assert!(text.contains("Kitten"), "{text:?}");
    }

    #[test]
    fn object_components_keep_their_sprite_and_resolve_their_fallback() {
        let (global, region, id) = crate::command::tests::world_with("Kitten", 2);
        let component = Tag::Compound(vec![
            ("sprite".into(), Tag::String("minecraft:block/stone".into())),
            ("fallback".into(), Tag::String("stone".into())),
        ]);
        let resolved = resolve(&global, &region, id, id, Uuid::nil(), &component, 0);
        let Tag::Compound(entries) = resolved.tag() else {
            panic!("object 应该还是复合标签");
        };
        assert!(
            entries.iter().any(|(key, value)| key == "sprite"
                && *value == Tag::String("minecraft:block/stone".into()))
        );
        assert!(entries.iter().any(|(key, _)| key == "fallback"));
    }
}
