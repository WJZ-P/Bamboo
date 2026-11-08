//! 实体选择器参数。官方语法是 `玩家名`、UUID、`@p`、`@a[...]` 这些。
//!
//! Bamboo 目前只有玩家，解析阶段仍然按官方的选择器结构保存，执行时再看服务端有哪些玩家。
//! 官方选择器里依赖 NBT、记分板、进度和战利品表的选项在这里会明确报不支持。

use uuid::Uuid;

use crate::{
    ParseError, Value,
    identifier::Identifier,
    reader::{StringReader, parse_java_double},
    value::{EntitySelector, GameMode, NumberBounds, SortOrder},
};

const OPTIONS_START: char = '[';
const OPTIONS_END: char = ']';
const OPTION_SEPARATOR: char = ',';
const OPTION_ASSIGNMENT: char = '=';
const INVERT: char = '!';
const TAG_PREFIX: char = '#';

/// 官方 `InvertableSetOptionState`。同一个选项最多写一个肯定值，否定值和标签可以写多个。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Limitation {
    None,
    Single,
    Multiple,
}

impl Limitation {
    const fn can_parse_positive(self) -> bool {
        matches!(self, Self::None)
    }

    const fn can_parse_negative(self) -> bool {
        !matches!(self, Self::Single)
    }

    const fn can_parse_any(self) -> bool {
        !matches!(self, Self::Multiple)
    }

    fn mark(self, inverted: bool) -> Self {
        if inverted {
            Self::Multiple
        } else {
            Self::Single
        }
    }
}

struct EntityParser<'a> {
    reader: &'a mut StringReader,
    max_results: i32,
    includes_entities: bool,
    world_limited: bool,
    distance: Option<NumberBounds<f64>>,
    level: Option<NumberBounds<i32>>,
    x: Option<f64>,
    y: Option<f64>,
    z: Option<f64>,
    delta_x: Option<f64>,
    delta_y: Option<f64>,
    delta_z: Option<f64>,
    rotation_x: Option<NumberBounds<f32>>,
    rotation_y: Option<NumberBounds<f32>>,
    order: SortOrder,
    current_entity: bool,
    player_name: Option<String>,
    uuid: Option<Uuid>,
    type_name: Option<String>,
    name_filters: Vec<(String, bool)>,
    type_filters: Vec<(String, bool)>,
    type_tag_filters: Vec<(String, bool)>,
    game_mode_filters: Vec<(GameMode, bool)>,
    team_filters: Vec<(String, bool)>,
    tag_filters: Vec<(String, bool)>,
    name_limit: Limitation,
    game_mode_limit: Limitation,
    type_limit: Limitation,
    team_limit: Limitation,
    type_tags: Vec<String>,
    limited: bool,
    sorted: bool,
    uses_selectors: bool,
}

impl<'a> EntityParser<'a> {
    fn new(reader: &'a mut StringReader) -> Self {
        Self {
            reader,
            max_results: 0,
            includes_entities: false,
            world_limited: false,
            distance: None,
            level: None,
            x: None,
            y: None,
            z: None,
            delta_x: None,
            delta_y: None,
            delta_z: None,
            rotation_x: None,
            rotation_y: None,
            order: SortOrder::Arbitrary,
            current_entity: false,
            player_name: None,
            uuid: None,
            type_name: None,
            name_filters: Vec::new(),
            type_filters: Vec::new(),
            type_tag_filters: Vec::new(),
            game_mode_filters: Vec::new(),
            team_filters: Vec::new(),
            tag_filters: Vec::new(),
            name_limit: Limitation::None,
            game_mode_limit: Limitation::None,
            type_limit: Limitation::None,
            team_limit: Limitation::None,
            type_tags: Vec::new(),
            limited: false,
            sorted: false,
            uses_selectors: false,
        }
    }

    fn parse(mut self) -> Result<EntitySelector, ParseError> {
        if self.reader.can_read() && self.reader.peek() == '@' {
            self.reader.skip();
            self.parse_selector()?;
        } else {
            self.parse_name_or_uuid()?;
        }
        Ok(self.build())
    }

    fn parse_selector(&mut self) -> Result<(), ParseError> {
        self.uses_selectors = true;
        if !self.reader.can_read() {
            return Err(ParseError::new("argument.entity.selector.missing"));
        }
        let start = self.reader.cursor();
        let selector_type = self.reader.read();
        match selector_type {
            'a' => {
                self.max_results = i32::MAX;
                self.includes_entities = false;
                self.order = SortOrder::Arbitrary;
                self.type_name = Some("minecraft:player".into());
            }
            'e' => {
                self.max_results = i32::MAX;
                self.includes_entities = true;
                self.order = SortOrder::Arbitrary;
            }
            'n' => {
                self.max_results = 1;
                self.includes_entities = true;
                self.order = SortOrder::Nearest;
            }
            'p' => {
                self.max_results = 1;
                self.includes_entities = false;
                self.order = SortOrder::Nearest;
                self.type_name = Some("minecraft:player".into());
            }
            'r' => {
                self.max_results = 1;
                self.includes_entities = false;
                self.order = SortOrder::Random;
                self.type_name = Some("minecraft:player".into());
            }
            's' => {
                self.max_results = 1;
                self.includes_entities = true;
                self.current_entity = true;
            }
            other => {
                self.reader.set_cursor(start);
                return Err(ParseError::text(
                    "argument.entity.selector.unknown",
                    format!("@{other}"),
                ));
            }
        }

        if self.reader.can_read() && self.reader.peek() == OPTIONS_START {
            self.reader.skip();
            self.parse_options()?;
        }
        Ok(())
    }

    fn parse_name_or_uuid(&mut self) -> Result<(), ParseError> {
        let start = self.reader.cursor();
        let name = self.reader.read_string()?;
        match Uuid::parse_str(&name) {
            Ok(uuid) => {
                self.uuid = Some(uuid);
                self.includes_entities = true;
            }
            Err(_) => {
                if name.is_empty() || name.chars().count() > 16 {
                    self.reader.set_cursor(start);
                    return Err(ParseError::new("argument.entity.invalid"));
                }
                self.includes_entities = false;
                self.player_name = Some(name);
            }
        }
        self.max_results = 1;
        Ok(())
    }

    fn parse_options(&mut self) -> Result<(), ParseError> {
        self.reader.skip_whitespace();
        while self.reader.can_read() && self.reader.peek() != OPTIONS_END {
            self.reader.skip_whitespace();
            let start = self.reader.cursor();
            let key = self.reader.read_string()?;
            self.check_applicable(&key, start)?;
            self.reader.skip_whitespace();
            if !self.reader.can_read() || self.reader.peek() != OPTION_ASSIGNMENT {
                self.reader.set_cursor(start);
                return Err(ParseError::text("argument.entity.options.valueless", key));
            }
            self.reader.skip();
            self.reader.skip_whitespace();
            self.apply_option(&key, start)?;
            self.reader.skip_whitespace();
            if self.reader.can_read() {
                if self.reader.peek() != OPTION_SEPARATOR {
                    if self.reader.peek() != OPTIONS_END {
                        return Err(ParseError::new("argument.entity.options.unterminated"));
                    }
                    break;
                }
                self.reader.skip();
            }
        }
        if self.reader.can_read() {
            self.reader.skip();
        } else {
            return Err(ParseError::new("argument.entity.options.unterminated"));
        }
        Ok(())
    }

    fn check_applicable(&mut self, key: &str, start: usize) -> Result<(), ParseError> {
        let applicable = match key {
            "name" => self.name_limit.can_parse_any(),
            "distance" => self.distance.is_none(),
            "level" => self.level.is_none(),
            "x" => self.x.is_none(),
            "y" => self.y.is_none(),
            "z" => self.z.is_none(),
            "dx" => self.delta_x.is_none(),
            "dy" => self.delta_y.is_none(),
            "dz" => self.delta_z.is_none(),
            "x_rotation" => self.rotation_x.is_none(),
            "y_rotation" => self.rotation_y.is_none(),
            "limit" => !self.current_entity && !self.limited,
            "sort" => !self.current_entity && !self.sorted,
            "gamemode" => self.game_mode_limit.can_parse_any(),
            "type" => self.type_limit.can_parse_any(),
            "team" => true,
            "tag" => true,
            "nbt" | "scores" | "advancements" | "predicate" => true,
            _ => {
                return Err(self.rollback(
                    start,
                    ParseError::text("argument.entity.options.unknown", key),
                ));
            }
        };
        if applicable {
            Ok(())
        } else {
            Err(self.rollback(
                start,
                ParseError::text("argument.entity.options.inapplicable", key),
            ))
        }
    }

    fn apply_option(&mut self, key: &str, start: usize) -> Result<(), ParseError> {
        match key {
            "name" => self.apply_name(start),
            "distance" => self.apply_distance(start),
            "level" => self.apply_level(start),
            "x" => self.apply_coordinate(CoordinateKind::X),
            "y" => self.apply_coordinate(CoordinateKind::Y),
            "z" => self.apply_coordinate(CoordinateKind::Z),
            "dx" => self.apply_coordinate(CoordinateKind::DeltaX),
            "dy" => self.apply_coordinate(CoordinateKind::DeltaY),
            "dz" => self.apply_coordinate(CoordinateKind::DeltaZ),
            "x_rotation" => {
                self.rotation_x = Some(parse_bounds(
                    self.reader,
                    parse_float,
                    "parsing.float.invalid",
                    false,
                )?);
                Ok(())
            }
            "y_rotation" => {
                self.rotation_y = Some(parse_bounds(
                    self.reader,
                    parse_float,
                    "parsing.float.invalid",
                    false,
                )?);
                Ok(())
            }
            "limit" => self.apply_limit(start),
            "sort" => self.apply_sort(start),
            "gamemode" => self.apply_game_mode(start),
            "type" => self.apply_type(start),
            "team" => self.apply_team(start),
            "tag" => self.apply_tag(),
            "nbt" | "scores" | "advancements" | "predicate" => Err(self.rollback(
                start,
                ParseError::text(
                    "command.exception",
                    format!("entity selector option '{key}' is not supported by Bamboo"),
                ),
            )),
            _ => unreachable!("check_applicable 已经拦下未知选项"),
        }
    }

    fn apply_name(&mut self, start: usize) -> Result<(), ParseError> {
        let inverted = self.should_invert_value();
        let name = self.reader.read_string()?;
        if !self.name_limit.can_parse_positive() && !inverted {
            return Err(self.inapplicable(start, "name"));
        }
        if !self.name_limit.can_parse_negative() && inverted {
            return Err(self.inapplicable(start, "name"));
        }
        self.name_limit = self.name_limit.mark(inverted);
        self.name_filters.push((name, inverted));
        Ok(())
    }

    fn apply_distance(&mut self, start: usize) -> Result<(), ParseError> {
        let bounds = parse_bounds(self.reader, parse_double, "parsing.double.invalid", true)?;
        if bounds.min.is_some_and(|min| min < 0.0) || bounds.max.is_some_and(|max| max < 0.0) {
            return Err(self.rollback(
                start,
                ParseError::new("argument.entity.options.distance.negative"),
            ));
        }
        self.distance = Some(bounds);
        self.world_limited = true;
        Ok(())
    }

    fn apply_level(&mut self, start: usize) -> Result<(), ParseError> {
        let bounds = parse_bounds(self.reader, parse_int, "parsing.int.invalid", true)?;
        if bounds.min.is_some_and(|min| min < 0) || bounds.max.is_some_and(|max| max < 0) {
            return Err(self.rollback(
                start,
                ParseError::new("argument.entity.options.level.negative"),
            ));
        }
        self.level = Some(bounds);
        self.includes_entities = false;
        Ok(())
    }

    fn apply_coordinate(&mut self, kind: CoordinateKind) -> Result<(), ParseError> {
        self.world_limited = true;
        let value = self.reader.read_double()?;
        match kind {
            CoordinateKind::X => self.x = Some(value),
            CoordinateKind::Y => self.y = Some(value),
            CoordinateKind::Z => self.z = Some(value),
            CoordinateKind::DeltaX => self.delta_x = Some(value),
            CoordinateKind::DeltaY => self.delta_y = Some(value),
            CoordinateKind::DeltaZ => self.delta_z = Some(value),
        }
        Ok(())
    }

    fn apply_limit(&mut self, start: usize) -> Result<(), ParseError> {
        let count = self.reader.read_int()?;
        if count < 1 {
            return Err(self.rollback(
                start,
                ParseError::new("argument.entity.options.limit.toosmall"),
            ));
        }
        self.max_results = count;
        self.limited = true;
        Ok(())
    }

    fn apply_sort(&mut self, start: usize) -> Result<(), ParseError> {
        let name = self.reader.read_unquoted_string();
        self.order = match name.as_str() {
            "nearest" => SortOrder::Nearest,
            "furthest" => SortOrder::Furthest,
            "random" => SortOrder::Random,
            "arbitrary" => SortOrder::Arbitrary,
            _ => {
                return Err(self.rollback(
                    start,
                    ParseError::text("argument.entity.options.sort.irreversible", name),
                ));
            }
        };
        self.sorted = true;
        Ok(())
    }

    fn apply_game_mode(&mut self, start: usize) -> Result<(), ParseError> {
        let inverted = self.should_invert_value();
        if !self.game_mode_limit.can_parse_positive() && !inverted {
            return Err(self.inapplicable(start, "gamemode"));
        }
        if !self.game_mode_limit.can_parse_negative() && inverted {
            return Err(self.inapplicable(start, "gamemode"));
        }
        let name = self.reader.read_unquoted_string();
        let Some(mode) = GameMode::by_name(&name) else {
            return Err(self.rollback(
                start,
                ParseError::text("argument.entity.options.mode.invalid", name),
            ));
        };
        self.includes_entities = false;
        self.game_mode_limit = self.game_mode_limit.mark(inverted);
        self.game_mode_filters.push((mode, inverted));
        Ok(())
    }

    fn apply_type(&mut self, start: usize) -> Result<(), ParseError> {
        let inverted = self.should_invert_value();
        if self.is_tag() {
            if !self.type_limit.can_parse_negative() {
                return Err(self.inapplicable(start, "type"));
            }
            let id = Identifier::read(self.reader)?;
            let name = id.to_registry_name();
            if self.type_tags.contains(&name) {
                return Err(self.inapplicable(start, "type"));
            }
            self.type_limit = Limitation::Multiple;
            self.type_tags.push(name.clone());
            self.type_tag_filters.push((name, inverted));
        } else {
            if !self.type_limit.can_parse_positive() && !inverted {
                return Err(self.inapplicable(start, "type"));
            }
            if !self.type_limit.can_parse_negative() && inverted {
                return Err(self.inapplicable(start, "type"));
            }
            let id = Identifier::read(self.reader)?;
            let name = id.to_registry_name();
            if name == "minecraft:player" && !inverted {
                self.includes_entities = false;
            }
            self.type_limit = self.type_limit.mark(inverted);
            if !inverted {
                self.type_name = Some(name.clone());
            }
            self.type_filters.push((name, inverted));
        }
        Ok(())
    }

    fn apply_team(&mut self, start: usize) -> Result<(), ParseError> {
        let inverted = self.should_invert_value();
        let team = self.reader.read_unquoted_string();
        if !self.team_limit.can_parse_positive() && !inverted {
            return Err(self.inapplicable(start, "team"));
        }
        if !self.team_limit.can_parse_negative() && inverted {
            return Err(self.inapplicable(start, "team"));
        }
        self.team_limit = self.team_limit.mark(inverted);
        self.team_filters.push((team, inverted));
        Ok(())
    }

    fn apply_tag(&mut self) -> Result<(), ParseError> {
        let inverted = self.should_invert_value();
        let tag = self.reader.read_unquoted_string();
        self.tag_filters.push((tag, inverted));
        Ok(())
    }

    fn should_invert_value(&mut self) -> bool {
        self.reader.skip_whitespace();
        if self.reader.can_read() && self.reader.peek() == INVERT {
            self.reader.skip();
            self.reader.skip_whitespace();
            true
        } else {
            false
        }
    }

    fn is_tag(&mut self) -> bool {
        self.reader.skip_whitespace();
        if self.reader.can_read() && self.reader.peek() == TAG_PREFIX {
            self.reader.skip();
            self.reader.skip_whitespace();
            true
        } else {
            false
        }
    }

    fn rollback(&mut self, start: usize, error: ParseError) -> ParseError {
        self.reader.set_cursor(start);
        error
    }

    fn inapplicable(&mut self, start: usize, key: &str) -> ParseError {
        self.rollback(
            start,
            ParseError::text("argument.entity.options.inapplicable", key),
        )
    }

    fn build(self) -> EntitySelector {
        EntitySelector {
            max_results: self.max_results,
            includes_entities: self.includes_entities,
            world_limited: self.world_limited,
            distance: self.distance,
            level: self.level,
            x: self.x,
            y: self.y,
            z: self.z,
            delta_x: self.delta_x,
            delta_y: self.delta_y,
            delta_z: self.delta_z,
            rotation_x: self.rotation_x,
            rotation_y: self.rotation_y,
            order: self.order,
            current_entity: self.current_entity,
            player_name: self.player_name,
            uuid: self.uuid,
            type_name: self.type_name,
            name_filters: self.name_filters,
            type_filters: self.type_filters,
            type_tag_filters: self.type_tag_filters,
            game_mode_filters: self.game_mode_filters,
            team_filters: self.team_filters,
            tag_filters: self.tag_filters,
            uses_selectors: self.uses_selectors,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum CoordinateKind {
    X,
    Y,
    Z,
    DeltaX,
    DeltaY,
    DeltaZ,
}

/// 官方 `EntityArgument` 的四种组合。`single` 是只能选一个，`players_only` 是只能选玩家。
pub fn parse_entity(
    reader: &mut StringReader,
    single: bool,
    players_only: bool,
) -> Result<Value, ParseError> {
    let selector = EntityParser::new(reader).parse()?;
    if selector.max_results > 1 && single {
        reader.set_cursor(0);
        return Err(ParseError::new(if players_only {
            "argument.player.toomany"
        } else {
            "argument.entity.toomany"
        }));
    }
    if selector.includes_entities && players_only && !selector.is_self_selector() {
        reader.set_cursor(0);
        return Err(ParseError::new("argument.player.entities"));
    }
    Ok(Value::Entity(Box::new(selector)))
}

fn read_bounds_number(reader: &mut StringReader) -> String {
    let start = reader.cursor();
    while reader.can_read() && is_allowed_bounds_char(reader) {
        reader.skip();
    }
    reader.slice(start, reader.cursor())
}

fn is_allowed_bounds_char(reader: &StringReader) -> bool {
    let character = reader.peek();
    if character.is_ascii_digit() || character == '-' {
        return true;
    }
    character == '.' && (!reader.can_read_n(2) || reader.peek_at(1) != '.')
}

/// 官方 `MinMaxBounds.Bounds.fromReader`。
fn parse_bounds<T: PartialOrd + Copy>(
    reader: &mut StringReader,
    parse_value: fn(&str) -> Option<T>,
    invalid_key: &'static str,
    reject_swapped: bool,
) -> Result<NumberBounds<T>, ParseError> {
    if !reader.can_read() {
        return Err(ParseError::new("argument.range.empty"));
    }
    let start = reader.cursor();
    let min_text = read_bounds_number(reader);
    let min = parse_bound_value(reader, start, &min_text, parse_value, invalid_key)?;
    let (min, max) = if reader.can_read_n(2) && reader.peek() == '.' && reader.peek_at(1) == '.' {
        reader.skip();
        reader.skip();
        let max_text = read_bounds_number(reader);
        let max = parse_bound_value(reader, start, &max_text, parse_value, invalid_key)?;
        (min, max)
    } else {
        (min, min)
    };
    if min.is_none() && max.is_none() {
        return Err(ParseError::new("argument.range.empty"));
    }
    if reject_swapped && min.zip(max).is_some_and(|(min, max)| min > max) {
        reader.set_cursor(start);
        return Err(ParseError::new("argument.range.swapped"));
    }
    Ok(NumberBounds { min, max })
}

fn parse_bound_value<T>(
    reader: &mut StringReader,
    start: usize,
    raw: &str,
    parse_value: fn(&str) -> Option<T>,
    invalid_key: &'static str,
) -> Result<Option<T>, ParseError> {
    if raw.is_empty() {
        return Ok(None);
    }
    parse_value(raw).map(Some).ok_or_else(|| {
        reader.set_cursor(start);
        ParseError::text(invalid_key, raw)
    })
}

fn parse_int(raw: &str) -> Option<i32> {
    raw.parse().ok()
}

fn parse_float(raw: &str) -> Option<f32> {
    parse_java_double(raw).map(|value| value as f32)
}

fn parse_double(raw: &str) -> Option<f64> {
    parse_java_double(raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ErrorArg;

    fn entity(input: &str) -> EntitySelector {
        entity_with(input, false, false)
    }

    fn entity_with(input: &str, single: bool, players_only: bool) -> EntitySelector {
        match parse_entity(&mut StringReader::new(input), single, players_only).unwrap() {
            Value::Entity(selector) => *selector,
            other => panic!("不是实体选择器：{other:?}"),
        }
    }

    fn error(input: &str) -> ParseError {
        match parse_entity(&mut StringReader::new(input), false, false) {
            Ok(value) => panic!("不该成功：{value:?}"),
            Err(error) => error,
        }
    }

    #[test]
    fn names_and_uuids_become_single_results() {
        let selector = entity("Steve");
        assert_eq!(selector.player_name.as_deref(), Some("Steve"));
        assert_eq!(selector.max_results, 1);
        assert!(!selector.includes_entities);

        let uuid = entity_with("dd12be42-52a9-4a91-a8a1-11c01849e498", true, false);
        assert!(uuid.uuid.is_some());
        assert!(uuid.includes_entities);
        assert_eq!(uuid.player_name, None);
    }

    #[test]
    fn selector_types_set_the_official_defaults() {
        let all = entity("@a");
        assert_eq!(all.max_results, i32::MAX);
        assert!(!all.includes_entities);
        assert_eq!(all.order, SortOrder::Arbitrary);

        let nearest = entity("@p");
        assert_eq!(nearest.max_results, 1);
        assert_eq!(nearest.order, SortOrder::Nearest);

        let random = entity("@r");
        assert_eq!(random.order, SortOrder::Random);

        let all_entities = entity("@e");
        assert!(all_entities.includes_entities);
        assert_eq!(all_entities.order, SortOrder::Arbitrary);

        let closest = entity("@n");
        assert_eq!(closest.order, SortOrder::Nearest);

        let this = entity("@s");
        assert!(this.current_entity);
        assert!(this.includes_entities);
    }

    #[test]
    fn selectors_report_bad_types_and_missing_types() {
        let missing = error("@");
        assert_eq!(missing.key, "argument.entity.selector.missing");
        let unknown = error("@x");
        assert_eq!(unknown.key, "argument.entity.selector.unknown");
        assert_eq!(unknown.args, [crate::ErrorArg::Text("@x".into())]);
    }

    #[test]
    fn options_parse_like_the_official_ones() {
        let selector = entity(
            "@e[name=!Alex,distance=..10,level=1..5,gamemode=creative,limit=3,sort=furthest,team=red,tag=foo,type=!zombie,x=1,y=2,z=3,dx=4,dy=5,dz=6,x_rotation=10..20,y_rotation=-5..5]",
        );
        assert_eq!(selector.name_filters, [("Alex".to_owned(), true)]);
        assert_eq!(
            selector.distance,
            Some(NumberBounds {
                min: None,
                max: Some(10.0)
            })
        );
        assert_eq!(
            selector.level,
            Some(NumberBounds {
                min: Some(1),
                max: Some(5)
            })
        );
        assert_eq!(selector.game_mode_filters, [(GameMode::Creative, false)]);
        assert_eq!(selector.max_results, 3);
        assert_eq!(selector.order, SortOrder::Furthest);
        assert_eq!(selector.team_filters, [("red".to_owned(), false)]);
        assert_eq!(selector.tag_filters, [("foo".to_owned(), false)]);
        assert_eq!(
            selector.type_filters,
            [("minecraft:zombie".to_owned(), true)]
        );
        assert_eq!(
            (selector.x, selector.y, selector.z),
            (Some(1.0), Some(2.0), Some(3.0))
        );
        assert_eq!(
            (selector.delta_x, selector.delta_y, selector.delta_z),
            (Some(4.0), Some(5.0), Some(6.0))
        );
        assert_eq!(
            selector.rotation_x,
            Some(NumberBounds {
                min: Some(10.0),
                max: Some(20.0)
            })
        );
        assert_eq!(
            selector.rotation_y,
            Some(NumberBounds {
                min: Some(-5.0),
                max: Some(5.0)
            })
        );
        assert!(selector.uses_selectors);
    }

    #[test]
    fn duplicate_and_inapplicable_options_use_the_official_keys() {
        assert_eq!(
            error("@e[limit=2,limit=3]").key,
            "argument.entity.options.inapplicable"
        );
        assert_eq!(
            error("@e[name=a,name=b]").key,
            "argument.entity.options.inapplicable"
        );
        assert_eq!(
            error("@s[limit=2]").key,
            "argument.entity.options.inapplicable"
        );
        assert_eq!(
            error("@e[distance=1,distance=2]").key,
            "argument.entity.options.inapplicable"
        );
        assert_eq!(error("@e[wat=1]").key, "argument.entity.options.unknown");
        assert_eq!(error("@e[limit]").key, "argument.entity.options.valueless");
        assert_eq!(
            error("@e[sort=sideways]").key,
            "argument.entity.options.sort.irreversible"
        );
        assert_eq!(
            error("@e[gamemode=hardcore]").key,
            "argument.entity.options.mode.invalid"
        );
        assert_eq!(
            error("@e[limit=0]").key,
            "argument.entity.options.limit.toosmall"
        );
        assert_eq!(
            error("@e[distance=..-1]").key,
            "argument.entity.options.distance.negative"
        );
        assert_eq!(
            error("@e[level=-1..]").key,
            "argument.entity.options.level.negative"
        );
        assert_eq!(error("@e[distance=5..1]").key, "argument.range.swapped");
    }

    #[test]
    fn single_and_players_only_checks_use_the_official_keys() {
        let mut reader = StringReader::new("@a");
        let error = parse_entity(&mut reader, true, false).unwrap_err();
        assert_eq!(error.key, "argument.entity.toomany");
        assert_eq!(reader.cursor(), 0);

        let mut reader = StringReader::new("@a");
        let error = parse_entity(&mut reader, true, true).unwrap_err();
        assert_eq!(error.key, "argument.player.toomany");

        let mut reader = StringReader::new("@e");
        let error = parse_entity(&mut reader, false, true).unwrap_err();
        assert_eq!(error.key, "argument.player.entities");

        // @s 是玩家自己，允许出现在只收玩家的参数里。
        assert!(parse_entity(&mut StringReader::new("@s"), true, true).is_ok());
    }

    #[test]
    fn unsupported_options_fail_transparently() {
        let error = error("@e[nbt={a:1}]");
        assert_eq!(error.key, "command.exception");
        assert!(matches!(&error.args[0], ErrorArg::Text(text) if text.contains("nbt")));
    }
}
