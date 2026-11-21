//! 狐狸专属的目标：`Fox` 里的内部类，外加它用到的 `FleeSunGoal`、
//! `StrollThroughVillageGoal` 和 `MoveToBlockGoal` 子类。
//!
//! The fox-only goals: the inner classes of `Fox`, plus the `FleeSunGoal`,
//! `StrollThroughVillageGoal` and `MoveToBlockGoal` subclasses it uses.

use mc_bamboo_core::{BlockPos, Vec3, mth};
use mc_bamboo_data::{blocks, items};
use mc_bamboo_protocol::{item::ItemStack, packet::play::EquipmentSlot};

use super::{
    Mob, MobType,
    fox::{
        FLAG_CROUCHING, FLAG_FACEPLANTED, FLAG_INTERESTED, FLAG_POUNCING, FLAG_SITTING,
        FLAG_SLEEPING,
    },
    goals::{self, CombatTarget, Flag, reduced_tick_delay, within_home},
    living,
};
use crate::{
    entity::{
        Entity, EntityUid,
        ctx::{BlockView, Command, EntityCtx},
        store::KindPublic,
    },
    state::AGE_3,
};

/// 狐狸自己的目标。
///
/// The fox's own goals.
#[derive(Debug, Clone, PartialEq)]
pub enum FoxGoal {
    /// `FoxFloatGoal`。
    Float,
    /// `FaceplantGoal`。
    Faceplant { countdown: i32 },
    /// `StalkPreyGoal`。
    StalkPrey,
    /// `FoxPounceGoal`。
    Pounce,
    /// `SeekShelterGoal`（`FleeSunGoal`，速度 1.25）。
    SeekShelter { interval: i32, wanted: Vec3 },
    /// `SleepGoal`。
    Sleep { countdown: i32 },
    /// `FoxStrollThroughVillageGoal`（间隔 200）。
    StrollThroughVillage,
    /// `FoxEatBerriesGoal`（`MoveToBlockGoal`，速度 1.2F，水平 12、竖直 1）。
    EatBerries(EatBerries),
    /// `FoxSearchForItemsGoal`。
    SearchForItems,
    /// `PerchAndSearchGoal`。
    Perch(Perch),
    /// `DefendTrustedTargetGoal`。
    DefendTrusted { timestamp: i32 },
}

/// `MoveToBlockGoal` 的字段加 `ticksWaited`。
#[derive(Debug, Clone, PartialEq)]
pub struct EatBerries {
    next_start_tick: i32,
    try_ticks: i32,
    max_stay_ticks: i32,
    block_pos: BlockPos,
    reached_target: bool,
    ticks_waited: i32,
}

/// `PerchAndSearchGoal` 的字段。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Perch {
    rel_x: f64,
    rel_z: f64,
    look_time: i32,
    looks_remaining: i32,
}

impl FoxGoal {
    pub(super) const fn seek_shelter() -> Self {
        Self::SeekShelter {
            interval: reduced_tick_delay(100),
            wanted: Vec3::new(0.0, 0.0, 0.0),
        }
    }

    pub(super) const fn eat_berries() -> Self {
        Self::EatBerries(EatBerries {
            next_start_tick: 0,
            try_ticks: 0,
            max_stay_ticks: 0,
            block_pos: BlockPos::new(0, 0, 0),
            reached_target: false,
            ticks_waited: 0,
        })
    }

    pub(super) const fn perch() -> Self {
        Self::Perch(Perch {
            rel_x: 0.0,
            rel_z: 0.0,
            look_time: 0,
            looks_remaining: 0,
        })
    }

    pub(super) const fn flags(&self) -> &'static [Flag] {
        match self {
            Self::Float => &[Flag::Jump],
            Self::Faceplant { .. } | Self::Sleep { .. } => &[Flag::Move, Flag::Look, Flag::Jump],
            Self::StalkPrey | Self::Perch(_) => &[Flag::Move, Flag::Look],
            Self::Pounce | Self::EatBerries(_) => &[Flag::Move, Flag::Jump],
            Self::SeekShelter { .. } | Self::StrollThroughVillage | Self::SearchForItems => {
                &[Flag::Move]
            }
            Self::DefendTrusted { .. } => &[Flag::Target],
        }
    }

    pub(super) const fn name(&self) -> &'static str {
        match self {
            Self::Float => "FoxFloatGoal",
            Self::Faceplant { .. } => "FaceplantGoal",
            Self::StalkPrey => "StalkPreyGoal",
            Self::Pounce => "FoxPounceGoal",
            Self::SeekShelter { .. } => "SeekShelterGoal",
            Self::Sleep { .. } => "SleepGoal",
            Self::StrollThroughVillage => "FoxStrollThroughVillageGoal",
            Self::EatBerries(_) => "FoxEatBerriesGoal",
            Self::SearchForItems => "FoxSearchForItemsGoal",
            Self::Perch(_) => "PerchAndSearchGoal",
            Self::DefendTrusted { .. } => "DefendTrustedTargetGoal",
        }
    }

    /// `requiresUpdateEveryTick`。
    pub(super) const fn every_tick(&self) -> bool {
        matches!(self, Self::Float | Self::EatBerries(_))
    }

    /// `isInterruptable`：只有扑跳不能被打断。
    pub(super) const fn interruptable(&self) -> bool {
        !matches!(self, Self::Pounce)
    }

    pub(super) fn can_use(
        &mut self,
        me: &mut Entity,
        mob: &mut Mob,
        ctx: &mut dyn EntityCtx,
    ) -> bool {
        match self {
            Self::Float => float_can_use(me, mob),
            Self::Faceplant { .. } => mob.fox.has(FLAG_FACEPLANTED),
            Self::StalkPrey => stalk_can_use(me, mob, ctx),
            Self::Pounce => pounce_can_use(me, mob, ctx),
            Self::SeekShelter { interval, wanted } => {
                shelter_can_use(me, mob, ctx, interval, wanted)
            }
            Self::Sleep { countdown } => {
                mob.xxa == 0.0
                    && mob.yya == 0.0
                    && mob.zza == 0.0
                    && (can_sleep(me, mob, ctx, countdown) || mob.fox.is_sleeping())
            }
            Self::StrollThroughVillage => {
                // `StrollThroughVillageGoal.canUse`：白天直接拒绝；夜里取一次
                // `nextInt(interval)`，Bamboo 没有村庄 POI，`isCloseToVillage` 恒假。
                // Bamboo has no village POIs, so `isCloseToVillage` is always false.
                if ctx.blocks().is_bright_outside() {
                    return false;
                }
                let _ = me.random.next_int_bounded(reduced_tick_delay(200));
                false
            }
            Self::EatBerries(berries) => {
                !mob.fox.is_sleeping() && berries.can_use(me, mob, ctx.blocks())
            }
            Self::SearchForItems => search_can_use(me, mob, ctx),
            Self::Perch(_) => perch_can_use(me, mob, ctx),
            Self::DefendTrusted { .. } => {
                // `DefendTrustedTargetGoal.canUse`：先取 `nextInt(randomInterval)`。
                // 信任对象的 `getLastHurtByMob` 这一期拿不到（玩家视图里没有），
                // 所以找到信任对象也不会触发保护。
                // The trusted entity's `getLastHurtByMob` is not exposed yet, so
                // a found trusted entity never triggers the defence.
                let _ = me.random.next_int_bounded(reduced_tick_delay(10));
                false
            }
        }
    }

    pub(super) fn can_continue(
        &mut self,
        me: &mut Entity,
        mob: &mut Mob,
        ctx: &mut dyn EntityCtx,
    ) -> bool {
        match self {
            Self::Float | Self::SearchForItems | Self::StalkPrey | Self::DefendTrusted { .. } => {
                self.can_use(me, mob, ctx)
            }
            Self::Faceplant { countdown } => mob.fox.has(FLAG_FACEPLANTED) && *countdown > 0,
            Self::Pounce => pounce_can_continue(me, mob, ctx),
            Self::SeekShelter { .. } => !mob.navigation.is_done(),
            Self::Sleep { countdown } => can_sleep(me, mob, ctx, countdown),
            Self::StrollThroughVillage => false,
            Self::EatBerries(berries) => berries.can_continue(ctx.blocks()),
            Self::Perch(perch) => perch.looks_remaining > 0,
        }
    }

    pub(super) fn start(&mut self, me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
        match self {
            Self::Float => mob.fox.clear_states(),
            Self::Faceplant { countdown } => *countdown = reduced_tick_delay(40),
            Self::StalkPrey => {
                mob.fox.set(FLAG_SITTING, false);
                mob.fox.set(FLAG_FACEPLANTED, false);
            }
            Self::Pounce => pounce_start(me, mob, ctx),
            Self::SeekShelter { wanted, .. } => {
                mob.fox.clear_states();
                let nav = living::nav_mob(me, mob, ctx.blocks());
                mob.navigation.move_to(ctx.blocks(), &nav, *wanted, 1.25);
            }
            Self::Sleep { .. } => {
                for flag in [FLAG_SITTING, FLAG_CROUCHING, FLAG_INTERESTED] {
                    mob.fox.set(flag, false);
                }
                mob.jumping = false;
                mob.fox.set(FLAG_SLEEPING, true);
                mob.navigation.stop();
                mob.mover
                    .set_wanted_position(me.position.x, me.position.y, me.position.z, 0.0);
            }
            Self::StrollThroughVillage | Self::DefendTrusted { .. } => {}
            Self::EatBerries(berries) => {
                berries.ticks_waited = 0;
                mob.fox.set(FLAG_SITTING, false);
                berries.start(me, mob, ctx.blocks());
            }
            Self::SearchForItems => move_to_first_item(me, mob, ctx),
            Self::Perch(perch) => {
                perch.reset_look(me);
                perch.looks_remaining = 2 + me.random.next_int_bounded(3);
                mob.fox.set(FLAG_SITTING, true);
                mob.navigation.stop();
            }
        }
    }

    pub(super) fn stop(&mut self, me: &mut Entity, mob: &mut Mob) {
        match self {
            Self::Faceplant { .. } => mob.fox.set(FLAG_FACEPLANTED, false),
            Self::StalkPrey => {}
            Self::Pounce => {
                mob.fox.set(FLAG_CROUCHING, false);
                mob.fox.crouch_amount = 0.0;
                mob.fox.set(FLAG_INTERESTED, false);
                mob.fox.set(FLAG_POUNCING, false);
            }
            Self::Sleep { countdown } => {
                *countdown = me.random.next_int_bounded(reduced_tick_delay(140));
                mob.fox.clear_states();
            }
            Self::Perch(_) => mob.fox.set(FLAG_SITTING, false),
            Self::DefendTrusted { .. } => mob.target = None,
            Self::Float
            | Self::SeekShelter { .. }
            | Self::StrollThroughVillage
            | Self::EatBerries(_)
            | Self::SearchForItems => {}
        }
    }

    /// `StalkPreyGoal.stop` 要读目标和方块，单独放在这里。
    pub(super) fn stop_with_world(
        &mut self,
        me: &mut Entity,
        mob: &mut Mob,
        ctx: &mut dyn EntityCtx,
    ) {
        if matches!(self, Self::StalkPrey) {
            stalk_stop(me, mob, ctx);
        }
        self.stop(me, mob);
    }

    pub(super) fn tick(&mut self, me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
        match self {
            Self::Float => {
                if me.random.next_float() < 0.8 {
                    mob.jump.jump = true;
                }
            }
            Self::Faceplant { countdown } => *countdown -= 1,
            Self::StalkPrey => stalk_tick(me, mob, ctx),
            Self::Pounce => pounce_tick(me, mob, ctx),
            Self::EatBerries(berries) => berries.tick(me, mob, ctx),
            Self::SearchForItems => {
                if mob.equipment.get(EquipmentSlot::MainHand).is_empty() {
                    move_to_first_item(me, mob, ctx);
                }
            }
            Self::Perch(perch) => {
                perch.look_time -= 1;
                if perch.look_time <= 0 {
                    perch.looks_remaining -= 1;
                    perch.reset_look(me);
                }
                let eye = me.position.y + f64::from(mob.eye_height);
                mob.look.set_look_at(
                    me.position.x + perch.rel_x,
                    eye,
                    me.position.z + perch.rel_z,
                    super::control::MAX_HEAD_Y_ROT as f32,
                    super::control::MAX_HEAD_X_ROT as f32,
                );
            }
            Self::SeekShelter { .. }
            | Self::Sleep { .. }
            | Self::StrollThroughVillage
            | Self::DefendTrusted { .. } => {}
        }
    }
}

// ---- 浮水 ----

/// `FoxFloatGoal.canUse`：`isInFluidDeeperThan(0.25, #entity_floatable)` 或者在岩浆里。
fn float_can_use(me: &Entity, mob: &Mob) -> bool {
    living::fluid_height(mob, mc_bamboo_data::tags::fluid_tags::ENTITY_FLOATABLE) > 0.25
        || living::is_in_lava(me, mob)
}

// ---- 盯梢和扑跳 ----

/// 当前目标和它是不是兔子 / 鸡（`STALKABLE_PREY`）。
fn prey(mob: &Mob, ctx: &dyn EntityCtx) -> Option<(CombatTarget, bool)> {
    let target = goals::combat_target(mob, ctx)?;
    let stalkable = ctx.other(target.uid).is_some_and(|other| {
        matches!(&other.kind, KindPublic::Mob(public) if public.mob_type == MobType::Rabbit)
    });
    Some((target, stalkable))
}

fn stalk_can_use(me: &Entity, mob: &Mob, ctx: &dyn EntityCtx) -> bool {
    if mob.fox.is_sleeping() {
        return false;
    }
    let Some((target, stalkable)) = prey(mob, ctx) else {
        return false;
    };
    stalkable
        && me.position.distance_to_sqr(target.position) > 36.0
        && !mob.fox.has(FLAG_CROUCHING)
        && !mob.fox.has(FLAG_INTERESTED)
        && !mob.jumping
}

fn look_at_target(mob: &mut Mob, target: CombatTarget, y_max: f32, x_max: f32) {
    mob.look.set_look_at(
        target.position.x,
        target.eye_y(),
        target.position.z,
        y_max,
        x_max,
    );
}

fn stalk_stop(me: &Entity, mob: &mut Mob, ctx: &dyn EntityCtx) {
    let target = goals::combat_target(mob, ctx);
    if let Some(target) = target
        && super::fox::is_path_clear(me, ctx.blocks(), target.position)
    {
        mob.fox.set(FLAG_INTERESTED, true);
        mob.fox.set(FLAG_CROUCHING, true);
        mob.navigation.stop();
        look_at_target(mob, target, 75.0, 40.0);
    } else {
        mob.fox.set(FLAG_INTERESTED, false);
        mob.fox.set(FLAG_CROUCHING, false);
    }
}

fn stalk_tick(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    let Some(target) = goals::combat_target(mob, ctx) else {
        return;
    };
    look_at_target(mob, target, 75.0, 40.0);
    if me.position.distance_to_sqr(target.position) <= 36.0 {
        mob.fox.set(FLAG_INTERESTED, true);
        mob.fox.set(FLAG_CROUCHING, true);
        mob.navigation.stop();
    } else {
        let nav = living::nav_mob(me, mob, ctx.blocks());
        mob.navigation
            .move_to_entity(ctx.blocks(), &nav, target.position, 1.5, 1);
    }
}

fn pounce_can_use(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) -> bool {
    if !mob.fox.is_fully_crouched() {
        return false;
    }
    // `getMotionDirection()` 就是 `getDirection()`，这一条恒真。
    // `getMotionDirection()` is `getDirection()`, so that check always passes.
    let Some(target) = goals::combat_target(mob, ctx) else {
        return false;
    };
    let clear = super::fox::is_path_clear(me, ctx.blocks(), target.position);
    if !clear {
        let nav = living::nav_mob(me, mob, ctx.blocks());
        let _ = mob.navigation.create_path(
            ctx.blocks(),
            &nav,
            BlockPos::containing(target.position),
            0,
        );
        mob.fox.set(FLAG_CROUCHING, false);
        mob.fox.set(FLAG_INTERESTED, false);
    }
    clear
}

fn pounce_can_continue(me: &Entity, mob: &Mob, ctx: &dyn EntityCtx) -> bool {
    if goals::combat_target(mob, ctx).is_none() {
        return false;
    }
    let yd = me.delta.y;
    // 官方写成 `!(a < b)`，NaN 时为真。
    // Vanilla writes `!(a < b)`, which is true on NaN.
    let not_less = |a: f64, b: f64| !matches!(a.partial_cmp(&b), Some(std::cmp::Ordering::Less));
    (not_less(yd * yd, f64::from(0.05_f32))
        || not_less(f64::from(mob.x_rot.abs()), 15.0)
        || !me.on_ground)
        && !mob.fox.has(FLAG_FACEPLANTED)
}

fn pounce_start(me: &mut Entity, mob: &mut Mob, ctx: &dyn EntityCtx) {
    mob.jumping = true;
    mob.fox.set(FLAG_POUNCING, true);
    mob.fox.set(FLAG_INTERESTED, false);
    if let Some(target) = goals::combat_target(mob, ctx) {
        look_at_target(mob, target, 60.0, 30.0);
        let towards = Vec3::new(
            target.position.x - me.position.x,
            target.position.y - me.position.y,
            target.position.z - me.position.z,
        )
        .normalize();
        me.delta = me
            .delta
            .add(Vec3::new(towards.x * 0.8, 0.9, towards.z * 0.8));
    }
    mob.navigation.stop();
}

fn pounce_tick(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    let target = goals::combat_target(mob, ctx);
    if let Some(target) = target {
        look_at_target(mob, target, 60.0, 30.0);
    }
    if !mob.fox.has(FLAG_FACEPLANTED) {
        let movement = me.delta;
        if movement.y * movement.y < f64::from(0.03_f32) && mob.x_rot != 0.0 {
            // `Mth.rotLerp(0.2F, xRot, 0.0F)`。
            let rotated = mob.x_rot + 0.2 * mth::wrap_degrees(0.0 - mob.x_rot);
            mob.x_rot = super::control::clamp_x_rot(rotated, mob.x_rot);
        } else {
            let direction = (movement.x * movement.x + movement.z * movement.z).sqrt();
            let bias = if mob.jumping && movement.y > 0.0 {
                6.5_f32
            } else {
                1.0
            };
            let biased_y = movement.y * f64::from(bias);
            let len = (direction * direction + biased_y * biased_y).sqrt();
            if len > f64::from(1.0e-5_f32) {
                // `180.0F / (float) Math.PI` 在编译期折成 float 常量 57.29578F。
                // `180.0F / (float) Math.PI` folds to the float constant 57.29578F.
                let rotation =
                    java_signum(-biased_y) * (direction / len).acos() * 57.295_776_367_187_5;
                mob.x_rot = super::control::clamp_x_rot(rotation as f32, mob.x_rot);
            }
        }
    }
    let distance = target.map(|target| me.position.distance_to_sqr(target.position).sqrt());
    if let (Some(target), Some(distance)) = (target, distance)
        && distance as f32 <= 2.0
    {
        living::do_hurt_target(me, mob, ctx, target.uid);
    } else if mob.x_rot > 0.0
        && me.on_ground
        && me.delta.y as f32 != 0.0
        && ctx
            .blocks()
            .block_state(me.block_position())
            .is(blocks::SNOW)
    {
        mob.x_rot = 60.0;
        mob.target = None;
        super::fox::after_target_change(mob);
        mob.fox.set(FLAG_FACEPLANTED, true);
    }
}

/// `Math.signum`：零和 NaN 原样返回（保留 -0.0）。
///
/// `Math.signum`: zero and NaN come back unchanged, keeping -0.0.
fn java_signum(value: f64) -> f64 {
    if value == 0.0 || value.is_nan() {
        value
    } else {
        value.signum()
    }
}

// ---- 找掩护和睡觉 ----

/// `FoxBehaviorGoal.hasShelter`。
fn has_shelter(me: &Entity, mob: &Mob, view: &dyn BlockView) -> bool {
    let top = me.position.y + f64::from(mob.dimensions().1);
    let pos = BlockPos::containing(Vec3::new(me.position.x, top, me.position.z));
    !view.can_see_sky(pos) && goals::walk_target_value(mob, view, pos) >= 0.0
}

/// `FoxBehaviorGoal.alertable`：12 格（竖直 6 格）内有让狐狸警觉的活物。
fn alertable(me: &Entity, mob: &Mob, ctx: &dyn EntityCtx) -> bool {
    let area = me.bounding_box().inflate(12.0, 6.0, 12.0);
    let view = ctx.blocks();
    let mut found = false;
    view.for_each_player(&mut |player| {
        if found || player.spectator || player.creative || !player.alive {
            return;
        }
        if view.difficulty() == 0 || mob.fox.trusts(player.uuid) || player.sneaking {
            return;
        }
        if !super::avoid::player_box(player).intersects(area) {
            return;
        }
        let mut visibility = 1.0_f64;
        if player.invisible {
            visibility *= 0.7 * f64::from(mth::java_max_f32(player.armor_cover, 0.1));
        }
        let limit = mth::java_max(12.0 * visibility, 2.0);
        found = me.position.distance_to_sqr(player.position) <= limit * limit;
    });
    if found {
        return true;
    }
    ctx.for_each_other_in(area, &mut |other| {
        let KindPublic::Mob(public) = &other.kind else {
            return;
        };
        if found || other.uid == me.id || other.removed || !public.is_alive() {
            return;
        }
        // `FoxAlertableEntitiesSelector`：别的狐狸不算；其余生物不睡觉、不潜行就算。
        // Other foxes never count; every other mob that is awake does.
        if public.mob_type == MobType::Fox {
            return;
        }
        found = me.position.distance_to_sqr(other.position) <= 144.0;
    });
    found
}

/// `SleepGoal.canSleep`：倒计时没到就减一。
fn can_sleep(me: &Entity, mob: &Mob, ctx: &dyn EntityCtx, countdown: &mut i32) -> bool {
    if *countdown > 0 {
        *countdown -= 1;
        return false;
    }
    ctx.blocks().is_bright_outside()
        && has_shelter(me, mob, ctx.blocks())
        && !alertable(me, mob, ctx)
        && !mob.in_powder_snow
}

/// `SeekShelterGoal.canUse`。
fn shelter_can_use(
    me: &mut Entity,
    mob: &Mob,
    ctx: &dyn EntityCtx,
    interval: &mut i32,
    wanted: &mut Vec3,
) -> bool {
    if mob.fox.is_sleeping() || mob.target.is_some() {
        return false;
    }
    let view = ctx.blocks();
    if view.is_thundering() && view.can_see_sky(me.block_position()) {
        return set_wanted_pos(me, mob, view, wanted);
    }
    if *interval > 0 {
        *interval -= 1;
        return false;
    }
    *interval = 100;
    // `ServerLevel.isVillage(pos)`：没有村庄 POI，恒假。
    // `ServerLevel.isVillage(pos)` is always false without village POIs.
    view.is_bright_outside()
        && view.can_see_sky(me.block_position())
        && set_wanted_pos(me, mob, view, wanted)
}

/// `FleeSunGoal.setWantedPos` → `getHidePos`：10 次采样，用狐狸（`Animal`）的路径权重。
fn set_wanted_pos(me: &mut Entity, mob: &Mob, view: &dyn BlockView, wanted: &mut Vec3) -> bool {
    let centre = me.block_position();
    for _ in 0..10 {
        let x = me.random.next_int_bounded(20) - 10;
        let y = me.random.next_int_bounded(6) - 3;
        let z = me.random.next_int_bounded(20) - 10;
        let pos = centre.offset(x, y, z);
        if !view.can_see_sky(pos) && goals::walk_target_value(mob, view, pos) < 0.0 {
            *wanted = Vec3::new(
                f64::from(pos.x) + 0.5,
                f64::from(pos.y),
                f64::from(pos.z) + 0.5,
            );
            return true;
        }
    }
    false
}

// ---- 找东西 ----

/// 8 格内能捡的物品（`ALLOWED_ITEMS`：没有拾取延迟、还活着）。
fn items_nearby(me: &Entity, ctx: &dyn EntityCtx) -> Vec<(EntityUid, Vec3)> {
    let area = me.bounding_box().inflate(8.0, 8.0, 8.0);
    let mut found = Vec::new();
    ctx.for_each_other_in(area, &mut |other| {
        if let KindPublic::Item { pickup_delay, .. } = &other.kind
            && *pickup_delay <= 0
            && !other.removed
        {
            found.push((other.uid, other.position));
        }
    });
    found
}

fn search_can_use(me: &mut Entity, mob: &Mob, ctx: &dyn EntityCtx) -> bool {
    if !mob.equipment.get(EquipmentSlot::MainHand).is_empty() {
        return false;
    }
    if mob.target.is_some() || mob.last_hurt_by_mob.is_some() {
        return false;
    }
    if !mob.fox.can_move() {
        return false;
    }
    if me.random.next_int_bounded(reduced_tick_delay(10)) != 0 {
        return false;
    }
    !items_nearby(me, ctx).is_empty()
}

fn move_to_first_item(me: &Entity, mob: &mut Mob, ctx: &dyn EntityCtx) {
    if let Some((_, position)) = items_nearby(me, ctx).first() {
        let nav = living::nav_mob(me, mob, ctx.blocks());
        mob.navigation
            .move_to_entity(ctx.blocks(), &nav, *position, f64::from(1.2_f32), 1);
    }
}

// ---- 坐着四处看 ----

fn perch_can_use(me: &mut Entity, mob: &Mob, ctx: &dyn EntityCtx) -> bool {
    mob.last_hurt_by_mob.is_none()
        && me.random.next_float() < 0.02
        && !mob.fox.is_sleeping()
        && mob.target.is_none()
        && mob.navigation.is_done()
        && !alertable(me, mob, ctx)
        && !mob.fox.has(FLAG_POUNCING)
        && !mob.fox.has(FLAG_CROUCHING)
}

impl Perch {
    /// `PerchAndSearchGoal.resetLook`：`adjustedTickDelay` 在非每刻目标上折半。
    fn reset_look(&mut self, me: &mut Entity) {
        let angle = std::f64::consts::TAU * me.random.next_double();
        self.rel_x = angle.cos();
        self.rel_z = angle.sin();
        self.look_time = reduced_tick_delay(80 + me.random.next_int_bounded(20));
    }
}

// ---- 吃浆果 ----

impl EatBerries {
    /// `MoveToBlockGoal.canUse`。
    fn can_use(&mut self, me: &mut Entity, mob: &Mob, view: &dyn BlockView) -> bool {
        if self.next_start_tick > 0 {
            self.next_start_tick -= 1;
            return false;
        }
        self.next_start_tick = reduced_tick_delay(200 + me.random.next_int_bounded(200));
        match find_berries(me.block_position(), mob, view) {
            Some(pos) => {
                self.block_pos = pos;
                true
            }
            None => false,
        }
    }

    /// `MoveToBlockGoal.canContinueToUse`。
    fn can_continue(&self, view: &dyn BlockView) -> bool {
        self.try_ticks >= -self.max_stay_ticks
            && self.try_ticks <= 1200
            && is_berry_target(view, self.block_pos)
    }

    /// `MoveToBlockGoal.start`。
    fn start(&mut self, me: &mut Entity, mob: &mut Mob, view: &dyn BlockView) {
        self.move_to(me, mob, view, self.block_pos.above());
        self.try_ticks = 0;
        let bound = me.random.next_int_bounded(1200) + 1200;
        self.max_stay_ticks = me.random.next_int_bounded(bound) + 1200;
    }

    fn move_to(&self, me: &Entity, mob: &mut Mob, view: &dyn BlockView, at: BlockPos) {
        let nav = living::nav_mob(me, mob, view);
        let target = Vec3::new(
            f64::from(at.x) + 0.5,
            f64::from(at.y),
            f64::from(at.z) + 0.5,
        );
        mob.navigation
            .move_to(view, &nav, target, f64::from(1.2_f32));
    }

    /// `FoxEatBerriesGoal.tick` → `MoveToBlockGoal.tick`（接受距离 2，100 刻重算一次）。
    fn tick(&mut self, me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
        if self.reached_target {
            if self.ticks_waited >= 40 {
                self.on_reached(me, mob, ctx);
            } else {
                self.ticks_waited += 1;
            }
        } else if me.random.next_float() < 0.05 {
            living::play_sound(me, mob, "minecraft:entity.fox.sniff", 1.0, 1.0);
        }
        let target = self.block_pos.above();
        let centre = Vec3::new(
            f64::from(target.x) + 0.5,
            f64::from(target.y) + 0.5,
            f64::from(target.z) + 0.5,
        );
        if me.position.distance_to_sqr(centre) < 4.0 {
            self.reached_target = true;
            self.try_ticks -= 1;
        } else {
            self.reached_target = false;
            self.try_ticks += 1;
            if self.try_ticks % 100 == 0 {
                self.move_to(me, mob, ctx.blocks(), target);
            }
        }
    }

    /// `FoxEatBerriesGoal.onReachedTarget`。
    fn on_reached(&self, me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
        if !ctx.blocks().block_game_rules().mob_griefing {
            return;
        }
        let state = ctx.blocks().block_state(self.block_pos);
        if state.is(blocks::SWEET_BERRY_BUSH) {
            let mouth_empty = mob.equipment.get(EquipmentSlot::MainHand).is_empty();
            if mouth_empty {
                mob.equipment.set(
                    EquipmentSlot::MainHand,
                    ItemStack::new(items::SWEET_BERRIES, 1),
                );
            }
            living::play_sound(
                me,
                mob,
                "minecraft:block.sweet_berry_bush.pick_berries",
                1.0,
                1.0,
            );
            ctx.emit(
                Command::FoxPickBerries {
                    pos: self.block_pos,
                    age: state.get(AGE_3),
                    took_one: mouth_empty,
                },
                me,
            );
        } else if has_glow_berries(state) {
            ctx.emit(
                Command::FoxPickGlowBerries {
                    pos: self.block_pos,
                },
                me,
            );
        }
    }
}

/// `CaveVines.hasGlowBerries`。
fn has_glow_berries(state: crate::state::BlockState) -> bool {
    (state.is(blocks::CAVE_VINES) || state.is(blocks::CAVE_VINES_PLANT))
        && state.get(crate::state::BERRIES)
}

/// `FoxEatBerriesGoal.isValidTarget`。
fn is_berry_target(view: &dyn BlockView, pos: BlockPos) -> bool {
    let state = view.block_state(pos);
    (state.is(blocks::SWEET_BERRY_BUSH) && state.get(AGE_3) >= 2) || has_glow_berries(state)
}

/// `MoveToBlockGoal.findNearestBlock`，水平 12、竖直 1。
fn find_berries(origin: BlockPos, mob: &Mob, view: &dyn BlockView) -> Option<BlockPos> {
    for dy in [-1, 0, -2] {
        for radius in 0..12 {
            for dx in alternating(0, radius) {
                let start_z = if dx.abs() == radius { 0 } else { radius };
                for dz in alternating(start_z, radius) {
                    let pos = origin.offset(dx, dy, dz);
                    if within_home(mob, pos) && is_berry_target(view, pos) {
                        return Some(pos);
                    }
                }
            }
        }
    }
    None
}

fn alternating(start: i32, max: i32) -> impl Iterator<Item = i32> {
    std::iter::successors(Some(start), |&n| Some(if n > 0 { -n } else { 1 - n }))
        .take_while(move |&n| n <= max)
}
