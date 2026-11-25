//! 生物的一刻：`Entity.commonTick` → `LivingEntity.tick` → `Mob.tick`，以及受伤、死亡、
//! 跳跃、行走（`travel`）和推挤。
//!
//! 顺序照 `ServerLevel` 的实体循环：先 `checkDespawn`，再 `commonTick`，再 `tick`。
//! 移动和方块效果在 [`super::motion`]。
//!
//! One mob tick, in the order of the `ServerLevel` entity loop. Movement and block
//! effects live in [`super::motion`].

use mc_bamboo_core::{BlockPos, Vec3, mth};
use mc_bamboo_data::{
    FluidTag,
    registries::{attribute, mob_effect},
    tags::{block_tags, fluid_tags},
};

use super::{
    DamageKind, DamageSource, FluidTracker, Mob, MobEvent,
    control::{
        self, MAX_HEAD_Y_ROT, Operation, clamp_x_rot, look_x_rot, look_y_rot, move_rotlerp,
        rotate_towards,
    },
    effects::EffectAction,
    equine::Voice,
    goals::Flag,
    motion,
    navigation::NavMob,
    path::PathMob,
    timing,
};
use crate::{
    entity::{
        Entity, EntityKind, EntityUid, KindPublic, PrimedTnt,
        ctx::{BlockView, Command, EntityCtx, ThrottlePlan},
    },
    shape::{self, CollisionContext},
};

/// 生物的一刻，`Entity.tick` 交到这里。`kind` 在这期间被换成一个占位。
pub(crate) fn tick(me: &mut Entity, ctx: &mut dyn EntityCtx, plan: ThrottlePlan) {
    let mut kind = std::mem::replace(&mut me.kind, EntityKind::Tnt(PrimedTnt { fuse: 0 }));
    if let EntityKind::Mob(mob) = &mut kind {
        // 计划（分档、豁免、补偿）由驱动在算阶段之前按同一份快照算好；这里不再
        // 重判状态，精确和并行两种驱动才能逐位一致。
        // 测试构建的压测探针：按类型和离最近玩家的距离档记账。`cfg(test)` 之外是空操作。
        #[cfg(test)]
        {
            let mut nearest = f64::INFINITY;
            ctx.blocks().for_each_player(&mut |player| {
                if player.alive && !player.spectator {
                    let dx = player.position.x - me.position.x;
                    let dz = player.position.z - me.position.z;
                    nearest = nearest.min(dx * dx + dz * dz);
                }
            });
            timing::begin_mob!(mob.mob_type.entity_type(), nearest);
        }
        tick_mob(me, mob, ctx, plan);
        timing::end_mob!();
    }
    me.kind = kind;
    super::enderman::flush_block_goal(me, ctx);
}

/// 对一个生物跑一段外部逻辑（受伤、命令），期间 `kind` 也换成占位。
pub(crate) fn with_mob<R>(
    me: &mut Entity,
    f: impl FnOnce(&mut Entity, &mut Mob) -> R,
) -> Option<R> {
    let mut kind = std::mem::replace(&mut me.kind, EntityKind::Tnt(PrimedTnt { fuse: 0 }));
    let result = match &mut kind {
        EntityKind::Mob(mob) => Some(f(me, mob)),
        _ => None,
    };
    me.kind = kind;
    result
}

fn tick_mob(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx, plan: ThrottlePlan) {
    timing::span!(Other);
    // `Silverfish.tick` / `Endermite.tick` 在 `super.tick()` 之前把身体朝向设成头朝向。
    // `Silverfish.tick` / `Endermite.tick` set the body rotation before `super.tick()`.
    if matches!(
        mob.mob_type,
        super::MobType::Silverfish | super::MobType::Endermite
    ) {
        mob.y_body_rot = me.y_rot;
    }
    // 官方在 `ServerLevel.tick` 的 uid 顺序里对每个实体先 `checkDespawn` 再决定 tick；
    // 乘客的这次检查发生在它的 `rideTick` 之后，由驱动补做，这里跳过。
    // Vanilla checks despawn for every entity in uid order before ticking; a
    // passenger's check runs after its `rideTick`, so the driver does it and we skip.
    if me.vehicle.is_none() {
        check_despawn(me, mob, ctx);
    }
    if me.removed {
        return;
    }
    // 独立寻路阶段：上一刻的结果在这一刻开头装进导航 / 留给原调用点取走。
    super::pathfinding::install(mob, ctx.blocks());
    // `Pufferfish.tick` 在 `super.tick()` 之前跑膨胀 / 收缩。
    // `Pufferfish.tick` runs the inflate/deflate machine before `super.tick()`.
    super::fish::pre_tick(me, mob, ctx);
    // `Entity.commonTick`。
    if mob.invulnerable_time > 0 {
        mob.invulnerable_time -= 1;
    }
    me.old_position = me.position;
    mob.y_rot_o = me.y_rot;
    mob.x_rot_o = mob.x_rot;
    me.tick_count += 1;
    // `ZombieVillager.tick`：转化倒计时在 `super.tick()` 之前。
    // `ZombieVillager.tick`: the cure countdown runs ahead of `super.tick()`.
    if mob.mob_type == super::MobType::ZombieVillager {
        super::zombie_villager::tick(me, mob, ctx);
    }
    // `Skeleton.tick`：`freezingTracker.tick()` 在 `super.tick()` 之前。
    // `Skeleton.tick`: `freezingTracker.tick()` runs ahead of `super.tick()`.
    if mob.mob_type == super::MobType::Skeleton {
        super::conversion::tick_freezing(me, mob, ctx);
    }
    // `Creeper.tick` 在 `super.tick()` 之前推进膨胀，可能当刻爆炸。
    // `Creeper.tick` advances the swell before `super.tick()` and may explode here.
    if mob.mob_type == super::MobType::Creeper {
        super::creeper::tick(me, mob, ctx);
    }
    // `SulfurCube.tick` 在 `super.tick()` 之前推进导火索 / 红石点燃。
    if mob.mob_type == super::MobType::SulfurCube {
        super::sulfur_cube::tick(me, mob, ctx);
    }
    // `ShoulderRidingEntity.tick`：上肩冷却先加一。
    // `ShoulderRidingEntity.tick`: the shoulder cooldown goes up first.
    if mob.mob_type == super::MobType::Parrot {
        mob.parrot.ride_cooldown += 1;
    }
    // `Sniffer.tick` 在 `super.tick()` 之前：挖掘时掉种子。
    // `Sniffer.tick` before `super.tick()`: drop the seed while digging.
    if mob.mob_type == super::MobType::Sniffer {
        super::sniffer::tick_before(me, mob, ctx);
    }
    // `LivingEntity.tick` → `Entity.tick` → `Mob.baseTick`。
    base_tick(me, mob, ctx);
    // `LivingEntity.tick`：`updatingUsingItem()` 紧跟 `super.tick()`，在装备比较和
    // `aiStep` 之前。生物里只有弩走这条路。
    // `LivingEntity.tick`: `updatingUsingItem()` right after `super.tick()`, ahead of the
    // equipment diff and `aiStep`. Among mobs only the crossbow uses it.
    if mob.mob_type == super::MobType::Pillager && !me.removed {
        super::crossbow::use_tick(me, mob, ctx);
    }
    detect_equipment_updates(me, mob, ctx);
    // `LivingEntity.tick`：睡在床上但床没了时立刻起床。
    // `LivingEntity.tick`: wake up if sleeping and the bed no longer exists.
    if mob.mob_type == super::MobType::Villager
        && mob.villager.sleeping
        && !super::villager::check_bed_exists(mob, ctx.blocks())
    {
        super::villager::stop_sleeping(me, mob, None, ctx);
    }
    // `Drowned.updateSwimming()` 在 `Entity.baseTick` 里，紧跟流体跟踪之后。
    // `Drowned.updateSwimming()` runs in `Entity.baseTick`, right after the fluid update.
    if mob.mob_type == super::MobType::Drowned {
        let target_in_water = super::goals::current_target_in_water(mob, ctx);
        super::drowned::update_swimming(me, mob, target_in_water);
    }
    if !me.removed {
        if mob.mob_type == super::MobType::Parrot {
            // `Parrot.aiStep` 在 `super.aiStep` 之前的部分。
            // The part of `Parrot.aiStep` before `super.aiStep`.
            super::parrot::before_ai_step(me, mob, ctx);
        }
        ai_step(me, mob, ctx, plan);
        if mob.mob_type == super::MobType::Parrot && !me.removed {
            // `Parrot.aiStep` 结尾的 `calculateFlapping`。
            // `calculateFlapping` at the end of `Parrot.aiStep`.
            super::parrot::after_ai_step(me, mob);
        }
    }
    // `Bee.tick` 在 `super.tick()` 之后的花蜜粒子与翻滚动画。
    if mob.mob_type == super::MobType::Bee && !me.removed {
        super::bee_goals::nectar_particles(me, mob, ctx);
    }
    // `Zombie.tick` 结尾的 `drowningTracker.tick()`，在 `super.tick()` 之后。
    // The `drowningTracker.tick()` at the end of `Zombie.tick`, after `super.tick()`.
    if mob.mob_type == super::MobType::Zombie {
        super::drowned::tick_drowning(me, mob, ctx);
    }
    // `Spider.tick` 在 `super.tick()` 之后把这一刻的 `horizontalCollision` 写进攀爬标志。
    // `Spider.tick` records this tick's `horizontalCollision` after `super.tick()`.
    if mob.mob_type.climbs_walls() && !me.removed {
        super::spider::tick(me, mob);
    }
    // `AbstractNautilus.tick` 在 `super.tick()` 之后：冲刺冷却、气泡取数。
    // `AbstractNautilus.tick` runs after `super.tick()`: dash cooldown and bubbles.
    if super::nautilus::is_nautilus(mob.mob_type) && !me.removed {
        super::nautilus::tick(me, mob);
    }
    // `tickHeadTurn` → `Mob` 改成 `BodyRotationControl.clientTick`。
    body_rotation_tick(me, mob, ctx.blocks().game_time());
    range_checks(me, mob);
    // `LivingEntity.tick`：睡着的生物（狐狸、村民）抬头角清零。
    // `LivingEntity.tick`: a sleeping mob (fox or villager) levels its head.
    if (mob.mob_type == super::MobType::Fox && mob.fox.is_sleeping())
        || (mob.mob_type == super::MobType::Villager && mob.villager.sleeping)
    {
        mob.x_rot = 0.0;
    }
    // `refreshDirtyAttributes` → `onAttributeUpdated(MAX_HEALTH)`：`/summon` 出来的马科
    // 在第一刻才把血量夹到随机出来的最大血量。
    // `refreshDirtyAttributes`: a summoned equine clamps its health on its first tick.
    clamp_health(mob);
    // `Mob.tick`：每 5 刻按乘客和载具重设控制标志。官方 `Mob.updateControlFlags`：
    // 控制乘客是生物、且不在 `#non_controlling_rider` 里时，MOVE / JUMP / LOOK 都关掉
    // （26.3 标签是史莱姆 / 岩浆怪 / 硫磺方块怪，建模范围内是史莱姆和硫磺方块怪；
    // 岩浆怪不在建模范围）。玩家骑手不算生物，标志保持打开；船不在建模范围，
    // JUMP 和 MOVE 同值。
    //
    // Vanilla `Mob.updateControlFlags`: a mob passenger outside
    // `#non_controlling_rider` (slime, magma cube, sulfur cube; slime and sulfur
    // cube are modeled here) disables MOVE / JUMP / LOOK. A player rider is not a
    // mob, so the flags stay on; boats are out of scope.
    if me.tick_count % 5 == 0 {
        let mob_rider = !mob.no_ai
            && mob.first_passenger().is_some_and(|uid| {
                ctx.other(uid).is_some_and(|public| match &public.kind {
                    crate::entity::KindPublic::Mob(rider) => !matches!(
                        rider.mob_type,
                        super::MobType::Slime | super::MobType::SulfurCube
                    ),
                    _ => false,
                })
            });
        let free = !mob_rider;
        mob.goals.set_control_flag(Flag::Move, free);
        mob.goals.set_control_flag(Flag::Jump, free);
        mob.goals.set_control_flag(Flag::Look, free);
    }
    // `AbstractCubeMob.tick`：`super.tick()` 之后的落地判断。硫磺方块怪的
    // `getParticleType()` 是固定的 `sulfur_cube_goo`，这里用同一套取数顺序，
    // 声音换成它自己的 squish / bounce。
    if matches!(
        mob.mob_type,
        super::MobType::Slime | super::MobType::SulfurCube
    ) {
        super::slime::after_tick(me, mob);
    }
    // `PolarBear.tick` 在 `super.tick()` 之后：吼叫冷却和持久愤怒。
    // `PolarBear.tick` after `super.tick()`: the growl cooldown and persistent anger.
    if mob.mob_type == super::MobType::PolarBear {
        super::polar_bear::tick_end(me, mob, ctx);
    }
    // `Panda.tick` 在 `super.tick()` 之后：担心基因、不乐意计数、喷嚏和打滚。
    // `Panda.tick` after `super.tick()`: the worried gene, the unhappy counter,
    // sneezing and rolling.
    if mob.mob_type == super::MobType::Panda {
        super::panda::tick_end(me, mob, ctx);
    }
    if mob.mob_type == super::MobType::Fox && !me.removed {
        super::fox::tick_after(me, mob, ctx);
    }
    if mob.mob_type == super::MobType::Wolf && !me.removed {
        super::wolf::tick_after(me, mob, ctx);
    }
    if mob.mob_type == super::MobType::Cat && !me.removed {
        super::cat::tick_after(me, mob);
    }
    // `Armadillo.tick` 在 `super.tick()` 之后夹头、数 `inStateTicks`。
    // `Armadillo.tick` clamps the head and counts `inStateTicks` after `super.tick()`.
    if mob.mob_type == super::MobType::Armadillo {
        super::armadillo::tick_after(mob);
    }
    // `Bat.tick` 在 `super.tick()` 之后：倒挂时钉住位置，飞行时竖直速度乘 0.6。
    // `Bat.tick` after `super.tick()`: pin a hanging bat, damp a flying bat's vertical speed.
    if mob.mob_type == super::MobType::Bat {
        super::bat::tick_after(me, mob);
    }
    // `Camel.tick` 在 `super.tick()` 之后：冲刺、冷却、夹头、坐在水里站起。
    // `Camel.tick` after `super.tick()`: dash, cooldown, head clamp, stand up in water.
    if mob.mob_type.is_camel() {
        super::camel::tick_after(me, mob, ctx);
    }
    // `AbstractHorse.tick` 在 `super.tick()` 之后推进计时器。
    // `AbstractHorse.tick` advances its timers after `super.tick()`.
    if mob.mob_type.is_equine() {
        super::equine::tick_end(mob);
    }
    // `SkeletonTrapGoal.tick` 每刻都跑（Bamboo 的目标选择器有隔刻判定），
    // 在 `Mob.tick` 结尾单独看一次 `hasNearbyAlivePlayer`；命中就发落地命令。
    if mob.mob_type == super::MobType::SkeletonHorse
        && mob.skeleton_horse.as_ref().is_some_and(|state| state.trap)
        && super::skeleton_horse::trap_player_near(me.position, ctx.blocks())
    {
        let spawn = super::skeleton_horse::trap_tick(me, mob, ctx);
        ctx.emit(crate::entity::ctx::Command::SkeletonTrap(spawn), me);
    }
    // `SkeletonHorse.aiStep` 在 `super.aiStep()` 之后数陷阱时间，到 18000 刻消失。
    // `SkeletonHorse.aiStep` counts the trap lifetime after `super.aiStep()`.
    if mob.mob_type == super::MobType::SkeletonHorse {
        super::skeleton_horse::tick_trap(me, mob);
    }
    // `Dolphin.tick()` 在 `super.tick()` 之后：湿度、干死、搁浅扑腾。
    // `Dolphin.tick()` after `super.tick()`: moistness, drying out, beached flopping.
    if mob.mob_type == super::MobType::Dolphin {
        super::dolphin::tick(me, mob, ctx);
    }
    // `Axolotl.customServerAiStep` 尾部：把 `PLAY_DEAD_TICKS` 记忆映射成元数据。
    // Tail of `Axolotl.customServerAiStep`: map the PLAY_DEAD_TICKS memory to metadata.
    if mob.mob_type == super::MobType::Axolotl {
        super::axolotl::sync_playing_dead(mob);
    }
}

/// `LivingEntity.detectEquipmentUpdates`：在 `aiStep` 之前比较装备，变了的槽发给跟踪的玩家。
/// 靴子槽变化时还要撤掉旧的 location 效果并对当前装备立即重跑一次。
///
/// `LivingEntity.detectEquipmentUpdates`: compare equipment before `aiStep`. A boot-slot
/// change also deactivates the old location effects and re-runs on current equipment.
fn detect_equipment_updates(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    let update = mob.equipment.detect_updates(&mut mob.attributes);
    let feet_changed = update
        .changes
        .iter()
        .any(|(slot, _)| *slot == mc_bamboo_protocol::packet::play::EquipmentSlot::Feet);
    if update.hand_swap {
        mob.events.push(MobEvent::EntityEvent { event: 55 });
    }
    if !update.changes.is_empty() {
        mob.events.push(MobEvent::Equipment(update.changes));
    }
    if feet_changed {
        super::enchantment_wiring::reequip(me, mob, ctx);
    }
}

/// `Mob.burnUndead`：晒到太阳时，`sunProtectionSlot()`（头）有东西就替它挡；可损坏的
/// 扣 `nextInt(2)` 点耐久（坏了广播实体事件 49），不可损坏的什么也不做。头上空着才着火 8 秒。
fn burn_undead(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    use mc_bamboo_protocol::packet::play::EquipmentSlot;
    let slot = EquipmentSlot::Head;
    let stack = mob.equipment.get(slot);
    if stack.is_empty() {
        motion::ignite_for_seconds(me, mob, 8.0);
        return;
    }
    if !crate::durability::is_damageable_stack(stack) {
        return;
    }
    let amount = me.random.next_int_bounded(2);
    let before = stack.clone();
    let mut broke = false;
    let blocks = ctx.blocks();
    mob.equipment.edit(slot, |stack| {
        broke = blocks.hurt_mob_equipment(stack, amount);
    });
    if broke {
        // `LivingEntity.onEquippedItemBroken`：广播破损事件，当场摘掉坏之前那件的属性修饰
        // （`stopLocationBasedEffects`）；下一刻的装备比较再摘一次是空操作。
        mob.events.push(MobEvent::EntityEvent {
            event: super::equipment::break_event(slot),
        });
        super::equipment::remove_stack_modifiers(&before, slot, &mut mob.attributes);
    }
}

/// `LivingEntity.tick` 的 `rangeChecks`。
fn range_checks(me: &Entity, mob: &mut Mob) {
    fn fold(current: f32, old: &mut f32) {
        while current - *old < -180.0 {
            *old -= 360.0;
        }
        while current - *old >= 180.0 {
            *old += 360.0;
        }
    }
    fold(me.y_rot, &mut mob.y_rot_o);
    fold(mob.y_body_rot, &mut mob.y_body_rot_o);
    fold(mob.x_rot, &mut mob.x_rot_o);
    fold(mob.y_head_rot, &mut mob.y_head_rot_o);
}

/// `isAlive()`。
pub(crate) fn is_alive(me: &Entity, mob: &Mob) -> bool {
    !me.removed && mob.health > 0.0
}

/// `Mob.checkDespawn`。牛不会被远离玩家删掉，但最近的玩家决定 `noActionTime` 归不归零。
pub(crate) fn check_despawn(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    // 和平难度 + `!isAllowedInPeaceful()` 直接丢弃，且不再取随机数。
    if ctx.blocks().difficulty() == 0 && !mob.mob_type.allowed_in_peaceful() {
        me.discard();
        return;
    }
    let persistent = mob.requires_custom_persistence();
    let mut nearest: Option<f64> = None;
    ctx.blocks().for_each_player(&mut |player| {
        if player.spectator {
            return;
        }
        let distance = player.position.distance_to_sqr(me.position);
        if nearest.is_none_or(|best| distance < best) {
            nearest = Some(distance);
        }
    });
    let Some(distance) = nearest else {
        return;
    };
    let far = mob.mob_type.despawn_distance();
    let far_sqr = f64::from(far * far);
    let near_sqr = 32.0 * 32.0;
    // 官方第一段：超过 `despawnDistance` 且 `removeWhenFarAway` 为真立刻丢弃，
    // 不看 `noActionTime`，也不取随机数。牛恒为假，行为与原来一致。
    if !persistent && distance > far_sqr && mob.removes_when_far_away_at(distance, me.tick_count) {
        me.discard();
        return;
    }
    if !persistent
        && mob.no_action_time > 600
        && me.random.next_int_bounded(800) == 0
        && distance > near_sqr
    {
        // 官方第二段：`noDespawnDistance`（所有类别都是 32）之外的 1/800 消失。
        if mob.removes_when_far_away_at(distance, me.tick_count) {
            me.discard();
        }
    } else if distance < near_sqr {
        mob.no_action_time = 0;
    }
}

/// 对一个已经拼好的实体跑 `Mob.checkDespawn`。乘客的检查在 `rideTick` 之后补做，
/// 调用方负责跳过已经移除的实体。
///
/// Run `Mob.checkDespawn` on a fully assembled entity. The driver calls this after
/// a passenger's `rideTick`; callers skip removed entities.
pub(crate) fn check_despawn_entity(me: &mut Entity, ctx: &mut dyn EntityCtx) {
    me.with_mob(|me, mob| check_despawn(me, mob, ctx));
}

// ---- 流体 ----

/// `getFluidHeight(tag)`。
pub(crate) fn fluid_height(mob: &Mob, tag: FluidTag) -> f64 {
    let mut height = 0.0;
    for tracker in &mob.fluid_trackers {
        if tracker.height > height && fluid_is_tagged(tracker.fluid, tag) {
            height = tracker.height;
        }
    }
    height
}

fn fluid_is_tagged(id: i32, tag: FluidTag) -> bool {
    crate::state::fluid_type_is_tagged(id, tag)
}

/// `isEyeInFluid(tag)`。
pub(crate) fn eye_in_fluid(mob: &Mob, tag: FluidTag) -> bool {
    mob.fluid_trackers
        .iter()
        .any(|tracker| tracker.eyes_inside && fluid_is_tagged(tracker.fluid, tag))
}

/// `getFluidJumpThreshold`。
pub(crate) fn fluid_jump_threshold(mob: &Mob) -> f64 {
    if f64::from(mob.eye_height) < 0.4 {
        0.0
    } else {
        0.4
    }
}

/// `isInLava()`。
pub(crate) fn is_in_lava(me: &Entity, mob: &Mob) -> bool {
    !me.first_tick && fluid_height(mob, fluid_tags::LAVA) > 0.0
}

pub(crate) fn is_in_liquid(me: &Entity, mob: &Mob) -> bool {
    me.in_water || is_in_lava(me, mob)
}

/// `getEyeY()`。
pub(crate) fn eye_y(me: &Entity, mob: &Mob) -> f64 {
    me.position.y + f64::from(mob.eye_height)
}

/// `getLastDamageSource()`：40 刻以内的那一次。
pub(crate) fn last_damage_source(mob: &Mob, now: i64) -> Option<DamageSource> {
    if now - mob.last_damage_stamp > 40 {
        None
    } else {
        mob.last_damage_source
    }
}

/// 导航要的生物快照。骑手借载具导航时直接给载具那份，路径按载具的尺寸、位置和
/// 惩罚算（官方 `PathNavigation.mob` 是载具）。
///
/// The navigation snapshot. While a rider borrows the vehicle's navigation this is
/// the vehicle's snapshot, so paths use the vehicle's size, position and malus
/// (vanilla `PathNavigation.mob` is the vehicle).
pub(crate) fn nav_mob(me: &Entity, mob: &Mob, view: &dyn BlockView) -> NavMob {
    mob.borrowed_nav_mob
        .unwrap_or_else(|| nav_mob_own(me, mob, view))
}

/// 不看借用状态，按这个生物自己算快照：骑手 tick 自己那份导航时用。
///
/// The mob's own snapshot, ignoring any borrowed vehicle state; used when a rider
/// ticks its own navigation.
pub(crate) fn nav_mob_own(me: &Entity, mob: &Mob, view: &dyn BlockView) -> NavMob {
    let (width, height) = mob.dimensions();
    NavMob {
        path: PathMob {
            position: me.position,
            on_ground: me.on_ground,
            in_floatable_fluid: fluid_height(mob, fluid_tags::ENTITY_FLOATABLE) > 0.0,
            in_water: me.in_water,
            width,
            height,
            max_up_step: max_up_step(mob),
            max_fall_distance: if mob.mob_type == super::MobType::Creeper {
                super::creeper::max_fall_distance(mob)
            } else {
                3
            },
            malus: mob.malus,
            context: me.mob_collision_context(mob),
        },
        speed: mob.speed,
        in_liquid: is_in_liquid(me, mob),
        follow_range: mob.attributes.value(attribute::FOLLOW_RANGE),
        game_time: view.game_time(),
        owner_uid: me.id,
    }
}

/// `Mob.isSunBurnTick`：官方 `nextFloat` 在 `magic > 0.5` 之后取，
/// 且 `MONSTERS_BURN` 为假时在取数之前就短路。
fn sun_burn_tick(me: &mut Entity, mob: &Mob, ctx: &mut dyn EntityCtx) -> bool {
    let view = ctx.blocks();
    // 官方 `Mob.isSunBurnTick` 先读 `MONSTERS_BURN`；为假时一个随机数都不取。
    if !view.monsters_burn() {
        return false;
    }
    let eye = BlockPos::containing(Vec3::new(me.position.x, eye_y(me, mob), me.position.z));
    let raw = view.max_local_raw_brightness(eye, view.sky_darken()) as f32 / 15.0;
    let magic = raw / (4.0 - 3.0 * raw);
    if magic <= 0.5 {
        return false;
    }
    let roll = me.random.next_float() * 30.0 < (magic - 0.4) * 2.0;
    let wet =
        me.is_in_water() || mob.in_powder_snow || mob.was_in_powder_snow || view.is_raining_at(eye);
    roll && !wet && view.can_see_sky(eye)
}

/// `LivingEntity.maxUpStep()`。
pub(crate) fn max_up_step(mob: &Mob) -> f32 {
    mob.attributes.value(attribute::STEP_HEIGHT) as f32
}

/// `AgeableWaterCreature.baseTick`：先记下氧气，`super.baseTick()` 之后再 `handleAirSupply`；
/// 别的生物只有 `super.baseTick()`。
///
/// `AgeableWaterCreature.baseTick` remembers the air first and handles it after
/// `super.baseTick()`; every other mob only runs the latter.
fn base_tick(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    let pre_tick_air = mob.air_supply;
    // 官方 `LivingEntity.baseTick` 开头的 `EnchantmentHelper.tickEffects`（TICK 组件）。
    // `EnchantmentHelper.tickEffects` at the head of vanilla `LivingEntity.baseTick`.
    super::enchantment_wiring::tick(me, mob, ctx);
    mob_base_tick(me, mob, ctx);
    if super::nautilus::is_nautilus(mob.mob_type) {
        // `Nautilus.handleAirSupply`：水里回满 300，离水每刻掉 1，到 -20 受 2 点干死伤害。
        // `Nautilus.handleAirSupply`: refill to 300 in water, decrement on land,
        // and take two dry-out damage at -20.
        super::water::handle_air_supply(me, mob, ctx, pre_tick_air);
    } else if super::squid::is_squid(mob.mob_type) || super::fish::is_fish(mob.mob_type) {
        // `AgeableWaterCreature` / `WaterAnimal.handleAirSupply`（鱿鱼和鱼）。
        // `AgeableWaterCreature` / `WaterAnimal.handleAirSupply` (squid and fish).
        super::water::handle_air_supply(me, mob, ctx, pre_tick_air);
    } else if mob.mob_type == super::MobType::Axolotl && !mob.no_ai {
        // `Axolotl.baseTick` 的 `handleAirSupply`（`!isNoAi()` 才跑）。
        // `Axolotl.baseTick`'s `handleAirSupply` (only when `!isNoAi()`).
        super::axolotl::handle_air_supply(me, mob, ctx, pre_tick_air);
    }
    // 海豚把 `handleAirSupply` 覆写成空实现，氧气完全由 `LivingEntity.baseTick` 算；
    // 美西螈 `isNoAi()` 时也只剩 `LivingEntity.baseTick` 的那一层。
    // A dolphin overrides `handleAirSupply` to a no-op, so air is entirely
    // `LivingEntity.baseTick`; a no-AI axolotl falls back to that layer too.
}

/// `Entity.spawnSprintParticle`：服务端虽然不发粒子，但仍消耗两次 `random.nextDouble()`。
///
/// `Entity.spawnSprintParticle`: the server does not emit particles, yet still draws two `random.nextDouble()`s.
fn spawn_sprint_particle(me: &mut Entity, view: &dyn BlockView) {
    use mc_bamboo_data::blocks;
    let state = view.block_state(me.on_pos(view, 0.2));
    if state.is_air()
        || matches!(
            state.block_id() as u32,
            blocks::BARRIER
                | blocks::BUBBLE_COLUMN
                | blocks::END_GATEWAY
                | blocks::END_PORTAL
                | blocks::LIGHT
                | blocks::WATER
                | blocks::LAVA
                | blocks::MOVING_PISTON
                | blocks::STRUCTURE_VOID
        )
    {
        return;
    }
    me.random.next_double();
    me.random.next_double();
}

/// `Mob.baseTick` → `LivingEntity.baseTick` → `Entity.baseTick`。
fn mob_base_tick(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    // `Entity.baseTick` 开头：载具被移除时当场 `stopRiding`，再递减下车冷却。
    // 实体乘客的 `vehicle` 在游戏/世界驱动里维护，这里只按官方递减冷却：
    // 载具已离表的摘除由驱动的 `rideTick` 递归负责。
    if me.boarding_cooldown > 0 {
        me.boarding_cooldown -= 1;
    }
    // 冲刺且不在水/岩浆里、不潜行、活着时，脚下方块可见就消耗两次 `nextDouble`（粒子位置）。
    // While sprinting out of water/lava, uncrouched and alive, a visible block below draws two `nextDouble`s.
    // `IronGolem.canSpawnSprintParticle()` 完全覆写缺省实现：不看疾跑 / 潜行 / 水，
    // 只看水平速度的平方和一次 `nextInt(5)`。
    if mob.mob_type == super::MobType::IronGolem {
        if me.delta.x * me.delta.x + me.delta.z * me.delta.z > 2.500_000_277_905_201e-7
            && me.random.next_int_bounded(5) == 0
        {
            spawn_sprint_particle(me, ctx.blocks());
        }
    } else if mob.sprinting
        && !me.in_water
        && !mob.ocelot.crouching
        && !mob.cat.crouching
        && !is_in_lava(me, mob)
        && is_alive(me, mob)
    {
        spawn_sprint_particle(me, ctx.blocks());
    }
    mob.was_in_powder_snow = mob.in_powder_snow;
    mob.in_powder_snow = false;
    mob.was_eye_in_water = eye_in_fluid(mob, fluid_tags::WATER);
    motion::update_fluid_interaction(me, mob, ctx.blocks());
    if me.remaining_fire_ticks > 0 {
        if me.remaining_fire_ticks % 20 == 0 && !is_in_lava(me, mob) {
            hurt_server(
                me,
                mob,
                ctx,
                DamageSource::environment(DamageKind::OnFire),
                1.0,
            );
        }
        me.remaining_fire_ticks -= 1;
    }
    if is_in_lava(me, mob) {
        me.fall_distance *= 0.5;
    }
    if me.position.y < f64::from(ctx.blocks().min_y() - 64) {
        hurt_server(
            me,
            mob,
            ctx,
            DamageSource::environment(DamageKind::OutOfWorld),
            4.0,
        );
    }
    me.first_tick = false;
    // `Entity.baseTick`：服务端每刻按剩余着火刻数重写共享标志位。
    // `Entity.baseTick`: the server rewrites the shared flag from the remaining fire ticks.
    mob.visible_fire = me.remaining_fire_ticks > 0;
    // `Entity.baseTick` 末尾的 `Leashable.tickLeash`（服务端且自己是 Leashable）。
    // The `Leashable.tickLeash` at the end of `Entity.baseTick`.
    super::leash::tick_leash(me, mob, ctx);
    // `LivingEntity.baseTick` 自己的部分。
    if is_alive(me, mob) {
        if is_in_wall(me, mob, ctx.blocks()) {
            hurt_server(
                me,
                mob,
                ctx,
                DamageSource::environment(DamageKind::InWall),
                1.0,
            );
        }
        let eye = BlockPos::containing(Vec3::new(me.position.x, eye_y(me, mob), me.position.z));
        if eye_in_fluid(mob, fluid_tags::WATER)
            && !ctx
                .blocks()
                .block_state(eye)
                .is(mc_bamboo_data::blocks::BUBBLE_COLUMN)
        {
            // 牛不能在水下呼吸，没有效果；`OXYGEN_BONUS` 是 0，不取随机数。溺尸在
            // `#can_breathe_under_water` 里，水生生物和水呼吸效果同理不消耗空气，
            // 反而按官方补气；海豚 / 美西螈的最大氧气和补气方式按类型分派。
            // A cow cannot breathe underwater and has no effect; `OXYGEN_BONUS` is zero so
            // there is no draw. The drowned is in `#can_breathe_under_water`, and aquatic
            // mobs and the water-breathing effect refill air as vanilla does; dolphins and
            // axolotls override the max and refill by kind.
            let can_breathe = mob.mob_type == super::MobType::Drowned
                || mob.mob_type.can_breathe_under_water()
                || mob
                    .effects
                    .has(mc_bamboo_data::registries::mob_effect::WATER_BREATHING);
            let max_air = mob.mob_type.max_air_supply();
            if can_breathe {
                if mob.air_supply < max_air {
                    mob.air_supply = mob.mob_type.increase_air_supply(mob.air_supply);
                }
            } else {
                mob.air_supply -= 1;
                if mob.air_supply <= -20 {
                    mob.air_supply = 0;
                    hurt_server(
                        me,
                        mob,
                        ctx,
                        DamageSource::environment(DamageKind::Drown),
                        2.0,
                    );
                }
            }
        } else if mob.air_supply < mob.mob_type.max_air_supply() {
            mob.air_supply = mob.mob_type.increase_air_supply(mob.air_supply);
        }
        // 官方：`blockPosition()` 与 `lastPos` 不等时更新并 `onChangedBlock`。
        // Vanilla: when `blockPosition()` differs from `lastPos`, update it and
        // call `onChangedBlock` → `runLocationChangedEffects`.
        let pos = me.block_position();
        if mob.last_pos != Some(pos) {
            mob.last_pos = Some(pos);
            super::enchantment_wiring::run_location(me, mob, ctx);
        }
    }
    if mob.hurt_time > 0 {
        mob.hurt_time -= 1;
    }
    if mob.damage_cooldown > 0 {
        mob.damage_cooldown -= 1;
    }
    if mob.is_dead_or_dying() {
        tick_death(me, mob, ctx);
    }
    if mob.last_hurt_by_player_memory_time > 0 {
        mob.last_hurt_by_player_memory_time -= 1;
    } else {
        mob.last_hurt_by_player = None;
        mob.last_hurt_by_player_ref = None;
    }
    if mob.last_hurt_by_mob.is_some() || mob.last_hurt_by_mob_ref.is_some() {
        // `getLastHurtByMob()` 按引用找实体：玩家不在实体表里，从玩家表找；读盘后只剩
        // UUID 的引用找不到实体，当作没了。
        // Players are not in the entity table; a reference left with only a UUID
        // after a load resolves to nothing.
        let alive = match mob.last_hurt_by_mob {
            Some(attacker) => match ctx.other(attacker) {
                Some(other) => match &other.kind {
                    KindPublic::Mob(public) => public.is_alive() && !other.removed,
                    _ => !other.removed,
                },
                None => {
                    let mut alive = false;
                    ctx.blocks().for_each_player(&mut |player| {
                        if player.uid == attacker {
                            alive = player.alive;
                        }
                    });
                    alive
                }
            },
            None => false,
        };
        if !alive || me.tick_count - mob.last_hurt_by_mob_timestamp > 100 {
            mob.last_hurt_by_mob = None;
            mob.last_hurt_by_mob_ref = None;
        }
    }
    mob.y_head_rot_o = mob.y_head_rot;
    mob.y_body_rot_o = mob.y_body_rot;
    tick_effects(me, mob, ctx);
    // `Mob.baseTick`：先取随机数，再拿自增前的值比较。
    if is_alive(me, mob) {
        let roll = me.random.next_int_bounded(1000);
        let time = mob.ambient_sound_time;
        mob.ambient_sound_time += 1;
        if roll < time {
            mob.ambient_sound_time = -mob.mob_type.ambient_sound_interval();
            if mob.mob_type == super::MobType::Fox {
                super::fox::play_ambient_sound(me, mob, ctx.blocks());
                return;
            }
            if mob.mob_type == super::MobType::Wolf {
                super::wolf::play_ambient_sound(me, mob, ctx.blocks().game_time());
                return;
            }
            if mob.mob_type == super::MobType::Cat {
                super::cat::play_ambient_sound(me, mob);
                return;
            }
            if mob.mob_type == super::MobType::Parrot {
                // `Parrot.getAmbientSound` 用世界随机源挑模仿音。
                // `Parrot.getAmbientSound` picks an imitation with the level random.
                super::parrot::ambient_sound(me, mob, ctx);
                return;
            }
            // 受惊的犰狳没有叫声：`makeSound` 不跑，也不取音高。
            // A scared armadillo has no ambient sound, so no pitch draws either.
            if super::armadillo::is_scared(mob) {
                return;
            }
            // `Bat.getAmbientSound`：倒挂时四次里三次不叫，不叫就不取音高。
            // `Bat.getAmbientSound`: a hanging bat stays quiet three times in four and
            // then draws no pitch.
            if mob.mob_type == super::MobType::Bat && super::bat::ambient_is_muted(me, mob) {
                return;
            }
            // `Entity.makeSound` 遇到空的叫声直接返回，也不取音高的两个随机数。
            // `Entity.makeSound` returns on a null sound without drawing the pitch.
            let Some(sound) = ambient_voice(me, mob) else {
                return;
            };
            let pitch = voice_pitch(me, mob);
            play_sound(me, mob, sound, mob.mob_type.sound_volume(), pitch);
        }
    }
}

/// `Mob.getAmbientSound`：苦力怕和河豚没有叫声；海龟只有成年、在水外、站在地上时才叫。
///
/// `Mob.getAmbientSound`: creepers and pufferfish have none, and a turtle only calls as
/// an adult standing on land out of the water.
fn ambient_voice(me: &Entity, mob: &Mob) -> Option<&'static str> {
    match mob.mob_type {
        kind if kind.has_no_ambient_sound() => None,
        // `Nautilus.getAmbientSound`：幼年 / 成年、水下 / 岸上四套；僵尸版两套。
        // `Nautilus.getAmbientSound`: four water/land and baby/adult variants;
        // the zombie version has two.
        kind if super::nautilus::is_nautilus(kind) => {
            super::nautilus::ambient_voice(kind, mob.is_baby(), me.in_water)
        }
        super::MobType::Turtle if me.in_water || !me.on_ground || mob.is_baby() => None,
        // `Dolphin.getAmbientSound`：水里 / 水外两种。
        // `Dolphin.getAmbientSound`: water / land variants.
        super::MobType::Dolphin => Some(super::dolphin::ambient_voice(me.in_water)),
        // `Axolotl.getAmbientSound` 加 `playAmbientSound` 的装死判断。
        // `Axolotl.getAmbientSound` plus `playAmbientSound`'s playing-dead gate.
        super::MobType::Axolotl => {
            super::axolotl::ambient_voice(mob.axolotl.playing_dead, me.in_water)
        }
        // `Villager.getAmbientSound`：睡觉时返回 null，不出声也不取音高。
        // `Villager.getAmbientSound`: returns null while sleeping, drawing no pitch.
        super::MobType::Villager if mob.villager.sleeping => None,
        // `Sniffer.getAmbientSound`：挖掘 / 搜寻时不叫。
        // `Sniffer.getAmbientSound`: silent while digging or searching.
        super::MobType::Sniffer => super::sniffer::ambient_voice(mob),
        _ => Some(voice_of(mob, Voice::Ambient, me.in_water)),
    }
}

/// 叫声、受伤和死亡的声音：马科按种类和幼年分派，其余按类型，牛用声音变体。
///
/// Ambient, hurt and death sounds: equines by kind and age, others by type,
/// cows by their sound variant.
fn voice_of(mob: &Mob, voice: Voice, in_water: bool) -> &'static str {
    // 硫磺方块怪按大小和是否吞了方块分派四种声音。
    // Sulfur cubes dispatch their four sounds by size and body item.
    if mob.mob_type == super::MobType::SulfurCube {
        return super::sulfur_cube::voice(mob, voice);
    }
    // 守卫者 / 远古守卫者：环境、受伤、死亡音都分水中 / 岸上两套。
    // Guardian / elder guardian: ambient, hurt and death sounds all have water / land
    // variants.
    if super::nautilus::is_nautilus(mob.mob_type) {
        let which = match voice {
            Voice::Ambient => super::nautilus::Voice::Ambient,
            Voice::Hurt => super::nautilus::Voice::Hurt,
            Voice::Death => super::nautilus::Voice::Death,
            Voice::Eat => super::nautilus::Voice::Eat,
            Voice::Angry => super::nautilus::Voice::Hurt,
        };
        return super::nautilus::voice(mob.mob_type, mob.is_baby(), in_water, which);
    }
    if super::guardian::is_guardian(mob.mob_type) {
        let elder = mob.mob_type == super::MobType::ElderGuardian;
        return match voice {
            Voice::Hurt => super::guardian::hurt_sound(elder, in_water),
            Voice::Death => super::guardian::death_sound(elder, in_water),
            _ => super::guardian::ambient_sound(elder, in_water),
        };
    }
    if mob.mob_type == super::MobType::Goat {
        return super::goat::voice(mob, voice);
    }
    if mob.mob_type.is_equine() {
        return super::equine::voice(mob, voice);
    }
    // 海龟的受伤和死亡音按幼年分两种，叫声只在陆地上有（见 `play_ambient_sound`）。
    // Turtle hurt and death sounds depend on age; the ambient sound is land-only.
    if super::turtle::is_turtle(mob.mob_type) {
        return match voice {
            Voice::Hurt if mob.is_baby() => "minecraft:entity.turtle.hurt_baby",
            Voice::Hurt => "minecraft:entity.turtle.hurt",
            Voice::Death if mob.is_baby() => "minecraft:entity.turtle.death_baby",
            Voice::Death => "minecraft:entity.turtle.death",
            Voice::Ambient => "minecraft:entity.turtle.ambient_land",
            Voice::Eat | Voice::Angry => "minecraft:entity.turtle.ambient_land",
        };
    }
    match voice {
        Voice::Hurt if super::armadillo::is_scared(mob) => {
            "minecraft:entity.armadillo.hurt_reduced"
        }
        Voice::Hurt => mob
            .mob_type
            .hurt_sound()
            .unwrap_or_else(|| match mob.mob_type {
                super::MobType::Wolf => super::wolf::sound(mob, super::wolf::Sound::Hurt),
                super::MobType::Cat => super::cat::sound(mob, super::cat::Sound::Hurt),
                super::MobType::Pig => mob.pig_sound_variant.hurt(mob.is_baby()),
                super::MobType::Chicken => super::chicken::sounds(mob)[1],
                super::MobType::Slime => super::slime::hurt_sound(mob),
                super::MobType::Drowned => super::drowned::hurt_sound(in_water),
                _ => mob.sound_variant.hurt(),
            }),
        Voice::Death => mob
            .mob_type
            .death_sound()
            .unwrap_or_else(|| match mob.mob_type {
                super::MobType::Wolf => super::wolf::sound(mob, super::wolf::Sound::Death),
                super::MobType::Cat => super::cat::sound(mob, super::cat::Sound::Death),
                super::MobType::Pig => mob.pig_sound_variant.death(mob.is_baby()),
                super::MobType::Chicken => super::chicken::sounds(mob)[2],
                super::MobType::Slime => super::slime::death_sound(mob),
                super::MobType::Drowned => super::drowned::death_sound(in_water),
                _ => mob.sound_variant.death(),
            }),
        _ => ambient_sound(mob, in_water),
    }
}

/// `onAttributeUpdated(MAX_HEALTH)` / `setHealth`：血量不超过最大血量。
///
/// `onAttributeUpdated(MAX_HEALTH)` / `setHealth`: keep health within the maximum.
pub(crate) fn clamp_health(mob: &mut Mob) {
    let max = mob.attributes.value(attribute::MAX_HEALTH) as f32;
    if mob.health > max {
        mob.health = max;
    }
}

/// `LivingEntity.makeSound`：生物自己的音量和 `getVoicePitch` 的两次取数。
///
/// `LivingEntity.makeSound`: the mob's own volume and the two `getVoicePitch` draws.
pub(crate) fn make_sound(me: &mut Entity, mob: &mut Mob, sound: &'static str) {
    let pitch = voice_pitch(me, mob);
    play_sound(me, mob, sound, mob.mob_type.sound_volume(), pitch);
}

/// `LivingEntity.tickEffects` → `MobEffectInstance.tickServer`。
/// 动作里真正要 `hurtServer` 的中毒 / 凋零走生物自己的受伤入口，其余直接改状态。
///
/// `LivingEntity.tickEffects` → `MobEffectInstance.tickServer`. Poison and
/// wither damage go through the mob's own hurt path; the rest change state here.
fn tick_effects(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    let state = super::effects::EffectState {
        health: mob.health,
        max_health: mob.attributes.value(attribute::MAX_HEALTH) as f32,
        absorption: mob.absorption,
    };
    let events = mob.effects.tick(me.tick_count, state);
    if events.attributes_changed {
        mob.effects.rebuild_attributes(&mut mob.attributes);
        let max = mob.attributes.value(attribute::MAX_HEALTH) as f32;
        if mob.health > max {
            mob.health = max;
        }
    }
    let mut hurts = Vec::new();
    for tick in events.actions {
        match tick.action {
            EffectAction::Heal(amount) | EffectAction::InstantHeal(amount) => {
                if mob.health < mob.attributes.value(attribute::MAX_HEALTH) as f32 {
                    let max = mob.attributes.value(attribute::MAX_HEALTH) as f32;
                    mob.health = (mob.health + amount).min(max);
                }
            }
            EffectAction::MagicDamage(amount) => {
                // `PoisonMobEffect`：血量高于 1 才真的打一次。
                if mob.health > 1.0 {
                    hurts.push((DamageKind::Magic, amount));
                }
            }
            EffectAction::WitherDamage(amount) => hurts.push((DamageKind::Wither, amount)),
            EffectAction::InstantDamage(amount) => hurts.push((DamageKind::Magic, amount)),
            EffectAction::Exhaustion(_) | EffectAction::Eat { .. } => {
                // 牛没有 `FoodData`，官方的 `HungerMobEffect` 和
                // `SaturationMobEffect` 也只在目标是 `Player` 时做事。
                // A cow has no `FoodData`; these effects only act on players.
            }
            EffectAction::Absorption(amount) => {
                mob.absorption = mob.absorption.max(amount);
            }
            EffectAction::AbsorptionEnded => {
                mob.effects.remove(tick.effect);
            }
        }
    }
    for (kind, amount) in hurts {
        hurt_server(me, mob, ctx, DamageSource::environment(kind), amount);
    }
}

/// `LivingEntity.canBeAffected`：按生物标签筛掉免疫的状态效果。
///
/// `LivingEntity.canBeAffected`: filters out effects the mob type is immune to.
pub fn can_be_affected(kind: super::MobType, effect: i32) -> bool {
    // `Parched.canBeAffected`：对虚弱免疫（自己射的箭不会反过来削弱它）。
    // `Parched.canBeAffected`: immune to weakness, so its own arrows cannot weaken it.
    if kind == super::MobType::Parched && effect == mob_effect::WEAKNESS {
        return false;
    }
    if kind.in_entity_tag("minecraft:immune_to_infested") {
        return effect != mob_effect::INFESTED;
    }
    if kind.in_entity_tag("minecraft:immune_to_oozing") {
        return effect != mob_effect::OOZING;
    }
    if kind.in_entity_tag("minecraft:ignores_poison_and_regen") {
        return effect != mob_effect::REGENERATION && effect != mob_effect::POISON;
    }
    true
}

/// `LivingEntity.addEffect`：先检查免疫标签，再给生物挂上效果并刷新属性。
///
/// `LivingEntity.addEffect`: checks `canBeAffected`, adds the effect and refreshes attributes.
pub fn add_effect(mob: &mut Mob, instance: super::effects::EffectInstance) -> bool {
    if !can_be_affected(mob.mob_type, instance.effect)
        || !super::spider::can_be_affected(mob, instance.effect)
    {
        return false;
    }
    let events = mob.effects.add(instance);
    if events.attributes_changed {
        mob.effects.rebuild_attributes(&mut mob.attributes);
    }
    !events.changes.is_empty()
}

/// `LivingEntity.removeAllEffects`。
pub fn clear_effects(mob: &mut Mob) -> bool {
    let events = mob.effects.remove_all();
    if events.attributes_changed {
        mob.effects.rebuild_attributes(&mut mob.attributes);
    }
    !events.changes.is_empty()
}

/// `LivingEntity.removeEffect`。
pub fn remove_effect(mob: &mut Mob, effect: i32) -> bool {
    let events = mob.effects.remove(effect);
    if events.attributes_changed {
        mob.effects.rebuild_attributes(&mut mob.attributes);
    }
    !events.changes.is_empty()
}

/// `Entity.playSound`：静音的生物不出声（音高这些参数调用方已经取过）。
///
/// `Entity.playSound`: a silent mob makes no sound (the caller already drew the pitch).
pub(crate) fn play_sound(me: &Entity, mob: &mut Mob, sound: &'static str, volume: f32, pitch: f32) {
    if mob.silent {
        return;
    }
    mob.events.push(MobEvent::Sound {
        sound,
        volume,
        pitch,
        position: me.position,
        hostile: mob.mob_type.is_enemy(),
        seed: None,
    });
}

/// `Mob.playAmbientSound`：`makeSound(getAmbientSound())`。
pub(crate) fn play_ambient_sound(me: &mut Entity, mob: &mut Mob) {
    // `Armadillo.getAmbientSound` 受惊时是 `null`：`makeSound` 不跑，也不取音高。
    // A scared armadillo has no ambient sound, so `makeSound` and its pitch draws are skipped.
    if super::armadillo::is_scared(mob) {
        return;
    }
    if mob.mob_type == super::MobType::Wolf {
        // 刚生成的狼还没有仇恨，时刻不影响叫声。
        // A fresh wolf has no anger, so the clock does not matter.
        super::wolf::play_ambient_sound(me, mob, 0);
        return;
    }
    if mob.mob_type == super::MobType::Cat {
        super::cat::play_ambient_sound(me, mob);
        return;
    }
    if mob.mob_type.has_no_ambient_sound() {
        return;
    }
    if mob.mob_type == super::MobType::Bat && super::bat::ambient_is_muted(me, mob) {
        return;
    }
    if super::turtle::is_turtle(mob.mob_type) {
        // `Turtle.getAmbientSound`：只有成年的、在水外站在地上时才叫，其余没有声音，
        // 也不取音高的两个随机数。
        // `Turtle.getAmbientSound`: only an adult on land out of water makes a sound;
        // otherwise no sound and no pitch draws.
        if !me.in_water && me.on_ground && !mob.is_baby() {
            let pitch = voice_pitch(me, mob);
            play_sound(
                me,
                mob,
                "minecraft:entity.turtle.ambient_land",
                mob.mob_type.sound_volume(),
                pitch,
            );
        }
        return;
    }
    if mob.mob_type.is_equine() {
        // 马科的叫声和音量（0.8）按自己的类。
        // Equines use their own ambient sound and volume (0.8).
        let sound = voice_of(mob, Voice::Ambient, me.in_water);
        make_sound(me, mob, sound);
        return;
    }
    // 史莱姆和硫磺方块怪的 `getAmbientSound()` 是空：`makeSound(null)` 不放音效，
    // 也不取音高。
    // Slimes and sulfur cubes have a null `getAmbientSound()`: `makeSound(null)`
    // plays nothing and draws no pitch.
    if matches!(
        mob.mob_type,
        super::MobType::Slime | super::MobType::SulfurCube
    ) {
        return;
    }
    let pitch = voice_pitch(me, mob);
    play_sound(
        me,
        mob,
        ambient_sound(mob, me.in_water),
        sound_volume(mob),
        pitch,
    );
}

/// `getSoundVolume()`：史莱姆按大小算，其余按类型。
///
/// `getSoundVolume()`: by size for slimes, by type otherwise.
fn sound_volume(mob: &Mob) -> f32 {
    if matches!(
        mob.mob_type,
        super::MobType::Slime | super::MobType::SulfurCube
    ) {
        super::slime::sound_volume(mob)
    } else {
        mob.mob_type.sound_volume()
    }
}

fn ambient_sound(mob: &Mob, in_water: bool) -> &'static str {
    // 守卫者的环境音按水 / 岸和种类分派（表里返回 `None`）。
    // Guardian ambience is dispatched by water / land and kind (the table returns
    // `None`).
    if super::guardian::is_guardian(mob.mob_type) {
        return super::guardian::ambient_sound(
            mob.mob_type == super::MobType::ElderGuardian,
            in_water,
        );
    }
    match mob.mob_type {
        super::MobType::Goat => super::goat::voice(mob, Voice::Ambient),
        super::MobType::Enderman if mob.enderman.creepy => "minecraft:entity.enderman.scream",
        super::MobType::Pig => mob.pig_sound_variant.ambient(mob.is_baby()),
        super::MobType::Chicken => super::chicken::sounds(mob)[0],
        super::MobType::Drowned => super::drowned::ambient_sound(in_water),
        super::MobType::PolarBear => super::polar_bear::ambient_sound(mob.is_baby()),
        super::MobType::Panda => super::panda::ambient_sound(&mob.panda),
        _ => mob
            .mob_type
            .ambient_sound()
            .unwrap_or(mob.sound_variant.ambient()),
    }
}

/// 生物叫声的音高取两次随机数；幼年基础音高为 1.5。
///
/// The voice pitch takes two draws and uses a higher base pitch for babies.
pub(super) fn voice_pitch(me: &mut Entity, mob: &Mob) -> f32 {
    let first = me.random.next_float();
    let second = me.random.next_float();
    let base = if mob.is_baby() { 1.5 } else { 1.0 };
    let pitch = if mob.mob_type == super::MobType::ZombieVillager {
        // `ZombieVillager.getVoicePitch()`：幼年 2.0、成年 1.0。
        // `ZombieVillager.getVoicePitch()`: 2.0 for babies, 1.0 for adults.
        (first - second) * 0.2 + if mob.is_baby() { 2.0 } else { 1.0 }
    } else {
        (first - second) * 0.2 + base
    };
    if mob.mob_type == super::MobType::Bat {
        // `Bat.getVoicePitch()`：`super.getVoicePitch() * 0.95F`。
        // `Bat.getVoicePitch()`: `super.getVoicePitch() * 0.95F`.
        pitch * 0.95
    } else {
        pitch
    }
}

/// `Entity.isInWall`：眼睛那一小片和会让人窒息的方块重叠。
fn is_in_wall(me: &Entity, mob: &Mob, view: &dyn BlockView) -> bool {
    // `LivingEntity.isInWall`：睡着的不窒息（狐狸与村民）。
    // `LivingEntity.isInWall`: sleepers never suffocate (foxes and villagers).
    if (mob.mob_type == super::MobType::Fox && mob.fox.is_sleeping())
        || (mob.mob_type == super::MobType::Villager && mob.villager.sleeping)
    {
        return false;
    }
    let width = f64::from(me.dimensions.0 * 0.8);
    let eye = eye_y(me, mob);
    let bounds = mc_bamboo_core::Aabb::new(
        me.position.x - width / 2.0,
        eye - 1.0e-6 / 2.0,
        me.position.z - width / 2.0,
        me.position.x + width / 2.0,
        eye + 1.0e-6 / 2.0,
        me.position.z + width / 2.0,
    );
    let min = BlockPos::containing(Vec3::new(bounds.min_x, bounds.min_y, bounds.min_z));
    let max = BlockPos::containing(Vec3::new(bounds.max_x, bounds.max_y, bounds.max_z));
    for x in min.x..=max.x {
        for y in min.y..=max.y {
            for z in min.z..=max.z {
                let pos = BlockPos::new(x, y, z);
                let state = view.block_state(pos);
                if state.is_air() || !state.is_suffocating() {
                    continue;
                }
                let boxes = shape::collision_boxes(state, pos, CollisionContext::Empty);
                if boxes.iter().any(|part| part.at(pos).intersects(bounds)) {
                    return true;
                }
            }
        }
    }
    false
}

/// `tickDeath`：第 20 刻移除。
fn tick_death(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    if mob.mob_type == super::MobType::Creaking && super::creaking::tear_down_death(me, mob) {
        return;
    }
    mob.death_time += 1;
    if mob.death_time >= 20 && !me.removed {
        mob.events.push(MobEvent::Poof);
        // `AbstractCubeMob.remove`：先分裂，再 `super.remove`。
        // `AbstractCubeMob.remove`: split first, then `super.remove`.
        if matches!(
            mob.mob_type,
            super::MobType::Slime | super::MobType::SulfurCube
        ) {
            super::slime::split_on_remove(me, mob, ctx);
        }
        me.discard();
    }
}

// ---- aiStep ----

/// `Animal.aiStep` → `AgeableMob.aiStep` → `Mob.aiStep` → `LivingEntity.aiStep`。
fn ai_step(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx, plan: ThrottlePlan) {
    mob.needs_sync = std::mem::take(&mut mob.knockback_sync);
    // `Witch.aiStep` 的喝药段在 `super.aiStep()`（`Monster.aiStep` 的亮度）之前。
    //
    // The potion part of `Witch.aiStep` runs before `super.aiStep()`, which is the
    // `Monster.aiStep` light handling.
    if mob.mob_type == super::MobType::Witch {
        super::witch::ai_step(me, mob, ctx);
    }
    let fox = mob.mob_type == super::MobType::Fox;
    if fox {
        super::fox::ai_step_before(me, mob, ctx);
    }
    let equine = mob.mob_type.is_equine();
    if equine {
        super::equine::ai_step_start(me, mob);
    }
    if mob.mob_type == super::MobType::Enderman {
        super::enderman::ai_step(me, mob, ctx);
    }
    // `Guardian.aiStep` 的服务端部分在 `super.aiStep()`（Monster）之前。
    // The server part of `Guardian.aiStep` runs before `super.aiStep()` (Monster).
    if super::guardian::is_guardian(mob.mob_type) {
        super::guardian::ai_step(me, mob);
    }
    // `Creaking.aiStep` 在 `super.aiStep()` 之前：先 `checkCanMove`（读上一刻的大脑记忆），
    // 再走 `Monster.aiStep`（`customServerAiStep` 跑大脑、传感器）。
    if mob.mob_type == super::MobType::Creaking {
        super::creaking::ai_step(me, mob, ctx);
    }
    animal_ai_step(me, mob, ctx, plan);
    if mob.mob_type == super::MobType::Bee {
        let target_close = super::goals::combat_target(mob, ctx)
            .is_some_and(|target| target.position.distance_to_sqr(me.position) < 4.0);
        let view = ctx.blocks();
        let hive_valid = super::bee_goals::hive_valid(mob, view, me);
        super::bee::ai_step(
            mob,
            me.tick_count,
            view.game_time(),
            hive_valid,
            target_close,
        );
    }
    // 铁傀儡与雪傀儡的 `aiStep` 先跑 `super.aiStep()`，再跑自己的那一段。
    // Golem `aiStep` overrides run their own part after `super.aiStep()`.
    if mob.mob_type == super::MobType::IronGolem {
        super::iron_golem::ai_step(me, mob, ctx);
    }
    if mob.mob_type == super::MobType::SnowGolem {
        super::snow_golem::ai_step(me, mob, ctx);
    }
    // `Endermite.aiStep` 在 `super.aiStep()` 之后推进寿命。
    // `Endermite.aiStep` advances its lifetime after `super.aiStep()`.
    if mob.mob_type == super::MobType::Endermite {
        super::endermite::ai_step(me, mob);
    }
    if equine {
        super::equine::ai_step_end(me, mob, ctx);
    }
}

/// `Animal.aiStep` 一路到 `LivingEntity.aiStep` 的公共部分。
///
/// The shared chain from `Animal.aiStep` down to `LivingEntity.aiStep`.
fn animal_ai_step(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx, plan: ThrottlePlan) {
    // `AbstractFish.aiStep` 在 `LivingEntity.aiStep` 之前：只有这一支的鱼离水才扑腾。
    // `AbstractFish.aiStep` runs before `LivingEntity.aiStep`: only fish flop out of water.
    if super::fish::is_fish(mob.mob_type) {
        super::fish::ai_step(me, mob);
    }
    let fox = mob.mob_type == super::MobType::Fox;
    // `Monster.aiStep` 先于 `LivingEntity.aiStep`：亮处每刻多涨 2 点 noActionTime。
    // `Entity.getLightLevelDependentMagicValue` 取眼睛所在格，
    // `LevelReader.getMaxLocalRawBrightness(pos)` 带上 `getSkyDarken()`。
    if mob.mob_type.is_monster() {
        let view = ctx.blocks();
        let eye = BlockPos::containing(Vec3::new(me.position.x, eye_y(me, mob), me.position.z));
        let raw = view.max_local_raw_brightness(eye, view.sky_darken()) as f32 / 15.0;
        let magic = raw / (4.0 - 3.0 * raw);
        // `Raider.updateNoActionTime` 覆写成无条件加 2，不看亮度。
        // `Raider.updateNoActionTime` overrides it to an unconditional +2.
        if magic > 0.5 || mob.mob_type.is_raider() {
            mob.no_action_time += 2;
        }
    }
    if mob.no_jump_delay > 0 {
        mob.no_jump_delay -= 1;
    }
    let delta = me.delta;
    let mut x = delta.x;
    let mut y = delta.y;
    let mut z = delta.z;
    if delta.x.abs() < 0.003 {
        x = 0.0;
    }
    if delta.z.abs() < 0.003 {
        z = 0.0;
    }
    if delta.y.abs() < 0.003 {
        y = 0.0;
    }
    me.delta = Vec3::new(x, y, z);
    // `applyInput`。
    mob.xxa *= 0.98;
    mob.zza *= 0.98;
    // `isImmobile()`：缺省是濒死，马科另有覆写。
    // `isImmobile()`: dying by default; equines override it.
    if super::equine::is_immobile(mob) {
        mob.jumping = false;
        mob.xxa = 0.0;
        mob.zza = 0.0;
    } else if !mob.no_ai && mob.controlling_passenger().is_none() {
        // 官方 `Mob.isEffectiveAi()` 在服务端等于「没有玩家在控制」：有人骑着上鞍的载具时
        // AI 停摆，位置完全由客户端决定。
        //
        // On the server vanilla `Mob.isEffectiveAi()` means "no player is in
        // control": AI stops while a player rides a saddled vehicle, and the
        // client alone decides where it goes.
        server_ai_step(me, mob, ctx, plan);
    }
    // 休眠档每 20 刻的完整 tick：物理照跑，但这一刻不主动走动。
    if plan.no_active_move {
        mob.jumping = false;
        mob.xxa = 0.0;
        mob.zza = 0.0;
        mob.mover.operation = super::control::Operation::Wait;
        mob.navigation.stop();
    }
    // 跳。
    if mob.jumping {
        let fluid = if is_in_lava(me, mob) {
            fluid_height(mob, fluid_tags::LAVA)
        } else {
            fluid_height(mob, fluid_tags::WATER)
        };
        let in_water = me.in_water && fluid > 0.0;
        let threshold = fluid_jump_threshold(mob);
        // 岩浆那一支是 `!isInShallowFluid(LAVA)`，NaN 时为真，不能写成 `>`。
        // The lava branch is `!isInShallowFluid(LAVA)`, true on NaN, so it is not `>`.
        let deep_lava = !matches!(
            fluid_height(mob, fluid_tags::LAVA).partial_cmp(&fluid_jump_threshold(mob)),
            Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
        );
        if (in_water && (!me.on_ground || fluid > threshold))
            || (is_in_lava(me, mob) && (!me.on_ground || deep_lava))
        {
            jump_in_liquid(me, mob);
        } else if (me.on_ground || (in_water && fluid <= threshold)) && mob.no_jump_delay == 0 {
            if matches!(
                mob.mob_type,
                super::MobType::Slime | super::MobType::SulfurCube
            ) {
                super::slime::jump_from_ground(me, mob, ctx.blocks());
            } else {
                jump_from_ground(me, mob, ctx.blocks());
            }
            mob.no_jump_delay = 10;
        }
    } else {
        mob.no_jump_delay = 0;
    }
    // 走。
    let input = Vec3::new(f64::from(mob.xxa), f64::from(mob.yya), f64::from(mob.zza));
    // `LivingEntity.aiStep`：缓降 / 漂浮每刻在 travel **之前**清 `fallDistance`，
    // 这样 `Entity.move` 这一刻累计的位移仍会留到下一刻。
    if mob.effects.has(mob_effect::SLOW_FALLING) || mob.effects.has(mob_effect::LEVITATION) {
        me.fall_distance = 0.0;
    }
    if !mob.no_ai {
        timing::span!(Travel);
        // 官方 `LivingEntity.travel`：有人控制时走 `travelRidden`，服务端不模拟载具的运动，
        // 只跑 `tickRidden` 再把速度清零；位置由客户端的 `move_vehicle` 决定。
        //
        // Vanilla `LivingEntity.travel`: with a controlling passenger it goes through
        // `travelRidden`; the server does not simulate the vehicle, it only runs
        // `tickRidden` and zeroes the velocity. The position comes from the client's
        // `move_vehicle` packet.
        if let Some(rider) = mob.controlling_passenger() {
            if let Some(view) = ctx.player(rider) {
                if mob.mob_type.is_equine() {
                    super::equine::tick_ridden(me, mob, view.yaw, view.pitch);
                } else if mob.mob_type == super::MobType::Pig {
                    super::pig::tick_ridden(me, mob, view.yaw, view.pitch);
                } else if super::nautilus::is_nautilus(mob.mob_type) {
                    super::nautilus_ai::ridden_rotation(me, mob, view.yaw, view.pitch);
                }
            }
            me.delta = Vec3::default();
        } else if mob.mob_type == super::MobType::Phantom {
            super::phantom::travel(me, mob, ctx, input);
        } else if super::squid::is_squid(mob.mob_type) {
            // `Squid.travel`：只有 `move(SELF, deltaMovement)`。
            // `Squid.travel` is a bare `move(SELF, deltaMovement)`.
            let movement = me.delta;
            motion::move_living(me, mob, ctx, movement);
        } else {
            travel(me, mob, ctx, input);
        }
    }
    {
        timing::span!(BlockEffects);
        motion::apply_effects_from_blocks(me, mob, ctx);
    }
    // 冻结。
    let can_freeze = can_freeze(mob);
    if !mob.in_powder_snow || !can_freeze {
        mob.ticks_frozen = (mob.ticks_frozen - 2).max(0);
    }
    if let Some(speed) = mob.attributes.instance(attribute::MOVEMENT_SPEED) {
        speed.remove_modifier(POWDER_SNOW_SLOW);
    }
    let legacy = me.on_pos(ctx.blocks(), 0.2);
    if !ctx.blocks().block_state(legacy).is_air() && mob.ticks_frozen > 0 {
        let percent = mob.ticks_frozen.min(140) as f32 / 140.0;
        let amount = -0.05_f32 * percent;
        if let Some(speed) = mob.attributes.instance(attribute::MOVEMENT_SPEED) {
            speed.modifiers.push(super::attributes::Modifier {
                id: POWDER_SNOW_SLOW.to_owned(),
                amount: f64::from(amount),
                operation: super::attributes::Operation::AddValue,
                permanent: false,
            });
        }
    }
    if me.tick_count % 40 == 0 && mob.ticks_frozen >= 140 && can_freeze {
        hurt_server(
            me,
            mob,
            ctx,
            DamageSource::environment(DamageKind::Freeze),
            1.0,
        );
    }
    {
        timing::span!(Push);
        push_entities(me, mob, ctx);
    }
    // `LivingEntity.aiStep` 尾部：`isSensitiveToWater && isInWaterOrRain` 每刻 1 点溺水伤害。
    // Tail of `LivingEntity.aiStep`: `isSensitiveToWater && isInWaterOrRain` deals
    // one drown damage per tick.
    if mob.mob_type == super::MobType::SnowGolem {
        let blocks = ctx.blocks();
        let feet = me.block_position();
        let eye_top = BlockPos::containing(Vec3::new(
            me.position.x,
            me.bounding_box().max_y,
            me.position.z,
        ));
        if me.in_water || blocks.is_raining_at(feet) || blocks.is_raining_at(eye_top) {
            hurt_server(
                me,
                mob,
                ctx,
                DamageSource::environment(DamageKind::Drown),
                1.0,
            );
        }
    }
    // `LivingEntity.aiStep` 尾部：`isSensitiveToWater && isInWaterOrRain` 每刻 1 点溺水伤害。
    // Tail of `LivingEntity.aiStep`: `isSensitiveToWater && isInWaterOrRain` deals
    // one drown damage per tick.
    if mob.mob_type == super::MobType::SnowGolem {
        let blocks = ctx.blocks();
        let feet = me.block_position();
        let eye_top = BlockPos::containing(Vec3::new(
            me.position.x,
            me.bounding_box().max_y,
            me.position.z,
        ));
        if me.in_water || blocks.is_raining_at(feet) || blocks.is_raining_at(eye_top) {
            hurt_server(
                me,
                mob,
                ctx,
                DamageSource::environment(DamageKind::Drown),
                1.0,
            );
        }
    }
    if mob.mob_type == super::MobType::Enderman {
        super::enderman::hurt_if_wet(me, mob, ctx);
    }
    // `Mob.aiStep` 在 `LivingEntity.aiStep` 之后跑 `BURN_IN_DAYLIGHT` → `burnUndead()`。
    // 官方用 `EntityTypeTags.BURN_IN_DAYLIGHT` + `isSunSensitive()`；尸壳不烧。
    if mob.mob_type.burns_in_daylight() && is_alive(me, mob) && sun_burn_tick(me, mob, ctx) {
        burn_undead(me, mob, ctx);
    }
    // `Mob.aiStep` 的 looting：熊猫空着手、脚下有能吃的东西时拿进主手。
    // `Mob.aiStep` looting: an empty-handed panda takes edible items at its feet.
    if mob.mob_type == super::MobType::Panda
        && mob.can_pick_up_loot
        && is_alive(me, mob)
        && !mob.dead
        && ctx.blocks().block_game_rules().mob_griefing
    {
        let mut found = None;
        ctx.for_each_other_in(me.bounding_box().inflate(1.0, 0.0, 1.0), &mut |other| {
            if found.is_some() || other.removed {
                return;
            }
            if let KindPublic::Item {
                stack,
                pickup_delay,
                ..
            } = &other.kind
                && !stack.is_empty()
                && super::panda::wants_pick_up(mob, stack, *pickup_delay)
            {
                found = Some(other.uid);
            }
        });
        if let Some(item) = found {
            ctx.emit(crate::entity::ctx::Command::PickUpItem { item }, me);
        }
    }
    // `Mob.aiStep` 的捡东西那一段：狐狸也一样。
    // The same `Mob.aiStep` pickup loop, for foxes.
    if fox {
        super::fox::pick_up_items(me, mob, ctx);
    }
    // `Mob.aiStep` 的捡东西那一段：村民（`InventoryCarrier.pickUpItem`）。
    // The same `Mob.aiStep` pickup loop, for villagers (`InventoryCarrier.pickUpItem`).
    if mob.mob_type == super::MobType::Villager
        && mob.can_pick_up_loot
        && is_alive(me, mob)
        && !mob.dead
        && ctx.blocks().block_game_rules().mob_griefing
    {
        let mut items = Vec::new();
        ctx.for_each_other_in(me.bounding_box().inflate(1.0, 0.0, 1.0), &mut |other| {
            if other.removed {
                return;
            }
            if let KindPublic::Item {
                stack,
                pickup_delay,
                ..
            } = &other.kind
                && !stack.is_empty()
                && *pickup_delay <= 0
                && mob.villager.wants_to_pick_up(stack)
            {
                items.push((other.uid, stack.clone()));
            }
        });
        for (item, stack) in items {
            if mob.villager.wants_to_pick_up(&stack) {
                let leftover = mob.villager.inventory.add_item(stack);
                let remaining_count = if leftover.is_empty() {
                    0
                } else {
                    leftover.count
                };
                ctx.emit(
                    crate::entity::ctx::Command::VillagerPickUpItem {
                        item,
                        remaining_count,
                    },
                    me,
                );
            }
        }
    }
    // `AgeableMob.aiStep`：僵尸和鱼都不在这一层，年龄/求爱都不动。
    if mob.mob_type.is_ageable() && mob.forced_age_timer > 0 {
        if mob.forced_age_timer % 4 == 0 {
            for _ in 0..3 {
                me.random.next_double();
            }
        }
        mob.forced_age_timer -= 1;
    }
    if mob.mob_type.is_ageable() && is_alive(me, mob) {
        let was_baby = mob.is_baby();
        if mob.is_baby() && !mob.age_locked {
            set_age(me, mob, mob.age + 1);
        } else if mob.age > 0 {
            set_age(me, mob, mob.age - 1);
        }
        // `Turtle.ageBoundaryReached`：长大成人时按 `turtle_grow` 表掉鳞甲。
        // `Turtle.ageBoundaryReached`: a turtle that grows up drops from `turtle_grow`.
        if was_baby
            && !mob.is_baby()
            && super::turtle::is_turtle(mob.mob_type)
            && ctx.blocks().block_game_rules().mob_drops
        {
            let request = super::EntityLootRequest::new(super::turtle::grow_loot(), me, mob, None);
            ctx.emit(crate::entity::ctx::Command::DropEntityLoot(request), me);
        }
        // `Villager.ageBoundaryReached`：幼年长大时重建大脑并切换到成年日程。
        // `Villager.ageBoundaryReached`: rebuilds the brain and switches to the adult schedule.
        if was_baby != mob.is_baby() && mob.mob_type == super::MobType::Villager {
            super::villager_ai::refresh_brain(me, mob, ctx);
        }
    }
    if super::turtle::is_turtle(mob.mob_type) {
        super::turtle::ai_step(me, mob, ctx);
    }
    if mob.mob_type == super::MobType::Tadpole {
        super::tadpole::ai_step(me, mob, ctx);
    }
    if mob.age_lock_particle_timer > 0 {
        if mob.age_lock_particle_timer % 2 == 0 {
            for _ in 0..3 {
                me.random.next_double();
            }
        }
        mob.age_lock_particle_timer -= 1;
    }
    // `Animal.aiStep`：心形粒子在服务端不显示，但参数照算。
    if mob.age != 0 {
        mob.in_love = 0;
    }
    if mob.in_love > 0 {
        mob.in_love -= 1;
        if mob.in_love % 10 == 0 {
            for _ in 0..3 {
                me.random.next_gaussian();
            }
            for _ in 0..3 {
                me.random.next_double();
            }
        }
    }
    if mob.mob_type == super::MobType::Rabbit {
        super::rabbit::ai_step(mob);
    }
    if fox {
        super::fox::ai_step_after(me, mob);
    }
    if mob.mob_type == super::MobType::Wolf {
        super::wolf::ai_step_after(me, mob, ctx);
    }
    if mob.mob_type == super::MobType::Chicken {
        super::chicken::ai_step(me, mob, ctx);
    }
    if super::squid::is_squid(mob.mob_type) {
        super::squid::ai_step(me, mob);
    }
    // `Pufferfish.aiStep` 的尾部：膨胀时蜇附近的生物。
    // The tail of `Pufferfish.aiStep`: sting nearby mobs while puffed up.
    super::fish::puffer_touch(me, mob, ctx);
}

/// `LivingEntity.canFreeze()`：盔甲槽里有 `#freeze_immune_wearables` 就不会冻，否则看类型
/// （[`super::MobType::can_freeze`]）。生物没有旁观模式。
///
/// `LivingEntity.canFreeze()`: any `#freeze_immune_wearables` armour piece prevents freezing;
/// otherwise the type decides ([`super::MobType::can_freeze`]). Mobs are never spectators.
pub(crate) fn can_freeze(mob: &Mob) -> bool {
    use mc_bamboo_data::tags::item_tags::FREEZE_IMMUNE_WEARABLES;
    use mc_bamboo_protocol::packet::play::EquipmentSlot::{Body, Chest, Feet, Head, Legs};
    // `EquipmentSlotGroup.ARMOR`：四件人形盔甲加身体槽。
    // `EquipmentSlotGroup.ARMOR`: the four humanoid armour slots plus the body slot.
    let immune = [Feet, Legs, Chest, Head, Body].into_iter().any(|slot| {
        let stack = mob.equipment.get(slot);
        !stack.is_empty() && crate::state::item_is_tagged(stack.item, FREEZE_IMMUNE_WEARABLES)
    });
    !immune && mob.mob_type.can_freeze()
}

/// `LivingEntity.SPEED_MODIFIER_POWDER_SNOW_ID`。
const POWDER_SNOW_SLOW: &str = "minecraft:powder_snow";

/// `AgeableMob.setAge`：跨过 0 时换尺寸。
pub(crate) fn set_age(me: &mut Entity, mob: &mut Mob, age: i32) {
    let old = mob.age;
    mob.age = age;
    if (old < 0) != (age < 0) {
        let (width, height, eye) = mob.mob_type.dimensions(age < 0);
        me.dimensions = (width, height);
        mob.eye_height = eye;
        if mob.mob_type == super::MobType::Goat {
            // 山羊的尺寸还看姿势；`ageBoundaryReached` 换攻击力。
            // Goat size also depends on the pose; `ageBoundaryReached` swaps the attack damage.
            let (width, height, eye) = super::goat::pose_dimensions(age < 0, mob.goat.long_jumping);
            me.dimensions = (width, height);
            mob.eye_height = eye;
            super::goat::age_boundary_reached(mob);
        }
    }
}

/// `Animal.spawnChildFromBreeding` 的父亲一侧：年龄设成 6000、清掉求爱、发爱心，
/// 伴侣、幼崽和经验球交给命令落地。
///
/// The father's side of `Animal.spawnChildFromBreeding`: age 6000, love reset, hearts;
/// the partner, offspring and orb are applied by a command.
pub(crate) fn breed_with(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    partner: crate::entity::EntityUid,
) {
    mob.in_love = 0;
    set_age(me, mob, 6000);
    mob.events.push(MobEvent::Hearts);
    ctx.emit(
        Command::BreedAnimals {
            variant: mob.variant,
            lover: mob.love_cause,
            // 哞菇借这个字节传自己的品种（落地时生物状态不在实体上）。
            // Mooshrooms pass their type in this byte: the mob state is detached
            // from the entity when the command lands.
            wool: if mob.mob_type == super::MobType::Mooshroom {
                mob.mooshroom.brown as u8
            } else {
                mob.wool
            },
            partner,
            position: me.position,
            // 熊猫父亲的基因（落地时父亲不在实体表里）。
            // The panda father's genes (it is not in the table when this lands).
            father_genes: (mob.mob_type == super::MobType::Panda).then(|| {
                (
                    mob.panda.main_gene.id() as u8,
                    mob.panda.hidden_gene.id() as u8,
                )
            }),
            kind: mob.mob_type,
            father_rabbit: mob.rabbit.variant,
            father_fox: mob.fox.variant,
            father_love: mob.love_cause,
            father_wolf: Box::new((mob.wolf, mob.tame)),
            father_cat: mob.cat,
            father_screaming: mob.goat.screaming,
            father_axolotl: mob.axolotl.variant,
        },
        me,
    );
}

/// `Mob.serverAiStep`。
fn server_ai_step(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx, plan: ThrottlePlan) {
    mob.no_action_time += 1;
    mob.seen.clear();
    if plan.decisions_run {
        timing::span!(Goals);
        // 官方 `Mob.serverAiStep`：两个选择器共用同一个隔刻判断，先 target 后 goal。
        // `tickCount` 是实体自己的 tick 数；`id` 用网络实体 ID。
        let mut goals = std::mem::take(&mut mob.goals);
        if mob.mob_type.is_skeleton() {
            goals.reassess_skeleton_weapon(me, mob, ctx);
        }
        let mut targets = std::mem::take(&mut mob.target_goals);
        let phase = me.tick_count.wrapping_add(me.id);
        let fox = mob.mob_type == super::MobType::Fox;
        if phase % 2 != 0 && me.tick_count > 1 {
            {
                timing::span!(Targeting);
                targets.tick_running(false, me, mob, ctx);
                if fox {
                    super::fox::after_target_change(mob);
                }
            }
            goals.tick_running(false, me, mob, ctx);
        } else {
            {
                timing::span!(Targeting);
                targets.tick(me, mob, ctx);
                // `Fox.setTarget(null)` 当场结束保护，后面的目标就看得到。
                // `Fox.setTarget(null)` ends the defence at once for the goals after it.
                if fox {
                    super::fox::after_target_change(mob);
                }
            }
            goals.tick(me, mob, ctx);
        }
        if fox {
            super::fox::after_target_change(mob);
        }
        mob.target_goals = targets;
        mob.goals = goals;
        // `BreedGoal.breed` 就在这一刻发生：父亲的年龄先设成 6000，
        // 后面 `aiStep` 会照官方再减一；伴侣和幼崽交给命令落地。
        if let Some(partner) = mob.pending_breed.take() {
            breed_with(me, mob, ctx, partner);
        }
    }
    // `Mob.serverAiStep` 用字段 `this.navigation.tick()`，不是 getter。借载具导航时
    // 共享导航归载具自己的 serverAiStep tick，这里只 tick 骑手自己那份（通常闲置），
    // 换回载具导航后再让大脑与控制段继续看载具导航。
    //
    // Vanilla `Mob.serverAiStep` ticks the `this.navigation` field, not the getter.
    // The shared navigation is ticked by the vehicle's own step; here the rider
    // only ticks its own idle navigation, then restores the vehicle's for brains.
    timing::span!(Navigation);
    let own_navigation = mob.swap_to_own_navigation();
    let nav = if own_navigation {
        nav_mob_own(me, mob, ctx.blocks())
    } else {
        nav_mob(me, mob, ctx.blocks())
    };
    // `Bee$1.tick`：授粉期间跳过飞行导航 tick。
    // `Bee$1.tick`: skip flying navigation tick while pollinating.
    let skip_nav = mob.mob_type == super::MobType::Bee && mob.bee.pollinating;
    if !skip_nav && let Some((wanted, speed)) = mob.navigation.tick(ctx.blocks(), &nav) {
        // 官方 `this.mob.getMoveControl()`：借导航时 getter 转到载具，而此时
        // `mob.mover` 仍是载具那份（控制段才临时换回自己的）。
        // Vanilla `this.mob.getMoveControl()`: while borrowing the getter resolves to
        // the vehicle, and `mob.mover` is still the vehicle's here.
        if mob.mob_type == super::MobType::Rabbit {
            super::rabbit::set_wanted(me, mob, wanted, speed);
        } else {
            mob.mover
                .set_wanted_position(wanted.x, wanted.y, wanted.z, speed);
        }
    }
    mob.restore_borrowed_navigation();
    // 官方顺序：`navigation.tick` 之后才是 `customServerAiStep`，跑大脑的生物在这里跑大脑。
    // 大脑和目标同属「决策」，降频时一起跳过；导航沿已有路径仍每刻照跑。
    // Vanilla order: `customServerAiStep`, where brain mobs run their brain, comes after
    // `navigation.tick`. Brains are decisions and skip with the goals when throttled.
    if plan.decisions_run {
        tick_brains(me, mob, ctx, plan.decision_gap);
    }
    // `Animal.customServerAiStep`。
    if mob.age != 0 {
        mob.in_love = 0;
    }
    // 官方 `moveControl.tick` / `lookControl.tick` / `jumpControl.tick` 用字段：借导航时
    // 临时换回骑手自己的移动控制（视线 / 跳跃控制本来就不转给载具）。
    // Vanilla ticks the `this.moveControl` field; while borrowing, temporarily swap the
    // rider's own move control back in (look / jump controls never delegate).
    mob.swap_to_own_mover();
    tick_controls(me, mob, ctx);
    mob.restore_borrowed_mover();
}

/// `Mob.serverAiStep` 的 `moveControl.tick` / `lookControl.tick` / `jumpControl.tick`。
/// 单独成函数是为了让兔子的提前 `return` 只退出这一段，不影响借导航的换回。
///
/// The control ticks of `Mob.serverAiStep`, split out so the rabbit's early
/// `return` does not skip restoring a borrowed move control.
fn tick_controls(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    if mob.mob_type == super::MobType::Rabbit {
        super::rabbit::custom_server_ai_step(me, mob);
        super::rabbit::before_move_control(me, mob);
        move_control_tick(me, mob, ctx.blocks());
        look_control_tick(me, mob);
        super::rabbit::tick_jump_control(me, mob);
        return;
    }
    if mob.mob_type == super::MobType::Fox {
        // `FoxMoveControl.tick` 只在 `canMove` 时跑；`FoxLookControl.tick` 睡着时整个跳过。
        // `FoxMoveControl` runs only when the fox can move; its look control skips sleep.
        if mob.fox.can_move() {
            move_control_tick(me, mob, ctx.blocks());
        }
        if !mob.fox.is_sleeping() {
            look_control_tick(me, mob);
        }
    } else if mob.mob_type == super::MobType::Slime {
        super::slime::move_control_tick(me, mob);
        look_control_tick(me, mob);
    } else if mob.mob_type == super::MobType::SulfurCube {
        super::sulfur_cube::move_control_tick(me, mob);
        super::sulfur_cube::look_control_tick(me, mob);
    } else if mob.mob_type == super::MobType::Phantom {
        // `PhantomLookControl.tick` 是空的。
        // `PhantomLookControl.tick` is empty.
        super::phantom::move_control_tick(me, mob);
    } else if mob.mob_type == super::MobType::Tadpole {
        move_control_tick(me, mob, ctx.blocks());
        super::tadpole::look_control_tick(me, mob);
    } else if mob.mob_type == super::MobType::Frog {
        // `Frog.FrogLookControl.resetXRotOnTick`：舌头有目标时不把俯仰归零。
        // `Frog.FrogLookControl.resetXRotOnTick`: the pitch is kept while the tongue has
        // a target.
        let keep_x_rot = super::frog::has_tongue_target(mob, ctx);
        super::frog::move_control_tick(me, mob);
        look_control_tick_with(me, mob, !keep_x_rot);
    } else if mob.mob_type == super::MobType::Drowned {
        let target = super::goals::current_combat_target(mob, ctx);
        let target_in_water = super::goals::current_target_in_water(mob, ctx);
        super::drowned::move_control_tick(me, mob, ctx.blocks(), target, target_in_water);
        look_control_tick(me, mob);
    } else if mob.mob_type == super::MobType::Axolotl {
        // `AxolotlMoveControl` / `AxolotlLookControl`：装死时两支都停。
        // `AxolotlMoveControl` / `AxolotlLookControl`: both stop while playing dead.
        super::axolotl::move_control_tick(me, mob);
        super::axolotl::look_control_tick(me, mob);
    } else if mob.mob_type == super::MobType::Dolphin {
        // 海豚的 `SmoothSwimmingMoveControl(85, 10, 0.02F, 0.1F, true)` 与
        // `SmoothSwimmingLookControl(this, 10)`。
        // The dolphin's `SmoothSwimmingMoveControl(85, 10, 0.02F, 0.1F, true)` and
        // `SmoothSwimmingLookControl(this, 10)`.
        super::smooth_swim::move_tick(me, mob, &super::smooth_swim::FROG);
        super::smooth_swim::look_tick(me, mob, 10.0);
    } else if super::nautilus::is_nautilus(mob.mob_type) {
        // `AbstractNautilus` 的 `SmoothSwimmingMoveControl` / `SmoothSwimmingLookControl`。
        // The `AbstractNautilus` smooth-swimming move and look controls.
        super::smooth_swim::move_tick(me, mob, &super::nautilus::MOVE_PARAMS);
        super::smooth_swim::look_tick(me, mob, 10.0);
    } else if mob.mob_type == super::MobType::Bee {
        // `Bee`: `FlyingMoveControl(this, 20, true)` + `BeeLookControl`（生气时跳过，授粉时不重置俯仰）。
        // `Bee`: `FlyingMoveControl(this, 20, true)` + `BeeLookControl` (skipped while angry;
        // keeps pitch while pollinating).
        move_control_tick(me, mob, ctx.blocks());
        if !mob.anger.is_angry(ctx.blocks().game_time()) {
            look_control_tick_with(me, mob, !mob.bee.pollinating);
        }
    } else {
        if mob.mob_type.is_camel() {
            // `CamelMoveControl.tick` 在 `super.tick()` 之前。
            // `CamelMoveControl.tick` runs before `super.tick()`.
            super::camel::before_move_control(me, mob, ctx.blocks().game_time());
        }
        move_control_tick(me, mob, ctx.blocks());
        look_control_tick(me, mob);
    }
    // `JumpControl.tick`。
    mob.jumping = mob.jump.jump;
    mob.jump.jump = false;
}

/// 大脑（含传感器）和 `customServerAiStep` 里那一支专属决策。照官方在 `navigation.tick`
/// 之后调用；降频时和目标一起跳过，导航沿已有路径、移动 / 视线 / 跳跃控制仍每刻照跑。
fn tick_brains(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx, decision_gap: i64) {
    if mob.mob_type == super::MobType::Armadillo {
        super::armadillo::custom_server_ai_step(me, mob, ctx);
    }
    if mob.mob_type == super::MobType::Goat {
        super::goat::custom_server_ai_step(me, mob, ctx);
    }
    if mob.mob_type == super::MobType::Frog {
        super::frog::custom_server_ai_step(me, mob, ctx);
    }
    if mob.mob_type == super::MobType::Tadpole {
        super::tadpole::custom_server_ai_step(me, mob, ctx);
    }
    if mob.mob_type.is_camel() {
        super::camel::custom_server_ai_step(me, mob, ctx);
    }
    if mob.mob_type == super::MobType::Axolotl {
        super::axolotl::custom_server_ai_step(me, mob, ctx);
    }
    if mob.mob_type == super::MobType::Enderman {
        super::enderman::custom_server_ai_step(me, mob, ctx);
    }
    if mob.mob_type == super::MobType::Bat {
        super::bat::custom_server_ai_step(me, mob, ctx);
    }
    if mob.mob_type == super::MobType::Bee {
        super::bee::custom_server_ai_step(me, mob, ctx);
    }
    if mob.mob_type == super::MobType::Ocelot {
        super::ocelot::custom_server_ai_step(mob);
    }
    if mob.mob_type == super::MobType::Cat {
        super::cat::custom_server_ai_step(mob);
    }
    if mob.mob_type == super::MobType::Villager {
        super::villager_ai::custom_server_ai_step(me, mob, ctx, decision_gap);
    }
    if super::nautilus::is_nautilus(mob.mob_type) {
        super::nautilus_ai::custom_server_ai_step(me, mob, ctx);
    }
    if mob.mob_type == super::MobType::Creaking {
        super::creaking_ai::custom_server_ai_step(me, mob, ctx);
    }
    if mob.mob_type == super::MobType::Sniffer {
        super::sniffer::custom_server_ai_step(me, mob, ctx);
    }
    if mob.mob_type == super::MobType::ElderGuardian {
        // `ElderGuardian.customServerAiStep`：范围挖掘疲劳和家。
        // `ElderGuardian.customServerAiStep`: area mining fatigue and home.
        super::guardian::custom_server_ai_step(me, mob, ctx);
    }
}

/// `Mob.getHeadRotSpeed()`：青蛙 35，其余 10。
///
/// `Mob.getHeadRotSpeed()`: 35 for frogs, 10 otherwise.
pub(crate) fn head_rot_speed(mob: &Mob) -> f32 {
    if mob.mob_type == super::MobType::Frog {
        35.0
    } else {
        control::HEAD_ROT_SPEED as f32
    }
}

/// `Mob.setSpeed`：同时设 `zza`。
pub(crate) fn set_speed(mob: &mut Mob, speed: f32) {
    mob.speed = speed;
    mob.zza = speed;
}

/// `MoveControl.tick`（牛不横移，`STRAFE` 走不到）。
pub(crate) fn move_control_tick(me: &mut Entity, mob: &mut Mob, view: &dyn BlockView) {
    // `CreakingMoveControl.tick`：冻结时整个跳过 `MoveControl.tick`。
    // `CreakingMoveControl.tick`: skip `MoveControl.tick` entirely while frozen.
    if mob.mob_type == super::MobType::Creaking && !mob.creaking.can_move {
        return;
    }
    // `Panda.PandaMoveControl.tick`：做不了动作时不响应移动控制。
    // `Panda.PandaMoveControl.tick`: no move control while an action is playing.
    if mob.mob_type == super::MobType::Panda && !mob.panda_can_perform_action(view.is_thundering())
    {
        return;
    }
    if super::guardian::is_guardian(mob.mob_type) {
        // `Guardian` 用 `GuardianMoveControl`。
        // Guardians use `GuardianMoveControl`.
        super::guardian::move_control_tick(me, mob);
        return;
    }
    if super::fish::is_fish(mob.mob_type) {
        super::fish::move_control_tick(me, mob);
        return;
    }
    if mob.mob_type == super::MobType::Parrot {
        // `Parrot` 用 `FlyingMoveControl`。
        // Parrots use `FlyingMoveControl`.
        super::parrot::move_control_tick(me, mob);
        return;
    }
    if mob.mob_type == super::MobType::Bee {
        // `Bee` 用 `FlyingMoveControl(this, 20, true)`：最大俯仰步长 20°，空闲时原地悬停不恢复重力。
        // `Bee` uses `FlyingMoveControl(this, 20, true)`: max turn 20°, hovers in place.
        super::parrot::flying_move_control_tick(me, mob, 20.0, true);
        return;
    }
    if super::turtle::is_turtle(mob.mob_type) {
        super::turtle::move_control_tick(me, mob);
        return;
    }
    match mob.mover.operation {
        Operation::MoveTo => {
            mob.mover.operation = Operation::Wait;
            let dx = mob.mover.wanted_x - me.position.x;
            let dz = mob.mover.wanted_z - me.position.z;
            let dy = mob.mover.wanted_y - me.position.y;
            let distance = dx * dx + dy * dy + dz * dz;
            if distance < 2.500_000_277_905_201e-7 {
                mob.zza = 0.0;
                return;
            }
            let yaw = (mth::atan2(dz, dx) * 57.295_776_367_187_5) as f32 - 90.0;
            me.y_rot = move_rotlerp(me.y_rot, yaw, 90.0);
            let speed =
                (mob.mover.speed_modifier * mob.attributes.value(attribute::MOVEMENT_SPEED)) as f32;
            set_speed(mob, speed);
            let pos = me.block_position();
            let state = view.block_state(pos);
            let piston = state
                .is(mc_bamboo_data::blocks::MOVING_PISTON)
                .then(|| view.moving_piston_shape(pos))
                .flatten();
            let boxes = shape::block_collision_boxes(
                state,
                pos,
                CollisionContext::Empty,
                piston.as_deref(),
                view.shulker_box_shape(pos),
            );
            let width = mth::java_max_f32(1.0, me.dimensions.0);
            let step_over =
                dy > f64::from(max_up_step(mob)) && dx * dx + dz * dz < f64::from(width);
            let inside = !boxes.is_empty()
                && me.position.y < super::path::max_y(&boxes) + f64::from(pos.y)
                && control::blocks_jump(state);
            if step_over || inside {
                mob.jump.jump = true;
                mob.mover.operation = Operation::Jumping;
            }
        }
        Operation::Jumping => {
            let speed =
                (mob.mover.speed_modifier * mob.attributes.value(attribute::MOVEMENT_SPEED)) as f32;
            set_speed(mob, speed);
            if me.on_ground || is_in_liquid(me, mob) {
                mob.mover.operation = Operation::Wait;
            }
        }
        Operation::Strafe => super::skeleton::strafe(me, mob, view),
        Operation::Wait => mob.zza = 0.0,
    }
}

/// `LookControl.tick`。
/// `LookControl.tick` 的公开入口，给别的生物模块在默认路径复用。
pub(crate) fn look_control_tick_public(me: &mut Entity, mob: &mut Mob) {
    look_control_tick(me, mob);
}

/// `LookControl.tick`。
fn look_control_tick(me: &mut Entity, mob: &mut Mob) {
    let reset_x_rot = mob.mob_type != super::MobType::Fox || super::fox::resets_x_rot(mob);
    look_control_tick_with(me, mob, reset_x_rot);
}

/// `LookControl.tick`，`resetXRotOnTick()` 的结果由调用方给。
///
/// `LookControl.tick` with the caller's `resetXRotOnTick()`.
fn look_control_tick_with(me: &mut Entity, mob: &mut Mob, reset_x_rot: bool) {
    if reset_x_rot {
        mob.x_rot = clamp_x_rot(0.0, mob.x_rot);
    }
    if mob.look.look_at_cooldown > 0 {
        mob.look.look_at_cooldown -= 1;
        if let Some(target) = look_y_rot(&mob.look, me.position.x, me.position.z) {
            mob.y_head_rot = rotate_towards(mob.y_head_rot, target, mob.look.y_max_rot_speed);
        }
        if let Some(target) = look_x_rot(&mob.look, me.position.x, eye_y(me, mob), me.position.z) {
            let rotated = rotate_towards(mob.x_rot, target, mob.look.x_max_rot_angle);
            mob.x_rot = clamp_x_rot(rotated, mob.x_rot);
        }
    } else {
        mob.y_head_rot = rotate_towards(mob.y_head_rot, mob.y_body_rot, 10.0);
    }
    if !mob.navigation.is_done() {
        mob.y_head_rot =
            mth::rotate_if_necessary(mob.y_head_rot, mob.y_body_rot, max_head_y_rot(mob));
    }
}

/// `Mob.getMaxHeadYRot()`：犰狳受惊时 0、平时 32，其余 75。
///
/// `Mob.getMaxHeadYRot()`: 0 for a scared armadillo, 32 otherwise; 75 for the rest.
fn max_head_y_rot(mob: &Mob) -> f32 {
    if mob.mob_type == super::MobType::Armadillo {
        super::armadillo::max_head_y_rot(mob)
    } else if mob.mob_type == super::MobType::Goat {
        // `Goat.getMaxHeadYRot`。
        15.0
    } else if mob.mob_type == super::MobType::Frog {
        // `Frog.getMaxHeadYRot`。
        5.0
    } else if mob.mob_type.is_camel() {
        // `Camel.getMaxHeadYRot()`。
        super::camel::max_head_y_rot()
    } else if mob.mob_type == super::MobType::Sniffer {
        // `Sniffer.getMaxHeadYRot()`。
        // `Sniffer.getMaxHeadYRot()`.
        50.0
    } else {
        MAX_HEAD_Y_ROT as f32
    }
}

/// `BodyRotationControl.clientTick`；犰狳受惊、骆驼 `refuseToMove()` 时整个跳过。
///
/// `BodyRotationControl.clientTick`; skipped entirely for a scared armadillo and
/// for a camel that `refuseToMove()` (`CamelBodyRotationControl`).
fn body_rotation_tick(me: &Entity, mob: &mut Mob, time: i64) {
    if super::armadillo::is_scared(mob) {
        return;
    }
    if mob.mob_type == super::MobType::Phantom {
        super::phantom::body_rotation_tick(me, mob);
        return;
    }
    if mob.mob_type.is_camel() && super::camel::refuse_to_move(mob, time) {
        return;
    }
    let dx = me.position.x - me.old_position.x;
    let dz = me.position.z - me.old_position.z;
    let max = max_head_y_rot(mob);
    if dx * dx + dz * dz > 2.500_000_277_905_201e-7 {
        mob.y_body_rot = me.y_rot;
        mob.y_head_rot = mth::rotate_if_necessary(mob.y_head_rot, mob.y_body_rot, max);
        mob.body.last_stable_y_head_rot = mob.y_head_rot;
        mob.body.head_stable_time = 0;
        return;
    }
    if (mob.y_head_rot - mob.body.last_stable_y_head_rot).abs() > 15.0 {
        mob.body.head_stable_time = 0;
        mob.body.last_stable_y_head_rot = mob.y_head_rot;
        mob.y_body_rot = mth::rotate_if_necessary(mob.y_body_rot, mob.y_head_rot, max);
    } else {
        mob.body.head_stable_time += 1;
        if mob.body.head_stable_time > 10 {
            let since = mob.body.head_stable_time - 10;
            let progress = mth::clamp_f32(since as f32 / 10.0, 0.0, 1.0);
            let limit = max * (1.0 - progress);
            mob.y_body_rot = mth::rotate_if_necessary(mob.y_body_rot, mob.y_head_rot, limit);
        }
    }
}

// ---- 跳跃 ----

/// `Mob.jumpInLiquid`：会浮水时加 `(double) 0.04F`；蜜蜂固定加 `0.01`。
fn jump_in_liquid(me: &mut Entity, mob: &Mob) {
    let lift = if mob.mob_type == super::MobType::Bee {
        0.01
    } else if mob.navigation.can_float() {
        f64::from(0.04_f32)
    } else {
        0.3
    };
    me.delta = me.delta.add(Vec3::new(0.0, lift, 0.0));
}

/// `getBlockJumpFactor`：只有蜂蜜块是 0.5。
pub(super) fn block_jump_factor(me: &Entity, view: &dyn BlockView) -> f32 {
    let factor = |state: crate::state::BlockState| {
        if state.is(mc_bamboo_data::blocks::HONEY_BLOCK) {
            0.5_f32
        } else {
            1.0
        }
    };
    let here = factor(view.block_state(me.block_position()));
    let below = factor(view.block_state(me.on_pos(view, 0.500_001)));
    if f64::from(here) == 1.0 { below } else { here }
}

/// `LivingEntity.getJumpBoostPower()`：每级 +0.1。
///
/// `LivingEntity.getJumpBoostPower()`: +0.1 per level.
pub(crate) fn jump_boost_power(mob: &Mob) -> f32 {
    mob.effects
        .amplifier(mob_effect::JUMP_BOOST)
        .map_or(0.0, |amplifier| 0.1 * (amplifier.max(0) as f32 + 1.0))
}

/// `LivingEntity.setSprinting`：更新共享疾跑标志并在 `MOVEMENT_SPEED` 上增删
/// `minecraft:sprinting`（+0.3F，`ADD_MULTIPLIED_TOTAL`，临时修饰符）。
///
/// `LivingEntity.setSprinting`: update the shared sprinting flag and add/remove the
/// transient `minecraft:sprinting` (+0.3F, `ADD_MULTIPLIED_TOTAL`) speed modifier.
pub(super) fn set_sprinting(mob: &mut Mob, sprinting: bool) {
    mob.sprinting = sprinting;
    if let Some(speed) = mob.attributes.instance(attribute::MOVEMENT_SPEED) {
        let removed = speed.remove_modifier("minecraft:sprinting");
        if sprinting {
            speed.modifiers.push(super::attributes::Modifier {
                id: "minecraft:sprinting".to_owned(),
                amount: f64::from(0.3_f32),
                operation: super::attributes::Operation::AddMultipliedTotal,
                permanent: false,
            });
        }
        if removed || sprinting {
            speed.touch_sync();
        }
    }
}

/// `jumpFromGround`。
fn jump_from_ground(me: &mut Entity, mob: &mut Mob, view: &dyn BlockView) {
    let strength = mob.attributes.value(attribute::JUMP_STRENGTH) as f32;
    let boost = jump_boost_power(mob);
    let is_rabbit = mob.mob_type == super::MobType::Rabbit;
    let multiplier = if is_rabbit {
        super::rabbit::jump_power_multiplier(me, mob)
    } else {
        1.0
    };
    let power = strength * multiplier * block_jump_factor(me, view) + boost;
    if power > 1.0e-5 {
        me.delta.y = mth::java_max(f64::from(power), me.delta.y);
        if mob.sprinting {
            let angle = me.y_rot * (std::f32::consts::PI / 180.0);
            let sin = f64::from(mth::sin(f64::from(angle)));
            let cos = f64::from(mth::cos(f64::from(angle)));
            me.delta = me.delta.add(Vec3::new(-sin * 0.2, 0.0, cos * 0.2));
        }
        mob.needs_sync = true;
    }
    if is_rabbit {
        super::rabbit::after_ground_jump(me, mob);
    }
}

// ---- travel ----

/// `LivingEntity.travel`。
fn travel(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx, mut input: Vec3) {
    // `Camel.travel`：`refuseToMove && onGround` 时把水平速度和输入清零，再跑 `super.travel`。
    // `Camel.travel`: zero horizontal velocity / input while refusing on ground, then
    // `super.travel`.
    if mob.mob_type.is_camel() {
        super::camel::travel(me, mob, ctx.blocks().game_time(), &mut input);
    }
    let in_liquid = is_in_liquid(me, mob);
    let levitation = mob.effects.amplifier(mob_effect::LEVITATION);
    if in_liquid {
        // `Drowned.travelInWater` 覆写只在眼睛也进水、且想游的时候接管。
        // The `Drowned.travelInWater` override only takes over with the eyes under water
        // while it wants to swim.
        let drowned_swims = if mob.mob_type == super::MobType::Drowned {
            let target_in_water = super::goals::current_target_in_water(mob, ctx);
            super::drowned::travel_in_water(me, mob, ctx, input, target_in_water)
        } else {
            false
        };
        if !drowned_swims {
            travel_in_fluid(me, mob, ctx, input);
        }
    } else {
        travel_in_air(me, mob, ctx, input, levitation);
    }
    // 官方漂浮分支在 `travel` 里替换重力项；水里的漂浮效果由
    // `travel_in_fluid` 的重力分支处理，这里只补一个非水 / 水外的等价调整，
    // 已经由 `travel_in_air` 的 `levitation` 参数覆盖。
    if in_liquid && let Some(amplifier) = levitation {
        let target = 0.05 * f64::from(amplifier.max(0) + 1);
        let delta = me.delta;
        me.delta = Vec3::new(delta.x, delta.y + (target - delta.y) * 0.2, delta.z);
    }
}

/// `computeModifiedFriction`。
fn modified_friction(friction: f32, modifier: f32) -> f32 {
    mth::clamp_f32(1.0 - (1.0 - friction) * modifier, 0.0, 1.0)
}

/// `getEffectiveGravity`：读调用时的速度。
fn effective_gravity(me: &Entity, mob: &Mob) -> f64 {
    if mob.no_gravity {
        return 0.0;
    }
    let gravity = mob.attributes.value(attribute::GRAVITY);
    // `LivingEntity.getEffectiveGravity`：下落时缓降把重力压到 0.01。
    if me.delta.y <= 0.0 && mob.effects.has(mob_effect::SLOW_FALLING) {
        gravity.min(0.01)
    } else {
        gravity
    }
}

/// `travelInAir`。
fn travel_in_air(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    input: Vec3,
    levitation: Option<i32>,
) {
    let below = me.on_pos(ctx.blocks(), 0.500_001);
    let block_friction = if me.on_ground {
        modified_friction(
            crate::entity::item::friction(ctx.blocks().block_state(below)),
            mob.attributes.value(attribute::FRICTION_MODIFIER) as f32,
        )
    } else {
        1.0
    };
    let movement = relative_friction_movement(me, mob, ctx, input, block_friction);
    // `LivingEntity.travel`：漂浮效果替换掉重力项。
    let y = match levitation {
        Some(amplifier) => {
            let target = 0.05 * f64::from(amplifier.max(0) + 1);
            movement.y + (target - movement.y) * 0.2
        }
        None => movement.y - effective_gravity(me, mob),
    };
    // `shouldDiscardFriction()`：长跳途中速度原样保留。
    // `shouldDiscardFriction()`: a long jump keeps its velocity untouched.
    if mob.discard_friction {
        me.delta = Vec3::new(movement.x, y, movement.z);
        return;
    }
    let air_drag = mob.attributes.value(attribute::AIR_DRAG_MODIFIER) as f32;
    let horizontal_air = modified_friction(0.91, air_drag);
    let horizontal = block_friction * horizontal_air;
    // `LivingEntity.travelInAir`：`omnidirectionalAirMover()`（鹦鹉、蜜蜂）竖直方向也用
    // 0.91 的空气阻力，而不是 0.98。
    // `LivingEntity.travelInAir`: an `omnidirectionalAirMover()` (parrot, bee) uses the
    // 0.91 air drag vertically instead of 0.98.
    // `SulfurCube.omnidirectionalAirMover()` 是 `hasBodyItem()`，装了方块时
    // 竖直方向也用 0.91 的空气阻力。
    let vertical = if matches!(mob.mob_type, super::MobType::Parrot | super::MobType::Bee)
        || (mob.mob_type == super::MobType::SulfurCube && super::sulfur_cube::has_body_item(mob))
    {
        horizontal_air
    } else {
        modified_friction(0.98, air_drag)
    };
    me.delta = Vec3::new(
        movement.x * f64::from(horizontal),
        y * f64::from(vertical),
        movement.z * f64::from(horizontal),
    );
}

/// `handleRelativeFrictionAndCalculateMovement`。
fn relative_friction_movement(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    input: Vec3,
    friction: f32,
) -> Vec3 {
    let speed = if me.on_ground {
        if f64::from(friction) > 0.6 {
            mob.speed * (0.216_000_02 / (friction * friction * friction))
        } else {
            mob.speed
        }
    } else {
        0.02
    };
    move_relative(me, speed, input);
    me.delta = handle_on_climbable(me, mob, ctx.blocks(), me.delta);
    let movement = me.delta;
    motion::move_living(me, mob, ctx, movement);
    let mut delta = me.delta;
    // 细雪那一支：上一刻在细雪里、又能在细雪上走的生物（兔子、狐狸）往上爬。
    // The powder snow branch: walkers that were in powder snow climb up.
    if (me.horizontal_collision || mob.jumping)
        && (on_climbable(me, mob, ctx.blocks())
            || (mob.was_in_powder_snow && mob.walks_on_powder_snow()))
    {
        delta = Vec3::new(delta.x, 0.2, delta.z);
    }
    delta
}

/// `Entity.moveRelative`。
pub(crate) fn move_relative(me: &mut Entity, speed: f32, input: Vec3) {
    let length = input.length_sqr();
    if length < 1.0e-7 {
        return;
    }
    let scaled = if length > 1.0 {
        input.normalize()
    } else {
        input
    }
    .scale(f64::from(speed));
    let angle = f64::from(me.y_rot * 0.017_453_292);
    let sin = f64::from(mth::sin(angle));
    let cos = f64::from(mth::cos(angle));
    let push = Vec3::new(
        scaled.x * cos - scaled.z * sin,
        scaled.y,
        scaled.z * cos + scaled.x * sin,
    );
    me.delta = me.delta.add(push);
}

/// `LivingEntity.onClimbable`：可攀爬的方块，或下面是同向梯子的开着的活板门。
/// 蜘蛛改写这一条，只看自己的攀爬标志。
///
/// `LivingEntity.onClimbable`: a climbable block, or an open trapdoor over a matching
/// ladder. Spiders override it to read their own climbing flag.
pub(crate) fn on_climbable(me: &Entity, mob: &mut Mob, view: &dyn BlockView) -> bool {
    if mob.mob_type.climbs_walls() {
        return super::spider::on_climbable(mob);
    }
    // `AbstractHorse.onClimbable` 恒假，马科不爬梯子和藤蔓；`Phantom.onClimbable` 也恒假。
    // `AbstractHorse.onClimbable` is always false; equines never climb ladders or vines.
    // `Phantom.onClimbable` is always false as well.
    if mob.mob_type.is_equine() || mob.mob_type == super::MobType::Phantom {
        return false;
    }
    let pos = me.block_position();
    let state = view.block_state(pos);
    if state.is_tagged(block_tags::CLIMBABLE) {
        mob.last_climbable_pos = Some(pos);
        return true;
    }
    if state.is_a(mc_bamboo_data::BlockAncestor::TrapDoorBlock)
        && state.try_get(crate::state::OPEN) == Some(true)
    {
        let below = view.block_state(pos.below());
        if below.is(mc_bamboo_data::blocks::LADDER)
            && below.try_get(crate::state::HORIZONTAL_FACING)
                == state.try_get(crate::state::HORIZONTAL_FACING)
        {
            mob.last_climbable_pos = Some(pos);
            return true;
        }
    }
    false
}

fn handle_on_climbable(me: &mut Entity, mob: &mut Mob, view: &dyn BlockView, delta: Vec3) -> Vec3 {
    if !on_climbable(me, mob, view) {
        return delta;
    }
    me.fall_distance = 0.0;
    let limit = f64::from(0.15_f32);
    let x = mth::clamp(delta.x, -limit, limit);
    let z = mth::clamp(delta.z, -limit, limit);
    let y = mth::java_max(delta.y, -limit);
    Vec3::new(x, y, z)
}

/// `travelInFluid`。
fn travel_in_fluid(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx, input: Vec3) {
    // `AbstractFish.travelInWater` 整个换掉了水里的推进。
    // `AbstractFish.travelInWater` replaces the whole in-water path.
    if me.in_water && super::nautilus::is_nautilus(mob.mob_type) {
        super::nautilus::travel_in_water(me, mob, ctx, input);
        return;
    }
    if me.in_water && super::fish::is_fish(mob.mob_type) {
        super::fish::travel_in_water(me, mob, ctx, input);
        return;
    }
    // `Guardian.travelInWater` 换掉水里的推进。
    // `Guardian.travelInWater` replaces the whole in-water thrust.
    if me.in_water && super::guardian::is_guardian(mob.mob_type) {
        super::guardian::travel_in_water(me, mob, ctx, input);
        return;
    }
    // `Frog.travelInWater` 换掉了水里的推进。
    // `Frog.travelInWater` replaces the in-water thrust.
    if me.in_water && mob.mob_type == super::MobType::Frog {
        super::frog::travel_in_water(me, mob, ctx, input);
        return;
    }
    // `Turtle.travelInWater` 换掉了水里的推进。
    // `Turtle.travelInWater` replaces the in-water thrust.
    if me.in_water && super::turtle::is_turtle(mob.mob_type) {
        super::turtle::travel_in_water(me, mob, ctx, input);
        return;
    }
    // `Dolphin.travelInWater` / `Axolotl.travelInWater` 换掉水里的推进。
    // `Dolphin.travelInWater` / `Axolotl.travelInWater` replace the in-water thrust.
    if me.in_water && mob.mob_type == super::MobType::Dolphin {
        super::dolphin::travel_in_water(me, mob, ctx, input);
        return;
    }
    if me.in_water && mob.mob_type == super::MobType::Axolotl {
        super::axolotl::travel_in_water(me, mob, ctx, input);
        return;
    }
    let falling = me.delta.y <= 0.0;
    let old_y = me.position.y;
    let gravity = effective_gravity(me, mob);
    if me.in_water {
        let slow = mob.mob_type.water_slow_down();
        let speed = 0.02_f32;
        let mut efficiency = mob.attributes.value(attribute::WATER_MOVEMENT_EFFICIENCY) as f32;
        if !me.on_ground {
            efficiency *= 0.5;
        }
        let (slow, speed) = if efficiency > 0.0 {
            (
                slow + (0.546_000_06 - slow) * efficiency,
                speed + (mob.speed - speed) * efficiency,
            )
        } else {
            (slow, speed)
        };
        move_relative(me, speed, input);
        let movement = me.delta;
        motion::move_living(me, mob, ctx, movement);
        let mut delta = me.delta;
        if me.horizontal_collision && on_climbable(me, mob, ctx.blocks()) {
            delta = Vec3::new(delta.x, 0.2, delta.z);
        }
        delta = delta.multiply(f64::from(slow), f64::from(0.8_f32), f64::from(slow));
        me.delta = fluid_falling_adjusted(gravity, falling, delta);
    } else {
        move_relative(me, 0.02, input);
        let movement = me.delta;
        motion::move_living(me, mob, ctx, movement);
        if fluid_height(mob, fluid_tags::LAVA) <= fluid_jump_threshold(mob) {
            me.delta = me.delta.multiply(0.5, f64::from(0.8_f32), 0.5);
            me.delta = fluid_falling_adjusted(gravity, falling, me.delta);
        } else {
            me.delta = me.delta.scale(0.5);
        }
        if gravity != 0.0 {
            me.delta = me.delta.add(Vec3::new(0.0, -gravity / 4.0, 0.0));
        }
    }
    jump_out_of_fluid(me, mob, ctx.blocks(), old_y);
}

/// `getFluidFallingAdjustedMovement`。
fn fluid_falling_adjusted(gravity: f64, falling: bool, delta: Vec3) -> Vec3 {
    if gravity == 0.0 {
        return delta;
    }
    let y = if falling
        && (delta.y - 0.005).abs() >= 0.003
        && (delta.y - gravity / 16.0).abs() < 0.003
    {
        -0.003
    } else {
        delta.y - gravity / 16.0
    };
    Vec3::new(delta.x, y, delta.z)
}

/// `jumpOutOfFluid`。
fn jump_out_of_fluid(me: &mut Entity, mob: &Mob, view: &dyn BlockView, old_y: f64) {
    let delta = me.delta;
    if !me.horizontal_collision {
        return;
    }
    let dy = delta.y + f64::from(0.6_f32) - me.position.y + old_y;
    let moved = me.bounding_box().moved(delta.x, dy, delta.z);
    if motion::is_free(me, mob, view, moved) {
        me.delta = Vec3::new(delta.x, f64::from(0.3_f32), delta.z);
    }
}

// ---- 推挤 ----

/// `LivingEntity.pushEntities`：身边的生物互相推开。蝙蝠把它覆写成空的，别人的
/// `EntitySelector.pushableBy` 也因为 `isPushable()` 为假而跳过蝙蝠。
///
/// `LivingEntity.pushEntities`: nearby mobs shove each other apart. Bats override it with
/// an empty body, and `EntitySelector.pushableBy` skips them as `isPushable()` is false.
/// `Entity.getRootVehicle()`：沿 `vehicle` 往上走到没有载具的那个 uid（自己骑自己时
/// 到环为止）。`isPassengerOfSameVehicle` 用它判断两边是否同一棵树。
///
/// `Entity.getRootVehicle()`: walk `vehicle` links up to the topmost uid, stopping
/// at a cycle. `isPassengerOfSameVehicle` compares these.
fn root_vehicle(uid: EntityUid, vehicle: Option<EntityUid>, ctx: &dyn EntityCtx) -> EntityUid {
    let mut root = uid;
    let mut current = vehicle;
    let mut seen = 0_u32;
    while let Some(next) = current {
        root = next;
        seen += 1;
        if seen > 64 {
            break;
        }
        current = ctx.other(next).and_then(|public| public.vehicle);
    }
    root
}

fn push_entities(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    if !mob.mob_type.is_pushable() {
        return;
    }
    let my_root = root_vehicle(me.id, me.vehicle, ctx);
    let rules = ctx.blocks().block_game_rules();
    let cap = rules.max_entity_collisions;
    let tick = ctx.blocks().game_time();
    // 刻号不同就从 0 起算，不必每刻扫一遍实体表；精确串行和并行算阶段都读
    // 同一个岛时钟，两种驱动的计数一致。
    let used = if mob.push_count.0 == tick {
        mob.push_count.1
    } else {
        0
    };
    let remaining = if cap == 0 {
        usize::MAX
    } else {
        cap.saturating_sub(used)
    };
    let bounds = me.bounding_box();
    let mut others = Vec::new();
    // `IronGolem.doPush` 要看被推的是不是敌对生物，只有铁傀儡才记下对方的公开状态。
    // `IronGolem.doPush` checks whether the pushed mob is an enemy, so only iron golems
    // keep the other mob's public state.
    let iron_golem = mob.mob_type == super::MobType::IronGolem;
    let mut golem_pushed = Vec::new();
    // 上限下推到查询里：够数就停止枚举，塔场景不必把几百个候选全排一遍。
    ctx.pushable_others(bounds, remaining, &mut |other| {
        if let KindPublic::Mob(public) = &other.kind
            && !other.removed
            && public.is_alive()
            && public.mob_type.is_pushable()
            && root_vehicle(other.uid, other.vehicle, ctx) != my_root
        {
            others.push((other.uid, other.position));
            if iron_golem {
                golem_pushed.push((other.uid, public.as_ref().clone()));
            }
            return true;
        }
        false
    });
    // `getPushableEntities` 也收玩家（旁观者除外）；玩家那一侧的推力服务端不生效，
    // 这里只算生物自己被推开的那一半。跳过自己的乘客和这一刻刚甩掉的骑手。
    // Players count as pushable too (not spectators); only the mob's half applies.
    // Skip own passengers and a rider just ejected this tick.
    let ejected = mob.ejected_passenger.take();
    let mut players = Vec::new();
    ctx.blocks().for_each_player(&mut |player| {
        if player.alive
            && !player.spectator
            && !mob.passengers.contains(&player.uid)
            && ejected != Some(player.uid)
            && super::avoid::player_box(player).intersects(bounds)
        {
            players.push(player.position);
        }
    });
    // `maxEntityCramming` 的伤害判定用的是**没截断**的名单：Paper 的
    // `maxEntityCollisions` 不改这条规则。上限打开且名单可能被截断时才补一次
    // 完整计数（只有开着挤压伤害的场景才需要，塔压测把规则设成 0 就跳过）。
    if others.is_empty() && players.is_empty() {
        return;
    }
    let cramming = rules.max_entity_cramming;
    // 上限打开时还要按完整的候选数判定伤害；名单已经够触发就不用再数一遍，
    // 绝大多数拥挤场景走这条快路径。
    let crowd = if cap == 0
        || cramming <= 0
        || others.len() + players.len() > (cramming - 1).max(0) as usize
    {
        (others.len() + players.len()) as i32
    } else {
        let mut count = 0;
        ctx.for_each_other_in(bounds, &mut |other| {
            if matches!(&other.kind, KindPublic::Mob(public)
                if !other.removed && public.is_alive() && public.mob_type.is_pushable())
            {
                count += 1;
            }
        });
        count + players.len() as i32
    };
    if cramming > 0 && crowd > cramming - 1 && me.random.next_int_bounded(4) == 0 {
        timing::record_cramming!();
        hurt_server(
            me,
            mob,
            ctx,
            DamageSource::environment(DamageKind::Cramming),
            6.0,
        );
    }
    timing::record_push!(others.len() as u64);
    // `Entity.push(Entity)` 只有 `!entity.isVehicle()` 才推得动自己这一半；载具和它的
    // 乘客共享 root vehicle，整对被 `isPassengerOfSameVehicle` 跳过，所以这里也不推。
    // `Entity.push(Entity)` pushes its own half only when `!entity.isVehicle()`; a
    // vehicle and its passenger share a root vehicle and the pair is skipped by
    // `isPassengerOfSameVehicle`, so a ridden mob stays put.
    let pushable_self = is_alive(me, mob)
        && !on_climbable(me, mob, ctx.blocks())
        && !mob.is_vehicle()
        && !(mob.mob_type == super::MobType::Villager && mob.villager.sleeping);
    // 玩家排在生物后面（官方先实体后玩家）；上限只保留前面的名额。
    let mut handled = 0;
    // `IronGolem.doPush` 在挤压伤害那次取数之后、逐个推开时取 `nextInt(20)`；
    // 只算上限以内真正被推到的生物，上限关着时就是全部。
    // `IronGolem.doPush` draws `nextInt(20)` after the cramming roll, once per pushed
    // mob; with the cap only the mobs within it count, all of them when it is off.
    let golem_limit = remaining.saturating_sub(players.len().min(remaining));
    for (uid, public) in golem_pushed.iter().take(golem_limit) {
        super::iron_golem::do_push(me, mob, public, *uid);
    }
    for position in players {
        if handled >= remaining {
            break;
        }
        if pushable_self && let Some((dx, dz)) = push_amount(position, me.position) {
            me.delta = me.delta.add(Vec3::new(dx, 0.0, dz));
            // `Entity.push(Entity)` 推开自己那一半时也置 `needsSync`。
            // The self half of `Entity.push(Entity)` also sets `needsSync`.
            mob.needs_sync = true;
        }
        handled += 1;
    }
    for (uid, position) in others {
        if handled >= remaining {
            break;
        }
        ctx.emit(
            Command::PushMob {
                other: uid,
                from: position,
                self_pushable: pushable_self,
            },
            me,
        );
        handled += 1;
    }
    mob.push_count = (tick, used.saturating_add(handled));
}

/// `Entity.push(Entity)` 的数学部分：返回推开的水平量。
pub(crate) fn push_amount(from: Vec3, to: Vec3) -> Option<(f64, f64)> {
    let mut dx = to.x - from.x;
    let mut dz = to.z - from.z;
    let mut max = dx.abs().max(dz.abs());
    if max < f64::from(0.01_f32) {
        return None;
    }
    max = max.sqrt();
    dx /= max;
    dz /= max;
    let mut inverse = 1.0 / max;
    if inverse > 1.0 {
        inverse = 1.0;
    }
    dx *= inverse;
    dz *= inverse;
    dx *= f64::from(0.05_f32);
    dz *= f64::from(0.05_f32);
    Some((dx, dz))
}

// ---- 视线 ----

/// `Sensing.hasLineOfSight` 加 `LivingEntity.hasLineOfSight`：从眼睛到目标眼睛，碰撞形状挡住就看不见。
pub(crate) fn has_line_of_sight(
    me: &Entity,
    mob: &mut Mob,
    view: &dyn BlockView,
    uid: crate::entity::EntityUid,
    target_eye: Vec3,
) -> bool {
    if let Some(&(_, seen)) = mob.seen.iter().find(|(seen_uid, _)| *seen_uid == uid) {
        return seen;
    }
    let from = Vec3::new(me.position.x, eye_y(me, mob), me.position.z);
    let seen = if target_eye.distance_to_sqr(from).sqrt() > 128.0 {
        false
    } else {
        view.clip_collider(from, target_eye, me.mob_collision_context(mob))
            .is_none()
    };
    mob.seen.push((uid, seen));
    seen
}

// ---- 受伤和死亡 ----

/// `hurtServer` 的虚分派：鱿鱼一支有自己的覆盖，其余直接走 `LivingEntity.hurtServer`。
///
/// The virtual dispatch of `hurtServer`: squid override it, everything else goes straight
/// to `LivingEntity.hurtServer`.
pub(crate) fn hurt_server(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    source: DamageSource,
    amount: f32,
) -> bool {
    // `Creaking.hurtServer`：绑定心的嘎枝不掉血，改由心放树脂和连线效果。
    if mob.mob_type == super::MobType::Creaking
        && let Some(home) = super::creaking::hurt_server(me, mob, &source)
    {
        ctx.emit(super::super::ctx::Command::CreakingHeartHurt { home }, me);
        return true;
    }
    // `Panda.hurtServer`：先站起来，再做官方的无敌和冷却判定。
    // `Panda.hurtServer`: stand up first, then the vanilla invulnerability checks.
    if mob.mob_type == super::MobType::Panda {
        mob.panda.sit(false);
    }
    // `Parrot.hurtServer`：受伤时取消坐下命令。
    // `Parrot.hurtServer`: damage clears the sitting order.
    if mob.mob_type == super::MobType::Parrot {
        mob.tame.ordered_to_sit = false;
    }
    // `Bee.hurtServer`：非无敌受伤时停止授粉。
    // `Bee.hurtServer`: stop pollinating on non-invulnerable damage.
    if mob.mob_type == super::MobType::Bee && !mob.invulnerable {
        super::bee_goals::stop_pollinating(mob);
    }
    // `Armadillo.hurtServer`：受惊时先把伤害变成 `(damage - 1) / 2`。
    // `Armadillo.hurtServer`: while scared, damage becomes `(damage - 1) / 2` first.
    let amount = if mob.mob_type == super::MobType::Armadillo {
        super::armadillo::reduce_damage(mob, amount)
    } else {
        amount
    };
    if mob.mob_type == super::MobType::SulfurCube {
        super::sulfur_cube::prime_on_hurt(me, mob, &source);
    }
    if mob.mob_type == super::MobType::IronGolem {
        return super::iron_golem::hurt_server(me, mob, ctx, source, amount);
    }
    // `Silverfish.hurtServer`：无敌判定之后先唤醒附近虫蚀方块里的同伴。
    // `Silverfish.hurtServer`: after the invulnerability check, wake nearby friends.
    if mob.mob_type == super::MobType::Silverfish && !is_invulnerable_to(me, mob, &source) {
        super::silverfish::notify_hurt(mob, &source);
    }
    if super::squid::is_squid(mob.mob_type) {
        return super::squid::hurt_server(me, mob, ctx, source, amount);
    }
    if mob.mob_type == super::MobType::Bat {
        return super::bat::hurt_server(me, mob, ctx, source, amount);
    }
    // `Guardian.hurtServer` 的尖刺反伤与 `randomStrollGoal.trigger()` 在父类之前。
    // `Guardian.hurtServer` runs its thorns reflection and stroll trigger before its
    // superclass.
    if super::guardian::is_guardian(mob.mob_type) {
        return super::guardian::hurt_server(me, mob, ctx, source, amount);
    }
    // `Axolotl.hurtServer` 在父类之前做装死判定（`PLAY_DEAD_TICKS`）。
    // `Axolotl.hurtServer` runs the play-dead roll before the superclass.
    if mob.mob_type == super::MobType::Axolotl {
        let has_source = source.attacker.is_some() || source.direct.is_some();
        super::axolotl::maybe_play_dead(me, mob, amount, has_source);
    }
    living_hurt_server(me, mob, ctx, source, amount)
}

/// `isInvulnerableTo` 里已建模的部分：已移除，或无敌且伤害不能穿透。
///
/// The modelled part of `isInvulnerableTo`: removed, or invulnerable to a source that
/// does not bypass it.
pub(super) fn is_invulnerable_to(me: &Entity, mob: &Mob, source: &DamageSource) -> bool {
    me.removed
        || ((mob.invulnerable || mob.invulnerable_time > 0)
            && !source.kind.bypasses_invulnerability()
            && !source.creative_player)
}

/// `LivingEntity.hurtServer`。
pub(super) fn living_hurt_server(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    source: DamageSource,
    amount: f32,
) -> bool {
    if is_invulnerable_to(me, mob, &source) {
        return false;
    }
    // `Wolf.hurtServer`：没有免疫就先站起来，再走 `super.hurtServer`。
    // `Wolf.hurtServer` stands the wolf up before `super.hurtServer`.
    super::wolf::before_hurt(mob);
    if mob.is_dead_or_dying() {
        return false;
    }
    if mob.mob_type == super::MobType::Enderman {
        if matches!(
            source.kind,
            DamageKind::Arrow | DamageKind::Trident | DamageKind::Spit
        ) {
            super::enderman::dodge_projectile(me, mob, ctx);
            return false;
        }
        let result = hurt_living(me, mob, ctx, source, amount);
        super::enderman::after_hurt(me, mob, ctx, source);
        return result;
    }
    hurt_living(me, mob, ctx, source, amount)
}

fn hurt_living(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    source: DamageSource,
    amount: f32,
) -> bool {
    // 抗火在受伤冷却、音效及攻击者归因之前拒绝火焰伤害。
    // Fire Resistance rejects fire damage before cooldowns, sounds, or attacker bookkeeping.
    if source.kind.is_fire() && mob.effects.has(mob_effect::FIRE_RESISTANCE) {
        return false;
    }
    // `LivingEntity.hurtServer`：睡在床上受伤先起床。
    // `LivingEntity.hurtServer`: wake up before taking damage.
    if mob.mob_type == super::MobType::Villager && mob.villager.sleeping {
        super::villager::stop_sleeping(me, mob, None, ctx);
    }
    mob.no_action_time = 0;
    let mut amount = if amount < 0.0 { 0.0 } else { amount };
    if amount.is_nan() || amount.is_infinite() {
        amount = f32::MAX;
    }
    let full_hit = if mob.damage_cooldown as f32 > 10.0 {
        if amount <= mob.last_hurt {
            return false;
        }
        actually_hurt(me, mob, source, amount - mob.last_hurt);
        after_actually_hurt(me, mob, &source, ctx.blocks().game_time());
        mob.last_hurt = amount;
        false
    } else {
        mob.last_hurt = amount;
        mob.damage_cooldown = 20;
        actually_hurt(me, mob, source, amount);
        after_actually_hurt(me, mob, &source, ctx.blocks().game_time());
        mob.hurt_duration = 10;
        mob.hurt_time = mob.hurt_duration;
        true
    };
    if let Some(attacker) = source.attacker {
        // `resolveMobResponsibleForDamage`：`#no_anger`（山羊冲撞）不记仇。
        // `resolveMobResponsibleForDamage`: `#no_anger` (a goat ram) leaves no grudge.
        if !source.kind.no_anger() {
            mob.last_hurt_by_mob = Some(attacker);
            mob.last_hurt_by_mob_ref = source.attacker_uuid;
            mob.last_hurt_by_mob_timestamp = me.tick_count;
        }
        if source.attacker_is_player {
            mob.last_hurt_by_player = Some(attacker);
            mob.last_hurt_by_player_ref = source.attacker_uuid;
            mob.last_hurt_by_player_memory_time = 100;
        } else if let Some(owner) = source.owner_credit {
            // `resolvePlayerResponsibleForDamage`：驯服的狼咬的算主人的。
            // A tamed wolf's bite is credited to its owner.
            mob.last_hurt_by_player = None;
            mob.last_hurt_by_player_ref = Some(owner);
            mob.last_hurt_by_player_memory_time = 100;
        }
    }
    if full_hit {
        mob.events.push(MobEvent::Damage {
            kind: source.kind,
            attacker: source.attacker,
            direct: source.direct,
        });
        if !source.kind.no_impact() {
            mob.hurt_marked = true;
        }
        if !source.kind.no_knockback() {
            let (dx, dz) = source
                .knockback_direction(me.position)
                .unwrap_or((0.0, 0.0));
            knockback(me, mob, f64::from(0.4_f32), dx, dz);
        }
    }
    if mob.is_dead_or_dying() {
        if full_hit {
            let pitch = voice_pitch(me, mob);
            let sound = voice_of(mob, Voice::Death, me.in_water);
            play_sound(me, mob, sound, sound_volume(mob), pitch);
        }
        die(me, mob, ctx, source);
    } else if full_hit {
        // `Mob.playHurtSound`：先重置叫声计时，再取音高。
        mob.ambient_sound_time = -mob.mob_type.ambient_sound_interval();
        let pitch = voice_pitch(me, mob);
        let sound = voice_of(mob, Voice::Hurt, me.in_water);
        play_sound(me, mob, sound, sound_volume(mob), pitch);
    }
    mob.last_damage_source = Some(source);
    mob.last_damage_stamp = ctx.blocks().game_time();
    if mob.mob_type.is_equine() {
        super::equine::after_hurt(me, mob);
    }
    true
}

/// 物种覆写的 `actuallyHurt` 在 `super` 之后的那部分。
///
/// The part of species `actuallyHurt` overrides after `super`.
fn after_actually_hurt(me: &mut Entity, mob: &mut Mob, source: &DamageSource, time: i64) {
    if mob.mob_type == super::MobType::Armadillo {
        super::armadillo::after_actually_hurt(me, mob, source);
    }
    // `Camel.actuallyHurt` 在 `super` 之前 `standUpInstantly()`；纯状态改动、不取随机数，
    // 放在 `super` 之后对可观察状态等价。
    // `Camel.actuallyHurt` calls `standUpInstantly()` before `super`; a pure state change
    // with no random draw, so after `super` is equivalent for observable state.
    if mob.mob_type.is_camel() {
        super::camel::after_actually_hurt(mob, time);
    }
}

/// `Animal.actuallyHurt` → `LivingEntity.actuallyHurt`。没有护甲、效果和吸收。
fn actually_hurt(me: &mut Entity, mob: &mut Mob, source: DamageSource, amount: f32) {
    mob.in_love = 0;
    let _ = me;
    let mut amount = amount;
    if !source.kind.bypasses_armor() {
        let armor = mth::floor(mob.attributes.value(attribute::ARMOR)) as f32;
        let toughness = mob.attributes.value(attribute::ARMOR_TOUGHNESS) as f32;
        let factor = 2.0 + toughness / 4.0;
        let effective = mth::clamp_f32(armor - amount / factor, armor * 0.2, 20.0);
        amount *= 1.0 - effective / 25.0;
    }
    if amount <= 0.0 {
        amount = 0.0;
    }
    // `LivingEntity.getDamageAfterMagicAbsorb`：女巫覆写了它（魔法 / 荆棘只吃 15%，
    // 自己打自己伤害为 0）。抗性效果和附魔减伤这一期没建模。
    //
    // `LivingEntity.getDamageAfterMagicAbsorb`: the witch override (magic and thorns deal
    // only 15%, self-inflicted damage is zero). Resistance and enchantment reduction are
    // not modelled yet.
    if mob.mob_type == super::MobType::Witch {
        amount = super::witch::damage_after_magic_absorb(source, amount, me.id);
    }
    let before = amount;
    amount = mth::java_max_f32(amount - mob.absorption, 0.0);
    mob.absorption = (mob.absorption - (before - amount)).max(0.0);
    if amount == 0.0 {
        return;
    }
    let max = mob.attributes.value(attribute::MAX_HEALTH) as f32;
    mob.health = mth::clamp_f32(mob.health - amount, 0.0, max);
}

/// `LivingEntity.knockback`。
pub(crate) fn knockback(me: &mut Entity, mob: &mut Mob, strength: f64, dx: f64, dz: f64) {
    let strength = strength * (1.0 - mob.attributes.value(attribute::KNOCKBACK_RESISTANCE));
    if strength <= 0.0 {
        return;
    }
    // 官方在这里置 `needsSync`，留到下一次 `sendChanges`。
    // Vanilla sets `needsSync` here and keeps it until the next `sendChanges`.
    mob.knockback_sync = true;
    let (mut dx, mut dz) = (dx, dz);
    let delta = me.delta;
    while dx * dx + dz * dz < f64::from(1.0e-5_f32) {
        dx = (me.random.next_double() - me.random.next_double()) * 0.01;
        dz = (me.random.next_double() - me.random.next_double()) * 0.01;
    }
    let push = Vec3::new(dx, 0.0, dz).normalize().scale(strength);
    me.delta = Vec3::new(
        delta.x / 2.0 - push.x,
        if me.on_ground {
            mth::java_min(0.4, delta.y / 2.0 + strength)
        } else {
            delta.y
        },
        delta.z / 2.0 - push.z,
    );
}

/// `Mob.doHurtTarget`：ATTACK_DAMAGE 加空手的伤害修正，再走玩家/生物的伤害入口。
/// 玩家不在实体表里，所以命中玩家记成同刻落地的 [`Command::HurtPlayer`]。
///
/// `Mob.doHurtTarget`: ATTACK_DAMAGE through the player / mob hurt path.
pub(crate) fn do_hurt_target(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    target: crate::entity::EntityUid,
) {
    // `Creeper.doHurtTarget` 直接返回真：近战目标只负责追，伤害全靠爆炸。
    // `Creeper.doHurtTarget` just returns true: melee only chases, the blast does the damage.
    if mob.mob_type == super::MobType::Creeper {
        return;
    }
    let raw_damage = mob.attributes.value(attribute::ATTACK_DAMAGE).max(0.0);
    // `Bee.doHurtTarget` 把攻击力先截成整数再转浮点：`(float)(int)getAttributeValue(ATTACK_DAMAGE)`。
    // `Bee.doHurtTarget` truncates attack damage to `int` before casting to `float`.
    let amount = if mob.mob_type == super::MobType::Bee {
        (raw_damage as i32) as f32
    } else {
        raw_damage as f32
    };
    // `Panda.doHurtTarget`：不咬回去的好斗熊猫记下这一口，`PandaHurtByTargetGoal` 会停手。
    // `Panda.doHurtTarget`: a non-aggressive panda records the bite so its goal stops.
    let bites = mob.mob_type == super::MobType::Panda;
    if bites && !mob.panda.is_aggressive() {
        mob.panda.did_bite = true;
    }
    let mut source = DamageSource::environment(if mob.mob_type == super::MobType::Bee {
        DamageKind::Sting
    } else {
        DamageKind::MobAttack
    })
    .with_non_player_cause(true);
    source.attacker = Some(me.id);
    source.direct = Some(me.id);
    source.attacker_uuid = Some(mob.uuid);
    source.attacker_type = Some(mob.mob_type.entity_type());
    source.position = Some(me.position);
    if super::tame::is_tamable(mob.mob_type) && mob.tame.is_tame() {
        source.owner_credit = mob.tame.owner;
    }
    // 玩家在 `for_each_player` 里；查不到（已离岛/死亡）就当作没打中。
    let is_player = {
        let mut found = false;
        ctx.blocks().for_each_player(&mut |player| {
            if player.uid == target && player.alive {
                found = true;
            }
        });
        found
    };
    if is_player {
        let attacker = {
            let mut whole = me.clone();
            whole.kind = crate::entity::EntityKind::Mob(Box::new(mob.clone()));
            whole.split().public
        };
        let hurt = ctx.hurt_player(target, source, amount, me, &attacker);
        // `CaveSpider.doHurtTarget`：打中之后按难度给中毒。
        // `CaveSpider.doHurtTarget`: poison the target after a landed hit.
        if hurt && let Some(effect) = super::bee::poison_instance(mob, ctx.blocks().difficulty()) {
            ctx.emit(
                Command::AddPlayerEffect {
                    target,
                    effect: effect.effect,
                    duration: effect.duration,
                    amplifier: effect.amplifier,
                },
                me,
            );
        }
        // `Zombie.doHurtTarget`：打中了、空手、自己着火时，按区域难度传染火焰：
        // `nextFloat() < f * 0.3` 才 `igniteForSeconds(2 * (int) f)`。
        if hurt
            && mob.mob_type.is_zombie()
            && mob
                .equipment
                .get(mc_bamboo_protocol::packet::play::EquipmentSlot::MainHand)
                .is_empty()
            && me.remaining_fire_ticks > 0
        {
            let difficulty = ctx.blocks().effective_difficulty(me.block_position());
            if me.random.next_float() < difficulty * 0.3 {
                let seconds = 2 * difficulty as i32;
                ctx.emit(
                    Command::IgnitePlayer {
                        target,
                        ticks: mth::floor_f32(seconds as f32 * 20.0),
                    },
                    me,
                );
            }
        }
        // `Husk.doHurtTarget`：打中了、空手时给 `HUNGER`，时长 `140 * (int) f`。
        // `LivingEntity.addEffect(new MobEffectInstance(effect, duration), mob)`：
        // 等级 0、缺省标志，走和洞穴蜘蛛中毒同一个命令。
        if hurt
            && mob.mob_type == super::MobType::Husk
            && mob
                .equipment
                .get(mc_bamboo_protocol::packet::play::EquipmentSlot::MainHand)
                .is_empty()
        {
            let difficulty = ctx.blocks().effective_difficulty(me.block_position());
            ctx.emit(
                Command::AddPlayerEffect {
                    target,
                    effect: mc_bamboo_data::registries::mob_effect::HUNGER,
                    duration: 140 * difficulty as i32,
                    amplifier: 0,
                },
                me,
            );
        }
        // `Mob.playAttackSound` 的熊猫版本：咬中才响。
        // The panda `Mob.playAttackSound`: only on a hit.
        if bites && hurt {
            play_sound(me, mob, "minecraft:entity.panda.bite", 1.0, 1.0);
        }
        // `Bee.doHurtTarget`：只有 `hurtServer` 返回真时才置 `HasStung`、熄怒、播蜇刺声。
        // `Bee.doHurtTarget`: set `HasStung`, stop anger, and play sting sound only on a landed hit.
        if hurt && mob.mob_type == super::MobType::Bee {
            super::bee::after_attack(mob);
            play_sound(me, mob, "minecraft:entity.bee.sting", 1.0, 1.0);
        }
    } else if let Some(other) = ctx.other(target) {
        // `Zombie.killedEntity`：普通难度下僵尸打村民要先抽一次自己的
        // `nextBoolean()`，为真不转化；困难必转、简单不抽也不转。官方在
        // 死亡那一刻按攻击者判定，Bamboo 的受害者在自己的 tick 里结算，
        // 所以在这里预读「目标是不是将要死的村民」，把抽签结果放进伤害源。
        //
        // `Zombie.killedEntity`: on NORMAL a zombie rolls its own `nextBoolean()`
        // before converting a villager; HARD always converts, EASY neither draws
        // nor converts. Vanilla checks the attacker at death time; Bamboo settles
        // death in the victim's tick, so carry the roll in the damage source.
        let lethal_villager = {
            let crate::entity::EntityPublic { kind, .. } = &other;
            matches!(kind, crate::entity::KindPublic::Mob(public)
                if public.mob_type == super::MobType::Villager
                    && public.health > 0.0
                    && public.health <= amount)
        };
        let difficulty = ctx.blocks().difficulty();
        let mut source = source;
        if lethal_villager && mob.mob_type.is_zombie() {
            source.attacker_type = Some(mob.mob_type.entity_type());
            source = source.with_zombie_attacker();
            if difficulty == 2 {
                source = source.with_villager_conversion_roll(me.random.next_boolean());
            }
        }
        ctx.emit(
            Command::HurtMob {
                target,
                source,
                amount,
                on_hit: super::bee::poison_instance(mob, ctx.blocks().difficulty()),
            },
            me,
        );
        if bites {
            play_sound(me, mob, "minecraft:entity.panda.bite", 1.0, 1.0);
        }
        if mob.mob_type == super::MobType::Bee {
            super::bee::after_attack(mob);
            play_sound(me, mob, "minecraft:entity.bee.sting", 1.0, 1.0);
        }
    }
}

/// 落地 `Command::HurtPlayer`（并行驱动的缓冲命令）：能当场落地就当场落地，
/// 否则转成 `Effect::HurtPlayer`，由游戏层在同一刻的效果阶段调正式的 `hurt_player`。
pub(crate) fn apply_hurt_player(
    world: &mut crate::world::World,
    me: &Entity,
    target: crate::entity::EntityUid,
    source: DamageSource,
    amount: f32,
) {
    let attacker = me.clone().split().public;
    let _ = world.hurt_player_from_mob(target, source, amount, Some(&attacker));
}

/// 落地 `Command::TrampleTurtleEgg`：`TurtleEggBlock.destroyEgg` 从「还是不是蛋」开始的部分。
/// `nextInt(chance) == 0` 才 `decreaseEggs`。
///
/// Applies `Command::TrampleTurtleEgg`: the part of `TurtleEggBlock.destroyEgg` from the
/// "is it still an egg" test on; `nextInt(chance) == 0` is required before `decreaseEggs`.
pub(crate) fn apply_trample_turtle_egg(
    world: &mut crate::world::World,
    pos: BlockPos,
    chance: i32,
) {
    let state = world.block_state(pos);
    if !state.is(mc_bamboo_data::blocks::TURTLE_EGG) {
        return;
    }
    if world.random().next_int(chance) != 0 {
        return;
    }
    crate::behaviour::misc::turtle_egg::decrease_eggs(world, pos, state);
}

/// 落地 [`Command::EatBlock`]：重查目标方块仍在原位后再 `destroyBlock(pos, false)` 吃草，
/// 或在下面的草方块上发 2001 事件、以标志 2 换成泥土。
///
/// Applies [`Command::EatBlock`]: re-checks the target block before running
/// `destroyBlock(pos, false)` for a plant, or a 2001 event and a flag-2 swap to
/// dirt for the grass block below.
pub(crate) fn apply_eat_block(world: &mut crate::world::World, pos: BlockPos, grass_below: bool) {
    if !grass_below {
        if world
            .block_state(pos)
            .is_tagged(block_tags::EDIBLE_FOR_SHEEP)
        {
            world.destroy_block(pos, false, crate::world::UPDATE_LIMIT);
        }
        return;
    }
    if !world
        .block_state(pos)
        .is(mc_bamboo_data::blocks::GRASS_BLOCK)
    {
        return;
    }
    world.push_effect(crate::Effect::Destroyed {
        pos,
        state: crate::state::BlockState(mc_bamboo_data::blocks::GRASS_BLOCK),
        except_initiator: false,
    });
    world.set_block(
        pos,
        crate::state::BlockState(mc_bamboo_data::blocks::DIRT),
        2,
    );
}

/// `LivingEntity.die`：掉落交给 [`Command::DropMobLoot`]，经验只在被玩家打过时掉。
fn die(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx, source: DamageSource) {
    if me.removed || mob.dead {
        return;
    }
    mob.dead = true;
    // `AbstractVillager.die` → `stopTrading()`（覆盖普通死亡与僵尸转化分支）。
    // `AbstractVillager.die` → `stopTrading()` (covers regular death and zombie conversion).
    if mob.mob_type == super::MobType::Villager {
        mob.villager.stop_trading();
    }
    if mob.mob_type == super::MobType::Wolf {
        super::wolf::on_death(mob);
    }
    // `Fox.dropAllDeathLoot`：嘴里的东西先掉，不看幼年和 `doMobLoot`。
    // `Fox.dropAllDeathLoot` drops the mouth item first, babies and rules aside.
    if mob.mob_type == super::MobType::Fox {
        super::fox::drop_mouth_item(me, mob, ctx);
    }
    let by_player = mob.last_hurt_by_player_memory_time > 0;
    if mob.mob_type.is_skeleton() {
        mob.events.push(MobEvent::Death);
        super::skeleton_drops::drop_on_death(me, mob, ctx, source, by_player);
        return;
    }
    if mob.mob_type.is_equine() {
        mob.events.push(MobEvent::Death);
        super::equine::drop_on_death(me, mob, ctx, source, by_player);
        return;
    }
    let adult = !mob.is_baby();
    let drops = ctx.blocks().block_game_rules().mob_drops;
    // 村民不是 `Animal`：只有战利品表（官方的 `entities/villager` 是空表），
    // `xpReward` 是 0，没有 `Animal.getBaseExperienceReward` 那次抽数。
    //
    // Villagers are not `Animal`s: only the loot table (vanilla's
    // `entities/villager` is empty), and `xpReward` is 0 with no animal experience draw.
    if mob.mob_type == super::MobType::Villager {
        if mob.villager.sleeping {
            super::villager::stop_sleeping(me, mob, None, ctx);
        }
        // `Villager.die` 在 `super.die` 之前归还全部 POI 票（覆盖普通死亡与僵尸转化）。
        // `Villager.die` releases all POI tickets before `super.die` (covers both
        // regular death and zombie conversion).
        super::villager::release_all_pois(me, mob, ctx);
        // 僵尸杀死村民时官方走 `Zombie.killedEntity` 的转化分支。
        // Vanilla routes a villager killed by a zombie through
        // `Zombie.killedEntity`'s conversion branch.
        if super::conversion::zombie_killed_villager(me, mob, ctx, &source) {
            return;
        }
        mob.events.push(MobEvent::Death);
        if drops {
            ctx.emit(
                Command::DropMobLoot(super::MobDeathLoot::new(me, mob, source)),
                me,
            );
        }
        return;
    }
    if mob.mob_type == super::MobType::SulfurCube {
        // `SulfurCube.getBaseExperienceReward`：幼年 0，成年 `1 + nextInt(2)`。
        // `SulfurCube.getBaseExperienceReward`: 0 for babies, `1 + nextInt(2)` for adults.
        let amount = if by_player && drops && adult {
            1 + me.random.next_int_bounded(2)
        } else {
            0
        };
        mob.events.push(MobEvent::Death);
        if drops {
            ctx.emit(
                Command::DropMobLoot(super::MobDeathLoot::new(me, mob, source)),
                me,
            );
        }
        if amount > 0 {
            ctx.emit(Command::AwardDeathExperience { amount }, me);
        }
        return;
    }
    if mob.mob_type == super::MobType::Slime {
        // `Slime.setSize` 把 `xpReward` 设成大小；史莱姆没有装备加成。
        // `Slime.setSize` sets `xpReward` to the size; slimes carry no equipment bonus.
        mob.events.push(MobEvent::Death);
        if drops {
            ctx.emit(
                Command::DropMobLoot(super::MobDeathLoot::new(me, mob, source)),
                me,
            );
        }
        if by_player && drops {
            let amount = mob.cube.size;
            ctx.emit(Command::AwardDeathExperience { amount }, me);
        }
        return;
    }
    // 幻翼不是 `Monster`，但经验是构造函数里固定的 5，走下面怪物那一支。
    // Phantoms are not `Monster`s, yet their fixed 5 experience takes the monster branch.
    if !mob.mob_type.is_monster() && mob.mob_type != super::MobType::Phantom {
        mob.events.push(MobEvent::Death);
        // `Tadpole.shouldDropExperience` 为假：不掉经验，也不取那次随机数。蝙蝠的
        // `xpReward` 是 0，`Mob.getBaseExperienceReward` 不取随机数。
        // `Tadpole.shouldDropExperience` is false: no experience and no draw for it. A
        // bat's `xpReward` is 0 and `Mob.getBaseExperienceReward` draws nothing.
        let experience = by_player
            && adult
            && drops
            && !matches!(mob.mob_type, super::MobType::Tadpole | super::MobType::Bat);
        animal_death_drops(me, mob, ctx, source, experience, adult && drops);
        return;
    }
    // `Monster.xpReward=5`，幼年 `Zombie.getBaseExperienceReward` 乘 2.5。
    let base = if adult {
        mob.mob_type.xp_reward()
    } else {
        (f64::from(mob.mob_type.xp_reward()) * 2.5) as i32
    };
    let experience = if by_player && drops { base } else { 0 };
    mob.events.push(MobEvent::Death);
    if drops {
        ctx.emit(
            Command::DropMobLoot(super::MobDeathLoot::new(me, mob, source)),
            me,
        );
    }
    if experience > 0 {
        ctx.emit(Command::AwardDeathExperience { amount: experience }, me);
    }
}

/// 父亲那一份繁殖数据（落地时父亲被拆出来，读不到）。
///
/// The father's breeding data; the father is detached when the command lands.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BreedFather {
    pub(crate) kind: super::MobType,
    pub(crate) variant: super::CowVariant,
    pub(crate) wool: u8,
    pub(crate) rabbit: super::rabbit::Variant,
    pub(crate) fox: super::fox::Variant,
    pub(crate) love: Option<[u8; 16]>,
    pub(crate) wolf: super::wolf::Wolf,
    pub(crate) tame: super::tame::Tamable,
    pub(crate) cat: super::cat::Cat,
    /// 山羊父亲是否尖叫。
    ///
    /// Whether the goat father is screaming.
    pub(crate) screaming: bool,
    /// 熊猫父亲的显性 / 隐性基因。
    ///
    /// The panda father's main and hidden genes.
    pub(crate) genes: Option<(u8, u8)>,
    /// 美西螈父亲的花色。
    ///
    /// The axolotl father's variant.
    pub(crate) axolotl: i32,
}
/// `LivingEntity.dropAllDeathLoot` 的动物分支：战利品表、`Mob.dropCustomDeathLoot`
/// 按槽位顺序掉装备（鞍），最后 `Animal.getBaseExperienceReward` 取 `1 + nextInt(3)`。
///
/// The animal branch of `dropAllDeathLoot`: loot table, then equipment slots in ordinal
/// order (the saddle), then `Animal.getBaseExperienceReward` draws `1 + nextInt(3)`.
fn animal_death_drops(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    source: DamageSource,
    experience: bool,
    loot: bool,
) {
    let by_player = mob.last_hurt_by_player_memory_time > 0;
    if loot {
        ctx.emit(
            Command::DropMobLoot(super::MobDeathLoot::new(me, mob, source)),
            me,
        );
        let looting = if source.attacker_is_player {
            source.looting_level
        } else {
            0
        };
        for slot in super::equipment::SLOTS {
            if let Some(stack) = super::skeleton_drops::drop_slot(
                &mut mob.equipment,
                slot,
                by_player,
                looting,
                &mut me.random,
            ) {
                ctx.emit(Command::DropItem { stack }, me);
            }
        }
    }
    if experience {
        let amount = 1 + me.random.next_int_bounded(3);
        ctx.emit(Command::AwardDeathExperience { amount }, me);
    }
}

/// [`Command::BreedAnimals`] 的落地：`Animal.spawnChildFromBreeding` 和
/// `finalizeSpawnChildFromBreeding`。父母的年龄设成 6000、求爱清零，父亲发爱心事件，
/// 按 `mob_drops` 生成一颗 `1 + random.nextInt(7)` 的经验球。
///
/// Breeding: spawn the calf, set both parents' age to 6000, reset their love,
/// broadcast hearts on this parent and award one experience orb.
pub(crate) fn apply_breed(
    me: &mut Entity,
    partner: crate::entity::EntityUid,
    position: Vec3,
    parent: BreedFather,
    lover: Option<[u8; 16]>,
    world: &mut crate::world::World,
) {
    let kind = parent.kind;
    // `Turtle.TurtleBreedGoal`：不生成幼崽，给母亲挂一颗蛋。
    // `Turtle.TurtleBreedGoal`: the mother gets an egg instead of a calf.
    if super::turtle::is_turtle(kind) {
        super::turtle::breed(me, partner, position, world);
        return;
    }
    // `Frog.spawnChildFromBreeding`：只有 `finalizeSpawnChildFromBreeding` 那一半。
    // `Frog.spawnChildFromBreeding`: only the `finalizeSpawnChildFromBreeding` half.
    if kind == super::MobType::Frog {
        super::frog::breed(me, partner, position, world);
        return;
    }
    // `Sniffer.spawnChildFromBreeding`：不产幼崽，下一颗蛋。
    // `Sniffer.spawnChildFromBreeding`: no calf — it lays an egg.
    if kind == super::MobType::Sniffer {
        super::sniffer::breed(me, partner, position, world);
        return;
    }
    let mother = world.entity(partner).and_then(|entity| {
        entity
            .kind
            .mob()
            .map(|mob| (mob.rabbit.variant, mob.fox.variant, mob.love_cause))
    });
    let mother_wolf = world
        .entity(partner)
        .and_then(|entity| entity.kind.mob().map(|mob| mob.wolf));
    let mother_cat = world
        .entity(partner)
        .and_then(|entity| entity.kind.mob().map(|mob| mob.cat));
    let mother_axolotl = world
        .entity(partner)
        .and_then(|entity| entity.kind.mob().map(|mob| mob.axolotl.variant));
    let mut rabbit_variant = super::rabbit::Variant::default();
    let mut fox = None;
    let mut wolf = None;
    let mut cat = None;
    // 哞菇的品种自己抽（`getOffspringVariant`），不取牛那个 `nextBoolean`。
    // Mooshrooms draw their own type (`getOffspringVariant`) instead of the cow `nextBoolean`.
    let calf_brown = (kind == super::MobType::Mooshroom).then(|| {
        let mine = parent.wool != 0;
        let mut mate = false;
        world.with_entity_mut(partner, &mut |entity| {
            mate = entity.kind.mob().is_some_and(|mob| mob.mooshroom.brown);
        });
        super::mooshroom::offspring_brown(mine, mate, &mut me.random)
    });
    // 马科的 `getBreedOffspring` 自己取数（属性遗传和花色），也不取 `nextBoolean`。
    // Equine `getBreedOffspring` makes its own draws instead of `nextBoolean`.
    let foal = if kind.is_equine() {
        let Some(first) = me.kind.mob() else {
            return;
        };
        let Some(second) = world.entity(partner) else {
            return;
        };
        let Some(second) = second.kind.mob() else {
            return;
        };
        let Some(child) = super::equine::offspring(&mut me.random, first, second, position) else {
            return;
        };
        Some(child)
    } else {
        None
    };
    // 熊猫的基因在 `getBreedOffspring` 里抽：父 / 母各抽一次 `nextBoolean`，幼崽自己抽
    // 初始的 `nextBoolean` 和两次 1/32 的变异。
    // Panda genes are drawn in `getBreedOffspring`: one `nextBoolean` per parent, plus the
    // calf's own initial `nextBoolean` and two 1/32 mutations.
    let panda_genes = if kind == super::MobType::Panda {
        let father = parent.genes.map(|(main, hidden)| {
            (
                super::panda::Gene::from_id(i32::from(main)),
                super::panda::Gene::from_id(i32::from(hidden)),
            )
        });
        let mother = world.entity(partner).and_then(|entity| {
            entity.kind.mob().map(|mob| {
                (
                    (mob.panda.main_gene, mob.panda.hidden_gene),
                    entity.random.state(),
                )
            })
        });
        father.zip(mother)
    } else {
        None
    };
    let equine = foal.is_some();
    let sheep = kind == super::MobType::Sheep;
    let mut from_father = true;
    let mut axolotl_variant = None;
    match kind {
        super::MobType::Wolf => {
            wolf = Some(super::wolf::offspring(
                me,
                (&parent.wolf, &parent.tame),
                mother_wolf,
                world,
            ));
        }
        super::MobType::Cat => {
            cat = Some(super::cat::offspring(
                me,
                (&parent.cat, &parent.tame),
                mother_cat,
                world,
            ));
        }
        super::MobType::Rabbit => {
            let mother = mother.map_or(parent.rabbit, |(rabbit, _, _)| rabbit);
            rabbit_variant = super::rabbit::offspring_variant(me, parent.rabbit, mother, world);
        }
        super::MobType::Fox => {
            let (variant, love) = mother.map_or((parent.fox, None), |(_, fox, love)| (fox, love));
            fox = Some(super::fox::offspring(
                me,
                parent.fox,
                variant,
                [parent.love, love],
            ));
        }
        // 这些种类都不取父亲的 `nextBoolean`：羊按配方混色，熊猫 / 哞菇 / 犰狳 / 山羊
        // 的品种或基因在别处抽。
        // None of these draw the father's `nextBoolean`: sheep mix colours, and the
        // panda / mooshroom / armadillo / goat traits are drawn elsewhere.
        super::MobType::Sheep
        | super::MobType::Panda
        | super::MobType::Mooshroom
        | super::MobType::Ocelot
        | super::MobType::Armadillo
        | super::MobType::Goat => {}
        // 美西螈先赌 1/1200 的稀有蓝，没中才用 `nextBoolean` 取父母之一。
        // The axolotl rolls 1/1200 for the rare blue first, then one `nextBoolean`
        // picks a parent.
        super::MobType::Axolotl => {
            axolotl_variant = Some(super::axolotl::offspring_variant(
                &mut me.random,
                parent.axolotl,
                mother_axolotl.unwrap_or(super::axolotl::DEFAULT_VARIANT),
            ));
        }
        _ if equine => {}
        _ => {
            // 牛、猪、鸡用父亲的随机源 `nextBoolean` 选花纹。
            // Cows, pigs and chickens pick the coat with the parent's `nextBoolean`.
            from_father = me.random.next_boolean();
        }
    }
    let mut child_variant = parent.variant;
    let mut partner_wool = parent.wool;
    let mut partner_screaming = false;
    world.with_entity_mut(partner, &mut |entity| {
        entity.with_mob(|me, mob| {
            if !from_father {
                child_variant = mob.variant;
            }
            partner_wool = mob.wool;
            partner_screaming = mob.goat.screaming;
            mob.in_love = 0;
            set_age(me, mob, 6000);
        });
    });
    // 官方先构造幼崽（拿到 ID）再构造经验球；岛种子源和网络 ID 都按这个顺序。
    let lamb_color = sheep.then(|| {
        super::sheep::offspring_color(
            mc_bamboo_core::sign::DyeColor::from_id(i32::from(parent.wool & 15)),
            mc_bamboo_core::sign::DyeColor::from_id(i32::from(partner_wool & 15)),
            world.random(),
        )
    });
    // `finalizeSpawnChildFromBreeding` 只在 `MOB_DROPS` 开着时取经验值；它在
    // `getBreedOffspring` 之后，所以熊猫的父母基因取数排在它前面。
    // `finalizeSpawnChildFromBreeding` draws the orb value only with `MOB_DROPS`, and
    // after `getBreedOffspring`, so the panda parent gene draws come first.
    let mob_drops = world.block_game_rules().mob_drops;
    let mut calf = foal.unwrap_or_else(|| Entity::by_mob_type(kind, position));
    // 熊猫幼崽要先拿到 ID 和随机种子，基因才是从它自己的随机源抽的。
    // A panda calf takes its id and random seed first, so the genes come from its own random.
    let panda_prepared = panda_genes.is_some();
    if panda_prepared {
        calf = world.prepare_entity(calf);
        if let Some((father, (mother, mother_state))) = panda_genes {
            let mut mother_random = crate::entity::JavaRandom::from_state(mother_state);
            let genes = super::panda::offspring_genes(
                &mut calf,
                &mut me.random,
                father,
                &mut mother_random,
                mother,
            );
            let new_state = mother_random.state();
            world.with_entity_mut(partner, &mut |entity| {
                entity.random = crate::entity::JavaRandom::from_state(new_state);
            });
            calf.with_mob(|_me, mob| {
                mob.panda.main_gene = genes.0;
                mob.panda.hidden_gene = genes.1;
                super::panda::set_attributes(mob);
            });
        }
    }
    let xp = if mob_drops {
        1 + me.random.next_int_bounded(7)
    } else {
        0
    };
    if kind == super::MobType::Goat {
        // 父亲正被拆出来跑 tick，尖叫标志由命令带过来。
        // The father is detached for its tick; its screaming flag rides on the command.
        super::goat::breed_offspring(&mut calf, parent.screaming, partner_screaming, world);
    }
    // `setBaby(true)` 在 `addFreshEntity` 之前设好年龄和尺寸；`snapTo` 只清 yRot，
    // 构造时的 `yHeadRot` 保留。
    calf.y_rot = 0.0;
    calf.with_mob(|me, mob| {
        mob.rabbit.variant = rabbit_variant;
        if let Some(fox) = fox.take() {
            mob.fox = fox;
        }
        if let Some((wolf, tamable)) = wolf.take() {
            mob.wolf = wolf;
            mob.tame = tamable;
            if tamable.is_tame() {
                super::tame::set_tame(mob, true, true);
            }
        }
        if let Some((cat, tamable)) = cat.take() {
            mob.cat = cat;
            mob.tame = tamable;
            if tamable.is_tame() {
                super::tame::set_tame(mob, true, true);
            }
        }
        set_age(me, mob, -24000);
        match lamb_color {
            Some(color) => super::sheep::set_color(mob, color),
            None => mob.variant = child_variant,
        }
        if let Some(brown) = calf_brown {
            mob.mooshroom.brown = brown;
        }
        if let Some(variant) = axolotl_variant {
            mob.axolotl.variant = variant;
            // `Axolotl.getBreedOffspring` 的 `baby.setPersistenceRequired()`。
            // `baby.setPersistenceRequired()` from `Axolotl.getBreedOffspring`.
            mob.persistence_required = true;
        }
        mob.y_rot_o = 0.0;
        mob.y_body_rot = 0.0;
        mob.y_body_rot_o = 0.0;
    });
    let calf_id = if panda_prepared {
        world.add_prepared_entity(calf)
    } else {
        world.spawn_entity(calf)
    };
    // `finish_mob_random` 在分配 ID 之后重取了朝向；`snapTo` 把 yRot 清成 0，
    // 但构造时的 yHeadRot 要保留。
    world.with_entity_mut(calf_id, &mut |entity| {
        entity.y_rot = 0.0;
    });
    // `Animal.finalizeSpawnChildFromBreeding`：给求爱者发 BRED_ANIMALS，
    // 三个实体分别是父亲、母亲、幼崽（种类相同）。
    world.push_effect(crate::level::Effect::BredAnimals {
        father: kind.name(),
        mother: kind.name(),
        child: kind.name(),
        lover,
        position,
    });
    if mob_drops {
        // 繁殖不是 `ExperienceOrb.award`：只生成一颗球，值就是 `1 + nextInt(7)`。
        let orb = Entity::experience_orb(position, xp);
        world.spawn_entity(orb);
    }
}

/// `FluidTracker` 的构造，给 [`super::motion`] 用。
pub(crate) fn tracker_for(trackers: &mut Vec<FluidTracker>, fluid: i32) -> usize {
    if let Some(index) = trackers.iter().position(|tracker| tracker.fluid == fluid) {
        return index;
    }
    trackers.push(FluidTracker {
        fluid,
        height: 0.0,
        eyes_inside: false,
    });
    trackers.len() - 1
}

/// [`Command::PushMob`] 的落地：`other.push(me)`，对方先被推开，自己再被推回来。
pub(crate) fn apply_push(
    me: &mut Entity,
    other: crate::entity::EntityUid,
    from: Vec3,
    self_pushable: bool,
    world: &mut crate::world::World,
) {
    let _ = from;
    let Some(target) = world.entity(other) else {
        return;
    };
    // `LivingEntity.push(Entity)`：睡着的（狐狸、村民）谁也推不动，也不把人推回去。
    // `LivingEntity.push(Entity)`: a sleeper (fox or villager) is not pushed and pushes nobody back.
    if matches!(
        &target.kind,
        EntityKind::Mob(mob)
            if (mob.mob_type == super::MobType::Fox && mob.fox.is_sleeping())
                || (mob.mob_type == super::MobType::Villager && mob.villager.sleeping)
    ) {
        return;
    }
    let Some((dx, dz)) = push_amount(target.position, me.position) else {
        return;
    };
    let other_pushable =
        matches!(&target.kind, EntityKind::Mob(mob) if mob.health > 0.0) && !target.removed;
    if other_pushable {
        world.with_entity_mut(other, &mut |entity| {
            entity.delta = entity.delta.add(Vec3::new(-dx, 0.0, -dz));
            // 被推开的一方和 `Entity.push(Entity)` 一样置 `needsSync`。
            // The pushed side sets `needsSync` as in `Entity.push(Entity)`.
            entity.with_mob(|_, mob| mob.needs_sync = true);
        });
    }
    if self_pushable {
        me.delta = me.delta.add(Vec3::new(dx, 0.0, dz));
        me.with_mob(|_, mob| mob.needs_sync = true);
    }
}
