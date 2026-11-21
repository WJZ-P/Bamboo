//! 羊：羊毛颜色和自然颜色、剪毛、染色、吃草长回羊毛、繁殖时的颜色混合。
//!
//! Sheep: wool colour and natural spawn colours, shearing, dyeing, regrowing wool by
//! eating grass, and the colour mix of bred lambs.

use mc_bamboo_core::sign::DyeColor;
use mc_bamboo_data::{
    entity_types, items,
    recipe::{Ingredient, RecipeKind},
    registries::{attribute, data_component_type as kinds},
    tags::item_tags,
};
use mc_bamboo_protocol::{
    components::{ComponentValue, defaults},
    item::ItemStack,
};

use super::{
    CowVariant, Mob, MobEvent, MobType,
    attributes::Attributes,
    goals::{Goal, GoalSelector, WrappedGoal},
    interact::{MobInteractRequest, MobInteraction},
};
use crate::{InteractionResult, entity::Entity, random::LegacyRandom, state::item_is_tagged};

/// `DATA_WOOL_ID` 的第 5 位：已剪毛。低 4 位是颜色。
///
/// Bit 4 of `DATA_WOOL_ID` marks a sheared sheep; the low four bits hold the colour.
pub const SHEARED_BIT: u8 = 16;

/// 成年和幼年的（宽，高，眼高）。成年眼高是 `EntityTypes.SHEEP` 的 `eyeHeight(1.235F)`，
/// 幼年是 `Sheep.BABY_DIMENSIONS`。
///
/// Adult and baby (width, height, eye height): `eyeHeight(1.235F)` on the entity type and
/// `Sheep.BABY_DIMENSIONS`.
pub(super) const fn dimensions(baby: bool) -> (f32, f32, f32) {
    if baby {
        (0.45, 0.65, 0.656_25)
    } else {
        let facts = &entity_types::FACTS[MobType::Sheep.entity_type() as usize];
        (facts.width, facts.height, 1.235)
    }
}

/// `Sheep.createAttributes`：生命 8，速度 `0.23F`。
///
/// `Sheep.createAttributes`: 8 health and a `0.23F` movement speed.
pub(super) fn attributes() -> Attributes {
    let mut attributes = Attributes::cow();
    attributes.set_base(attribute::MAX_HEALTH, 8.0);
    attributes.set_base(attribute::MOVEMENT_SPEED, f64::from(0.23_f32));
    attributes
}

/// `Sheep.registerGoals`：牛的那一组换速度，跟随父母之后插 `EatBlockGoal`（优先级 5）。
///
/// `Sheep.registerGoals`: the cow set with sheep speeds and `EatBlockGoal` at priority 5.
pub(super) fn goals(navigation: &mut super::navigation::Navigation) -> GoalSelector {
    let mut selector = GoalSelector::cow(navigation);
    for wrapped in &mut selector.goals {
        if wrapped.priority >= 5 {
            wrapped.priority += 1;
        }
        match &mut wrapped.goal {
            Goal::Panic { speed, .. } => *speed = 1.25,
            Goal::Tempt { speed, .. } => *speed = 1.1,
            Goal::FollowParent { speed, .. } => *speed = 1.1,
            _ => {}
        }
    }
    let at = selector
        .goals
        .iter()
        .position(|wrapped| wrapped.priority > 5)
        .unwrap_or(selector.goals.len());
    selector.goals.insert(
        at,
        WrappedGoal {
            priority: 5,
            running: false,
            goal: Goal::EatBlock { eat_tick: 0 },
        },
    );
    selector
}

/// `Sheep.getColor`。
pub fn color(mob: &Mob) -> DyeColor {
    DyeColor::from_id(i32::from(mob.wool & 15))
}

/// `Sheep.setColor`：只换低 4 位。
///
/// `Sheep.setColor`: replaces only the low four bits.
pub fn set_color(mob: &mut Mob, color: DyeColor) {
    mob.wool = (mob.wool & 240) | (color.id() as u8 & 15);
}

/// `Sheep.isSheared`。
pub const fn is_sheared(mob: &Mob) -> bool {
    mob.wool & SHEARED_BIT != 0
}

/// `Sheep.setSheared`。
pub fn set_sheared(mob: &mut Mob, sheared: bool) {
    if sheared {
        mob.wool |= SHEARED_BIT;
    } else {
        mob.wool &= !SHEARED_BIT;
    }
}

/// `SheepColorSpawnRules.getSheepColor`：按群系选一组权重，先抽外层（总权重 100），
/// 抽到常见色时再按 499:1 抽一次粉色。
///
/// `SheepColorSpawnRules.getSheepColor`: one draw over the outer weights (100 total) and,
/// for the common colour, a second 499:1 draw against pink.
pub(super) fn spawn_color(
    climate: CowVariant,
    random: &dyn crate::random::RandomSource,
) -> DyeColor {
    use DyeColor::{Black, Brown, Gray, LightGray, White};
    let (singles, common) = match climate {
        CowVariant::Temperate => ([(Black, 5), (Gray, 5), (LightGray, 5), (Brown, 3)], White),
        CowVariant::Warm => ([(Gray, 5), (LightGray, 5), (White, 5), (Black, 3)], Brown),
        CowVariant::Cold => ([(LightGray, 5), (Gray, 5), (White, 5), (Brown, 3)], Black),
    };
    let mut selection = random.next_int(100);
    for (color, weight) in singles {
        if selection < weight {
            return color;
        }
        selection -= weight;
    }
    if random.next_int(500) < 499 {
        common
    } else {
        DyeColor::Pink
    }
}

/// `Sheep.ate`：`Mob.ate` 只发游戏事件；长回羊毛，幼年 `ageUp(60)`。
///
/// `Sheep.ate`: regrow wool, and a baby that can age up gains 60 seconds.
pub(super) fn ate(me: &mut Entity, mob: &mut Mob) {
    set_sheared(mob, false);
    if mob.is_baby() && !mob.age_locked {
        super::interact::age_up(me, mob, 60, false);
    }
}

/// 一堆物品的 `DYE` 组件（染料物品默认带）。
///
/// The `DYE` component of a stack; dye items carry it by default.
fn dye_of(stack: &ItemStack) -> Option<DyeColor> {
    let patch = stack.patch();
    match defaults::resolve(stack.item, &patch, kinds::DYE) {
        Some(ComponentValue::Dye(id)) => Some(DyeColor::from_id(*id)),
        _ => None,
    }
}

/// 剪刀、染料和食物以外的交互：`Sheep.mobInteract` 的剪刀分支在
/// `Animal.mobInteract` 之前，染料是之后 `DyeItem.interactLivingEntity` 的那一步。
///
/// Shears come before `Animal.mobInteract` in `Sheep.mobInteract`; a dye is the later
/// `DyeItem.interactLivingEntity` step.
pub(super) fn interact_shears(mob: &Mob, req: &MobInteractRequest) -> Option<MobInteraction> {
    if req.held.is_empty() || req.held.item != items::SHEARS {
        return None;
    }
    if is_sheared(mob) || mob.is_baby() {
        // 服务端也返回 `CONSUME`：挥手但什么都不做。
        // The server returns `CONSUME` too: the action is consumed with no effect.
        return Some(MobInteraction {
            result: InteractionResult::Consume,
            ..MobInteraction::pass()
        });
    }
    Some(MobInteraction {
        result: InteractionResult::SuccessServer,
        shear: true,
        ride: false,

        ..MobInteraction::pass()
    })
}

/// `DyeItem.interactLivingEntity`：没剪过毛、颜色不同才染；声音由玩家发出（`playSound(player, ..)`
/// 不发给这名玩家自己）。
///
/// `DyeItem.interactLivingEntity`: only an unsheared sheep of another colour is dyed. The
/// sound skips the dyeing player.
pub(super) fn interact_dye(me: &Entity, mob: &mut Mob, req: &MobInteractRequest) -> MobInteraction {
    let Some(dye) = dye_of(&req.held).filter(|_| !req.held.is_empty()) else {
        return MobInteraction::pass();
    };
    if mob.is_dead_or_dying() || is_sheared(mob) || color(mob) == dye {
        return MobInteraction::pass();
    }
    mob.events.push(MobEvent::PlayerSound {
        sound: "minecraft:item.dye.use",
        position: me.position,
    });
    set_color(mob, dye);
    MobInteraction {
        result: InteractionResult::Success,
        shrink_held: !req.creative,
        ..MobInteraction::pass()
    }
}

/// `Sheep.shear` 加上 `mobInteract` 之后的 `hurtAndBreak(1)`：音效、剪羊毛战利品表、每团羊毛
/// 一个物品实体（高 1 格）加羊自己随机源的五个 `nextFloat` 速度，最后标成已剪。
///
/// `Sheep.shear` plus the later `hurtAndBreak(1)`: the sound, the shearing loot table, one
/// item per wool at +1 height with five `nextFloat` draws from the sheep, then sheared.
pub(crate) fn apply_shear(
    world: &mut crate::world::World,
    entity: &mut Entity,
    hand: mc_bamboo_core::Hand,
    tool: &ItemStack,
) {
    let Some(request) = entity.with_mob(|me, mob| {
        mob.events.push(MobEvent::PlayerSound {
            sound: "minecraft:entity.sheep.shear",
            position: me.position,
        });
        super::EntityLootRequest::new("minecraft:shearing/sheep", me, mob, Some(tool.clone()))
    }) else {
        return;
    };
    let position = request.origin;
    let drops = world.entity_loot_drops(request);
    let at = position.add(mc_bamboo_core::Vec3::new(0.0, 1.0, 0.0));
    for drop in drops {
        for _ in 0..drop.count {
            let mut single = drop.clone();
            single.count = 1;
            let Some(id) = world.spawn_at_location(at, single) else {
                continue;
            };
            let random = &mut entity.random;
            let dx = (random.next_float() - random.next_float()) * 0.1;
            let dy = random.next_float() * 0.05;
            let dz = (random.next_float() - random.next_float()) * 0.1;
            world.with_entity_mut(id, &mut |item| {
                item.delta = item.delta.add(mc_bamboo_core::Vec3::new(
                    f64::from(dx),
                    f64::from(dy),
                    f64::from(dz),
                ));
            });
        }
    }
    entity.with_mob(|_, mob| set_sheared(mob, true));
    crate::durability::hurt_hand(world, hand, 1);
}

/// `Sheep.isFood`。
pub(super) fn is_food(item: i32) -> bool {
    item_is_tagged(item, item_tags::SHEEP_FOOD)
}

/// `DyeColor.findColorMixInRecipes`：两种染料（2×1）在合成配方里能合出的染料颜色。
/// 对每种颜色只有一个染料物品，所以就是查第一个匹配的配方。
///
/// `DyeColor.findColorMixInRecipes`: the dye colour a 2×1 crafting grid of the two dyes
/// makes. Each colour has one dye item, so this is the first matching recipe.
pub fn mixed_color(first: DyeColor, second: DyeColor) -> Option<DyeColor> {
    let dyed = mc_bamboo_data::recipes::DYED;
    let (a, b) = (dyed[first.id() as usize], dyed[second.id() as usize]);
    mc_bamboo_data::recipes::RECIPES.iter().find_map(|recipe| {
        let result = match &recipe.kind {
            RecipeKind::Shapeless {
                ingredients,
                result,
            } if two_match(
                ingredients.first(),
                ingredients.get(1),
                ingredients.len(),
                a,
                b,
            ) =>
            {
                result
            }
            RecipeKind::Shaped {
                width: 2,
                height: 1,
                pattern: [Some(x), Some(y)],
                result,
            } if x.test(a) && y.test(b) => result,
            _ => return None,
        };
        dyed.iter()
            .position(|item| *item == result.item)
            .map(|id| DyeColor::from_id(id as i32))
    })
}

/// 无序配方的两格匹配：`ShapelessRecipe.matches` 对两件原料就是两种分配之一成立。
///
/// Two-input shapeless matching: one of the two assignments must hold.
fn two_match(x: Option<&Ingredient>, y: Option<&Ingredient>, len: usize, a: i32, b: i32) -> bool {
    let (Some(x), Some(y)) = (x, y) else {
        return false;
    };
    len == 2 && ((x.test(a) && y.test(b)) || (x.test(b) && y.test(a)))
}

/// `Sheep.getBreedOffspring` 的颜色：能混就混，否则世界随机源 `nextBoolean` 选一方。
///
/// The lamb colour of `Sheep.getBreedOffspring`: the recipe mix, or a world-random parent.
pub(crate) fn offspring_color(
    father: DyeColor,
    mother: DyeColor,
    random: &LegacyRandom,
) -> DyeColor {
    mixed_color(father, mother).unwrap_or_else(|| {
        if random.next_boolean() {
            father
        } else {
            mother
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dye_items_follow_the_colour_order() {
        let dyed = mc_bamboo_data::recipes::DYED;
        assert_eq!(dyed[0], items::WHITE_DYE);
        assert_eq!(dyed[15], items::BLACK_DYE);
    }

    #[test]
    fn known_dye_mixes_come_from_recipes() {
        use DyeColor::*;
        assert_eq!(mixed_color(Black, White), Some(Gray));
        assert_eq!(mixed_color(White, Gray), Some(LightGray));
        assert_eq!(mixed_color(Blue, Green), Some(Cyan));
        assert_eq!(mixed_color(Red, Yellow), Some(Orange));
        assert_eq!(mixed_color(White, White), None);
        assert_eq!(mixed_color(Black, Red), None);
    }

    #[test]
    fn wool_bits_keep_colour_and_shear_state_apart() {
        let mut mob = Mob::new(MobType::Sheep);
        set_color(&mut mob, DyeColor::Lime);
        set_sheared(&mut mob, true);
        assert_eq!(color(&mob), DyeColor::Lime);
        assert!(is_sheared(&mob));
        set_sheared(&mut mob, false);
        assert_eq!(mob.wool, 5);
    }

    #[test]
    fn eat_block_rechecks_the_live_block_before_mutating() {
        let mut level = crate::testing::TestLevel::new();
        let pos = mc_bamboo_core::BlockPos::new(0, 63, 0);
        level.set(pos, crate::BlockState(mc_bamboo_data::blocks::GRASS_BLOCK));
        let mut ticks = crate::BlockTicks::default();
        let mut world = crate::world::World::new(&mut level, &mut ticks);
        super::super::living::apply_eat_block(&mut world, pos, true);
        super::super::living::apply_eat_block(&mut world, pos, true);
        assert_eq!(
            world.block_state(pos),
            crate::BlockState(mc_bamboo_data::blocks::DIRT)
        );
        let destroyed = world
            .into_effects()
            .into_iter()
            .filter(|effect| matches!(effect, crate::Effect::Destroyed { .. }))
            .count();
        assert_eq!(destroyed, 1);
    }
}
