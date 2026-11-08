//! 配方查找和匹配，对应官方的 `net.minecraft.world.item.crafting`。
//!
//! 只依赖 `mc-bamboo-data` 里导出的配方表，不碰网络、世界线程和物品组件编解码。
//! 输入的物品栈按“默认组件”处理：组件还是不透明字节，特殊配方里需要读组件的地方
//! 见 [`RecipeKind`](mc_bamboo_data::RecipeKind) 各个变体的说明。
//!
//! 查找语义和官方 `RecipeManager.getRecipeFor` 一致：先用提示的配方，
//! 再按注册表顺序取第一条匹配的；输入为空时一个都不找。

mod input;
mod manager;
mod matching;

pub use input::{BrewingInput, CraftingInput, ItemStack, RecipeInput, SmithingInput};
pub use manager::RecipeManager;
