//! `AllayAi`：官方大脑结构表和悦灵专属行为。
//!
//! `BRAIN_PROVIDER` 是四个传感器、三个常驻记忆、两个活动（`CORE` / `IDLE`）。
//! 全部行为按 26.3 字节码落地：捡同种掉落物、送给喜欢的玩家 / 最近响过的音符盒、
//! 跟随、闲逛，以及振动到达后记下音符盒位置。

use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_data::blocks;
use mc_bamboo_protocol::components::value::GlobalPos;

use super::{
    Mob,
    allay::{ITEM_PICKUP_COOLDOWN, NOTEBLOCK_COOLDOWN},
    brain::{
        Activity, ActivityData, Brain, BrainTemplate, Cx, TemplateCache,
        behavior::{Class, Gate, Kind, MoveToTargetSink, Speed},
        current_position, direction_from_rotation,
        memory::{
            MemoryStatus::{self, Registered, ValueAbsent, ValuePresent},
            MemoryType::{self, *},
            MemoryValue, PositionTracker, WalkTarget,
        },
        sensor::SensorKind,
        template_cache,
    },
};
use crate::entity::ctx::EntityCtx;

use MemoryType as M;

/// 悦灵专属行为。
#[derive(Debug, Clone, PartialEq)]
pub enum Behavior {
    /// `GoToWantedItem.create(allay -> true, 1.75, true, 32)`。
    GoToWantedItem,
    /// `GoAndGiveItemsToTarget(..., 2.25, 20, throwItem, cooldown, 60, hasItemToThrow)`。
    GoAndGiveItemsToTarget,
    /// `StayCloseToTarget.create(getItemDepositPosition, !hasWantedItem, 4, 16, 2.25)`。
    StayCloseToTarget,
    /// `RandomStroll.fly(1)`，飞行随机落点。
    FlyStroll,
}

const GO_TO_WANTED_REQUIRED: &[(MemoryType, MemoryStatus)] = &[
    (WalkTarget, Registered),
    (LookTarget, Registered),
    (NearestVisibleWantedItem, ValuePresent),
    (ItemPickupCooldownTicks, Registered),
];
const GIVE_REQUIRED: &[(MemoryType, MemoryStatus)] = &[
    (LookTarget, Registered),
    (WalkTarget, Registered),
    (ItemPickupCooldownTicks, Registered),
];
const STAY_CLOSE_REQUIRED: &[(MemoryType, MemoryStatus)] =
    &[(LookTarget, Registered), (WalkTarget, Registered)];
const FLY_STROLL_REQUIRED: &[(MemoryType, MemoryStatus)] = &[(WalkTarget, ValueAbsent)];

impl Behavior {
    pub(super) fn class(&self) -> Class {
        match self {
            Self::GoAndGiveItemsToTarget => Class::Timed { min: 20, max: 20 },
            _ => Class::OneShot,
        }
    }

    pub(super) fn required(&self) -> &'static [(MemoryType, MemoryStatus)] {
        match self {
            Self::GoToWantedItem => GO_TO_WANTED_REQUIRED,
            Self::GoAndGiveItemsToTarget => GIVE_REQUIRED,
            Self::StayCloseToTarget => STAY_CLOSE_REQUIRED,
            Self::FlyStroll => FLY_STROLL_REQUIRED,
        }
    }

    pub(super) fn name(&self) -> &'static str {
        match self {
            Self::GoAndGiveItemsToTarget => "GoAndGiveItemsToTarget",
            _ => "OneShot",
        }
    }

    pub(super) fn extra_start(&mut self, cx: &mut Cx) -> bool {
        match self {
            Self::GoAndGiveItemsToTarget => can_throw_item_to_target(cx),
            _ => true,
        }
    }

    pub(super) fn start(&mut self, cx: &mut Cx) {
        if let Self::GoAndGiveItemsToTarget = self
            && let Some(tracker) = item_deposit_position(cx)
        {
            set_walk_and_look(cx, tracker, 2.25, 3);
        }
    }

    pub(super) fn can_still_use(&mut self, cx: &mut Cx) -> bool {
        match self {
            Self::GoAndGiveItemsToTarget => can_throw_item_to_target(cx),
            _ => false,
        }
    }

    pub(super) fn tick(&mut self, cx: &mut Cx) {
        let Self::GoAndGiveItemsToTarget = self else {
            return;
        };
        let Some(tracker) = item_deposit_position(cx) else {
            return;
        };
        let Some(at) = current_position(cx, tracker) else {
            return;
        };
        let eye = Vec3::new(
            cx.me.position.x,
            cx.me.position.y + f64::from(cx.mob.eye_height),
            cx.me.position.z,
        );
        if at.distance_to_sqr(eye) < 9.0 {
            throw_item(cx, at);
            cx.memories.set(
                ItemPickupCooldownTicks,
                MemoryValue::Int(ITEM_PICKUP_COOLDOWN),
            );
        }
    }

    pub(super) fn stop(&mut self, _cx: &mut Cx) {}

    pub(super) fn trigger(&mut self, cx: &mut Cx) -> bool {
        match self {
            // `GoToWantedItem.create(predicate, 1.75, true, 32)`：WALK_TARGET 已登记即可覆盖，
            // 但 ITEM_PICKUP_COOLDOWN_TICKS 不能有值，且要能捡东西。
            Self::GoToWantedItem => trigger_go_to_wanted_item(cx),
            // `StayCloseToTarget.create(...)`：没有想捡的掉落物才跟目标。
            Self::StayCloseToTarget => trigger_stay_close(cx),
            // `RandomStroll.fly(1)`：飞行随机落点。
            Self::FlyStroll => trigger_fly_stroll(cx),
            _ => false,
        }
    }
}

static TEMPLATE: TemplateCache = template_cache();

/// `Allay.BRAIN_PROVIDER`：四个传感器、三个常驻记忆、两个活动。
pub fn brain() -> Brain {
    Brain::cached(&TEMPLATE, || {
        BrainTemplate::new_with_memories(
            &[
                SensorKind::NearestLivingEntities,
                SensorKind::NearestPlayers,
                SensorKind::HurtBy,
                SensorKind::NearestItems,
            ],
            &[
                M::LikedPlayer,
                M::LikedNoteblockPosition,
                M::LikedNoteblockCooldownTicks,
            ],
            vec![core_activity(), idle_activity()],
        )
    })
}

/// `AllayAi.initCoreActivity`：`ActivityData.create(CORE, 0, [...])`，优先级从 0 顺排
/// 0..5。
fn core_activity() -> ActivityData {
    ActivityData::sequential(
        Activity::Core,
        0,
        vec![
            Kind::Swim { chance: 0.8 },
            Kind::AnimalPanic {
                speed: 2.5,
                environmental: false,
                armadillo: false,
                camel: false,
            },
            Kind::LookAtTargetSink { min: 45, max: 90 },
            Kind::MoveToTargetSink(MoveToTargetSink::new(false)),
            Kind::CountDownCooldown(M::LikedNoteblockCooldownTicks),
            Kind::CountDownCooldown(ItemPickupCooldownTicks),
        ],
    )
}

/// `AllayAi.initIdleActivity`：`ActivityData.create(IDLE, 0, [...])`，优先级从 0 顺排
/// 0..4（最后一个是 `RunOne`）。
fn idle_activity() -> ActivityData {
    ActivityData::sequential(
        Activity::Idle,
        0,
        vec![
            Kind::Allay(Behavior::GoToWantedItem),
            Kind::Allay(Behavior::GoAndGiveItemsToTarget),
            Kind::Allay(Behavior::StayCloseToTarget),
            Kind::LookAtPlayerSometimes {
                max_dist: 6.0,
                min_interval: 30,
                max_interval: 60,
                ticks_until_next_start: 0,
            },
            Kind::Gate(Box::new(Gate::run_one(
                Vec::new(),
                vec![
                    (Kind::Allay(Behavior::FlyStroll), 2),
                    (
                        Kind::WalkTargetFromLook {
                            speed: Speed::Fixed(1.0),
                            close_enough: 3,
                        },
                        2,
                    ),
                    (Kind::DoNothing { min: 30, max: 60 }, 1),
                ],
            ))),
        ],
    )
}

/// `AllayAi.updateActivity`：`setActiveActivityToFirstValid([IDLE])`。
pub fn update_activity(brain: &mut Brain) {
    brain.set_active_activity_to_first_valid(&[Activity::Idle]);
}

/// `AllayAi.hearNoteblock`：设 / 刷新喜欢的音符盒和 600 刻冷却。
pub fn hear_noteblock(mob: &mut Mob, ctx: &dyn EntityCtx, pos: BlockPos) {
    let _ = ctx;
    let Some(brain) = mob.brain.as_mut() else {
        return;
    };
    let global = GlobalPos {
        dimension: "minecraft:overworld".to_owned(),
        pos,
    };
    let existing = brain
        .memories
        .global_pos(M::LikedNoteblockPosition)
        .cloned();
    match existing {
        None => {
            brain
                .memories
                .set(M::LikedNoteblockPosition, MemoryValue::GlobalPos(global));
            brain.memories.set(
                M::LikedNoteblockCooldownTicks,
                MemoryValue::Int(NOTEBLOCK_COOLDOWN),
            );
        }
        Some(old) if old == global => {
            brain.memories.set(
                M::LikedNoteblockCooldownTicks,
                MemoryValue::Int(NOTEBLOCK_COOLDOWN),
            );
        }
        _ => {}
    }
}

/// `AllayAi.getItemDepositPosition`：优先 1024 格内、仍是音符盒、冷却没过期的
/// 音符盒上方；否则擦掉那份记忆，改送喜欢的玩家。
fn item_deposit_position(cx: &mut Cx) -> Option<PositionTracker> {
    if let Some(gp) = cx.memories.global_pos(M::LikedNoteblockPosition).cloned()
        && should_deposit_at_noteblock(cx, &gp)
    {
        return Some(PositionTracker::at_block(gp.pos.above()));
    }
    cx.memories.erase(M::LikedNoteblockPosition);
    // `new EntityTracker(player, true)`：`trackEyeHeight` 为真、`targetEyeHeight` 为假。
    // `new EntityTracker(player, true)`: `trackEyeHeight` true, `targetEyeHeight` false.
    liked_player(cx).map(|uid| PositionTracker::Entity {
        uid,
        track_eye: true,
        target_eye: false,
    })
}

/// `AllayAi.shouldDepositItemsAtLikedNoteblock`：冷却记忆存在、同维度 1024 格内、
/// 那一格仍是音符盒。
fn should_deposit_at_noteblock(cx: &Cx, gp: &GlobalPos) -> bool {
    if super::allay::oracle_tamper() == super::allay::Tamper::DepositPlayerOnly {
        return false;
    }
    if gp.dimension != "minecraft:overworld" {
        return false;
    }
    let block = BlockPos::containing(cx.me.position);
    let dx = f64::from(gp.pos.x - block.x);
    let dy = f64::from(gp.pos.y - block.y);
    let dz = f64::from(gp.pos.z - block.z);
    if dx * dx + dy * dy + dz * dz > 1024.0 * 1024.0 {
        return false;
    }
    if !cx.ctx.blocks().block_state(gp.pos).is(blocks::NOTE_BLOCK) {
        return false;
    }
    cx.memories.has(M::LikedNoteblockCooldownTicks)
}

/// `AllayAi.getLikedPlayer`：生存 / 创造且在 64 格内的玩家。对拍里的玩家视图
/// 已在传感器扫描时写进 `NEAREST_PLAYERS`；这里按 UUID 反查。
fn liked_player(cx: &Cx) -> Option<crate::entity::EntityUid> {
    let uuid = cx.memories.uuid(M::LikedPlayer)?;
    let mut found = None;
    cx.ctx.blocks().for_each_player(&mut |player| {
        if player.uuid == Some(uuid)
            && (player.game_mode == 0 || player.game_mode == 1)
            && player.position.distance_to_sqr(cx.me.position) < 64.0 * 64.0
        {
            found = Some(player.uid);
        }
    });
    found
}

/// `GoAndGiveItemsToTarget.canThrowItemToTarget`：背包里有东西且找得到送达点。
fn can_throw_item_to_target(cx: &mut Cx) -> bool {
    cx.mob.allay.has_item_to_throw() && item_deposit_position(cx).is_some()
}

/// `BehaviorUtils.setWalkAndLookTargetMemories(entity, tracker, speed, closeEnough)`。
fn set_walk_and_look(cx: &mut Cx, tracker: PositionTracker, speed: f32, close: i32) {
    cx.memories.set(LookTarget, MemoryValue::Look(tracker));
    cx.memories.set(
        WalkTarget,
        MemoryValue::Walk(WalkTarget {
            target: tracker,
            speed,
            close_enough: close,
        }),
    );
}

/// `GoToWantedItem` 的一次性触发。
fn trigger_go_to_wanted_item(cx: &mut Cx) -> bool {
    // `tryGet(WALK_TARGET).isEmpty()`：已经有要去的地方就不再触发。
    // `tryGet(WALK_TARGET).isEmpty()`: do not trigger while a walk target is set.
    if cx.memories.has(WalkTarget) {
        return false;
    }
    // `entity.canPickUpLoot()`：悦灵的覆写 = 不在捡物冷却里且手里有模板物品。
    // `entity.canPickUpLoot()`: the allay override = not on pickup cooldown and
    // holding a template item.
    if cx.memories.has(ItemPickupCooldownTicks)
        || !super::allay::Allay::has_item_in_hand(&cx.mob.allay, cx.mob)
    {
        return false;
    }
    let Some((uid, remembered)) = cx.memories.wanted_item() else {
        return false;
    };
    let mut at = remembered;
    cx.ctx.with_other(uid, &mut |public| {
        if !public.removed {
            at = public.position;
        }
    });
    if at.distance_to_sqr(cx.me.position) >= 32.0 * 32.0 {
        return false;
    }
    // `WorldBorder.isWithinBounds(item.blockPosition())`。
    if !cx
        .ctx
        .blocks()
        .world_border()
        .is_within_bounds_pos(BlockPos::containing(at))
    {
        return false;
    }
    let tracker = PositionTracker::Item {
        uid,
        pos: at,
        track_eye: false,
        target_eye: false,
    };
    cx.memories.set(
        LookTarget,
        MemoryValue::Look(PositionTracker::Item {
            uid,
            pos: at,
            track_eye: true,
            target_eye: true,
        }),
    );
    cx.memories.set(
        WalkTarget,
        MemoryValue::Walk(WalkTarget {
            target: tracker,
            speed: 1.75,
            close_enough: 0,
        }),
    );
    true
}

/// `StayCloseToTarget.create(..., closeEnough=4, tooFar=16, speed=2.25)`。
fn trigger_stay_close(cx: &mut Cx) -> bool {
    if cx.memories.has(NearestVisibleWantedItem) {
        return false;
    }
    let Some(tracker) = item_deposit_position(cx) else {
        return false;
    };
    let Some(at) = current_position(cx, tracker) else {
        return false;
    };
    // `closerThan(tracker.currentPosition(), tooFar=16)`：16 格内就不跟了。
    // `closerThan(tracker.currentPosition(), tooFar=16)`: no need to follow within 16.
    if cx.me.position.distance_to_sqr(at) < 16.0 * 16.0 {
        return false;
    }
    cx.memories.set(LookTarget, MemoryValue::Look(tracker));
    cx.memories.set(
        WalkTarget,
        MemoryValue::Walk(WalkTarget {
            target: tracker,
            speed: 2.25,
            close_enough: 4,
        }),
    );
    true
}

/// `RandomStroll.fly(1)`：`getTargetFlyPos(mob, 10, 7)` →
/// `AirAndWaterRandomPos.getPos(mob, 10, 7, -2, dir.x, dir.z, PI/2)`。
fn trigger_fly_stroll(cx: &mut Cx) -> bool {
    // `getViewVector(0)`：partialTick=0 时俯仰是身体 `x_rot_o`，偏航是 `LivingEntity`
    // 覆写后的头部 `y_head_rot_o`（不是身体 `y_rot_o`）。
    // `getViewVector(0)`: at partialTick 0 the pitch is the body `x_rot_o` while the yaw
    // is the head `y_head_rot_o` (the `LivingEntity` override), not the body `y_rot_o`.
    let dir = direction_from_rotation(cx.mob.x_rot_o, cx.mob.y_head_rot_o);
    let pos = super::goals::air_and_water_random_pos(
        cx.me,
        cx.mob,
        cx.ctx.blocks(),
        10,
        7,
        -2,
        dir.x,
        dir.z,
        f64::from(std::f32::consts::FRAC_PI_2),
    );
    cx.memories.set_or_erase(
        WalkTarget,
        pos.map(|pos| MemoryValue::Walk(WalkTarget::to_point(pos, 1.0, 0))),
    );
    true
}

/// `AllayAi.throwItem`：从背包拿一个，朝目标上方以 `THROW_VELOCITY` 扔出，
/// 每 7 刻有 1/10 的机会播一声随机音高的 `ALLAY_THROW`。
fn throw_item(cx: &mut Cx, at: Vec3) {
    let stack = cx.mob.allay.remove_one();
    if stack.is_empty() {
        return;
    }
    let target = at.add(Vec3::new(0.0, 1.0, 0.0));
    let eye_y = cx.me.position.y + f64::from(cx.mob.eye_height);
    let origin = Vec3::new(
        cx.me.position.x,
        eye_y - f64::from(0.2_f32),
        cx.me.position.z,
    );
    let direction = target.subtract(cx.me.position).normalize();
    let delta = direction.multiply(0.2, 0.3, 0.2);
    cx.ctx
        .emit(super::allay::throw_command(stack, origin, delta), cx.me);
    let time = cx.ctx.blocks().game_time();
    // `level.random.nextDouble() < 0.9`，先抽一个 double 再抽音高下标。
    if time % 7 == 0 && cx.ctx.blocks().level_random_double() < 0.9 {
        let pitch =
            THROW_SOUND_PITCHES[cx.level_next_int(THROW_SOUND_PITCHES.len() as i32) as usize];
        super::living::play_sound(
            cx.me,
            cx.mob,
            "minecraft:entity.allay.item_thrown",
            1.0,
            pitch,
        );
    }
}

/// `Allay.THROW_SOUND_PITCHES`，16 个音高。
const THROW_SOUND_PITCHES: [f32; 16] = [
    0.5625, 0.625, 0.75, 0.9375, 1.0, 1.0, 1.125, 1.25, 1.5, 1.875, 2.0, 2.25, 2.5, 3.0, 3.75, 4.0,
];

#[cfg(test)]
mod tests {
    use super::*;

    /// 官方的传感器、活动和行为结构表。
    #[test]
    fn brain_structure_matches_vanilla() {
        let brain = brain();
        assert_eq!(
            brain
                .sensor_table()
                .iter()
                .map(|row| row.0)
                .collect::<Vec<_>>(),
            vec![
                "NearestLivingEntitySensor",
                "PlayerSensor",
                "HurtBySensor",
                "NearestItemSensor"
            ]
        );
        let table = brain.behavior_table();
        let core: Vec<_> = table
            .iter()
            .filter(|row| row.1 == "core")
            .map(|row| row.2.as_str())
            .collect();
        let idle: Vec<_> = table
            .iter()
            .filter(|row| row.1 == "idle")
            .map(|row| row.2.as_str())
            .collect();
        assert_eq!(
            core,
            vec![
                "Swim",
                "AnimalPanic",
                "LookAtTargetSink",
                "MoveToTargetSink",
                "CountDownCooldownTicks",
                "CountDownCooldownTicks"
            ]
        );
        assert_eq!(
            idle,
            vec![
                "OneShot",
                "GoAndGiveItemsToTarget",
                "OneShot",
                "OneShot",
                "RunOne[OneShot,OneShot,DoNothing]"
            ]
        );
        assert_eq!(table.len(), core.len() + idle.len());
    }

    /// 音符盒冷却记忆能按官方的 600 刻写回。
    #[test]
    fn hearing_a_noteblock_sets_the_cooldown() {
        let mut brain = brain();
        let global = GlobalPos {
            dimension: "minecraft:overworld".to_owned(),
            pos: BlockPos::new(5, 64, 3),
        };
        brain
            .memories
            .set(M::LikedNoteblockPosition, MemoryValue::GlobalPos(global));
        brain.memories.set(
            M::LikedNoteblockCooldownTicks,
            MemoryValue::Int(NOTEBLOCK_COOLDOWN),
        );
        assert_eq!(
            brain.memories.int(M::LikedNoteblockCooldownTicks),
            Some(NOTEBLOCK_COOLDOWN)
        );
    }
}
