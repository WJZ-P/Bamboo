//! 官方战利品表的求值。
//!
//! 数据包里的表、条件、数字提供器原样放在 `mc-bamboo-data`，这里把官方
//! `LootTable`、`LootPool`、条目、条件和函数的行为逐个移植过来。
//! 随机数的取数次数和顺序和官方相同，种子相同时结果逐位一致。
//!
//! 调用方一般直接用三个入口：[`block_drops`]、[`entity_death_drops`] 和
//! [`chest_loot`]，它们分别对应官方触发战利品表的三个地方。

mod condition;
mod context;
mod drops;
mod entity_parts;
mod entry;
mod error;
#[cfg(test)]
mod eval_tests;
mod function;
mod json;
mod names;
mod number;
mod pool;
mod predicate;
mod random_sequence;
mod stack;
mod table;
pub mod trade;

pub use condition::{FloatRangePredicate, IntRangePredicate, LootCondition};
pub use context::{
    AdvancementView, BlockEntityView, DamageSourceView, EffectView, EntityTarget, EntityView,
    EquipmentSlot, ExplorationMapRequest, ExplorationMapResult, InputView, LevelView,
    LightningView, LootContext, LootParams, PlayerView, Position, entity_enchantment_level,
};
pub use drops::{
    BlockDropParams, ChestParams, EntityDeathParams, EntityLootParams, block_drops, chest_loot,
    entity_death_drops, entity_loot_drops,
};
pub use entity_parts::{
    FluidPredicate, LightningBoltPredicate, MobEffectsPredicate, NbtPredicate, PlayerPredicate,
    compare_nbt,
};
pub use error::{LootError, Result};
pub use function::{LootFunction, mob_spawn_enchantments};
pub use names::{
    EnchantmentDef, Registry, biome_id, block_experience_set, block_id, block_name,
    enchantment_def, enchantment_defs_all, enchantment_id, enchantment_name, entity_type_id,
    entity_type_name, has_repair_with_xp, holder_set, instrument_name, item_id, item_name,
    mob_effect_name, potion_name, repair_with_xp_factor, tag_ids,
};
pub use number::{
    FloatProvider, IntProvider, LevelBasedValue, parse_float_provider, parse_int_provider,
};
pub use pool::LootPool;
pub use predicate::{
    BlockPredicate, ComponentMatchers, DamageSourcePredicate, DistancePredicate, DoubleBounds,
    EnchantmentPredicate, EntityEquipmentPredicate, EntityFlagsPredicate, EntityPredicate,
    EntityTagPredicate, IntBounds, ItemPredicate, LightPredicate, LocationPredicate,
    MovementPredicate, PartialComponent, RaiderPredicate, StatePropertiesPredicate,
};
pub use random_sequence::{RandomSequence, RandomSequences};
pub use stack::{
    Components, ENCHANTMENTS as ENCHANTMENTS_COMPONENT, EnchantmentLevels, LootStack,
    NoEnchantments,
};
pub use table::{LootContainer, LootTable, create_stack_splitter, shuffle_and_split_items};
