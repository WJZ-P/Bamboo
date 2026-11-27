//! 用官方 `Villager.updateTrades` 对拍 Bamboo 的交易生成。
//!
//! Java 一侧对每个职业、等级、品种和世界种子建一个村民，第一次 `getOffers()`
//! 时懒生成交易，再按 `MerchantOffers.CODEC` 编成 JSON 输出。这里用同一套参数
//! 跑 `mc-bamboo-loot::trade`，把结果编成同样的 JSON，逐项比较。

use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};

use mc_bamboo_data::villager as data;
use mc_bamboo_loot::{
    ExplorationMapRequest, ExplorationMapResult, LevelView, RandomSequences,
    trade::{
        TradeParams, TradeSet, add_offers_from_trade_set, add_offers_from_trade_set_with_level,
    },
};
use mc_bamboo_protocol::{
    components::{
        ComponentPatch, PatchEntry,
        archive::{Dyn, components, read},
        cursor::Registry,
    },
    item::ItemStack,
    merchant::{ItemCost, MerchantOffer, encode_offers},
};
use serde_json::{Map, Value};

use crate::oracle::collect_log;

const USAGE: &str = "usage: trade-oracle <java> [--only PREFIX] [--show N]";
const DEFAULT_SHOW: usize = 5;

pub fn run(mut command: Command, args: &[&str]) -> Result<(), String> {
    let mut forwarded = Vec::new();
    let mut show = DEFAULT_SHOW;
    let mut args = args.iter();
    while let Some(&arg) = args.next() {
        match arg {
            "--only" => {
                let prefix = args.next().ok_or(USAGE)?;
                forwarded.extend(["--only", prefix]);
            }
            "--show" => {
                show = args
                    .next()
                    .ok_or(USAGE)?
                    .parse()
                    .map_err(|_| USAGE.to_owned())?;
            }
            _ => return Err(USAGE.to_owned()),
        }
    }
    let mut child = command
        .args(&forwarded)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot run java: {error}"))?;
    let log = collect_log(&mut child)?;
    let stdout = child.stdout.take().ok_or("java has no stdout")?;
    let mut report = Report {
        show,
        ..Report::default()
    };
    let read = report.read_lines(BufReader::new(stdout));
    if read.is_err() {
        let _ = child.kill();
    }
    let status = child
        .wait()
        .map_err(|error| format!("cannot wait for java: {error}"))?;
    let log = log.recv().unwrap_or_default().join("\n");
    read.map_err(|error| format!("{error}\n{log}"))?;
    if !status.success() {
        return Err(format!("TradeOracle.java failed: {status}\n{log}"));
    }
    report.finish()
}

#[derive(Default)]
struct Report {
    cases: usize,
    mismatches: usize,
    show: usize,
    examples: Vec<String>,
}

impl Report {
    fn read_lines(&mut self, source: BufReader<impl Read>) -> Result<(), String> {
        for line in source.lines() {
            let line = line.map_err(|error| format!("cannot read from java: {error}"))?;
            if !line.starts_with('{') {
                continue;
            }
            let case: Value = serde_json::from_str(&line)
                .map_err(|error| format!("bad trade oracle line: {error}: {line}"))?;
            if case["kind"] == "summary" {
                continue;
            }
            self.cases += 1;
            if let Err(error) = self.compare(&case) {
                self.mismatches += 1;
                if self.examples.len() < self.show {
                    self.examples.push(error);
                }
            }
        }
        Ok(())
    }

    fn compare(&self, case: &Value) -> Result<(), String> {
        if case["kind"] == "wandering_trade" {
            return self.compare_wandering(case);
        }
        let profession = case["profession"].as_str().unwrap_or("?");
        let level: i32 = case["level"]
            .as_i64()
            .unwrap_or(0)
            .try_into()
            .map_err(|_| "bad level".to_owned())?;
        let variant = case["type"].as_str().unwrap_or("plains");
        let seed: i64 = case["seed"]
            .as_str()
            .unwrap_or("0")
            .parse()
            .map_err(|_| "bad seed".to_owned())?;
        let id = data::profession_id(&format!("minecraft:{profession}"))
            .ok_or_else(|| format!("unknown profession {profession}"))?;
        let set_name = data::profession(id)
            .ok_or_else(|| format!("no profession {id}"))?
            .trade_sets_by_level
            .get(level.saturating_sub(1) as usize)
            .copied()
            .unwrap_or("");
        if set_name.is_empty() {
            return Err(format!("{profession}/{level} has no trade set"));
        }
        let set = TradeSet::parse(set_name).map_err(|error| format!("{set_name}: {error}"))?;
        let params = TradeParams {
            world_seed: seed,
            origin: mc_bamboo_loot::Position::new(1.5, 64.0, 2.5),
            villager_type: Box::leak(format!("minecraft:{variant}").into_boxed_str()),
        };
        let sequences = RandomSequences::new(seed);
        let fallback = mc_bamboo_blocks::random::XoroshiroRandom::new(seed, seed ^ 0x9e37_79b9);
        let mut offers = Vec::new();
        // 探险地图场景：注入一个固定结构落点的假世界视图，和 Java 侧的
        // `FakeLevel.findNearestMapStructure` 对齐；其余场景仍走无世界的路径。
        // Explorer-map cases: inject a fake level view reporting the same fixed
        // structure position as Java's `FakeLevel.findNearestMapStructure`.
        let explorer = case["kind"] == "explorer_trade";
        let level_view = ExplorerLevel::default();
        let generated = if explorer {
            add_offers_from_trade_set_with_level(
                &set,
                &params,
                Some(&level_view),
                Some(&sequences),
                &fallback,
                &mut offers,
            )
        } else {
            add_offers_from_trade_set(&set, &params, Some(&sequences), &fallback, &mut offers)
        };
        generated.map_err(|error| format!("{profession}/{level}/{variant}/{seed}: {error}"))?;
        let encoded = encode_offers(
            &offers
                .iter()
                .map(to_merchant_offer)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| format!("{profession}/{level}: {error}"))?,
        )
        .map_err(|error| format!("{profession}/{level}: {error}"))?;
        let ours = dyn_to_json(&encoded);
        let expected = &case["offers"];
        if canonical(&ours) != canonical(expected) {
            return Err(format!(
                "{profession}/{level}/{variant}/{seed}\n  official: {expected}\n  bamboo:   {ours}"
            ));
        }
        Ok(())
    }

    /// `WanderingTrader.updateTrades`：buying → uncommon → common 三张集，
    /// 共用一条按世界种子建的 `RandomSequences`（名字不同，互不干扰）。
    ///
    /// `WanderingTrader.updateTrades`: buying → uncommon → common, all on the
    /// same world-seeded `RandomSequences` (distinct sequence names).
    fn compare_wandering(&self, case: &Value) -> Result<(), String> {
        let seed: i64 = case["seed"]
            .as_str()
            .unwrap_or("0")
            .parse()
            .map_err(|_| "bad seed".to_owned())?;
        let sequences = RandomSequences::new(seed);
        let fallback = mc_bamboo_blocks::random::XoroshiroRandom::new(seed, seed ^ 0x9e37_79b9);
        let params = TradeParams {
            world_seed: seed,
            origin: mc_bamboo_loot::Position::new(1.5, 64.0, 2.5),
            villager_type: mc_bamboo_data::villager::DEFAULT_TYPE,
        };
        let mut offers = Vec::new();
        for name in mc_bamboo_game::merchant::WANDERING_TRADE_SETS {
            let set = TradeSet::parse(name).map_err(|error| format!("{name}: {error}"))?;
            add_offers_from_trade_set(&set, &params, Some(&sequences), &fallback, &mut offers)
                .map_err(|error| format!("wandering_trader/{seed}/{name}: {error}"))?;
        }
        let encoded = encode_offers(
            &offers
                .iter()
                .map(to_merchant_offer)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| format!("wandering_trader/{seed}: {error}"))?,
        )
        .map_err(|error| format!("wandering_trader/{seed}: {error}"))?;
        let ours = dyn_to_json(&encoded);
        let expected = &case["offers"];
        if canonical(&ours) != canonical(expected) {
            return Err(format!(
                "wandering_trader/{seed}\n  official: {expected}\n  bamboo:   {ours}"
            ));
        }
        Ok(())
    }

    fn finish(&self) -> Result<(), String> {
        if self.mismatches > 0 {
            return Err(format!(
                "{} / {} trade cases differ:\n{}",
                self.mismatches,
                self.cases,
                self.examples.join("\n")
            ));
        }
        println!("trade-oracle: {} cases, 0 mismatches", self.cases);
        Ok(())
    }
}

/// Gson 把 `float` 写成最短的 float 十进制，serde_json 写的是转成 `f64` 后的
/// 最短十进制。两边都按 f32 的值规范化再比，数值不同仍然会被发现。
fn canonical(value: &Value) -> String {
    match value {
        Value::Number(number) => {
            if let Some(integer) = number.as_i64() {
                return integer.to_string();
            }
            if let Some(integer) = number.as_u64() {
                return integer.to_string();
            }
            number
                .as_f64()
                .map(|raw| (raw as f32).to_string())
                .unwrap_or_else(|| "null".to_owned())
        }
        Value::Bool(value) => value.to_string(),
        Value::String(value) => value.clone(),
        Value::Array(values) => format!(
            "[{}]",
            values.iter().map(canonical).collect::<Vec<_>>().join(",")
        ),
        Value::Object(entries) => {
            let mut fields: Vec<_> = entries.iter().collect();
            fields.sort_by(|a, b| a.0.cmp(b.0));
            format!(
                "{{{}}}",
                fields
                    .iter()
                    .map(|(name, value)| format!("{name}:{}", canonical(value)))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
        Value::Null => "null".to_owned(),
    }
}

/// 把求值的中性交易转成协议层的 `MerchantOffer`，组件按存档 codec 的 JSON 表示解码。
fn to_merchant_offer(
    offer: &mc_bamboo_loot::trade::GeneratedOffer,
) -> Result<MerchantOffer, String> {
    Ok(MerchantOffer {
        base_cost_a: to_cost(&offer.cost_a)?,
        cost_b: offer.cost_b.as_ref().map(to_cost).transpose()?,
        result: to_stack(&offer.result),
        uses: 0,
        max_uses: offer.max_uses,
        reward_exp: true,
        special_price_diff: 0,
        demand: 0,
        price_multiplier: offer.price_multiplier,
        xp: offer.xp,
    })
}

fn to_cost(price: &mc_bamboo_loot::trade::TradePrice) -> Result<ItemCost, String> {
    let mut components = Vec::new();
    for (name, value) in &price.components {
        let kind = read::name_id(Registry::DATA_COMPONENT_TYPE, name)
            .ok_or_else(|| format!("unknown component {name}"))?;
        let tag = json_to_tag(value).to_tag();
        let value =
            components::decode_value(kind, &tag, 0).map_err(|error| format!("{name}: {error}"))?;
        components.push(mc_bamboo_protocol::components::predicate::TypedComponent { kind, value });
    }
    Ok(ItemCost {
        item: price.item,
        count: price.count,
        components,
    })
}

fn to_stack(stack: &mc_bamboo_loot::LootStack) -> ItemStack {
    // 组件转成类型化补丁：和 `loot_stacks::loot_stack` 的路径一致，但这里
    // 解不开的组件直接忽略，方便在 oracle 里比较其余字段。
    let mut patch = ComponentPatch::new();
    for (name, value) in &stack.components {
        let Some(kind) = read::name_id(Registry::DATA_COMPONENT_TYPE, name) else {
            continue;
        };
        if let Ok(value) = components::decode_value(kind, &json_to_tag(value).to_tag(), 0) {
            patch.put(kind, PatchEntry::Set(value));
        }
    }
    let (added, removed) = patch.to_opaque();
    ItemStack {
        item: stack.item,
        count: stack.count,
        bundle_selected: None,
        added,
        removed,
    }
}

fn json_to_tag(value: &Value) -> Dyn {
    match value {
        Value::Null => Dyn::Byte(0),
        Value::Bool(value) => Dyn::Bool(*value),
        Value::Number(number) => {
            if let Some(value) = number.as_i64() {
                if let Ok(value) = i8::try_from(value) {
                    return Dyn::Byte(value);
                }
                if let Ok(value) = i16::try_from(value) {
                    return Dyn::Short(value);
                }
                if let Ok(value) = i32::try_from(value) {
                    return Dyn::Int(value);
                }
                return Dyn::Long(value);
            }
            if let Some(value) = number.as_f64() {
                let as_float = value as f32;
                return if f64::from(as_float) == value {
                    Dyn::Float(as_float)
                } else {
                    Dyn::Double(value)
                };
            }
            Dyn::Double(0.0)
        }
        Value::String(value) => Dyn::String(value.clone()),
        Value::Array(values) => Dyn::List(values.iter().map(json_to_tag).collect()),
        Value::Object(entries) => Dyn::Map(
            entries
                .iter()
                .map(|(name, value)| (name.clone(), json_to_tag(value)))
                .collect(),
        ),
    }
}

/// 官方 `NbtOps.convertTo(JsonOps, tag)`：整数按最小 NBT 类型写。
fn dyn_to_json(value: &Dyn) -> Value {
    match value {
        Dyn::Byte(value) => Value::from(*value),
        Dyn::Short(value) => Value::from(*value),
        Dyn::Int(value) => Value::from(*value),
        Dyn::Long(value) => Value::from(*value),
        Dyn::Float(value) => Value::from(*value),
        Dyn::Double(value) => Value::from(*value),
        Dyn::Bool(value) => Value::Bool(*value),
        Dyn::String(value) => Value::String(value.clone()),
        Dyn::List(values) => Value::Array(values.iter().map(dyn_to_json).collect()),
        Dyn::Map(entries) => Value::Object(
            entries
                .iter()
                .map(|(name, value)| (name.clone(), dyn_to_json(value)))
                .collect::<Map<String, Value>>(),
        ),
        Dyn::ByteArray(values) => Value::Array(values.iter().map(|v| Value::from(*v)).collect()),
        Dyn::IntArray(values) => Value::Array(values.iter().map(|v| Value::from(*v)).collect()),
        Dyn::LongArray(values) => Value::Array(values.iter().map(|v| Value::from(*v)).collect()),
    }
}

/// 探险地图场景的假世界：和 `TradeOracle.java` 的 `FakeLevel` 一样，结构搜索固定
/// 返回 `(1234, 0, -567)`，`getFreeMapId` 从 0 递增。
///
/// Fake world for explorer scenarios: like `TradeOracle.java`'s `FakeLevel`, the
/// structure search returns a fixed `(1234, 0, -567)` and map ids start at 0.
#[derive(Default)]
struct ExplorerLevel {
    next_map_id: std::cell::Cell<i32>,
}

impl LevelView for ExplorerLevel {
    fn is_loaded(&self, _x: i32, _y: i32, _z: i32) -> Option<bool> {
        Some(true)
    }
    fn block_state(&self, _x: i32, _y: i32, _z: i32) -> Option<mc_bamboo_blocks::BlockState> {
        Some(mc_bamboo_blocks::BlockState::AIR)
    }
    fn biome(&self, _x: i32, _y: i32, _z: i32) -> Option<i32> {
        Some(
            mc_bamboo_data::registries::biome::NAMES
                .iter()
                .position(|name| *name == "minecraft:plains")
                .unwrap_or(0) as i32,
        )
    }
    fn can_see_sky(&self, _x: i32, _y: i32, _z: i32) -> Option<bool> {
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
        _request: &ExplorationMapRequest<'_>,
    ) -> mc_bamboo_loot::Result<Option<ExplorationMapResult>> {
        let id = self.next_map_id.get();
        self.next_map_id.set(id + 1);
        Ok(Some(ExplorationMapResult {
            map_id: id,
            target_x: 1234,
            target_z: -567,
        }))
    }
}
