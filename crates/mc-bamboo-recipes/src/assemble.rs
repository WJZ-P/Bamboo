//! 配方的产物组装和制作剩余物，对应官方各个 `Recipe.assemble` 和
//! `CraftingRecipe.getRemainingItems`。
//!
//! 输入的布局由调用方决定，和官方传进 `assemble` 的那个输入对象一致：
//!
//! - 合成类配方（`Shaped`、`Shapeless`、`Dye`、`Transmute`、`Imbue`、烟花、
//!   书复制、旗帜复制、盾牌装饰、修复、装饰罐）：裁掉空行空列后的网格，按行排列；
//! - 烧炼、切石：`[原料]`；
//! - 锻造：`[模板, 基底, 添加物]`；
//! - 酿造：`[输入, 材料]`。
//!
//! 组件补丁按 `mc-bamboo-protocol` 的 `ComponentPatch` 读写。合并顺序和官方
//! `ItemStack` 的构造方法一致：先放模板（或新组件），再把原物品的补丁叠上去，
//! 和默认组件相同的条目会被去掉。

use mc_bamboo_data::{
    FireworkShape, Recipe, RecipeKind, Stack, items,
    registries::{SYNCED_REGISTRIES, data_component_type as kinds},
};
use mc_bamboo_protocol::{
    components::{
        ComponentPatch, ComponentValue, PatchEntry, cursor::Cursor, cursor::Holder, defaults,
        stack::ItemStackTemplate, value::FireworkExplosion,
    },
    item::ItemStack,
};

/// 官方 `DataComponents.DYE` 的默认颜色：白色。
const WHITE_DYE: i32 = 0;
/// 官方 `MapPostProcessing.SCALE`。
const MAP_POST_PROCESSING_SCALE: i32 = 1;
/// 官方 `FireworkExplosion.Shape.SMALL_BALL`。
const SMALL_BALL: i32 = 0;

/// 一组 `(纹理扩散色, 烟花色)`，下标是 `DyeColor` 的 ID（白到黑）。
///
/// 这是官方 `DyeColor` 的常量，和服务端版本绑定；染色和烟花配方要用。
/// 纹理色是 `ARGB.opaque` 之后的值，烟花色本来就是 RGB。
const DYE_COLORS: [(i32, i32); 16] = [
    (0x00F9_FFFE, 0x00F0_F0F0), // white
    (0x00F9_801D, 0x00EB_8844), // orange
    (0x00C7_4EBD, 0x00C3_54CD), // magenta
    (0x003A_B3DA, 0x0066_89D3), // light_blue
    (0x00FE_D83D, 0x00DE_CF2A), // yellow
    (0x0080_C71F, 0x0041_CD34), // lime
    (0x00F3_8BAA, 0x00D8_8198), // pink
    (0x0047_4F52, 0x0043_4343), // gray
    (0x009D_9D97, 0x00AB_ABAB), // light_gray
    (0x0016_9C9C, 0x0028_7697), // cyan
    (0x0089_32B8, 0x007B_2FBE), // purple
    (0x003C_44AA, 0x0025_3192), // blue
    (0x0083_5432, 0x0051_301A), // brown
    (0x005E_7C16, 0x003B_511A), // green
    (0x00B0_2E26, 0x00B3_312C), // red
    (0x001D_1D21, 0x001E_1B1B), // black
];

/// 官方 `ItemStackTemplate.create()`。
fn create(template: Stack) -> ItemStack {
    let patch = template_patch(template.components);
    stack_with(template.item, template.count, patch)
}

/// 官方 `TransmuteRecipe.createWithOriginalComponents`：
/// 用模板的产物，叠上输入物品的组件补丁。
fn create_with_original(
    item: i32,
    count: i32,
    template_components: &[u8],
    input: &ItemStack,
    extra_count: i32,
) -> ItemStack {
    let template = template_patch(template_components);
    let patch = applied(item, &patch_of(input), &template);
    stack_with(item, count + extra_count, patch)
}

/// 一个带组件补丁的物品栈。
fn stack_with(item: i32, count: i32, patch: ComponentPatch) -> ItemStack {
    let (added, removed) = patch.to_opaque();
    ItemStack {
        item,
        count,
        added,
        removed,
    }
}

/// 一个配方模板自带组件补丁的解码。
pub(crate) fn template_patch(bytes: &[u8]) -> ComponentPatch {
    let mut cursor = Cursor::new(bytes);
    ComponentPatch::read(&mut cursor).unwrap_or_default()
}

/// 一个物品栈上的组件补丁，官方的 `ItemStack.getComponentsPatch()`。
///
/// 客户端的组件进背包前会经过 `validate_stack`，正常不会解不开；真的解不开时
/// 当成空补丁，按默认组件算，不让世界线程崩在配方上。
pub(crate) fn patch_of(stack: &ItemStack) -> ComponentPatch {
    ComponentPatch::from_opaque(stack).unwrap_or_default()
}

/// 官方 `PatchedDataComponentMap.applyPatch`：把 `overlay` 叠到 `base` 上。
///
/// 值等于默认组件时条目消失；移除一个本来就没有默认值的组件时也不留条目。
pub(crate) fn applied(
    item: i32,
    base: &ComponentPatch,
    overlay: &ComponentPatch,
) -> ComponentPatch {
    let mut patch = base.clone();
    for (kind, entry) in overlay.entries() {
        match entry {
            PatchEntry::Set(value) => set(item, &mut patch, *kind, Some(value.clone())),
            PatchEntry::Removed => set(item, &mut patch, *kind, None),
        }
    }
    patch
}

/// 官方 `PatchedDataComponentMap.set`：设成默认值等于移除，移除没有默认值的组件是空操作。
fn set(item: i32, patch: &mut ComponentPatch, kind: i32, value: Option<ComponentValue>) {
    let default = defaults::of_item(item).get(kind);
    match value {
        Some(value) => {
            if default == Some(&value) {
                patch.remove(kind);
            } else {
                patch.put(kind, PatchEntry::Set(value));
            }
        }
        None => {
            if default.is_some() {
                patch.put(kind, PatchEntry::Removed);
            } else {
                patch.remove(kind);
            }
        }
    }
}

/// 一个输入格上某个组件的生效值，官方的 `ItemStack.get`。
fn resolved(stack: &ItemStack, kind: i32) -> Option<ComponentValue> {
    crate::matching::resolved(stack, kind)
}

/// 物品栈上 `potion_contents` 里的药水注册表 ID，官方的
/// `stack.get(DataComponents.POTION_CONTENTS).potion().map(Holder::value)`。
///
/// 没有 `potion_contents` 组件时返回 `None`；有组件但没有药水（只是自定义颜色）
/// 也返回 `None`。酿造台用它构造 [`BrewingInput`](crate::BrewingInput)。
pub fn potion_id(stack: &ItemStack) -> Option<i32> {
    match resolved(stack, kinds::POTION_CONTENTS) {
        Some(ComponentValue::PotionContents { potion, .. }) => potion,
        _ => None,
    }
}

/// 官方 `ItemStack.copyWithCount`。
fn copy_with_count(stack: &ItemStack, count: i32) -> ItemStack {
    if stack.is_empty() {
        return ItemStack::empty();
    }
    let mut copy = stack.clone();
    copy.count = count;
    copy
}

/// 组装一个配方的产物。返回 `None` 表示官方的 `ItemStack.EMPTY`。
pub fn assemble(recipe: &Recipe, input: &[ItemStack]) -> Option<ItemStack> {
    match &recipe.kind {
        RecipeKind::Shaped { result, .. }
        | RecipeKind::Shapeless { result, .. }
        | RecipeKind::Cooking { result, .. }
        | RecipeKind::Stonecutting { result, .. }
        | RecipeKind::Brewing { result, .. } => Some(create(*result)),
        RecipeKind::Transmute {
            input: input_ingredient,
            material,
            result_item,
            result_count,
            result_components,
            add_material_count,
            ..
        } => assemble_transmute(
            input,
            *input_ingredient,
            *material,
            *result_item,
            *result_count,
            result_components,
            *add_material_count,
        ),
        RecipeKind::Dye {
            target,
            dye,
            result,
        } => assemble_dye(input, *target, *dye, *result),
        RecipeKind::Imbue { result, .. } => {
            let source = input.get(4)?;
            let mut stack = create(*result);
            let mut patch = patch_of(&stack);
            set(
                stack.item,
                &mut patch,
                kinds::POTION_CONTENTS,
                resolved(source, kinds::POTION_CONTENTS),
            );
            stack = stack_with(stack.item, stack.count, patch);
            Some(stack)
        }
        RecipeKind::SmithingTransform { result, .. } => {
            let base_stack = input.get(1)?;
            Some(create_with_original(
                result.item,
                result.count,
                result.components,
                base_stack,
                0,
            ))
        }
        RecipeKind::SmithingTrim { pattern, .. } => {
            assemble_trim(input.get(1)?, input.get(2)?, pattern)
        }
        RecipeKind::FireworkRocket {
            fuel, star, result, ..
        } => assemble_firework_rocket(input, *fuel, *star, *result),
        RecipeKind::FireworkStar {
            shapes,
            trail,
            twinkle,
            dye,
            result,
            ..
        } => assemble_firework_star(input, shapes, *trail, *twinkle, *dye, *result),
        RecipeKind::FireworkStarFade {
            target,
            dye,
            result,
        } => assemble_firework_star_fade(input, *target, *dye, *result),
        RecipeKind::MapExtending {
            map,
            result_item,
            result_count,
            result_components,
            ..
        } => assemble_map_extending(input, *map, *result_item, *result_count, result_components),
        RecipeKind::BannerDuplicate { banner, result } => {
            let source = input.iter().find(|slot| {
                !slot.is_empty()
                    && banner.test(slot.item)
                    && (1..=6).contains(&banner_pattern_count(slot))
            })?;
            let item = result.item;
            Some(create_with_original(
                item,
                result.count,
                result.components,
                source,
                0,
            ))
        }
        RecipeKind::BookCloning {
            source,
            material,
            result,
            ..
        } => assemble_book_cloning(input, *source, *material, *result),
        RecipeKind::ShieldDecoration {
            banner,
            target,
            result,
        } => assemble_shield(input, *banner, *target, *result),
        RecipeKind::RepairItem => assemble_repair(input),
        RecipeKind::DecoratedPot { result, .. } => assemble_decorated_pot(input, *result),
    }
}

fn assemble_transmute(
    input: &[ItemStack],
    input_ingredient: mc_bamboo_data::Ingredient,
    material: mc_bamboo_data::Ingredient,
    result_item: Option<i32>,
    result_count: i32,
    result_components: &[u8],
    add_material_count: bool,
) -> Option<ItemStack> {
    let mut found_input: Option<&ItemStack> = None;
    let mut material_count = 0;
    for slot in input {
        if slot.is_empty() {
            continue;
        }
        if input_ingredient.test(slot.item) {
            if !add_material_count && found_input.is_some() {
                continue;
            }
            found_input = Some(slot);
        } else if material.test(slot.item) {
            material_count += 1;
        }
    }
    let input_stack = found_input?;
    let extra = if add_material_count {
        material_count
    } else {
        0
    };
    let item = result_item.unwrap_or(input_stack.item);
    Some(create_with_original(
        item,
        result_count,
        result_components,
        input_stack,
        extra,
    ))
}

fn assemble_dye(
    input: &[ItemStack],
    target: mc_bamboo_data::Ingredient,
    dye: mc_bamboo_data::Ingredient,
    result: Stack,
) -> Option<ItemStack> {
    let mut target_stack: Option<&ItemStack> = None;
    let mut dyes = Vec::new();
    for slot in input {
        if slot.is_empty() {
            continue;
        }
        if target.test(slot.item) {
            if target_stack.is_some() {
                return None;
            }
            target_stack = Some(slot);
        } else if dye.test(slot.item) {
            dyes.push(dye_id(slot));
        } else {
            return None;
        }
    }
    let target_stack = target_stack?;
    if dyes.is_empty() {
        return None;
    }

    let current = match resolved(target_stack, kinds::DYED_COLOR) {
        Some(ComponentValue::DyedColor(rgb)) => Some(rgb),
        _ => None,
    };
    let new_dyed_color = apply_dyes(current, &dyes);
    let mut stack = create_with_original(
        result.item,
        result.count,
        result.components,
        target_stack,
        0,
    );
    let mut patch = patch_of(&stack);
    set(
        stack.item,
        &mut patch,
        kinds::DYED_COLOR,
        Some(ComponentValue::DyedColor(new_dyed_color)),
    );
    stack = stack_with(stack.item, stack.count, patch);
    Some(stack)
}

/// 官方 `DyedItemColor.applyDyes`：已有颜色和所有染料一起取平均，再按最亮分量归一。
fn apply_dyes(current: Option<i32>, dyes: &[i32]) -> i32 {
    let mut red_total = 0i64;
    let mut green_total = 0i64;
    let mut blue_total = 0i64;
    let mut intensity_total = 0i64;
    let mut color_count = 0i64;

    let mut add = |rgb: i32| {
        let red = (rgb >> 16) & 0xFF;
        let green = (rgb >> 8) & 0xFF;
        let blue = rgb & 0xFF;
        intensity_total += red.max(green).max(blue) as i64;
        red_total += red as i64;
        green_total += green as i64;
        blue_total += blue as i64;
        color_count += 1;
    };
    if let Some(current) = current {
        add(current);
    }
    for &dye in dyes {
        add(texture_diffuse_color(dye));
    }

    let red = red_total / color_count;
    let green = green_total / color_count;
    let blue = blue_total / color_count;
    let average_intensity = intensity_total as f32 / color_count as f32;
    let result_intensity = red.max(green).max(blue) as f32;
    let scale = |channel: i64| -> i64 {
        if result_intensity == 0.0 {
            0
        } else {
            (channel as f32 * average_intensity / result_intensity) as i64
        }
    };
    let red = scale(red).clamp(0, 255) as i32;
    let green = scale(green).clamp(0, 255) as i32;
    let blue = scale(blue).clamp(0, 255) as i32;
    (red << 16) | (green << 8) | blue
}

fn texture_diffuse_color(dye: i32) -> i32 {
    DYE_COLORS
        .get(dye as usize)
        .map_or(DYE_COLORS[WHITE_DYE as usize].0, |&(diffuse, _)| diffuse)
}

fn firework_color(dye: i32) -> i32 {
    DYE_COLORS
        .get(dye as usize)
        .map_or(DYE_COLORS[WHITE_DYE as usize].1, |&(_, firework)| firework)
}

/// 一个染料物品的 `DYE` 组件，官方 `getOrDefault(DYE, WHITE)`。
fn dye_id(stack: &ItemStack) -> i32 {
    match resolved(stack, kinds::DYE) {
        Some(ComponentValue::Dye(dye)) => dye,
        _ => WHITE_DYE,
    }
}

fn assemble_trim(base: &ItemStack, addition: &ItemStack, pattern: &str) -> Option<ItemStack> {
    let material = match resolved(addition, kinds::PROVIDES_TRIM_MATERIAL) {
        Some(ComponentValue::ProvidesTrimMaterial(material)) => material,
        _ => return None,
    };
    let pattern = Holder::Id(trim_pattern_id(pattern)?);
    let existing = match resolved(base, kinds::TRIM) {
        Some(ComponentValue::Trim { material, pattern }) => Some((material, pattern)),
        _ => None,
    };
    if existing == Some((material.clone(), pattern.clone())) {
        return None;
    }

    let mut stack = copy_with_count(base, 1);
    let mut patch = patch_of(&stack);
    set(
        stack.item,
        &mut patch,
        kinds::TRIM,
        Some(ComponentValue::Trim { material, pattern }),
    );
    stack = stack_with(stack.item, stack.count, patch);
    Some(stack)
}

/// `minecraft:trim_pattern` 同步注册表里的下标，网络上的 holder ID 就是它。
fn trim_pattern_id(name: &str) -> Option<i32> {
    let registry = SYNCED_REGISTRIES
        .iter()
        .find(|registry| registry.name == "minecraft:trim_pattern")?;
    registry
        .entries
        .iter()
        .position(|entry| *entry == name)
        .map(|index| index as i32)
}

fn assemble_firework_rocket(
    input: &[ItemStack],
    fuel: mc_bamboo_data::Ingredient,
    star: mc_bamboo_data::Ingredient,
    result: Stack,
) -> Option<ItemStack> {
    let mut explosions = Vec::new();
    let mut fuel_count = 0;
    for slot in input {
        if slot.is_empty() {
            continue;
        }
        if fuel.test(slot.item) {
            fuel_count += 1;
        } else if star.test(slot.item)
            && let Some(ComponentValue::FireworkExplosion(explosion)) =
                resolved(slot, kinds::FIREWORK_EXPLOSION)
        {
            explosions.push(explosion);
        }
    }

    let mut overlay = ComponentPatch::new();
    set(
        result.item,
        &mut overlay,
        kinds::FIREWORKS,
        Some(ComponentValue::Fireworks {
            flight_duration: fuel_count,
            explosions,
        }),
    );
    let patch = applied(result.item, &overlay, &template_patch(result.components));
    Some(stack_with(result.item, result.count, patch))
}

fn assemble_firework_star(
    input: &[ItemStack],
    shapes: &[(FireworkShape, mc_bamboo_data::Ingredient)],
    trail: mc_bamboo_data::Ingredient,
    twinkle: mc_bamboo_data::Ingredient,
    dye: mc_bamboo_data::Ingredient,
    result: Stack,
) -> Option<ItemStack> {
    let mut shape = SMALL_BALL;
    let mut has_twinkle = false;
    let mut has_trail = false;
    let mut colors = Vec::new();
    for slot in input {
        if slot.is_empty() {
            continue;
        }
        if let Some(found) = shapes
            .iter()
            .find(|(_, ingredient)| ingredient.test(slot.item))
            .map(|(shape, _)| *shape)
        {
            shape = firework_shape_id(found);
        } else if twinkle.test(slot.item) {
            has_twinkle = true;
        } else if trail.test(slot.item) {
            has_trail = true;
        } else if dye.test(slot.item) {
            colors.push(firework_color(dye_id(slot)));
        }
    }

    let mut stack = create(result);
    let mut patch = patch_of(&stack);
    set(
        stack.item,
        &mut patch,
        kinds::FIREWORK_EXPLOSION,
        Some(ComponentValue::FireworkExplosion(FireworkExplosion {
            shape,
            colors,
            fade_colors: Vec::new(),
            has_trail,
            has_twinkle,
        })),
    );
    stack = stack_with(stack.item, stack.count, patch);
    Some(stack)
}

fn firework_shape_id(shape: FireworkShape) -> i32 {
    match shape {
        FireworkShape::SmallBall => 0,
        FireworkShape::LargeBall => 1,
        FireworkShape::Star => 2,
        FireworkShape::Creeper => 3,
        FireworkShape::Burst => 4,
    }
}

fn assemble_firework_star_fade(
    input: &[ItemStack],
    target: mc_bamboo_data::Ingredient,
    dye: mc_bamboo_data::Ingredient,
    result: Stack,
) -> Option<ItemStack> {
    let mut colors = Vec::new();
    let mut target_stack: Option<&ItemStack> = None;
    for slot in input {
        if dye.test(slot.item) {
            colors.push(firework_color(dye_id(slot)));
        } else if target.test(slot.item) {
            target_stack = Some(slot);
        }
    }
    let target_stack = target_stack?;
    if colors.is_empty() {
        return None;
    }

    let mut stack = create_with_original(
        result.item,
        result.count,
        result.components,
        target_stack,
        0,
    );
    let mut patch = patch_of(&stack);
    let mut explosion = match resolved(&stack, kinds::FIREWORK_EXPLOSION) {
        Some(ComponentValue::FireworkExplosion(explosion)) => explosion,
        _ => FireworkExplosion {
            shape: SMALL_BALL,
            colors: Vec::new(),
            fade_colors: Vec::new(),
            has_trail: false,
            has_twinkle: false,
        },
    };
    explosion.fade_colors = colors;
    set(
        stack.item,
        &mut patch,
        kinds::FIREWORK_EXPLOSION,
        Some(ComponentValue::FireworkExplosion(explosion)),
    );
    stack = stack_with(stack.item, stack.count, patch);
    Some(stack)
}

fn assemble_map_extending(
    input: &[ItemStack],
    map: mc_bamboo_data::Ingredient,
    result_item: Option<i32>,
    result_count: i32,
    result_components: &[u8],
) -> Option<ItemStack> {
    let source = input
        .iter()
        .find(|slot| map.test(slot.item) && resolved(slot, kinds::MAP_ID).is_some())?;
    let item = result_item.unwrap_or(source.item);
    let mut stack = create_with_original(item, result_count, result_components, source, 0);
    let mut patch = patch_of(&stack);
    set(
        stack.item,
        &mut patch,
        kinds::MAP_POST_PROCESSING,
        Some(ComponentValue::MapPostProcessing(MAP_POST_PROCESSING_SCALE)),
    );
    stack = stack_with(stack.item, stack.count, patch);
    Some(stack)
}

fn assemble_book_cloning(
    input: &[ItemStack],
    source: mc_bamboo_data::Ingredient,
    material: mc_bamboo_data::Ingredient,
    result: Stack,
) -> Option<ItemStack> {
    let mut count = 0;
    let mut source_stack: Option<&ItemStack> = None;
    for slot in input {
        if slot.is_empty() {
            continue;
        }
        if source.test(slot.item) && resolved(slot, kinds::WRITTEN_BOOK_CONTENT).is_some() {
            if source_stack.is_some() {
                return None;
            }
            source_stack = Some(slot);
        } else if material.test(slot.item) {
            count += 1;
        } else {
            return None;
        }
    }
    let source_stack = source_stack?;
    let content = resolved(source_stack, kinds::WRITTEN_BOOK_CONTENT)?;
    let copied = match content {
        ComponentValue::WrittenBookContent {
            title,
            author,
            generation,
            pages,
            resolved,
        } => ComponentValue::WrittenBookContent {
            title,
            author,
            generation: generation + 1,
            pages,
            resolved,
        },
        _ => return None,
    };

    let mut stack = create_with_original(
        result.item,
        result.count,
        result.components,
        source_stack,
        count - 1,
    );
    let mut patch = patch_of(&stack);
    set(
        stack.item,
        &mut patch,
        kinds::WRITTEN_BOOK_CONTENT,
        Some(copied),
    );
    stack = stack_with(stack.item, stack.count, patch);
    Some(stack)
}

fn assemble_shield(
    input: &[ItemStack],
    banner: mc_bamboo_data::Ingredient,
    target: mc_bamboo_data::Ingredient,
    result: Stack,
) -> Option<ItemStack> {
    let mut patterns = None;
    let mut base_color = WHITE_DYE;
    let mut target_stack: Option<&ItemStack> = None;
    for slot in input {
        if slot.is_empty() {
            continue;
        }
        if banner.test(slot.item) && is_banner_item(slot.item) {
            patterns = resolved(slot, kinds::BANNER_PATTERNS);
            base_color = banner_color(slot.item);
        } else if target.test(slot.item) {
            target_stack = Some(slot);
        }
    }
    let target_stack = target_stack?;

    let mut stack = create_with_original(
        result.item,
        result.count,
        result.components,
        target_stack,
        0,
    );
    let mut patch = patch_of(&stack);
    set(stack.item, &mut patch, kinds::BANNER_PATTERNS, patterns);
    set(
        stack.item,
        &mut patch,
        kinds::BASE_COLOR,
        Some(ComponentValue::BaseColor(base_color)),
    );
    stack = stack_with(stack.item, stack.count, patch);
    Some(stack)
}

fn assemble_repair(input: &[ItemStack]) -> Option<ItemStack> {
    let mut first: Option<&ItemStack> = None;
    let mut second: Option<&ItemStack> = None;
    let mut non_empty = 0;
    for slot in input {
        if slot.is_empty() {
            continue;
        }
        non_empty += 1;
        if first.is_none() {
            first = Some(slot);
        } else if second.is_none() {
            second = Some(slot);
        }
    }
    if non_empty != 2 {
        return None;
    }
    let (first, second) = (first?, second?);
    // 官方 `getItemsToCombine`：同一个物品、各一个、都带耐久组件。
    if first.item != second.item
        || first.count != 1
        || second.count != 1
        || resolved(first, kinds::MAX_DAMAGE).is_none()
        || resolved(second, kinds::MAX_DAMAGE).is_none()
        || resolved(first, kinds::DAMAGE).is_none()
        || resolved(second, kinds::DAMAGE).is_none()
    {
        return None;
    }

    let first_max = max_damage(first)?;
    let second_max = max_damage(second)?;
    let durability = first_max.max(second_max);
    let remaining_1 = first_max - damage_value(first, first_max);
    let remaining_2 = second_max - damage_value(second, second_max);
    let remaining = remaining_1 + remaining_2 + durability * 5 / 100;

    let mut stack = ItemStack::new(first.item, 1);
    let mut patch = ComponentPatch::new();
    set(
        stack.item,
        &mut patch,
        kinds::MAX_DAMAGE,
        Some(ComponentValue::MaxDamage(durability)),
    );
    let damage = (durability - remaining).max(0).min(durability);
    set(
        stack.item,
        &mut patch,
        kinds::DAMAGE,
        Some(ComponentValue::Damage(damage)),
    );

    // 只复制诅咒附魔，官方 `RepairItemRecipe.assemble`。
    //
    // 官方的 `EnchantmentHelper.updateEnchantments` 在新物品上读不到附魔组件时直接返回，
    // 所以只有在结果本来就有这个组件（物品自带默认值）时才写回去。没有诅咒时写的是空表，
    // `ItemStack.set` 发现它等于默认值，补丁里不留「移除」。
    //
    // `updateEnchantments` returns immediately when the new item has no enchantment
    // component, so this only runs when the item carries that component as a default.
    // No curses writes an empty map. `ItemStack.set` sees the default and leaves no removal.
    let enchant_kind = enchantment_component(stack.item);
    if defaults::resolve(stack.item, &patch, enchant_kind).is_some() {
        let first_enchantments = enchantments(first, enchant_kind);
        let second_enchantments = enchantments(second, enchant_kind);
        let mut curses = Vec::new();
        for (enchantment, level) in first_enchantments.iter().chain(&second_enchantments) {
            if !is_curse(*enchantment) {
                continue;
            }
            match curses
                .iter_mut()
                .find(|(existing, _): &&mut (i32, i32)| existing == enchantment)
            {
                Some(existing) => existing.1 = existing.1.max(*level),
                None => curses.push((*enchantment, *level)),
            }
        }
        let value = if enchant_kind == kinds::STORED_ENCHANTMENTS {
            ComponentValue::StoredEnchantments(curses)
        } else {
            ComponentValue::Enchantments(curses)
        };
        set(stack.item, &mut patch, enchant_kind, Some(value));
    }

    stack = stack_with(stack.item, stack.count, patch);
    Some(stack)
}

/// 官方 `EnchantmentHelper.getComponentType`：成书用 `stored_enchantments`，其他用 `enchantments`。
fn enchantment_component(item: i32) -> i32 {
    if item == items::ENCHANTED_BOOK {
        kinds::STORED_ENCHANTMENTS
    } else {
        kinds::ENCHANTMENTS
    }
}

fn max_damage(stack: &ItemStack) -> Option<i32> {
    match resolved(stack, kinds::MAX_DAMAGE) {
        Some(ComponentValue::MaxDamage(value)) => Some(value),
        _ => None,
    }
}

/// 官方 `ItemStack.getDamageValue`：读 `DAMAGE` 再夹在 `0..=max_damage`。
fn damage_value(stack: &ItemStack, max_damage: i32) -> i32 {
    match resolved(stack, kinds::DAMAGE) {
        Some(ComponentValue::Damage(value)) => value.clamp(0, max_damage),
        _ => 0,
    }
}

fn enchantments(stack: &ItemStack, kind: i32) -> Vec<(i32, i32)> {
    match resolved(stack, kind) {
        Some(ComponentValue::Enchantments(entries)) => entries,
        _ => Vec::new(),
    }
}

/// 附魔注册表里的 `binding_curse`、`vanishing_curse`。
fn is_curse(enchantment: i32) -> bool {
    let Some(registry) = SYNCED_REGISTRIES
        .iter()
        .find(|registry| registry.name == "minecraft:enchantment")
    else {
        return false;
    };
    let name = usize::try_from(enchantment)
        .ok()
        .and_then(|index| registry.entries.get(index));
    matches!(
        name,
        Some(&"minecraft:binding_curse") | Some(&"minecraft:vanishing_curse")
    )
}

fn assemble_decorated_pot(input: &[ItemStack], result: Stack) -> Option<ItemStack> {
    let decoration = |index: usize| -> Option<ItemStackTemplate> {
        let slot = input.get(index)?;
        if slot.is_empty() {
            return None;
        }
        Some(ItemStackTemplate {
            item: slot.item,
            count: 1,
            components: patch_of(slot),
        })
    };
    let decorations = ComponentValue::PotDecorations {
        back: decoration(1),
        left: decoration(3),
        right: decoration(5),
        front: decoration(7),
    };

    let mut overlay = ComponentPatch::new();
    set(
        result.item,
        &mut overlay,
        kinds::POT_DECORATIONS,
        Some(decorations),
    );
    let patch = applied(result.item, &overlay, &template_patch(result.components));
    Some(stack_with(result.item, result.count, patch))
}

/// 一个输入格上 `BANNER_PATTERNS` 的层数。
fn banner_pattern_count(stack: &ItemStack) -> usize {
    match resolved(stack, kinds::BANNER_PATTERNS) {
        Some(ComponentValue::BannerPatterns(layers)) => layers.len(),
        _ => 0,
    }
}

fn is_banner_item(item: i32) -> bool {
    usize::try_from(item)
        .ok()
        .and_then(|index| items::ITEMS.get(index))
        .is_some_and(|entry| entry.kind == mc_bamboo_data::ItemKind::Banner)
}

/// 旗帜物品的颜色，官方 `BannerItem.getColor()`。名字是 `<颜色>_banner` 或
/// `<颜色>_wall_banner`，颜色名就是 `DyeColor` 的名字。
fn banner_color(item: i32) -> i32 {
    let Some(entry) = usize::try_from(item)
        .ok()
        .and_then(|index| items::ITEMS.get(index))
    else {
        return WHITE_DYE;
    };
    let name = entry.name.strip_prefix("minecraft:").unwrap_or(entry.name);
    let name = name
        .strip_suffix("_wall_banner")
        .or_else(|| name.strip_suffix("_banner"))
        .unwrap_or(name);
    dye_color_by_name(name).unwrap_or(WHITE_DYE)
}

fn dye_color_by_name(name: &str) -> Option<i32> {
    Some(match name {
        "white" => 0,
        "orange" => 1,
        "magenta" => 2,
        "light_blue" => 3,
        "yellow" => 4,
        "lime" => 5,
        "pink" => 6,
        "gray" => 7,
        "light_gray" => 8,
        "cyan" => 9,
        "purple" => 10,
        "blue" => 11,
        "brown" => 12,
        "green" => 13,
        "red" => 14,
        "black" => 15,
        _ => return None,
    })
}

/// 制作剩余物，对应官方 `CraftingRecipe.getRemainingItems`。
///
/// 返回值长度和输入一致，一格一个；空栈表示这一格没有剩余物。
pub fn remaining_items(recipe: &Recipe, input: &[ItemStack]) -> Vec<ItemStack> {
    match &recipe.kind {
        RecipeKind::BannerDuplicate { .. } => input
            .iter()
            .map(|slot| {
                if slot.is_empty() {
                    ItemStack::empty()
                } else if let Some(remainder) = remainder(slot) {
                    remainder
                } else if banner_pattern_count(slot) > 0 {
                    copy_with_count(slot, 1)
                } else {
                    ItemStack::empty()
                }
            })
            .collect(),
        RecipeKind::BookCloning { .. } => {
            let mut result = Vec::with_capacity(input.len());
            let mut kept_source = false;
            for slot in input {
                if slot.is_empty() {
                    result.push(ItemStack::empty());
                } else if let Some(remainder) = remainder(slot) {
                    result.push(remainder);
                } else if !kept_source && resolved(slot, kinds::WRITTEN_BOOK_CONTENT).is_some() {
                    result.push(copy_with_count(slot, 1));
                    kept_source = true;
                } else {
                    result.push(ItemStack::empty());
                }
            }
            result
        }
        _ => input
            .iter()
            .map(|slot| remainder(slot).unwrap_or_else(ItemStack::empty))
            .collect(),
    }
}

/// 官方 `Item.getCraftingRemainder()`，物品上的 `craftingRemainingItem` 字段。
///
/// 这和 `use_remainder` 组件不是一回事：喝牛奶才看组件，合成和烧燃料看这个字段。
///
/// Vanilla `Item.getCraftingRemainder()`, the item's `craftingRemainingItem` field.
///
/// This is not the `use_remainder` component. Drinking milk reads the component;
/// crafting and fuel consumption read this field.
pub(crate) fn remainder(stack: &ItemStack) -> Option<ItemStack> {
    if stack.is_empty() {
        return None;
    }
    let (item, count, patch) = mc_bamboo_data::crafting_remainders::of(stack.item)?;
    let (added, removed) = if patch.is_empty() {
        (Vec::new(), Vec::new())
    } else {
        let mut cursor = Cursor::new(patch);
        let parsed = ComponentPatch::read(&mut cursor).ok()?;
        if cursor.finish().is_err() {
            return None;
        }
        parsed.to_opaque()
    };
    Some(ItemStack {
        item,
        count,
        added,
        removed,
    })
}
