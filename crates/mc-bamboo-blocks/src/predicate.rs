//! 方块谓词，官方 `AdventureModePredicate` 和 `BlockPredicate.matches(BlockInWorld)`。
//!
//! `can_break` 和 `can_place_on` 组件里的每条谓词先比方块、再比方块状态属性。
//! 带 NBT 的谓词要求方块实体，Bamboo 还读不出方块实体的 NBT，一律按不匹配算；
//! `components` 字段是给 `matches(ServerLevel, pos)` 用的，官方的 `BlockInWorld`
//! 这个重载不看，这里也不看。
//!
//! 属性的类型和顺序都来自生成的数据，见 [`mc_bamboo_data::PropertyKind`]。

use mc_bamboo_data::{PropertyDef, PropertyKind, block_id_of_state, block_tag};
use mc_bamboo_protocol::components::{
    cursor::HolderSet,
    predicate::{BlockPredicate, PropertyMatcher, StateProperties, ValueMatcher},
};

use crate::state::BlockState;

/// 官方 `AdventureModePredicate.test`：有一条谓词匹配就算匹配。
pub fn any_matches(predicates: &[BlockPredicate], state: BlockState) -> bool {
    predicates.iter().any(|predicate| matches(predicate, state))
}

/// 官方 `BlockPredicate.matches(BlockInWorld)`：方块、方块状态属性、方块实体 NBT。
pub fn matches(predicate: &BlockPredicate, state: BlockState) -> bool {
    if let Some(blocks) = &predicate.blocks {
        let block = block_id_of_state(state.0).unwrap_or_default() as i32;
        if !blocks_match(blocks, block) {
            return false;
        }
    }
    if let Some(properties) = &predicate.properties
        && !properties_match(properties, state)
    {
        return false;
    }
    predicate.nbt.is_none()
}

/// 官方 `BlockPredicate.matchesState` 里 `state.is(blocks)` 那一半。
fn blocks_match(blocks: &HolderSet, block: i32) -> bool {
    match blocks {
        HolderSet::Tag(name) => block_tag(name).is_some_and(|ids| ids.contains(&block)),
        HolderSet::Ids(ids) => ids.contains(&block),
    }
}

/// 官方 `StatePropertiesPredicate.matches(BlockState)`：每一条都要满足。
fn properties_match(properties: &StateProperties, state: BlockState) -> bool {
    properties
        .0
        .iter()
        .all(|matcher| property_matches(matcher, state))
}

/// 官方 `PropertyMatcher.match`：找不到属性名就是不匹配。
fn property_matches(matcher: &PropertyMatcher, state: BlockState) -> bool {
    let Some(def) = state
        .block()
        .properties
        .iter()
        .find(|property| property.name == matcher.name)
    else {
        return false;
    };
    let Some(actual) = state
        .property_text(&matcher.name)
        .and_then(|text| parse_value(def, text))
    else {
        return false;
    };
    match &matcher.matcher {
        // `ExactMatcher.match`：两边都解析成属性值再比；期望值解析不出来就是不匹配。
        ValueMatcher::Exact(expected) => {
            parse_value(def, expected).is_some_and(|value| value == actual)
        }
        // `RangedMatcher.match`：缺的端点不设限，端点解析不出来就是不匹配。
        ValueMatcher::Ranged { min, max } => {
            min.as_deref()
                .is_none_or(|min| parse_value(def, min).is_some_and(|value| actual >= value))
                && max
                    .as_deref()
                    .is_none_or(|max| parse_value(def, max).is_some_and(|value| actual <= value))
        }
    }
}

/// 官方 `Property.getValue`：把一个字符串解析成属性值，解析不出来时返回 `None`。
///
/// 整数属性按 `Integer.parseInt` 解析，再检查在 `min..=max` 里；布尔属性只认
/// `true` 和 `false`；枚举属性按序列化名查表，再换成枚举常量的声明序号。
fn parse_value(def: &PropertyDef, text: &str) -> Option<Value> {
    match def.kind {
        PropertyKind::Integer { min, max } => parse_java_int(text)
            .filter(|value| (min..=max).contains(value))
            .map(Value::Integer),
        PropertyKind::Boolean => match text {
            "true" => Some(Value::Boolean(true)),
            "false" => Some(Value::Boolean(false)),
            _ => None,
        },
        PropertyKind::Enum { ordinals } => {
            let index = def.values.iter().position(|value| *value == text)?;
            ordinals.get(index).map(|&ordinal| Value::Enum(ordinal))
        }
    }
}

/// OpenJDK 26.0.1 里 `Character.digit(codeUnit, 10) >= 0` 的基本多文种平面区间起点。
///
/// 每个区间连续 10 个码元，数值是 `码元 - 起点`。补充平面也有这样的区间，但
/// `Integer.parseInt` 按 UTF-16 码元调用 `Character.digit(char, 10)`，代理项不是数字，
/// 那些区间不在这里。
const DECIMAL_DIGIT_ZERO: &[u16] = &[
    0x0030, 0x0660, 0x06F0, 0x07C0, 0x0966, 0x09E6, 0x0A66, 0x0AE6, 0x0B66, 0x0BE6, 0x0C66, 0x0CE6,
    0x0D66, 0x0DE6, 0x0E50, 0x0ED0, 0x0F20, 0x1040, 0x1090, 0x17E0, 0x1810, 0x1946, 0x19D0, 0x1A80,
    0x1A90, 0x1B50, 0x1BB0, 0x1C40, 0x1C50, 0xA620, 0xA8D0, 0xA900, 0xA9D0, 0xA9F0, 0xAA50, 0xABF0,
    0xFF10,
];

/// `Character.digit(codeUnit, 10)`。认不出来时返回 `None`，官方返回 `-1`。
fn java_digit(unit: u16) -> Option<i32> {
    DECIMAL_DIGIT_ZERO.iter().find_map(|&zero| {
        let offset = unit.wrapping_sub(zero);
        (offset < 10).then_some(i32::from(offset))
    })
}

/// `Integer.parseInt`，进制固定 10。溢出、空串、多余字符都是 `None`。
///
/// 符号只认 ASCII 的 `+` 和 `-`。数字按 UTF-16 码元交给 [`java_digit`]，和官方
/// `s.charAt` 再 `Character.digit(char, 10)` 一样。累加用负数，这样 `Integer.MIN_VALUE` 放得下。
fn parse_java_int(text: &str) -> Option<i32> {
    let units: Vec<u16> = text.encode_utf16().collect();
    let len = units.len();
    if len == 0 {
        return None;
    }
    let mut negative = false;
    let mut index = 0;
    let mut limit = -i32::MAX;
    if units[0] < u16::from(b'0') {
        if units[0] == u16::from(b'-') {
            negative = true;
            limit = i32::MIN;
        } else if units[0] != u16::from(b'+') {
            return None;
        }
        if len == 1 {
            return None;
        }
        index += 1;
    }
    let multmin = limit / 10;
    let mut result = 0i32;
    while index < len {
        let digit = java_digit(units[index])?;
        index += 1;
        if result < multmin {
            return None;
        }
        let product = result.checked_mul(10)?;
        if product < limit + digit {
            return None;
        }
        result = product - digit;
    }
    Some(if negative { result } else { -result })
}

/// 属性值在官方 `compareTo` 里的顺序：整数按数值，布尔 `false < true`，枚举按声明序号。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Value {
    Integer(i32),
    Boolean(bool),
    Enum(u16),
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::{PropertyKind, block_id_of_state, blocks};
    use mc_bamboo_nbt::Tag;

    use super::*;
    use crate::state::LAYERS;

    fn predicate(
        blocks: Option<HolderSet>,
        properties: Option<StateProperties>,
        nbt: Option<Vec<(String, Tag)>>,
    ) -> BlockPredicate {
        BlockPredicate {
            blocks,
            properties,
            nbt,
            components: Default::default(),
        }
    }

    fn block_ids(states: &[u32]) -> HolderSet {
        HolderSet::Ids(
            states
                .iter()
                .map(|&state| block_id_of_state(state).unwrap() as i32)
                .collect(),
        )
    }

    fn exact(name: &str, value: &str) -> StateProperties {
        StateProperties(vec![PropertyMatcher {
            name: name.to_owned(),
            matcher: ValueMatcher::Exact(value.to_owned()),
        }])
    }

    fn ranged(name: &str, min: Option<&str>, max: Option<&str>) -> StateProperties {
        StateProperties(vec![PropertyMatcher {
            name: name.to_owned(),
            matcher: ValueMatcher::Ranged {
                min: min.map(str::to_owned),
                max: max.map(str::to_owned),
            },
        }])
    }

    fn def(state: BlockState, name: &str) -> &'static PropertyDef {
        state
            .block()
            .properties
            .iter()
            .find(|property| property.name == name)
            .expect("这块方块有这个属性")
    }

    #[test]
    fn an_empty_predicate_matches_every_block() {
        let predicate = predicate(None, None, None);
        assert!(matches(&predicate, BlockState(blocks::STONE)));
        assert!(matches(&predicate, BlockState(blocks::DIRT)));
    }

    #[test]
    fn block_lists_and_tags_are_matched() {
        let stone = BlockState(blocks::STONE);
        assert!(matches(
            &predicate(Some(block_ids(&[blocks::STONE])), None, None),
            stone
        ));
        assert!(!matches(
            &predicate(Some(block_ids(&[blocks::DIRT])), None, None),
            stone
        ));
        assert!(matches(
            &predicate(
                Some(HolderSet::Tag("minecraft:mineable/pickaxe".to_owned())),
                None,
                None,
            ),
            stone
        ));
        assert!(!matches(
            &predicate(
                Some(HolderSet::Tag("minecraft:mineable/pickaxe".to_owned())),
                None,
                None,
            ),
            BlockState(blocks::WATER)
        ));
    }

    #[test]
    fn exact_properties_must_match_the_block_state() {
        let log = BlockState(blocks::OAK_LOG);
        assert!(matches(
            &predicate(None, Some(exact("axis", "y")), None),
            log
        ));
        assert!(!matches(
            &predicate(None, Some(exact("axis", "x")), None),
            log
        ));
        // 属性名不存在，和官方 `getProperty` 返回空一样。
        assert!(!matches(
            &predicate(None, Some(exact("nope", "y")), None),
            log
        ));
    }

    #[test]
    fn every_property_of_a_predicate_must_match() {
        let properties = StateProperties(vec![
            PropertyMatcher {
                name: "axis".to_owned(),
                matcher: ValueMatcher::Exact("y".to_owned()),
            },
            PropertyMatcher {
                name: "no_such_property".to_owned(),
                matcher: ValueMatcher::Exact("y".to_owned()),
            },
        ]);
        assert!(!matches(
            &predicate(None, Some(properties), None),
            BlockState(blocks::OAK_LOG)
        ));
    }

    #[test]
    fn exact_integer_properties_parse_the_value_like_java() {
        let snow = BlockState(blocks::SNOW).with(LAYERS, 3);
        let matches_at = |expected| {
            matches(
                &predicate(None, Some(exact("layers", expected)), None),
                snow,
            )
        };
        // `IntegerProperty.getValue` 用的是 `Integer.parseInt`，前导零和正号都行。
        assert!(matches_at("3"));
        assert!(matches_at("03"));
        assert!(matches_at("+3"));
        // 全角 `３`、阿拉伯-印度数字 `٣`，`Character.digit` 都是 3。
        assert!(matches_at("\u{FF13}"));
        assert!(matches_at("\u{0663}"));
        assert!(matches_at("+\u{FF13}"));
        assert!(matches_at("\u{FF10}\u{FF13}"));
        // 全角加号不是 `+`，上标 `³` 也不是十进制数字。
        assert!(!matches_at("\u{FF0B}\u{FF13}"));
        assert!(!matches_at("\u{00B3}"));
        // 超出属性的取值范围，官方解析成空。
        assert!(!matches_at("30"));
        assert!(!matches_at("not a number"));
    }

    #[test]
    fn java_int_accepts_every_bmp_decimal_run_and_rejects_the_rest() {
        assert_eq!(parse_java_int("-2147483648"), Some(i32::MIN));
        assert_eq!(parse_java_int("2147483647"), Some(i32::MAX));
        assert_eq!(parse_java_int("2147483648"), None);
        assert_eq!(parse_java_int("-2147483649"), None);
        assert_eq!(parse_java_int(""), None);
        assert_eq!(parse_java_int("+"), None);
        assert_eq!(parse_java_int("-"), None);
        assert_eq!(parse_java_int("++3"), None);
        assert_eq!(parse_java_int(" 3"), None);
        assert_eq!(parse_java_int("3 "), None);
        assert_eq!(parse_java_int("3a"), None);
        assert_eq!(parse_java_int("-\u{FF10}"), Some(0));
        // 补充平面的十进制数字，`Character.digit(codePoint)` 认，`parseInt` 不认。
        assert_eq!(parse_java_int("\u{104A0}"), None);
        assert_eq!(parse_java_int("\u{1D7CE}"), None);
        assert_eq!(parse_java_int("\u{1D7D2}"), None);

        let snow = BlockState(blocks::SNOW);
        let layers = def(snow, "layers");
        for &zero in DECIMAL_DIGIT_ZERO {
            let digit = char::from_u32(u32::from(zero) + 3).unwrap();
            assert_eq!(
                parse_value(layers, &digit.to_string()),
                Some(Value::Integer(3)),
                "U+{zero:04X}"
            );
            let before = char::from_u32(u32::from(zero) - 1).unwrap();
            assert_eq!(
                parse_java_int(&before.to_string()),
                None,
                "U+{:04X}",
                zero - 1
            );
            let after = char::from_u32(u32::from(zero) + 10).unwrap();
            assert_eq!(
                parse_java_int(&after.to_string()),
                None,
                "U+{:04X}",
                zero + 10
            );
        }
    }

    #[test]
    fn ranged_properties_compare_numbers() {
        let snow = BlockState(blocks::SNOW).with(LAYERS, 3);
        let matches_at = |min, max| {
            matches(
                &predicate(None, Some(ranged("layers", min, max)), None),
                snow,
            )
        };
        assert!(matches_at(Some("2"), Some("4")));
        assert!(matches_at(Some("3"), Some("3")));
        assert!(matches_at(None, Some("3")));
        assert!(!matches_at(Some("4"), None));
        assert!(!matches_at(None, Some("2")));
        // 端点解析不出来时不匹配，官方也是。
        assert!(!matches_at(Some("not a number"), None));
    }

    #[test]
    fn ranged_properties_on_an_enum_use_the_declaration_order() {
        // `axis` 的值按 x、y、z 声明，默认是 y。
        let log = BlockState(blocks::OAK_LOG);
        let matches_at =
            |min, max| matches(&predicate(None, Some(ranged("axis", min, max)), None), log);
        assert!(matches_at(Some("x"), Some("z")));
        assert!(matches_at(Some("y"), None));
        assert!(!matches_at(Some("z"), None));
        assert!(!matches_at(None, Some("x")));
    }

    #[test]
    fn ranged_boolean_properties_order_false_before_true() {
        // 官方 `BooleanProperty` 的取值列表是 [true, false]，而 `false < true`。
        let stairs = BlockState(blocks::OAK_STAIRS);
        let matches_at = |min, max| {
            matches(
                &predicate(None, Some(ranged("waterlogged", min, max)), None),
                stairs,
            )
        };
        assert!(matches_at(Some("false"), None));
        assert!(!matches_at(Some("true"), None));
        assert!(matches_at(None, Some("false")));
        assert!(matches_at(None, Some("true")));
    }

    #[test]
    fn ranged_vertical_direction_uses_the_enum_order() {
        // 官方的取值列表是 [UP, DOWN]，但 `Direction` 枚举里 `DOWN < UP`，默认是 up。
        let dripstone = BlockState(blocks::POINTED_DRIPSTONE);
        let matches_at = |min, max| {
            matches(
                &predicate(None, Some(ranged("vertical_direction", min, max)), None),
                dripstone,
            )
        };
        assert!(matches_at(Some("down"), None));
        assert!(matches_at(Some("up"), Some("up")));
        assert!(!matches_at(None, Some("down")));
    }

    #[test]
    fn nbt_predicates_never_match_because_block_entities_cannot_be_read() {
        assert!(!matches(
            &predicate(None, None, Some(Vec::new())),
            BlockState(blocks::STONE)
        ));
    }

    /// 生成器导出的类型和顺序就是官方的类型和顺序。
    #[test]
    fn the_exported_kinds_are_the_official_ones() {
        let stairs = BlockState(blocks::OAK_STAIRS);
        assert_eq!(def(stairs, "waterlogged").kind, PropertyKind::Boolean);

        let snow = BlockState(blocks::SNOW);
        assert_eq!(
            def(snow, "layers").kind,
            PropertyKind::Integer { min: 1, max: 8 }
        );

        // `Axis` 按 X、Y、Z 声明，取值列表就是声明顺序。
        let log = BlockState(blocks::OAK_LOG);
        assert_eq!(
            def(log, "axis").kind,
            PropertyKind::Enum {
                ordinals: &[0, 1, 2]
            }
        );

        // 取值列表是 [UP, DOWN]，序号却是 `Direction` 的声明序号 [1, 0]。
        let dripstone = BlockState(blocks::POINTED_DRIPSTONE);
        assert_eq!(
            def(dripstone, "vertical_direction").kind,
            PropertyKind::Enum { ordinals: &[1, 0] }
        );
    }

    #[test]
    fn property_values_parse_only_within_their_type() {
        let snow = BlockState(blocks::SNOW);
        let layers = def(snow, "layers");
        assert_eq!(parse_value(layers, "1"), Some(Value::Integer(1)));
        assert_eq!(parse_value(layers, "9"), None);

        let stairs = BlockState(blocks::OAK_STAIRS);
        let waterlogged = def(stairs, "waterlogged");
        assert_eq!(parse_value(waterlogged, "true"), Some(Value::Boolean(true)));
        assert_eq!(parse_value(waterlogged, "TRUE"), None);

        let log = BlockState(blocks::OAK_LOG);
        let axis = def(log, "axis");
        assert_eq!(parse_value(axis, "z"), Some(Value::Enum(2)));
        assert_eq!(parse_value(axis, "diagonal"), None);
    }
}
