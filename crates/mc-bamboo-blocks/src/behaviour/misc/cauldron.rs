//! `CauldronBlock`、`LayeredCauldronBlock`、`LavaCauldronBlock` 和官方的 `CauldronInteractions`。
//!
//! 桶的条目不看组件。水瓶、玻璃瓶、可洗染料的物品、旗帜和染色潜影盒按官方那张表处理。
//! 桶和玻璃瓶在改完方块之后放 `BOTTLE_EMPTY`、`BOTTLE_FILL` 或对应的桶音效。洗染料、旗帜和潜影盒官方不出声。
//! 统计和 `gameEvent` 没有对应的系统，这里不发。
//!
//! Bucket entries ignore components. Water bottles, glass bottles, dyeable items, banners and
//! dyed shulker boxes follow the vanilla table. Buckets and glass bottles play `BOTTLE_EMPTY`,
//! `BOTTLE_FILL`, or the matching bucket sound after the block changes. Washing dye, banners
//! and shulker boxes is silent in vanilla. Statistics and `gameEvent` have no system here.

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{
    BlockAncestor, blocks, items, registries::data_component_type as kinds, sounds::sound_event,
    tags::item_tags,
};
use mc_bamboo_protocol::{
    components::{ComponentPatch, ComponentValue, defaults, value::BannerLayer},
    item::ItemStack,
};

use super::speleothem::{cauldron_fill_fluid, stalactite_tip_above};
use crate::{
    behaviour::Behaviour,
    context::UseContext,
    fluid::Fluid,
    interact::InteractionResult,
    item_exchange::{self, ItemExchange},
    state::{BlockState, LEVEL_CAULDRON, block_of_item, item_is_tagged},
    world::World,
};

/// 三个类的 `useItemOn` 和 `tick` 都在父类 `AbstractCauldronBlock` 里，只有查表的内容不同。
pub(super) struct Cauldron;

/// `MAX_FILL_LEVEL`。
const MAX_FILL_LEVEL: i32 = 3;

/// `LayeredCauldronBlock.lowerFillLevel`。
fn lower_fill_level(state: BlockState, world: &mut World, pos: BlockPos) {
    let level = state.get(LEVEL_CAULDRON) - 1;
    let lowered = if level == 0 {
        BlockState(blocks::CAULDRON)
    } else {
        state.with(LEVEL_CAULDRON, level)
    };
    world.set_block_and_update(pos, lowered);
}

/// 炼药锅交互里的声音都是 `BLOCKS`，音量音高都是 1，`except` 是空。
///
/// Cauldron interaction sounds are `BLOCKS`, volume and pitch 1, and `except` is null.
fn play_cauldron_sound(world: &mut World, pos: BlockPos, sound: &str) {
    world.play_block_sound(pos, sound_event(sound), 1.0, 1.0);
}

/// `emptyBucket` 的结果：换成 `filled`，然后放倒空的声音。
///
/// `emptyBucket`: replace the block, then play the empty sound.
fn fill_with(
    filled: BlockState,
    sound: &str,
    world: &mut World,
    pos: BlockPos,
) -> InteractionResult {
    world.set_block_and_update(pos, filled);
    play_cauldron_sound(world, pos, sound);
    InteractionResult::Success
}

/// `fillLavaInteraction`、`fillPowderSnowInteraction`：水下倒不进去，但是吃掉这次点击。
fn fill_with_unless_under_water(
    filled: BlockState,
    sound: &str,
    world: &mut World,
    pos: BlockPos,
) -> InteractionResult {
    if world.is_water_at(pos.above()) {
        InteractionResult::Consume
    } else {
        fill_with(filled, sound, world, pos)
    }
}

/// `fillBucket`：水和细雪要满了才能舀，岩浆总是满的。
fn fill_bucket(state: BlockState, world: &mut World, pos: BlockPos) -> InteractionResult {
    let is_layered = state.is(blocks::WATER_CAULDRON) || state.is(blocks::POWDER_SNOW_CAULDRON);
    if !state.is(blocks::LAVA_CAULDRON)
        && !(is_layered && state.get(LEVEL_CAULDRON) == MAX_FILL_LEVEL)
    {
        return InteractionResult::TryEmptyHandInteraction;
    }
    let sound = if state.is(blocks::LAVA_CAULDRON) {
        "minecraft:item.bucket.fill_lava"
    } else if state.is(blocks::POWDER_SNOW_CAULDRON) {
        "minecraft:item.bucket.fill_powder_snow"
    } else {
        "minecraft:item.bucket.fill"
    };
    fill_with(BlockState(blocks::CAULDRON), sound, world, pos)
}

/// 分层炼药锅灌满时的状态。
///
/// A layered cauldron filled to `MAX_FILL_LEVEL`.
fn full_layered(block: u32) -> BlockState {
    BlockState(block).with(LEVEL_CAULDRON, MAX_FILL_LEVEL)
}

/// `Items.DYED_SHULKER_BOX`：染了色的潜影盒，普通的潜影盒不在里面。
fn is_dyed_shulker_box(item: i32) -> bool {
    item != items::SHULKER_BOX && block_of_item(item).is_a(BlockAncestor::ShulkerBoxBlock)
}

/// 官方 `ItemStack.get`：空物品堆没有组件，没传物品堆时看默认组件。
///
/// `ItemStack.get`: an empty stack has no components. With no stack, use the item defaults.
fn component_of(ctx: &UseContext, kind: i32) -> Option<ComponentValue> {
    let (item, patch) = match ctx.stack {
        Some(stack) if stack.is_empty() => return None,
        Some(stack) => (stack.item, stack.patch()),
        None => (ctx.item, ComponentPatch::new()),
    };
    defaults::resolve(item, &patch, kind).cloned()
}

/// 官方 `PotionContents.is(Potions.WATER)`：是水，而且没有自定义效果。
///
/// `PotionContents.is(Potions.WATER)`: the potion is water and has no custom effects.
fn is_water_potion(ctx: &UseContext) -> bool {
    let Some(water) = item_exchange::water_potion_id() else {
        return false;
    };
    match component_of(ctx, kinds::POTION_CONTENTS) {
        Some(ComponentValue::PotionContents {
            potion: Some(id),
            custom_effects,
            ..
        }) => id == water && custom_effects.is_empty(),
        _ => false,
    }
}

/// 官方 `ItemStack.has(DataComponents.DYED_COLOR)`。
///
/// `ItemStack.has(DataComponents.DYED_COLOR)`.
fn has_dyed_color(ctx: &UseContext) -> bool {
    component_of(ctx, kinds::DYED_COLOR).is_some()
}

/// 官方 `getOrDefault(BANNER_PATTERNS, EMPTY)`。
///
/// `getOrDefault(BANNER_PATTERNS, EMPTY)`.
fn banner_layers(ctx: &UseContext) -> Vec<BannerLayer> {
    match component_of(ctx, kinds::BANNER_PATTERNS) {
        Some(ComponentValue::BannerPatterns(layers)) => layers,
        _ => Vec::new(),
    }
}

/// 调用方传了物品堆时才记账。没有物品堆时方块照样变，物品栏无从改起。
///
/// Record the exchange only when the caller passed a stack. The block still changes without one.
fn record_fill(world: &mut World, ctx: &UseContext, replacement: ItemStack, limit_creative: bool) {
    if ctx.stack.is_some_and(|stack| !stack.is_empty()) {
        world.set_item_exchange(ItemExchange::Fill {
            replacement,
            limit_creative,
        });
    }
}

/// 空锅和水锅上的 `Items.POTION`。满水的水锅直接 `TRY_WITH_EMPTY_HAND`。
///
/// `Items.POTION` on an empty or water cauldron. A full water cauldron returns
/// `TRY_WITH_EMPTY_HAND` immediately.
fn pour_potion(
    state: BlockState,
    world: &mut World,
    ctx: &UseContext,
    pos: BlockPos,
) -> InteractionResult {
    let water = state.is(blocks::WATER_CAULDRON);
    if water && state.get(LEVEL_CAULDRON) == MAX_FILL_LEVEL {
        return InteractionResult::TryEmptyHandInteraction;
    }
    if !state.is(blocks::CAULDRON) && !water {
        return InteractionResult::TryEmptyHandInteraction;
    }
    if !is_water_potion(ctx) {
        return InteractionResult::TryEmptyHandInteraction;
    }
    // 官方 `createFilledResult` 的 `limitCreativeStackSize` 用默认的 true。
    // Vanilla `createFilledResult` keeps the default `limitCreativeStackSize` of true.
    record_fill(world, ctx, ItemStack::new(items::GLASS_BOTTLE, 1), true);
    let filled = if water {
        state.cycle(LEVEL_CAULDRON)
    } else {
        BlockState(blocks::WATER_CAULDRON)
    };
    world.set_block_and_update(pos, filled);
    // `BOTTLE_EMPTY`，在方块换成水之后。
    // `BOTTLE_EMPTY`, after the block becomes water.
    play_cauldron_sound(world, pos, "minecraft:item.bottle.empty");
    InteractionResult::Success
}

/// 水锅上的 `Items.GLASS_BOTTLE`：舀起一瓶水。
///
/// `Items.GLASS_BOTTLE` on a water cauldron: take one bottle of water.
fn fill_bottle(
    state: BlockState,
    world: &mut World,
    ctx: &UseContext,
    pos: BlockPos,
) -> InteractionResult {
    if !state.is(blocks::WATER_CAULDRON) {
        return InteractionResult::TryEmptyHandInteraction;
    }
    record_fill(world, ctx, item_exchange::water_potion_stack(), true);
    lower_fill_level(state, world, pos);
    // `BOTTLE_FILL`，在用掉一层水之后。洗染料、旗帜和潜影盒不出声。
    // `BOTTLE_FILL`, after one level of water is used. Dye, banners and shulker boxes stay silent.
    play_cauldron_sound(world, pos, "minecraft:item.bottle.fill");
    InteractionResult::Success
}

/// `dyedItemIteration`：整堆去掉 `dyed_color`，再减一层水。
///
/// `dyedItemIteration`: remove `dyed_color` from the whole stack, then lower the water.
fn wash_dye(
    state: BlockState,
    world: &mut World,
    ctx: &UseContext,
    pos: BlockPos,
) -> InteractionResult {
    if !has_dyed_color(ctx) {
        return InteractionResult::TryEmptyHandInteraction;
    }
    if ctx.stack.is_some_and(|stack| !stack.is_empty()) {
        world.set_item_exchange(ItemExchange::RemoveDyedColor);
    }
    lower_fill_level(state, world, pos);
    InteractionResult::Success
}

/// `bannerInteraction`：去掉最后一层图案。`limitCreativeStackSize` 是 false。
///
/// `bannerInteraction`: drop the last pattern layer. `limitCreativeStackSize` is false.
fn wash_banner(
    state: BlockState,
    world: &mut World,
    ctx: &UseContext,
    pos: BlockPos,
) -> InteractionResult {
    let layers = banner_layers(ctx);
    if layers.is_empty() {
        return InteractionResult::TryEmptyHandInteraction;
    }
    if let Some(stack) = ctx.stack.filter(|stack| !stack.is_empty()) {
        let mut cleaned = stack.clone();
        cleaned.count = 1;
        let kept = layers[..layers.len() - 1].to_vec();
        item_exchange::set_component(
            &mut cleaned,
            kinds::BANNER_PATTERNS,
            ComponentValue::BannerPatterns(kept),
        );
        record_fill(world, ctx, cleaned, false);
    }
    lower_fill_level(state, world, pos);
    InteractionResult::Success
}

/// `shulkerBoxInteraction`：换成不染色的潜影盒，补丁套到新物品上。
///
/// `shulkerBoxInteraction`: become an undyed shulker box, with this patch applied to it.
fn wash_shulker(
    state: BlockState,
    world: &mut World,
    ctx: &UseContext,
    pos: BlockPos,
    item: i32,
) -> InteractionResult {
    if !is_dyed_shulker_box(item) {
        return InteractionResult::TryEmptyHandInteraction;
    }
    if let Some(stack) = ctx.stack.filter(|stack| !stack.is_empty()) {
        let cleaned = item_exchange::transmute_copy(stack, items::SHULKER_BOX, 1);
        record_fill(world, ctx, cleaned, false);
    }
    lower_fill_level(state, world, pos);
    InteractionResult::Success
}

/// `canReceiveStalactiteDrip`。
pub(super) fn can_receive_drip(state: BlockState, fluid: Fluid) -> bool {
    state.is(blocks::CAULDRON) || state.is(blocks::WATER_CAULDRON) && fluid == Fluid::Water
}

/// `receiveStalactiteDrip`。
fn receive_drip(state: BlockState, world: &mut World, pos: BlockPos, fluid: Fluid) {
    let filled = if !state.is(blocks::CAULDRON) {
        let level = state.get(LEVEL_CAULDRON);
        if level == MAX_FILL_LEVEL {
            return;
        }
        state.with(LEVEL_CAULDRON, level + 1)
    } else if fluid == Fluid::Water {
        BlockState(blocks::WATER_CAULDRON)
    } else {
        BlockState(blocks::LAVA_CAULDRON)
    };
    world.set_block_and_update(pos, filled);
}

impl Behaviour for Cauldron {
    fn use_item_on(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        let pos = ctx.clicked_pos();
        let item = ctx.item;
        // 官方 `Dispatcher.get` 先查标签。WATER 表只有这一个标签，顺序没有别的可能。
        // `Dispatcher.get` checks tags first. The water map has only this tag, so order cannot vary.
        if state.is(blocks::WATER_CAULDRON)
            && item_is_tagged(item, item_tags::CAULDRON_CAN_REMOVE_DYE)
        {
            return wash_dye(state, world, ctx, pos);
        }
        match item {
            items::WATER_BUCKET => fill_with(
                full_layered(blocks::WATER_CAULDRON),
                "minecraft:item.bucket.empty",
                world,
                pos,
            ),
            items::LAVA_BUCKET => fill_with_unless_under_water(
                BlockState(blocks::LAVA_CAULDRON),
                "minecraft:item.bucket.empty_lava",
                world,
                pos,
            ),
            items::POWDER_SNOW_BUCKET => fill_with_unless_under_water(
                full_layered(blocks::POWDER_SNOW_CAULDRON),
                "minecraft:item.bucket.empty_powder_snow",
                world,
                pos,
            ),
            items::BUCKET => fill_bucket(state, world, pos),
            items::POTION => pour_potion(state, world, ctx, pos),
            items::GLASS_BOTTLE => fill_bottle(state, world, ctx, pos),
            item if state.is(blocks::WATER_CAULDRON)
                && item_is_tagged(item, item_tags::BANNERS) =>
            {
                wash_banner(state, world, ctx, pos)
            }
            item if state.is(blocks::WATER_CAULDRON) && is_dyed_shulker_box(item) => {
                wash_shulker(state, world, ctx, pos, item)
            }
            _ => InteractionResult::TryEmptyHandInteraction,
        }
    }

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        let fluid =
            stalactite_tip_above(world, pos).and_then(|tip| cauldron_fill_fluid(world, tip));
        if let Some(fluid) = fluid
            && can_receive_drip(state, fluid)
        {
            receive_drip(state, world, pos, fluid);
        }
    }

    /// 岩浆锅总是满的，官方直接写 3。空锅没有覆盖，读出来是 0。
    fn analog_output_signal(
        &self,
        state: BlockState,
        _world: &World,
        _pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        if state.is_a(BlockAncestor::LayeredCauldronBlock) {
            state.get(LEVEL_CAULDRON)
        } else if state.is_a(BlockAncestor::LavaCauldronBlock) {
            MAX_FILL_LEVEL
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::Hand;
    use mc_bamboo_data::registries::data_component_type as kinds;
    use mc_bamboo_protocol::{
        components::{
            ComponentPatch, ComponentValue, PatchEntry,
            cursor::Holder,
            defaults,
            stack::ItemStackTemplate,
            value::{BannerLayer, EffectDetails, MobEffectInstance},
        },
        item::ItemStack,
    };

    use super::*;
    use crate::{
        behaviour::misc::test_support::tick,
        state::{SPELEOTHEM_THICKNESS, SpeleothemThickness, VERTICAL_DIRECTION},
        testing::{TestLevel, actor_holding, top_hit},
        ticks::BlockTicks,
        use_item_on_with_stack,
    };

    const CAULDRON: BlockPos = BlockPos::new(0, 64, 0);
    const TIP: BlockPos = BlockPos::new(0, 67, 0);

    const DYEABLE: [i32; 6] = [
        items::LEATHER_HELMET,
        items::LEATHER_CHESTPLATE,
        items::LEATHER_LEGGINGS,
        items::LEATHER_BOOTS,
        items::LEATHER_HORSE_ARMOR,
        items::WOLF_ARMOR,
    ];

    struct Clicked {
        result: InteractionResult,
        state: BlockState,
        exchange: Option<ItemExchange>,
    }

    fn water(level: i32) -> BlockState {
        BlockState(blocks::WATER_CAULDRON).with(LEVEL_CAULDRON, level)
    }

    fn click(state: BlockState, stack: Option<&ItemStack>, item: i32) -> Clicked {
        let mut level = TestLevel::new();
        level.set(CAULDRON, state);
        let mut ticks = BlockTicks::new();
        let actor = actor_holding(item);
        let mut ctx = UseContext::new(&actor, Hand::Main, top_hit(CAULDRON));
        ctx.stack = stack;
        if let Some(stack) = stack.filter(|stack| !stack.is_empty()) {
            ctx.item = stack.item;
        }
        let mut world = World::new(&mut level, &mut ticks);
        let result = Cauldron.use_item_on(state, &mut world, &ctx);
        Clicked {
            result,
            state: world.block_state(CAULDRON),
            exchange: world.item_exchange().cloned(),
        }
    }

    fn click_stack(state: BlockState, stack: &ItemStack) -> Clicked {
        click(state, Some(stack), stack.item)
    }

    fn with_component(item: i32, count: i32, kind: i32, value: ComponentValue) -> ItemStack {
        let mut stack = ItemStack::new(item, count);
        item_exchange::set_component(&mut stack, kind, value);
        stack
    }

    fn with_two(
        item: i32,
        count: i32,
        first: (i32, ComponentValue),
        second: (i32, ComponentValue),
    ) -> ItemStack {
        let mut stack = with_component(item, count, first.0, first.1);
        item_exchange::set_component(&mut stack, second.0, second.1);
        stack
    }

    fn water_contents(
        color: Option<i32>,
        effects: Vec<MobEffectInstance>,
        name: Option<String>,
    ) -> ComponentValue {
        ComponentValue::PotionContents {
            potion: item_exchange::water_potion_id(),
            custom_color: color,
            custom_effects: effects,
            custom_name: name,
        }
    }

    fn one_effect() -> MobEffectInstance {
        MobEffectInstance {
            effect: 1,
            details: EffectDetails {
                amplifier: 0,
                duration: 20,
                ambient: false,
                show_particles: true,
                show_icon: true,
                hidden_effect: None,
            },
        }
    }

    fn base_pattern() -> i32 {
        mc_bamboo_data::registries::SYNCED_REGISTRIES
            .iter()
            .find(|registry| registry.name == "minecraft:banner_pattern")
            .and_then(|registry| {
                registry
                    .entries
                    .iter()
                    .position(|name| *name == "minecraft:base")
            })
            .and_then(|id| i32::try_from(id).ok())
            .expect("minecraft:base is a banner pattern")
    }

    fn patterns(colors: &[i32]) -> ComponentValue {
        ComponentValue::BannerPatterns(
            colors
                .iter()
                .map(|&color| BannerLayer {
                    pattern: Holder::Id(base_pattern()),
                    color,
                })
                .collect(),
        )
    }

    fn layer_colors(stack: &ItemStack) -> Vec<i32> {
        match defaults::resolve(stack.item, &stack.patch(), kinds::BANNER_PATTERNS) {
            Some(ComponentValue::BannerPatterns(layers)) => {
                layers.iter().map(|layer| layer.color).collect()
            }
            _ => Vec::new(),
        }
    }

    fn filled(exchange: &Option<ItemExchange>) -> (&ItemStack, bool) {
        match exchange {
            Some(ItemExchange::Fill {
                replacement,
                limit_creative,
            }) => (replacement, *limit_creative),
            other => panic!("expected createFilledResult, got {other:?}"),
        }
    }

    fn assert_unchanged(clicked: &Clicked, state: BlockState) {
        assert_eq!(clicked.result, InteractionResult::TryEmptyHandInteraction);
        assert_eq!(clicked.state, state);
        assert!(clicked.exchange.is_none());
    }

    /// 炼药锅上面隔两格挂着钟乳石，钟乳石上面是 `source`，中间隔着一格石头。
    fn drip_level(cauldron: BlockState, source: BlockState) -> TestLevel {
        let mut level = TestLevel::new();
        level.set(CAULDRON, cauldron);
        let stalactite = BlockState(blocks::POINTED_DRIPSTONE)
            .with(VERTICAL_DIRECTION, Direction::Down)
            .with(SPELEOTHEM_THICKNESS, SpeleothemThickness::Tip);
        level.set(TIP, stalactite);
        level.set(TIP.above(), BlockState(blocks::STONE));
        level.set(TIP.above().above(), source);
        level
    }

    fn dripped(cauldron: u32, source: u32) -> BlockState {
        let mut level = drip_level(BlockState(cauldron), BlockState(source));
        tick(&mut level, CAULDRON);
        level.get(CAULDRON)
    }

    #[test]
    fn water_above_a_stalactite_starts_a_water_cauldron() {
        assert_eq!(
            dripped(blocks::CAULDRON, blocks::WATER),
            BlockState(blocks::WATER_CAULDRON)
        );
    }

    #[test]
    fn mud_counts_as_water() {
        assert_eq!(
            dripped(blocks::CAULDRON, blocks::MUD),
            BlockState(blocks::WATER_CAULDRON)
        );
    }

    #[test]
    fn lava_fills_only_an_empty_cauldron() {
        assert_eq!(
            dripped(blocks::CAULDRON, blocks::LAVA),
            BlockState(blocks::LAVA_CAULDRON)
        );
        assert_eq!(
            dripped(blocks::WATER_CAULDRON, blocks::LAVA),
            BlockState(blocks::WATER_CAULDRON)
        );
    }

    #[test]
    fn water_raises_a_water_cauldron_up_to_full() {
        let level_of = |state: BlockState| state.get(LEVEL_CAULDRON);
        assert_eq!(level_of(dripped(blocks::WATER_CAULDRON, blocks::WATER)), 2);
        let full = BlockState(blocks::WATER_CAULDRON).with(LEVEL_CAULDRON, MAX_FILL_LEVEL);
        let mut level = drip_level(full, BlockState::WATER);
        tick(&mut level, CAULDRON);
        assert_eq!(level.get(CAULDRON), full);
    }

    #[test]
    fn a_solid_block_stops_the_drip() {
        let mut level = drip_level(BlockState(blocks::CAULDRON), BlockState::WATER);
        level.set(CAULDRON.above(), BlockState(blocks::STONE));
        tick(&mut level, CAULDRON);
        assert_eq!(level.get(CAULDRON), BlockState(blocks::CAULDRON));
    }

    #[test]
    fn water_is_the_first_potion() {
        assert_eq!(item_exchange::water_potion_id(), Some(0));
    }

    #[test]
    fn a_water_bottle_fills_an_empty_cauldron_to_one_level() {
        let stack = with_component(
            items::POTION,
            1,
            kinds::POTION_CONTENTS,
            water_contents(None, Vec::new(), None),
        );
        let clicked = click_stack(BlockState(blocks::CAULDRON), &stack);
        assert_eq!(clicked.result, InteractionResult::Success);
        assert_eq!(clicked.state, water(1));
        let (replacement, limit) = filled(&clicked.exchange);
        assert!(limit);
        assert!(replacement.matches(&ItemStack::new(items::GLASS_BOTTLE, 1)));
    }

    #[test]
    fn a_water_bottle_raises_a_water_cauldron_until_it_is_full() {
        let stack = with_component(
            items::POTION,
            4,
            kinds::POTION_CONTENTS,
            water_contents(None, Vec::new(), None),
        );
        let raised = click_stack(water(1), &stack);
        assert_eq!(raised.state, water(2));
        let full = click_stack(water(2), &stack);
        assert_eq!(full.state, water(3));
        assert_unchanged(&click_stack(water(3), &stack), water(3));
    }

    #[test]
    fn water_with_a_custom_colour_or_name_still_counts_as_water() {
        let colored = with_component(
            items::POTION,
            1,
            kinds::POTION_CONTENTS,
            water_contents(Some(0x112233), Vec::new(), None),
        );
        let named = with_component(
            items::POTION,
            1,
            kinds::POTION_CONTENTS,
            water_contents(None, Vec::new(), Some("Spring".to_string())),
        );
        assert_eq!(
            click_stack(BlockState(blocks::CAULDRON), &colored).state,
            water(1)
        );
        assert_eq!(
            click_stack(BlockState(blocks::CAULDRON), &named).state,
            water(1)
        );
    }

    #[test]
    fn other_potions_and_custom_effects_do_not_fill() {
        let empty = BlockState(blocks::CAULDRON);
        let plain = ItemStack::new(items::POTION, 1);
        let mundane = with_component(
            items::POTION,
            1,
            kinds::POTION_CONTENTS,
            ComponentValue::PotionContents {
                potion: Some(1),
                custom_color: None,
                custom_effects: Vec::new(),
                custom_name: None,
            },
        );
        let mixed = with_component(
            items::POTION,
            1,
            kinds::POTION_CONTENTS,
            water_contents(None, vec![one_effect()], None),
        );
        let splash = with_component(
            items::SPLASH_POTION,
            1,
            kinds::POTION_CONTENTS,
            water_contents(None, Vec::new(), None),
        );
        let lingering = with_component(
            items::LINGERING_POTION,
            1,
            kinds::POTION_CONTENTS,
            water_contents(None, Vec::new(), None),
        );
        for stack in [&plain, &mundane, &mixed, &splash, &lingering] {
            assert_unchanged(&click_stack(empty, stack), empty);
            assert_unchanged(&click_stack(water(1), stack), water(1));
        }
    }

    #[test]
    fn a_water_bottle_does_nothing_to_lava_or_powder_snow() {
        let stack = with_component(
            items::POTION,
            1,
            kinds::POTION_CONTENTS,
            water_contents(None, Vec::new(), None),
        );
        let lava = BlockState(blocks::LAVA_CAULDRON);
        let snow = full_layered(blocks::POWDER_SNOW_CAULDRON);
        assert_unchanged(&click_stack(lava, &stack), lava);
        assert_unchanged(&click_stack(snow, &stack), snow);
    }

    #[test]
    fn a_glass_bottle_lowers_every_water_level_and_returns_a_water_potion() {
        let bottle = ItemStack::new(items::GLASS_BOTTLE, 2);
        let cases = [
            (1, BlockState(blocks::CAULDRON)),
            (2, water(1)),
            (3, water(2)),
        ];
        for (level, expect) in cases {
            let clicked = click_stack(water(level), &bottle);
            assert_eq!(clicked.result, InteractionResult::Success);
            assert_eq!(clicked.state, expect);
            let (replacement, limit) = filled(&clicked.exchange);
            assert!(limit);
            assert!(replacement.matches(&item_exchange::water_potion_stack()));
        }
    }

    #[test]
    fn a_glass_bottle_without_a_stack_still_lowers_the_water() {
        let clicked = click(water(3), None, items::GLASS_BOTTLE);
        assert_eq!(clicked.result, InteractionResult::Success);
        assert_eq!(clicked.state, water(2));
        assert!(clicked.exchange.is_none());
    }

    #[test]
    fn a_glass_bottle_does_not_take_from_other_cauldrons() {
        let bottle = ItemStack::new(items::GLASS_BOTTLE, 1);
        let empty = BlockState(blocks::CAULDRON);
        let lava = BlockState(blocks::LAVA_CAULDRON);
        let snow = water_like_snow();
        assert_unchanged(&click_stack(empty, &bottle), empty);
        assert_unchanged(&click_stack(lava, &bottle), lava);
        assert_unchanged(&click_stack(snow, &bottle), snow);
    }

    fn water_like_snow() -> BlockState {
        full_layered(blocks::POWDER_SNOW_CAULDRON)
    }

    #[test]
    fn dyed_items_in_the_tag_lose_their_colour_and_one_level_of_water() {
        for item in DYEABLE {
            let plain = ItemStack::new(item, 1);
            assert_unchanged(&click_stack(water(2), &plain), water(2));
            let dyed = with_two(
                item,
                1,
                (kinds::DYED_COLOR, ComponentValue::DyedColor(0x336699)),
                (
                    kinds::ENCHANTMENTS,
                    ComponentValue::Enchantments(vec![(0, 1)]),
                ),
            );
            let clicked = click_stack(water(1), &dyed);
            assert_eq!(clicked.result, InteractionResult::Success);
            assert_eq!(clicked.state, BlockState(blocks::CAULDRON));
            assert_eq!(clicked.exchange, Some(ItemExchange::RemoveDyedColor));
            let mut washed = dyed.clone();
            item_exchange::wash_dyed_color(&mut washed);
            assert_eq!(washed.count, 1);
            assert!(washed.patch().entry(kinds::DYED_COLOR).is_none());
            assert_eq!(
                washed.patch().get(kinds::ENCHANTMENTS),
                Some(&ComponentValue::Enchantments(vec![(0, 1)]))
            );
        }
    }

    #[test]
    fn dye_outside_the_tag_stays_on() {
        let stick = with_component(
            items::STICK,
            1,
            kinds::DYED_COLOR,
            ComponentValue::DyedColor(1),
        );
        assert_unchanged(&click_stack(water(3), &stick), water(3));
        let helmet = with_component(
            items::LEATHER_HELMET,
            1,
            kinds::DYED_COLOR,
            ComponentValue::DyedColor(1),
        );
        let empty = BlockState(blocks::CAULDRON);
        assert_unchanged(&click_stack(empty, &helmet), empty);
        assert_unchanged(
            &click_stack(BlockState(blocks::LAVA_CAULDRON), &helmet),
            BlockState(blocks::LAVA_CAULDRON),
        );
    }

    #[test]
    fn a_banner_loses_its_last_pattern_and_one_level_of_water() {
        let repair = (kinds::REPAIR_COST, ComponentValue::RepairCost(4));
        let one = with_two(
            items::WHITE_BANNER,
            3,
            (kinds::BANNER_PATTERNS, patterns(&[2])),
            repair.clone(),
        );
        let clicked = click_stack(water(2), &one);
        assert_eq!(clicked.result, InteractionResult::Success);
        assert_eq!(clicked.state, water(1));
        let (replacement, limit) = filled(&clicked.exchange);
        assert!(!limit);
        assert_eq!(replacement.count, 1);
        assert_eq!(replacement.item, items::WHITE_BANNER);
        assert!(replacement.patch().entry(kinds::BANNER_PATTERNS).is_none());
        assert!(layer_colors(replacement).is_empty());
        assert_eq!(
            replacement.patch().get(kinds::REPAIR_COST),
            Some(&ComponentValue::RepairCost(4))
        );

        let two = with_component(
            items::BLACK_BANNER,
            1,
            kinds::BANNER_PATTERNS,
            patterns(&[1, 4]),
        );
        let clicked = click_stack(water(3), &two);
        assert_eq!(clicked.state, water(2));
        let (replacement, _) = filled(&clicked.exchange);
        assert_eq!(layer_colors(replacement), vec![1]);
    }

    #[test]
    fn a_banner_without_patterns_is_left_alone() {
        let plain = ItemStack::new(items::WHITE_BANNER, 1);
        let explicit = with_component(
            items::WHITE_BANNER,
            1,
            kinds::BANNER_PATTERNS,
            patterns(&[]),
        );
        let shield = with_component(items::SHIELD, 1, kinds::BANNER_PATTERNS, patterns(&[1]));
        assert_unchanged(&click_stack(water(2), &plain), water(2));
        assert_unchanged(&click_stack(water(2), &explicit), water(2));
        assert_unchanged(&click_stack(water(2), &shield), water(2));
        let empty = BlockState(blocks::CAULDRON);
        assert_unchanged(
            &click_stack(
                empty,
                &with_component(
                    items::WHITE_BANNER,
                    1,
                    kinds::BANNER_PATTERNS,
                    patterns(&[1]),
                ),
            ),
            empty,
        );
    }

    #[test]
    fn a_dyed_shulker_box_becomes_plain_and_keeps_its_other_components() {
        let contents = ComponentValue::Container(vec![Some(ItemStackTemplate {
            item: items::STONE,
            count: 4,
            components: ComponentPatch::new(),
        })]);
        let dyed = with_two(
            items::WHITE_SHULKER_BOX,
            1,
            (kinds::CONTAINER, contents.clone()),
            (kinds::REPAIR_COST, ComponentValue::RepairCost(7)),
        );
        let clicked = click_stack(water(1), &dyed);
        assert_eq!(clicked.result, InteractionResult::Success);
        assert_eq!(clicked.state, BlockState(blocks::CAULDRON));
        let (replacement, limit) = filled(&clicked.exchange);
        assert!(!limit);
        assert_eq!(replacement.item, items::SHULKER_BOX);
        assert_eq!(replacement.count, 1);
        assert_eq!(replacement.patch().get(kinds::CONTAINER), Some(&contents));
        assert_eq!(
            replacement.patch().get(kinds::REPAIR_COST),
            Some(&ComponentValue::RepairCost(7))
        );
        assert!(replacement.patch().get(kinds::ITEM_NAME).is_none());

        let orange = ItemStack::new(items::ORANGE_SHULKER_BOX, 1);
        assert_eq!(click_stack(water(3), &orange).state, water(2));
    }

    #[test]
    fn a_removed_container_stays_removed_on_the_plain_box() {
        let mut dyed = ItemStack::new(items::WHITE_SHULKER_BOX, 1);
        item_exchange::remove_component(&mut dyed, kinds::CONTAINER);
        let clicked = click_stack(water(2), &dyed);
        let (replacement, _) = filled(&clicked.exchange);
        assert_eq!(replacement.item, items::SHULKER_BOX);
        assert!(matches!(
            replacement.patch().entry(kinds::CONTAINER),
            Some(PatchEntry::Removed)
        ));
    }

    #[test]
    fn a_dyed_shulker_without_a_stack_still_lowers_the_water() {
        let clicked = click(water(2), None, items::ORANGE_SHULKER_BOX);
        assert_eq!(clicked.result, InteractionResult::Success);
        assert_eq!(clicked.state, water(1));
        assert!(clicked.exchange.is_none());
    }

    #[test]
    fn a_plain_shulker_box_is_not_washed() {
        let plain = ItemStack::new(items::SHULKER_BOX, 1);
        assert_unchanged(&click_stack(water(3), &plain), water(3));
        let dyed = ItemStack::new(items::WHITE_SHULKER_BOX, 1);
        let empty = BlockState(blocks::CAULDRON);
        assert_unchanged(&click_stack(empty, &dyed), empty);
        assert_unchanged(
            &click_stack(BlockState(blocks::LAVA_CAULDRON), &dyed),
            BlockState(blocks::LAVA_CAULDRON),
        );
    }

    #[test]
    fn buckets_still_fill_and_empty_without_an_item_exchange() {
        let water_bucket = ItemStack::new(items::WATER_BUCKET, 1);
        let filled_up = click_stack(BlockState(blocks::CAULDRON), &water_bucket);
        assert_eq!(filled_up.result, InteractionResult::Success);
        assert_eq!(filled_up.state, full_layered(blocks::WATER_CAULDRON));
        assert!(filled_up.exchange.is_none());

        let bucket = ItemStack::new(items::BUCKET, 1);
        assert_unchanged(&click_stack(water(2), &bucket), water(2));
        let emptied = click_stack(water(3), &bucket);
        assert_eq!(emptied.result, InteractionResult::Success);
        assert_eq!(emptied.state, BlockState(blocks::CAULDRON));
        assert!(emptied.exchange.is_none());
    }

    #[test]
    fn a_lava_bucket_under_water_is_consumed_and_does_not_fill() {
        let mut level = TestLevel::new();
        let state = BlockState(blocks::CAULDRON);
        level.set(CAULDRON, state);
        level.set(CAULDRON.above(), BlockState::WATER);
        let mut ticks = BlockTicks::new();
        let actor = actor_holding(items::LAVA_BUCKET);
        let ctx = UseContext::new(&actor, Hand::Main, top_hit(CAULDRON));
        let mut world = World::new(&mut level, &mut ticks);
        let result = Cauldron.use_item_on(state, &mut world, &ctx);
        assert_eq!(result, InteractionResult::Consume);
        assert_eq!(world.block_state(CAULDRON), state);
        assert!(world.item_exchange().is_none());
    }

    #[test]
    fn the_outcome_carries_the_bottle_exchange_and_consumes_the_click() {
        let mut level = TestLevel::new();
        level.set(CAULDRON, water(3));
        let actor = actor_holding(items::GLASS_BOTTLE);
        let stack = ItemStack::new(items::GLASS_BOTTLE, 2);
        let outcome = use_item_on_with_stack(
            &mut level,
            &mut BlockTicks::new(),
            &actor,
            Hand::Main,
            &top_hit(CAULDRON),
            &stack,
        );
        assert_eq!(outcome.result, InteractionResult::Success);
        assert!(outcome.placed_at.is_none());
        assert_eq!(level.get(CAULDRON), water(2));
        let (replacement, limit) = filled(&outcome.item_exchange);
        assert!(limit);
        assert!(replacement.matches(&item_exchange::water_potion_stack()));
    }

    #[test]
    fn a_patterned_banner_on_water_does_not_place_the_banner() {
        let mut level = TestLevel::new();
        level.set(CAULDRON, water(1));
        let actor = actor_holding(items::WHITE_BANNER);
        let stack = with_component(
            items::WHITE_BANNER,
            1,
            kinds::BANNER_PATTERNS,
            patterns(&[3]),
        );
        let outcome = use_item_on_with_stack(
            &mut level,
            &mut BlockTicks::new(),
            &actor,
            Hand::Main,
            &top_hit(CAULDRON),
            &stack,
        );
        assert_eq!(outcome.result, InteractionResult::Success);
        assert!(outcome.placed_at.is_none());
        assert_eq!(level.get(CAULDRON), BlockState(blocks::CAULDRON));
    }
}
