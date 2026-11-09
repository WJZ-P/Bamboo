//! 计分板、队伍和 Boss 栏的参数。
//!
//! 目标、队伍、准则、运算、显示位置、记分板名字、颜色、文本和样式。
//! 存在性留到执行时再查，解析只认语法。
//!
//! Arguments for the scoreboard, teams, and boss bars.
//!
//! Objectives, teams, criteria, operations, display slots, score holders, colors,
//! text, and styles. Existence is checked at execution; parsing only checks syntax.

use mc_bamboo_nbt::{Tag, parse_snbt_prefix};
use mc_bamboo_protocol::components::text::checked_component;

use crate::{
    ParseError, Value,
    argument::entity,
    reader::StringReader,
    value::{DisplaySlot, ObjectiveCriteria, ScoreHolderSpec, ScoreOperation, TeamColorId},
};

const STYLE_KEYS: &[&str] = &[
    "color",
    "shadow_color",
    "bold",
    "italic",
    "underlined",
    "strikethrough",
    "obfuscated",
    "click_event",
    "hover_event",
    "insertion",
    "font",
];

const CUSTOM_STATS: &[&str] = &[
    "leave_game",
    "play_time",
    "total_world_time",
    "time_since_death",
    "time_since_rest",
    "sneak_time",
    "walk_one_cm",
    "crouch_one_cm",
    "sprint_one_cm",
    "walk_on_water_one_cm",
    "fall_one_cm",
    "climb_one_cm",
    "fly_one_cm",
    "walk_under_water_one_cm",
    "minecart_one_cm",
    "boat_one_cm",
    "pig_one_cm",
    "happy_ghast_one_cm",
    "horse_one_cm",
    "aviate_one_cm",
    "swim_one_cm",
    "strider_one_cm",
    "nautilus_one_cm",
    "jump",
    "drop",
    "damage_dealt",
    "damage_dealt_absorbed",
    "damage_dealt_resisted",
    "damage_taken",
    "damage_blocked_by_shield",
    "damage_absorbed",
    "damage_resisted",
    "deaths",
    "mob_kills",
    "animals_bred",
    "player_kills",
    "fish_caught",
    "talked_to_villager",
    "traded_with_villager",
    "eat_cake_slice",
    "fill_cauldron",
    "use_cauldron",
    "clean_armor",
    "clean_banner",
    "clean_shulker_box",
    "interact_with_brewingstand",
    "interact_with_beacon",
    "inspect_dropper",
    "inspect_hopper",
    "inspect_dispenser",
    "play_noteblock",
    "tune_noteblock",
    "pot_flower",
    "trigger_trapped_chest",
    "open_enderchest",
    "enchant_item",
    "play_record",
    "interact_with_furnace",
    "interact_with_crafting_table",
    "open_chest",
    "sleep_in_bed",
    "sleep_in_straw_bed",
    "open_shulker_box",
    "open_barrel",
    "interact_with_blast_furnace",
    "interact_with_smoker",
    "interact_with_lectern",
    "interact_with_campfire",
    "interact_with_cartography_table",
    "interact_with_loom",
    "interact_with_stonecutter",
    "bell_ring",
    "raid_trigger",
    "raid_win",
    "interact_with_anvil",
    "interact_with_grindstone",
    "target_hit",
    "interact_with_smithing_table",
];

/// 官方 `ObjectiveArgument.parse`：一个不带引号的词，现在不查它在不在。
///
/// Official `ObjectiveArgument.parse`: one unquoted word. Existence is not checked yet.
pub fn parse_objective(reader: &mut StringReader) -> Result<Value, ParseError> {
    Ok(Value::String(reader.read_unquoted_string()))
}

/// 官方 `TeamArgument.parse`。
///
/// Official `TeamArgument.parse`.
pub fn parse_team(reader: &mut StringReader) -> Result<Value, ParseError> {
    Ok(Value::String(reader.read_unquoted_string()))
}

/// 官方 `ObjectiveCriteriaArgument.parse`。认不出来时游标回到这个词的开头。
///
/// Official `ObjectiveCriteriaArgument.parse`. An unknown name moves the cursor back to
/// the start of the word.
pub fn parse_criteria(reader: &mut StringReader) -> Result<Value, ParseError> {
    let start = reader.cursor();
    let name = read_until_space(reader);
    match criteria_by_name(&name) {
        Some(criteria) => Ok(Value::Criteria(criteria)),
        None => {
            reader.set_cursor(start);
            Err(ParseError::text("argument.criteria.invalid", name))
        }
    }
}

/// 官方 `OperationArgument.parse`。不认识的符号不把游标拨回去。
///
/// Official `OperationArgument.parse`. An unknown symbol does not move the cursor back.
pub fn parse_operation(reader: &mut StringReader) -> Result<Value, ParseError> {
    let text = read_until_space(reader);
    let operation = match text.as_str() {
        "=" => ScoreOperation::Assign,
        "+=" => ScoreOperation::Add,
        "-=" => ScoreOperation::Subtract,
        "*=" => ScoreOperation::Multiply,
        "/=" => ScoreOperation::Divide,
        "%=" => ScoreOperation::Modulo,
        "<" => ScoreOperation::Min,
        ">" => ScoreOperation::Max,
        "><" => ScoreOperation::Swap,
        _ => return Err(ParseError::new("arguments.operation.invalid")),
    };
    Ok(Value::Operation(operation))
}

/// 官方 `ScoreboardSlotArgument.parse`。
///
/// Official `ScoreboardSlotArgument.parse`.
pub fn parse_display_slot(reader: &mut StringReader) -> Result<Value, ParseError> {
    let name = reader.read_unquoted_string();
    match DisplaySlot::parse(&name) {
        Some(slot) => Ok(Value::DisplaySlot(slot)),
        None => Err(ParseError::text(
            "argument.scoreboardDisplaySlot.invalid",
            name,
        )),
    }
}

/// 官方 `TeamColorArgument.parse`。
///
/// Official `TeamColorArgument.parse`.
pub fn parse_team_color(reader: &mut StringReader) -> Result<Value, ParseError> {
    let name = reader.read_unquoted_string();
    match TeamColorId::parse(&name) {
        Some(color) => Ok(Value::TeamColor(color)),
        None => Err(ParseError::text("argument.color.invalid", name)),
    }
}

/// 官方 `ScoreHolderArgument.parse`。`*`、`#名字`、UUID、玩家名和选择器都留到执行时再分辨。
///
/// Official `ScoreHolderArgument.parse`. `*`, `#name`, UUIDs, player names, and selectors
/// are distinguished at execution.
pub fn parse_score_holder(
    reader: &mut StringReader,
    multiple: bool,
    allow_selectors: bool,
) -> Result<Value, ParseError> {
    if reader.can_read() && reader.peek() == '@' {
        if !allow_selectors {
            return Err(ParseError::new("argument.entity.selector.not_allowed"));
        }
        let Value::Entity(selector) = entity::parse_entity(reader, false, false)? else {
            return Err(ParseError::new("argument.entity.notfound.entity"));
        };
        if !multiple && selector.max_results > 1 {
            return Err(ParseError::new("argument.entity.toomany"));
        }
        return Ok(Value::ScoreHolder(ScoreHolderSpec::Selector(selector)));
    }
    let text = read_until_space(reader);
    let spec = if text == "*" {
        ScoreHolderSpec::Wildcard
    } else {
        ScoreHolderSpec::Text(text)
    };
    Ok(Value::ScoreHolder(spec))
}

/// 官方 `StyleArgument`：一段复合 SNBT，按样式字段校验。
///
/// Official `StyleArgument`: one compound of SNBT, checked as style fields.
pub fn parse_style(reader: &mut StringReader) -> Result<Value, ParseError> {
    let start = reader.cursor();
    let tag = read_snbt(reader, "argument.style.invalid")?;
    match style_tag(&tag) {
        Some(style) => Ok(Value::Style(style)),
        None => {
            reader.set_cursor(start);
            Err(ParseError::text("argument.style.invalid", "invalid style"))
        }
    }
}

pub fn suggest_criteria(lower: &str, out: &mut Vec<String>) {
    for name in [
        "dummy",
        "trigger",
        "deathCount",
        "playerKillCount",
        "totalKillCount",
        "health",
        "food",
        "air",
        "armor",
        "xp",
        "level",
    ] {
        suggest_word(name, lower, out);
    }
    for color in TeamColorId::suggestions() {
        suggest_word(&format!("teamkill.{color}"), lower, out);
        suggest_word(&format!("killedByTeam.{color}"), lower, out);
    }
}

pub fn suggest_operations(lower: &str, out: &mut Vec<String>) {
    for operation in ["=", "+=", "-=", "*=", "/=", "%=", "<", ">", "><"] {
        suggest_word(operation, lower, out);
    }
}

pub fn suggest_slots(lower: &str, out: &mut Vec<String>) {
    for slot in DisplaySlot::suggestions() {
        suggest_word(slot, lower, out);
    }
}

pub fn suggest_colors(lower: &str, out: &mut Vec<String>) {
    for color in TeamColorId::suggestions() {
        suggest_word(color, lower, out);
    }
}

pub fn suggest_holders(lower: &str, player_names: &[&str], out: &mut Vec<String>) {
    suggest_word("*", lower, out);
    for name in player_names {
        suggest_word(name, lower, out);
    }
}

fn criteria_by_name(name: &str) -> Option<ObjectiveCriteria> {
    if is_custom(name) {
        return Some(ObjectiveCriteria {
            name: name.to_owned(),
            read_only: matches!(name, "health" | "food" | "air" | "armor" | "xp" | "level"),
            hearts: name == "health",
        });
    }
    stat_criteria(name)
}

fn is_custom(name: &str) -> bool {
    matches!(
        name,
        "dummy"
            | "trigger"
            | "deathCount"
            | "playerKillCount"
            | "totalKillCount"
            | "health"
            | "food"
            | "air"
            | "armor"
            | "xp"
            | "level"
    ) || team_criterion(name)
}

fn team_criterion(name: &str) -> bool {
    let Some(color) = name
        .strip_prefix("teamkill.")
        .or_else(|| name.strip_prefix("killedByTeam."))
    else {
        return false;
    };
    TeamColorId::parse(color).is_some()
}

fn stat_criteria(name: &str) -> Option<ObjectiveCriteria> {
    let (type_name, value_name) = name.split_once(':')?;
    let type_id = type_name.replace('.', ":");
    let value_id = value_name.replace('.', ":");
    let known = match type_id.as_str() {
        "minecraft:mined" => mc_bamboo_data::block_by_name(&value_id).is_some(),
        "minecraft:crafted"
        | "minecraft:used"
        | "minecraft:broken"
        | "minecraft:picked_up"
        | "minecraft:dropped" => mc_bamboo_data::item_by_name(&value_id).is_some(),
        "minecraft:killed" | "minecraft:killed_by" => {
            mc_bamboo_data::registries::entity_type::NAMES
                .binary_search(&value_id.as_str())
                .is_ok()
        }
        "minecraft:custom" => value_id
            .strip_prefix("minecraft:")
            .is_some_and(|path| CUSTOM_STATS.contains(&path)),
        _ => false,
    };
    if !known {
        return None;
    }
    Some(ObjectiveCriteria {
        name: format!(
            "{}:{}",
            type_id.replace(':', "."),
            value_id.replace(':', ".")
        ),
        read_only: false,
        hearts: false,
    })
}

fn read_snbt(reader: &mut StringReader, key: &'static str) -> Result<Tag, ParseError> {
    let start = reader.cursor();
    let rest = reader.remaining();
    match parse_snbt_prefix(&rest) {
        Ok((tag, consumed)) => {
            reader.set_cursor(start + consumed);
            Ok(tag)
        }
        Err(error) => {
            let chars = rest.chars().count();
            reader.set_cursor(start + error.position.min(chars));
            Err(ParseError::text(key, error.message))
        }
    }
}

fn style_tag(tag: &Tag) -> Option<Tag> {
    let Tag::Compound(entries) = tag else {
        return None;
    };
    let mut wrapped = vec![("text".to_owned(), Tag::String(String::new()))];
    for (key, value) in entries {
        if key != "text" {
            wrapped.push((key.clone(), value.clone()));
        }
    }
    let checked = checked_component(&Tag::Compound(wrapped))?;
    Some(extract_style(checked.tag()))
}

fn extract_style(tag: &Tag) -> Tag {
    let Tag::Compound(entries) = tag else {
        return Tag::Compound(Vec::new());
    };
    Tag::Compound(
        entries
            .iter()
            .filter(|(key, _)| STYLE_KEYS.contains(&key.as_str()))
            .cloned()
            .collect(),
    )
}

fn read_until_space(reader: &mut StringReader) -> String {
    let start = reader.cursor();
    while reader.can_read() && reader.peek() != ' ' {
        reader.skip();
    }
    reader.slice(start, reader.cursor())
}

fn suggest_word(candidate: &str, lower: &str, out: &mut Vec<String>) {
    if super::matches_sub_str(lower, &candidate.to_lowercase()) {
        out.push(candidate.to_owned());
    }
}
