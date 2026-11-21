//! 生物的生成入口：`/summon`（官方 `SummonCommand.createEntity`）和刷怪蛋
//! （`SpawnEggItem.useOn` → `EntityType.spawn`），以及两者共用的 `finalizeSpawn`。
//!
//! 随机数的来源和先后照官方：构造函数里的 UUID 和朝向取实体自己的随机源
//! （[`crate::EntityStore::prepare`]），`finalizeSpawn` 和刷怪蛋的朝向取世界随机源。
//! 不做自然刷怪。
//!
//! Mob spawn entry points: `/summon` and spawn eggs, plus the shared
//! `finalizeSpawn`. Random draws follow vanilla: constructor draws come from the
//! entity's own random, `finalizeSpawn` and the egg facing from the level random.

use mc_bamboo_core::{Axis, BlockPos, Vec3, mth};
use mc_bamboo_data::registries::attribute;
use mc_bamboo_nbt::Tag;

use super::{
    CowSoundVariant, CowVariant, MobType,
    attributes::{Modifier, Operation},
    living, nbt,
};
use crate::{
    entity::{Entity, EntityUid, ctx::BlockView, physics},
    shape::{Aabb, CollisionContext},
    world::World,
};

/// 官方 `EntitySpawnReason` 里牛用得到的几种。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnReason {
    Command,
    SpawnItemUse,
    Breeding,
}

/// `/summon` 失败的原因，对应官方的四条错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SummonError {
    /// `commands.summon.invalidPosition`。
    InvalidPosition,
    /// `commands.summon.failed`：NBT 读不进去。
    Failed(String),
    /// `commands.summon.failed.uuid`。
    DuplicateUuid,
}

/// `Mob.RANDOM_SPAWN_BONUS_ID`。
pub(super) const RANDOM_SPAWN_BONUS: &str = "minecraft:random_spawn_bonus";

/// `Level.isInSpawnableBounds`：竖直 `[-2e7, 2e7)`，水平绝对值小于 3e7。
pub fn is_in_spawnable_bounds(pos: BlockPos) -> bool {
    (-20_000_000..20_000_000).contains(&pos.y)
        && (-30_000_000..30_000_000).contains(&pos.x)
        && (-30_000_000..30_000_000).contains(&pos.z)
}

/// `SummonCommand.createEntity` 加 `tryAddFreshEntityWithPassengers`，只认牛。
/// `nbt` 为 `None` 时（命令没带 NBT）跑 `finalizeSpawn`；带了 NBT 就不跑，和官方一样。
///
/// `/summon` for a cow. Without NBT `finalizeSpawn` runs; with NBT it does not.
pub fn summon_cow(
    world: &mut World,
    pos: Vec3,
    nbt: Option<&Tag>,
) -> Result<EntityUid, SummonError> {
    summon_mob(world, MobType::Cow, pos, nbt)
}

/// `/summon`：按实体类型构造、读 NBT、`snapTo`，不带 NBT 时跑 `finalizeSpawn`。
pub fn summon_mob(
    world: &mut World,
    kind: MobType,
    pos: Vec3,
    nbt: Option<&Tag>,
) -> Result<EntityUid, SummonError> {
    if !is_in_spawnable_bounds(BlockPos::containing(pos)) {
        return Err(SummonError::InvalidPosition);
    }
    let randomize = nbt.is_none();
    let mut entries = match nbt {
        Some(Tag::Compound(entries)) => entries.clone(),
        _ => Vec::new(),
    };
    entries.retain(|(key, _)| key != "id");
    entries.push(("id".to_owned(), Tag::String(kind.name().to_owned())));
    let tag = Tag::Compound(entries);
    // 构造（ID、种子、UUID、朝向）→ `load` → `snapTo(pos, yRot, xRot)`。
    // Construct, load, then snap to the target with the loaded rotation.
    let mut entity = world.prepare_entity(Entity::by_mob_type(kind, Vec3::default()));
    nbt::load_into(&mut entity, &tag).map_err(|error| SummonError::Failed(error.0))?;
    let x_rot = entity.kind.mob().map_or(0.0, |mob| mob.x_rot);
    let y_rot = entity.y_rot;
    snap_to(&mut entity, pos, y_rot, x_rot);
    if randomize {
        finalize_spawn(world, kind, &mut entity, SpawnReason::Command);
    }
    let uuid = entity.kind.mob().map(|mob| mob.uuid).unwrap_or_default();
    if world.has_mob_uuid(&uuid) {
        return Err(SummonError::DuplicateUuid);
    }
    Ok(world.add_prepared_entity(entity))
}

/// `SpawnEggItem.useOn` 走到 `EntityType.spawn(level, stack, player, pos, SPAWN_ITEM_USE,
/// true, offset_more)`：在 `pos` 方块的中心放一头牛，朝向随机，然后叫一声。
/// `custom_name` 是刷怪蛋上的 `custom_name` 组件（NBT 形式）。返回生成的 ID。
///
/// A spawn egg used on a block: a cow centred on `pos`, random facing, then the
/// ambient sound. Returns the new id.
pub fn spawn_cow_from_egg(
    world: &mut World,
    pos: BlockPos,
    offset_more: bool,
    custom_name: Option<Tag>,
) -> EntityUid {
    spawn_egg(world, MobType::Cow, pos, offset_more, custom_name)
}

/// `SpawnEggItem.useOn`：按刷怪蛋对应的实体类型生成。
pub fn spawn_egg(
    world: &mut World,
    kind: MobType,
    pos: BlockPos,
    offset_more: bool,
    custom_name: Option<Tag>,
) -> EntityUid {
    let mut entity = world.prepare_entity(Entity::by_mob_type(kind, Vec3::default()));
    // `shouldOffsetY`：先摆到上一格，再按碰撞算往下落多少。
    // Place it one block up first, then drop it onto the collisions.
    let centre = Vec3::new(
        f64::from(pos.x) + 0.5,
        f64::from(pos.y + 1),
        f64::from(pos.z) + 0.5,
    );
    entity.position = centre;
    let offset = spawn_y_offset(&*world, pos, offset_more, entity.bounding_box());
    let yaw = mth::wrap_degrees(world.random().next_float() * 360.0);
    snap_to(
        &mut entity,
        Vec3::new(centre.x, f64::from(pos.y) + offset, centre.z),
        yaw,
        0.0,
    );
    entity.with_mob(|me, mob| {
        mob.y_head_rot = me.y_rot;
        mob.y_body_rot = me.y_rot;
    });
    finalize_spawn(world, kind, &mut entity, SpawnReason::SpawnItemUse);
    // `appendComponentsConfig`：刷怪蛋的 `custom_name` 套到实体上。
    if let Some(name) = custom_name {
        entity.with_mob(|_, mob| mob.custom_name = Some(name));
    }
    let uid = world.add_prepared_entity(entity);
    // `Mob.playAmbientSound`：放进世界之后叫一声，用生物自己的随机源取音高。
    if kind == MobType::Fox {
        // `Fox.playAmbientSound` 要看天色和 16 格内的玩家，先在世界里取好。
        // `Fox.playAmbientSound` reads the sky and nearby players, gathered first.
        let bright = crate::entity::ctx::BlockView::is_bright_outside(&*world);
        let mut players = Vec::new();
        crate::entity::ctx::BlockView::for_each_player(&*world, &mut |player| {
            players.push(*player)
        });
        world.with_entity_mut(uid, &mut |entity| {
            entity
                .with_mob(|me, mob| super::fox::play_ambient_sound_with(me, mob, bright, &players));
        });
        return uid;
    }
    world.with_entity_mut(uid, &mut |entity| {
        entity.with_mob(living::play_ambient_sound);
    });
    uid
}

/// `EntityType.getYOffset`：脚下那一格（`offset_more` 时再往下一格）的碰撞形状，
/// 让站在上一格的实体沿 Y 往下扫 1（或 2）格。
fn spawn_y_offset(view: &dyn BlockView, pos: BlockPos, offset_more: bool, moving: Aabb) -> f64 {
    let min_y = if offset_more {
        f64::from(pos.y) - 1.0
    } else {
        f64::from(pos.y)
    };
    let area = Aabb::new(
        f64::from(pos.x),
        min_y,
        f64::from(pos.z),
        f64::from(pos.x + 1),
        f64::from(pos.y + 1),
        f64::from(pos.z + 1),
    );
    let boxes: Vec<Aabb> = view
        .block_collisions(area, CollisionContext::Empty)
        .into_iter()
        .flat_map(|(_, boxes)| boxes)
        .collect();
    let distance = if offset_more { -2.0 } else { -1.0 };
    1.0 + physics::collide_along(Axis::Y, moving, &boxes, distance)
}

/// `Entity.snapTo(x, y, z, yRot, xRot)`：位置、朝向、上一刻的位置和朝向一起设。
pub(crate) fn snap_to(entity: &mut Entity, pos: Vec3, y_rot: f32, x_rot: f32) {
    entity.position = pos;
    entity.old_position = pos;
    entity.y_rot = y_rot;
    entity.with_mob(|_, mob| {
        mob.x_rot = x_rot;
        mob.y_rot_o = y_rot;
        mob.x_rot_o = x_rot;
    });
}

/// `Cow.finalizeSpawn` → `AgeableMob.finalizeSpawn` → `Mob.finalizeSpawn`，`groupData` 为空。
///
/// 1. 花色按脚下群系挑（只有一个最高优先级的候选，但官方仍 `nextInt(1)` 取一次）；
/// 2. 叫声在两种里 `nextInt(2)`；
/// 3. `AgeableMobGroupData(true)` 的组大小是 0，`getGroupSize() > 0` 为假，不取数也不变小；
/// 4. 跟随距离加一个 `triangle(0, 0.11485)` 的修饰符（没有时才加）；
/// 5. `nextFloat() < 0.05` 左撇子。
pub(crate) fn finalize_spawn(
    world: &mut World,
    kind: MobType,
    entity: &mut Entity,
    reason: SpawnReason,
) {
    let _ = reason;
    if kind == MobType::Skeleton {
        super::skeleton::finalize_spawn(world, entity);
        return;
    }
    if kind.is_monster() {
        finalize_monster(world, kind, entity);
        return;
    }
    let biome = world.biome_id(entity.block_position());
    let random = world.random();
    if kind == MobType::Fox {
        entity.with_mob(|_, mob| {
            super::fox::finalize_spawn(mob, biome, random);
            finalize_animal(mob, random);
        });
        return;
    }
    if kind == MobType::Rabbit {
        entity.with_mob(|_, mob| {
            mob.rabbit.variant = super::rabbit::random_variant(biome, random);
            finalize_animal(mob, random);
        });
        return;
    }
    let variant = cow_variant_for_biome(biome);
    let _ = random.next_int(1);
    let sound = if random.next_int(2) == 0 {
        CowSoundVariant::Classic
    } else {
        CowSoundVariant::Moody
    };
    entity.with_mob(|_, mob| {
        mob.variant = variant;
        mob.sound_variant = sound;
        finalize_animal(mob, random);
    });
}

fn finalize_animal(mob: &mut super::Mob, random: &crate::random::LegacyRandom) {
    if let Some(range) = mob.attributes.instance(attribute::FOLLOW_RANGE)
        && range.modifier(RANDOM_SPAWN_BONUS).is_none()
    {
        let amount = random.triangle(0.0, 0.114_850_000_000_000_01);
        range.modifiers.push(Modifier {
            id: RANDOM_SPAWN_BONUS.to_owned(),
            amount,
            operation: Operation::AddMultipliedBase,
            permanent: true,
        });
    }
    mob.left_handed = random.next_float() < 0.05;
}

/// `Zombie.finalizeSpawn` 的随机取数顺序（本期不生成鸡骑士/增援）：
/// 捡物 `nextFloat < 0.55*specialMultiplier` → 幼年 `nextFloat < 0.05` →
/// 破门 `nextFloat < 0.1` → 随机装备 `nextFloat` + `nextInt(6)`。
fn finalize_monster(world: &mut World, kind: MobType, entity: &mut Entity) {
    let _ = kind;
    let random = world.random();
    let can_pick_up_loot = random.next_float() < 0.55;
    let baby = random.next_float() < 0.05;
    let can_break_doors = random.next_float() < 0.1;
    // `Monster.populateDefaultEquipmentSlots` + `Zombie.populateDefaultEquipmentSlots`。
    let has_weapon = random.next_float() < 0.01;
    let weapon = if has_weapon { random.next_int(6) } else { 0 };
    entity.with_mob(|me, mob| {
        mob.can_pick_up_loot = can_pick_up_loot;
        if baby {
            mob.set_zombie_baby(true);
            let (width, height, eye) = mob.mob_type.dimensions(true);
            me.dimensions = (width, height);
            mob.eye_height = eye;
        }
        mob.can_break_doors = can_break_doors;
        if can_break_doors {
            mob.navigation.flags.can_open_doors = true;
            mob.goals.insert_break_door();
        }
        if has_weapon {
            // 剑 / 长矛 / 铲子；长矛会让 `SpearUseGoal` 启动，这一期不建模它。
            let item = match weapon {
                0 => mc_bamboo_data::items::IRON_SWORD,
                1 => mc_bamboo_data::items::IRON_SPEAR,
                _ => mc_bamboo_data::items::IRON_SHOVEL,
            };
            mob.equipment.set(
                mc_bamboo_protocol::packet::play::EquipmentSlot::MainHand,
                mc_bamboo_protocol::item::ItemStack::new(item, 1),
            );
        }
    });
}

/// `cow_variant` 的生成条件：暖和冷各按一个群系标签，优先级 1；其余是温带。
fn cow_variant_for_biome(biome: i32) -> CowVariant {
    if biome_tag_contains("minecraft:spawns_warm_variant_farm_animals", biome) {
        CowVariant::Warm
    } else if biome_tag_contains("minecraft:spawns_cold_variant_farm_animals", biome) {
        CowVariant::Cold
    } else {
        CowVariant::Temperate
    }
}

pub(super) fn biome_tag_contains(name: &str, biome: i32) -> bool {
    mc_bamboo_data::tags::TAG_GROUPS
        .iter()
        .filter(|group| group.registry == "minecraft:worldgen/biome")
        .flat_map(|group| group.tags.iter())
        .any(|tag| tag.name == name && tag.ids.contains(&biome))
}
