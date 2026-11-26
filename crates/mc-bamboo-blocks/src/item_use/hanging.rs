//! `HangingEntityItem` / `ItemFrameItem` 的 `useOn`：放置画、物品展示框和荧光物品展示框。
//!
//! `HangingEntityItem.useOn` and `ItemFrameItem`: placing paintings, item frames
//! and glow item frames.

use mc_bamboo_core::Direction;
use mc_bamboo_data::{items, sounds};

use crate::{
    context::UseContext,
    entity::{
        Entity, EntityKind,
        hanging::{
            create_painting, item_frame_survives, painting_survives, recalculate_painting,
            stack_custom_name, stack_painting_variant,
        },
    },
    interact::InteractionResult,
    item_exchange::ItemExchange,
    world::World,
};

/// `HangingEntityItem.mayPlace` / `ItemFrameItem.mayPlace`。
fn may_place(world: &World, ctx: &UseContext, face: Direction, is_painting: bool) -> bool {
    let place_pos = ctx.clicked_pos().relative(face);
    if is_painting {
        if face.axis().is_vertical() {
            return false;
        }
    } else if !world.is_in_world_bounds(place_pos) {
        return false;
    }
    ctx.actor.may_build
}

/// `HangingEntityItem.useOn`：画、物品展示框、荧光物品展示框共用入口。
pub(super) fn use_on(world: &mut World, ctx: &UseContext) -> InteractionResult {
    let clicked = ctx.clicked_pos();
    let face = ctx.clicked_face();
    let place_pos = clicked.relative(face);
    let is_painting = ctx.item == items::PAINTING;
    if !may_place(world, ctx, face, is_painting) {
        return InteractionResult::Fail;
    }

    let mut entity = if is_painting {
        let Some(painting) = create_painting(world, place_pos, face) else {
            return InteractionResult::Consume;
        };
        painting
    } else {
        let glowing = ctx.item == items::GLOW_ITEM_FRAME;
        world.prepare_entity(Entity::item_frame(place_pos, face, glowing))
    };

    // `EntityType.createDefaultStackConfig`：把物品上的 `CUSTOM_NAME` / `PAINTING_VARIANT`
    // 应用到新实体上（画的显式变体可能更大，所以下面还要再跑一次 `survives()`）。
    if let Some(stack) = ctx.stack {
        let custom_name = stack_custom_name(stack);
        match &mut entity.kind {
            EntityKind::Painting(p) => {
                if let Some(name) = custom_name {
                    p.custom_name = Some(name);
                }
                if let Some(variant) = stack_painting_variant(stack) {
                    p.variant = variant;
                    recalculate_painting(&mut entity);
                }
            }
            EntityKind::ItemFrame(f) => {
                if let Some(name) = custom_name {
                    f.custom_name = Some(name);
                }
            }
            _ => {}
        }
    }

    let survives = match &entity.kind {
        EntityKind::Painting(p) => painting_survives(world, &entity, p),
        EntityKind::ItemFrame(f) => item_frame_survives(world, &entity, f),
        _ => false,
    };
    if !survives {
        return InteractionResult::Consume;
    }

    let (position, sound) = match &entity.kind {
        EntityKind::Painting(_) => (
            entity.position,
            sounds::sound_event("minecraft:entity.painting.place"),
        ),
        EntityKind::ItemFrame(f) => (entity.position, f.place_sound()),
        _ => return InteractionResult::Pass,
    };
    world.play_sound(position, sound, sounds::NEUTRAL, 1.0, 1.0, false);
    world.add_prepared_entity(entity);
    world.set_item_exchange(ItemExchange::Shrink);
    InteractionResult::Success
}
