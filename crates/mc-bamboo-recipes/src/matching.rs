//! 各类配方的 `matches`，一一对应官方的配方类。

use mc_bamboo_data::{
    FireworkShape, Ingredient, ItemKind, PotionIngredient, Recipe, RecipeKind, is_damageable, items,
    registries::data_component_type as kinds,
};
use mc_bamboo_protocol::components::{ComponentValue, defaults};

use crate::assemble::{applied, patch_of, template_patch};
use crate::input::{BrewingInput, CraftingInput, ItemStack, RecipeInput, SmithingInput};

impl RecipeInput for CraftingInput {
    fn is_empty(&self) -> bool {
        self.ingredient_count() == 0
    }

    fn matches(&self, recipe: &Recipe) -> bool {
        crafting_matches(self, &recipe.kind)
    }
}

impl RecipeInput for ItemStack {
    fn is_empty(&self) -> bool {
        ItemStack::is_empty(self)
    }

    fn matches(&self, recipe: &Recipe) -> bool {
        match &recipe.kind {
            RecipeKind::Cooking { ingredient, .. }
            | RecipeKind::Stonecutting { ingredient, .. } => ingredient_matches(*ingredient, self),
            _ => false,
        }
    }
}

impl RecipeInput for SmithingInput {
    fn is_empty(&self) -> bool {
        self.template.is_empty() && self.base.is_empty() && self.addition.is_empty()
    }

    fn matches(&self, recipe: &Recipe) -> bool {
        match &recipe.kind {
            RecipeKind::SmithingTransform {
                template,
                base,
                addition,
                ..
            } => {
                optional_ingredient_matches(*template, &self.template)
                    && ingredient_matches(*base, &self.base)
                    && optional_ingredient_matches(*addition, &self.addition)
            }
            RecipeKind::SmithingTrim {
                template,
                base,
                addition,
                ..
            } => {
                ingredient_matches(*template, &self.template)
                    && ingredient_matches(*base, &self.base)
                    && ingredient_matches(*addition, &self.addition)
            }
            _ => false,
        }
    }
}

impl RecipeInput for BrewingInput {
    fn is_empty(&self) -> bool {
        self.input.is_empty() && self.reagent.is_empty()
    }

    fn matches(&self, recipe: &Recipe) -> bool {
        match &recipe.kind {
            RecipeKind::Brewing { input, reagent, .. } => {
                potion_ingredient_matches(*input, &self.input, self.input_potion)
                    && potion_ingredient_matches(*reagent, &self.reagent, self.reagent_potion)
            }
            _ => false,
        }
    }
}

fn crafting_matches(input: &CraftingInput, kind: &RecipeKind) -> bool {
    match kind {
        RecipeKind::Shaped {
            width,
            height,
            pattern,
            ..
        } => shaped_matches(input, *width, *height, pattern),
        RecipeKind::Shapeless { ingredients, .. } => shapeless_matches(input, ingredients),
        RecipeKind::Transmute { .. } => transmute_matches(input, kind),
        RecipeKind::Dye { target, dye, .. } => dye_matches(input, *target, *dye),
        RecipeKind::Imbue {
            source, material, ..
        } => imbue_matches(input, *source, *material),
        RecipeKind::FireworkRocket {
            shell, fuel, star, ..
        } => firework_rocket_matches(input, *shell, *fuel, *star),
        RecipeKind::FireworkStar {
            shapes,
            trail,
            twinkle,
            fuel,
            dye,
            ..
        } => firework_star_matches(input, shapes, *trail, *twinkle, *fuel, *dye),
        RecipeKind::FireworkStarFade { target, dye, .. } => {
            firework_star_fade_matches(input, *target, *dye)
        }
        RecipeKind::MapExtending { .. } => map_extending_matches(input),
        RecipeKind::BannerDuplicate { banner, .. } => banner_duplicate_matches(input, *banner),
        RecipeKind::BookCloning {
            source,
            material,
            min_generation,
            max_generation,
            ..
        } => book_cloning_matches(input, *source, *material, *min_generation, *max_generation),
        RecipeKind::ShieldDecoration { banner, target, .. } => {
            shield_decoration_matches(input, *banner, *target)
        }
        RecipeKind::RepairItem => repair_item_matches(input),
        RecipeKind::DecoratedPot {
            back,
            left,
            right,
            front,
            ..
        } => decorated_pot_matches(input, *back, *left, *right, *front),
        _ => false,
    }
}

/// 官方的 `Ingredient.test(ItemStack)`：空栈永远不匹配。
fn ingredient_matches(ingredient: Ingredient, stack: &ItemStack) -> bool {
    !stack.is_empty() && ingredient.test(stack.item)
}

/// 官方的 `Ingredient.testOptionalIngredient`：没有原料时要求这一格是空的。
fn optional_ingredient_matches(ingredient: Option<Ingredient>, stack: &ItemStack) -> bool {
    match ingredient {
        Some(ingredient) => ingredient_matches(ingredient, stack),
        None => stack.is_empty(),
    }
}

/// `ShapedRecipePattern.matches`：先比原料数和尺寸，再按镜像、正向依次试。
fn shaped_matches(
    input: &CraftingInput,
    width: u8,
    height: u8,
    pattern: &[Option<Ingredient>],
) -> bool {
    let (width, height) = (usize::from(width), usize::from(height));
    let present = pattern.iter().filter(|cell| cell.is_some()).count();
    if input.ingredient_count() != present {
        return false;
    }
    if input.width() != width || input.height() != height {
        return false;
    }
    if !is_symmetrical(width, height, pattern)
        && shaped_matches_flipped(input, width, height, pattern)
    {
        return true;
    }
    shaped_matches_in_direction(input, width, height, pattern, false)
}

/// 官方的 `Util.isSymmetrical`：宽度为 1，或者左右两半逐格相等。
fn is_symmetrical(width: usize, height: usize, pattern: &[Option<Ingredient>]) -> bool {
    if width == 1 {
        return true;
    }
    let center = width / 2;
    for y in 0..height {
        for left in 0..center {
            let right = width - 1 - left;
            if pattern[left + y * width] != pattern[right + y * width] {
                return false;
            }
        }
    }
    true
}

fn shaped_matches_flipped(
    input: &CraftingInput,
    width: usize,
    height: usize,
    pattern: &[Option<Ingredient>],
) -> bool {
    shaped_matches_in_direction(input, width, height, pattern, true)
}

fn shaped_matches_in_direction(
    input: &CraftingInput,
    width: usize,
    height: usize,
    pattern: &[Option<Ingredient>],
    flip: bool,
) -> bool {
    for y in 0..height {
        for x in 0..width {
            let pattern_x = if flip { width - x - 1 } else { x };
            let expected = pattern[pattern_x + y * width];
            if !optional_ingredient_matches(expected, input.get(x, y)) {
                return false;
            }
        }
    }
    true
}

/// `ShapelessRecipe.matches`。
fn shapeless_matches(input: &CraftingInput, ingredients: &[Ingredient]) -> bool {
    if input.ingredient_count() != ingredients.len() {
        return false;
    }
    if input.size() == 1 && ingredients.len() == 1 {
        return ingredient_matches(ingredients[0], input.slot(0));
    }

    // 官方的 `StackedContents` 是二分图匹配：每种物品的容量是这种物品的非空格子数。
    // 每个原料只要一份，所以就是原料到格子的完美匹配。
    let slots: Vec<&ItemStack> = input
        .slots()
        .iter()
        .filter(|slot| !slot.is_empty())
        .collect();
    let mut assigned = vec![None; slots.len()];
    for ingredient in 0..ingredients.len() {
        let mut visited = vec![false; slots.len()];
        if !assign_ingredient(ingredient, ingredients, &slots, &mut assigned, &mut visited) {
            return false;
        }
    }
    true
}

fn assign_ingredient(
    ingredient: usize,
    ingredients: &[Ingredient],
    slots: &[&ItemStack],
    assigned: &mut [Option<usize>],
    visited: &mut [bool],
) -> bool {
    for (slot, stack) in slots.iter().enumerate() {
        if visited[slot] || !ingredient_matches(ingredients[ingredient], stack) {
            continue;
        }
        visited[slot] = true;
        if match assigned[slot] {
            Some(other) => assign_ingredient(other, ingredients, slots, assigned, visited),
            None => true,
        } {
            assigned[slot] = Some(ingredient);
            return true;
        }
    }
    false
}

/// `TransmuteRecipe.matches`。
fn transmute_matches(input: &CraftingInput, kind: &RecipeKind) -> bool {
    let RecipeKind::Transmute {
        input: input_ingredient,
        material,
        min_material,
        max_material,
        result_item,
        result_count,
        result_components,
        add_material_count,
    } = *kind
    else {
        return false;
    };
    let count = i32::try_from(input.ingredient_count()).unwrap_or(i32::MAX);
    if count < min_material + 1 || count > max_material + 1 {
        return false;
    }

    let mut found_input = None;
    let mut material_count = 0;
    for slot in input.slots() {
        if slot.is_empty() {
            continue;
        }
        if input_ingredient.test(slot.item) {
            if found_input.is_some() {
                return false;
            }
            found_input = Some(slot);
        } else {
            if !material.test(slot.item) {
                return false;
            }
            material_count += 1;
            if material_count > max_material {
                return false;
            }
        }
    }

    let Some(found_input) = found_input else {
        return false;
    };
    if material_count < min_material || material_count > max_material {
        return false;
    }
    let result_count = if add_material_count {
        material_count + result_count
    } else {
        result_count
    };
    if result_count != 1 {
        return true;
    }
    // 官方比较产物和输入是不是同一个物品、同一套组件。产物沿用输入的组件改动，
    // 模板自己带的组件再叠上去，所以要真的合并一遍补丁再比。
    if result_item.unwrap_or(found_input.item) != found_input.item {
        return true;
    }
    let template = template_patch(result_components);
    let result = applied(found_input.item, &patch_of(found_input), &template);
    result != patch_of(found_input)
}

/// `DyeRecipe.matches`。
fn dye_matches(input: &CraftingInput, target: Ingredient, dye: Ingredient) -> bool {
    if input.ingredient_count() < 2 {
        return false;
    }
    let mut has_target = false;
    let mut has_dyes = false;
    for slot in input.slots() {
        if slot.is_empty() {
            continue;
        }
        if target.test(slot.item) {
            if has_target {
                return false;
            }
            has_target = true;
        } else if !dye.test(slot.item) || !has_component(slot, kinds::DYE) {
            return false;
        } else {
            has_dyes = true;
        }
    }
    has_target && has_dyes
}

/// `ImbueRecipe.matches`：3x3 摆满，中心是药水。
fn imbue_matches(input: &CraftingInput, source: Ingredient, material: Ingredient) -> bool {
    if input.width() != 3 || input.height() != 3 || input.ingredient_count() != 9 {
        return false;
    }
    for y in 0..3 {
        for x in 0..3 {
            let ingredient = if x == 1 && y == 1 { source } else { material };
            if !ingredient_matches(ingredient, input.get(x, y)) {
                return false;
            }
        }
    }
    true
}

/// `FireworkRocketRecipe.matches`。
fn firework_rocket_matches(
    input: &CraftingInput,
    shell: Ingredient,
    fuel: Ingredient,
    star: Ingredient,
) -> bool {
    if input.ingredient_count() < 2 {
        return false;
    }
    let mut has_shell = false;
    let mut fuel_count = 0;
    for slot in input.slots() {
        if slot.is_empty() {
            continue;
        }
        if shell.test(slot.item) {
            if has_shell {
                return false;
            }
            has_shell = true;
        } else if fuel.test(slot.item) {
            fuel_count += 1;
            if fuel_count > 3 {
                return false;
            }
        } else if !star.test(slot.item) {
            return false;
        }
    }
    has_shell && fuel_count >= 1
}

/// `FireworkStarRecipe.matches`。
fn firework_star_matches(
    input: &CraftingInput,
    shapes: &[(FireworkShape, Ingredient)],
    trail: Ingredient,
    twinkle: Ingredient,
    fuel: Ingredient,
    dye: Ingredient,
) -> bool {
    if input.ingredient_count() < 2 {
        return false;
    }
    let mut has_fuel = false;
    let mut has_dye = false;
    let mut has_shape = false;
    let mut has_trail = false;
    let mut has_twinkle = false;
    for slot in input.slots() {
        if slot.is_empty() {
            continue;
        }
        if twinkle.test(slot.item) {
            if has_twinkle {
                return false;
            }
            has_twinkle = true;
        } else if trail.test(slot.item) {
            if has_trail {
                return false;
            }
            has_trail = true;
        } else if fuel.test(slot.item) {
            if has_fuel {
                return false;
            }
            has_fuel = true;
        } else if dye.test(slot.item) && has_component(slot, kinds::DYE) {
            has_dye = true;
        } else {
            if !shapes
                .iter()
                .any(|(_, ingredient)| ingredient.test(slot.item))
            {
                return false;
            }
            if has_shape {
                return false;
            }
            has_shape = true;
        }
    }
    has_fuel && has_dye
}

/// `FireworkStarFadeRecipe.matches`。
fn firework_star_fade_matches(input: &CraftingInput, target: Ingredient, dye: Ingredient) -> bool {
    if input.ingredient_count() < 2 {
        return false;
    }
    let mut has_dye = false;
    let mut has_target = false;
    for slot in input.slots() {
        if slot.is_empty() {
            continue;
        }
        if dye.test(slot.item) && has_component(slot, kinds::DYE) {
            has_dye = true;
        } else {
            if !target.test(slot.item) {
                return false;
            }
            if has_target {
                return false;
            }
            has_target = true;
        }
    }
    has_target && has_dye
}

/// `MapExtendingRecipe.matches`。
///
/// 官方摆法是 `###/#x#/###`，`x` 是地图；之后还要找带 `MAP_ID` 的地图，
/// 再查地图存档的缩放等级。这里能读到 `MAP_ID`，但地图存档（缩放等级）还没有，
/// 所以匹配结果仍恒为 false，见最终报告的“没做的”。
fn map_extending_matches(_input: &CraftingInput) -> bool {
    false
}

/// `BannerDuplicateRecipe.matches`。
///
/// 官方要求一面旗帜带 1..=6 层图案，另一面不带；两面颜色相同。
fn banner_duplicate_matches(input: &CraftingInput, banner: Ingredient) -> bool {
    if input.ingredient_count() != 2 {
        return false;
    }
    let mut color = None;
    let mut has_target = false;
    let mut has_source = false;
    for slot in input.slots() {
        if slot.is_empty() {
            continue;
        }
        if !banner.test(slot.item) || !is_banner_item(slot.item) {
            return false;
        }
        match color {
            None => color = Some(slot.item),
            Some(item) if item != slot.item => return false,
            Some(_) => {}
        }
        let patterns = banner_pattern_count(slot);
        if patterns > 6 {
            return false;
        }
        if patterns > 0 {
            if has_source {
                return false;
            }
            has_source = true;
        } else {
            if has_target {
                return false;
            }
            has_target = true;
        }
    }
    has_source && has_target
}

/// `BookCloningRecipe.matches`。
///
/// 官方要求源物品带 `WRITTEN_BOOK_CONTENT`，世代在允许范围内。
fn book_cloning_matches(
    input: &CraftingInput,
    source: Ingredient,
    material: Ingredient,
    min_generation: i32,
    max_generation: i32,
) -> bool {
    if input.ingredient_count() < 2 {
        return false;
    }
    let mut has_material = false;
    let mut has_source = false;
    for slot in input.slots() {
        if slot.is_empty() {
            continue;
        }
        if source.test(slot.item) {
            let Some(generation) = written_book_generation(slot) else {
                return false;
            };
            if !(min_generation..=max_generation).contains(&generation) {
                return false;
            }
            if has_source {
                return false;
            }
            has_source = true;
        } else {
            if !material.test(slot.item) {
                return false;
            }
            has_material = true;
        }
    }
    has_source && has_material
}

/// `ShieldDecorationRecipe.matches`。
///
/// 一面旗帜加一个空图案的目标。
fn shield_decoration_matches(
    input: &CraftingInput,
    banner: Ingredient,
    target: Ingredient,
) -> bool {
    if input.ingredient_count() != 2 {
        return false;
    }
    let mut has_clear_target = false;
    let mut has_pattern_banner = false;
    for slot in input.slots() {
        if slot.is_empty() {
            continue;
        }
        if banner.test(slot.item) && is_banner_item(slot.item) {
            if has_pattern_banner {
                return false;
            }
            has_pattern_banner = true;
        } else {
            if !target.test(slot.item) {
                return false;
            }
            if has_clear_target {
                return false;
            }
            if banner_pattern_count(slot) != 0 {
                return false;
            }
            has_clear_target = true;
        }
    }
    has_clear_target && has_pattern_banner
}

/// `RepairItemRecipe.matches`。
fn repair_item_matches(input: &CraftingInput) -> bool {
    if input.ingredient_count() != 2 {
        return false;
    }
    let mut first = None;
    for slot in input.slots() {
        if slot.is_empty() {
            continue;
        }
        let Some(first) = first else {
            first = Some(slot);
            continue;
        };
        return repair_pair(first, slot);
    }
    false
}

fn repair_pair(first: &ItemStack, second: &ItemStack) -> bool {
    second.item == first.item
        && first.count == 1
        && second.count == 1
        && is_damageable(first.item)
        && is_damageable(second.item)
}

/// `DecoratedPotRecipe.matches`：四块陶片在田字格的四个边上。
fn decorated_pot_matches(
    input: &CraftingInput,
    back: Ingredient,
    left: Ingredient,
    right: Ingredient,
    front: Ingredient,
) -> bool {
    if input.width() != 3 || input.height() != 3 || input.ingredient_count() != 4 {
        return false;
    }
    ingredient_matches(back, input.get(1, 0))
        && ingredient_matches(left, input.get(0, 1))
        && ingredient_matches(right, input.get(2, 1))
        && ingredient_matches(front, input.get(1, 2))
}

fn potion_ingredient_matches(
    ingredient: PotionIngredient,
    stack: &ItemStack,
    potion: Option<i32>,
) -> bool {
    if !ingredient_matches(ingredient.item, stack) {
        return false;
    }
    match ingredient.potions {
        None => true,
        Some(potions) => potion.is_some_and(|potion| potions.binary_search(&potion).is_ok()),
    }
}

/// 物品是不是 `BannerItem`。
fn is_banner_item(item: i32) -> bool {
    usize::try_from(item)
        .ok()
        .and_then(|index| items::ITEMS.get(index))
        .is_some_and(|entry| entry.kind == ItemKind::Banner)
}

/// 一个物品栈上某个组件的生效值，官方的 `ItemStack.get`。
pub(crate) fn resolved(stack: &ItemStack, kind: i32) -> Option<ComponentValue> {
    match mc_bamboo_protocol::components::ComponentPatch::from_opaque(stack) {
        Ok(patch) => defaults::resolve(stack.item, &patch, kind).cloned(),
        // 客户端的组件在进背包前会经过 `validate_stack`；真解不开时按默认组件算，
        // 不让配方查找把世界线程带崩。
        Err(_) => defaults::of_item(stack.item).get(kind).cloned(),
    }
}

/// 官方的 `ItemStack.has(type)`。
fn has_component(stack: &ItemStack, kind: i32) -> bool {
    resolved(stack, kind).is_some()
}

/// 旗帜上 `BANNER_PATTERNS` 的层数，官方 `getOrDefault(BANNER_PATTERNS, EMPTY).layers().size()`。
fn banner_pattern_count(stack: &ItemStack) -> usize {
    match resolved(stack, kinds::BANNER_PATTERNS) {
        Some(ComponentValue::BannerPatterns(layers)) => layers.len(),
        _ => 0,
    }
}

/// 成书的世代，官方 `WrittenBookContent.generation()`。没有内容时返回 `None`。
fn written_book_generation(stack: &ItemStack) -> Option<i32> {
    match resolved(stack, kinds::WRITTEN_BOOK_CONTENT) {
        Some(ComponentValue::WrittenBookContent { generation, .. }) => Some(generation),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ITEM_A: i32 = 1;
    const ITEM_B: i32 = 2;
    const ITEM_C: i32 = 3;
    const A: Ingredient = Ingredient {
        tag: None,
        items: &[ITEM_A],
    };
    const B: Ingredient = Ingredient {
        tag: None,
        items: &[ITEM_B],
    };

    fn one(item: i32) -> ItemStack {
        ItemStack::new(item, 1)
    }

    fn grid(width: usize, height: usize, items: &[i32]) -> CraftingInput {
        let slots: Vec<ItemStack> = items
            .iter()
            .map(|&item| {
                if item == 0 {
                    ItemStack::empty()
                } else {
                    one(item)
                }
            })
            .collect();
        CraftingInput::of(width, height, &slots)
    }

    #[test]
    fn shaped_recipe_ignores_empty_border() {
        const PATTERN: &[Option<Ingredient>] = &[Some(A), Some(B), None, Some(A)];
        // 2x2 的配方放在 3x3 的右下角，左边和上边各空一行一列。
        let input = grid(3, 3, &[0, 0, 0, 0, ITEM_A, ITEM_B, 0, 0, ITEM_A]);
        assert_eq!((input.width(), input.height()), (2, 2));
        assert!(shaped_matches(&input, 2, 2, PATTERN));
    }

    #[test]
    fn shaped_recipe_matches_mirrored_grid() {
        const PATTERN: &[Option<Ingredient>] = &[Some(A), Some(B)];
        let normal = grid(2, 1, &[ITEM_A, ITEM_B]);
        let mirrored = grid(2, 1, &[ITEM_B, ITEM_A]);
        assert!(shaped_matches(&normal, 2, 1, PATTERN));
        assert!(shaped_matches(&mirrored, 2, 1, PATTERN));
    }

    #[test]
    fn shaped_recipe_rejects_wrong_size_and_count() {
        const PATTERN: &[Option<Ingredient>] = &[Some(A), Some(B)];
        let too_wide = grid(2, 1, &[ITEM_A, ITEM_B]);
        assert!(!shaped_matches(&too_wide, 1, 1, &[Some(A)]));
        let missing = grid(2, 1, &[ITEM_A, 0]);
        assert!(!shaped_matches(&missing, 2, 1, PATTERN));
    }

    #[test]
    fn shapeless_recipe_reassigns_slots() {
        // 原料 0 可以放 A 或 B，原料 1 只能放 A。格子先给 A、B 时，
        // 必须把原料 0 挪到 B，才能给原料 1 腾出 A。
        const WIDE: Ingredient = Ingredient {
            tag: None,
            items: &[ITEM_A, ITEM_B],
        };
        const INGREDIENTS: [Ingredient; 2] = [WIDE, A];
        let input = grid(2, 1, &[ITEM_A, ITEM_B]);
        assert!(shapeless_matches(&input, &INGREDIENTS));
    }

    #[test]
    fn shapeless_recipe_needs_every_ingredient() {
        const INGREDIENTS: [Ingredient; 2] = [A, B];
        let input = grid(2, 1, &[ITEM_A, ITEM_C]);
        assert!(!shapeless_matches(&input, &INGREDIENTS));
        let with_extra = grid(2, 2, &[ITEM_A, ITEM_B, ITEM_C, 0]);
        assert!(!shapeless_matches(&with_extra, &INGREDIENTS));
    }

    #[test]
    fn empty_checks_use_the_protocol_stack() {
        assert!(ItemStack::empty().is_empty());
        assert!(!ItemStack::new(ITEM_A, 1).is_empty());
        assert_eq!(
            CraftingInput::of(3, 3, &std::array::from_fn::<_, 9, _>(|_| ItemStack::empty())).size(),
            0
        );
    }
}
