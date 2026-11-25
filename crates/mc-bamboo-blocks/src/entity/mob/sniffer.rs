//! 嗅探兽：官方 `Sniffer`、`SnifferAi` 和它的六个专属行为。跑大脑的生物：闲逛、被诱惑、
//! 恐慌、繁殖，还有它独有的一条闭环——嗅闻找能挖的方块、走过去、挖土掉种子、记下挖过的
//! 位置，繁殖时不生幼崽而是下一颗蛋。
//!
//! 状态机（`Sniffer.State`）藏在 `DATA_STATE` 元数据里：`transitionTo` 按目标状态放声音、
//! 换尺寸（挖掘时更矮），`tick` 在挖掘态按时掉种子（`minecraft:gameplay/sniffer_digging`）。
//!
//! Sniffers: vanilla `Sniffer`, `SnifferAi` and their six behaviours. A brain-driven mob:
//! idling, temptation, panic and breeding, plus its own closed loop — sniffing out a
//! diggable block, walking there, digging for a seed and remembering dug positions, and
//! laying an egg instead of birthing a calf.
//!
//! The `Sniffer.State` machine lives in the `DATA_STATE` metadata: `transitionTo` plays
//! the state sound and swaps dimensions (shorter while digging), and `tick` drops the
//! seed (`minecraft:gameplay/sniffer_digging`) while digging.

use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_data::{PathType, items, registries::attribute, tags::block_tags};
use mc_bamboo_protocol::item::ItemStack;

use super::{
    Mob, MobEvent, MobType,
    attributes::Attributes,
    brain::{
        Activity, ActivityData, Brain, Cx,
        behavior::{Class, Gate, Kind, MoveToTargetSink, Speed},
        current_block_position, direction_from_rotation,
        memory::{
            Memories,
            MemoryStatus::{self, ValueAbsent, ValuePresent},
            MemoryType::{self, *},
            MemoryValue, PositionTracker, WalkTarget,
        },
        sensor::SensorKind,
    },
    living,
};
use crate::entity::{Entity, EntityUid, ctx::EntityCtx};

/// `EntityTypes.SNIFFER`：1.9×1.75，眼高 1.05；幼年 `scale(0.5)` → 0.95×0.875，眼高 0.525。
///
/// `EntityTypes.SNIFFER`: 1.9 by 1.75, eye 1.05; babies `scale(0.5)` → 0.95 by 0.875,
/// eye 0.525.
pub(super) fn dimensions(baby: bool) -> (f32, f32, f32) {
    if baby {
        (0.95, 0.875, 0.525)
    } else {
        (1.9, 1.75, 1.05)
    }
}

/// `DIGGING_DIMENSIONS`：`scalable(1.9, 1.75 - 0.4).withEyeHeight(0.81)`，再乘年龄缩放。
///
/// `DIGGING_DIMENSIONS`: `scalable(1.9, 1.75 - 0.4).withEyeHeight(0.81)`, scaled by age.
pub(super) fn pose_dimensions(baby: bool, state: State) -> (f32, f32, f32) {
    if state == State::Digging {
        let scale = if baby { 0.5 } else { 1.0 };
        (1.9 * scale, 1.35 * scale, 0.81 * scale)
    } else {
        dimensions(baby)
    }
}

/// `Sniffer.createAttributes`：动物那一套，速度 0.1、生命 14。
///
/// `Sniffer.createAttributes`: the animal set with 0.1 speed and 14 health.
pub(super) fn attributes() -> Attributes {
    let mut table = Attributes::cow();
    table.set_base(attribute::MOVEMENT_SPEED, 0.1);
    table.set_base(attribute::MAX_HEALTH, 14.0);
    table
}

/// `Sniffer.State`。
///
/// `Sniffer.State`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum State {
    #[default]
    Idling,
    FeelingHappy,
    Scenting,
    Sniffing,
    Searching,
    Digging,
    Rising,
}

impl State {
    /// 网络 ID（`ByIdMap`）。
    ///
    /// The network id (`ByIdMap`).
    pub const fn id(self) -> i32 {
        match self {
            Self::Idling => 0,
            Self::FeelingHappy => 1,
            Self::Scenting => 2,
            Self::Sniffing => 3,
            Self::Searching => 4,
            Self::Digging => 5,
            Self::Rising => 6,
        }
    }

    /// 存档名。
    ///
    /// The saved name.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Idling => "idling",
            Self::FeelingHappy => "feeling_happy",
            Self::Scenting => "scenting",
            Self::Sniffing => "sniffing",
            Self::Searching => "searching",
            Self::Digging => "digging",
            Self::Rising => "rising",
        }
    }
}

/// 嗅探兽自己的字段：状态机和掉种子的时刻（`DATA_DROP_SEED_AT_TICK`）。
///
/// The sniffer's own fields: the state machine and the drop-seed tick
/// (`DATA_DROP_SEED_AT_TICK`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Sniffer {
    pub state: State,
    pub drop_seed_at_tick: i32,
}

/// `SnifferAi` 里的嗅探兽专属行为。
///
/// The sniffer-only behaviours of `SnifferAi`.
#[derive(Debug, Clone, PartialEq)]
pub enum Behavior {
    /// `Searching`（时长 600）。
    Searching,
    /// `Sniffing`（40..80）。
    Sniffing,
    /// `Digging`（160..180）。
    Digging,
    /// `FinishedDigging`（40）。
    FinishedDigging,
    /// `Scenting`（40..80）。
    Scenting,
    /// `FeelingHappy`（40..100）。
    FeelingHappy,
}

const SEARCHING_REQUIRED: &[(MemoryType, MemoryStatus)] = &[
    (WalkTarget, ValuePresent),
    (IsPanicking, ValueAbsent),
    (SnifferSniffingTarget, ValuePresent),
];
const SNIFFING_REQUIRED: &[(MemoryType, MemoryStatus)] = &[
    (WalkTarget, ValueAbsent),
    (SnifferSniffingTarget, ValueAbsent),
    (SniffCooldown, ValueAbsent),
];
const DIGGING_REQUIRED: &[(MemoryType, MemoryStatus)] = &[
    (IsPanicking, ValueAbsent),
    (WalkTarget, ValueAbsent),
    (SnifferDigging, ValuePresent),
    (SniffCooldown, ValueAbsent),
];
const FINISHED_DIGGING_REQUIRED: &[(MemoryType, MemoryStatus)] = &[
    (IsPanicking, ValueAbsent),
    (WalkTarget, ValueAbsent),
    (SnifferDigging, ValuePresent),
    (SniffCooldown, ValuePresent),
];
const SCENTING_REQUIRED: &[(MemoryType, MemoryStatus)] = &[
    (IsPanicking, ValueAbsent),
    (SnifferDigging, ValueAbsent),
    (SnifferSniffingTarget, ValueAbsent),
    (SnifferHappy, ValueAbsent),
    (BreedTarget, ValueAbsent),
];
const FEELING_HAPPY_REQUIRED: &[(MemoryType, MemoryStatus)] = &[(SnifferHappy, ValuePresent)];

impl Behavior {
    pub(super) fn class(&self) -> Class {
        let (min, max) = match self {
            Self::Searching => (600, 600),
            Self::Sniffing => (40, 80),
            Self::Digging => (160, 180),
            Self::FinishedDigging => (40, 40),
            Self::Scenting => (40, 80),
            Self::FeelingHappy => (40, 100),
        };
        Class::Timed { min, max }
    }

    pub(super) fn required(&self) -> &'static [(MemoryType, MemoryStatus)] {
        match self {
            Self::Searching => SEARCHING_REQUIRED,
            Self::Sniffing => SNIFFING_REQUIRED,
            Self::Digging => DIGGING_REQUIRED,
            Self::FinishedDigging => FINISHED_DIGGING_REQUIRED,
            Self::Scenting => SCENTING_REQUIRED,
            Self::FeelingHappy => FEELING_HAPPY_REQUIRED,
        }
    }

    pub(super) fn name(&self) -> &'static str {
        match self {
            Self::Searching => "Searching",
            Self::Sniffing => "Sniffing",
            Self::Digging => "Digging",
            Self::FinishedDigging => "FinishedDigging",
            Self::Scenting => "Scenting",
            Self::FeelingHappy => "FeelingHappy",
        }
    }

    pub(super) fn extra_start(&mut self, cx: &mut Cx) -> bool {
        match self {
            Self::Searching | Self::Digging => can_sniff(cx),
            Self::Sniffing => !cx.mob.is_baby() && can_sniff(cx),
            Self::Scenting => !is_tempted(cx),
            Self::FinishedDigging | Self::FeelingHappy => true,
        }
    }

    pub(super) fn start(&mut self, cx: &mut Cx) {
        let state = match self {
            Self::Searching => State::Searching,
            Self::Sniffing => State::Sniffing,
            Self::Digging => State::Digging,
            Self::FinishedDigging => State::Rising,
            Self::Scenting => State::Scenting,
            Self::FeelingHappy => State::FeelingHappy,
        };
        transition_to(cx.me, cx.mob, state);
    }

    pub(super) fn can_still_use(&mut self, cx: &mut Cx) -> bool {
        match self {
            Self::Searching => {
                if !can_sniff(cx) {
                    transition_to(cx.me, cx.mob, State::Idling);
                    return false;
                }
                let Some(walk) = cx.memories.walk_target() else {
                    return false;
                };
                let walk_block = current_block_position(cx, walk.target);
                let sniffing_target = cx.memories.block_pos(SnifferSniffingTarget);
                match (walk_block, sniffing_target) {
                    (Some(walk_block), Some(target)) => walk_block == target,
                    _ => false,
                }
            }
            Self::Sniffing => can_sniff(cx),
            Self::Digging => cx.memories.has(SnifferDigging) && can_dig(cx) && cx.mob.in_love <= 0,
            Self::FinishedDigging => cx.memories.has(SnifferDigging),
            Self::Scenting | Self::FeelingHappy => true,
        }
    }

    pub(super) fn stop(&mut self, cx: &mut Cx, timed_out: bool) {
        match self {
            Self::Searching => {
                if can_dig(cx) && can_sniff(cx) {
                    cx.memories.set(SnifferDigging, MemoryValue::Bool(true));
                }
                cx.memories.erase(WalkTarget);
                cx.memories.erase(SnifferSniffingTarget);
            }
            Self::Sniffing => {
                transition_to(cx.me, cx.mob, State::Idling);
                if timed_out && let Some(pos) = calculate_dig_position(cx) {
                    cx.memories
                        .set(SnifferSniffingTarget, MemoryValue::BlockPos(pos));
                    cx.memories.set(
                        WalkTarget,
                        MemoryValue::Walk(WalkTarget {
                            target: PositionTracker::at_block(pos),
                            speed: 1.25,
                            close_enough: 0,
                        }),
                    );
                }
            }
            Self::Digging => {
                if timed_out {
                    cx.memories
                        .set_with_ttl(SniffCooldown, MemoryValue::Unit, 9600);
                } else {
                    reset_sniffing(cx);
                }
            }
            Self::FinishedDigging => {
                transition_to(cx.me, cx.mob, State::Idling);
                store_explored_position(cx, timed_out);
                cx.memories.erase(SnifferDigging);
                cx.memories.set(SnifferHappy, MemoryValue::Bool(true));
            }
            Self::Scenting => transition_to(cx.me, cx.mob, State::Idling),
            Self::FeelingHappy => {
                transition_to(cx.me, cx.mob, State::Idling);
                cx.memories.erase(SnifferHappy);
            }
        }
    }

    pub(super) fn tick(&mut self, _cx: &mut Cx) {}

    pub(super) fn trigger(&mut self, _cx: &mut Cx) -> bool {
        false
    }
}

/// `Sniffer.BRAIN_PROVIDER`：四个传感器（顺序照官方）和 `SnifferAi.getActivities()`。
///
/// `Sniffer.BRAIN_PROVIDER`: four sensors in vanilla order and `SnifferAi.getActivities()`.
pub(super) fn brain() -> Brain {
    use crate::entity::mob::brain::{BrainTemplate, TemplateCache, template_cache};
    static TEMPLATE: TemplateCache = template_cache();
    Brain::cached(&TEMPLATE, || {
        BrainTemplate::new_with_memories(
            &[
                SensorKind::NearestLivingEntities,
                SensorKind::HurtBy,
                SensorKind::NearestPlayers,
                SensorKind::FoodTemptations,
            ],
            &[SnifferExploredPositions],
            vec![
                core_activity(),
                idle_activity(),
                sniff_activity(),
                dig_activity(),
            ],
        )
    })
}

/// `SnifferAi.initCoreActivity`。
fn core_activity() -> ActivityData {
    ActivityData::sequential(
        Activity::Core,
        0,
        vec![
            Kind::Swim { chance: 0.8 },
            Kind::AnimalPanic {
                speed: 2.0,
                environmental: false,
                armadillo: false,
                camel: false,
            },
            Kind::MoveToTargetSink(MoveToTargetSink::with_duration(500, 700, false)),
            Kind::CountDownCooldown(TemptationCooldownTicks),
        ],
    )
}

/// `SnifferAi.initIdleActivity`。
fn idle_activity() -> ActivityData {
    ActivityData::with_priorities(
        Activity::Idle,
        vec![
            (
                0,
                Kind::MakeLove {
                    partner: MobType::Sniffer,
                    speed: 1.0,
                    close_enough: 2,
                    spawn_child_at: 0,
                },
            ),
            (
                1,
                Kind::FollowTemptation {
                    speed: Speed::fixed(1.25),
                    close_adult: 3.5,
                    close_baby: 2.5,
                },
            ),
            (2, Kind::LookAtTargetSink { min: 45, max: 90 }),
            (3, Kind::Sniffer(Behavior::FeelingHappy)),
            (
                4,
                Kind::Gate(Box::new(Gate::run_one(
                    Vec::new(),
                    vec![
                        (
                            Kind::WalkTargetFromLook {
                                speed: Speed::fixed(1.0),
                                close_enough: 3,
                            },
                            2,
                        ),
                        (Kind::Sniffer(Behavior::Scenting), 1),
                        (Kind::Sniffer(Behavior::Sniffing), 1),
                        (Kind::LookAtPlayer { max_dist: 6.0 }, 1),
                        (
                            Kind::RandomStroll {
                                speed: 1.0,
                                horizontal: 10,
                                vertical: 7,
                            },
                            1,
                        ),
                        (Kind::DoNothing { min: 5, max: 20 }, 2),
                    ],
                ))),
            ),
        ],
    )
    .when(vec![(SnifferDigging, ValueAbsent)])
}

/// `SnifferAi.initSniffingActivity`。
fn sniff_activity() -> ActivityData {
    ActivityData::with_priorities(
        Activity::Sniff,
        vec![(0, Kind::Sniffer(Behavior::Searching))],
    )
    .when(vec![
        (IsPanicking, ValueAbsent),
        (SnifferSniffingTarget, ValuePresent),
        (WalkTarget, ValuePresent),
    ])
}

/// `SnifferAi.initDigActivity`。
fn dig_activity() -> ActivityData {
    ActivityData::with_priorities(
        Activity::Dig,
        vec![
            (0, Kind::Sniffer(Behavior::Digging)),
            (0, Kind::Sniffer(Behavior::FinishedDigging)),
        ],
    )
    .when(vec![
        (IsPanicking, ValueAbsent),
        (WalkTarget, ValueAbsent),
        (SnifferDigging, ValuePresent),
    ])
}

/// `Sniffer.getAmbientSound`：挖掘 / 搜寻时不叫。
///
/// `Sniffer.getAmbientSound`: silent while digging or searching.
pub(super) fn ambient_voice(mob: &Mob) -> Option<&'static str> {
    match mob.sniffer.state {
        State::Digging | State::Searching => None,
        _ => Some("minecraft:entity.sniffer.idle"),
    }
}

/// `Sniffer.customServerAiStep` 里 `super` 之前的部分：大脑和 `SnifferAi.updateActivity`。
///
/// The part of `Sniffer.customServerAiStep` before `super`: the brain and
/// `SnifferAi.updateActivity`.
pub(super) fn custom_server_ai_step(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    if let Some(mut brain) = mob.brain.take() {
        let gap = ctx.throttle_plan().decision_gap;
        brain.tick_by(me, mob, ctx, gap);
        brain.set_active_activity_to_first_valid(&[Activity::Dig, Activity::Sniff, Activity::Idle]);
        mob.brain = Some(brain);
    }
}

/// `Sniffer.tick` 在 `super.tick()` 之前：挖掘时掉种子（搜寻音是客户端效果，不建模）。
///
/// `Sniffer.tick` before `super.tick()`: drop the seed while digging (the searching
/// sound is client-only and not modelled).
pub(super) fn tick_before(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    if mob.sniffer.state != State::Digging {
        return;
    }
    drop_seed(me, mob, ctx);
}

/// `Sniffer.transitionTo(state)`：放声音、改尺寸、`onDiggingStart` / `onScentingStart`。
///
/// `Sniffer.transitionTo(state)`: play the sound, refresh dimensions, and run
/// `onDiggingStart` / `onScentingStart`.
fn transition_to(me: &mut Entity, mob: &mut Mob, state: State) {
    match state {
        State::Idling => {}
        State::FeelingHappy => {
            living::play_sound(me, mob, "minecraft:entity.sniffer.happy", 1.0, 1.0);
        }
        State::Scenting => {
            let pitch = if mob.is_baby() { 1.3 } else { 1.0 };
            living::play_sound(me, mob, "minecraft:entity.sniffer.scenting", 1.0, pitch);
        }
        State::Sniffing => {
            living::play_sound(me, mob, "minecraft:entity.sniffer.sniffing", 1.0, 1.0);
        }
        State::Searching => {}
        State::Digging => {
            mob.sniffer.drop_seed_at_tick = me.tick_count + 120;
            // `onDiggingStart`：`broadcastEntityEvent(this, 63)`。
            // `onDiggingStart`: `broadcastEntityEvent(this, 63)`.
            mob.events.push(MobEvent::EntityEvent { event: 63 });
        }
        State::Rising => {
            living::play_sound(me, mob, "minecraft:entity.sniffer.digging_stop", 1.0, 1.0);
        }
    }
    mob.sniffer.state = state;
    // 挖掘态的尺寸更矮。
    let (width, height, eye) = pose_dimensions(mob.is_baby(), state);
    me.dimensions = (width, height);
    mob.eye_height = eye;
}

/// `Sniffer.isTempted`：`TEMPTING_PLAYER` 记忆在不在。
///
/// `Sniffer.isTempted`: whether the `TEMPTING_PLAYER` memory is present.
fn is_tempted(cx: &Cx) -> bool {
    cx.memories.has(TemptingPlayer)
}

/// `Sniffer.canSniff`：没被诱惑、没恐慌、不在水里、没求爱、在地上、没骑也没拴。
///
/// `Sniffer.canSniff`: not tempted, not panicking, not in water, not in love, on the
/// ground, not a passenger and not leashed (leashing is not modelled, always false).
fn can_sniff(cx: &Cx) -> bool {
    !is_tempted(cx)
        && !cx.mob.is_panicking()
        && !cx.me.in_water
        && cx.mob.in_love <= 0
        && cx.me.on_ground
        && cx.me.vehicle.is_none()
}

/// `Sniffer.canDig()`：成年的 `canSniff` 再要求脚下前方那格能挖。
///
/// `Sniffer.canDig()`: an adult `canSniff` whose head block can be dug.
fn can_dig(cx: &mut Cx) -> bool {
    if cx.mob.is_baby() {
        return false;
    }
    if !can_sniff(cx) {
        return false;
    }
    can_dig_at(cx, get_head_block(cx.me, cx.mob).below())
}

/// 挖过的位置不再挖：`SNIFFER_EXPLORED_POSITIONS` 里有同维度的 `GlobalPos` 就跳过。
///
/// A dug position is not dug again: a matching `GlobalPos` in
/// `SNIFFER_EXPLORED_POSITIONS` skips it.
fn already_explored(memories: &Memories, pos: BlockPos) -> bool {
    memories
        .global_pos_list(SnifferExploredPositions)
        .is_some_and(|list| {
            list.iter()
                .any(|gp| gp.dimension == "minecraft:overworld" && gp.pos == pos)
        })
}

/// `Sniffer.canDig(BlockPos)`：在 `#sniffer_diggable_block` 里、没挖过、走得到。
///
/// `Sniffer.canDig(BlockPos)`: in `#sniffer_diggable_block`, not yet explored, and
/// reachable by the navigation.
fn can_dig_at(cx: &mut Cx, pos: BlockPos) -> bool {
    let view = cx.ctx.blocks();
    if !view
        .block_state(pos)
        .is_tagged(block_tags::SNIFFER_DIGGABLE_BLOCK)
    {
        return false;
    }
    if already_explored(cx.memories, pos) {
        return false;
    }
    let nav = living::nav_mob(cx.me, cx.mob, view);
    let path = cx.mob.navigation.create_path(view, &nav, pos, 1);
    path.is_some_and(|path| path.reached)
}

/// `Sniffer.getHeadPosition`：`position + forward * 2.25`。
///
/// `Sniffer.getHeadPosition`: `position + forward * 2.25`.
fn get_head_position(me: &Entity, mob: &Mob) -> Vec3 {
    let forward = direction_from_rotation(mob.x_rot, me.y_rot);
    me.position.add(forward.scale(2.25))
}

/// `Sniffer.getHeadBlock`：头位置再上抬 0.2 那格。
///
/// `Sniffer.getHeadBlock`: the block containing the head position raised by 0.2.
fn get_head_block(me: &Entity, mob: &Mob) -> BlockPos {
    let head = get_head_position(me, mob);
    BlockPos::containing(Vec3::new(head.x, me.position.y + 0.2, head.z))
}

/// `Sniffer.calculateDigPosition`：五档随机落点里第一块能挖的（随机落点的下面那格）。
///
/// `Sniffer.calculateDigPosition`: the block below the first random spot, out of five
/// ever-wider tiers, whose below cell is diggable.
fn calculate_dig_position(cx: &mut Cx) -> Option<BlockPos> {
    for i in 0..5 {
        let pos = super::goals::land_random_pos(cx.me, cx.mob, cx.ctx.blocks(), 10 + 2 * i, 3)?;
        let block = BlockPos::containing(pos);
        if !cx.ctx.blocks().world_border().is_within_bounds_pos(block) {
            continue;
        }
        let below = block.below();
        if can_dig_at(cx, below) {
            return Some(below);
        }
    }
    None
}

/// `Sniffer.dropSeed`：`DATA_DROP_SEED_AT_TICK == tickCount` 时按 `sniffer_digging` 礼物表
/// 在头那一格掉种子，放掉种子的声音。
///
/// `Sniffer.dropSeed`: when `DATA_DROP_SEED_AT_TICK == tickCount`, roll the
/// `sniffer_digging` gift table at the head block and play the drop-seed sound.
fn drop_seed(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    if mob.sniffer.drop_seed_at_tick != me.tick_count {
        return;
    }
    let head = get_head_block(me, mob);
    let request = super::EntityLootRequest {
        table: "minecraft:gameplay/sniffer_digging",
        kind: MobType::Sniffer,
        origin: Vec3::new(f64::from(head.x), f64::from(head.y), f64::from(head.z)),
        wool: 0,
        variant: super::CowVariant::Temperate,
        tool: None,
    };
    ctx.emit(crate::entity::ctx::Command::DropEntityLoot(request), me);
    living::play_sound(me, mob, "minecraft:entity.sniffer.drop_seed", 1.0, 1.0);
}

/// `Sniffer.storeExploredPosition`：挖过的位置插到 `SNIFFER_EXPLORED_POSITIONS` 头部，
/// 只留前 21 个。
///
/// `Sniffer.storeExploredPosition`: prepend the dug position to
/// `SNIFFER_EXPLORED_POSITIONS`, keeping at most 21 entries.
fn store_explored_position(cx: &mut Cx, explored: bool) {
    if !explored {
        return;
    }
    let mut list: Vec<mc_bamboo_protocol::components::value::GlobalPos> = cx
        .memories
        .global_pos_list(SnifferExploredPositions)
        .map(<[_]>::to_vec)
        .unwrap_or_default();
    list.truncate(20);
    list.insert(
        0,
        mc_bamboo_protocol::components::value::GlobalPos {
            dimension: "minecraft:overworld".to_owned(),
            pos: cx.me.on_pos(cx.ctx.blocks(), 0.2),
        },
    );
    cx.memories
        .set(SnifferExploredPositions, MemoryValue::GlobalPosList(list));
}

/// `SnifferAi.resetSniffing`：清掉挖掘和嗅闻目标，回到 `IDLING`。
///
/// `SnifferAi.resetSniffing`: forget the digging and sniffing target, back to `IDLING`.
fn reset_sniffing(cx: &mut Cx) {
    cx.memories.erase(SnifferDigging);
    cx.memories.erase(SnifferSniffingTarget);
    transition_to(cx.me, cx.mob, State::Idling);
}

/// `Sniffer` 构造函数：能浮水，水 / 细雪 / 谨慎伤害三格寻路代价 -1。
///
/// The `Sniffer` constructor: floats, and water / powder snow / damage-cautious cells
/// cost -1.
pub(super) fn configure(mob: &mut Mob) {
    mob.navigation.flags.can_float = true;
    mob.malus.set(PathType::Water, -1.0);
    mob.malus.set(PathType::OnTopOfPowderSnow, -1.0);
    mob.malus.set(PathType::DamageCautious, -1.0);
}

/// `Sniffer.spawnChildFromBreeding`：不产幼崽，下一颗嗅探兽蛋。伴侣年龄设 6000、清求爱，
/// 按 `mob_drops` 掉经验，放蛋落下的声音。
///
/// `Sniffer.spawnChildFromBreeding`: no calf — drop a sniffer egg. The partner's age
/// becomes 6000 and its love resets; an experience orb drops when `mob_drops` is on, and
/// the egg-plop sound plays.
pub(super) fn breed(
    me: &mut Entity,
    partner: EntityUid,
    position: Vec3,
    world: &mut crate::world::World,
) {
    world.with_entity_mut(partner, &mut |entity| {
        entity.with_mob(|me, mob| {
            mob.in_love = 0;
            living::set_age(me, mob, 6000);
        });
    });
    if world.block_game_rules().mob_drops {
        let xp = me.random.next_int_bounded(7) + 1;
        world.spawn_entity(Entity::experience_orb(position, xp));
    }
    let egg = ItemStack::new(items::SNIFFER_EGG, 1);
    world.spawn_at_location(position, egg);
    // `playSound(SNIFFER_EGG_PLOP, 1.0, (this.random.nextFloat() - this.random.nextFloat()) * 0.2 + 0.5)`。
    let pitch = (me.random.next_float() - me.random.next_float()) * 0.2 + 0.5;
    me.with_mob(|me, mob| {
        living::play_sound(me, mob, "minecraft:block.sniffer_egg.plop", 1.0, pitch);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 四个活动、四个传感器，初始活动是 core + idle；五种新记忆都登记了。
    ///
    /// Four activities and four sensors, core + idle to begin with; all five new
    /// memories are registered.
    #[test]
    fn sniffer_brain_has_vanilla_activities_and_sensors() {
        let brain = brain();
        let kinds: Vec<SensorKind> = brain.sensors.iter().map(|sensor| sensor.kind).collect();
        assert_eq!(
            kinds,
            [
                SensorKind::NearestLivingEntities,
                SensorKind::HurtBy,
                SensorKind::NearestPlayers,
                SensorKind::FoodTemptations,
            ]
        );
        assert_eq!(brain.active_names(), vec!["core", "idle"]);
        for kind in [
            SnifferExploredPositions,
            SnifferSniffingTarget,
            SnifferDigging,
            SnifferHappy,
            SniffCooldown,
        ] {
            assert!(brain.memories.is_registered(kind), "{}", kind.name());
        }
    }

    /// 成年 / 幼年 / 挖掘尺寸和属性照官方字节码。
    ///
    /// Adult, baby and digging dimensions plus the attributes match the bytecode.
    #[test]
    fn sniffer_dimensions_and_attributes_match_vanilla() {
        assert_eq!(dimensions(false), (1.9, 1.75, 1.05));
        assert_eq!(dimensions(true), (0.95, 0.875, 0.525));
        assert_eq!(pose_dimensions(false, State::Digging), (1.9, 1.35, 0.81));
        assert_eq!(pose_dimensions(true, State::Digging), (0.95, 0.675, 0.405));
        assert_eq!(State::Idling.id(), 0);
        assert_eq!(State::Rising.id(), 6);
        let table = attributes();
        assert_eq!(table.value(attribute::MOVEMENT_SPEED), 0.1);
        assert_eq!(table.value(attribute::MAX_HEALTH), 14.0);
    }

    /// 挖过的位置记进 `SNIFFER_EXPLORED_POSITIONS` 后不再可挖；别的坐标和别的维度不算。
    ///
    /// A position recorded in `SNIFFER_EXPLORED_POSITIONS` is not dug again; other
    /// coordinates and other dimensions do not count.
    #[test]
    fn explored_positions_are_not_diggable_again() {
        use mc_bamboo_protocol::components::value::GlobalPos;

        let dug = BlockPos::new(3, 63, -5);
        let mut fresh = brain();
        assert!(!already_explored(&fresh.memories, dug));
        fresh.memories.set(
            SnifferExploredPositions,
            MemoryValue::GlobalPosList(vec![GlobalPos {
                dimension: "minecraft:overworld".to_owned(),
                pos: dug,
            }]),
        );
        assert!(already_explored(&fresh.memories, dug));
        assert!(!already_explored(&fresh.memories, BlockPos::new(4, 63, -5)));

        let mut other_dimension = brain();
        other_dimension.memories.set(
            SnifferExploredPositions,
            MemoryValue::GlobalPosList(vec![GlobalPos {
                dimension: "minecraft:the_nether".to_owned(),
                pos: dug,
            }]),
        );
        assert!(!already_explored(&other_dimension.memories, dug));
    }
}
