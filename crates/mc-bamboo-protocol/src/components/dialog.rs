//! 官方 `Dialog.CODEC`：注册表里的三个对话框，或者一份内联定义。
//!
//! 内联定义按 `DialogTypes` 分派。暂停游戏时 `after_action` 必须会解除暂停。
//!
//! `Dialog.CODEC`: one of the three synced dialogs, or an inline definition.
//! Inline dialogs dispatch through `DialogTypes`. A dialog that pauses the game
//! needs an `after_action` that unpauses it.

use mc_bamboo_data::registries::SYNCED_REGISTRIES;
use mc_bamboo_nbt::Tag;

use super::{
    archive::read::{self, identifier},
    java_hash::iteration_order,
    text_wire,
};
use crate::{codec::DecodeError, components::cursor::parse_identifier};

const TYPES: [&str; 5] = [
    "minecraft:notice",
    "minecraft:server_links",
    "minecraft:dialog_list",
    "minecraft:multi_action",
    "minecraft:confirmation",
];

/// 把对话框解出来再按官方 `Codec` 写回去。
///
/// Decode a dialog and write what the official codec would write.
pub(crate) fn rewrite(tag: &Tag, depth: u32) -> Result<Tag, DecodeError> {
    match tag {
        Tag::String(name) => {
            let name = parse_identifier(name).map_err(|_| bad("dialog name is invalid"))?;
            if dialog_names().contains(&name.as_str()) {
                Ok(Tag::String(name))
            } else {
                Err(bad("unknown dialog"))
            }
        }
        Tag::Compound(entries) => inline(entries, depth),
        _ => Err(bad("dialog must be a name or a definition")),
    }
}

fn inline(entries: &[(String, Tag)], depth: u32) -> Result<Tag, DecodeError> {
    let kind = match field(entries, "type") {
        Some(Tag::String(kind)) => {
            parse_identifier(kind).map_err(|_| bad("dialog type is invalid"))?
        }
        _ => return Err(bad("dialog needs a type")),
    };
    if !TYPES.contains(&kind.as_str()) {
        return Err(bad("unknown dialog type"));
    }
    let mut fields = vec![("type".to_owned(), Tag::String(kind.clone()))];
    fields.extend(common(entries, depth)?);
    match kind.as_str() {
        "minecraft:notice" => {
            if let Some(action) = field(entries, "action") {
                fields.push(("action".to_owned(), button(action, depth)?));
            }
        }
        "minecraft:server_links" => fields.extend(button_list(entries, depth, false)?),
        "minecraft:dialog_list" => {
            let dialogs =
                field(entries, "dialogs").ok_or_else(|| bad("dialog list needs dialogs"))?;
            fields.push(("dialogs".to_owned(), holder_set(dialogs, depth)?));
            fields.extend(button_list(entries, depth, true)?);
        }
        "minecraft:multi_action" => {
            let actions = field(entries, "actions").ok_or_else(|| bad("dialog needs actions"))?;
            let values = read::list(actions).map_err(|_| bad("actions must be a list"))?;
            if values.is_empty() {
                return Err(bad("actions must not be empty"));
            }
            let buttons = values
                .iter()
                .map(|value| button(value, depth))
                .collect::<Result<Vec<_>, _>>()?;
            fields.push(("actions".to_owned(), Tag::List(buttons)));
            fields.extend(optional_button(entries, "exit_action", depth)?);
            fields.extend(positive(entries, "columns", 2)?);
        }
        "minecraft:confirmation" => {
            let yes = field(entries, "yes").ok_or_else(|| bad("confirmation needs yes"))?;
            let no = field(entries, "no").ok_or_else(|| bad("confirmation needs no"))?;
            fields.push(("yes".to_owned(), button(yes, depth)?));
            fields.push(("no".to_owned(), button(no, depth)?));
        }
        _ => return Err(bad("unknown dialog type")),
    }
    Ok(finish(fields))
}

fn common(entries: &[(String, Tag)], depth: u32) -> Result<Vec<(String, Tag)>, DecodeError> {
    let title = field(entries, "title").ok_or_else(|| bad("dialog needs a title"))?;
    let mut fields = vec![("title".to_owned(), text_wire::canonical(title, depth + 1)?)];
    if let Some(external) = field(entries, "external_title") {
        fields.push((
            "external_title".to_owned(),
            text_wire::canonical(external, depth + 1)?,
        ));
    }
    let escape = optional_bool(entries, "can_close_with_escape", true)?;
    let pause = optional_bool(entries, "pause", true)?;
    let after = match field(entries, "after_action") {
        Some(tag) => action_name(tag)?,
        None => "close",
    };
    if pause && after == "none" {
        return Err(bad("pausing dialog must unpause"));
    }
    if !escape {
        fields.push(("can_close_with_escape".to_owned(), Tag::Byte(0)));
    }
    if !pause {
        fields.push(("pause".to_owned(), Tag::Byte(0)));
    }
    if after != "close" {
        fields.push(("after_action".to_owned(), Tag::String(after.to_owned())));
    }
    if let Some(body) = field(entries, "body") {
        let encoded = body_list(body, depth)?;
        if !matches!(encoded, Tag::List(ref items) if items.is_empty()) {
            fields.push(("body".to_owned(), encoded));
        }
    }
    if let Some(inputs) = field(entries, "inputs") {
        let values = read::list(inputs).map_err(|_| bad("inputs must be a list"))?;
        if !values.is_empty() {
            let encoded = values
                .iter()
                .map(|value| input(value, depth))
                .collect::<Result<Vec<_>, _>>()?;
            fields.push(("inputs".to_owned(), Tag::List(encoded)));
        }
    }
    Ok(fields)
}

fn button_list(
    entries: &[(String, Tag)],
    depth: u32,
    width: bool,
) -> Result<Vec<(String, Tag)>, DecodeError> {
    let mut fields = optional_button(entries, "exit_action", depth)?;
    fields.extend(positive(entries, "columns", 2)?);
    if width {
        fields.extend(ranged(entries, "button_width", 150, 1, 1024)?);
    }
    Ok(fields)
}

fn optional_button(
    entries: &[(String, Tag)],
    name: &str,
    depth: u32,
) -> Result<Vec<(String, Tag)>, DecodeError> {
    match field(entries, name) {
        Some(tag) => Ok(vec![(name.to_owned(), button(tag, depth)?)]),
        None => Ok(Vec::new()),
    }
}

fn button(tag: &Tag, depth: u32) -> Result<Tag, DecodeError> {
    let entries = read::compound(tag).map_err(|_| bad("button must be a compound"))?;
    let label = field(entries, "label").ok_or_else(|| bad("button needs a label"))?;
    let mut fields = vec![("label".to_owned(), text_wire::canonical(label, depth + 1)?)];
    if let Some(tooltip) = field(entries, "tooltip") {
        fields.push((
            "tooltip".to_owned(),
            text_wire::canonical(tooltip, depth + 1)?,
        ));
    }
    fields.extend(ranged(entries, "width", 150, 1, 1024)?);
    if let Some(action) = field(entries, "action") {
        fields.push(("action".to_owned(), dialog_action(action, depth)?));
    }
    Ok(finish(fields))
}

fn dialog_action(tag: &Tag, depth: u32) -> Result<Tag, DecodeError> {
    let entries = read::compound(tag).map_err(|_| bad("dialog action must be a compound"))?;
    let kind = match field(entries, "type") {
        Some(Tag::String(kind)) => {
            parse_identifier(kind).map_err(|_| bad("dialog action type is invalid"))?
        }
        _ => return Err(bad("dialog action needs a type")),
    };
    let mut fields = vec![("type".to_owned(), Tag::String(kind.clone()))];
    match kind.as_str() {
        "minecraft:dynamic/run_command" => {
            let template = string_field(entries, "template")?;
            if !macro_template(&template) {
                return Err(bad("command template has no variable"));
            }
            fields.push(("template".to_owned(), Tag::String(template)));
        }
        "minecraft:dynamic/custom" => {
            fields.push((
                "id".to_owned(),
                Tag::String(identifier_field(entries, "id")?),
            ));
            if let Some(additions) = field(entries, "additions") {
                read::compound(additions).map_err(|_| bad("additions must be a compound"))?;
                fields.push(("additions".to_owned(), additions.clone()));
            }
        }
        "minecraft:open_url" => fields.push((
            "url".to_owned(),
            Tag::String(http_url(string_field(entries, "url")?)?),
        )),
        "minecraft:run_command" | "minecraft:suggest_command" => {
            let command = string_field(entries, "command")?;
            chat(&command)?;
            fields.push(("command".to_owned(), Tag::String(command)));
        }
        "minecraft:show_dialog" => {
            let dialog =
                field(entries, "dialog").ok_or_else(|| bad("show_dialog needs a dialog"))?;
            fields.push(("dialog".to_owned(), rewrite(dialog, depth + 1)?));
        }
        "minecraft:change_page" => {
            let page = positive_number(
                field(entries, "page").ok_or_else(|| bad("change_page needs a page"))?,
            )?;
            fields.push(("page".to_owned(), Tag::Int(page)));
        }
        "minecraft:copy_to_clipboard" => {
            fields.push((
                "value".to_owned(),
                Tag::String(string_field(entries, "value")?),
            ));
        }
        "minecraft:custom" => {
            fields.push((
                "id".to_owned(),
                Tag::String(identifier_field(entries, "id")?),
            ));
            if let Some(payload) = field(entries, "payload") {
                fields.push(("payload".to_owned(), payload.clone()));
            }
        }
        _ => return Err(bad("unknown dialog action")),
    }
    Ok(finish(fields))
}

fn body_list(tag: &Tag, depth: u32) -> Result<Tag, DecodeError> {
    let values = if read::compound(tag).is_ok() {
        vec![tag.clone()]
    } else {
        read::list(tag).map_err(|_| bad("dialog body must be a list"))?
    };
    let bodies = values
        .iter()
        .map(|value| body(value, depth))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(match bodies.len() {
        1 => bodies.into_iter().next().unwrap(),
        _ => Tag::List(bodies),
    })
}

fn body(tag: &Tag, depth: u32) -> Result<Tag, DecodeError> {
    let entries = read::compound(tag).map_err(|_| bad("dialog body must be a compound"))?;
    let kind = match field(entries, "type") {
        Some(Tag::String(kind)) => {
            parse_identifier(kind).map_err(|_| bad("dialog body type is invalid"))?
        }
        _ => return Err(bad("dialog body needs a type")),
    };
    let mut fields = vec![("type".to_owned(), Tag::String(kind.clone()))];
    match kind.as_str() {
        "minecraft:plain_message" => {
            let contents =
                field(entries, "contents").ok_or_else(|| bad("plain message needs contents"))?;
            fields.push((
                "contents".to_owned(),
                text_wire::canonical(contents, depth + 1)?,
            ));
            fields.extend(ranged(entries, "width", 200, 1, 1024)?);
        }
        "minecraft:item" => {
            let item = field(entries, "item").ok_or_else(|| bad("item body needs an item"))?;
            fields.push(("item".to_owned(), text_wire::item_template(item)?));
            if let Some(description) = field(entries, "description") {
                fields.push(("description".to_owned(), plain_or_text(description, depth)?));
            }
            fields.extend(optional_flag(entries, "show_decorations", true)?);
            fields.extend(optional_flag(entries, "show_tooltip", true)?);
            fields.extend(ranged(entries, "width", 16, 1, 256)?);
            fields.extend(ranged(entries, "height", 16, 1, 256)?);
        }
        _ => return Err(bad("unknown dialog body")),
    }
    Ok(finish(fields))
}

fn plain_or_text(tag: &Tag, depth: u32) -> Result<Tag, DecodeError> {
    if field(compound_or_empty(tag), "type").is_some()
        || field(compound_or_empty(tag), "contents").is_some()
    {
        let entries = read::compound(tag).map_err(|_| bad("description must be text"))?;
        let contents =
            field(entries, "contents").ok_or_else(|| bad("description needs contents"))?;
        let mut fields = vec![(
            "contents".to_owned(),
            text_wire::canonical(contents, depth + 1)?,
        )];
        fields.extend(ranged(entries, "width", 200, 1, 1024)?);
        return Ok(finish(fields));
    }
    text_wire::canonical(tag, depth + 1)
}

fn input(tag: &Tag, depth: u32) -> Result<Tag, DecodeError> {
    let entries = read::compound(tag).map_err(|_| bad("input must be a compound"))?;
    let key = string_field(entries, "key")?;
    if !variable_name(&key) {
        return Err(bad("input name is invalid"));
    }
    let kind = match field(entries, "type") {
        Some(Tag::String(kind)) => {
            parse_identifier(kind).map_err(|_| bad("input type is invalid"))?
        }
        _ => return Err(bad("input needs a type")),
    };
    let mut fields = vec![
        ("key".to_owned(), Tag::String(key)),
        ("type".to_owned(), Tag::String(kind.clone())),
    ];
    match kind.as_str() {
        "minecraft:boolean" => {
            let label = field(entries, "label").ok_or_else(|| bad("input needs a label"))?;
            fields.push(("label".to_owned(), text_wire::canonical(label, depth + 1)?));
            fields.extend(optional_flag(entries, "initial", false)?);
            fields.extend(optional_string(entries, "on_true", "true")?);
            fields.extend(optional_string(entries, "on_false", "false")?);
        }
        "minecraft:text" => fields.extend(text_input(entries, depth)?),
        "minecraft:single_option" => fields.extend(single_option(entries, depth)?),
        "minecraft:number_range" => fields.extend(number_range(entries, depth)?),
        _ => return Err(bad("unknown input type")),
    }
    Ok(finish(fields))
}

fn text_input(entries: &[(String, Tag)], depth: u32) -> Result<Vec<(String, Tag)>, DecodeError> {
    let label = field(entries, "label").ok_or_else(|| bad("input needs a label"))?;
    let mut fields = ranged(entries, "width", 200, 1, 1024)?;
    fields.push(("label".to_owned(), text_wire::canonical(label, depth + 1)?));
    fields.extend(optional_flag(entries, "label_visible", true)?);
    let initial = match field(entries, "initial") {
        Some(Tag::String(value)) => value.clone(),
        Some(_) => return Err(bad("initial must be a string")),
        None => String::new(),
    };
    let max_length = match field(entries, "max_length") {
        Some(tag) => positive_number(tag)?,
        None => 32,
    };
    if initial.encode_utf16().count() > max_length as usize {
        return Err(bad("initial text is too long"));
    }
    if !initial.is_empty() {
        fields.push(("initial".to_owned(), Tag::String(initial)));
    }
    if max_length != 32 {
        fields.push(("max_length".to_owned(), Tag::Int(max_length)));
    }
    if let Some(multiline) = field(entries, "multiline") {
        fields.push(("multiline".to_owned(), multiline_options(multiline)?));
    }
    Ok(fields)
}

fn multiline_options(tag: &Tag) -> Result<Tag, DecodeError> {
    let entries = read::compound(tag).map_err(|_| bad("multiline must be a compound"))?;
    let mut fields = Vec::new();
    if let Some(max_lines) = field(entries, "max_lines") {
        fields.push((
            "max_lines".to_owned(),
            Tag::Int(positive_number(max_lines)?),
        ));
    }
    fields.extend(ranged(entries, "height", -1, 1, 512)?);
    Ok(finish(fields))
}

fn single_option(entries: &[(String, Tag)], depth: u32) -> Result<Vec<(String, Tag)>, DecodeError> {
    let mut fields = ranged(entries, "width", 200, 1, 1024)?;
    let options = field(entries, "options").ok_or_else(|| bad("options are required"))?;
    let values = read::list(options).map_err(|_| bad("options must be a list"))?;
    if values.is_empty() {
        return Err(bad("options must not be empty"));
    }
    let mut initial = 0;
    let mut encoded = Vec::new();
    for value in &values {
        let (tag, is_initial) = option_entry(value, depth)?;
        if is_initial {
            initial += 1;
        }
        encoded.push(tag);
    }
    if initial > 1 {
        return Err(bad("multiple initial options"));
    }
    fields.push(("options".to_owned(), Tag::List(encoded)));
    let label = field(entries, "label").ok_or_else(|| bad("input needs a label"))?;
    fields.push(("label".to_owned(), text_wire::canonical(label, depth + 1)?));
    fields.extend(optional_flag(entries, "label_visible", true)?);
    Ok(fields)
}

fn option_entry(tag: &Tag, depth: u32) -> Result<(Tag, bool), DecodeError> {
    if let Tag::String(id) = tag {
        return Ok((
            finish(vec![("id".to_owned(), Tag::String(id.clone()))]),
            false,
        ));
    }
    let entries = read::compound(tag).map_err(|_| bad("option must be a string or a compound"))?;
    let id = string_field(entries, "id")?;
    let initial = optional_bool(entries, "initial", false)?;
    let mut fields = vec![("id".to_owned(), Tag::String(id))];
    if let Some(display) = field(entries, "display") {
        fields.push((
            "display".to_owned(),
            text_wire::canonical(display, depth + 1)?,
        ));
    }
    if initial {
        fields.push(("initial".to_owned(), Tag::Byte(1)));
    }
    Ok((finish(fields), initial))
}

fn number_range(entries: &[(String, Tag)], depth: u32) -> Result<Vec<(String, Tag)>, DecodeError> {
    let label = field(entries, "label").ok_or_else(|| bad("input needs a label"))?;
    let mut fields = ranged(entries, "width", 200, 1, 1024)?;
    fields.push(("label".to_owned(), text_wire::canonical(label, depth + 1)?));
    fields.extend(optional_string(
        entries,
        "label_format",
        "options.generic_value",
    )?);
    let start = float_field(entries, "start")?;
    let end = float_field(entries, "end")?;
    fields.push(("start".to_owned(), Tag::Float(start)));
    fields.push(("end".to_owned(), Tag::Float(end)));
    if let Some(initial) = field(entries, "initial") {
        let initial = read::float(initial).map_err(|_| bad("initial must be a number"))?;
        let min = start.min(end);
        let max = start.max(end);
        if initial < min || initial > max {
            return Err(bad("initial value is outside the range"));
        }
        fields.push(("initial".to_owned(), Tag::Float(initial)));
    }
    if let Some(step) = field(entries, "step") {
        let step = read::float(step).map_err(|_| bad("step must be a number"))?;
        if step <= 0.0 {
            return Err(bad("step must be positive"));
        }
        fields.push(("step".to_owned(), Tag::Float(step)));
    }
    Ok(fields)
}

fn holder_set(tag: &Tag, depth: u32) -> Result<Tag, DecodeError> {
    if let Tag::String(name) = tag {
        return holder_name(name).map(Tag::String);
    }
    if let Tag::List(items) = tag {
        if items.is_empty() {
            return Err(bad("dialog list is empty"));
        }
        let values = items
            .iter()
            .map(|item| rewrite(item, depth + 1))
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(if values.len() == 1 {
            values.into_iter().next().unwrap()
        } else {
            Tag::List(values)
        });
    }
    rewrite(tag, depth + 1)
}

fn holder_name(name: &str) -> Result<String, DecodeError> {
    if let Some(tag) = name.strip_prefix('#') {
        let tag = parse_identifier(tag).map_err(|_| bad("dialog tag is invalid"))?;
        if dialog_tags().contains(&tag.as_str()) {
            return Ok(format!("#{tag}"));
        }
        return Err(bad("unknown dialog tag"));
    }
    let name = parse_identifier(name).map_err(|_| bad("dialog name is invalid"))?;
    if dialog_names().contains(&name.as_str()) {
        Ok(name)
    } else {
        Err(bad("unknown dialog"))
    }
}

fn dialog_names() -> &'static [&'static str] {
    SYNCED_REGISTRIES
        .iter()
        .find(|registry| registry.name == "minecraft:dialog")
        .map(|registry| registry.entries)
        .unwrap_or(&[])
}

fn dialog_tags() -> Vec<&'static str> {
    mc_bamboo_data::tags::TAG_GROUPS
        .iter()
        .find(|group| group.registry == "minecraft:dialog")
        .map(|group| group.tags.iter().map(|tag| tag.name).collect())
        .unwrap_or_default()
}

fn action_name(tag: &Tag) -> Result<&'static str, DecodeError> {
    match read::string(tag).unwrap_or("") {
        "close" => Ok("close"),
        "none" => Ok("none"),
        "wait_for_response" => Ok("wait_for_response"),
        _ => Err(bad("unknown after_action")),
    }
}

fn optional_bool(
    entries: &[(String, Tag)],
    name: &str,
    default: bool,
) -> Result<bool, DecodeError> {
    match field(entries, name) {
        Some(tag) => read::bool_value(tag).map_err(|_| bad("expected a boolean")),
        None => Ok(default),
    }
}

fn optional_flag(
    entries: &[(String, Tag)],
    name: &str,
    default: bool,
) -> Result<Vec<(String, Tag)>, DecodeError> {
    let value = optional_bool(entries, name, default)?;
    if value == default {
        Ok(Vec::new())
    } else {
        Ok(vec![(
            name.to_owned(),
            Tag::Byte(if value { 1 } else { 0 }),
        )])
    }
}

fn optional_string(
    entries: &[(String, Tag)],
    name: &str,
    default: &str,
) -> Result<Vec<(String, Tag)>, DecodeError> {
    match field(entries, name) {
        Some(Tag::String(value)) if value == default => Ok(Vec::new()),
        Some(Tag::String(value)) => Ok(vec![(name.to_owned(), Tag::String(value.clone()))]),
        Some(_) => Err(bad("expected a string")),
        None => Ok(Vec::new()),
    }
}

fn positive(
    entries: &[(String, Tag)],
    name: &str,
    default: i32,
) -> Result<Vec<(String, Tag)>, DecodeError> {
    ranged(entries, name, default, 1, i32::MAX)
}

fn ranged(
    entries: &[(String, Tag)],
    name: &str,
    default: i32,
    min: i32,
    max: i32,
) -> Result<Vec<(String, Tag)>, DecodeError> {
    let value = match field(entries, name) {
        Some(tag) => read::int(tag).map_err(|_| bad("expected a number"))?,
        None => return Ok(Vec::new()),
    };
    if !(min..=max).contains(&value) {
        return Err(bad("number is out of range"));
    }
    if value == default {
        Ok(Vec::new())
    } else {
        Ok(vec![(name.to_owned(), Tag::Int(value))])
    }
}

fn positive_number(tag: &Tag) -> Result<i32, DecodeError> {
    let value = read::int(tag).map_err(|_| bad("expected a number"))?;
    if value < 1 {
        return Err(bad("number must be positive"));
    }
    Ok(value)
}

fn float_field(entries: &[(String, Tag)], name: &str) -> Result<f32, DecodeError> {
    let tag = field(entries, name).ok_or_else(|| bad("missing number"))?;
    read::float(tag).map_err(|_| bad("expected a number"))
}

fn string_field(entries: &[(String, Tag)], name: &str) -> Result<String, DecodeError> {
    match field(entries, name) {
        Some(Tag::String(value)) => Ok(value.clone()),
        _ => Err(bad("missing string field")),
    }
}

fn identifier_field(entries: &[(String, Tag)], name: &str) -> Result<String, DecodeError> {
    identifier(field(entries, name).ok_or_else(|| bad("missing id"))?)
        .map_err(|_| bad("field is not a resource location"))
}

fn http_url(url: String) -> Result<String, DecodeError> {
    let Some((scheme, _)) = url.split_once(':') else {
        return Err(bad("url needs a protocol"));
    };
    if !matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https") {
        return Err(bad("url protocol is not allowed"));
    }
    if url.chars().any(|c| {
        c.is_whitespace()
            || c.is_control()
            || matches!(c, '<' | '>' | '"' | '{' | '}' | '|' | '\\' | '^' | '`')
    }) {
        return Err(bad("url is not valid"));
    }
    Ok(url)
}

fn chat(text: &str) -> Result<(), DecodeError> {
    if text.chars().all(crate::text::is_allowed_chat_character) {
        Ok(())
    } else {
        Err(bad("disallowed chat character"))
    }
}

fn macro_template(text: &str) -> bool {
    let mut rest = text;
    let mut found = false;
    while let Some(start) = rest.find("$(") {
        let after = &rest[start + 2..];
        let Some(end) = after.find(')') else {
            return false;
        };
        if !variable_name(&after[..end]) {
            return false;
        }
        found = true;
        rest = &after[end + 1..];
    }
    found
}

fn variable_name(name: &str) -> bool {
    name.chars().all(|c| c.is_alphanumeric() || c == '_')
}

fn compound_or_empty(tag: &Tag) -> &[(String, Tag)] {
    match tag {
        Tag::Compound(entries) => entries,
        _ => &[],
    }
}

fn field<'a>(entries: &'a [(String, Tag)], name: &str) -> Option<&'a Tag> {
    entries
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value)
}

fn finish(fields: Vec<(String, Tag)>) -> Tag {
    let keys: Vec<String> = fields.iter().map(|(key, _)| key.clone()).collect();
    let order = iteration_order(&keys);
    Tag::Compound(
        order
            .into_iter()
            .map(|index| fields[index].clone())
            .collect(),
    )
}

fn bad(reason: &'static str) -> DecodeError {
    DecodeError::InvalidTextComponent(reason)
}
