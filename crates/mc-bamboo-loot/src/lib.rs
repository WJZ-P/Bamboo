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
mod entry;
mod error;
mod function;
mod json;
mod names;
mod number;
mod pool;
mod predicate;
mod random_sequence;
mod stack;
mod table;

pub use condition::{FloatRangePredicate, IntRangePredicate, LootCondition};
pub use context::{
    entity_enchantment_level, BlockEntityView, DamageSourceView, EntityTarget, EntityView,
    EquipmentSlot, LevelView, LootContext, LootParams, Position,
};
pub use drops::{
    block_drops, chest_loot, entity_death_drops, BlockDropParams, ChestParams, EntityDeathParams,
};
pub use error::{LootError, Result};
pub use function::LootFunction;
pub use names::{
    biome_id, block_id, block_name, enchantment_def, enchantment_defs_all, enchantment_id,
    enchantment_name, entity_type_id, entity_type_name, instrument_name, item_id, item_name,
    mob_effect_name, potion_name, tag_ids, EnchantmentDef, Registry,
};
pub use number::{FloatProvider, IntProvider, LevelBasedValue};
pub use pool::LootPool;
pub use predicate::{
    BlockPredicate, ComponentMatchers, DamageSourcePredicate, DistancePredicate, DoubleBounds,
    EnchantmentPredicate, EntityEquipmentPredicate, EntityFlagsPredicate, EntityPredicate,
    EntityTagPredicate, IntBounds, ItemPredicate, LightPredicate, LocationPredicate,
    MovementPredicate, PartialComponent, RaiderPredicate, StatePropertiesPredicate,
};
pub use random_sequence::{RandomSequence, RandomSequences};
pub use stack::{Components, EnchantmentLevels, LootStack, NoEnchantments};
pub use table::{create_stack_splitter, shuffle_and_split_items, LootContainer, LootTable};
