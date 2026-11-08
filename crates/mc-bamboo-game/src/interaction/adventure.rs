//! 冒险模式的 `can_break` 和 `can_place_on` 组件，官方 `ItemStack.canBreakBlockInAdventureMode`、
//! `canPlaceOnBlockInAdventureMode` 和 `AdventureModePredicate`。
//!
//! 谓词的匹配在 [`mc_bamboo_blocks::predicate`] 里，和 `can_break` 共用一套代码。

use mc_bamboo_blocks::{BlockState, predicate};
use mc_bamboo_data::registries::data_component_type as kinds;
use mc_bamboo_protocol::{
    components::{ComponentPatch, ComponentValue, defaults},
    item::ItemStack,
};

/// 手持物品在冒险模式下能不能挖这一格，官方 `ItemStack.canBreakBlockInAdventureMode`。
///
/// `can_break` 不存在、读不出来或者一条谓词都不匹配时都是 `false`。
pub(crate) fn can_break(held: &ItemStack, state: BlockState) -> bool {
    predicates_match(held, kinds::CAN_BREAK, state)
}

/// 手持物品在冒险模式下能不能用在这一格上，官方 `ItemStack.canPlaceOnBlockInAdventureMode`。
///
/// `can_place_on` 不存在、读不出来或者一条谓词都不匹配时都是 `false`。
pub(crate) fn can_place_on(held: &ItemStack, state: BlockState) -> bool {
    predicates_match(held, kinds::CAN_PLACE_ON, state)
}

/// 官方 `AdventureModePredicate.test`：有一条谓词匹配就算匹配。
fn predicates_match(held: &ItemStack, kind: i32, state: BlockState) -> bool {
    if held.is_empty() {
        return false;
    }
    let Ok(patch) = ComponentPatch::from_opaque(held) else {
        return false;
    };
    let Some(value) = defaults::resolve(held.item, &patch, kind) else {
        return false;
    };
    match value {
        ComponentValue::CanBreak(predicates) | ComponentValue::CanPlaceOn(predicates) => {
            predicate::any_matches(predicates, state)
        }
        _ => false,
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
    use mc_bamboo_protocol::{
        components::{
            ComponentValue,
            cursor::HolderSet,
            predicate::{BlockPredicate, PropertyMatcher, StateProperties, ValueMatcher},
        },
        item::ItemStack,
    };

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

    /// 一个带 `can_place_on` 组件的物品。
    fn placeable(predicates: Vec<BlockPredicate>) -> ItemStack {
        let mut value = Vec::new();
        ComponentValue::CanPlaceOn(predicates).write(&mut value);
        ItemStack {
            item: items::STICK,
            count: 1,
            added: vec![(kinds::CAN_PLACE_ON, value)],
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

    #[test]
    fn can_place_on_is_independent_of_can_break() {
        // `can_break` 的组件管不了放置，反过来也一样。
        let placeable = placeable(vec![predicate(
            Some(block_ids(&[blocks::STONE])),
            None,
            None,
        )]);
        assert!(can_place_on(&placeable, stone()));
        assert!(!can_break(&placeable, stone()));
        assert!(!can_place_on(&stack(Vec::new()), stone()));
        assert!(!can_place_on(&ItemStack::empty(), stone()));
        assert!(!can_place_on(&ItemStack::new(items::STICK, 1), stone()));
    }
}
