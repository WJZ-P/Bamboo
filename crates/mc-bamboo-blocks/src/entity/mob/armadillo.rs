//! 犰狳：官方 `Armadillo` 和 `ArmadilloAi`。第一种跑大脑的生物：恐慌、受惊缩成球、
//! 跟随诱惑、繁殖、闲逛和环顾，定时掉鳞甲，刷子能刷下鳞甲。
//!
//! 状态机（`ArmadilloState`）每次真的变了才把 `inStateTicks` 清零，和官方
//! `onSyncedDataUpdated` 一样；`tick` 结束时加一。
//!
//! Armadillos: vanilla `Armadillo` and `ArmadilloAi`, the first brain-driven mob:
//! panic, rolling up when scared, following temptation, breeding, strolling and looking
//! around, timed scute shedding and brushing scutes off.
//!
//! The state machine (`ArmadilloState`) resets `inStateTicks` only on a real change,
//! like vanilla's `onSyncedDataUpdated`, and `tick` adds one at the end.

use mc_bamboo_data::{items, registries::attribute};
use mc_bamboo_protocol::item::ItemStack;

use super::{
    Mob, MobEvent, MobType,
    attributes::Attributes,
    brain::{
        Activity, ActivityData, Brain, Cx,
        behavior::{Class, Gate, Kind, MoveToTargetSink, Speed},
        memory::{
            MemoryStatus::{self, ValueAbsent, ValuePresent},
            MemoryType, MemoryValue,
        },
        sensor::{self, SensorKind},
    },
    interact::{MobInteractRequest, MobInteraction},
    living,
};
use crate::{
    InteractionResult,
    entity::{
        Entity,
        ctx::{Command, EntityCtx},
    },
};

/// `EntityTypes.ARMADILLO`：0.7×0.65，眼高 0.26；幼年 `scale(0.6F).withEyeHeight(0.21875F)`。
///
/// `EntityTypes.ARMADILLO`: 0.7 by 0.65, eye 0.26; babies `scale(0.6F)` with eye 0.21875.
pub(super) const fn dimensions(baby: bool) -> (f32, f32, f32) {
    if baby {
        (0.7 * 0.6, 0.65 * 0.6, 0.218_75)
    } else {
        (0.7, 0.65, 0.26)
    }
}

/// `Armadillo.createAttributes`：生命 12，速度 0.14（double）。
///
/// `Armadillo.createAttributes`: 12 health and a 0.14 (double) movement speed.
pub(super) fn attributes() -> Attributes {
    let mut attributes = Attributes::cow();
    attributes.set_base(attribute::MAX_HEALTH, 12.0);
    attributes.set_base(attribute::MOVEMENT_SPEED, 0.14);
    attributes
}

/// `Armadillo.ArmadilloState`。
///
/// `Armadillo.ArmadilloState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum State {
    #[default]
    Idle,
    Rolling,
    Scared,
    Unrolling,
}

impl State {
    /// 网络 ID（`ByIdMap`）。
    ///
    /// The network id (`ByIdMap`).
    pub const fn id(self) -> i32 {
        match self {
            Self::Idle => 0,
            Self::Rolling => 1,
            Self::Scared => 2,
            Self::Unrolling => 3,
        }
    }

    /// 存档名。
    ///
    /// The saved name.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Rolling => "rolling",
            Self::Scared => "scared",
            Self::Unrolling => "unrolling",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        [Self::Idle, Self::Rolling, Self::Scared, Self::Unrolling]
            .into_iter()
            .find(|state| state.name() == name)
    }

    const fn animation_duration(self) -> i64 {
        match self {
            Self::Idle => 0,
            Self::Rolling => 10,
            Self::Scared => 50,
            Self::Unrolling => 30,
        }
    }

    const fn is_threatened(self) -> bool {
        !matches!(self, Self::Idle)
    }
}

/// 犰狳自己的字段。
///
/// The armadillo's own fields.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Armadillo {
    pub state: State,
    pub in_state_ticks: i64,
    pub scute_time: i32,
}

/// `ArmadilloAi` 里的犰狳专属行为。
///
/// The armadillo-only behaviours of `ArmadilloAi`.
#[derive(Debug, Clone, PartialEq)]
pub enum Behavior {
    /// `ARMADILLO_ROLLING_OUT`：危险记忆没了、还缩着就展开。
    ///
    /// `ARMADILLO_ROLLING_OUT`: unroll once the danger memory is gone.
    RollOut,
    /// `ArmadilloBallUp`。
    BallUp {
        next_peek_timer: i32,
        danger_was_around: bool,
    },
}

const ROLL_OUT_REQUIRED: &[(MemoryType, MemoryStatus)] =
    &[(MemoryType::DangerDetectedRecently, ValueAbsent)];

impl Behavior {
    pub(super) fn class(&self) -> Class {
        match self {
            Self::RollOut => Class::OneShot,
            // `BALL_UP_STAY_IN_STATE`：5 分钟。
            // `BALL_UP_STAY_IN_STATE`: five minutes.
            Self::BallUp { .. } => Class::Timed {
                min: 6000,
                max: 6000,
            },
        }
    }

    pub(super) fn required(&self) -> &'static [(MemoryType, MemoryStatus)] {
        match self {
            Self::RollOut => ROLL_OUT_REQUIRED,
            Self::BallUp { .. } => &[],
        }
    }

    pub(super) fn name(&self) -> &'static str {
        match self {
            Self::RollOut => "OneShot",
            Self::BallUp { .. } => "ArmadilloBallUp",
        }
    }

    pub(super) fn trigger(&mut self, cx: &mut Cx) -> bool {
        if is_scared(cx.mob) {
            roll_out(cx.me, cx.mob);
            true
        } else {
            false
        }
    }

    pub(super) fn extra_start(&mut self, cx: &mut Cx) -> bool {
        cx.me.on_ground && !living::is_in_liquid(cx.me, cx.mob)
    }

    pub(super) fn start(&mut self, cx: &mut Cx) {
        roll_up(cx.me, cx.mob);
    }

    pub(super) fn can_still_use(&mut self, cx: &mut Cx) -> bool {
        cx.mob.armadillo.state.is_threatened()
    }

    pub(super) fn stop(&mut self, cx: &mut Cx) {
        if !can_stay_rolled_up(cx.me, cx.mob, cx.memories.has(MemoryType::IsPanicking)) {
            roll_out(cx.me, cx.mob);
        }
    }

    /// `ArmadilloBallUp.tick`。
    ///
    /// `ArmadilloBallUp.tick`.
    pub(super) fn tick(&mut self, cx: &mut Cx) {
        let Self::BallUp {
            next_peek_timer,
            danger_was_around,
        } = self
        else {
            return;
        };
        if *next_peek_timer > 0 {
            *next_peek_timer -= 1;
        }
        let armadillo = cx.mob.armadillo;
        if armadillo.state == State::Rolling
            && armadillo.in_state_ticks > State::Rolling.animation_duration()
        {
            switch_to_state(cx.mob, State::Scared);
            if cx.me.on_ground {
                living::play_sound(cx.me, cx.mob, "minecraft:entity.armadillo.land", 1.0, 1.0);
            }
            return;
        }
        let danger = cx
            .memories
            .time_until_expiry(MemoryType::DangerDetectedRecently);
        let around = danger > 75;
        if around != *danger_was_around {
            *next_peek_timer = pick_next_peek_timer(cx.me);
        }
        *danger_was_around = around;
        match armadillo.state {
            State::Scared => {
                if *next_peek_timer == 0 && cx.me.on_ground && around {
                    cx.mob.events.push(MobEvent::EntityEvent { event: 64 });
                    *next_peek_timer = pick_next_peek_timer(cx.me);
                }
                if danger < State::Unrolling.animation_duration() {
                    living::play_sound(
                        cx.me,
                        cx.mob,
                        "minecraft:entity.armadillo.unroll_start",
                        1.0,
                        1.0,
                    );
                    switch_to_state(cx.mob, State::Unrolling);
                }
            }
            State::Unrolling if danger > State::Unrolling.animation_duration() => {
                switch_to_state(cx.mob, State::Scared);
            }
            _ => {}
        }
    }
}

/// `pickNextPeekTimer`：`SCARED` 的动画长度加 `nextIntBetweenInclusive(100, 400)`。
///
/// `pickNextPeekTimer`: the `SCARED` animation length plus `nextIntBetweenInclusive(100, 400)`.
fn pick_next_peek_timer(me: &mut Entity) -> i32 {
    State::Scared.animation_duration() as i32 + me.random.next_int_bounded(301) + 100
}

/// `Armadillo.BRAIN_PROVIDER`：五个传感器，核心、闲逛、恐慌三个活动。
///
/// `Armadillo.BRAIN_PROVIDER`: five sensors and the core, idle and panic activities.
pub(super) fn brain() -> Brain {
    use crate::entity::mob::brain::{BrainTemplate, TemplateCache, template_cache};
    static TEMPLATE: TemplateCache = template_cache();
    Brain::cached(&TEMPLATE, || {
        BrainTemplate::new(
            &[
                SensorKind::NearestLivingEntities,
                SensorKind::HurtBy,
                SensorKind::FoodTemptations,
                SensorKind::NearestAdult,
                SensorKind::ArmadilloScareDetected,
            ],
            vec![core_activity(), idle_activity(), panic_activity()],
        )
    })
}

fn core_activity() -> ActivityData {
    ActivityData::sequential(
        Activity::Core,
        0,
        vec![
            Kind::Swim { chance: 0.8 },
            Kind::AnimalPanic {
                speed: 2.0,
                environmental: true,
                armadillo: true,
                camel: false,
            },
            Kind::LookAtTargetSink { min: 45, max: 90 },
            Kind::MoveToTargetSink(MoveToTargetSink::new(true)),
            Kind::CountDownCooldown(MemoryType::TemptationCooldownTicks),
            Kind::CountDownCooldown(MemoryType::GazeCooldownTicks),
            Kind::Armadillo(Behavior::RollOut),
        ],
    )
}

fn idle_activity() -> ActivityData {
    ActivityData::with_priorities(
        Activity::Idle,
        vec![
            (
                0,
                Kind::LookAtPlayerSometimes {
                    max_dist: 6.0,
                    min_interval: 30,
                    max_interval: 60,
                    ticks_until_next_start: 0,
                },
            ),
            (
                1,
                Kind::MakeLove {
                    partner: MobType::Armadillo,
                    speed: 1.0,
                    close_enough: 1,
                    spawn_child_at: 0,
                },
            ),
            (
                2,
                Kind::Gate(Box::new(Gate::run_one(
                    Vec::new(),
                    vec![
                        (
                            Kind::FollowTemptation {
                                speed: Speed::fixed(1.25),
                                close_adult: 2.0,
                                close_baby: 1.0,
                            },
                            1,
                        ),
                        (
                            Kind::BabyFollowAdult {
                                min: 5,
                                max: 16,
                                speed: Speed::fixed(1.25),
                            },
                            1,
                        ),
                    ],
                ))),
            ),
            (
                3,
                Kind::RandomLookAround {
                    min_interval: 150,
                    max_interval: 250,
                    max_yaw: 30.0,
                    min_pitch: 0.0,
                    pitch_range: 0.0,
                },
            ),
            (
                4,
                Kind::Gate(Box::new(Gate::run_one(
                    vec![(MemoryType::WalkTarget, ValueAbsent)],
                    vec![
                        (Kind::RandomStroll { speed: 1.0 }, 1),
                        (
                            Kind::WalkTargetFromLook {
                                speed: Speed::fixed(1.0),
                                close_enough: 3,
                            },
                            1,
                        ),
                        (Kind::DoNothing { min: 30, max: 60 }, 1),
                    ],
                ))),
            ),
        ],
    )
}

fn panic_activity() -> ActivityData {
    ActivityData::with_priorities(
        Activity::Panic,
        vec![(
            0,
            Kind::Armadillo(Behavior::BallUp {
                next_peek_timer: 0,
                danger_was_around: false,
            }),
        )],
    )
    .when(vec![
        (MemoryType::DangerDetectedRecently, ValuePresent),
        (MemoryType::IsPanicking, ValueAbsent),
    ])
}

/// `isScared()`：状态不是 `IDLE`。
///
/// `isScared()`: the state is not `IDLE`.
pub(super) fn is_scared(mob: &Mob) -> bool {
    mob.mob_type == MobType::Armadillo && mob.armadillo.state != State::Idle
}

/// `switchToState`：真的变了才清零 `inStateTicks`。
///
/// `switchToState`: `inStateTicks` resets only on a real change.
fn switch_to_state(mob: &mut Mob, state: State) {
    if mob.armadillo.state != state {
        mob.armadillo.state = state;
        mob.armadillo.in_state_ticks = 0;
    }
}

/// `canStayRolledUp`：不在恐慌、不在液体里、没拴着、不骑也不被骑。
///
/// `canStayRolledUp`: not panicking, not in liquid, not leashed, no rider or vehicle.
fn can_stay_rolled_up(me: &Entity, mob: &Mob, panicking: bool) -> bool {
    !panicking && !living::is_in_liquid(me, mob)
}

/// `rollUp`：停在原地、清掉求爱、`makeSound(ROLL)`、换成 `ROLLING`。
///
/// `rollUp`: stop in place, reset love, `makeSound(ROLL)`, switch to `ROLLING`.
fn roll_up(me: &mut Entity, mob: &mut Mob) {
    if is_scared(mob) {
        return;
    }
    // `Mob.stopInPlace`。
    // `Mob.stopInPlace`.
    mob.navigation.stop();
    mob.xxa = 0.0;
    mob.yya = 0.0;
    mob.speed = 0.0;
    mob.zza = 0.0;
    me.delta = mc_bamboo_core::Vec3::new(0.0, 0.0, 0.0);
    mob.in_love = 0;
    living::make_sound(me, mob, "minecraft:entity.armadillo.roll");
    switch_to_state(mob, State::Rolling);
}

/// `rollOut`：`makeSound(UNROLL_FINISH)`，回到 `IDLE`。
///
/// `rollOut`: `makeSound(UNROLL_FINISH)`, back to `IDLE`.
pub(super) fn roll_out(me: &mut Entity, mob: &mut Mob) {
    if !is_scared(mob) {
        return;
    }
    living::make_sound(me, mob, "minecraft:entity.armadillo.unroll_finish");
    switch_to_state(mob, State::Idle);
}

/// `Armadillo.isScaredBy`：在 7×2×7 的扩张盒里，且是亡灵、上次打它的、
/// 或者在疾跑 / 骑乘的非旁观玩家。
///
/// `Armadillo.isScaredBy`: inside the box inflated by 7×2×7, and undead, the last
/// attacker, or a non-spectator player who sprints or rides.
fn is_scared_by(me: &Entity, mob: &Mob, view: &sensor::LivingView) -> bool {
    if !me
        .bounding_box()
        .inflate(7.0, 2.0, 7.0)
        .intersects(view.bounding_box)
    {
        return false;
    }
    if view
        .mob_type
        .is_some_and(|kind| kind.in_entity_tag("minecraft:undead"))
    {
        return true;
    }
    if mob.last_hurt_by_mob == Some(view.uid) {
        return true;
    }
    view.player && !view.spectator && view.sprinting
}

/// `MobSensor` 的 `doTick`：缩得住才扫描，否则清掉危险记忆。
///
/// The `MobSensor` `doTick`: scan only when able to stay rolled up, else forget danger.
pub(super) fn sense_scare(cx: &mut Cx) {
    let panicking = cx.memories.has(MemoryType::IsPanicking);
    if !can_stay_rolled_up(cx.me, cx.mob, panicking) {
        cx.memories.erase(MemoryType::DangerDetectedRecently);
        return;
    }
    let Some(list) = cx.memories.entities(MemoryType::NearestLivingEntities) else {
        return;
    };
    let list = list.to_vec();
    let scared = list
        .into_iter()
        .any(|uid| sensor::living(cx, uid).is_some_and(|view| is_scared_by(cx.me, cx.mob, &view)));
    if scared {
        cx.memories.set_with_ttl(
            MemoryType::DangerDetectedRecently,
            MemoryValue::Bool(true),
            80,
        );
    }
}

/// `Armadillo.customServerAiStep` 里 `super` 之前的部分：大脑、换活动、掉鳞甲。
///
/// The part of `Armadillo.customServerAiStep` before `super`: brain, activity update
/// and scute shedding.
pub(super) fn custom_server_ai_step(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    if let Some(mut brain) = mob.brain.take() {
        brain.tick(me, mob, ctx);
        brain.set_active_activity_to_first_valid(&[Activity::Panic, Activity::Idle]);
        mob.brain = Some(brain);
    }
    if !living::is_alive(me, mob) {
        return;
    }
    mob.armadillo.scute_time -= 1;
    if mob.armadillo.scute_time > 0 || !ctx.blocks().block_game_rules().mob_drops {
        return;
    }
    let request = super::EntityLootRequest::new("minecraft:gameplay/armadillo_shed", me, mob, None);
    ctx.emit(Command::DropEntityLoot(request), me);
    let pitch = (me.random.next_float() - me.random.next_float()) * 0.2 + 1.0;
    living::play_sound(me, mob, "minecraft:entity.armadillo.scute_drop", 1.0, pitch);
    mob.armadillo.scute_time = pick_next_scute_drop_time(me);
}

/// `pickNextScuteDropTime`：`nextInt(6000) + 6000`。
///
/// `pickNextScuteDropTime`: `nextInt(6000) + 6000`.
pub(super) fn pick_next_scute_drop_time(me: &mut Entity) -> i32 {
    me.random.next_int_bounded(6000) + 6000
}

/// `Armadillo.tick` 在 `super.tick()` 之后：受惊时把头夹回身体朝向，`inStateTicks` 加一。
///
/// `Armadillo.tick` after `super.tick()`: clamp the head to the body while scared and
/// count `inStateTicks`.
pub(super) fn tick_after(mob: &mut Mob) {
    if is_scared(mob) {
        let limit = max_head_y_rot(mob);
        let delta = mc_bamboo_core::mth::wrap_degrees(mob.y_body_rot - mob.y_head_rot);
        let clamped = mc_bamboo_core::mth::clamp_f32(delta, -limit, limit);
        mob.y_head_rot += delta - clamped;
    }
    mob.armadillo.in_state_ticks += 1;
}

/// `getMaxHeadYRot`：受惊时 0，否则 32。
///
/// `getMaxHeadYRot`: 0 while scared, else 32.
pub(super) fn max_head_y_rot(mob: &Mob) -> f32 {
    if is_scared(mob) { 0.0 } else { 32.0 }
}

/// `Armadillo.hurtServer`：受惊时伤害先变成 `(damage - 1) / 2`。
///
/// `Armadillo.hurtServer`: while scared the damage becomes `(damage - 1) / 2` first.
pub(super) fn reduce_damage(mob: &Mob, amount: f32) -> f32 {
    if is_scared(mob) {
        (amount - 1.0) / 2.0
    } else {
        amount
    }
}

/// `Armadillo.actuallyHurt` 在 `super` 之后：活体打的记下危险并缩起来，
/// 环境恐慌伤害让它展开。
///
/// `Armadillo.actuallyHurt` after `super`: a living attacker records danger and rolls
/// it up; environmental panic damage unrolls it.
pub(super) fn after_actually_hurt(me: &mut Entity, mob: &mut Mob, source: &super::DamageSource) {
    if mob.no_ai || mob.is_dead_or_dying() {
        return;
    }
    if source.attacker.is_some() {
        let panicking = mob.is_panicking();
        if let Some(brain) = mob.brain.as_mut() {
            brain.memories.set_with_ttl(
                MemoryType::DangerDetectedRecently,
                MemoryValue::Bool(true),
                80,
            );
        }
        if can_stay_rolled_up(me, mob, panicking) {
            roll_up(me, mob);
        }
    } else if super::goals::damage_type_tagged("minecraft:panic_environmental_causes", source.kind)
    {
        roll_out(me, mob);
    }
}

/// `Armadillo.mobInteract`：刷子刷下鳞甲（幼年不行），受惊时 `FAIL`，否则走动物交互。
///
/// `Armadillo.mobInteract`: a brush sheds a scute (not from babies), `FAIL` while
/// scared, else the animal interaction.
pub(super) fn interact(
    me: &mut Entity,
    mob: &mut Mob,
    req: &MobInteractRequest,
) -> Option<MobInteraction> {
    let _ = me;
    if req.held.item == items::BRUSH && !mob.is_baby() {
        // 掉落、音效和刷子耐久由 `World::interact_mob` 按 `shear` 标志接着做。
        // The drop, sound and brush wear follow in `World::interact_mob` via `shear`.
        return Some(MobInteraction {
            result: InteractionResult::Success,
            shear: true,
            ..MobInteraction::pass()
        });
    }
    if is_scared(mob) {
        return Some(MobInteraction {
            result: InteractionResult::Fail,
            ..MobInteraction::pass()
        });
    }
    None
}

/// `brushOffScute` 加 `mobInteract` 之后的 `hurtAndBreak(16)`：按
/// `minecraft:brush/armadillo` 掉落、放刷子音效、刷子扣 16 点耐久。
///
/// `brushOffScute` plus the later `hurtAndBreak(16)`: drop from
/// `minecraft:brush/armadillo`, play the brush sound and wear the brush by 16.
pub(crate) fn apply_brush(
    world: &mut crate::world::World,
    entity: &mut Entity,
    hand: mc_bamboo_core::Hand,
    tool: &ItemStack,
) {
    let Some(request) = entity.with_mob(|me, mob| {
        super::EntityLootRequest::new("minecraft:brush/armadillo", me, mob, Some(tool.clone()))
    }) else {
        return;
    };
    let origin = request.origin;
    for stack in world.entity_loot_drops(request) {
        world.spawn_at_location(origin, stack);
    }
    entity.with_mob(|me, mob| {
        living::play_sound(me, mob, "minecraft:entity.armadillo.brush", 1.0, 1.0);
    });
    crate::durability::hurt_hand(world, hand, 16);
}
