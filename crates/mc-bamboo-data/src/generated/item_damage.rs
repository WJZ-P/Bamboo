// 这个文件由 mc-bamboo-vanilla 生成，不要手动修改。

use crate::{
    ItemDamageEffect, ItemDamageEnchantment, LevelBasedValue, ToolMatch, ValueEffect,
};
use crate::tags::item_tags;

/// 带 `minecraft:item_damage` 效果的附魔。顺序是附魔名字的字典序，查找按注册表 ID。
pub static ITEM_DAMAGE: &[ItemDamageEnchantment] = &[
    ItemDamageEnchantment {
        enchantment: 40,
        effects: &[
            ItemDamageEffect {
                effect: ValueEffect::RemoveBinomial { chance: LevelBasedValue::Fraction { numerator: &LevelBasedValue::Linear { base: 2.0, per_level_above_first: 2.0 }, denominator: &LevelBasedValue::Linear { base: 10.0, per_level_above_first: 5.0 } } },
                requirement: Some(ToolMatch::Tag(item_tags::ENCHANTABLE_ARMOR)),
            },
            ItemDamageEffect {
                effect: ValueEffect::RemoveBinomial { chance: LevelBasedValue::Fraction { numerator: &LevelBasedValue::Linear { base: 1.0, per_level_above_first: 1.0 }, denominator: &LevelBasedValue::Linear { base: 2.0, per_level_above_first: 1.0 } } },
                requirement: Some(ToolMatch::Inverted(&ToolMatch::Tag(item_tags::ENCHANTABLE_ARMOR))),
            },
        ],
    },
];
