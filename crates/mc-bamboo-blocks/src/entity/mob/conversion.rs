//! 生物转化：官方 `ConversionTracker`（受影响计时 → 转化计时）和 `Mob.convertTo` 的
//! `ConversionType.SINGLE`（新建目标实体、搬状态、丢掉原来那只）。
//!
//! 骷髅在细雪里变流浪者用它；僵尸溺水变溺尸在官方是同一个类，以后可以直接复用。
//! 计数全是相对值，没有绝对时刻，岛拆合时随实体行整行搬。
//!
//! Mob conversion: vanilla `ConversionTracker` (affliction timer, then conversion timer)
//! and the `ConversionType.SINGLE` branch of `Mob.convertTo` (create the target, move the
//! state over, discard the source).
//!
//! Skeletons freezing into strays use it; zombies drowning into drowned use the same
//! vanilla class and can reuse this module. All counters are relative, so the tracker
//! moves with the entity row on island splits and merges.

use mc_bamboo_core::Vec3;
use mc_bamboo_nbt::Tag;

use super::{Mob, MobType, equipment::SLOTS, living, spawn};
use crate::{
    archive::tag,
    entity::{
        Entity,
        ctx::{Command, EntityCtx},
    },
    world::World,
};

/// `Entity.addTag` 的上限。
///
/// The cap of `Entity.addTag`.
const MAX_ENTITY_TAGS: usize = 1024;

/// 一个 `ConversionTracker` 的可变状态。`converting` 是那一位同步元数据。
///
/// The mutable state of one `ConversionTracker`. `converting` is its synced boolean.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ConversionTracker {
    /// `afflictionTime`：连续受影响的刻数，不受影响时是 -1。
    pub affliction_time: i32,
    /// `conversionTime`：转化倒计时，减到负数时转化。
    pub conversion_time: i32,
    pub converting: bool,
}

impl ConversionTracker {
    /// `ConversionTracker.tick` 里 `isAlive() && !isNoAi()` 之后的部分。返回真表示这一刻
    /// 该 `doConversion`。
    ///
    /// The body of `ConversionTracker.tick` past the `isAlive() && !isNoAi()` guard.
    /// Returns true when `doConversion` is due this tick.
    pub fn tick(&mut self, afflicted: bool, total_affliction: i32, total_conversion: i32) -> bool {
        if !afflicted {
            self.affliction_time = -1;
            self.converting = false;
            return false;
        }
        if self.converting {
            self.conversion_time -= 1;
            return self.conversion_time < 0;
        }
        self.affliction_time += 1;
        if self.affliction_time >= total_affliction {
            self.start(total_conversion);
        }
        false
    }

    /// `startConversion`。
    pub const fn start(&mut self, time: i32) {
        self.conversion_time = time;
        self.converting = true;
    }

    /// `addAdditionalSaveData`：不在转化 / 不受影响时两项各写 -1。
    ///
    /// `addAdditionalSaveData`: each entry is -1 when not converting / not afflicted.
    pub fn write(
        &self,
        out: &mut Vec<(String, Tag)>,
        conversion_key: &str,
        affliction_key: &str,
        afflicted: bool,
    ) {
        let conversion = if self.converting {
            self.conversion_time
        } else {
            -1
        };
        let affliction = if afflicted { self.affliction_time } else { -1 };
        out.push((conversion_key.to_owned(), Tag::Int(conversion)));
        out.push((affliction_key.to_owned(), Tag::Int(affliction)));
    }

    /// `readAdditionalSaveData`。
    pub fn read(&mut self, source: &Tag, conversion_key: &str, affliction_key: &str) {
        self.affliction_time = tag::int_or(source, affliction_key, 0);
        let conversion = tag::int_or(source, conversion_key, -1);
        if conversion == -1 {
            self.converting = false;
        } else {
            self.start(conversion);
        }
    }
}

/// `Skeleton.freezingTracker` 的参数：140 刻受冻、300 刻转化、世界事件 1048。
///
/// The `Skeleton.freezingTracker` parameters: 140 ticks afflicted, 300 converting, level
/// event 1048.
const FREEZE_TICKS: i32 = 140;
const STRAY_CONVERSION_TICKS: i32 = 300;
const SKELETON_CONVERTED_TO_STRAY: i32 = 1048;
pub(super) const STRAY_CONVERSION_KEY: &str = "StrayConversionTime";
pub(super) const FREEZING_KEY: &str = "FreezingTime";

/// `Skeleton.tick` 开头的 `freezingTracker.tick()`。`isInPowderSnow` 是上一刻方块效果留下
/// 的值（`baseTick` 还没清）。到点时 `convertTo`：和平难度下 `EntityType.create` 返回空，
/// 什么都不发生；否则造出流浪者，自己 `discard`，这一刻剩下的 `baseTick` 照跑、`aiStep` 不跑。
///
/// The `freezingTracker.tick()` at the head of `Skeleton.tick`. `isInPowderSnow` still holds
/// last tick's block effect. When due, `convertTo` does nothing in peaceful (`EntityType.create`
/// returns null); otherwise the stray is created and the skeleton discards itself, with the
/// rest of `baseTick` still running and `aiStep` skipped.
pub(super) fn tick_freezing(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    if !living::is_alive(me, mob) || mob.no_ai {
        return;
    }
    let due = mob
        .freeze_conversion
        .tick(mob.in_powder_snow, FREEZE_TICKS, STRAY_CONVERSION_TICKS);
    if !due || ctx.blocks().difficulty() == 0 {
        return;
    }
    ctx.emit(
        Command::ConvertMob {
            from: Box::new(mob.clone()),
            to: MobType::Stray,
            level_event: SKELETON_CONVERTED_TO_STRAY,
            zombie_attributes: false,
        },
        me,
    );
    me.discard();
}

/// 落地 [`crate::entity::ctx::Command::ConvertMob`]：`Mob.convertTo(type,
/// ConversionParams.single(mob, true, true), afterConversion)`。原来那只由发起的 tick
/// 自己 `discard`；这里造新的一只、搬状态、放进世界，再发转化音效的世界事件。
///
/// Lands [`crate::entity::ctx::Command::ConvertMob`]: `Mob.convertTo(type,
/// ConversionParams.single(mob, true, true), afterConversion)`. The source discards itself
/// in its tick; this creates the target, moves the state, adds it to the world and then
/// plays the conversion level event.
pub(crate) fn apply_convert(
    me: &Entity,
    from: &Mob,
    to: MobType,
    level_event: i32,
    zombie_attributes: bool,
    world: &mut World,
) {
    // `EntityType.create`：构造函数取种子、UUID 和朝向，和 `/summon` 同一条路。
    // `EntityType.create`: the constructor draws the seed, UUID and facing as `/summon` does.
    let mut entity = world.prepare_entity(Entity::by_mob_type(to, Vec3::default()));
    // `ConversionType.SINGLE.convert`：`copyPosition` 就是 `snapTo`。
    // `ConversionType.SINGLE.convert`: `copyPosition` is a `snapTo`.
    spawn::snap_to(&mut entity, me.position, me.y_rot, from.x_rot);
    entity.delta = me.delta;
    entity.fall_distance = me.fall_distance;
    entity.on_ground = me.on_ground;
    entity.with_mob(|_, mob| {
        copy_state(from, mob);
        // `ZombieVillager` 与 `Villager` 的互转：官方的 `AfterConversion` 把
        // `VillagerData` / `Gossips` / `Offers` / `Xp` 和 finalized 一起搬过去。
        //
        // `ZombieVillager` <-> `Villager` conversions: the official
        // `AfterConversion` carries `VillagerData`, `Gossips`, `Offers`, `Xp`
        // and the finalized flag over.
        if matches!(
            (from.mob_type, to),
            (
                super::MobType::Villager,
                super::MobType::ZombieVillager
            ) | (
                super::MobType::ZombieVillager,
                super::MobType::Villager
            )
        ) {
            super::zombie_villager::copy_villager_state(from, mob);
        }
        if to == super::MobType::Villager && from.mob_type == super::MobType::ZombieVillager {
            if let Some(player) = from.zombie_villager.conversion_player {
                mob.events.push(super::MobEvent::CuredZombieVillager { player });
            }
            // 治疗完成：新村民带 200 刻反胃；治疗者的进度事件由游戏层按
            // `ConversionPlayer` 发（声望系统还没建模）。
            //
            // Cure complete: the new villager gets 200 ticks of nausea. The
            // player's advancement trigger is emitted by the game layer from
            // `ConversionPlayer` (gossip is not modelled yet).
            super::zombie_villager::after_cure(mob);
        }
    });
    if zombie_attributes {
        // 僵尸的 `postConversionStep`：`converted.handleAttributes(specialMultiplier,
        // CONVERSION)`，取新实体自己的随机源。
        //
        // The zombie `postConversionStep`: `converted.handleAttributes(specialMultiplier,
        // CONVERSION)` on the new entity's own random.
        let special = super::zombie_spawn::special_multiplier(
            crate::entity::ctx::BlockView::effective_difficulty(world, entity.block_position()),
        );
        entity.with_mob(|me, mob| {
            super::zombie_spawn::handle_attributes(mob, &mut me.random, special);
        });
    }
    // `Zombie.convertVillagerToZombieVillager` 的 `AfterConversion`：
    // `finalizeSpawn(CONVERSION, ZombieGroupData(false, true))`，然后按村民的
    // 数据覆盖职业 / 八卦 / 交易 / 经验。僵尸侧 `finalizeSpawn` 的随机数用
    // 世界随机源；Bamboo 在构造时已经取过实体的构造随机数，这里只补世界那一段。
    //
    // `Zombie.convertVillagerToZombieVillager`'s `AfterConversion`:
    // `finalizeSpawn(CONVERSION, ZombieGroupData(false, true))`, then the villager's
    // data overwrites profession / gossip / offers / XP.
    let infected = from.mob_type == MobType::Villager && to == MobType::ZombieVillager;
    if infected {
        let mut group = Some(super::spawn::SpawnGroup::Zombie(
            super::zombie_spawn::ZombieGroupData {
                is_baby: false,
                can_spawn_jockey: true,
            },
        ));
        super::zombie_spawn::finalize_spawn(
            world,
            to,
            &mut entity,
            super::spawn::SpawnReason::Conversion,
            &mut group,
        );
    }
    if from.mob_type == MobType::ZombieVillager && to == MobType::Villager {
        // 治疗完成：官方让新村民跑一次 `finalizeSpawn(CONVERSION, null)`，
        // 已 finalized 时只会给跟随距离加成和左撇子；这里只需把治疗者的
        // UUID 交给游戏层发进度触发器。
        //
        // Cure complete: vanilla runs `finalizeSpawn(CONVERSION, null)` on the
        // new villager; a finalized one only gets the follow-range bonus and
        // left-handedness. The game layer uses the starter UUID for the trigger.
    }
    let pos = entity.block_position();
    world.add_prepared_entity(entity);
    if !from.silent {
        world.push_effect(crate::Effect::LevelEvent {
            event: level_event,
            pos,
            data: 0,
            except_initiator: false,
        });
    }
}

/// `SINGLE.convert` 里装备以下的部分加 `convertCommon`。乘客、载具、拴绳、睡觉位置和
/// 队伍 Bamboo 没有建模。
///
/// The part of `SINGLE.convert` from the equipment on, plus `convertCommon`. Passengers,
/// vehicles, leashes, sleeping positions and teams are not modelled.
fn copy_state(from: &Mob, to: &mut Mob) {
    for slot in SLOTS {
        let stack = from.equipment.get(slot);
        if !stack.is_empty() {
            to.equipment.set(slot, stack.clone());
            to.equipment
                .set_drop_chance(slot, from.equipment.drop_chance(slot));
        }
    }
    to.fall_flying = from.fall_flying;
    to.last_hurt_by_player_memory_time = from.last_hurt_by_player_memory_time;
    to.hurt_time = from.hurt_time;
    to.y_body_rot = from.y_body_rot;
    copy_common(from, to, true);
}

/// `ConversionType.convertCommon`：两种转化共用的部分。`SPLIT_ON_DEATH`（史莱姆分裂）只跑
/// 这一段。`preserve_can_pick_up_loot` 是 `ConversionParams.preserveCanPickUpLoot`。
/// `setSharedFlagOnFire` 有意略过：着火标志由剩余火焰刻数推出来。
///
/// `ConversionType.convertCommon`: the part both conversion types share; `SPLIT_ON_DEATH`
/// (slime splitting) runs only this. `preserve_can_pick_up_loot` is
/// `ConversionParams.preserveCanPickUpLoot`. `setSharedFlagOnFire` is deliberately skipped:
/// the fire flag derives from the remaining fire ticks.
pub(super) fn copy_common(from: &Mob, to: &mut Mob, preserve_can_pick_up_loot: bool) {
    to.absorption = from.absorption;
    for effect in from.effects.entries() {
        living::add_effect(to, effect);
    }
    if from.is_baby() && to.mob_type.uses_baby_metadata() {
        to.set_zombie_baby(true);
    }
    if preserve_can_pick_up_loot {
        to.can_pick_up_loot = from.can_pick_up_loot;
    }
    to.left_handed = from.left_handed;
    to.no_ai = from.no_ai;
    to.persistence_required |= from.persistence_required;
    to.custom_name_visible = from.custom_name_visible;
    to.invulnerable = from.invulnerable;
    to.invulnerable_time = from.invulnerable_time;
    to.no_gravity = from.no_gravity;
    to.portal_cooldown = from.portal_cooldown;
    to.silent = from.silent;
    for name in &from.tags {
        if to.tags.len() < MAX_ENTITY_TAGS && !to.tags.contains(name) {
            to.tags.push(name.clone());
        }
    }
    // `COMPONENTS_TO_COPY`：`custom_name` 和 `custom_data`（存档里的 `data`）。
    // `COMPONENTS_TO_COPY`: `custom_name` and `custom_data` (the saved `data` key).
    if from.custom_name.is_some() {
        to.custom_name.clone_from(&from.custom_name);
    }
    if let Some(data) = from.passthrough.iter().find(|(key, _)| key == "data") {
        to.passthrough.retain(|(key, _)| key != "data");
        to.passthrough.push(data.clone());
    }
    if from.can_break_doors && to.mob_type.uses_baby_metadata() {
        to.can_break_doors = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 140 刻受影响后开始转化，再过 300 刻（倒计时减到 -1）才转化；中途离开就清零。
    ///
    /// Conversion starts after 140 afflicted ticks and fires 301 ticks later when the
    /// countdown goes negative; leaving the affliction resets everything.
    #[test]
    fn tracker_follows_the_vanilla_counters() {
        let mut tracker = ConversionTracker::default();
        for _ in 0..139 {
            assert!(!tracker.tick(true, 140, 300));
            assert!(!tracker.converting);
        }
        assert!(!tracker.tick(true, 140, 300));
        assert!(tracker.converting);
        assert_eq!(tracker.conversion_time, 300);
        for _ in 0..300 {
            assert!(!tracker.tick(true, 140, 300));
        }
        assert!(tracker.tick(true, 140, 300));
        assert!(!tracker.tick(false, 140, 300));
        assert_eq!((tracker.affliction_time, tracker.converting), (-1, false));
    }

    #[test]
    fn tracker_nbt_uses_minus_one_for_idle_entries() {
        let mut out = Vec::new();
        let mut tracker = ConversionTracker::default();
        tracker.write(&mut out, "StrayConversionTime", "FreezingTime", false);
        assert_eq!(
            out,
            vec![
                ("StrayConversionTime".to_owned(), Tag::Int(-1)),
                ("FreezingTime".to_owned(), Tag::Int(-1)),
            ]
        );
        tracker.start(17);
        tracker.affliction_time = 140;
        let mut out = Vec::new();
        tracker.write(&mut out, "StrayConversionTime", "FreezingTime", true);
        let mut loaded = ConversionTracker::default();
        loaded.read(&Tag::Compound(out), "StrayConversionTime", "FreezingTime");
        assert_eq!(loaded, tracker);
    }
}

/// `ConversionParams.single(mob, keepEquipment, preserveCanPickUpLoot)` 的两个开关。
///
/// The two switches of `ConversionParams.single`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ConversionParams {
    pub keep_equipment: bool,
    pub preserve_can_pick_up_loot: bool,
}

/// `ConversionType.SINGLE.convert`：`to` 已经构造好（ID、种子、UUID、朝向都取过），
/// 这里只抄状态。血量不抄，新生物是满血；`yHeadRot` 保留构造时抽的值。
///
/// `ConversionType.SINGLE.convert` onto an already constructed `to`. Health is not
/// copied, and `yHeadRot` keeps the constructor's draw.
pub(crate) fn convert_single(from: &Entity, to: &mut Entity, params: ConversionParams) {
    let Some(old) = from.kind.mob() else {
        return;
    };
    spawn::snap_to(to, from.position, from.y_rot, old.x_rot);
    to.delta = from.delta;
    to.fall_distance = from.fall_distance;
    to.on_ground = from.on_ground;
    to.with_mob(|me, mob| {
        if params.keep_equipment {
            for slot in super::equipment::SLOTS {
                let stack = old.equipment.get(slot);
                if !stack.is_empty() {
                    mob.equipment.set(slot, stack.clone());
                    mob.equipment
                        .set_drop_chance(slot, old.equipment.drop_chance(slot));
                }
            }
        }
        mob.fall_flying = old.fall_flying;
        mob.last_hurt_by_player_memory_time = old.last_hurt_by_player_memory_time;
        mob.hurt_time = old.hurt_time;
        mob.y_body_rot = old.y_body_rot;
        convert_common(old, me, mob, params);
    });
}

/// `ConversionType.convertCommon`。
///
/// `ConversionType.convertCommon`.
fn convert_common(
    old: &super::Mob,
    me: &mut Entity,
    mob: &mut super::Mob,
    params: ConversionParams,
) {
    mob.absorption = old.absorption;
    for effect in old.effects.entries() {
        living::add_effect(mob, effect);
    }
    if !old.mob_type.is_monster() && !mob.mob_type.is_monster() {
        living::set_age(me, mob, old.age);
        mob.forced_age = old.forced_age;
        mob.forced_age_timer = old.forced_age_timer;
    }
    if params.preserve_can_pick_up_loot {
        mob.can_pick_up_loot = old.can_pick_up_loot;
    }
    mob.left_handed = old.left_handed;
    mob.no_ai = old.no_ai;
    if old.persistence_required {
        mob.persistence_required = true;
    }
    mob.custom_name_visible = old.custom_name_visible;
    mob.visible_fire = old.visible_fire;
    mob.invulnerable = old.invulnerable;
    mob.invulnerable_time = old.invulnerable_time;
    mob.no_gravity = old.no_gravity;
    mob.portal_cooldown = old.portal_cooldown;
    mob.silent = old.silent;
    for tag in &old.tags {
        if !mob.tags.contains(tag) {
            mob.tags.push(tag.clone());
        }
    }
    // `COMPONENTS_TO_COPY`：`CUSTOM_NAME` 和 `CUSTOM_DATA`（存档里的 `data`）。
    // `COMPONENTS_TO_COPY`: `CUSTOM_NAME` and `CUSTOM_DATA` (the saved `data` key).
    if old.custom_name.is_some() {
        mob.custom_name = old.custom_name.clone();
    }
    if let Some(data) = old.passthrough.iter().find(|(key, _)| key == "data") {
        mob.passthrough.retain(|(key, _)| key != "data");
        mob.passthrough.push(data.clone());
    }
}

/// `Zombie.killedEntity` 里僵尸杀死村民的转化分支。
///
/// 官方在 `Monster.killedEntity` 之后判断：难度为 NORMAL 或 HARD、死者是 `Villager`
/// 且攻击者是僵尸时，NORMAL 先用僵尸自己的随机源 `nextBoolean()` 掷一次（为真就不转化），
/// HARD 必定，然后 `convertVillagerToZombieVillager` 生成僵尸村民；转化成功会把击杀统计
/// 置为假（Bamboo 的击杀统计没建模，这里只跳过死亡掉落）。简单难度不转化、也不抽数。
/// 抽签已经在攻击者那一侧的 `doHurtTarget` 里完成，随 [`super::DamageSource`] 带过来。
///
/// The villager conversion branch of `Zombie.killedEntity`. Vanilla converts a villager
/// killed by a zombie on NORMAL (after the zombie's own `nextBoolean()` roll) or HARD;
/// EASY neither converts nor draws. The roll is made on the attacker side in
/// `doHurtTarget` and carried in [`super::DamageSource`].
pub(crate) fn zombie_killed_villager(
    me: &mut Entity,
    mob: &mut super::Mob,
    ctx: &mut dyn EntityCtx,
    source: &super::DamageSource,
) -> bool {
    let difficulty = ctx.blocks().difficulty();
    if !matches!(difficulty, 2 | 3) || mob.mob_type != super::MobType::Villager {
        return false;
    }
    let Some(attacker_type) = source.attacker_type else {
        return false;
    };
    if !super::MobType::by_entity_type(attacker_type).is_some_and(|kind| kind.is_zombie()) {
        return false;
    }
    // `nextBoolean()` 为真直接返回，不转化也不生成。
    // A true `nextBoolean()` returns without converting.
    if source.villager_conversion_roll.is_some_and(|roll| roll) {
        return false;
    }
    ctx.emit(
        Command::ConvertMob {
            from: Box::new(mob.clone()),
            to: super::MobType::ZombieVillager,
            level_event: super::zombie_villager::ZOMBIE_INFECTED_EVENT,
            zombie_attributes: false,
        },
        me,
    );
    true
}
