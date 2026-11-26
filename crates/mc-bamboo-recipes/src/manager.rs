//! 配方表的查找，对应官方的 `RecipeManager` 和 `RecipeMap`。

use std::collections::BTreeMap;
use std::sync::OnceLock;

use mc_bamboo_data::{Recipe, RecipeType, recipes::RECIPES};

use crate::input::RecipeInput;

/// [`RecipeType`] 的个数，`by_type` 的下标就是类型。
const TYPE_COUNT: usize = 8;

/// 全部配方和按类型的索引。
///
/// 条目顺序和官方注册表一致，查找时取第一条匹配的；`by_type` 只记下标，顺序不变。
pub struct RecipeManager {
    recipes: &'static [Recipe],
    by_type: [Vec<usize>; TYPE_COUNT],
    by_id: BTreeMap<&'static str, usize>,
}

impl RecipeManager {
    /// 用生成器导出的配方表建索引。
    pub fn new() -> Self {
        let mut by_type: [Vec<usize>; TYPE_COUNT] = std::array::from_fn(|_| Vec::new());
        let mut by_id = BTreeMap::new();
        for (index, recipe) in RECIPES.iter().enumerate() {
            by_type[type_index(recipe.recipe_type())].push(index);
            by_id.insert(recipe.id, index);
        }
        Self {
            recipes: RECIPES,
            by_type,
            by_id,
        }
    }

    /// 全进程共用的一份索引。
    pub fn global() -> &'static Self {
        static GLOBAL: OnceLock<RecipeManager> = OnceLock::new();
        GLOBAL.get_or_init(Self::new)
    }

    /// 全部配方，按注册表顺序。
    pub fn all(&self) -> &'static [Recipe] {
        self.recipes
    }

    /// 某个类型的全部配方，按注册表顺序。切石机同一个输入的多条结果就靠这个顺序。
    pub fn recipes_of(&self, kind: RecipeType) -> impl Iterator<Item = &'static Recipe> + '_ {
        self.by_type[type_index(kind)]
            .iter()
            .map(|&index| &self.recipes[index])
    }

    /// 按配方 ID 找配方，比如 `minecraft:oak_planks`。
    pub fn recipe(&self, id: &str) -> Option<&'static Recipe> {
        self.by_id.get(id).map(|&index| &self.recipes[index])
    }

    /// 官方的 `RecipeManager.getRecipeFor`。
    ///
    /// `hint` 是“上次用过的配方”，对应官方的配方键提示：ID 存在且类型相同就先试它。
    /// 没命中就按注册表顺序找第一条匹配的；输入为空时一条都不找。
    pub fn get_recipe_for<I: RecipeInput>(
        &self,
        kind: RecipeType,
        input: &I,
        hint: Option<&'static Recipe>,
    ) -> Option<&'static Recipe> {
        if let Some(hint) = hint
            && hint.recipe_type() == kind
            && input.matches(hint)
        {
            return Some(hint);
        }
        self.matching(kind, input).next()
    }

    /// 官方的 `RecipeMap.getRecipesFor`：某个类型里全部匹配的配方。
    pub fn matching<'a, I: RecipeInput + 'a>(
        &'a self,
        kind: RecipeType,
        input: &'a I,
    ) -> impl Iterator<Item = &'static Recipe> + 'a {
        let empty = input.is_empty();
        self.by_type[type_index(kind)]
            .iter()
            .filter(move |&&index| !empty && input.matches(&self.recipes[index]))
            .map(|&index| &self.recipes[index])
    }
}

impl Default for RecipeManager {
    fn default() -> Self {
        Self::new()
    }
}

const fn type_index(kind: RecipeType) -> usize {
    match kind {
        RecipeType::Crafting => 0,
        RecipeType::Smelting => 1,
        RecipeType::Blasting => 2,
        RecipeType::Smoking => 3,
        RecipeType::CampfireCooking => 4,
        RecipeType::Stonecutting => 5,
        RecipeType::Smithing => 6,
        RecipeType::Brewing => 7,
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::{RecipeKind, items};
    use mc_bamboo_protocol::item::ItemStack;

    use super::*;
    use crate::CraftingInput;

    /// 十六种挽具染色配方都在生成表里：白挽具加黑染料出黑挽具，
    /// 黑挽具不能再染成黑色（官方把目标色排除在候选之外）。
    ///
    /// All sixteen harness dye recipes are in the generated table: a white
    /// harness plus black dye yields a black harness, while a black harness
    /// is excluded from that recipe's candidates.
    #[test]
    fn harness_dye_recipes_cover_all_sixteen_colours() {
        let manager = RecipeManager::global();
        let dye_names = [
            "dye_white_harness",
            "dye_orange_harness",
            "dye_magenta_harness",
            "dye_light_blue_harness",
            "dye_yellow_harness",
            "dye_lime_harness",
            "dye_pink_harness",
            "dye_gray_harness",
            "dye_light_gray_harness",
            "dye_cyan_harness",
            "dye_purple_harness",
            "dye_blue_harness",
            "dye_brown_harness",
            "dye_green_harness",
            "dye_red_harness",
            "dye_black_harness",
        ];
        for name in dye_names {
            assert!(
                manager.recipe(&format!("minecraft:{name}")).is_some(),
                "missing {name}"
            );
        }
        let input = CraftingInput::of(
            2,
            1,
            &[
                ItemStack::new(items::WHITE_HARNESS, 1),
                ItemStack::new(items::BLACK_DYE, 1),
            ],
        );
        let found = manager
            .get_recipe_for(RecipeType::Crafting, &input, None)
            .expect("white harness + black dye has a recipe");
        assert_eq!(found.id, "minecraft:dye_black_harness");
        let RecipeKind::Shapeless { result, .. } = found.kind else {
            panic!("harness dyeing is shapeless");
        };
        assert_eq!(result.item, items::BLACK_HARNESS);
        let black_input = CraftingInput::of(
            2,
            1,
            &[
                ItemStack::new(items::BLACK_HARNESS, 1),
                ItemStack::new(items::BLACK_DYE, 1),
            ],
        );
        assert!(
            manager
                .get_recipe_for(RecipeType::Crafting, &black_input, None)
                .is_none()
        );
    }
}
