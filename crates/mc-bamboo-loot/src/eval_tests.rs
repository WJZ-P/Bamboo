//! 每种条件、函数和数值提供器都至少走一遍求值。
//!
//! Every condition, function, and number provider is evaluated at least once.

use mc_bamboo_blocks::BlockState;
use mc_bamboo_blocks::random::LegacyRandom;
use mc_bamboo_data::items;
use serde_json::{Value, json};

use crate::context::{
    DamageSourceView, EntityView, EquipmentSlot, LevelView, LootContext, LootParams, Position,
    entity_enchantment_level,
};
use crate::drops::{EntityDeathParams, entity_death_drops};
use crate::function::LootFunction;
use crate::names::{enchantment_id, entity_type_id, instrument_name};
use crate::number::{parse_float_provider, parse_int_provider, parse_level_based_value};
use crate::stack::LootStack;
use crate::{LootCondition, LootError, LootTable};

struct TestLevel;

impl LevelView for TestLevel {
    fn is_loaded(&self, _: i32, _: i32, _: i32) -> Option<bool> {
        Some(true)
    }

    fn block_state(&self, _: i32, _: i32, _: i32) -> Option<BlockState> {
        Some(BlockState::AIR)
    }

    fn biome(&self, _: i32, _: i32, _: i32) -> Option<i32> {
        Some(0)
    }

    fn can_see_sky(&self, _: i32, _: i32, _: i32) -> Option<bool> {
        Some(true)
    }

    fn is_raining(&self) -> Option<bool> {
        Some(false)
    }

    fn is_thundering(&self) -> Option<bool> {
        Some(false)
    }

    fn day_time(&self) -> Option<i64> {
        Some(0)
    }

    fn game_time(&self) -> Option<i64> {
        Some(1000)
    }

    fn dimension(&self) -> Option<&'static str> {
        Some("minecraft:overworld")
    }

    fn light(&self, _: i32, _: i32, _: i32) -> Option<i32> {
        Some(15)
    }

    fn clock_time(&self, clock: &str) -> Option<i64> {
        (clock == "minecraft:overworld").then_some(100)
    }
}

fn params() -> LootParams<'static> {
    LootParams::new(0.0)
}

fn context<'a>(params: &'a LootParams<'a>, random: &'a LegacyRandom) -> LootContext<'a, 'a, 'a> {
    LootContext::new(params, random)
}

fn level_params() -> LootParams<'static> {
    let level: &'static TestLevel = Box::leak(Box::new(TestLevel));
    LootParams {
        level: Some(level),
        origin: Some(Position::new(0.5, 1.5, 0.5)),
        ..LootParams::new(0.0)
    }
}

fn int_value(text: &str) -> i32 {
    let value: Value = serde_json::from_str(text).unwrap();
    let provider = parse_int_provider(&value, "test").unwrap();
    let stored = LootParams::new(0.0);
    let random = LegacyRandom::new(1);
    provider.get(&context(&stored, &random)).unwrap()
}

fn float_value(text: &str, level: i32) -> f32 {
    let value: Value = serde_json::from_str(text).unwrap();
    let provider = parse_float_provider(&value, "test").unwrap();
    let mut stored = LootParams::new(0.0);
    stored.enchantment_level = Some(level);
    let random = LegacyRandom::new(1);
    provider.get(&context(&stored, &random)).unwrap()
}

#[test]
fn block_loot_tables_are_sorted_for_binary_search() {
    let tables = mc_bamboo_data::loot::BLOCK_LOOT_TABLES;
    for pair in tables.windows(2) {
        assert!(
            pair[0].0 < pair[1].0,
            "{} is not before {}",
            pair[0].0,
            pair[1].0
        );
    }
    assert_eq!(
        mc_bamboo_data::loot::block_loot_table("minecraft:stone"),
        Some("minecraft:blocks/stone")
    );
    assert_eq!(
        mc_bamboo_data::loot::block_loot_table("minecraft:calcite"),
        Some("minecraft:blocks/calcite")
    );
}

#[test]
fn every_int_provider_kind_evaluates() {
    assert_eq!(int_value("4"), 4);
    assert_eq!(int_value(r#"{"type":"minecraft:constant","value":7}"#), 7);
    assert_eq!(
        int_value(r#"{"type":"minecraft:uniform","min":3,"max":3}"#),
        3
    );
    assert_eq!(
        int_value(r#"{"type":"minecraft:binomial","n":0,"p":1.0}"#),
        0
    );
    assert_eq!(
        int_value(
            r#"{"type":"minecraft:conditional","condition":{"type":"minecraft:random_chance","chance":0.0},"on_true":1,"on_false":2}"#
        ),
        2
    );
    assert_eq!(
        int_value(
            r#"{"type":"minecraft:number_dispatcher","cases":[{"condition":{"type":"minecraft:random_chance","chance":0.0},"value":1}],"default":6}"#
        ),
        6
    );
    assert_eq!(
        int_value(r#"{"type":"minecraft:weighted_list","distribution":[{"data":8,"weight":1}]}"#),
        8
    );
    assert_eq!(int_value(r#"{"type":"minecraft:add","inputs":[2,3]}"#), 5);
    assert_eq!(int_value(r#"{"type":"minecraft:avg","inputs":[2,4]}"#), 3);
    assert_eq!(int_value(r#"{"type":"minecraft:mul","inputs":[2,5]}"#), 10);
    assert_eq!(int_value(r#"{"type":"minecraft:min","inputs":[9,4]}"#), 4);
    assert_eq!(int_value(r#"{"type":"minecraft:max","inputs":[9,4]}"#), 9);
    assert_eq!(
        int_value(r#"{"type":"minecraft:div","left":8,"right":3}"#),
        2
    );
    assert_eq!(
        int_value(r#"{"type":"minecraft:mod","left":8,"right":3}"#),
        2
    );
    assert_eq!(int_value(r#"{"type":"minecraft:abs","input":-4}"#), 4);
    assert_eq!(int_value(r#"{"type":"minecraft:negate","input":4}"#), -4);
    assert_eq!(
        int_value(r#"{"type":"minecraft:from_float","input":3.9}"#),
        3
    );
    assert_eq!(
        int_value(r#"{"type":"minecraft:pow","base":2,"exponent":4}"#),
        16
    );
    assert_eq!(
        int_value(r#"{"type":"minecraft:sub","left":9,"right":4}"#),
        5
    );
    assert_eq!(
        int_value(r#"{"type":"minecraft:floor_div","left":-7,"right":3}"#),
        -3
    );
    assert_eq!(
        int_value(r#"{"type":"minecraft:floor_mod","left":-7,"right":3}"#),
        2
    );
    // 除以 0 是算术错误，官方吞成 0。
    // Division by zero is an arithmetic error. Official code turns it into 0.
    assert_eq!(
        int_value(r#"{"type":"minecraft:floor_div","left":1,"right":0}"#),
        0
    );
    for kind in ["score", "storage", "environment_attribute"] {
        let value = json!({"type": format!("minecraft:{kind}")});
        let provider = parse_int_provider(&value, "test").unwrap();
        let stored = LootParams::new(0.0);
        let random = LegacyRandom::new(1);
        let error = provider.get(&context(&stored, &random)).unwrap_err();
        assert!(matches!(error, LootError::Unsupported(_)), "{error}");
    }
}

#[test]
fn every_float_provider_kind_evaluates() {
    assert_eq!(float_value("1.5", 0), 1.5);
    assert_eq!(
        float_value(r#"{"type":"minecraft:constant","value":2.5}"#, 0),
        2.5
    );
    assert_eq!(
        float_value(r#"{"type":"minecraft:uniform","min":4.0,"max":4.0}"#, 0),
        4.0
    );
    assert_eq!(
        float_value(
            r#"{"type":"minecraft:conditional","condition":{"type":"minecraft:random_chance","chance":1.0},"on_true":3.0,"on_false":0.0}"#,
            0
        ),
        3.0
    );
    assert_eq!(
        float_value(
            r#"{"type":"minecraft:number_dispatcher","cases":[],"default":1.25}"#,
            0
        ),
        1.25
    );
    assert_eq!(
        float_value(
            r#"{"type":"minecraft:weighted_list","distribution":[{"data":6.0,"weight":1}]}"#,
            0
        ),
        6.0
    );
    assert_eq!(
        float_value(r#"{"type":"minecraft:add","inputs":[1.5,2.5]}"#, 0),
        4.0
    );
    assert_eq!(
        float_value(r#"{"type":"minecraft:avg","inputs":[1.0,3.0]}"#, 0),
        2.0
    );
    assert_eq!(
        float_value(r#"{"type":"minecraft:mul","inputs":[2.0,3.0]}"#, 0),
        6.0
    );
    assert_eq!(
        float_value(r#"{"type":"minecraft:min","inputs":[2.0,3.0]}"#, 0),
        2.0
    );
    assert_eq!(
        float_value(r#"{"type":"minecraft:max","inputs":[2.0,3.0]}"#, 0),
        3.0
    );
    assert_eq!(
        float_value(r#"{"type":"minecraft:length","inputs":[3.0,4.0]}"#, 0),
        5.0
    );
    assert_eq!(
        float_value(r#"{"type":"minecraft:sub","left":5.0,"right":1.5}"#, 0),
        3.5
    );
    assert_eq!(
        float_value(r#"{"type":"minecraft:div","left":8.0,"right":2.0}"#, 0),
        4.0
    );
    assert_eq!(
        float_value(r#"{"type":"minecraft:mod","left":5.0,"right":2.0}"#, 0),
        1.0
    );
    assert_eq!(
        float_value(r#"{"type":"minecraft:abs","input":-2.5}"#, 0),
        2.5
    );
    assert_eq!(
        float_value(r#"{"type":"minecraft:negate","input":2.5}"#, 0),
        -2.5
    );
    assert_eq!(
        float_value(r#"{"type":"minecraft:floor","input":2.9}"#, 0),
        2.0
    );
    assert_eq!(
        float_value(r#"{"type":"minecraft:ceil","input":2.1}"#, 0),
        3.0
    );
    assert_eq!(
        float_value(r#"{"type":"minecraft:round","input":2.4}"#, 0),
        2.0
    );
    assert_eq!(
        float_value(r#"{"type":"minecraft:truncate","input":-2.9}"#, 0),
        -2.0
    );
    assert_eq!(
        float_value(r#"{"type":"minecraft:sqrt","input":9.0}"#, 0),
        3.0
    );
    assert_eq!(
        float_value(r#"{"type":"minecraft:sin","input":0.0}"#, 0),
        0.0
    );
    assert_eq!(
        float_value(r#"{"type":"minecraft:cos","input":0.0}"#, 0),
        1.0
    );
    assert_eq!(
        float_value(r#"{"type":"minecraft:from_int","input":4}"#, 0),
        4.0
    );
    assert_eq!(
        float_value(r#"{"type":"minecraft:pow","base":2.0,"exponent":3.0}"#, 0),
        8.0
    );
    assert_eq!(
        float_value(
            r#"{"type":"minecraft:enchantment_level","amount":{"type":"minecraft:linear","base":1.0,"per_level_above_first":2.0}}"#,
            3
        ),
        5.0
    );
    // 除以 0 得到非有限值，官方吞成 0。
    // Division by zero is non-finite. Official code turns it into 0.
    assert_eq!(
        float_value(r#"{"type":"minecraft:div","left":1.0,"right":0.0}"#, 0),
        0.0
    );
    for kind in ["storage", "environment_attribute"] {
        let value = json!({"type": format!("minecraft:{kind}")});
        let provider = parse_float_provider(&value, "test").unwrap();
        let stored = LootParams::new(0.0);
        let random = LegacyRandom::new(1);
        let error = provider.get(&context(&stored, &random)).unwrap_err();
        assert!(matches!(error, LootError::Unsupported(_)), "{error}");
    }
}

#[test]
fn level_based_values_cover_every_formula() {
    let clamped = parse_level_based_value(
        &json!({"type":"minecraft:clamped","value":9.0,"min":1.0,"max":4.0}),
        "test",
        0,
    )
    .unwrap();
    assert_eq!(clamped.calculate(1), 4.0);
    let exponent = parse_level_based_value(
        &json!({"type":"minecraft:exponent","base":2.0,"power":3.0}),
        "test",
        0,
    )
    .unwrap();
    assert_eq!(exponent.calculate(1), 8.0);
    let fraction = parse_level_based_value(
        &json!({"type":"minecraft:fraction","numerator":9.0,"denominator":2.0}),
        "test",
        0,
    )
    .unwrap();
    assert_eq!(fraction.calculate(1), 4.5);
    let squared = parse_level_based_value(
        &json!({"type":"minecraft:levels_squared","added":1.0}),
        "test",
        0,
    )
    .unwrap();
    assert_eq!(squared.calculate(3), 10.0);
}

#[test]
fn composite_conditions_short_circuit() {
    let stored = params();
    let first =
        LootCondition::parse(&json!({"type":"minecraft:random_chance","chance":0.0})).unwrap();
    let both = LootCondition::parse(&json!({
        "type": "minecraft:all_of",
        "terms": [
            {"type": "minecraft:random_chance", "chance": 0.0},
            {"type": "minecraft:random_chance", "chance": 1.0}
        ]
    }))
    .unwrap();
    let left = LegacyRandom::new(5);
    let right = LegacyRandom::new(5);
    assert!(!first.test(&context(&stored, &left)).unwrap());
    assert!(!both.test(&context(&stored, &right)).unwrap());
    assert_eq!(left.seed(), right.seed());

    let any = LootCondition::parse(&json!({
        "type": "minecraft:any_of",
        "terms": [
            {"type": "minecraft:random_chance", "chance": 1.0},
            {"type": "minecraft:random_chance", "chance": 0.0}
        ]
    }))
    .unwrap();
    let only =
        LootCondition::parse(&json!({"type":"minecraft:random_chance","chance":1.0})).unwrap();
    let left = LegacyRandom::new(5);
    let right = LegacyRandom::new(5);
    assert!(only.test(&context(&stored, &left)).unwrap());
    assert!(any.test(&context(&stored, &right)).unwrap());
    assert_eq!(left.seed(), right.seed());

    let inverted = LootCondition::parse(&json!({
        "type": "minecraft:inverted",
        "term": {"type": "minecraft:random_chance", "chance": 0.0}
    }))
    .unwrap();
    let random = LegacyRandom::new(1);
    assert!(inverted.test(&context(&stored, &random)).unwrap());
}

#[test]
fn every_condition_kind_evaluates() {
    let mut stored = level_params();
    let random = LegacyRandom::new(2);

    let bonus = LootCondition::parse(&json!({
        "type": "minecraft:table_bonus",
        "enchantment": "minecraft:fortune",
        "chances": [1.0]
    }))
    .unwrap();
    assert!(bonus.test(&context(&stored, &random)).unwrap());

    stored.damage_source = Some(DamageSourceView {
        damage_type: 0,
        is_direct: true,
        ..DamageSourceView::default()
    });
    let damage = LootCondition::parse(&json!({
        "type": "minecraft:damage_source_properties",
        "predicate": {"is_direct": true}
    }))
    .unwrap();
    assert!(damage.test(&context(&stored, &random)).unwrap());

    stored.enchantment_active = Some(true);
    let active = LootCondition::parse(&json!({
        "type": "minecraft:enchantment_active_check",
        "active": true
    }))
    .unwrap();
    assert!(active.test(&context(&stored, &random)).unwrap());

    let scores = LootCondition::parse(&json!({
        "type": "minecraft:entity_scores",
        "entity": "this",
        "scores": {"dummy": 1}
    }))
    .unwrap();
    assert!(matches!(
        scores.test(&context(&stored, &random)),
        Err(LootError::Unsupported(_))
    ));

    let mut victim = EntityView::new(0);
    victim.is_on_fire = true;
    stored.this_entity = Some(victim);
    let properties = LootCondition::parse(&json!({
        "type": "minecraft:entity_properties",
        "entity": "this",
        "predicate": {"minecraft:flags": {"is_on_fire": true}}
    }))
    .unwrap();
    assert!(properties.test(&context(&stored, &random)).unwrap());

    assert!(
        !LootCondition::parse(&json!({"type":"minecraft:killed_by_player"}))
            .unwrap()
            .test(&context(&stored, &random))
            .unwrap()
    );
    stored.last_damage_player = Some(EntityView::new(0));
    assert!(
        LootCondition::parse(&json!({"type":"minecraft:killed_by_player"}))
            .unwrap()
            .test(&context(&stored, &random))
            .unwrap()
    );

    let chance = LootCondition::parse(&json!({
        "type": "minecraft:random_chance_with_enchanted_bonus",
        "enchantment": "minecraft:looting",
        "unenchanted_chance": 1.0,
        "enchanted_chance": 0.0
    }))
    .unwrap();
    assert!(chance.test(&context(&stored, &random)).unwrap());

    let boom = LootCondition::parse(&json!({"type":"minecraft:survives_explosion"})).unwrap();
    stored.explosion_radius = Some(1.0);
    let before = random.seed();
    assert!(boom.test(&context(&stored, &random)).unwrap());
    assert_ne!(before, random.seed());

    let location = LootCondition::parse(&json!({
        "type": "minecraft:location_check",
        "predicate": {"can_see_sky": true}
    }))
    .unwrap();
    assert!(location.test(&context(&stored, &random)).unwrap());

    stored.block_state = Some(BlockState(mc_bamboo_data::blocks::STONE));
    let block = LootCondition::parse(&json!({
        "type": "minecraft:match_block",
        "blocks": "minecraft:stone"
    }))
    .unwrap();
    assert!(block.test(&context(&stored, &random)).unwrap());

    stored.tool = Some(LootStack::new(items::SHEARS, 1));
    let tool = LootCondition::parse(&json!("minecraft:tool/can_shear")).unwrap();
    assert!(tool.test(&context(&stored, &random)).unwrap());

    let time = LootCondition::parse(&json!({
        "type": "minecraft:time_check",
        "clock": "minecraft:overworld",
        "value": {"min": 0, "max": 200}
    }))
    .unwrap();
    assert!(time.test(&context(&stored, &random)).unwrap());

    let ints = LootCondition::parse(&json!({
        "type": "minecraft:int_value_check",
        "value": 4,
        "test": {"min": 4, "max": 4}
    }))
    .unwrap();
    assert!(ints.test(&context(&stored, &random)).unwrap());

    let floats = LootCondition::parse(&json!({
        "type": "minecraft:float_value_check",
        "value": 0.5,
        "test": {"min": 0.0, "max": 1.0}
    }))
    .unwrap();
    assert!(floats.test(&context(&stored, &random)).unwrap());

    let weather = LootCondition::parse(&json!({
        "type": "minecraft:weather_check",
        "raining": false,
        "thundering": false
    }))
    .unwrap();
    assert!(weather.test(&context(&stored, &random)).unwrap());

    let attribute = LootCondition::parse(&json!({
        "type": "minecraft:environment_attribute_check",
        "environment": "minecraft:overworld",
        "attribute": "minecraft:game_time"
    }))
    .unwrap();
    assert!(matches!(
        attribute.test(&context(&stored, &random)),
        Err(LootError::Unsupported(_))
    ));
}

fn apply(text: Value, stack: LootStack, params: &LootParams<'_>) -> Result<LootStack, LootError> {
    let function = LootFunction::parse(&text)?;
    let random = LegacyRandom::new(3);
    function.apply(stack, &context(params, &random))
}

#[test]
fn every_function_kind_evaluates() {
    let mut stored = level_params();
    let stone = LootStack::new(items::STONE, 2);
    let counted = apply(
        json!({"type":"minecraft:set_count","count":5,"add":true}),
        stone.clone(),
        &stored,
    )
    .unwrap();
    assert_eq!(counted.count, 7);

    let replaced = apply(
        json!({"type":"minecraft:set_item","item":"minecraft:dirt"}),
        stone.clone(),
        &stored,
    )
    .unwrap();
    assert_eq!(replaced.item, items::DIRT);

    let patched = apply(
        json!({"type":"minecraft:set_components","components":{"minecraft:repair_cost":1}}),
        stone.clone(),
        &stored,
    )
    .unwrap();
    assert_eq!(
        patched
            .component("minecraft:repair_cost")
            .and_then(Value::as_i64),
        Some(1)
    );

    let fortune = enchantment_id("minecraft:fortune").unwrap();
    let mut tool = LootStack::new(items::DIAMOND_PICKAXE, 1);
    tool.enchant(fortune, 0);
    stored.tool = Some(tool);
    let bonus = apply(
        json!({
            "type": "minecraft:apply_bonus",
            "enchantment": "minecraft:fortune",
            "formula": "minecraft:ore_drops"
        }),
        stone.clone(),
        &stored,
    )
    .unwrap();
    assert_eq!(bonus.count, 2);
    let binomial = apply(
        json!({
            "type": "minecraft:apply_bonus",
            "enchantment": "minecraft:fortune",
            "formula": "minecraft:binomial_with_bonus_count",
            "parameters": {"extra": 0, "probability": 0.0}
        }),
        stone.clone(),
        &stored,
    )
    .unwrap();
    assert_eq!(binomial.count, 2);
    let uniform = apply(
        json!({
            "type": "minecraft:apply_bonus",
            "enchantment": "minecraft:fortune",
            "formula": "minecraft:uniform_bonus_count",
            "parameters": {"bonusMultiplier": 1}
        }),
        stone.clone(),
        &stored,
    )
    .unwrap();
    assert_eq!(uniform.count, 2);

    stored.block_entity = Some(crate::BlockEntityView {
        components: [("minecraft:custom_name".to_owned(), json!("Pot"))]
            .into_iter()
            .collect(),
    });
    let copied = apply(
        json!({"type":"minecraft:copy_components","source":"block_entity"}),
        stone.clone(),
        &stored,
    )
    .unwrap();
    assert_eq!(
        copied
            .component("minecraft:custom_name")
            .and_then(Value::as_str),
        Some("Pot")
    );
    let entity_copy = apply(
        json!({"type":"minecraft:copy_components","source":"this"}),
        stone.clone(),
        &stored,
    );
    assert!(matches!(entity_copy, Err(LootError::Unsupported(_))));

    let stairs = mc_bamboo_data::blocks::BLOCKS
        .iter()
        .find(|block| block.name == "minecraft:oak_stairs")
        .unwrap();
    let state = BlockState(stairs.default_state);
    let facing = state.property_text("facing").unwrap();
    stored.block_state = Some(state);
    let copied_state = apply(
        json!({"type":"minecraft:copy_state","block":"minecraft:oak_stairs","properties":["facing"]}),
        stone.clone(),
        &stored,
    )
    .unwrap();
    assert_eq!(
        copied_state
            .component("minecraft:block_state")
            .and_then(|value| value.get("facing"))
            .and_then(Value::as_str),
        Some(facing)
    );

    let discarded = apply(json!({"type":"minecraft:discard"}), stone.clone(), &stored).unwrap();
    assert!(discarded.is_empty());

    let book = apply(
        json!({"type":"minecraft:enchant_randomly","options":"minecraft:unbreaking"}),
        LootStack::new(items::BOOK, 1),
        &stored,
    )
    .unwrap();
    assert_eq!(book.item, items::ENCHANTED_BOOK);
    assert!(book.component("minecraft:stored_enchantments").is_some());

    let enchanted = apply(
        json!({"type":"minecraft:enchant_with_levels","levels":1}),
        LootStack::new(items::BOOK, 1),
        &stored,
    )
    .unwrap();
    assert_eq!(enchanted.item, items::ENCHANTED_BOOK);

    let looting = enchantment_id("minecraft:looting").unwrap();
    let mut sword = LootStack::new(items::DIAMOND_SWORD, 1);
    sword.enchant(looting, 2);
    let mut killer = EntityView::new(entity_type_id("minecraft:zombie").unwrap());
    killer.is_living = true;
    killer.equipment[EquipmentSlot::Mainhand.index()] = Some(sword);
    assert_eq!(entity_enchantment_level(&killer, looting), 2);
    stored.attacking_entity = Some(killer);
    let increased = apply(
        json!({
            "type": "minecraft:enchanted_count_increase",
            "enchantment": "minecraft:looting",
            "count": 1.0
        }),
        stone.clone(),
        &stored,
    )
    .unwrap();
    assert_eq!(increased.count, 4);

    stored.explosion_radius = None;
    let intact = apply(
        json!({"type":"minecraft:explosion_decay"}),
        stone.clone(),
        &stored,
    )
    .unwrap();
    assert_eq!(intact.count, 2);
    stored.explosion_radius = Some(1.0);
    let decayed = apply(
        json!({"type":"minecraft:explosion_decay"}),
        stone.clone(),
        &stored,
    )
    .unwrap();
    assert_eq!(decayed.count, 2);

    let filtered = apply(
        json!({
            "type": "minecraft:filtered",
            "item_filter": {"items": "minecraft:stone"},
            "on_pass": {"type": "minecraft:set_count", "count": 9}
        }),
        stone.clone(),
        &stored,
    )
    .unwrap();
    assert_eq!(filtered.count, 9);

    let smelted = apply(
        json!({"type":"minecraft:furnace_smelt"}),
        LootStack::new(items::RAW_IRON, 3),
        &stored,
    )
    .unwrap();
    assert_eq!(smelted.item, items::IRON_INGOT);
    assert_eq!(smelted.count, 3);

    let limited = apply(
        json!({"type":"minecraft:limit_count","limit":{"max":1}}),
        stone.clone(),
        &stored,
    )
    .unwrap();
    assert_eq!(limited.count, 1);

    let pick = LootStack::new(items::DIAMOND_PICKAXE, 1);
    let max_damage = pick.max_damage();
    let damaged = apply(
        json!({"type":"minecraft:set_damage","damage":0.0}),
        pick,
        &stored,
    )
    .unwrap();
    assert_eq!(damaged.damage_value(), max_damage);

    let named = apply(
        json!({"type":"minecraft:set_enchantments","enchantments":{"minecraft:unbreaking":1}}),
        LootStack::new(items::BOOK, 1),
        &stored,
    )
    .unwrap();
    assert_eq!(named.item, items::ENCHANTED_BOOK);

    let horn = instrument_name(0).unwrap();
    let instrument = apply(
        json!({"type":"minecraft:set_instrument","options": horn}),
        stone.clone(),
        &stored,
    )
    .unwrap();
    assert_eq!(
        instrument
            .component("minecraft:instrument")
            .and_then(Value::as_str),
        Some(horn)
    );

    let custom = apply(
        json!({"type":"minecraft:set_name","name":"Named","target":"item_name"}),
        stone.clone(),
        &stored,
    )
    .unwrap();
    assert_eq!(
        custom
            .component("minecraft:item_name")
            .and_then(Value::as_str),
        Some("Named")
    );
    let named_entity = apply(
        json!({"type":"minecraft:set_name","name":"Named","entity":"this"}),
        stone.clone(),
        &stored,
    );
    assert!(matches!(named_entity, Err(LootError::Unsupported(_))));

    let bottle = apply(
        json!({"type":"minecraft:set_ominous_bottle_amplifier","amplifier":2}),
        stone.clone(),
        &stored,
    )
    .unwrap();
    assert_eq!(
        bottle
            .component("minecraft:ominous_bottle_amplifier")
            .and_then(Value::as_i64),
        Some(2)
    );

    let potion = apply(
        json!({"type":"minecraft:set_potion","id":"minecraft:water"}),
        stone.clone(),
        &stored,
    )
    .unwrap();
    assert_eq!(
        potion
            .component("minecraft:potion_contents")
            .and_then(|value| value.get("potion"))
            .and_then(Value::as_str),
        Some("minecraft:water")
    );

    let stew = apply(
        json!({
            "type": "minecraft:set_stew_effect",
            "effects": [{"type": "minecraft:speed", "duration": 1}]
        }),
        LootStack::new(items::SUSPICIOUS_STEW, 1),
        &stored,
    )
    .unwrap();
    assert_eq!(
        stew.component("minecraft:suspicious_stew_effects")
            .and_then(Value::as_array)
            .and_then(|effects| effects.first())
            .and_then(|effect| effect.get("duration"))
            .and_then(Value::as_i64),
        Some(20)
    );

    let map = apply(
        json!({"type":"minecraft:exploration_map","destination":"minecraft:village"}),
        stone.clone(),
        &stored,
    );
    assert!(matches!(map, Err(LootError::Unsupported(_))));
    let lore = apply(
        json!({"type":"minecraft:set_lore","lore":[{"text":"line"}]}),
        stone.clone(),
        &stored,
    );
    assert!(matches!(lore, Err(LootError::Unsupported(_))));
    let named_ref = apply(json!("minecraft:set_count"), stone, &stored);
    assert!(matches!(named_ref, Err(LootError::Unsupported(_))));
}

#[test]
fn entry_kinds_follow_the_official_composition() {
    let stored = params();
    let random = LegacyRandom::new(1);
    let alternatives = LootTable::parse(&json!({
        "pools": [{
            "rolls": 1,
            "entries": [{
                "type": "minecraft:alternatives",
                "children": [
                    {"type": "minecraft:item", "name": "minecraft:stone"},
                    {"type": "minecraft:item", "name": "minecraft:dirt"}
                ]
            }]
        }]
    }))
    .unwrap();
    let dropped = alternatives.get_random_items(&stored, &random).unwrap();
    assert_eq!(dropped.len(), 1);
    assert_eq!(dropped[0].item, items::STONE);

    let sequence = LootTable::parse(&json!({
        "pools": [{
            "rolls": 1,
            "entries": [{
                "type": "minecraft:sequence",
                "children": [
                    {
                        "type": "minecraft:item",
                        "name": "minecraft:dirt",
                        "condition": {"type": "minecraft:random_chance", "chance": 0.0}
                    },
                    {"type": "minecraft:item", "name": "minecraft:stone"}
                ]
            }]
        }]
    }))
    .unwrap();
    let random = LegacyRandom::new(1);
    assert!(
        sequence
            .get_random_items(&stored, &random)
            .unwrap()
            .is_empty()
    );

    let group = LootTable::parse(&json!({
        "pools": [{
            "rolls": 1,
            "entries": [{
                "type": "minecraft:group",
                "children": [
                    {"type": "minecraft:item", "name": "minecraft:stone"},
                    {"type": "minecraft:item", "name": "minecraft:dirt"}
                ]
            }]
        }]
    }))
    .unwrap();
    let random = LegacyRandom::new(1);
    let dropped = group.get_random_items(&stored, &random).unwrap();
    assert_eq!(dropped.len(), 1);
    assert!(dropped[0].item == items::STONE || dropped[0].item == items::DIRT);

    let empty = LootTable::parse(&json!({
        "pools": [{"rolls": 1, "entries": [{"type": "minecraft:empty"}]}]
    }))
    .unwrap();
    let random = LegacyRandom::new(1);
    assert!(empty.get_random_items(&stored, &random).unwrap().is_empty());

    let dynamic = LootTable::parse(&json!({
        "pools": [{
            "rolls": 1,
            "entries": [{"type": "minecraft:dynamic", "name": "minecraft:contents"}]
        }]
    }))
    .unwrap();
    let random = LegacyRandom::new(1);
    assert!(
        dynamic
            .get_random_items(&stored, &random)
            .unwrap()
            .is_empty()
    );

    let slots = LootTable::parse(&json!({
        "pools": [{"rolls": 1, "entries": [{"type": "minecraft:slots"}]}]
    }))
    .unwrap();
    let random = LegacyRandom::new(1);
    assert!(matches!(
        slots.get_random_items(&stored, &random),
        Err(LootError::Unsupported(_))
    ));
}

#[test]
fn nonzero_loot_seed_matches_the_death_entry() {
    let mut entity = EntityView::new(entity_type_id("minecraft:zombie").unwrap());
    entity.is_living = true;
    entity.position = Some(Position::new(0.5, 1.0, 0.5));
    let level: &TestLevel = Box::leak(Box::new(TestLevel));
    let params = EntityDeathParams {
        entity,
        origin: Position::new(0.5, 1.0, 0.5),
        damage_source: DamageSourceView {
            damage_type: 0,
            is_direct: true,
            ..DamageSourceView::default()
        },
        attacking_entity: None,
        direct_attacking_entity: None,
        last_damage_player: None,
        luck: 0.0,
        level: Some(level),
    };
    let fallback = LegacyRandom::new(1);
    let from_entry =
        entity_death_drops("minecraft:blocks/stone", &params, 99, None, &fallback).unwrap();
    let random = LegacyRandom::new(99);
    let from_table = LootTable::from_name("minecraft:blocks/stone")
        .unwrap()
        .get_random_items(&params.loot_params(), &random)
        .unwrap();
    assert_eq!(from_entry, from_table);
}
