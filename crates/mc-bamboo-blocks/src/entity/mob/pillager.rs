//! 掠夺者：官方 `PatrollingMonster` → `Raider` → `AbstractIllager` → `Pillager`。
//!
//! 自己的行为：拿弩（`RangedCrossbowAttackGoal`，装填 25 刻、等 20–39 刻再射）、巡逻
//! （`LongDistancePatrolGoal`）、巡逻途中先原地对峙再进攻（`HoldGroundAttackGoal`）。
//! 生成时主手一把弩，三百分之一带穿透 I；队长头上戴必掉的灾厄旗帜。
//!
//! 袭击不在范围内：`Raider` 注册的四个袭击目标只占着注册顺序（`canUse` 恒假、不取
//! 随机数），`Wave` / `CanJoinRaid` / `RaidId` 和 5 格背包只当存档数据。
//!
//! Pillager: vanilla `PatrollingMonster` → `Raider` → `AbstractIllager` → `Pillager`. Its
//! own behaviour: the crossbow (`RangedCrossbowAttackGoal`: 25 ticks to load, 20–39 more
//! before firing), patrols (`LongDistancePatrolGoal`) and the stand-off before a patrol
//! attacks (`HoldGroundAttackGoal`). It spawns with a crossbow, Piercing I one time in
//! 300, and a leader wears an always-dropped ominous banner. Raids are out of scope: the
//! four `Raider` raid goals only keep their registration slots (`canUse` is false and
//! draws nothing), and `Wave` / `CanJoinRaid` / `RaidId` and the 5-slot inventory are
//! stored data only.

use mc_bamboo_core::Vec3;
use mc_bamboo_data::{items, registries::attribute};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::{item::ItemStack, packet::play::EquipmentSlot};

use super::{
    Mob,
    attributes::Attributes,
    crossbow::CrossbowAttack,
    goals::{Goal, GoalSelector, TargetType},
    navigation::Navigation,
    raider::RaiderGoal,
    spawn::SpawnReason,
};
use crate::{World, entity::Entity, entity::ctx::BlockView};

/// 掠夺者自己的状态：`InventoryCarrier` 的 5 格背包，按存档里的 `Inventory` 列表原样保存。
/// 官方只在袭击里往里放白旗，这里不会改它。
///
/// The pillager's own state: the 5-slot `InventoryCarrier` inventory, kept as the saved
/// `Inventory` list. Vanilla only fills it with white banners during raids; nothing
/// changes it here.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Pillager {
    pub inventory: Vec<Tag>,
}

/// `Pillager.createAttributes()`：`Monster` 的缺省加 0.35 移速、24 血、5 攻击、32 跟随距离。
///
/// `Pillager.createAttributes()`: the monster defaults plus 0.35 speed, 24 health, 5
/// attack damage and a follow range of 32.
pub(super) fn attributes() -> Attributes {
    let mut table = Attributes::skeleton();
    table.set_base(attribute::MOVEMENT_SPEED, f64::from(0.35_f32));
    table.set_base(attribute::MAX_HEALTH, 24.0);
    table.set_base(attribute::ATTACK_DAMAGE, 5.0);
    table.set_base(attribute::FOLLOW_RANGE, 32.0);
    table
}

/// `Pillager.registerGoals`：父类链先注册——`PatrollingMonster` 的巡逻、`Raider` 的四个
/// 袭击目标——然后是自己的七个。`AvoidEntityGoal<Creaking>` 没有嘎枝可躲，只占位。
///
/// `Pillager.registerGoals`: the superclass chain registers first — the
/// `PatrollingMonster` patrol and the four `Raider` raid goals — then its own seven.
/// `AvoidEntityGoal<Creaking>` has no creaking to avoid and only keeps its slot.
pub(super) fn goals(navigation: &mut Navigation) -> GoalSelector {
    let mut goals = vec![(4, Goal::Raider(RaiderGoal::patrol()))];
    goals.extend(super::raider::raider_goals(
        super::raider::RaiderGoal::raid_goals(),
    ));
    goals.extend([
        (0, Goal::Float),
        (1, Goal::Unavailable("AvoidEntityGoal")),
        (
            2,
            Goal::Raider(RaiderGoal::HoldGround {
                hostile_radius_sqr: 10.0 * 10.0,
            }),
        ),
        (
            3,
            Goal::Raider(RaiderGoal::Crossbow(CrossbowAttack::new(1.0, 8.0))),
        ),
        (
            8,
            Goal::Stroll {
                speed: 0.6,
                interval: 120,
                probability: super::goals::PLAIN_STROLL,
                wanted: Vec3::default(),
                force_trigger: false,
            },
        ),
        (
            9,
            Goal::LookAtPlayer {
                look_distance: 15.0,
                probability: 1.0,
                look_at: None,
                look_time: 0,
            },
        ),
        (
            10,
            Goal::Raider(RaiderGoal::LookAtMob {
                look_distance: 15.0,
                probability: 0.02,
                look_at: None,
                look_time: 0,
            }),
        ),
    ]);
    navigation.flags.can_float = true;
    GoalSelector::from_ordered_pairs(goals)
}

/// `Pillager.registerGoals` 的 `targetSelector`：受伤反击（不记袭击者的仇，会叫同伴）、
/// 玩家、村民、铁傀儡。村民目标 `mustSee=false`。
///
/// The `targetSelector` of `Pillager.registerGoals`: hurt by (ignoring raiders, alerting
/// others), players, villagers and iron golems. The villager goal has `mustSee=false`.
pub(super) fn targets() -> GoalSelector {
    GoalSelector::from_ordered_pairs(vec![
        (
            1,
            Goal::HurtByTarget {
                timestamp: -1,
                unseen_ticks: 0,
            },
        ),
        (2, Goal::nearest_target(TargetType::Player)),
        (3, Goal::nearest_target_without_sight(TargetType::Villager)),
        (3, Goal::nearest_target(TargetType::IronGolem)),
    ])
}

/// `Pillager.finalizeSpawn`：主手放弩、按难度附魔，然后 `Raider`（能加入袭击）、
/// `PatrollingMonster`（队长和巡逻）、`Mob`（跟随距离加成、左撇子）。
///
/// `Pillager.finalizeSpawn`: the crossbow and its enchantments, then `Raider` (may join
/// raids), `PatrollingMonster` (leader and patrol) and `Mob` (follow-range bonus,
/// left-handedness).
pub(super) fn finalize_spawn(world: &mut World<'_>, entity: &mut Entity, reason: SpawnReason) {
    let special = ((BlockView::effective_difficulty(world, entity.block_position()) - 2.0) / 2.0)
        .clamp(0.0, 1.0);
    let random = world.random();
    entity.with_mob(|_, mob| {
        mob.equipment
            .set(EquipmentSlot::MainHand, ItemStack::new(items::CROSSBOW, 1));
        enchant_spawned_weapon(world, mob, random, special);
        mob.raider.can_join_raid = true;
        super::raider::finalize_patrol(mob, random, reason == SpawnReason::Patrol, true);
        super::spawn::finalize_mob(random, mob);
    });
}

/// `Pillager.enchantSpawnedWeapon`：先是 `Mob` 的那次（`0.25 * 特殊倍率` 的概率随机附魔），
/// 再抽 `nextInt(300)`，中了给弩套 `PILLAGER_SPAWN_CROSSBOW`（穿透 I，常量等级不取随机数）。
/// 四个盔甲槽是空的，`enchantSpawnedArmor` 不取数。
///
/// `Pillager.enchantSpawnedWeapon`: the `Mob` roll (random enchantment at `0.25 * special
/// multiplier`), then `nextInt(300)`; a hit applies `PILLAGER_SPAWN_CROSSBOW` (Piercing I,
/// a constant level that draws nothing). The armor slots are empty, so
/// `enchantSpawnedArmor` draws nothing.
fn enchant_spawned_weapon(
    world: &World<'_>,
    mob: &mut Mob,
    random: &crate::random::LegacyRandom,
    special: f32,
) {
    if random.next_float() < 0.25 * special {
        let mut stack = mob.equipment.get(EquipmentSlot::MainHand).clone();
        world
            .level()
            .enchant_mob_spawn_equipment(&mut stack, special);
        mob.equipment.set(EquipmentSlot::MainHand, stack);
    }
    if random.next_int(300) == 0 {
        let mut stack = mob.equipment.get(EquipmentSlot::MainHand).clone();
        if stack.item == items::CROSSBOW {
            add_piercing(&mut stack);
            mob.equipment.set(EquipmentSlot::MainHand, stack);
        }
    }
}

/// `SingleEnchantment(PIERCING, ConstantInt(1))`：`ItemEnchantments.Mutable.upgrade` 只在新
/// 等级更高时改。
///
/// `SingleEnchantment(PIERCING, ConstantInt(1))`: `ItemEnchantments.Mutable.upgrade`
/// only raises a level.
fn add_piercing(stack: &mut ItemStack) {
    use mc_bamboo_data::registries::data_component_type::ENCHANTMENTS;
    use mc_bamboo_protocol::components::{ComponentValue, defaults};
    let Some(piercing) = enchantment_id("minecraft:piercing") else {
        return;
    };
    let mut levels = match defaults::resolve(stack.item, &stack.patch(), ENCHANTMENTS) {
        Some(ComponentValue::Enchantments(levels)) => levels.clone(),
        _ => Vec::new(),
    };
    match levels.iter_mut().find(|(id, _)| *id == piercing) {
        Some((_, level)) => *level = (*level).max(1),
        None => levels.push((piercing, 1)),
    }
    crate::item_exchange::set_component(stack, ENCHANTMENTS, ComponentValue::Enchantments(levels));
}

fn enchantment_id(name: &str) -> Option<i32> {
    mc_bamboo_data::registries::SYNCED_REGISTRIES
        .iter()
        .find(|registry| registry.name == "minecraft:enchantment")?
        .entries
        .iter()
        .position(|entry| *entry == name)
        .map(|index| index as i32)
}

/// `PatrollingMonster.checkPatrollingMonsterSpawnRules`：方块光大于 8 不刷，否则只要脚下
/// 是能生成的方块（`Monster.checkAnyLightMonsterSpawnRules`，和平难度由调用方先挡掉）。
///
/// `PatrollingMonster.checkPatrollingMonsterSpawnRules`: no spawn above block light 8,
/// otherwise only a valid spawn block below (`Monster.checkAnyLightMonsterSpawnRules`;
/// the caller rejects peaceful first).
pub(super) fn check_spawn_rules(world: &World<'_>, pos: mc_bamboo_core::BlockPos) -> bool {
    world
        .level()
        .brightness(mc_bamboo_core::LightLayer::Block, pos)
        <= 8
        && super::natural::valid_mob_spawn_block(world, super::MobType::Pillager, pos)
}

/// `Pillager.addAdditionalSaveData` 自己的那一项：`writeInventoryToTag` 的 `Inventory` 列表。
///
/// The pillager's own save key: the `Inventory` list of `writeInventoryToTag`.
pub(super) fn save(mob: &Mob, out: &mut Vec<(String, Tag)>) {
    out.push((
        "Inventory".to_owned(),
        Tag::List(mob.pillager.inventory.clone()),
    ));
}

/// `Pillager.readAdditionalSaveData`：读回背包，并且读盘之后一律 `setCanPickUpLoot(true)`。
///
/// `Pillager.readAdditionalSaveData`: read the inventory back, and always
/// `setCanPickUpLoot(true)` after loading.
pub(super) fn load(mob: &mut Mob, tag: &Tag) {
    mob.pillager.inventory = match crate::archive::tag::field(tag, "Inventory") {
        Some(Tag::List(items)) => items.clone(),
        _ => Vec::new(),
    };
    mob.can_pick_up_loot = true;
}

#[cfg(test)]
mod tests {
    use super::super::MobType;
    use super::*;

    /// 属性照 `Pillager.createAttributes()`。
    ///
    /// Attributes follow `Pillager.createAttributes()`.
    #[test]
    fn attributes_follow_vanilla() {
        let table = attributes();
        assert_eq!(table.value(attribute::MAX_HEALTH), 24.0);
        assert_eq!(table.base(attribute::MOVEMENT_SPEED), f64::from(0.35_f32));
        assert_eq!(table.base(attribute::FOLLOW_RANGE), 32.0);
        assert_eq!(table.value(attribute::ATTACK_DAMAGE), 5.0);
    }

    /// 目标按官方的注册顺序和优先级。
    ///
    /// Goals keep the vanilla registration order and priorities.
    #[test]
    fn goals_and_targets_follow_vanilla() {
        let mob = Mob::new(MobType::Pillager);
        let names: Vec<(i32, &str)> = mob
            .goals
            .goals
            .iter()
            .map(|wrapped| (wrapped.priority, wrapped.goal.name()))
            .collect();
        assert_eq!(
            names,
            vec![
                (4, "LongDistancePatrolGoal"),
                (1, "ObtainRaidLeaderBannerGoal"),
                (3, "PathfindToRaidGoal"),
                (4, "RaiderMoveThroughVillageGoal"),
                (5, "RaiderCelebration"),
                (0, "FloatGoal"),
                (1, "AvoidEntityGoal"),
                (2, "HoldGroundAttackGoal"),
                (3, "RangedCrossbowAttackGoal"),
                (8, "RandomStrollGoal"),
                (9, "LookAtPlayerGoal"),
                (10, "LookAtPlayerGoal"),
            ]
        );
        let targets: Vec<i32> = mob
            .target_goals
            .goals
            .iter()
            .map(|wrapped| wrapped.priority)
            .collect();
        assert_eq!(targets, vec![1, 2, 3, 3]);
    }

    /// 穿透 I 只升不降。
    ///
    /// Piercing I only upgrades.
    #[test]
    fn piercing_is_added_once() {
        let mut stack = ItemStack::new(items::CROSSBOW, 1);
        add_piercing(&mut stack);
        add_piercing(&mut stack);
        assert_eq!(
            crate::entity::arrow::enchantment_level(&stack, "minecraft:piercing"),
            1
        );
    }

    /// `/summon` 和刷怪蛋给弩，带 NBT 的 `/summon` 不跑 `finalizeSpawn`。
    ///
    /// `/summon` and spawn eggs hand out the crossbow; `/summon` with NBT skips
    /// `finalizeSpawn`.
    #[test]
    fn summon_and_egg_equip_the_crossbow() {
        use super::super::spawn::{spawn_egg, summon_mob};
        use crate::BlockTicks;
        use mc_bamboo_core::BlockPos;
        let mut level = crate::testing::TestLevel::new();
        let mut ticks = BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let position = Vec3::new(0.5, 64.0, 0.5);
        let summon = summon_mob(&mut world, MobType::Pillager, position, None).unwrap();
        let egg = spawn_egg(
            &mut world,
            MobType::Pillager,
            BlockPos::new(1, 64, 0),
            true,
            false,
            None,
        );
        for uid in [summon, egg] {
            let entity = world.entity(uid).unwrap();
            let mob = entity.kind.mob().unwrap();
            assert!(super::super::crossbow::is_holding(mob));
            assert_eq!(mob.health, 24.0);
            assert!(mob.raider.can_join_raid);
            assert!(!mob.raider.patrolling);
        }
        let tag = Tag::Compound(Vec::new());
        let nbt = summon_mob(&mut world, MobType::Pillager, position, Some(&tag)).unwrap();
        let mob = world.entity(nbt).unwrap();
        let mob = mob.kind.mob().unwrap();
        assert!(!super::super::crossbow::is_holding(mob));
        assert!(mob.can_pick_up_loot);
    }
}
