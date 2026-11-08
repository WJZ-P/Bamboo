//! 导出的配方表和查找逻辑的自检：标签展开、每条配方能不能用自己的原料做出来、
//! 特殊配方在默认物品组件下的行为。

use std::collections::BTreeSet;

use mc_bamboo_data::{
    Ingredient, RecipeKind, RecipeType, items,
    recipes::{POTIONS, RECIPES},
    tags::TAG_GROUPS,
};
use mc_bamboo_recipes::{
    BrewingInput, CraftingInput, ItemStack, RecipeInput, RecipeManager, SmithingInput,
};

fn grid(stacks: &[ItemStack]) -> CraftingInput {
    assert!(stacks.len() <= 9);
    let mut slots: [ItemStack; 9] = std::array::from_fn(|_| ItemStack::empty());
    slots[..stacks.len()].clone_from_slice(stacks);
    CraftingInput::of(3, 3, &slots)
}

fn grid_from_items(items: &[i32]) -> CraftingInput {
    let stacks: Vec<ItemStack> = items.iter().map(|&item| ItemStack::new(item, 1)).collect();
    grid(&stacks)
}

/// 按配方的摆法放原料，按 3x3 摆，`of` 会裁回配方尺寸。
fn shaped_grid(kind: &RecipeKind) -> Option<CraftingInput> {
    let RecipeKind::Shaped {
        width,
        height,
        pattern,
        ..
    } = kind
    else {
        return None;
    };
    let mut slots: [ItemStack; 9] = std::array::from_fn(|_| ItemStack::empty());
    for y in 0..usize::from(*height) {
        for x in 0..usize::from(*width) {
            if let Some(ingredient) = pattern[x + y * usize::from(*width)] {
                slots[x + y * 3] = ItemStack::new(ingredient.items[0], 1);
            }
        }
    }
    Some(CraftingInput::of(3, 3, &slots))
}

fn shapeless_grid(kind: &RecipeKind) -> Option<CraftingInput> {
    let RecipeKind::Shapeless { ingredients, .. } = kind else {
        return None;
    };
    let stacks: Vec<ItemStack> = ingredients
        .iter()
        .map(|ingredient| ItemStack::new(ingredient.items[0], 1))
        .collect();
    Some(grid(&stacks))
}

fn each_ingredient(kind: &RecipeKind, mut visit: impl FnMut(Ingredient)) {
    match kind {
        RecipeKind::Shaped { pattern, .. } => {
            for ingredient in pattern.iter().flatten() {
                visit(*ingredient);
            }
        }
        RecipeKind::Shapeless { ingredients, .. } => {
            for ingredient in *ingredients {
                visit(*ingredient);
            }
        }
        RecipeKind::Transmute {
            input, material, ..
        } => {
            visit(*input);
            visit(*material);
        }
        RecipeKind::Dye { target, dye, .. } | RecipeKind::FireworkStarFade { target, dye, .. } => {
            visit(*target);
            visit(*dye);
        }
        RecipeKind::Imbue {
            source, material, ..
        } => {
            visit(*source);
            visit(*material);
        }
        RecipeKind::Cooking { ingredient, .. } | RecipeKind::Stonecutting { ingredient, .. } => {
            visit(*ingredient);
        }
        RecipeKind::SmithingTransform {
            template,
            base,
            addition,
            ..
        } => {
            for ingredient in [*template, Some(*base), *addition].into_iter().flatten() {
                visit(ingredient);
            }
        }
        RecipeKind::SmithingTrim {
            template,
            base,
            addition,
            ..
        } => {
            visit(*template);
            visit(*base);
            visit(*addition);
        }
        RecipeKind::Brewing { input, reagent, .. } => {
            visit(input.item);
            visit(reagent.item);
        }
        RecipeKind::FireworkRocket {
            shell, fuel, star, ..
        } => {
            visit(*shell);
            visit(*fuel);
            visit(*star);
        }
        RecipeKind::FireworkStar {
            shapes,
            trail,
            twinkle,
            fuel,
            dye,
            ..
        } => {
            for (_, ingredient) in *shapes {
                visit(*ingredient);
            }
            visit(*trail);
            visit(*twinkle);
            visit(*fuel);
            visit(*dye);
        }
        RecipeKind::MapExtending { map, material, .. } => {
            visit(*map);
            visit(*material);
        }
        RecipeKind::BannerDuplicate { banner, .. } => visit(*banner),
        RecipeKind::BookCloning {
            source, material, ..
        } => {
            visit(*source);
            visit(*material);
        }
        RecipeKind::ShieldDecoration { banner, target, .. } => {
            visit(*banner);
            visit(*target);
        }
        RecipeKind::RepairItem => {}
        RecipeKind::DecoratedPot {
            back,
            left,
            right,
            front,
            ..
        } => {
            visit(*back);
            visit(*left);
            visit(*right);
            visit(*front);
        }
    }
}

#[test]
fn recipe_ids_are_unique_and_indexed() {
    let manager = RecipeManager::global();
    let mut ids = BTreeSet::new();
    for recipe in RECIPES {
        assert!(ids.insert(recipe.id), "配方 ID 重复：{}", recipe.id);
        assert_eq!(manager.recipe(recipe.id), Some(recipe));
    }
    assert_eq!(manager.all().len(), RECIPES.len());
    assert!(!POTIONS.is_empty());
}

#[test]
fn every_recipe_type_lists_every_recipe_once() {
    let manager = RecipeManager::global();
    let types = [
        RecipeType::Crafting,
        RecipeType::Smelting,
        RecipeType::Blasting,
        RecipeType::Smoking,
        RecipeType::CampfireCooking,
        RecipeType::Stonecutting,
        RecipeType::Smithing,
        RecipeType::Brewing,
    ];
    let mut total = 0;
    for kind in types {
        for recipe in manager.recipes_of(kind) {
            assert_eq!(recipe.recipe_type(), kind, "{} 的类型不对", recipe.id);
            total += 1;
        }
    }
    assert_eq!(total, RECIPES.len());
}

#[test]
fn ingredients_point_at_real_items_and_tags() {
    let item_tags = TAG_GROUPS
        .iter()
        .find(|group| group.registry == "minecraft:item")
        .expect("物品标签表不存在");
    for recipe in RECIPES {
        each_ingredient(&recipe.kind, |ingredient| {
            assert!(!ingredient.items.is_empty(), "{} 的原料是空的", recipe.id);
            for &item in ingredient.items {
                assert!(
                    usize::try_from(item).is_ok_and(|id| id < items::ITEMS.len()),
                    "{} 引用了不存在的物品 {item}",
                    recipe.id
                );
            }
            if let Some(name) = ingredient.tag {
                let tag = item_tags
                    .tags
                    .iter()
                    .find(|tag| tag.name == name)
                    .unwrap_or_else(|| panic!("{} 引用了不存在的标签 {name}", recipe.id));
                let mut expected = tag.ids.to_vec();
                expected.sort_unstable();
                expected.dedup();
                assert_eq!(
                    ingredient.items, expected,
                    "{} 的标签 {name} 展开结果不对",
                    recipe.id
                );
            }
        });
    }
}

#[test]
fn every_recipe_matches_its_own_ingredients() {
    for recipe in RECIPES {
        match &recipe.kind {
            RecipeKind::Shaped { width, height, .. } => {
                let input = shaped_grid(&recipe.kind).expect("形状配方");
                assert!(
                    input.matches(recipe),
                    "{} 摆不出自己的 {width}x{height} 图案",
                    recipe.id
                );
                let mirrored = mirror(&input);
                assert!(mirrored.matches(recipe), "{} 不认镜像", recipe.id);
            }
            RecipeKind::Shapeless { .. } => {
                let input = shapeless_grid(&recipe.kind).expect("无序配方");
                assert!(input.matches(recipe), "{} 摆不出自己的原料", recipe.id);
            }
            RecipeKind::Cooking { ingredient, .. }
            | RecipeKind::Stonecutting { ingredient, .. } => {
                let stack = ItemStack::new(ingredient.items[0], 1);
                assert!(stack.matches(recipe), "{} 不认自己的输入", recipe.id);
            }
            RecipeKind::SmithingTransform {
                template,
                base,
                addition,
                ..
            } => {
                let input = SmithingInput::new(
                    optional_stack(*template),
                    ItemStack::new(base.items[0], 1),
                    optional_stack(*addition),
                );
                assert!(input.matches(recipe), "{} 不认自己的输入", recipe.id);
            }
            RecipeKind::SmithingTrim {
                template,
                base,
                addition,
                ..
            } => {
                let input = SmithingInput::new(
                    ItemStack::new(template.items[0], 1),
                    ItemStack::new(base.items[0], 1),
                    ItemStack::new(addition.items[0], 1),
                );
                assert!(input.matches(recipe), "{} 不认自己的输入", recipe.id);
            }
            RecipeKind::Brewing { input, reagent, .. } => {
                let brewing = BrewingInput::new(
                    ItemStack::new(input.item.items[0], 1),
                    input.potions.map(|potions| potions[0]),
                    ItemStack::new(reagent.item.items[0], 1),
                    reagent.potions.map(|potions| potions[0]),
                );
                assert!(brewing.matches(recipe), "{} 不认自己的输入", recipe.id);
            }
            _ => {}
        }
    }
}

fn optional_stack(ingredient: Option<Ingredient>) -> ItemStack {
    ingredient.map_or(ItemStack::empty(), |ingredient| {
        ItemStack::new(ingredient.items[0], 1)
    })
}

/// 把裁剪后的网格左右翻转。
fn mirror(input: &CraftingInput) -> CraftingInput {
    let mut slots = vec![ItemStack::empty(); input.width() * input.height()];
    for y in 0..input.height() {
        for x in 0..input.width() {
            slots[input.width() - 1 - x + y * input.width()] = input.get(x, y).clone();
        }
    }
    CraftingInput::of(input.width(), input.height(), &slots)
}

#[test]
fn manager_prefers_the_hinted_recipe() {
    let manager = RecipeManager::global();
    let recipe = manager.recipe("minecraft:acacia_boat").expect("没有橡木船");
    let input = shaped_grid(&recipe.kind).expect("船是有序配方");
    assert_eq!(
        manager
            .get_recipe_for(RecipeType::Crafting, &input, Some(recipe))
            .map(|found| found.id),
        Some(recipe.id)
    );
    assert_eq!(
        manager
            .get_recipe_for(RecipeType::Crafting, &input, None)
            .map(|found| found.id),
        Some(recipe.id)
    );
}

#[test]
fn empty_input_matches_nothing() {
    let manager = RecipeManager::global();
    let slots: [ItemStack; 9] = std::array::from_fn(|_| ItemStack::empty());
    let empty = CraftingInput::of(3, 3, &slots);
    assert!(empty.is_empty());
    for kind in [
        RecipeType::Crafting,
        RecipeType::Smelting,
        RecipeType::Stonecutting,
    ] {
        assert!(manager.get_recipe_for(kind, &empty, None).is_none());
    }
    let empty_stack = ItemStack::empty();
    assert!(empty_stack.is_empty());
    for kind in [
        RecipeType::Smelting,
        RecipeType::Blasting,
        RecipeType::Smoking,
        RecipeType::CampfireCooking,
        RecipeType::Stonecutting,
    ] {
        assert!(manager.get_recipe_for(kind, &empty_stack, None).is_none());
    }
    let empty_smithing = SmithingInput::new(ItemStack::empty(), ItemStack::empty(), ItemStack::empty());
    assert!(
        manager
            .get_recipe_for(RecipeType::Smithing, &empty_smithing, None)
            .is_none()
    );
    let empty_brewing = BrewingInput::new(ItemStack::empty(), None, ItemStack::empty(), None);
    assert!(
        manager
            .get_recipe_for(RecipeType::Brewing, &empty_brewing, None)
            .is_none()
    );
}

#[test]
fn shield_decoration_matches_a_banner_and_a_plain_shield() {
    let manager = RecipeManager::global();
    let recipe = manager
        .recipe("minecraft:shield_decoration")
        .expect("没有盾牌装饰");
    let input = grid_from_items(&[items::WHITE_BANNER, items::SHIELD]);
    assert!(input.matches(recipe));
    let two_banners = grid_from_items(&[items::WHITE_BANNER, items::WHITE_BANNER]);
    assert!(!two_banners.matches(recipe));
}

#[test]
fn repair_item_needs_two_single_damageable_items() {
    let manager = RecipeManager::global();
    let recipe = manager
        .recipe("minecraft:repair_item")
        .expect("没有修复物品");
    let one_each = grid(&[
        ItemStack::new(items::DIAMOND_PICKAXE, 1),
        ItemStack::new(items::DIAMOND_PICKAXE, 1),
    ]);
    assert!(one_each.matches(recipe));
    let stacked = grid(&[
        ItemStack::new(items::DIAMOND_PICKAXE, 2),
        ItemStack::new(items::DIAMOND_PICKAXE, 2),
    ]);
    assert!(!stacked.matches(recipe));
    let not_damageable = grid_from_items(&[items::STICK, items::STICK]);
    assert!(!not_damageable.matches(recipe));
}

#[test]
fn component_dependent_recipes_never_match_default_stacks() {
    let manager = RecipeManager::global();
    // 默认旗帜没有图案，复制不了。
    let banners = grid_from_items(&[items::WHITE_BANNER, items::WHITE_BANNER]);
    let banner_duplicate = manager
        .recipe("minecraft:white_banner_duplicate")
        .expect("没有白色旗帜复制");
    assert!(!banners.matches(banner_duplicate));

    // 默认成书没有内容，复制不了。
    let book = grid_from_items(&[items::WRITTEN_BOOK, items::PAPER]);
    let book_cloning = manager
        .recipe("minecraft:book_cloning")
        .expect("没有书复制");
    assert!(!book.matches(book_cloning));

    // 默认地图没有 MAP_ID，放大不了。
    let map = grid_from_items(&[
        items::PAPER,
        items::PAPER,
        items::PAPER,
        items::PAPER,
        items::FILLED_MAP,
        items::PAPER,
        items::PAPER,
        items::PAPER,
        items::PAPER,
    ]);
    let map_extending = manager
        .recipe("minecraft:map_extending")
        .expect("没有地图放大");
    assert!(!map.matches(map_extending));
}
