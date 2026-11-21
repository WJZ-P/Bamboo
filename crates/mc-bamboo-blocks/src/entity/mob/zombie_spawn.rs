//! 僵尸和尸壳的 `finalizeSpawn`（官方 `Zombie.finalizeSpawn`、`Husk.finalizeSpawn`）。
//!
//! 世界随机源的取数顺序：`Mob.finalizeSpawn` 的跟随距离 `triangle` 和左撇子 →
//! 捡物 → 组里第一只抽幼年 → 幼年时的鸡骑士两次判定 → 破门 → 盔甲与武器 →
//! 装备附魔 → 万圣节头饰 → 尸壳的骆驼尸壳判定。`handleAttributes` 用实体自己的随机源。
//!
//! Zombie and husk `finalizeSpawn` (vanilla `Zombie.finalizeSpawn`, `Husk.finalizeSpawn`).
//! Level-random order: the `Mob.finalizeSpawn` follow-range triangle and left-handedness,
//! loot pickup, the group's baby roll, the two chicken-jockey rolls for babies, door
//! breaking, armour and weapon, equipment enchantments, the Halloween head, then the
//! husk's camel-husk roll. `handleAttributes` draws from the entity's own random.

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::{items, registries::attribute, registries::entity_type};
use mc_bamboo_protocol::{item::ItemStack, packet::play::EquipmentSlot};

use super::{
    Mob, MobType,
    attributes::{Modifier, Operation},
    skeleton,
    spawn::{RANDOM_SPAWN_BONUS, SpawnGroup, SpawnReason},
};
use crate::{
    entity::{Entity, ctx::BlockView, random::JavaRandom},
    random::LegacyRandom,
    shape::Aabb,
    world::World,
};

/// `Zombie.ZOMBIE_RANDOM_SPAWN_BONUS_ID`。
const ZOMBIE_RANDOM_SPAWN_BONUS: &str = "minecraft:zombie_random_spawn_bonus";

/// `Zombie.LEADER_ZOMBIE_BONUS_ID`。
const LEADER_ZOMBIE_BONUS: &str = "minecraft:leader_zombie_bonus";

/// `Zombie.ZombieGroupData`：同一组共享的幼年结果和能不能骑鸡。
///
/// `Zombie.ZombieGroupData`: the baby roll and jockey flag shared by one group.
#[derive(Debug, Clone, Copy)]
pub struct ZombieGroupData {
    pub is_baby: bool,
    pub can_spawn_jockey: bool,
}

/// `Zombie.finalizeSpawn`，尸壳再接 `Husk.finalizeSpawn` 的骆驼尸壳分支。
///
/// `Zombie.finalizeSpawn`; husks continue with the camel-husk branch of
/// `Husk.finalizeSpawn`.
pub(super) fn finalize_spawn(
    world: &mut World<'_>,
    kind: MobType,
    entity: &mut Entity,
    reason: SpawnReason,
    group: &mut Option<SpawnGroup>,
) {
    let difficulty = BlockView::difficulty(world);
    let special = special_multiplier(BlockView::effective_difficulty(
        world,
        entity.block_position(),
    ));
    let random = world.random();
    entity.with_mob(|me, mob| {
        mob_finalize_spawn(mob, random);
        mob.can_pick_up_loot = random.next_float() < 0.55 * special;
        let data = zombie_group(random, group);
        if data.is_baby {
            set_baby(me, mob);
            if data.can_spawn_jockey {
                roll_chicken_jockey(random);
            }
        }
        set_can_break_doors(mob, random.next_float() < special * 0.1);
        skeleton::populate_armor(mob, random, difficulty, special);
        populate_weapon(mob, random, difficulty);
        skeleton::enchant_spawn_equipment(world, mob, random, special);
        if mob.equipment.get(EquipmentSlot::Head).is_empty() && world.level().is_halloween() {
            skeleton::equip_halloween_head(mob, random);
        }
        handle_attributes(mob, &mut me.random, special);
    });
    if kind == MobType::Husk {
        husk_finalize_spawn(world, entity, reason, special);
    }
}

/// `DifficultyInstance.getSpecialMultiplier`。
/// `DifficultyInstance.getSpecialMultiplier`：2 以下 0，4 以上 1，中间线性。
///
/// `DifficultyInstance.getSpecialMultiplier`: zero below 2, one above 4, linear between.
pub(super) fn special_multiplier(effective: f32) -> f32 {
    if effective < 2.0 {
        0.0
    } else if effective > 4.0 {
        1.0
    } else {
        (effective - 2.0) / 2.0
    }
}

/// `groupData == null` 时新建 `ZombieGroupData(getSpawnAsBabyOdds(random), true)`；
/// 组里后面的个体沿用第一只的结果，不再取数。
///
/// A null group gets `ZombieGroupData(getSpawnAsBabyOdds(random), true)`; later members
/// of the group reuse it without drawing.
fn zombie_group(random: &LegacyRandom, group: &mut Option<SpawnGroup>) -> ZombieGroupData {
    if let Some(SpawnGroup::Zombie(data)) = group {
        return *data;
    }
    let data = ZombieGroupData {
        is_baby: random.next_float() < 0.05,
        can_spawn_jockey: true,
    };
    *group = Some(SpawnGroup::Zombie(data));
    data
}

/// `Mob.finalizeSpawn`：跟随距离加 `triangle(0, 0.11485)`（没有时才加），再抽左撇子。
///
/// `Mob.finalizeSpawn`: a `triangle(0, 0.11485)` follow-range bonus when absent, then
/// left-handedness.
fn mob_finalize_spawn(mob: &mut Mob, random: &LegacyRandom) {
    if let Some(range) = mob.attributes.instance(attribute::FOLLOW_RANGE)
        && range.modifier(RANDOM_SPAWN_BONUS).is_none()
    {
        range.modifiers.push(Modifier {
            id: RANDOM_SPAWN_BONUS.to_owned(),
            amount: random.triangle(0.0, 0.114_850_000_000_000_01),
            operation: Operation::AddMultipliedBase,
            permanent: true,
        });
    }
    mob.left_handed = random.next_float() < 0.05;
}

/// `Zombie.setBaby(true)`：元数据、尺寸、眼高和速度修饰符。
fn set_baby(me: &mut Entity, mob: &mut Mob) {
    mob.set_zombie_baby(true);
    let (width, height, _) = mob.mob_type.dimensions(true);
    me.dimensions = (width, height);
}

/// 鸡骑士的两次判定。第一支要找附近没人骑的鸡，第二支要新建一只鸡；Bamboo 还没有鸡，
/// 两支都只取数；第二支命中时官方会多一只鸡并跑它的 `finalizeSpawn`，这里没有。
///
/// The two chicken-jockey rolls. The first looks for a nearby unridden chicken, the
/// second creates one; Bamboo has no chickens yet, so both only draw. A hit on the second
/// roll would add a chicken and run its `finalizeSpawn` in vanilla; that is missing here.
fn roll_chicken_jockey(random: &LegacyRandom) {
    if f64::from(random.next_float()) >= 0.05 {
        // 第二支：新建鸡的判定，命中也没有鸡可建。
        // Second branch: the new-chicken roll; a hit has no chicken to create.
        random.next_float();
    }
}

/// `Zombie.setCanBreakDoors`：地面寻路才生效；值变了才改寻路和目标。
///
/// `Zombie.setCanBreakDoors`: only for ground navigation, and only on a change.
fn set_can_break_doors(mob: &mut Mob, can_break_doors: bool) {
    if mob.can_break_doors == can_break_doors {
        return;
    }
    mob.can_break_doors = can_break_doors;
    mob.navigation.flags.can_open_doors = can_break_doors;
    if can_break_doors {
        mob.goals.insert_break_door();
    }
}

/// `Zombie.populateDefaultEquipmentSlots` 在盔甲之后的武器：困难 5%，其余 1%。
///
/// The weapon roll of `Zombie.populateDefaultEquipmentSlots`: 5% on hard, 1% otherwise.
fn populate_weapon(mob: &mut Mob, random: &LegacyRandom, difficulty: i32) {
    let chance = if difficulty == 3 { 0.05 } else { 0.01 };
    if random.next_float() >= chance {
        return;
    }
    let item = match random.next_int(6) {
        0 => items::IRON_SWORD,
        1 => items::IRON_SPEAR,
        _ => items::IRON_SHOVEL,
    };
    mob.equipment
        .set(EquipmentSlot::MainHand, ItemStack::new(item, 1));
}

/// `Zombie.handleAttributes`：增援概率、击退抗性、跟随距离和领袖僵尸，取实体随机源。
///
/// `Zombie.handleAttributes`: reinforcement chance, knockback resistance, follow range
/// and the leader bonus, all from the entity's own random.
fn handle_attributes(mob: &mut Mob, random: &mut JavaRandom, special: f32) {
    if let Some(chance) = mob.attributes.instance(attribute::SPAWN_REINFORCEMENTS) {
        chance.base = random.next_double() * f64::from(0.1_f32);
    }
    let knockback = random.next_double() * f64::from(0.05_f32);
    replace_modifier(
        mob,
        attribute::KNOCKBACK_RESISTANCE,
        RANDOM_SPAWN_BONUS,
        knockback,
        Operation::AddValue,
    );
    let follow = random.next_double() * 1.5 * f64::from(special);
    if follow > 1.0 {
        replace_modifier(
            mob,
            attribute::FOLLOW_RANGE,
            ZOMBIE_RANDOM_SPAWN_BONUS,
            follow,
            Operation::AddMultipliedTotal,
        );
    }
    if random.next_float() < special * 0.05 {
        let reinforcement = random.next_double() * 0.25 + 0.5;
        replace_modifier(
            mob,
            attribute::SPAWN_REINFORCEMENTS,
            LEADER_ZOMBIE_BONUS,
            reinforcement,
            Operation::AddValue,
        );
        let health = random.next_double() * 3.0 + 1.0;
        replace_modifier(
            mob,
            attribute::MAX_HEALTH,
            LEADER_ZOMBIE_BONUS,
            health,
            Operation::AddMultipliedTotal,
        );
        mob.health = mob.attributes.value(attribute::MAX_HEALTH) as f32;
        set_can_break_doors(mob, true);
    }
}

/// `AttributeInstance.addOrReplacePermanentModifier`。
fn replace_modifier(mob: &mut Mob, id: i32, name: &str, amount: f64, operation: Operation) {
    let Some(instance) = mob.attributes.instance(id) else {
        return;
    };
    instance.remove_modifier(name);
    instance.modifiers.push(Modifier {
        id: name.to_owned(),
        amount,
        operation,
        permanent: true,
    });
}

/// `Husk.finalizeSpawn`：僵尸那一套之后再抽一次捡物；自然生成时，骆驼尸壳的生成盒
/// 没有碰撞就 `nextFloat() < 0.1` 拿铁矛。Bamboo 还没有骆驼尸壳和焦尸，命中时只给长矛，
/// 不生成坐骑和骑手。
///
/// `Husk.finalizeSpawn`: after the zombie part, roll loot pickup again; for natural
/// spawns a clear camel-husk spawn box gives `nextFloat() < 0.1` for an iron spear.
/// Bamboo has no camel husks or parched yet, so a hit only grants the spear without the
/// mount and rider.
fn husk_finalize_spawn(
    world: &mut World<'_>,
    entity: &mut Entity,
    reason: SpawnReason,
    special: f32,
) {
    let can_pick_up_loot = world.random().next_float() < 0.55 * special;
    entity.with_mob(|_, mob| mob.can_pick_up_loot = can_pick_up_loot);
    if reason != SpawnReason::Natural {
        return;
    }
    let area = camel_husk_spawn_box(entity.block_position());
    if super::natural::no_collision(world, area) && world.random().next_float() < 0.1 {
        entity.with_mob(|_, mob| {
            mob.equipment.set(
                EquipmentSlot::MainHand,
                ItemStack::new(items::IRON_SPEAR, 1),
            );
        });
    }
}

/// `EntityTypes.CAMEL_HUSK.getSpawnAABB(x + 0.5, y, z + 0.5)`。
fn camel_husk_spawn_box(pos: BlockPos) -> Aabb {
    let facts = &mc_bamboo_data::entity_types::FACTS[entity_type::CAMEL_HUSK as usize];
    let half = f64::from(facts.width / 2.0);
    let x = f64::from(pos.x) + 0.5;
    let z = f64::from(pos.z) + 0.5;
    let y = f64::from(pos.y);
    Aabb::new(
        x - half,
        y,
        z - half,
        x + half,
        y + f64::from(facts.height),
        z + half,
    )
}
