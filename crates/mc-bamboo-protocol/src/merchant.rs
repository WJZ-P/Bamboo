//! 村民交易的存档和网络格式：官方 `ItemCost`、`MerchantOffer` 和 `MerchantOffers`。
//!
//! 存档键名、顺序和默认值照官方 `CODEC`；网络格式照官方 `STREAM_CODEC`。

use mc_bamboo_nbt::Tag;

use crate::codec::{write_bool, write_f32, write_i32, write_varint};
use crate::components::archive::dynamic::Dyn;
use crate::components::archive::read::{self, Error, Result};
use crate::components::archive::slots;
use crate::components::archive::write::{Fields, holder_name};
use crate::components::archive::{decode_exact_components, encode_exact};
use crate::components::cursor::Registry;
use crate::components::predicate::TypedComponent;
use crate::item::ItemStack;

/// 存档里组件的最大嵌套深度。
const DEPTH: u32 = 512;

/// 官方 `ItemCost`：要的物品、数量和必须精确匹配的组件。
#[derive(Debug, Clone, PartialEq)]
pub struct ItemCost {
    /// `minecraft:item` 注册表 ID。
    pub item: i32,
    pub count: i32,
    /// 官方 `DataComponentExactPredicate`。
    pub components: Vec<TypedComponent>,
}

impl ItemCost {
    /// 没有组件要求的价格。
    pub const fn new(item: i32, count: i32) -> Self {
        Self {
            item,
            count,
            components: Vec::new(),
        }
    }

    /// 官方 `CODEC`：`id` / `count`（永远写）/ `components`（空时省略）。
    pub fn encode(&self) -> Result<Dyn> {
        let components = if self.components.is_empty() {
            None
        } else {
            Some(encode_exact(&self.components, DEPTH)?)
        };
        Ok(Fields::new()
            .put("id", holder_name(Registry::ITEM, self.item)?)
            .always("count", Dyn::Int(self.count))
            .optional("components", components)
            .build())
    }

    /// 官方 `CODEC` 的解码。`count` 缺省为 1，必须为正。
    pub fn decode(tag: &Tag) -> Result<Self> {
        let entries = read::compound(tag)?;
        let item = read::registry_id(Registry::ITEM, read::required(entries, "id")?)?;
        let count = match read::field(entries, "count") {
            Some(tag) => read::int(tag)?,
            None => 1,
        };
        if count < 1 {
            return Err(Error::new(format!("value must be positive: {count}")));
        }
        let components = match read::field(entries, "components") {
            Some(tag) => decode_exact_components(tag, DEPTH)?,
            None => Vec::new(),
        };
        Ok(Self {
            item,
            count,
            components,
        })
    }

    /// 官方 `STREAM_CODEC`：物品 ID、varint 数量、精确组件列表。
    pub fn write(&self, out: &mut Vec<u8>) {
        write_varint(out, self.item);
        write_varint(out, self.count);
        write_varint(out, self.components.len() as i32);
        for component in &self.components {
            TypedComponent::write(out, component);
        }
    }
}

/// 官方 `MerchantOffer` 的全部持久状态。
#[derive(Debug, Clone, PartialEq)]
pub struct MerchantOffer {
    pub base_cost_a: ItemCost,
    pub cost_b: Option<ItemCost>,
    pub result: ItemStack,
    pub uses: i32,
    pub max_uses: i32,
    pub reward_exp: bool,
    pub special_price_diff: i32,
    pub demand: i32,
    pub price_multiplier: f32,
    pub xp: i32,
}

impl MerchantOffer {
    /// 官方 `CODEC`，键序 buy / buyB / sell / uses / maxUses / rewardExp /
    /// specialPrice / demand / priceMultiplier / xp。后七个等于默认值时省略。
    pub fn encode(&self) -> Result<Dyn> {
        let buy_b = match &self.cost_b {
            Some(cost) => Some(cost.encode()?),
            None => None,
        };
        Ok(Fields::new()
            .put("buy", self.base_cost_a.encode()?)
            .optional("buyB", buy_b)
            .put("sell", Dyn::from_tag(&slots::encode_item(&self.result)?))
            .default("uses", Dyn::Int(self.uses), &Dyn::Int(0))
            .default("maxUses", Dyn::Int(self.max_uses), &Dyn::Int(4))
            .default("rewardExp", Dyn::Bool(self.reward_exp), &Dyn::Bool(true))
            .default(
                "specialPrice",
                Dyn::Int(self.special_price_diff),
                &Dyn::Int(0),
            )
            .default("demand", Dyn::Int(self.demand), &Dyn::Int(0))
            .default(
                "priceMultiplier",
                Dyn::Float(self.price_multiplier),
                &Dyn::Float(0.0),
            )
            .default("xp", Dyn::Int(self.xp), &Dyn::Int(1))
            .build())
    }

    /// 官方 `CODEC` 的解码。`lenientOptionalFieldOf` 的字段解不开时取默认值。
    pub fn decode(tag: &Tag) -> Result<Self> {
        let entries = read::compound(tag)?;
        let base_cost_a = ItemCost::decode(read::required(entries, "buy")?)?;
        let cost_b = read::field(entries, "buyB").and_then(|tag| ItemCost::decode(tag).ok());
        let result = slots::decode_item(read::required(entries, "sell")?)?;
        let int = |name: &str, default: i32| {
            read::field(entries, name)
                .and_then(|tag| read::int(tag).ok())
                .unwrap_or(default)
        };
        let reward_exp = read::field(entries, "rewardExp")
            .and_then(|tag| read::bool_value(tag).ok())
            .unwrap_or(true);
        let price_multiplier = read::field(entries, "priceMultiplier")
            .and_then(|tag| read::float(tag).ok())
            .unwrap_or(0.0);
        Ok(Self {
            base_cost_a,
            cost_b,
            result,
            uses: int("uses", 0),
            max_uses: int("maxUses", 4),
            reward_exp,
            special_price_diff: int("specialPrice", 0),
            demand: int("demand", 0),
            price_multiplier,
            xp: int("xp", 1),
        })
    }

    /// 官方 `isOutOfStock`。
    pub const fn is_out_of_stock(&self) -> bool {
        self.uses >= self.max_uses
    }

    /// 官方 `writeToStream`：后面几个整数是定长 `writeInt`。
    pub fn write(&self, out: &mut Vec<u8>) {
        self.base_cost_a.write(out);
        self.result.write(out);
        match &self.cost_b {
            Some(cost) => {
                write_bool(out, true);
                cost.write(out);
            }
            None => write_bool(out, false),
        }
        write_bool(out, self.is_out_of_stock());
        write_i32(out, self.uses);
        write_i32(out, self.max_uses);
        write_i32(out, self.xp);
        write_i32(out, self.special_price_diff);
        write_f32(out, self.price_multiplier);
        write_i32(out, self.demand);
    }
}

/// 官方 `MerchantOffers.CODEC`：`{Recipes: [...]}`，空列表时省略键。
pub fn encode_offers(offers: &[MerchantOffer]) -> Result<Dyn> {
    let recipes = if offers.is_empty() {
        None
    } else {
        let mut list = Vec::with_capacity(offers.len());
        for offer in offers {
            list.push(offer.encode()?);
        }
        Some(Dyn::List(list))
    };
    Ok(Fields::new().optional("Recipes", recipes).build())
}

/// 官方 `MerchantOffers.CODEC` 的解码。
pub fn decode_offers(tag: &Tag) -> Result<Vec<MerchantOffer>> {
    let entries = read::compound(tag)?;
    let Some(list) = read::field(entries, "Recipes") else {
        return Ok(Vec::new());
    };
    read::list(list)?
        .iter()
        .map(MerchantOffer::decode)
        .collect()
}

/// 官方 `MerchantOffers.STREAM_CODEC`：varint 长度前缀的列表。
pub fn write_offers(out: &mut Vec<u8>, offers: &[MerchantOffer]) {
    write_varint(out, offers.len() as i32);
    for offer in offers {
        offer.write(out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offer() -> MerchantOffer {
        MerchantOffer {
            base_cost_a: ItemCost::new(1, 3),
            cost_b: None,
            result: ItemStack::new(1, 1),
            uses: 0,
            max_uses: 12,
            reward_exp: true,
            special_price_diff: 0,
            demand: 0,
            price_multiplier: 0.05,
            xp: 2,
        }
    }

    #[test]
    fn archive_round_trip() {
        let offers = vec![offer()];
        let tag = encode_offers(&offers).unwrap().to_tag();
        assert_eq!(decode_offers(&tag).unwrap(), offers);
    }

    #[test]
    fn empty_offers_omit_recipes() {
        assert_eq!(encode_offers(&[]).unwrap(), Dyn::Map(Vec::new()));
    }

    #[test]
    fn network_layout() {
        let mut out = Vec::new();
        write_offers(&mut out, &[offer()]);
        // 1 条；价格 A 是 (物品 1, 数量 3, 0 个组件)；结果 (数量 1, 物品 1, 0, 0)；无 B；未售罄。
        assert_eq!(&out[..10], &[1, 1, 3, 0, 1, 1, 0, 0, 0, 0]);
        assert_eq!(out.len(), 10 + 4 * 6);
    }
}
