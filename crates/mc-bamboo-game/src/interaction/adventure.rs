//! 冒险模式的 `can_break` 组件，官方 `ItemStack.canBreakBlockInAdventureMode`、
//! `AdventureModePredicate` 和 `BlockPredicate.matches(BlockInWorld)`。
//!
//! 谓词里的方块和方块状态属性都按官方匹配；方块实体 NBT 读不出来，见
//! [`predicate_matches`] 里的说明。

use std::cmp::Ordering;

use mc_bamboo_blocks::BlockState;
use mc_bamboo_data::{
    PropertyDef, block_id_of_state, block_tag, registries::data_component_type as kinds,
};
use mc_bamboo_protocol::{
    components::{
        ComponentPatch, ComponentValue,
        cursor::HolderSet,
        defaults,
        predicate::{BlockPredicate, PropertyMatcher, StateProperties, ValueMatcher},
    },
    item::ItemStack,
};

/// 手持物品在冒险模式下能不能挖这一格，官方 `ItemStack.canBreakBlockInAdventureMode`。
///
/// `can_break` 不存在、读不出来或者一条谓词都不匹配时都是 `false`。
pub(crate) fn can_break(held: &ItemStack, state: BlockState) -> bool {
    if held.is_empty() {
        return false;
    }
    let Ok(patch) = ComponentPatch::from_opaque(held) else {
        return false;
    };
    let Some(ComponentValue::CanBreak(predicates)) =
        defaults::resolve(held.item, &patch, kinds::CAN_BREAK)
    else {
        return false;
    };
    predicates
        .iter()
        .any(|predicate| predicate_matches(predicate, state))
}

/// 官方 `BlockPredicate.matches(BlockInWorld)`：方块、方块状态属性、方块实体 NBT。
///
/// 官方的 `BlockInWorld` 重载**不检查** `components` 字段（那是给 `matches(ServerLevel, pos)`
/// 用的），所以这里也不看。NBT 要求方块实体存在，Bamboo 还读不出方块实体的 NBT，
/// 带 `nbt` 的谓词一律按不匹配算。
fn predicate_matches(predicate: &BlockPredicate, state: BlockState) -> bool {
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
    let Some(actual) = state.property_text(&matcher.name) else {
        return false;
    };
    let Some(def) = state
        .block()
        .properties
        .iter()
        .find(|property| property.name == matcher.name)
    else {
        return false;
    };
    match &matcher.matcher {
        ValueMatcher::Exact(expected) => exact_matches(def, actual, expected),
        ValueMatcher::Ranged { min, max } => {
            // 当前值越过端点，或者端点解析不出来，都是不匹配。
            let inside = |bound: &str, too_far: Ordering| {
                compare(def, actual, bound).is_some_and(|order| order != too_far)
            };
            min.as_deref().is_none_or(|min| inside(min, Ordering::Less))
                && max
                    .as_deref()
                    .is_none_or(|max| inside(max, Ordering::Greater))
        }
    }
}

/// 官方 `ExactMatcher.match`：整数属性先把字符串按 `Integer.parseInt` 解析，
/// 其余类型按序列化名精确比较。
fn exact_matches(def: &PropertyDef, actual: &str, expected: &str) -> bool {
    if let Some((min, max)) = integer_bounds(def) {
        let Ok(expected) = expected.parse::<i32>() else {
            return false;
        };
        return (min..=max).contains(&expected) && actual.parse::<i32>() == Ok(expected);
    }
    actual == expected
}

/// 官方按属性值的类型比较。
///
/// 生成的数据里 `values` 是官方 `Property` 的取值列表，只有名字，类型从取值推：
/// 整数属性（`IntegerProperty`）比数值；布尔和枚举比顺序。布尔属性的取值列表是
/// `[true, false]`，但 `Boolean.compareTo` 里 `false < true`；`vertical_direction`
/// 官方的取值列表是 `[UP, DOWN]`，而 `Direction` 枚举里 `DOWN < UP`。这两处按下标
/// 反过来，其余的枚举取值列表就是声明顺序。
///
/// 端点不在属性的取值范围里时返回 `None`，和官方 `property.getValue` 返回空一样。
fn compare(def: &PropertyDef, actual: &str, bound: &str) -> Option<Ordering> {
    if let Some((min, max)) = integer_bounds(def) {
        let bound = bound
            .parse::<i32>()
            .ok()
            .filter(|bound| (min..=max).contains(bound))?;
        return Some(actual.parse::<i32>().ok()?.cmp(&bound));
    }
    Some(rank(def, actual)?.cmp(&rank(def, bound)?))
}

/// 整数属性的取值范围。官方的 `IntegerProperty` 取值是 `min..=max` 的连续整数，
/// 所以全部取值都能解析成整数，首尾就是端点。
fn integer_bounds(def: &PropertyDef) -> Option<(i32, i32)> {
    let mut values = def.values.iter().map(|value| value.parse::<i32>());
    let min = values.next()?.ok()?;
    let mut max = min;
    for value in values {
        max = value.ok()?;
    }
    Some((min, max))
}

/// 取值在官方 `compareTo` 里的位置。
fn rank(def: &PropertyDef, text: &str) -> Option<usize> {
    let index = def.values.iter().position(|value| *value == text)?;
    match def.values {
        ["true", "false"] | ["up", "down"] => Some(def.values.len() - 1 - index),
        _ => Some(index),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_bamboo_blocks::state::LAYERS;
    use mc_bamboo_data::{
        block_id_of_state, blocks, items, registries::data_component_type as kinds,
    };
    use mc_bamboo_nbt::Tag;

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

    /// 一个带 `can_break` 组件的物品。
    fn stack(predicates: Vec<BlockPredicate>) -> ItemStack {
        let mut value = Vec::new();
        ComponentValue::CanBreak(predicates).write(&mut value);
        ItemStack {
            item: items::STICK,
            count: 1,
            added: vec![(kinds::CAN_BREAK, value)],
            removed: Vec::new(),
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

    fn stone() -> BlockState {
        BlockState(blocks::STONE)
    }

    #[test]
    fn an_item_without_can_break_cannot_break() {
        assert!(!can_break(&ItemStack::empty(), stone()));
        assert!(!can_break(&ItemStack::new(items::STICK, 1), stone()));
        assert!(!can_break(
            &ItemStack::new(items::DIAMOND_PICKAXE, 1),
            stone()
        ));
    }

    #[test]
    fn a_matching_block_allows_breaking() {
        let stack = stack(vec![predicate(
            Some(block_ids(&[blocks::STONE])),
            None,
            None,
        )]);
        assert!(can_break(&stack, stone()));
        assert!(!can_break(&stack, BlockState(blocks::DIRT)));
    }

    #[test]
    fn a_tag_covers_every_block_in_it() {
        let stack = stack(vec![predicate(
            Some(HolderSet::Tag("minecraft:mineable/pickaxe".to_owned())),
            None,
            None,
        )]);
        assert!(can_break(&stack, stone()));
        assert!(!can_break(&stack, BlockState(blocks::WATER)));
    }

    #[test]
    fn predicates_are_any_of() {
        let stack = stack(vec![
            predicate(Some(block_ids(&[blocks::DIRT])), None, None),
            predicate(Some(block_ids(&[blocks::STONE])), None, None),
        ]);
        assert!(can_break(&stack, stone()));
    }

    #[test]
    fn a_predicate_without_blocks_matches_every_block() {
        let stack = stack(vec![predicate(None, None, None)]);
        assert!(can_break(&stack, stone()));
        assert!(can_break(&stack, BlockState(blocks::DIRT)));
    }

    #[test]
    fn exact_properties_must_match_the_block_state() {
        let log = BlockState(blocks::OAK_LOG);
        assert!(can_break(
            &stack(vec![predicate(None, Some(exact("axis", "y")), None)]),
            log
        ));
        assert!(!can_break(
            &stack(vec![predicate(None, Some(exact("axis", "x")), None)]),
            log
        ));
        // 属性名不存在，和官方 `getProperty` 返回空一样。
        assert!(!can_break(
            &stack(vec![predicate(None, Some(exact("nope", "y")), None)]),
            log
        ));
    }

    #[test]
    fn ranged_properties_compare_numbers() {
        let snow = BlockState(blocks::SNOW).with(LAYERS, 3);
        let matches = |min, max| {
            can_break(
                &stack(vec![predicate(
                    None,
                    Some(ranged("layers", min, max)),
                    None,
                )]),
                snow,
            )
        };
        assert!(matches(Some("2"), Some("4")));
        assert!(matches(Some("3"), Some("3")));
        assert!(matches(None, Some("3")));
        assert!(!matches(Some("4"), None));
        assert!(!matches(None, Some("2")));
        // 端点解析不出来时不匹配，官方也是。
        assert!(!matches(Some("not a number"), None));
    }

    #[test]
    fn ranged_properties_on_an_enum_use_the_declaration_order() {
        // `axis` 的值按 x、y、z 声明，默认是 y。
        let log = BlockState(blocks::OAK_LOG);
        let matches = |min, max| {
            can_break(
                &stack(vec![predicate(None, Some(ranged("axis", min, max)), None)]),
                log,
            )
        };
        assert!(matches(Some("x"), Some("z")));
        assert!(matches(Some("y"), None));
        assert!(!matches(Some("z"), None));
        assert!(!matches(None, Some("x")));
    }

    #[test]
    fn exact_integer_properties_parse_the_value_like_java() {
        let snow = BlockState(blocks::SNOW).with(LAYERS, 3);
        let matches = |expected| {
            can_break(
                &stack(vec![predicate(None, Some(exact("layers", expected)), None)]),
                snow,
            )
        };
        // `IntegerProperty.getValue` 用的是 `Integer.parseInt`，前导零和正号都行。
        assert!(matches("3"));
        assert!(matches("03"));
        assert!(matches("+3"));
        // 超出属性的取值范围，官方解析成空。
        assert!(!matches("30"));
        assert!(!matches("not a number"));
    }

    #[test]
    fn ranged_boolean_properties_order_false_before_true() {
        // 官方 `BooleanProperty` 的取值列表是 [true, false]，而 `false < true`。
        let stairs = BlockState(blocks::OAK_STAIRS);
        let matches = |min, max| {
            can_break(
                &stack(vec![predicate(
                    None,
                    Some(ranged("waterlogged", min, max)),
                    None,
                )]),
                stairs,
            )
        };
        assert!(matches(Some("false"), None));
        assert!(!matches(Some("true"), None));
        assert!(matches(None, Some("false")));
        assert!(matches(None, Some("true")));
    }

    #[test]
    fn ranged_vertical_direction_uses_the_enum_order() {
        // 官方的取值列表是 [UP, DOWN]，但 `Direction` 枚举里 `DOWN < UP`，默认是 up。
        let dripstone = BlockState(blocks::POINTED_DRIPSTONE);
        let matches = |min, max| {
            can_break(
                &stack(vec![predicate(
                    None,
                    Some(ranged("vertical_direction", min, max)),
                    None,
                )]),
                dripstone,
            )
        };
        assert!(matches(Some("down"), None));
        assert!(matches(Some("up"), Some("up")));
        assert!(!matches(None, Some("down")));
    }

    #[test]
    fn nbt_predicates_never_match_because_block_entities_cannot_be_read() {
        let stack = stack(vec![predicate(None, None, Some(Vec::new()))]);
        assert!(!can_break(&stack, stone()));
    }
}
