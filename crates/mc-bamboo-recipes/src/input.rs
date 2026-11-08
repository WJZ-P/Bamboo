//! 匹配用的输入：物品栈和三种工作台输入，对应官方的 `ItemStack`、`CraftingInput`、
//! `SmithingRecipeInput`、`BrewingInput`。
//!
//! 物品栈直接用协议层的 [`ItemStack`]：组件补丁是不透明字节，需要读组件的配方
//! 用 [`crate::assemble`] 里的解码函数，见 `mc-bamboo-protocol` 的组件模块。

use mc_bamboo_data::Recipe;

pub use mc_bamboo_protocol::item::ItemStack;

/// 合成网格，官方的 `CraftingInput`。
///
/// [`CraftingInput::of`] 和官方一样先裁掉四周全空的整行整列，
/// 所以把单个物品放在 3x3 的角落里会变成 1x1，配方也只按 1x1 比。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CraftingInput {
    width: usize,
    height: usize,
    slots: Vec<ItemStack>,
    ingredient_count: usize,
}

/// 裁剪后的合成输入和它在原网格里的位置，官方的 `CraftingInput.Positioned`。
///
/// `left` 和 `top` 是裁剪后的左上角原来在 `width * height` 网格里的坐标。结果槽取走产物时
/// 要靠它们找到原网格里该扣哪几格（比如 3x3 里摆在右下的 2x2 配方，裁剪后坐标从 0 开始）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PositionedCraftInput {
    pub input: CraftingInput,
    pub left: usize,
    pub top: usize,
}

impl CraftingInput {
    /// 官方的 `CraftingInput.of`：从 `width * height` 个格子建输入，并裁剪空行空列。
    pub fn of(width: usize, height: usize, slots: &[ItemStack]) -> Self {
        Self::of_positioned(width, height, slots).input
    }

    /// 官方的 `CraftingInput.ofPositioned`：和 [`Self::of`] 一样裁剪，
    /// 另外带回裁剪掉的左上角偏移。空输入是 `(0, 0)`。
    pub fn of_positioned(width: usize, height: usize, slots: &[ItemStack]) -> PositionedCraftInput {
        assert_eq!(
            slots.len(),
            width * height,
            "crafting input has {} slots, expected {width} * {height}",
            slots.len()
        );
        if slots.is_empty() {
            return PositionedCraftInput {
                input: Self::empty(),
                left: 0,
                top: 0,
            };
        }

        let mut left = width - 1;
        let mut right = 0;
        let mut top = height - 1;
        let mut bottom = 0;
        for y in 0..height {
            for x in 0..width {
                if !slots[x + y * width].is_empty() {
                    left = left.min(x);
                    right = right.max(x);
                    top = top.min(y);
                    bottom = bottom.max(y);
                }
            }
        }
        if right < left || bottom < top {
            return PositionedCraftInput {
                input: Self::empty(),
                left: 0,
                top: 0,
            };
        }

        let new_width = right - left + 1;
        let new_height = bottom - top + 1;
        let mut cropped = Vec::with_capacity(new_width * new_height);
        for y in top..=bottom {
            for x in left..=right {
                cropped.push(slots[x + y * width].clone());
            }
        }
        PositionedCraftInput {
            input: Self::from_slots(new_width, new_height, cropped),
            left,
            top,
        }
    }

    pub(crate) fn empty() -> Self {
        Self {
            width: 0,
            height: 0,
            slots: Vec::new(),
            ingredient_count: 0,
        }
    }

    fn from_slots(width: usize, height: usize, slots: Vec<ItemStack>) -> Self {
        let ingredient_count = slots.iter().filter(|slot| !slot.is_empty()).count();
        Self {
            width,
            height,
            slots,
            ingredient_count,
        }
    }

    /// 裁剪后的宽度。
    pub fn width(&self) -> usize {
        self.width
    }

    /// 裁剪后的高度。
    pub fn height(&self) -> usize {
        self.height
    }

    /// 格子数，官方 `size`。
    pub fn size(&self) -> usize {
        self.slots.len()
    }

    /// 非空格子数，官方 `ingredientCount`。
    pub fn ingredient_count(&self) -> usize {
        self.ingredient_count
    }

    /// 按一维下标取格子。
    pub fn slot(&self, index: usize) -> &ItemStack {
        &self.slots[index]
    }

    /// 按坐标取格子。
    pub fn get(&self, x: usize, y: usize) -> &ItemStack {
        &self.slots[x + y * self.width]
    }

    /// 全部格子，按行排列。
    pub fn slots(&self) -> &[ItemStack] {
        &self.slots
    }
}

/// 锻造台输入，官方的 `SmithingRecipeInput`。空位用空栈表示。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmithingInput {
    pub template: ItemStack,
    pub base: ItemStack,
    pub addition: ItemStack,
}

impl SmithingInput {
    pub fn new(template: ItemStack, base: ItemStack, addition: ItemStack) -> Self {
        Self {
            template,
            base,
            addition,
        }
    }
}

/// 酿造台输入，官方的 `BrewingInput`。
///
/// 药水内容在物品组件里，这里单独给出药水注册表 ID；`None` 表示这格不是带药水内容的物品。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrewingInput {
    pub input: ItemStack,
    /// 输入格物品的药水注册表 ID，见 `mc_bamboo_data::recipes::POTIONS`。
    pub input_potion: Option<i32>,
    pub reagent: ItemStack,
    /// 材料格物品的药水注册表 ID。
    pub reagent_potion: Option<i32>,
}

impl BrewingInput {
    pub const fn new(
        input: ItemStack,
        input_potion: Option<i32>,
        reagent: ItemStack,
        reagent_potion: Option<i32>,
    ) -> Self {
        Self {
            input,
            input_potion,
            reagent,
            reagent_potion,
        }
    }
}

/// 配方输入，官方的 `RecipeInput`：能判断空不空，也能试一个配方能不能做出来。
pub trait RecipeInput {
    /// 官方的 `RecipeInput.isEmpty`。空输入在 [`RecipeManager`](crate::RecipeManager) 里直接找不到配方。
    fn is_empty(&self) -> bool;

    /// 这个输入能不能做出 `recipe`。配方类型对不上时返回 false。
    fn matches(&self, recipe: &Recipe) -> bool;
}
