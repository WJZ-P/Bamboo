//! `SignBlock` 和它的子类 `StandingSignBlock`、`WallSignBlock`。
//!
//! 告示牌的内容存在世界层，这里只按官方的规则读写它：右键能用的物品、编辑者、看清楚的是哪一面。

use mc_bamboo_core::{
    BlockPos, Direction, Vec3, mth,
    sign::{DyeColor, SignContent, SignText, SignTextSlot},
};
use mc_bamboo_data::{Block, BlockAncestor, items};

use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{
            default_state, is_water_source, lost_wall_support, rotation_degrees, rotation_segment,
            tick_water_if_waterlogged, wall_state_for_placement,
        },
    },
    context::{Actor, PlaceContext, UseContext},
    interact::InteractionResult,
    level::Effect,
    shape::{self, Aabb},
    state::{BlockState, HORIZONTAL_FACING, ROTATION_16, WATERLOGGED},
    world::World,
};

/// 能用在告示牌上的物品，官方实现 `SignApplicator` 的那几个类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Applicator {
    Dye(DyeColor),
    GlowInkSac,
    InkSac,
    Honeycomb,
}

impl Applicator {
    /// `SignApplicator.canApplyToSign`：染料和墨囊要求这一面已经有字，蜜脾不看内容。
    fn can_apply(self, text: &SignText) -> bool {
        match self {
            Self::Honeycomb => true,
            Self::Dye(_) | Self::GlowInkSac | Self::InkSac => text.has_message(),
        }
    }
}

/// `itemStack.getItem() instanceof SignApplicator`。16 种染料在注册表里按颜色顺序连成一串。
fn applicator_of(item: i32) -> Option<Applicator> {
    let dye = item - items::WHITE_DYE;
    if (0..DyeColor::ALL.len() as i32).contains(&dye) {
        return Some(Applicator::Dye(DyeColor::ALL[dye as usize]));
    }
    match item {
        items::GLOW_INK_SAC => Some(Applicator::GlowInkSac),
        items::INK_SAC => Some(Applicator::InkSac),
        items::HONEYCOMB => Some(Applicator::Honeycomb),
        _ => None,
    }
}

/// `SignApplicator.tryApplyToSign`。成功时内容已经写进世界。
fn apply(applicator: Applicator, world: &mut World, pos: BlockPos, slot: SignTextSlot) -> bool {
    match applicator {
        Applicator::Dye(color) => update_text(world, pos, slot, |text| text.with_color(color)),
        Applicator::GlowInkSac => {
            update_text(world, pos, slot, |text| text.with_glowing_text(true))
        }
        Applicator::InkSac => update_text(world, pos, slot, |text| text.with_glowing_text(false)),
        // `HoneycombItem.tryApplyToSign`：上蜡成功后再发 3003 的光照效果。
        Applicator::Honeycomb => {
            if !set_waxed(world, pos, true) {
                return false;
            }
            world.push_effect(Effect::SignWaxed { pos });
            true
        }
    }
}

/// `SignBlockEntity.updateText`：新的一面和旧的一样时返回 `false`，不碰世界。
fn update_text(
    world: &mut World,
    pos: BlockPos,
    slot: SignTextSlot,
    change: impl FnOnce(SignText) -> SignText,
) -> bool {
    let Some(mut sign) = world.sign(pos) else {
        return false;
    };
    let new_text = change(sign.text(slot).clone());
    if new_text == *sign.text(slot) {
        return false;
    }
    *sign.text_mut(slot) = new_text;
    mark_updated(world, pos, sign)
}

/// `SignBlockEntity.setWaxed`：只有真的变了才算成功。
fn set_waxed(world: &mut World, pos: BlockPos, waxed: bool) -> bool {
    let Some(mut sign) = world.sign(pos) else {
        return false;
    };
    if sign.waxed == waxed {
        return false;
    }
    sign.waxed = waxed;
    mark_updated(world, pos, sign)
}

/// `SignBlockEntity.markUpdated`：写下内容，再让这一格重新广播方块和方块实体数据。
fn mark_updated(world: &mut World, pos: BlockPos, sign: SignContent) -> bool {
    let written = world.set_sign(pos, sign);
    if written {
        world.push_effect(Effect::SignChanged { pos });
    }
    written
}

/// `SignBlock.openTextEdit`：登记编辑者，再给这名玩家打开编辑器。
fn open_text_edit(world: &mut World, pos: BlockPos, slot: SignTextSlot, actor: &Actor) {
    let Some(mut sign) = world.sign(pos) else {
        return;
    };
    sign.editor = Some(actor.uuid);
    if world.set_sign(pos, sign) {
        world.push_effect(Effect::OpenSignEditor { pos, slot });
    }
}

/// `SignBlock.otherPlayerIsEditingSign`。
fn other_player_is_editing_sign(sign: &SignContent, actor: &Actor) -> bool {
    sign.editor.is_some_and(|editor| editor != actor.uuid)
}

/// `SignBlockEntity.getSlotPlayerIsFacing`：玩家站在告示牌的这一面，就编辑这一面。
fn slot_player_is_facing(state: BlockState, pos: BlockPos, actor: &Actor) -> SignTextSlot {
    let center = sign_hitbox_center(state, pos);
    let xd = actor.position.x - (f64::from(pos.x) + center.x);
    let zd = actor.position.z - (f64::from(pos.z) + center.z);
    let player_y_rot = (zd.atan2(xd) * 180.0 / f64::from(std::f32::consts::PI)) as f32 - 90.0;
    if mth::degrees_difference_abs(y_rotation_degrees(state), player_y_rot) <= 90.0 {
        SignTextSlot::Front
    } else {
        SignTextSlot::Back
    }
}

/// `SignBlock.getYRotationDegrees`：贴墙的两种看 `FACING`，站着的和吊在顶上的看 16 向的段。
fn y_rotation_degrees(state: BlockState) -> f32 {
    match state.try_get(HORIZONTAL_FACING) {
        Some(facing) => facing.to_yaw(),
        None => rotation_degrees(state.get(ROTATION_16)),
    }
}

/// `SignBlock.getSignHitboxCenterPosition`：只有 `WallSignBlock` 覆盖，
/// 中心取的是贴着墙那一侧的选中框，其余告示牌都是方块的几何中心。
fn sign_hitbox_center(state: BlockState, pos: BlockPos) -> Vec3 {
    if !state.is_a(BlockAncestor::WallSignBlock) {
        return Vec3::new(0.5, 0.5, 0.5);
    }
    let boxes = shape::outline_boxes(state, pos);
    let bounds = boxes
        .iter()
        .copied()
        .reduce(|first, second| {
            Aabb::new(
                first.min_x.min(second.min_x),
                first.min_y.min(second.min_y),
                first.min_z.min(second.min_z),
                first.max_x.max(second.max_x),
                first.max_y.max(second.max_y),
                first.max_z.max(second.max_z),
            )
        })
        .expect("告示牌的选中框不会是空的");
    Vec3::new(
        (bounds.min_x + bounds.max_x) / 2.0,
        (bounds.min_y + bounds.max_y) / 2.0,
        (bounds.min_z + bounds.max_z) / 2.0,
    )
}

/// `SignBlock`。
pub(super) struct Sign;

impl Behaviour for Sign {
    fn update_shape(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        direction: Direction,
        neighbor_pos: BlockPos,
        neighbor: BlockState,
    ) -> BlockState {
        tick_water_if_waterlogged(state, world, pos);
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    /// `SignBlock.useItemOn`：只有染料、发光墨囊、墨囊和蜜脾能用在没上蜡、也没别人在编辑的告示牌上。
    fn use_item_on(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        let pos = ctx.clicked_pos();
        let Some(sign) = world.sign(pos) else {
            return InteractionResult::Pass;
        };
        let Some(applicator) = applicator_of(ctx.item).filter(|_| ctx.actor.may_build) else {
            return InteractionResult::TryEmptyHandInteraction;
        };
        if sign.waxed || other_player_is_editing_sign(&sign, ctx.actor) {
            return InteractionResult::TryEmptyHandInteraction;
        }
        let slot = slot_player_is_facing(state, pos, ctx.actor);
        if !applicator.can_apply(sign.text(slot)) || !apply(applicator, world, pos, slot) {
            return InteractionResult::TryEmptyHandInteraction;
        }
        // `executeClickCommandsIfPresent`、`awardStat`、`gameEvent` 和消耗物品在这里没有对应物。
        InteractionResult::Success
    }

    /// `SignBlock.useWithoutItem`：上蜡的只回一个失败音效，其余情况打开编辑器或者什么也不做。
    fn use_without_item(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        let pos = ctx.clicked_pos();
        let Some(sign) = world.sign(pos) else {
            return InteractionResult::Pass;
        };
        let slot = slot_player_is_facing(state, pos, ctx.actor);
        // `executeClickCommandsIfPresent` 只看每行样式里的点击事件，这里存的都是纯文本。
        if sign.waxed {
            // 官方的 `WAXED_SIGN_INTERACT_FAIL` 音效。
            return InteractionResult::SuccessServer;
        }
        if other_player_is_editing_sign(&sign, ctx.actor)
            || !ctx.actor.may_build
            || !sign.text(slot).has_editable_text()
        {
            return InteractionResult::Pass;
        }
        open_text_edit(world, pos, slot, ctx.actor);
        InteractionResult::SuccessServer
    }

    /// `SignBlock.setPlacedBy`：放好以后直接给放置者打开正面。
    fn set_placed_by(&self, _state: BlockState, world: &mut World, pos: BlockPos, placer: &Actor) {
        let Some(sign) = world.sign(pos) else {
            return;
        };
        if !sign.waxed && sign.front.has_editable_text() {
            open_text_edit(world, pos, SignTextSlot::Front, placer);
        }
    }
}

/// `StandingSignBlock`。
pub(super) struct StandingSign;

impl Behaviour for StandingSign {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let state = default_state(block)
            .with(ROTATION_16, rotation_segment(ctx.rotation() + 180.0))
            .with(WATERLOGGED, is_water_source(world, ctx.clicked_pos()));
        Some(state)
    }

    fn can_survive(&self, _state: BlockState, world: &World, pos: BlockPos) -> bool {
        world.block_state(pos.below()).is_solid()
    }

    fn update_shape(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        direction: Direction,
        neighbor_pos: BlockPos,
        neighbor: BlockState,
    ) -> BlockState {
        if direction == Direction::Down && !state.can_survive(world, pos) {
            return BlockState::AIR;
        }
        Sign.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn use_item_on(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        Sign.use_item_on(state, world, ctx)
    }

    fn use_without_item(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        Sign.use_without_item(state, world, ctx)
    }

    fn set_placed_by(&self, state: BlockState, world: &mut World, pos: BlockPos, placer: &Actor) {
        Sign.set_placed_by(state, world, pos, placer);
    }
}

/// `WallSignBlock`。
pub(super) struct WallSign;

impl Behaviour for WallSign {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let water = is_water_source(world, ctx.clicked_pos());
        wall_state_for_placement(default_state(block), ctx, world)
            .map(|state| state.with(WATERLOGGED, water))
    }

    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        let support = state.get(HORIZONTAL_FACING).opposite();
        world.block_state(pos.relative(support)).is_solid()
    }

    fn update_shape(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        direction: Direction,
        neighbor_pos: BlockPos,
        neighbor: BlockState,
    ) -> BlockState {
        if lost_wall_support(state, world, pos, direction) {
            return BlockState::AIR;
        }
        Sign.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn use_item_on(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        Sign.use_item_on(state, world, ctx)
    }

    fn use_without_item(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        Sign.use_without_item(state, world, ctx)
    }

    fn set_placed_by(&self, state: BlockState, world: &mut World, pos: BlockPos, placer: &Actor) {
        Sign.set_placed_by(state, world, pos, placer);
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::blocks;

    use super::*;
    use crate::{
        BlockHit, Level,
        testing::{TestLevel, actor_holding, top_hit, use_item},
    };

    const FLOOR: BlockPos = BlockPos::new(0, 64, 0);
    const SIGN: BlockPos = BlockPos::new(0, 65, 0);
    const CEILING: BlockPos = BlockPos::new(0, 66, 0);

    /// 脚下是石头、这一格放着 `state` 的世界。
    fn level_with(state: BlockState) -> TestLevel {
        let mut level = TestLevel::new();
        level.set(FLOOR, BlockState(blocks::STONE));
        level.set(SIGN, state);
        level
    }

    /// 给正面写上一行字。
    fn with_front_text(level: &mut TestLevel, text: &str) {
        let mut content = SignContent::BLANK;
        content.front.messages[0] = text.to_owned();
        level.set_sign(SIGN, content);
    }

    /// 站在告示牌南边（正面）或者北边（背面）的玩家。
    fn actor_holding_on(item: i32, south: bool) -> Actor {
        let mut actor = actor_holding(item);
        actor.position = Vec3::new(0.5, 70.0, if south { 2.5 } else { -1.5 });
        actor
    }

    /// 点在 `pos` 底面的中间。
    fn bottom_hit(pos: BlockPos) -> BlockHit {
        BlockHit {
            pos,
            face: Direction::Down,
            location: Vec3::new(
                f64::from(pos.x) + 0.5,
                f64::from(pos.y),
                f64::from(pos.z) + 0.5,
            ),
            inside: false,
        }
    }

    #[test]
    fn every_dye_is_an_applicator_in_colour_order() {
        for (offset, color) in DyeColor::ALL.into_iter().enumerate() {
            let item = items::WHITE_DYE + offset as i32;
            assert_eq!(
                items::ITEMS[item as usize].name,
                format!("minecraft:{}_dye", color.name())
            );
            assert_eq!(applicator_of(item), Some(Applicator::Dye(color)));
        }
        // 染料在注册表里前后都是别的物品。
        assert_eq!(applicator_of(items::COCOA_BEANS), None);
        assert_eq!(applicator_of(items::BONE_MEAL), None);
    }

    #[test]
    fn ink_and_honeycomb_are_the_other_applicators() {
        assert_eq!(applicator_of(items::INK_SAC), Some(Applicator::InkSac));
        assert_eq!(
            applicator_of(items::GLOW_INK_SAC),
            Some(Applicator::GlowInkSac)
        );
        assert_eq!(applicator_of(items::HONEYCOMB), Some(Applicator::Honeycomb));
        assert_eq!(applicator_of(items::AIR), None);
        assert_eq!(applicator_of(items::STONE), None);
    }

    #[test]
    fn wall_signs_measure_from_the_side_against_the_wall() {
        // 默认朝北：牌子贴着南边的墙，选中框是 z 的 14/16 到 16/16、y 的 4.5/16 到 12.5/16。
        let wall = BlockState(blocks::OAK_WALL_SIGN);
        assert_eq!(
            sign_hitbox_center(wall, SIGN),
            Vec3::new(0.5, 8.5 / 16.0, 15.0 / 16.0)
        );
        // 站着的告示牌没有覆盖，取方块中心。
        assert_eq!(
            sign_hitbox_center(BlockState(blocks::OAK_SIGN), SIGN),
            Vec3::new(0.5, 0.5, 0.5)
        );
    }

    #[test]
    fn the_front_is_the_side_the_sign_faces() {
        // 站着的告示牌默认是段 8，正面朝北。
        let north_front = BlockState(blocks::OAK_SIGN);
        assert_eq!(
            slot_player_is_facing(north_front, SIGN, &actor_holding_on(items::AIR, false)),
            SignTextSlot::Front
        );
        assert_eq!(
            slot_player_is_facing(north_front, SIGN, &actor_holding_on(items::AIR, true)),
            SignTextSlot::Back
        );

        // 段 0 反过来朝南。
        let south_front = north_front.with(ROTATION_16, 0);
        assert_eq!(
            slot_player_is_facing(south_front, SIGN, &actor_holding_on(items::AIR, true)),
            SignTextSlot::Front
        );
        assert_eq!(
            slot_player_is_facing(south_front, SIGN, &actor_holding_on(items::AIR, false)),
            SignTextSlot::Back
        );
    }

    #[test]
    fn wall_signs_use_their_facing_for_the_front() {
        let north = BlockState(blocks::OAK_WALL_SIGN);
        assert_eq!(
            slot_player_is_facing(north, SIGN, &actor_holding_on(items::AIR, false)),
            SignTextSlot::Front
        );
        assert_eq!(
            slot_player_is_facing(north, SIGN, &actor_holding_on(items::AIR, true)),
            SignTextSlot::Back
        );

        let south = north.with(HORIZONTAL_FACING, Direction::South);
        assert_eq!(
            slot_player_is_facing(south, SIGN, &actor_holding_on(items::AIR, true)),
            SignTextSlot::Front
        );
    }

    #[test]
    fn a_dye_recolours_the_side_in_front_of_the_player() {
        let mut level = level_with(BlockState(blocks::OAK_SIGN));
        with_front_text(&mut level, "hi");
        let outcome = use_item(
            &mut level,
            &actor_holding_on(items::RED_DYE, false),
            top_hit(SIGN),
        );
        assert_eq!(outcome.result, InteractionResult::Success);
        assert_eq!(outcome.effects, [Effect::SignChanged { pos: SIGN }]);

        let content = level.sign(SIGN).expect("告示牌");
        assert_eq!(content.front.color, DyeColor::Red);
        assert_eq!(content.back.color, DyeColor::Black);
    }

    #[test]
    fn a_dye_on_a_blank_sign_opens_the_editor_instead() {
        let mut level = level_with(BlockState(blocks::OAK_SIGN));
        let outcome = use_item(
            &mut level,
            &actor_holding_on(items::RED_DYE, false),
            top_hit(SIGN),
        );
        assert_eq!(outcome.result, InteractionResult::SuccessServer);
        assert_eq!(
            outcome.effects,
            [Effect::OpenSignEditor {
                pos: SIGN,
                slot: SignTextSlot::Front
            }]
        );
        assert_eq!(level.sign(SIGN).expect("告示牌").editor, Some([0; 16]));
    }

    #[test]
    fn glow_ink_makes_written_text_glow_and_ink_takes_it_back() {
        let mut level = level_with(BlockState(blocks::OAK_SIGN));
        with_front_text(&mut level, "hi");

        let glow = use_item(
            &mut level,
            &actor_holding_on(items::GLOW_INK_SAC, false),
            top_hit(SIGN),
        );
        assert_eq!(glow.result, InteractionResult::Success);
        assert!(level.sign(SIGN).expect("告示牌").front.glowing);

        let ink = use_item(
            &mut level,
            &actor_holding_on(items::INK_SAC, false),
            top_hit(SIGN),
        );
        assert_eq!(ink.result, InteractionResult::Success);
        assert!(!level.sign(SIGN).expect("告示牌").front.glowing);
    }

    #[test]
    fn ink_on_text_that_does_not_glow_falls_through_to_the_editor() {
        let mut level = level_with(BlockState(blocks::OAK_SIGN));
        with_front_text(&mut level, "hi");
        // `updateText` 没改动时 `tryApplyToSign` 返回假，官方接着当空手再试一次。
        let outcome = use_item(
            &mut level,
            &actor_holding_on(items::INK_SAC, false),
            top_hit(SIGN),
        );
        assert_eq!(outcome.result, InteractionResult::SuccessServer);
        assert_eq!(
            outcome.effects,
            [Effect::OpenSignEditor {
                pos: SIGN,
                slot: SignTextSlot::Front
            }]
        );
    }

    #[test]
    fn honeycomb_waxes_any_sign_and_then_refuses_to_wax_again() {
        let mut level = level_with(BlockState(blocks::OAK_SIGN));
        let outcome = use_item(
            &mut level,
            &actor_holding_on(items::HONEYCOMB, false),
            top_hit(SIGN),
        );
        assert_eq!(outcome.result, InteractionResult::Success);
        assert_eq!(
            outcome.effects,
            [
                Effect::SignChanged { pos: SIGN },
                Effect::SignWaxed { pos: SIGN }
            ]
        );
        assert!(level.sign(SIGN).expect("告示牌").waxed);

        let again = use_item(
            &mut level,
            &actor_holding_on(items::HONEYCOMB, false),
            top_hit(SIGN),
        );
        assert_eq!(again.result, InteractionResult::SuccessServer);
        assert!(again.effects.is_empty());
    }

    #[test]
    fn a_waxed_sign_ignores_dye_and_only_plays_the_failure_sound() {
        let mut level = level_with(BlockState(blocks::OAK_SIGN));
        with_front_text(&mut level, "hi");
        let mut content = level.sign(SIGN).expect("告示牌");
        content.waxed = true;
        level.set_sign(SIGN, content);

        let outcome = use_item(
            &mut level,
            &actor_holding_on(items::RED_DYE, false),
            top_hit(SIGN),
        );
        assert_eq!(outcome.result, InteractionResult::SuccessServer);
        assert!(outcome.effects.is_empty());
        assert_eq!(
            level.sign(SIGN).expect("告示牌").front.color,
            DyeColor::Black
        );
    }

    #[test]
    fn someone_elses_editor_blocks_dye_and_the_editor() {
        let mut level = level_with(BlockState(blocks::OAK_SIGN));
        with_front_text(&mut level, "hi");
        let mut content = level.sign(SIGN).expect("告示牌");
        content.editor = Some([1; 16]);
        level.set_sign(SIGN, content);

        let dye = use_item(
            &mut level,
            &actor_holding_on(items::RED_DYE, false),
            top_hit(SIGN),
        );
        assert_eq!(dye.result, InteractionResult::Pass);
        let empty = use_item(
            &mut level,
            &actor_holding_on(items::AIR, false),
            top_hit(SIGN),
        );
        assert_eq!(empty.result, InteractionResult::Pass);
        assert_eq!(level.sign(SIGN).expect("告示牌").editor, Some([1; 16]));
    }

    #[test]
    fn adventure_players_cannot_apply_dye_or_open_the_editor() {
        let mut level = level_with(BlockState(blocks::OAK_SIGN));
        with_front_text(&mut level, "hi");

        let mut dye = actor_holding_on(items::RED_DYE, false);
        dye.may_build = false;
        assert_eq!(
            use_item(&mut level, &dye, top_hit(SIGN)).result,
            InteractionResult::Pass
        );

        let mut empty = actor_holding_on(items::AIR, false);
        empty.may_build = false;
        assert_eq!(
            use_item(&mut level, &empty, top_hit(SIGN)).result,
            InteractionResult::Pass
        );
    }

    #[test]
    fn placing_a_sign_opens_the_front_face_for_its_placer() {
        let mut level = TestLevel::new();
        level.set(FLOOR, BlockState(blocks::STONE));
        let outcome = use_item(&mut level, &actor_holding(items::OAK_SIGN), top_hit(FLOOR));
        assert_eq!(outcome.result, InteractionResult::Success);
        assert_eq!(
            outcome.effects,
            [Effect::OpenSignEditor {
                pos: SIGN,
                slot: SignTextSlot::Front
            }]
        );
        assert_eq!(level.sign(SIGN).expect("告示牌").editor, Some([0; 16]));
    }

    #[test]
    fn placing_a_hanging_sign_opens_the_editor_too() {
        let mut level = TestLevel::new();
        level.set(FLOOR, BlockState(blocks::STONE));
        level.set(CEILING, BlockState(blocks::STONE));
        let outcome = use_item(
            &mut level,
            &actor_holding(items::OAK_HANGING_SIGN),
            bottom_hit(CEILING),
        );
        assert_eq!(outcome.result, InteractionResult::Success);
        assert_eq!(
            outcome.effects,
            [Effect::OpenSignEditor {
                pos: SIGN,
                slot: SignTextSlot::Front
            }]
        );
    }
}
