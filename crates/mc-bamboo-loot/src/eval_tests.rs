//! 每种条件、函数和数值提供器都至少走一遍求值。
//!
//! Every condition, function, and number provider is evaluated at least once.

use mc_bamboo_blocks::BlockState;
use mc_bamboo_blocks::random::LegacyRandom;
use mc_bamboo_data::items;
use serde_json::{Value, json};

use crate::context::{
    DamageSourceView, EntityView, EquipmentSlot, ExplorationMapRequest, ExplorationMapResult,
    LevelView, LootContext, LootParams, Position, entity_enchantment_level,
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

    fn score(&self, holder: &str, objective: &str) -> Option<Option<i32>> {
        Some((holder == "Kitten" && objective == "dummy").then_some(1))
    }

    fn command_storage(&self, storage: &str) -> Option<mc_bamboo_nbt::Tag> {
        use mc_bamboo_nbt::Tag;
        (storage == "minecraft:test")
            .then(|| Tag::Compound(vec![("Counter".to_owned(), Tag::Int(42))]))
    }

    fn environment_attribute(&self, name: &str, _origin: Option<(i32, i32, i32)>) -> Option<Value> {
        match name {
            "minecraft:test/int" => Some(json!(7)),
            "minecraft:test/float" => Some(json!(2.5)),
            _ => None,
        }
    }

    fn loot_table_tag(&self, name: &str) -> Option<Vec<String>> {
        match name {
            "minecraft:custom_pack_tag" => Some(Vec::new()),
            "minecraft:desert_archaeology" => Some(vec![
                "minecraft:archaeology/desert_pyramid".to_owned(),
                "minecraft:archaeology/desert_well".to_owned(),
            ]),
            _ => None,
        }
    }

    fn item_modifier(&self, name: &str) -> Option<std::sync::Arc<LootFunction>> {
        static LOOP: std::sync::OnceLock<std::sync::Arc<LootFunction>> = std::sync::OnceLock::new();
        (name == "minecraft:recursive_modifier").then(|| {
            LOOP.get_or_init(|| {
                std::sync::Arc::new(
                    LootFunction::parse(&json!([
                        {"type":"minecraft:set_count","count":2,"add":true},
                        "minecraft:recursive_modifier"
                    ]))
                    .unwrap(),
                )
            })
            .clone()
        })
    }

    fn slot_items(&self, source: &str) -> Option<Vec<LootStack>> {
        (source.contains("contents")).then(|| vec![LootStack::new(items::DIAMOND, 3)])
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

fn level_int_value(text: &str) -> Result<i32, LootError> {
    let value: Value = serde_json::from_str(text).unwrap();
    let provider = parse_int_provider(&value, "test").unwrap();
    let params = level_params();
    let random = LegacyRandom::new(1);
    provider.get(&context(&params, &random))
}

fn level_float_value(text: &str) -> Result<f32, LootError> {
    let value: Value = serde_json::from_str(text).unwrap();
    let provider = parse_float_provider(&value, "test").unwrap();
    let params = level_params();
    let random = LegacyRandom::new(1);
    provider.get(&context(&params, &random))
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
    // `score`：固定名字查到分数；缺实体或分数时按官方走 fallback。
    // `score`: a fixed name resolves to the score; a missing holder/score takes fallback.
    assert_eq!(
        level_int_value(
            r#"{"type":"minecraft:score","target":{"type":"minecraft:fixed","name":"Kitten"},"score":"dummy","fallback":9}"#
        )
        .unwrap(),
        1
    );
    assert_eq!(
        level_int_value(
            r#"{"type":"minecraft:score","target":"this","score":"dummy","fallback":9}"#
        )
        .unwrap(),
        9
    );
    // `storage`：路径有数值取它，没有取 fallback。
    // `storage`: a numeric path returns its value; a missing one takes fallback.
    assert_eq!(
        level_int_value(
            r#"{"type":"minecraft:storage","storage":"minecraft:test","path":"Counter","fallback":5}"#
        )
        .unwrap(),
        42
    );
    assert_eq!(
        level_int_value(
            r#"{"type":"minecraft:storage","storage":"minecraft:test","path":"Missing","fallback":5}"#
        )
        .unwrap(),
        5
    );
    // `environment_attribute`：世界侧提供值就转整数，否则报不支持。
    // `environment_attribute`: converts a supplied value to int, else reports unsupported.
    assert_eq!(
        level_int_value(
            r#"{"type":"minecraft:environment_attribute","attribute":"minecraft:test/int"}"#
        )
        .unwrap(),
        7
    );
    assert!(matches!(
        level_int_value(
            r#"{"type":"minecraft:environment_attribute","attribute":"minecraft:test/unknown"}"#
        ),
        Err(LootError::Unsupported(_))
    ));
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
    // `storage` 浮点提供器：路径有数值取它，没有取 fallback。
    // Float `storage` provider: a numeric path returns its value, else fallback.
    assert_eq!(
        level_float_value(
            r#"{"type":"minecraft:storage","storage":"minecraft:test","path":"Counter","fallback":0.5}"#
        )
        .unwrap(),
        42.0
    );
    assert_eq!(
        level_float_value(
            r#"{"type":"minecraft:storage","storage":"minecraft:test","path":"Missing","fallback":0.5}"#
        )
        .unwrap(),
        0.5
    );
    // `environment_attribute` 浮点提供器：提供值就转浮点，否则报不支持。
    // Float `environment_attribute`: converts a supplied value, else unsupported.
    assert_eq!(
        level_float_value(
            r#"{"type":"minecraft:environment_attribute","attribute":"minecraft:test/float"}"#
        )
        .unwrap(),
        2.5
    );
    assert!(matches!(
        level_float_value(
            r#"{"type":"minecraft:environment_attribute","attribute":"minecraft:test/unknown"}"#
        ),
        Err(LootError::Unsupported(_))
    ));
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
    // 官方 `EntityHasScoreCondition.test`：没有实体直接 false。
    // Official `EntityHasScoreCondition.test`: no entity is false.
    assert!(!scores.test(&context(&stored, &random)).unwrap());
    let mut scored = level_params();
    scored.this_entity = Some(EntityView::new(0));
    assert!(
        matches!(
            scores.test(&context(&scored, &random)),
            Err(LootError::Unsupported(_))
        ),
        "没有积分板名字时不能猜"
    );
    let mut holder = EntityView::new(0);
    holder.scoreboard_name = Some("Kitten".to_owned());
    scored.this_entity = Some(holder.clone());
    assert!(scores.test(&context(&scored, &random)).unwrap());
    holder.scoreboard_name = Some("Mochi".to_owned());
    scored.this_entity = Some(holder);
    assert!(
        !scores.test(&context(&scored, &random)).unwrap(),
        "没有分数判 false"
    );

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

    // `environment_attribute_check`：世界侧提供属性值时做 JSON 相等比较。
    // `environment_attribute_check`: compares the supplied attribute value as JSON.
    let matched = LootCondition::parse(&json!({
        "type": "minecraft:environment_attribute_check",
        "attribute": "minecraft:test/int",
        "value": 7
    }))
    .unwrap();
    let level = level_params();
    assert!(matched.test(&context(&level, &random)).unwrap());
    let differed = LootCondition::parse(&json!({
        "type": "minecraft:environment_attribute_check",
        "attribute": "minecraft:test/int",
        "value": 8
    }))
    .unwrap();
    assert!(!differed.test(&context(&level, &random)).unwrap());
    // 世界侧没接这个属性时不能编默认值，报不支持。
    // No wired-in attribute means unsupported, not a fabricated default.
    let missing = LootCondition::parse(&json!({
        "type": "minecraft:environment_attribute_check",
        "attribute": "minecraft:test/unknown",
        "value": 0
    }))
    .unwrap();
    assert!(matches!(
        missing.test(&context(&level, &random)),
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
    // 实体来源：从 `this_entity` 的组件映射复制（没有实体时原样退回）。
    // Entity source: copies from `this_entity`'s components (no-op without an entity).
    let mut entity_source = level_params();
    let mut holder = EntityView::new(0);
    holder
        .components
        .insert("minecraft:custom_name".to_owned(), json!("Mob"));
    entity_source.this_entity = Some(holder);
    let entity_copy = apply(
        json!({"type":"minecraft:copy_components","source":"this"}),
        stone.clone(),
        &entity_source,
    )
    .unwrap();
    assert_eq!(
        entity_copy
            .component("minecraft:custom_name")
            .and_then(Value::as_str),
        Some("Mob")
    );
    // `tool` 来源：工具物品堆上的组件。
    let mut tool_source = level_params();
    let mut tool = LootStack::new(items::DIAMOND_PICKAXE, 1);
    tool.set_component("minecraft:custom_name", json!("Pick"));
    tool_source.tool = Some(tool);
    let tool_copy = apply(
        json!({"type":"minecraft:copy_components","source":"tool"}),
        stone.clone(),
        &tool_source,
    )
    .unwrap();
    assert_eq!(
        tool_copy
            .component("minecraft:custom_name")
            .and_then(Value::as_str),
        Some("Pick")
    );

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
    // 假世界里找不到结构，官方原样退回输入物品堆。
    assert_eq!(map.unwrap(), stone);
    let lore = apply(
        json!({"type":"minecraft:set_lore","lore":[{"text":"line"}],"mode":"append"}),
        stone.clone(),
        &stored,
    )
    .unwrap();
    assert_eq!(
        lore.component("minecraft:lore").and_then(Value::as_array),
        Some(&vec![json!({"text":"line"})])
    );
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

    // `slots`：没有世界视图解析 slot_source 时报不支持；接了视图就产出物品。
    // `slots`: unsupported without a level view that resolves the slot source; with
    // one wired in it emits the slot items.
    let slots = LootTable::parse(&json!({
        "pools": [{"rolls": 1, "entries": [{"type": "minecraft:slots", "slot_source": {"type": "minecraft:contents"}}]}]
    }))
    .unwrap();
    let random = LegacyRandom::new(1);
    assert!(matches!(
        slots.get_random_items(&stored, &random),
        Err(LootError::Unsupported(_))
    ));
    let level = level_params();
    let from_slots = slots.get_random_items(&level, &random).unwrap();
    assert_eq!(from_slots.len(), 1);
    assert_eq!(from_slots[0].item, items::DIAMOND);
    assert_eq!(from_slots[0].count, 3);

    // `#` 战利品表标签：官方 26.3 数据包没有这类标签，空标签展开成空列表，
    // 不再报不支持，选中后不产出物品。
    // `#` loot table tag: vanilla 26.3 has none; an empty tag expands to nothing
    // instead of reporting unsupported, and produces no items.
    let tagged = LootTable::parse(&json!({
        "pools": [{"rolls": 1, "entries": [{"type": "minecraft:loot_table", "value": "#minecraft:custom_pack_tag"}]}]
    }))
    .unwrap();
    assert!(tagged.get_random_items(&level, &random).unwrap().is_empty());
    for expand in [false, true] {
        let table = LootTable::parse(&json!({
            "pools": [{"rolls": 1, "entries": [{"type": "minecraft:loot_table",
                "value": "#minecraft:desert_archaeology", "expand": expand}]}]
        }))
        .unwrap();
        let products = table
            .get_random_items(&level, &LegacyRandom::new(42))
            .unwrap();
        assert_eq!(products.len(), if expand { 1 } else { 2 });
    }
    let stack = apply(
        json!("minecraft:recursive_modifier"),
        LootStack::new(items::STONE, 1),
        &level,
    )
    .unwrap();
    assert_eq!(stack.count, 3);
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

/// 掠夺者的战利品表只有队长掉不祥之瓶；`type_specific/raider` 的 `is_captain` 读
/// `Raider.isCaptain()`，`has_raid` 缺省为假。
///
/// The pillager table drops the ominous bottle only for a captain: the
/// `type_specific/raider` predicate reads `Raider.isCaptain()` and defaults `has_raid`
/// to false.
#[test]
fn pillager_captain_drops_the_ominous_bottle() {
    let level: &TestLevel = Box::leak(Box::new(TestLevel));
    let drops_for = |captain: bool| {
        let mut entity = EntityView::new(entity_type_id("minecraft:pillager").unwrap());
        entity.is_living = true;
        entity.is_raider = true;
        entity.is_captain = captain;
        entity.position = Some(Position::new(0.5, 1.0, 0.5));
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
        entity_death_drops(
            "minecraft:entities/pillager",
            &params,
            7,
            None,
            &LegacyRandom::new(1),
        )
        .unwrap()
    };
    let captain = drops_for(true);
    assert_eq!(captain.len(), 1);
    assert_eq!(
        (captain[0].item, captain[0].count),
        (items::OMINOUS_BOTTLE, 1)
    );
    assert!(drops_for(false).is_empty());
}

/// 世界侧返回结构落点时，`exploration_map` 写出的组件和官方一致；返回 `None`
/// （假世界）时原样退回，后面的 `filtered` 会把没有 `map_id` 的地图丢掉。
///
/// When the world side returns a structure, `exploration_map` writes the same
/// components as vanilla; `None` (fake world) returns the input unchanged and a
/// following `filtered` discards the map with no `map_id`.
#[test]
fn exploration_map_writes_vanilla_components() {
    struct ExplorerLevel(Option<ExplorationMapResult>);

    impl LevelView for ExplorerLevel {
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
            Some(0)
        }
        fn dimension(&self) -> Option<&'static str> {
            Some("minecraft:overworld")
        }
        fn exploration_map(
            &self,
            request: &ExplorationMapRequest<'_>,
        ) -> crate::Result<Option<ExplorationMapResult>> {
            assert_eq!(request.destination, "#minecraft:on_treasure_maps");
            assert_eq!(request.decoration, "minecraft:red_x");
            assert_eq!(request.zoom, 1);
            assert_eq!(request.search_radius, 50);
            assert!(!request.skip_existing_chunks);
            assert_eq!(request.origin, (0, 1, 0));
            Ok(self.0.clone())
        }
    }

    let map = LootStack::new(items::BURIED_TREASURE_MAP, 1);
    let table = json!([
        {
            "type": "minecraft:exploration_map",
            "destination": "#minecraft:on_treasure_maps",
            "decoration": "minecraft:red_x",
            "skip_existing_chunks": false,
            "zoom": 1
        },
        {
            "type": "minecraft:filtered",
            "item_filter": {"predicates": {"minecraft:map_id": {}}},
            "on_fail": {"type": "minecraft:discard"}
        }
    ]);

    let found = ExplorerLevel(Some(ExplorationMapResult {
        map_id: 7,
        target_x: 123,
        target_z: -456,
    }));
    let params = LootParams {
        level: Some(&found),
        origin: Some(Position::new(0.5, 1.5, 0.5)),
        ..LootParams::new(0.0)
    };
    let result = apply(table.clone(), map.clone(), &params).unwrap();
    assert_eq!(result.item, items::BURIED_TREASURE_MAP);
    assert_eq!(result.count, 1);
    assert_eq!(result.component("minecraft:map_id"), Some(&json!(7)));
    assert_eq!(
        result.component("minecraft:map_decorations"),
        Some(&json!({
            "+": {
                "type": "minecraft:red_x",
                "x": 123.0,
                "z": -456.0,
                "rotation": 180.0
            }
        }))
    );

    let missing = ExplorerLevel(None);
    let params = LootParams {
        level: Some(&missing),
        origin: Some(Position::new(0.5, 1.5, 0.5)),
        ..LootParams::new(0.0)
    };
    let discarded = apply(table, map, &params).unwrap();
    assert!(
        discarded.is_empty(),
        "filtered discards the map with no map_id"
    );

    // 缺省值照官方 codec：decoration=mansion、zoom=2、radius=50、skip=true。
    // Vanilla codec defaults: decoration=mansion, zoom=2, radius=50, skip=true.
    let defaults = LootFunction::parse(&json!({
        "type": "minecraft:exploration_map",
        "destination": "#minecraft:on_woodland_mansion_maps"
    }))
    .unwrap();
    let level = TestLevel;
    let params = LootParams {
        level: Some(&level),
        origin: Some(Position::new(0.5, 1.5, 0.5)),
        ..LootParams::new(0.0)
    };
    let random = LegacyRandom::new(3);
    let unchanged = defaults
        .apply(LootStack::new(items::STONE, 1), &context(&params, &random))
        .unwrap();
    assert_eq!(unchanged, LootStack::new(items::STONE, 1));

    // 坏 zoom 报 Invalid，不悄悄截断。
    // A zoom outside a byte is Invalid instead of being silently truncated.
    assert!(matches!(
        LootFunction::parse(&json!({
            "type": "minecraft:exploration_map",
            "destination": "minecraft:mansion",
            "zoom": 999
        })),
        Err(LootError::Invalid(_))
    ));
}

/// 每个新做的组件函数都走一遍求值，检查写出的组件值。
///
/// Every newly implemented component function is evaluated and its component checked.
#[test]
fn extended_functions_evaluate() {
    let mut params = level_params();
    let stone = LootStack::new(items::STONE, 1);

    // `set_custom_data`：SNBT 复合标签并进 custom_data。
    let custom = apply(
        json!({"type":"minecraft:set_custom_data","tag":"{foo:1b,bar:\"x\"}"}),
        stone.clone(),
        &params,
    )
    .unwrap();
    let data = custom.component("minecraft:custom_data").unwrap();
    assert_eq!(data.get("foo").and_then(Value::as_i64), Some(1));
    assert_eq!(data.get("bar").and_then(Value::as_str), Some("x"));

    // `set_attributes`：先取 slot 再算 amount，按 id 覆盖。
    let attrs = apply(
        json!({"type":"minecraft:set_attributes","modifiers":[
            {"id":"minecraft:x","attribute":"minecraft:generic.max_health","operation":"add_value","amount":2.5,"slot":["mainhand"]}
        ]}),
        stone.clone(),
        &params,
    )
    .unwrap();
    let modifiers = attrs
        .component("minecraft:attribute_modifiers")
        .and_then(Value::as_array)
        .unwrap();
    assert_eq!(modifiers.len(), 1);
    assert_eq!(
        modifiers[0].get("amount").and_then(Value::as_f64),
        Some(2.5)
    );
    assert_eq!(
        modifiers[0].get("slot").and_then(Value::as_str),
        Some("mainhand")
    );

    // `set_loot_table`：container_loot 记表名和种子。
    let loot = apply(
        json!({"type":"minecraft:set_loot_table","loot_table_id":"minecraft:chests/simple_dungeon","seed":7}),
        stone.clone(),
        &params,
    )
    .unwrap();
    let container_loot = loot.component("minecraft:container_loot").unwrap();
    assert_eq!(
        container_loot.get("loot_table").and_then(Value::as_str),
        Some("minecraft:chests/simple_dungeon")
    );
    assert_eq!(container_loot.get("seed").and_then(Value::as_i64), Some(7));

    // `set_lore`：带实体解析时按官方报不支持。
    assert!(matches!(
        apply(
            json!({"type":"minecraft:set_lore","lore":[],"mode":"append","entity":"this"}),
            stone.clone(),
            &params,
        ),
        Err(LootError::Unsupported(_))
    ));

    // `set_banner_pattern`：append=false 直接替换 layers。
    let banner = apply(
        json!({"type":"minecraft:set_banner_pattern","patterns":[{"pattern":"minecraft:stripe_bottom","color":"red"}],"append":false}),
        stone.clone(),
        &params,
    )
    .unwrap();
    assert_eq!(
        banner
            .component("minecraft:banner_patterns")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(1)
    );

    // `set_fireworks`：flight_duration 和 explosions 列表。
    let fireworks = apply(
        json!({"type":"minecraft:set_fireworks","flight_duration":3,"explosions":{"mode":"append","values":[{"shape":"small_ball"}]}}),
        stone.clone(),
        &params,
    )
    .unwrap();
    let fireworks = fireworks.component("minecraft:fireworks").unwrap();
    assert_eq!(
        fireworks.get("flight_duration").and_then(Value::as_i64),
        Some(3)
    );
    assert_eq!(
        fireworks
            .get("explosions")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(1)
    );

    // `set_firework_explosion`：缺省字段补默认。
    let explosion = apply(
        json!({"type":"minecraft:set_firework_explosion","shape":"large_ball","colors":[1,2],"trail":true}),
        stone.clone(),
        &params,
    )
    .unwrap();
    let explosion = explosion.component("minecraft:firework_explosion").unwrap();
    assert_eq!(
        explosion.get("shape").and_then(Value::as_str),
        Some("large_ball")
    );
    assert_eq!(
        explosion.get("has_trail").and_then(Value::as_bool),
        Some(true)
    );
    assert!(
        !explosion
            .get("has_twinkle")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    );

    // `set_book_cover`：written_book_content 的封面字段。
    let book = apply(
        json!({"type":"minecraft:set_book_cover","title":"Hi","author":"Me","generation":2}),
        stone.clone(),
        &params,
    )
    .unwrap();
    let book = book.component("minecraft:written_book_content").unwrap();
    assert_eq!(book.get("author").and_then(Value::as_str), Some("Me"));
    assert_eq!(book.get("generation").and_then(Value::as_i64), Some(2));

    // `set_written_book_pages` / `set_writable_book_pages`：ListOperation 追加。
    let written = apply(
        json!({"type":"minecraft:set_written_book_pages","pages":["a","b"],"mode":"append"}),
        stone.clone(),
        &params,
    )
    .unwrap();
    assert_eq!(
        written
            .component("minecraft:written_book_content")
            .and_then(|value| value.get("pages"))
            .and_then(Value::as_array),
        Some(&vec![json!({"raw":"a"}), json!({"raw":"b"})])
    );
    let writable = apply(
        json!({"type":"minecraft:set_writable_book_pages","pages":["x"],"mode":"replace_all"}),
        stone.clone(),
        &params,
    )
    .unwrap();
    assert_eq!(
        writable
            .component("minecraft:writable_book_content")
            .and_then(|value| value.get("pages"))
            .and_then(Value::as_array),
        Some(&vec![json!({"raw":"x"})])
    );

    // `toggle_tooltips`：shown=false 记进 hidden_components，排序去重。
    let toggled = apply(
        json!({"type":"minecraft:toggle_tooltips","toggles":{"minecraft:lore":false,"minecraft:damage":true}}),
        stone.clone(),
        &params,
    )
    .unwrap();
    assert_eq!(
        toggled
            .component("minecraft:tooltip_display")
            .and_then(|value| value.get("hidden_components"))
            .and_then(Value::as_array),
        Some(&vec![json!("minecraft:lore")])
    );

    // `set_custom_model_data`：四个列表按 StandAlone 操作写，color/float 过提供器。
    let model = apply(
        json!({"type":"minecraft:set_custom_model_data","strings":{"mode":"append","values":["tag"]},"colors":{"mode":"append","values":[123]}}),
        stone.clone(),
        &params,
    )
    .unwrap();
    let model = model.component("minecraft:custom_model_data").unwrap();
    assert_eq!(
        model.get("strings").and_then(Value::as_array),
        Some(&vec![json!("tag")])
    );
    assert_eq!(
        model.get("colors").and_then(Value::as_array),
        Some(&vec![json!(123)])
    );

    // `set_contents`：展开条目并把物品写进 container。
    let contents = apply(
        json!({"type":"minecraft:set_contents","component":"minecraft:container","entries":[{"type":"minecraft:item","name":"minecraft:dirt"}]}),
        stone.clone(),
        &params,
    )
    .unwrap();
    let container_items = contents
        .component("minecraft:container")
        .and_then(Value::as_array)
        .unwrap();
    assert_eq!(container_items.len(), 1);
    assert_eq!(
        container_items[0]["item"]["id"].as_str(),
        Some("minecraft:dirt")
    );

    // `modify_contents`：对容器里每个非空物品应用 modifier。
    let mut container = stone.clone();
    container.set_component(
        "minecraft:container",
        json!([{"slot":0,"item":{"id":"minecraft:dirt","count":2}}]),
    );
    let modified = apply(
        json!({"type":"minecraft:modify_contents","component":"minecraft:container","modifier":{"type":"minecraft:set_count","count":5}}),
        container,
        &params,
    )
    .unwrap();
    let modified_items = modified
        .component("minecraft:container")
        .and_then(Value::as_array)
        .unwrap();
    assert_eq!(modified_items[0]["item"]["count"].as_i64(), Some(5));

    // `copy_name`：方块实体的 custom_name 写到物品。
    params.block_entity = Some(crate::BlockEntityView {
        components: [("minecraft:custom_name".to_owned(), json!("Pot"))]
            .into_iter()
            .collect(),
    });
    let copied = apply(
        json!({"type":"minecraft:copy_name","source":"block_entity"}),
        stone.clone(),
        &params,
    )
    .unwrap();
    assert_eq!(
        copied
            .component("minecraft:custom_name")
            .and_then(Value::as_str),
        Some("Pot")
    );

    // `fill_player_head`：玩家头 + 实体 profile 写 profile 组件。
    let mut head_params = level_params();
    let mut donor = EntityView::new(mc_bamboo_data::registries::entity_type::PLAYER);
    donor
        .components
        .insert("minecraft:profile".to_owned(), json!({"name": "Steve"}));
    head_params.this_entity = Some(donor);
    let head = apply(
        json!({"type":"minecraft:fill_player_head","entity":"this"}),
        LootStack::new(items::PLAYER_HEAD, 1),
        &head_params,
    )
    .unwrap();
    assert_eq!(
        head.component("minecraft:profile")
            .and_then(|value| value.get("name"))
            .and_then(Value::as_str),
        Some("Steve")
    );

    // `copy_custom_data`：实体 NBT 的 replace op 写进 custom_data。
    let mut nbt_params = level_params();
    let mut holder = EntityView::new(0);
    holder.nbt = Some(mc_bamboo_nbt::Tag::Compound(vec![(
        "value".to_owned(),
        mc_bamboo_nbt::Tag::Int(42),
    )]));
    nbt_params.this_entity = Some(holder);
    let copied = apply(
        json!({"type":"minecraft:copy_custom_data","source":{"type":"minecraft:context","target":"this"},"ops":[{"source":"value","target":"out","op":"replace"}]}),
        stone.clone(),
        &nbt_params,
    )
    .unwrap();
    assert_eq!(
        copied
            .component("minecraft:custom_data")
            .and_then(|value| value.get("out"))
            .and_then(Value::as_i64),
        Some(42)
    );
}
