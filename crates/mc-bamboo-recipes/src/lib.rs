//! 配方查找和匹配，对应官方的 `net.minecraft.world.item.crafting`。
//!
//! 只依赖 `mc-bamboo-data` 里导出的配方表，不碰网络和世界线程。物品组件通过
//! `mc-bamboo-protocol` 的补丁解码读取，需要读组件的特殊配方见
//! [`RecipeKind`](mc_bamboo_data::RecipeKind) 各个变体的说明。
//!
//! 查找语义和官方 `RecipeManager.getRecipeFor` 一致：先用提示的配方，
//! 再按注册表顺序取第一条匹配的；输入为空时一个都不找。

mod assemble;
mod input;
mod manager;
mod matching;
mod workstations;

pub use assemble::{assemble, potion_id, remaining_items};
pub use input::{
    BrewingInput, CraftingInput, ItemStack, PositionedCraftInput, RecipeInput, SmithingInput,
};
pub use manager::RecipeManager;
pub use workstations::{
    accepts_stonecutter_input, brewing_fuel, brewing_inputs, brewing_reagents, cooking_fuel,
    crafting_remainder, equipment_slot, equipment_slot_for_item, is_brewing_fuel,
    is_brewing_potion_input, is_brewing_reagent, is_cooking_fuel, is_cooking_input,
    is_equippable_in_slot, is_smithing_addition, is_smithing_base, is_smithing_template,
    item_tag_contains, max_stack_size, same_item_same_components, stonecutter_recipes,
};
