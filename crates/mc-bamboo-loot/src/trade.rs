//! 官方 `VillagerTrade` / `TradeSet` 的数据包解析和求值。
//!
//! 数据原文在 `mc-bamboo-data`：一个 `trade_set` 指向标签，标签展开成按数据包
//! 顺序排列的 `villager_trade` 条目。求值对应 `VillagerTrade.getOffer(LootContext)`
//! 和 `AbstractVillager.addOffersFromTradeSet`：按 `random_sequence` 取一条随机流，
//! 不重复挑选时从列表里随机删条目，生成失败（谓词不通过、物品被丢弃、价格为零）
//! 就继续抽下一条。
//!
//! 产出的 [`GeneratedOffer`] 用 JSON 形态的组件，物品堆转成网络包里的字节是
//! `mc-bamboo-game::loot_stacks::loot_stack` 的事，避免这个 crate 依赖协议层。

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock};

use mc_bamboo_data::villager as data;
use serde_json::Value;

use crate::condition::LootCondition;
use crate::context::{LootContext, LootParams, Position};
use crate::function::LootFunction;
use crate::json::{field, i32_value, object, opt_bool, opt_string, required};
use crate::names::{Registry, item_id, item_name};
use crate::number::{FloatProvider, IntProvider, parse_float_provider, parse_int_provider};
use crate::random_sequence::RandomSequences;
use crate::stack::{ADDITIONAL_TRADE_COST, Components, LootStack, STORED_ENCHANTMENTS};
use crate::{LootError, Result};

/// 一张交易求值出来的价格项：物品、数量、必须精确相等的组件（官方 `ItemCost`）。
#[derive(Debug, Clone, PartialEq)]
pub struct TradePrice {
    pub item: i32,
    pub count: i32,
    /// `DataComponentExactPredicate`：组件名 → 官方 codec 的 JSON 表示。
    pub components: BTreeMap<String, Value>,
}

/// 一张交易求值出来的结果，组件用 JSON 表示。
#[derive(Debug, Clone, PartialEq)]
pub struct GeneratedOffer {
    pub cost_a: TradePrice,
    pub cost_b: Option<TradePrice>,
    pub result: LootStack,
    pub max_uses: i32,
    pub xp: i32,
    pub price_multiplier: f32,
}

/// 官方 `TradeCost`：`id` / `count`（`ContextIntProvider`）/ `components`。
#[derive(Debug, Clone)]
struct TradeCost {
    item: i32,
    count: IntProvider,
    components: BTreeMap<String, Value>,
}

impl TradeCost {
    fn parse(value: &Value, what: &str) -> Result<Self> {
        let map = object(value, what)?;
        let item = {
            let name = required(map, "id", what)?.as_str().ok_or_else(|| {
                LootError::Invalid(format!("{what}.id must be a resource location"))
            })?;
            item_id(name)
                .ok_or_else(|| LootError::Invalid(format!("unknown item `{name}` in {what}")))?
        };
        let count = match field(map, "count") {
            Some(value) => parse_int_provider(value, &format!("{what}.count"))?,
            None => IntProvider::Constant(1),
        };
        let components = match field(map, "components") {
            Some(value) => object(value, &format!("{what}.components"))?
                .iter()
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect(),
            None => BTreeMap::new(),
        };
        Ok(Self {
            item,
            count,
            components,
        })
    }

    /// `TradeCost.toItemCost(context, extra)`：数量加附加价格后夹到物品最大堆叠数。
    fn to_price(&self, context: &LootContext<'_, '_, '_>, extra: i32) -> Result<TradePrice> {
        let wanted = self.count.get(context)?;
        let count = wanted
            .saturating_add(extra)
            .clamp(0, LootStack::new(self.item, 1).max_stack_size());
        Ok(TradePrice {
            item: self.item,
            count,
            components: self.components.clone(),
        })
    }
}

/// 官方 `ItemStackTemplate`，只需 `id` / `count` / 组件补丁。
#[derive(Debug, Clone)]
struct StackTemplate {
    stack: LootStack,
}

impl StackTemplate {
    fn parse(value: &Value, what: &str) -> Result<Self> {
        let map = object(value, what)?;
        let name = required(map, "id", what)?
            .as_str()
            .ok_or_else(|| LootError::Invalid(format!("{what}.id must be a resource location")))?;
        let item = item_id(name)
            .ok_or_else(|| LootError::Invalid(format!("unknown item `{name}` in {what}")))?;
        let count = match field(map, "count") {
            Some(value) => i32_value(value, &format!("{what}.count"))?,
            None => 1,
        };
        let mut stack = LootStack::new(item, count);
        if let Some(value) = field(map, "components") {
            for (name, value) in object(value, &format!("{what}.components"))? {
                if let Some(name) = name.strip_prefix('!') {
                    stack.remove_component(name);
                } else if value.is_null() {
                    stack.remove_component(name);
                } else {
                    stack.set_component(name, value.clone());
                }
            }
        }
        Ok(Self { stack })
    }
}

/// 官方 `VillagerTrade`。
#[derive(Debug)]
pub struct VillagerTrade {
    wants: TradeCost,
    additional_wants: Option<TradeCost>,
    gives: StackTemplate,
    max_uses: IntProvider,
    xp: IntProvider,
    price_multiplier: FloatProvider,
    merchant_predicate: Option<LootCondition>,
    given_item_modifier: Option<LootFunction>,
    /// 附魔注册表 ID：结果书里有其中任意一个附魔时附加价格翻倍。
    double_price_enchantments: Option<Vec<i32>>,
}

impl VillagerTrade {
    /// 解析一条内置交易（数据包里的 JSON 原文）。
    pub fn parse(value: &Value) -> Result<Self> {
        let map = object(value, "villager_trade")?;
        let wants = TradeCost::parse(required(map, "wants", "villager_trade")?, "wants")?;
        let additional_wants = field(map, "additional_wants")
            .map(|value| TradeCost::parse(value, "additional_wants"))
            .transpose()?;
        let gives = StackTemplate::parse(required(map, "gives", "villager_trade")?, "gives")?;
        let max_uses = field(map, "max_uses")
            .map(|value| parse_int_provider(value, "max_uses"))
            .transpose()?
            .unwrap_or(IntProvider::Constant(4));
        let xp = field(map, "xp")
            .map(|value| parse_int_provider(value, "xp"))
            .transpose()?
            .unwrap_or(IntProvider::Constant(1));
        let price_multiplier = field(map, "reputation_discount")
            .map(|value| parse_float_provider(value, "reputation_discount"))
            .transpose()?
            .unwrap_or(FloatProvider::Constant(0.0));
        let merchant_predicate = field(map, "merchant_predicate")
            .map(LootCondition::parse)
            .transpose()?;
        let given_item_modifier = field(map, "given_item_modifier")
            .map(LootFunction::parse)
            .transpose()?;
        let double_price_enchantments = field(map, "double_trade_price_enchantments")
            .map(|value| {
                crate::names::holder_set(value, Registry::Enchantment, "double_trade_price")
            })
            .transpose()?;
        Ok(Self {
            wants,
            additional_wants,
            gives,
            max_uses,
            xp,
            price_multiplier,
            merchant_predicate,
            given_item_modifier,
            double_price_enchantments,
        })
    }

    /// `VillagerTrade.getOffer(LootContext)`。谓词不通过或物品被丢弃时返回 `None`。
    pub fn get_offer(&self, context: &LootContext<'_, '_, '_>) -> Result<Option<GeneratedOffer>> {
        if let Some(condition) = &self.merchant_predicate
            && !condition.test(context)?
        {
            return Ok(None);
        }
        let mut result = self.gives.stack.clone();
        if let Some(modifier) = &self.given_item_modifier {
            result = modifier.apply(result, context)?;
            if result.is_empty() {
                return Ok(None);
            }
        }
        let mut extra: i32 = 0;
        if let Some(cost) = result.component(ADDITIONAL_TRADE_COST).cloned() {
            result.remove_component(ADDITIONAL_TRADE_COST);
            extra = extra.saturating_add(cost.as_i64().unwrap_or(0) as i32);
        }
        if let Some(enchantments) = &self.double_price_enchantments {
            let stored = result
                .component(STORED_ENCHANTMENTS)
                .and_then(Value::as_object);
            let doubles = stored.is_some_and(|levels| {
                levels.keys().any(|name| {
                    crate::names::enchantment_id(name).is_some_and(|id| enchantments.contains(&id))
                })
            });
            if doubles {
                extra = extra.saturating_mul(2);
            }
        }
        let cost_a = self.wants.to_price(context, extra)?;
        if cost_a.count < 1 {
            return Ok(None);
        }
        let cost_b = self
            .additional_wants
            .as_ref()
            .map(|cost| cost.to_price(context, 0))
            .transpose()?;
        if cost_b.as_ref().is_some_and(|cost| cost.count < 1) {
            return Ok(None);
        }
        Ok(Some(GeneratedOffer {
            cost_a,
            cost_b,
            result,
            max_uses: self.max_uses.get(context)?.max(1),
            xp: self.xp.get(context)?.max(0),
            price_multiplier: self.price_multiplier.get(context)?.max(0.0),
        }))
    }
}

/// 官方 `TradeSet`。
#[derive(Debug, Clone)]
pub struct TradeSet {
    /// 标签展开后的交易名，顺序照官方数据包。
    pub entries: Vec<String>,
    amount: IntProvider,
    allow_duplicates: bool,
    pub random_sequence: Option<String>,
}

impl TradeSet {
    /// 从 `trade_set` 的原文解析，标签展开表由生成器导出。
    pub fn parse(name: &str) -> Result<Self> {
        let raw = data::trade_set(name)
            .ok_or_else(|| LootError::Invalid(format!("unknown trade set `{name}`")))?;
        let value: Value = serde_json::from_str(raw)
            .map_err(|error| LootError::Invalid(format!("trade set `{name}`: {error}")))?;
        let map = object(&value, name)?;
        let entries = data::trade_set_entries(name)
            .ok_or_else(|| LootError::Invalid(format!("trade set `{name}` has no tag expansion")))?
            .iter()
            .map(|entry| (*entry).to_owned())
            .collect();
        let amount = parse_int_provider(required(map, "amount", name)?, "amount")?;
        let allow_duplicates = opt_bool(map, "allow_duplicates", name, false)?;
        let random_sequence = opt_string(map, "random_sequence", name)?.map(str::to_owned);
        Ok(Self {
            entries,
            amount,
            allow_duplicates,
            random_sequence,
        })
    }

    /// `TradeSet.calculateNumberOfTrades`。
    pub fn amount(&self, context: &LootContext<'_, '_, '_>) -> Result<i32> {
        self.amount.get(context)
    }
}

fn trade_cache() -> &'static Mutex<BTreeMap<String, Arc<VillagerTrade>>> {
    static CACHE: OnceLock<Mutex<BTreeMap<String, Arc<VillagerTrade>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// 按名字取一条解析好的交易。解析结果按名字缓存，数据包原文仍是唯一来源。
pub fn trade(name: &str) -> Result<Arc<VillagerTrade>> {
    if let Some(cached) = trade_cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(name)
    {
        return Ok(Arc::clone(cached));
    }
    let raw = data::trade(name)
        .ok_or_else(|| LootError::Invalid(format!("unknown villager trade `{name}`")))?;
    let value: Value = serde_json::from_str(raw)
        .map_err(|error| LootError::Invalid(format!("villager trade `{name}`: {error}")))?;
    let parsed = Arc::new(VillagerTrade::parse(&value)?);
    trade_cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(name.to_owned(), Arc::clone(&parsed));
    Ok(parsed)
}

/// 一次交易生成的世界参数。`villager_type` 是官方 `villager/variant` 组件的值，
/// `origin` 是村民坐标，附加价格组件开关固定为真（见 `addOffersFromTradeSet`）。
#[derive(Debug, Clone, Copy)]
pub struct TradeParams {
    pub world_seed: i64,
    pub origin: Position,
    pub villager_type: &'static str,
}

/// 生成一张 `trade_set` 的交易，追加到 `offers`。
///
/// `sequences` 是 `Region.loot_sequences` 那条按名字共享的随机流；`fallback` 只在
/// 交易集没写 `random_sequence` 时用，和官方 `LootContext.Builder.create` 的兜底一致。
pub fn add_offers_from_trade_set(
    set: &TradeSet,
    params: &TradeParams,
    sequences: Option<&RandomSequences>,
    fallback: &dyn mc_bamboo_blocks::random::RandomSource,
    offers: &mut Vec<GeneratedOffer>,
) -> Result<()> {
    let loot_params = trade_loot_params(params);
    let cached = set
        .random_sequence
        .as_deref()
        .and_then(|key| sequences.map(|sequences| sequences.get(key)));
    let source: &dyn mc_bamboo_blocks::random::RandomSource = match &cached {
        Some(source) => source.as_ref(),
        None => fallback,
    };
    let context = LootContext::new(&loot_params, source);
    let amount = set.amount(&context)?.max(0) as usize;
    let mut remaining: Vec<&str> = set.entries.iter().map(String::as_str).collect();
    let mut added = 0;
    while added < amount && !remaining.is_empty() {
        let index = source.next_int(remaining.len() as i32);
        if index < 0 || index as usize >= remaining.len() {
            return Err(LootError::Invalid("trade index out of range".to_owned()));
        }
        let name = remaining[index as usize];
        let offer = trade(name)?.get_offer(&context)?;
        if set.allow_duplicates {
            match offer {
                Some(offer) => {
                    offers.push(offer);
                    added += 1;
                }
                None => {
                    remaining.remove(index as usize);
                }
            }
        } else {
            remaining.remove(index as usize);
            if let Some(offer) = offer {
                offers.push(offer);
                added += 1;
            }
        }
    }
    Ok(())
}

/// `AbstractVillager.addOffersFromTradeSet` 里 `LootParams` 的四个参数。
fn trade_loot_params(params: &TradeParams) -> LootParams<'static> {
    let mut entity = crate::context::EntityView::new(0);
    entity.is_living = true;
    entity.position = Some(params.origin);
    entity.components.insert(
        "minecraft:villager/variant".to_owned(),
        Value::String(params.villager_type.to_owned()),
    );
    LootParams {
        origin: Some(params.origin),
        this_entity: Some(entity),
        additional_cost_component_allowed: Some(true),
        ..LootParams::default()
    }
}

/// 给对拍和存档用的可读名字：解析失败时回退成注册表 ID。
pub fn price_name(price: &TradePrice) -> String {
    item_name(price.item).unwrap_or("?").to_owned()
}

/// 把组件 JSON 表转成 `loot_stack` 要的补丁形态。
pub fn components_of(components: &BTreeMap<String, Value>) -> Components {
    components.clone()
}

/// 解析并求值一条交易的测试入口：随机源由调用方给，方便逐条对拍。
pub fn evaluate_trade(
    name: &str,
    params: &TradeParams,
    source: &dyn mc_bamboo_blocks::random::RandomSource,
) -> Result<Option<GeneratedOffer>> {
    let loot_params = trade_loot_params(params);
    let context = LootContext::new(&loot_params, source);
    trade(name)?.get_offer(&context)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_bamboo_blocks::random::XoroshiroRandom;

    #[test]
    fn every_builtin_trade_parses() {
        let mut count = 0;
        for (name, raw) in mc_bamboo_data::villager::VILLAGER_TRADES {
            let value: Value = serde_json::from_str(raw).unwrap();
            VillagerTrade::parse(&value).unwrap_or_else(|error| panic!("{name}: {error}"));
            count += 1;
        }
        assert_eq!(count, 391);
    }

    #[test]
    fn every_builtin_trade_set_parses_and_generates() {
        let params = TradeParams {
            world_seed: 0,
            origin: Position::new(0.5, 64.0, 0.5),
            villager_type: "minecraft:plains",
        };
        let sequences = RandomSequences::new(params.world_seed);
        let fallback = XoroshiroRandom::new(1234, 5678);
        let mut total = 0;
        for (name, _) in mc_bamboo_data::villager::TRADE_SETS {
            let set = TradeSet::parse(name).unwrap_or_else(|error| panic!("{name}: {error}"));
            let mut offers = Vec::new();
            add_offers_from_trade_set(&set, &params, Some(&sequences), &fallback, &mut offers)
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            assert!(!offers.is_empty(), "{name} generated no offers");
            total += offers.len();
        }
        assert!(total > 100, "generated only {total} offers");
    }
}
