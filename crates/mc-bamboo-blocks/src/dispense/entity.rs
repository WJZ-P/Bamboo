//! 会生成实体的注册表行为。刷怪蛋和弹射物仍不生成实体。
//! 盔甲架在发射器正前方生成，TNT 物品在规则为真时生成点燃的 TNT。
//!
//! 船和矿车放得下时只扣物品，不取 `level.random`，外层一次 1000 和 2000。放不下时套一层默认抛出。
//! 盔甲架照 `DispenseItemBehavior$1` 生成实体、应用 `entity_data`、扣一个，取一次 `level.random`；
//! 带实体数据的刷怪蛋走 `spawn` 返回 null：物品留着，一次 1000。
//! 没有实体数据的刷怪蛋是默认抛出。TNT 物品在 `tntExplodes` 为真时生成 `PrimedTnt`。
//! 箭、蛋、雪球、药水、烟花、火焰弹和风弹走默认抛出。官方那些用的是弹射物事件，不是这套随机数。
//!
//! Registry behaviours that spawn an entity. Spawn eggs and projectiles still do not create one.
//! An armour stand spawns in front of the dispenser; a TNT item spawns primed TNT when the
//! rule is true.
//!
//! A boat or minecart that can be placed only consumes the item. It does not draw from
//! `level.random`, and the outer dispense plays 1000 and 2000 once. Otherwise it nests a
//! default throw. An armour stand follows `DispenseItemBehavior$1`: spawn the entity, apply
//! `entity_data`, shrink one item and draw one `level.random` value. A spawn egg that carries
//! entity data follows `spawn` returning null: the item stays and 1000 plays once. A spawn egg
//! without entity data is a default throw. A TNT item spawns a `PrimedTnt` when `tntExplodes`
//! is true. Arrows, eggs, snowballs, potions, fireworks, fire charges and wind charges take
//! the default throw. Official uses a projectile event for those, not these random draws.

use mc_bamboo_core::{BlockPos, Direction, Vec3};
use mc_bamboo_data::{
    items, registries::data_component_type as kinds, registries::entity_type, tags::block_tags,
    tags::fluid_tags,
};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::{
    components::{ComponentPatch, ComponentValue, defaults},
    item::ItemStack,
};

use super::{Shot, nested_default, throw_one};
use crate::{
    behaviour::common::item_of,
    container::copy_with_count,
    dispenser::without_one,
    entity::{
        Entity,
        arrow::{ArrowShot, Pickup},
        mob::{MobType, nbt},
    },
    world::World,
};

pub(super) fn handles(item: i32) -> bool {
    is_projectile(item)
        || is_boat(item)
        || is_minecart(item)
        || item == items::ARMOR_STAND
        || item == items::TNT
}

pub(super) fn execute(
    world: &mut World,
    pos: BlockPos,
    facing: Direction,
    stack: ItemStack,
) -> Shot {
    let item = stack.item;
    if is_arrow(item) {
        return arrow(world, pos, facing, stack);
    }
    if matches!(item, items::EGG | items::BLUE_EGG | items::BROWN_EGG) {
        return egg(world, pos, facing, stack);
    }
    if item == items::FIREWORK_ROCKET {
        return firework(world, pos, facing, stack);
    }
    if is_projectile(item) {
        return Shot::plain(throw_one(world, pos, facing, stack));
    }
    if item == items::ARMOR_STAND {
        return armor_stand(world, pos, facing, stack);
    }
    if item == items::TNT {
        return dispense_tnt(world, pos, facing, stack);
    }
    if is_boat(item) {
        return boat(world, pos, facing, stack);
    }
    minecart(world, pos, facing, stack)
}

/// `DispenseItemBehavior$1`（`Items.ARMOR_STAND`）：在发射器正前方一格生成盔甲架。
///
/// `EntityType.create` 先用 `level.random` 抽一次初始朝向（随后被 `PostSpawnProcessor`
/// 的 `setYRot` 覆盖），照样取一次。链上三步依次是 `setYRot(facing.toYRot())`、
/// `applyComponentsFromItemStack`（物品的 `CUSTOM_NAME`）和 `updateCustomEntityTag`
/// （`ENTITY_DATA`，只在类型匹配时 `load`）；实体落在方块中心、`y` 取方块底面。
/// 生成成功后 `stack.shrink(1)`，声音和冒烟由外层 `dispense` 统一放 1000 / 2000。
///
/// `DispenseItemBehavior$1` (`Items.ARMOR_STAND`): spawn an armour stand one cell in front.
/// `EntityType.create` draws one `level.random` value for the initial yaw (later overwritten
/// by the `PostSpawnProcessor`'s `setYRot`), so take one draw too. The chain applies
/// `setYRot(facing.toYRot())`, `applyComponentsFromItemStack` (the item's `CUSTOM_NAME`) and
/// `updateCustomEntityTag` (`ENTITY_DATA`, loaded only when the type matches), in that order.
/// The entity lands at the cell centre with `y` at the block bottom. On success the stack
/// shrinks by one; the outer `dispense` plays 1000 / 2000.
fn armor_stand(world: &mut World, pos: BlockPos, facing: Direction, stack: ItemStack) -> Shot {
    let target = pos.relative(facing);
    let _ = world.next_random_float();
    let center = Vec3::new(
        f64::from(target.x) + 0.5,
        f64::from(target.y),
        f64::from(target.z) + 0.5,
    );
    let mut entity = world.prepare_entity(Entity::by_mob_type(MobType::ArmorStand, center));
    // `PostSpawnProcessor` 第一步：`setYRot(Direction.toYRot())`。
    entity.y_rot = facing.to_yaw();
    let patch = ComponentPatch::from_opaque(&stack).unwrap_or_default();
    // 第二步 `applyComponentsFromItemStack`：`CUSTOM_NAME` → `setCustomName`。
    if let Some(ComponentValue::CustomName(name)) = patch.get(kinds::CUSTOM_NAME) {
        let custom_name = name.tag().clone();
        entity.with_mob(|_, mob| {
            mob.custom_name = Some(custom_name);
            mob.needs_sync = true;
        });
    }
    // 第三步 `updateCustomEntityTag`：`TypedEntityData.loadInto` 是
    // `saveWithoutId` → `merge(ENTITY_DATA)` → `load`，所以物品的 `CUSTOM_NAME`、
    // 朝向等当前状态会保留，只有 `ENTITY_DATA` 里的键覆盖。类型不符时整步跳过。
    if let Some(ComponentValue::EntityData(data)) = patch.get(kinds::ENTITY_DATA)
        && data.kind == entity_type::ARMOR_STAND
    {
        let mut entries = match nbt::write_mob(&entity.clone().split()) {
            Ok(Tag::Compound(entries)) => entries,
            _ => Vec::new(),
        };
        for (key, value) in &data.tag {
            if let Some(slot) = entries.iter_mut().find(|(name, _)| name == key) {
                slot.1 = value.clone();
            } else {
                entries.push((key.clone(), value.clone()));
            }
        }
        let _ = nbt::load_into(&mut entity, &Tag::Compound(entries));
    }
    world.add_prepared_entity(entity);
    Shot::plain(without_one(&stack))
}

pub(super) fn is_spawn_egg(item: i32) -> bool {
    item_name(item).is_some_and(|name| name.ends_with("_spawn_egg"))
}

/// 带 `ENTITY_DATA` 时走 `spawn` 返回 null：物品留着，不取 `nextFloat`。
/// 没有这份数据时是默认抛出。
///
/// With `ENTITY_DATA`, follow `spawn` returning null: the item stays and `nextFloat` is not drawn.
/// Without that component this is a default throw.
pub(super) fn spawn_egg(
    world: &mut World,
    pos: BlockPos,
    facing: Direction,
    stack: ItemStack,
) -> Shot {
    let has_data = defaults::resolve(stack.item, &stack.patch(), kinds::ENTITY_DATA).is_some();
    if !has_data {
        return Shot::plain(throw_one(world, pos, facing, stack));
    }
    // `type.spawn` 返回 null，或者 `getType` 是 null：物品原样返回，不取 `nextFloat`。
    //
    // `type.spawn` returned null, or `getType` is null: the stack is returned unchanged.
    // The `nextFloat` is not drawn.
    Shot::plain(stack)
}

/// 注册表里的 TNT 物品。规则为假时物品留着，不取随机数。
///
/// 否则先试 `SulfurCubeBlockDispenseItemBehavior.dispenseBlock`。这个世界没有硫磺立方，
/// 搜索是空的，返回假，不取随机数，也不减数量。然后 `new PrimedTnt`：朝向是一次 `nextDouble`，
/// 声音是 `entity.tnt.primed`，数量减一。这不是 `TntBlock.prime`，不拆方块。
///
/// The TNT item in the registry. A false rule keeps the item and draws nothing.
///
/// Otherwise `SulfurCubeBlockDispenseItemBehavior.dispenseBlock` runs first. This world has no
/// sulfur cubes, so the search is empty, returns false, draws nothing and does not shrink.
/// Then `new PrimedTnt`: the yaw is one `nextDouble`, the sound is `entity.tnt.primed`, and the
/// count drops by one. This is not `TntBlock.prime` and does not remove a block.
fn dispense_tnt(world: &mut World, pos: BlockPos, facing: Direction, stack: ItemStack) -> Shot {
    if !world.block_game_rules().tnt_explodes {
        return Shot::optional(stack, false);
    }
    let target = pos.relative(facing);
    if world.spawn_tnt(target, None) < 0 {
        return Shot::optional(stack, false);
    }
    Shot::optional(without_one(&stack), true)
}

/// `BoatDispenseItemBehavior`。前方的流体是水，或者前方是空气且下面是水，才算放得下。
///
/// `BoatDispenseItemBehavior`. It can be placed when the front fluid is water, or the front
/// block is air and the block below is water.
fn boat(world: &mut World, pos: BlockPos, facing: Direction, stack: ItemStack) -> Shot {
    let front = pos.relative(facing);
    let on_water = world.fluid_state(front).is_tagged(fluid_tags::WATER)
        || world.block_state(front).is_air()
            && world
                .fluid_state(front.below())
                .is_tagged(fluid_tags::WATER);
    if on_water {
        Shot::plain(without_one(&stack))
    } else {
        Shot::plain(nested_default(world, pos, facing, stack))
    }
}

/// `MinecartDispenseItemBehavior`。前方是铁轨，或者前方是空气且下面是铁轨。
///
/// `MinecartDispenseItemBehavior`. The front is a rail, or the front is air and the block
/// below is a rail.
fn minecart(world: &mut World, pos: BlockPos, facing: Direction, stack: ItemStack) -> Shot {
    let front = pos.relative(facing);
    let state = world.block_state(front);
    let on_rail = state.is_tagged(block_tags::RAILS)
        || state.is_air()
            && world
                .block_state(front.below())
                .is_tagged(block_tags::RAILS);
    if on_rail {
        Shot::plain(without_one(&stack))
    } else {
        Shot::plain(nested_default(world, pos, facing, stack))
    }
}

/// `ProjectileDispenseBehavior` 里注册成弹射物的箭。`ArrowItem.asProjectile` 生成可拾取的箭。
///
/// An arrow registered as a projectile. `ArrowItem.asProjectile` builds a
/// pickup-capable arrow.
fn arrow(world: &mut World, pos: BlockPos, facing: Direction, stack: ItemStack) -> Shot {
    let (x, y, z) = facing.normal();
    // `DispenserBlock.getDispensePosition(source, 0.7, new Vec3(0, 0.1, 0))`。
    //
    // `DispenserBlock.getDispensePosition(source, 0.7, new Vec3(0, 0.1, 0))`.
    let (x, y, z) = (f64::from(x), f64::from(y), f64::from(z));
    let position = Vec3::new(
        f64::from(pos.x) + 0.5 + (0.7 * x),
        f64::from(pos.y) + 0.5 + (0.7 * y + 0.1),
        f64::from(pos.z) + 0.5 + (0.7 * z),
    );
    let shot = ArrowShot {
        position,
        direction: Vec3::new(x, y, z),
        power: 1.1,
        uncertainty: 6.0,
        source_movement: None,
        owner: None,
        owner_uuid: None,
        ammo: copy_with_count(&stack, 1),
        weapon: None,
        critical: false,
        pickup: Pickup::Allowed,
    };
    world.spawn_arrow_shot(shot);
    Shot::with_event(without_one(&stack), 1002)
}

/// `FireworkRocketItem.createDispenseConfig`：位置是 `blockSource.center()` 沿朝向
/// 外移 `0.5000099999997474`，力度 0.5、散布 1、事件 1004。`asProjectile` 生成的是
/// `shotAtAngle` 的烟花。
///
/// `FireworkRocketItem.createDispenseConfig`: the position is `blockSource.center()`
/// plus the facing step times `0.5000099999997474`, power 0.5, uncertainty 1 and event
/// 1004. `asProjectile` produces a `shotAtAngle` firework.
fn firework(world: &mut World, pos: BlockPos, facing: Direction, stack: ItemStack) -> Shot {
    let (sx, sy, sz) = facing.normal();
    let position = Vec3::new(
        f64::from(pos.x) + 0.5 + f64::from(sx) * 0.500_009_999_999_747_4,
        f64::from(pos.y) + 0.5 + f64::from(sy) * 0.500_009_999_999_747_4,
        f64::from(pos.z) + 0.5 + f64::from(sz) * 0.500_009_999_999_747_4,
    );
    crate::item_use::firework::spawn_dispensed(world, position, (sx, sy, sz), &stack);
    Shot::with_event(without_one(&stack), 1004)
}

/// `EggItem.asProjectile` 加默认的 `ProjectileItem.DispenseConfig`：位置同箭，力度 1.1，
/// 散布 6，事件 1002。
///
/// `EggItem.asProjectile` with the default `DispenseConfig`: the arrow's position,
/// power 1.1, uncertainty 6 and event 1002.
fn egg(world: &mut World, pos: BlockPos, facing: Direction, stack: ItemStack) -> Shot {
    let (x, y, z) = facing.normal();
    let (x, y, z) = (f64::from(x), f64::from(y), f64::from(z));
    let shot = crate::entity::thrown_egg::EggShot {
        position: Vec3::new(
            f64::from(pos.x) + 0.5 + (0.7 * x),
            f64::from(pos.y) + 0.5 + (0.7 * y + 0.1),
            f64::from(pos.z) + 0.5 + (0.7 * z),
        ),
        direction: Vec3::new(x, y, z),
        power: 1.1,
        uncertainty: 6.0,
        source_movement: None,
        owner: None,
        owner_uuid: None,
        item: stack.clone(),
        projectile: crate::entity::thrown_egg::ThrowableKind::Egg,
    };
    world.spawn_egg_shot(&shot);
    Shot::with_event(without_one(&stack), 1002)
}

fn is_arrow(item: i32) -> bool {
    matches!(
        item,
        items::ARROW | items::TIPPED_ARROW | items::SPECTRAL_ARROW
    )
}

fn is_projectile(item: i32) -> bool {
    matches!(
        item,
        items::ARROW
            | items::TIPPED_ARROW
            | items::SPECTRAL_ARROW
            | items::EGG
            | items::BLUE_EGG
            | items::BROWN_EGG
            | items::SNOWBALL
            | items::EXPERIENCE_BOTTLE
            | items::SPLASH_POTION
            | items::LINGERING_POTION
            | items::FIREWORK_ROCKET
            | items::FIRE_CHARGE
            | items::WIND_CHARGE
    )
}

/// `bootStrap` 里登记过的船和竹筏。没登记的物品不能因为名字像船就被扣掉。
///
/// Boats and rafts registered in `bootStrap`. An unregistered item is not consumed
/// just because its name looks like a boat.
fn is_boat(item: i32) -> bool {
    matches!(
        item,
        items::OAK_BOAT
            | items::SPRUCE_BOAT
            | items::BIRCH_BOAT
            | items::JUNGLE_BOAT
            | items::DARK_OAK_BOAT
            | items::ACACIA_BOAT
            | items::CHERRY_BOAT
            | items::MANGROVE_BOAT
            | items::POPLAR_BOAT
            | items::PALE_OAK_BOAT
            | items::BAMBOO_RAFT
            | items::OAK_CHEST_BOAT
            | items::SPRUCE_CHEST_BOAT
            | items::BIRCH_CHEST_BOAT
            | items::JUNGLE_CHEST_BOAT
            | items::DARK_OAK_CHEST_BOAT
            | items::ACACIA_CHEST_BOAT
            | items::CHERRY_CHEST_BOAT
            | items::MANGROVE_CHEST_BOAT
            | items::POPLAR_CHEST_BOAT
            | items::PALE_OAK_CHEST_BOAT
            | items::BAMBOO_CHEST_RAFT
    )
}

/// `bootStrap` 里登记过的矿车。刷怪笼矿车不在表里，走默认抛出。
///
/// Minecarts registered in `bootStrap`. A spawner minecart is not in the table and uses
/// the default throw.
fn is_minecart(item: i32) -> bool {
    matches!(
        item,
        items::MINECART
            | items::CHEST_MINECART
            | items::FURNACE_MINECART
            | items::TNT_MINECART
            | items::HOPPER_MINECART
            | items::COMMAND_BLOCK_MINECART
    )
}

fn item_name(item: i32) -> Option<&'static str> {
    item_of(item).map(|item| item.name)
}
