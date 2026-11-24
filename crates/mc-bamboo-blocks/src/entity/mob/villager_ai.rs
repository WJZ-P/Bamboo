//! 村民的大脑：官方 `Villager.BRAIN_PROVIDER` 与 `VillagerGoalPackages` 的 11 个包。
//!
//! 模板按「幼年 / 成年默认 / 成年农民」三份 `OnceLock<Arc<BrainTemplate>>` 缓存共享；
//! 农民的 `WORK` 包里 `WorkAtComposter`、`HarvestFarmland`（权重 2）和 `UseBonemeal`
//! （权重 4）与其余职业（`WorkAtPoi`、权重 5 和 7）不同，所以拆出单独模板。
//!
//! 期 3 完整挂载全部 11 个包的结构表（优先级、活动、类名、入口条件、退出清理记忆）和
//! 9 个传感器，并真实现核心移动、恐慌、冷静、日程切换、袭击随机数抽取与环顾；其余
//! POI / 职业 / 睡觉 / 繁殖 / 交易行为先以 [`Stub`] 占位，第 4 期起逐项接通。
//!
//! Villager brain: vanilla `Villager.BRAIN_PROVIDER` and the 11 packages of
//! `VillagerGoalPackages`. Three cached `BrainTemplate`s cover baby, default adult and
//! farmer adult villagers. Phase 3 mounts the complete structure table and 9 sensors,
//! implements the core movement, panic, calm-down, schedule, raid RNG and look
//! behaviours, and stubs Phase 4+ behaviours via [`Stub`].

use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_data::{spawning::MobCategory, villager as facts};

use super::{
    Mob, MobType,
    brain::{
        Activity, ActivityData, ActivitySchedule, Brain, BrainTemplate, TemplateCache,
        behavior::{Class, Cx, Gate, Kind, MoveToTargetSink, Speed},
        current_position,
        memory::{
            MemoryStatus::{self, Registered, ValueAbsent, ValuePresent},
            MemoryType::{self, *},
            MemoryValue, PositionTracker, WalkTarget,
        },
        sensor::{self, LivingView, SensorKind},
        template_cache,
    },
    goals,
};
use crate::entity::{Entity, ctx::BlockView, ctx::EntityCtx};

/// `Villager.SPEED_MODIFIER`。
const SPEED_MODIFIER: f32 = 0.5;

/// `Villager.BRAIN_PROVIDER` 的 9 个传感器，严格按官方登记顺序。
///
/// The 9 sensors of `Villager.BRAIN_PROVIDER`, in exact vanilla registration order.
pub const VILLAGER_SENSORS: [SensorKind; 9] = [
    SensorKind::NearestLivingEntities,
    SensorKind::NearestPlayers,
    SensorKind::NearestItems,
    SensorKind::NearestBed,
    SensorKind::HurtBy,
    SensorKind::VillagerHostiles,
    SensorKind::VillagerBabies,
    SensorKind::SecondaryPois,
    SensorKind::GolemDetected,
];

static ADULT_DEFAULT_TEMPLATE: TemplateCache = template_cache();
static ADULT_FARMER_TEMPLATE: TemplateCache = template_cache();
static BABY_TEMPLATE: TemplateCache = template_cache();

/// `SetEntityLookTarget` 的目标过滤器。
///
/// Target filter for `SetEntityLookTarget`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookFilter {
    Mob(MobType),
    Player,
    Category(MobCategory),
}

impl LookFilter {
    fn matches(self, view: &LivingView) -> bool {
        match self {
            Self::Mob(kind) => view.mob_type == Some(kind),
            Self::Player => view.player,
            Self::Category(category) => view
                .mob_type
                .is_some_and(|kind| kind.mob_category() == category),
        }
    }
}

/// `AcquirePoi` 占位的槽位种类：只保留官方的排期计时与世界随机数抽取，真正认领 POI 留到期 4。
///
/// Slot kind for the `AcquirePoi` scheduling stub: preserves vanilla's scheduling timer
/// and level RNG draws while deferring actual POI claiming to Phase 4.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoiAcquireSlot {
    JobSite,
    Home,
    MeetingPoint,
}

impl PoiAcquireSlot {
    const fn only_if_adult(self) -> bool {
        matches!(self, Self::JobSite | Self::MeetingPoint)
    }

    const fn required(self) -> &'static [(MemoryType, MemoryStatus)] {
        match self {
            Self::JobSite => &[(JobSite, ValueAbsent), (PotentialJobSite, ValueAbsent)],
            Self::Home => &[(Home, ValueAbsent)],
            Self::MeetingPoint => &[(MeetingPoint, ValueAbsent)],
        }
    }
}

/// 期 4 以后再实现的行为占位：保持官方的 `Class`、必需记忆表与对拍类名，永不启动。
///
/// Placeholders for behaviours deferred to Phase 4+: preserve vanilla's `Class`,
/// required memories and oracle class name, and never start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stub {
    InteractWithDoor,
    ValidateJobSite,
    ValidatePotentialJobSite,
    ValidateHome,
    ValidateMeetingPoint,
    PoiCompetitorScan,
    LookAndFollowTradingPlayerSink,
    GoToWantedItem,
    GoToPotentialJobSite,
    YieldJobSite,
    AssignProfessionFromJobSite,
    ResetProfession,
    WorkAtComposter,
    WorkAtPoi,
    StrollAroundJobSite,
    StrollToJobSite,
    StrollToSecondaryJobSite,
    HarvestFarmland,
    UseBonemeal,
    ShowTradesToPlayer,
    SetLookAndInteractPlayer,
    SetWalkTargetFromJobSite,
    SetWalkTargetFromHome,
    SetWalkTargetFromMeetingPoint,
    GiveGiftToHero,
    PlayTagWithOtherKids,
    InteractWithVillager,
    InteractWithBreedVillager,
    InteractWithCat,
    JumpOnBed,
    SleepInBed,
    SetClosestHomeAsWalkTarget,
    InsideBrownianWalk,
    MeetTriggerGate,
    TradeWithVillager,
    VillagerMakeLove,
    RingBell,
    PreRaidTriggerGate,
    ResetRaidStatus,
    RaidCelebrateTriggerGate,
    CelebrateVillagersSurvivedRaid,
    RaidLocateHidingPlace,
    SetHiddenState,
    LocateHidingPlace,
}

const REQ_DOOR: &[(MemoryType, MemoryStatus)] = &[
    (Path, ValuePresent),
    (DoorsToClose, Registered),
    (NearestLivingEntities, Registered),
];
const REQ_JOB_PRESENT: &[(MemoryType, MemoryStatus)] = &[(JobSite, ValuePresent)];
const REQ_POTENTIAL_JOB_PRESENT: &[(MemoryType, MemoryStatus)] =
    &[(PotentialJobSite, ValuePresent)];
const REQ_HOME_PRESENT: &[(MemoryType, MemoryStatus)] = &[(Home, ValuePresent)];
const REQ_MEETING_PRESENT: &[(MemoryType, MemoryStatus)] = &[(MeetingPoint, ValuePresent)];
const REQ_POI_COMPETITOR: &[(MemoryType, MemoryStatus)] = &[
    (JobSite, ValuePresent),
    (NearestLivingEntities, ValuePresent),
];
const REQ_TRADING_SINK: &[(MemoryType, MemoryStatus)] =
    &[(WalkTarget, Registered), (LookTarget, Registered)];
const REQ_WANTED_ITEM: &[(MemoryType, MemoryStatus)] = &[
    (WalkTarget, ValueAbsent),
    (LookTarget, Registered),
    (NearestVisibleWantedItem, ValuePresent),
    (ItemPickupCooldownTicks, Registered),
];
const REQ_YIELD_JOB: &[(MemoryType, MemoryStatus)] = &[
    (PotentialJobSite, ValuePresent),
    (JobSite, ValueAbsent),
    (NearestLivingEntities, ValuePresent),
    (WalkTarget, Registered),
    (LookTarget, Registered),
];
const REQ_ASSIGN_PROF: &[(MemoryType, MemoryStatus)] =
    &[(PotentialJobSite, ValuePresent), (JobSite, Registered)];
const REQ_RESET_PROF: &[(MemoryType, MemoryStatus)] = &[(JobSite, ValueAbsent)];
const REQ_WORK_AT_POI: &[(MemoryType, MemoryStatus)] =
    &[(JobSite, ValuePresent), (LookTarget, Registered)];
const REQ_STROLL_JOB: &[(MemoryType, MemoryStatus)] =
    &[(WalkTarget, Registered), (JobSite, ValuePresent)];
const REQ_STROLL_SECONDARY: &[(MemoryType, MemoryStatus)] = &[
    (WalkTarget, Registered),
    (SecondaryJobSite, ValuePresent),
    (JobSite, ValuePresent),
];
const REQ_HARVEST_FARMLAND: &[(MemoryType, MemoryStatus)] = &[
    (LookTarget, ValueAbsent),
    (WalkTarget, ValueAbsent),
    (SecondaryJobSite, ValuePresent),
];
const REQ_USE_BONEMEAL: &[(MemoryType, MemoryStatus)] =
    &[(LookTarget, ValueAbsent), (WalkTarget, ValueAbsent)];
const REQ_INTERACTION_PRESENT: &[(MemoryType, MemoryStatus)] = &[(InteractionTarget, ValuePresent)];
const REQ_LOOK_INTERACT: &[(MemoryType, MemoryStatus)] = &[
    (LookTarget, Registered),
    (InteractionTarget, ValueAbsent),
    (NearestVisibleLivingEntities, ValuePresent),
];
const REQ_WALK_FROM_JOB: &[(MemoryType, MemoryStatus)] = &[
    (CantReachWalkTargetSince, Registered),
    (WalkTarget, ValueAbsent),
    (JobSite, ValuePresent),
];
const REQ_WALK_FROM_HOME: &[(MemoryType, MemoryStatus)] = &[
    (CantReachWalkTargetSince, Registered),
    (WalkTarget, ValueAbsent),
    (Home, ValuePresent),
];
const REQ_WALK_FROM_MEETING: &[(MemoryType, MemoryStatus)] = &[
    (CantReachWalkTargetSince, Registered),
    (WalkTarget, ValueAbsent),
    (MeetingPoint, ValuePresent),
];
const REQ_GIVE_GIFT: &[(MemoryType, MemoryStatus)] = &[
    (WalkTarget, Registered),
    (LookTarget, Registered),
    (InteractionTarget, Registered),
    (NearestVisiblePlayer, ValuePresent),
];
const REQ_PLAY_TAG: &[(MemoryType, MemoryStatus)] = &[
    (VisibleVillagerBabies, ValuePresent),
    (WalkTarget, ValueAbsent),
    (LookTarget, Registered),
    (InteractionTarget, Registered),
];
const REQ_INTERACT_TARGET: &[(MemoryType, MemoryStatus)] = &[
    (InteractionTarget, Registered),
    (LookTarget, Registered),
    (WalkTarget, ValueAbsent),
    (NearestVisibleLivingEntities, ValuePresent),
];
const REQ_INTERACT_BREED: &[(MemoryType, MemoryStatus)] = &[
    (BreedTarget, Registered),
    (LookTarget, Registered),
    (WalkTarget, ValueAbsent),
    (NearestVisibleLivingEntities, ValuePresent),
];
const REQ_JUMP_ON_BED: &[(MemoryType, MemoryStatus)] =
    &[(NearestBed, ValuePresent), (WalkTarget, ValueAbsent)];
const REQ_SLEEP_IN_BED: &[(MemoryType, MemoryStatus)] = &[
    (Home, ValuePresent),
    (LastWoken, Registered),
    (LastSlept, Registered),
    (WalkTarget, Registered),
    (CantReachWalkTargetSince, Registered),
];
const REQ_CLOSEST_HOME: &[(MemoryType, MemoryStatus)] =
    &[(WalkTarget, ValueAbsent), (Home, ValueAbsent)];
const REQ_WALK_ABSENT: &[(MemoryType, MemoryStatus)] = &[(WalkTarget, ValueAbsent)];
const REQ_TRADE_VILLAGER: &[(MemoryType, MemoryStatus)] = &[
    (InteractionTarget, ValuePresent),
    (NearestVisibleLivingEntities, ValuePresent),
];
const REQ_MAKE_LOVE: &[(MemoryType, MemoryStatus)] = &[
    (BreedTarget, ValuePresent),
    (NearestVisibleLivingEntities, ValuePresent),
];
const REQ_LOCATE_HIDING: &[(MemoryType, MemoryStatus)] = &[
    (WalkTarget, ValueAbsent),
    (Home, Registered),
    (HidingPlace, Registered),
    (Path, Registered),
    (LookTarget, Registered),
    (BreedTarget, Registered),
    (InteractionTarget, Registered),
];
const REQ_SET_HIDDEN: &[(MemoryType, MemoryStatus)] =
    &[(HidingPlace, ValuePresent), (HeardBellTime, ValuePresent)];

impl Stub {
    const fn class(self) -> Class {
        match self {
            Self::LookAndFollowTradingPlayerSink => Class::Timed {
                min: i32::MAX,
                max: i32::MAX,
            },
            Self::GoToPotentialJobSite => Class::Timed {
                min: 1200,
                max: 1200,
            },
            Self::WorkAtComposter | Self::WorkAtPoi => Class::Timed { min: 60, max: 60 },
            Self::HarvestFarmland => Class::Timed { min: 200, max: 200 },
            Self::UseBonemeal => Class::Timed { min: 60, max: 60 },
            Self::ShowTradesToPlayer => Class::Timed {
                min: 400,
                max: 1600,
            },
            Self::GiveGiftToHero => Class::Timed { min: 100, max: 100 },
            Self::JumpOnBed => Class::Timed { min: 60, max: 60 },
            Self::SleepInBed => Class::Timed { min: 60, max: 60 },
            Self::TradeWithVillager => Class::Timed { min: 60, max: 60 },
            Self::VillagerMakeLove => Class::Timed { min: 350, max: 350 },
            Self::CelebrateVillagersSurvivedRaid => Class::Timed { min: 600, max: 600 },
            _ => Class::OneShot,
        }
    }

    const fn required(self) -> &'static [(MemoryType, MemoryStatus)] {
        match self {
            Self::InteractWithDoor => REQ_DOOR,
            Self::ValidateJobSite => REQ_JOB_PRESENT,
            Self::ValidatePotentialJobSite => REQ_POTENTIAL_JOB_PRESENT,
            Self::ValidateHome => REQ_HOME_PRESENT,
            Self::ValidateMeetingPoint => REQ_MEETING_PRESENT,
            Self::PoiCompetitorScan => REQ_POI_COMPETITOR,
            Self::LookAndFollowTradingPlayerSink => REQ_TRADING_SINK,
            Self::GoToWantedItem => REQ_WANTED_ITEM,
            Self::GoToPotentialJobSite => REQ_POTENTIAL_JOB_PRESENT,
            Self::YieldJobSite => REQ_YIELD_JOB,
            Self::AssignProfessionFromJobSite => REQ_ASSIGN_PROF,
            Self::ResetProfession => REQ_RESET_PROF,
            Self::WorkAtComposter | Self::WorkAtPoi => REQ_WORK_AT_POI,
            Self::StrollAroundJobSite | Self::StrollToJobSite => REQ_STROLL_JOB,
            Self::StrollToSecondaryJobSite => REQ_STROLL_SECONDARY,
            Self::HarvestFarmland => REQ_HARVEST_FARMLAND,
            Self::UseBonemeal => REQ_USE_BONEMEAL,
            Self::ShowTradesToPlayer => REQ_INTERACTION_PRESENT,
            Self::SetLookAndInteractPlayer => REQ_LOOK_INTERACT,
            Self::SetWalkTargetFromJobSite => REQ_WALK_FROM_JOB,
            Self::SetWalkTargetFromHome => REQ_WALK_FROM_HOME,
            Self::SetWalkTargetFromMeetingPoint => REQ_WALK_FROM_MEETING,
            Self::GiveGiftToHero => REQ_GIVE_GIFT,
            Self::PlayTagWithOtherKids => REQ_PLAY_TAG,
            Self::InteractWithVillager | Self::InteractWithCat => REQ_INTERACT_TARGET,
            Self::InteractWithBreedVillager => REQ_INTERACT_BREED,
            Self::JumpOnBed => REQ_JUMP_ON_BED,
            Self::SleepInBed => REQ_SLEEP_IN_BED,
            Self::SetClosestHomeAsWalkTarget => REQ_CLOSEST_HOME,
            Self::InsideBrownianWalk => REQ_WALK_ABSENT,
            Self::MeetTriggerGate
            | Self::PreRaidTriggerGate
            | Self::ResetRaidStatus
            | Self::RaidCelebrateTriggerGate
            | Self::CelebrateVillagersSurvivedRaid => &[],
            Self::TradeWithVillager => REQ_TRADE_VILLAGER,
            Self::VillagerMakeLove => REQ_MAKE_LOVE,
            Self::RingBell => REQ_MEETING_PRESENT,
            Self::RaidLocateHidingPlace | Self::LocateHidingPlace => REQ_LOCATE_HIDING,
            Self::SetHiddenState => REQ_SET_HIDDEN,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::LookAndFollowTradingPlayerSink => "LookAndFollowTradingPlayerSink",
            Self::GoToPotentialJobSite => "GoToPotentialJobSite",
            Self::WorkAtComposter => "WorkAtComposter",
            Self::WorkAtPoi => "WorkAtPoi",
            Self::HarvestFarmland => "HarvestFarmland",
            Self::UseBonemeal => "UseBonemeal",
            Self::ShowTradesToPlayer => "ShowTradesToPlayer",
            Self::GiveGiftToHero => "GiveGiftToHero",
            Self::JumpOnBed => "JumpOnBed",
            Self::SleepInBed => "SleepInBed",
            Self::TradeWithVillager => "TradeWithVillager",
            Self::VillagerMakeLove => "VillagerMakeLove",
            Self::CelebrateVillagersSurvivedRaid => "CelebrateVillagersSurvivedRaid",
            _ => "OneShot",
        }
    }
}

/// 村民专属行为与未接通占位。
///
/// Villager-specific behaviours and Phase 4+ placeholders.
#[derive(Debug, Clone, PartialEq)]
pub enum Behavior {
    /// `VillagerPanicTrigger`：`Behavior(60, 60)`，受伤或身边有敌对时切到 `PANIC`。
    PanicTrigger,
    /// `VillagerCalmDown`：`OneShot`，无伤害、无敌对且攻击者超出 6 格时清伤害记忆并回日程。
    CalmDown,
    /// `UpdateActivityFromSchedule`：`OneShot`，按 20 刻节流刷新日程活动。
    UpdateActivityFromSchedule,
    /// `SetWalkTargetAwayFrom.entity(memory, speed, desiredDist, checkNoWalkTarget)`。
    WalkAwayFromEntity {
        memory: MemoryType,
        speed: f32,
        desired_distance: i32,
        check_no_walk_target: bool,
    },
    /// `VillageBoundRandomStroll.create(speed, maxXZ, maxY)`。
    VillageBoundRandomStroll { speed: f32, max_xz: i32, max_y: i32 },
    /// `GoToClosestVillage.create(speed, closeEnough)`。
    GoToClosestVillage { speed: f32, close_enough: i32 },
    /// `SetRaidStatus`：`OneShot`，每刻抽 `level.random.nextInt(20)`，保持世界随机序列对齐。
    SetRaidStatus,
    /// `SetEntityLookTarget.create(filter, maxDist)`。
    SetEntityLookTarget { filter: LookFilter, max_dist: f32 },
    /// `WakeUp`：`OneShot`，非 `REST` 活动时叫醒正在睡觉的村民。
    WakeUp,
    /// `ReactToBell`：`OneShot`，听到钟声且无袭击时切到 `HIDE`。
    ReactToBell,
    /// `AcquirePoi` 排期占位：按官方计时抽取 `level.random.nextInt(20)`，真正占领 POI 在期 4。
    AcquirePoiStub {
        slot: PoiAcquireSlot,
        next_scheduled_start: i64,
    },
    /// 期 4 以后行为占位（永不启动）。
    Unavailable(Stub),
}

const REQ_CALM_DOWN: &[(MemoryType, MemoryStatus)] = &[
    (HurtBy, Registered),
    (HurtByEntity, Registered),
    (NearestHostile, Registered),
];
const REQ_AWAY_HOSTILE: &[(MemoryType, MemoryStatus)] =
    &[(WalkTarget, Registered), (NearestHostile, ValuePresent)];
const REQ_AWAY_HURT_BY: &[(MemoryType, MemoryStatus)] =
    &[(WalkTarget, Registered), (HurtByEntity, ValuePresent)];
const REQ_LOOK_ENTITY: &[(MemoryType, MemoryStatus)] = &[
    (LookTarget, ValueAbsent),
    (NearestVisibleLivingEntities, ValuePresent),
];
const REQ_HEARD_BELL: &[(MemoryType, MemoryStatus)] = &[(HeardBellTime, ValuePresent)];

impl Behavior {
    pub(crate) fn class(&self) -> Class {
        match self {
            Self::PanicTrigger => Class::Timed { min: 60, max: 60 },
            Self::Unavailable(stub) => stub.class(),
            _ => Class::OneShot,
        }
    }

    pub(crate) fn required(&self) -> &[(MemoryType, MemoryStatus)] {
        match self {
            Self::PanicTrigger
            | Self::UpdateActivityFromSchedule
            | Self::SetRaidStatus
            | Self::WakeUp => &[],
            Self::CalmDown => REQ_CALM_DOWN,
            Self::WalkAwayFromEntity { memory, .. } => match memory {
                NearestHostile => REQ_AWAY_HOSTILE,
                _ => REQ_AWAY_HURT_BY,
            },
            Self::VillageBoundRandomStroll { .. } | Self::GoToClosestVillage { .. } => {
                REQ_WALK_ABSENT
            }
            Self::SetEntityLookTarget { .. } => REQ_LOOK_ENTITY,
            Self::ReactToBell => REQ_HEARD_BELL,
            Self::AcquirePoiStub { slot, .. } => slot.required(),
            Self::Unavailable(stub) => stub.required(),
        }
    }

    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::PanicTrigger => "VillagerPanicTrigger",
            Self::Unavailable(stub) => stub.name(),
            _ => "OneShot",
        }
    }

    pub(crate) fn extra_start(&mut self, _cx: &mut Cx) -> bool {
        match self {
            Self::PanicTrigger => true,
            Self::Unavailable(_) => false,
            _ => false,
        }
    }

    pub(crate) fn start(&mut self, cx: &mut Cx) {
        if let Self::PanicTrigger = self
            && (is_hurt(cx) || has_hostile(cx))
        {
            if !cx.is_active(Activity::Panic) {
                cx.memories.erase(Path);
                cx.memories.erase(WalkTarget);
                cx.memories.erase(LookTarget);
                cx.memories.erase(BreedTarget);
                cx.memories.erase(InteractionTarget);
            }
            cx.set_active_activity_if_possible(Activity::Panic);
        }
    }

    pub(crate) fn can_still_use(&mut self, cx: &mut Cx) -> bool {
        match self {
            Self::PanicTrigger => is_hurt(cx) || has_hostile(cx),
            _ => false,
        }
    }

    pub(crate) fn tick(&mut self, _cx: &mut Cx) {
        // `VillagerPanicTrigger.tick`：每 100 刻调 `spawnGolemIfNeeded(3)`，铁傀儡生成在期 6。
    }

    pub(crate) fn stop(&mut self, _cx: &mut Cx) {}

    pub(crate) fn trigger(&mut self, cx: &mut Cx) -> bool {
        match self {
            Self::UpdateActivityFromSchedule => {
                cx.update_activity_from_schedule();
                true
            }
            Self::CalmDown => {
                let self_pos = cx.me.position;
                let hurt_entity_close = cx
                    .memories
                    .entity(HurtByEntity)
                    .and_then(|uid| sensor::living(cx, uid))
                    .is_some_and(|view| view.position.distance_to_sqr(self_pos) <= 36.0);
                let stay_panicked =
                    cx.memories.has(HurtBy) || cx.memories.has(NearestHostile) || hurt_entity_close;
                if !stay_panicked {
                    cx.memories.erase(HurtBy);
                    cx.memories.erase(HurtByEntity);
                    cx.update_activity_from_schedule();
                }
                true
            }
            Self::WalkAwayFromEntity {
                memory,
                speed,
                desired_distance,
                check_no_walk_target,
            } => {
                let walk = cx.memories.walk_target();
                if walk.is_some() && !*check_no_walk_target {
                    return false;
                }
                let Some(uid) = cx.memories.entity(*memory) else {
                    return false;
                };
                let Some(view) = sensor::living(cx, uid) else {
                    return false;
                };
                let pos = cx.me.position;
                let target_pos = view.position;
                let limit = f64::from(*desired_distance);
                if pos.distance_to_sqr(target_pos) >= limit * limit {
                    return false;
                }
                if let Some(existing) = walk
                    && existing.speed == *speed
                    && let Some(existing_pos) = current_position(cx, existing.target)
                {
                    let to_walk = existing_pos.subtract(pos);
                    let to_threat = target_pos.subtract(pos);
                    let dot =
                        to_walk.x * to_threat.x + to_walk.y * to_threat.y + to_walk.z * to_threat.z;
                    if dot < 0.0 {
                        return false;
                    }
                }
                for _ in 0..10 {
                    if let Some(away) = goals::land_random_pos_away(
                        cx.me,
                        cx.mob,
                        cx.ctx.blocks(),
                        16,
                        7,
                        target_pos,
                    ) {
                        cx.memories.set(
                            WalkTarget,
                            MemoryValue::Walk(WalkTarget::to_point(away, *speed, 0)),
                        );
                        break;
                    }
                }
                true
            }
            Self::VillageBoundRandomStroll {
                speed,
                max_xz,
                max_y,
            } => {
                let block_pos = cx.me.block_position();
                let blocks = cx.ctx.blocks();
                let pos = if blocks.poi_is_close_to_village(block_pos, 1) {
                    goals::land_random_pos(cx.me, cx.mob, blocks, *max_xz, *max_y)
                } else {
                    let current = section_of(block_pos);
                    let closest = find_section_closest_to_village(blocks, current, 2);
                    if closest != current {
                        let center = section_center(closest);
                        let towards = Vec3::new(
                            f64::from(center.x) + 0.5,
                            f64::from(center.y),
                            f64::from(center.z) + 0.5,
                        );
                        goals::default_random_pos_towards(
                            cx.me,
                            cx.mob,
                            blocks,
                            *max_xz,
                            *max_y,
                            towards,
                            std::f32::consts::FRAC_PI_2,
                        )
                    } else {
                        goals::land_random_pos(cx.me, cx.mob, blocks, *max_xz, *max_y)
                    }
                };
                cx.memories.set_or_erase(
                    WalkTarget,
                    pos.map(|at| MemoryValue::Walk(WalkTarget::to_point(at, *speed, 0))),
                );
                true
            }
            Self::GoToClosestVillage {
                speed,
                close_enough,
            } => {
                let block_pos = cx.me.block_position();
                let blocks = cx.ctx.blocks();
                if blocks.poi_is_close_to_village(block_pos, 1) {
                    return false;
                }
                let current_dist = blocks.poi_sections_to_village(block_pos);
                let mut best: Option<Vec3> = None;
                for _ in 0..5 {
                    let candidate =
                        goals::land_random_pos_weighted(cx.me, cx.mob, blocks, 15, 7, |pos| {
                            -f64::from(blocks.poi_sections_to_village(pos))
                        });
                    let Some(candidate) = candidate else {
                        continue;
                    };
                    let dist = blocks.poi_sections_to_village(BlockPos::containing(candidate));
                    if dist < current_dist {
                        best = Some(candidate);
                        break;
                    } else if dist == current_dist {
                        best = Some(candidate);
                    }
                }
                if let Some(target) = best {
                    cx.memories.set(
                        WalkTarget,
                        MemoryValue::Walk(WalkTarget::to_point(target, *speed, *close_enough)),
                    );
                }
                true
            }
            Self::SetRaidStatus => {
                // `SetRaidStatus`：即使 Bamboo 没有袭击系统，每刻也必须抽一次
                // `level.random.nextInt(20)`，否则整个世界的随机序列会错位。
                // `SetRaidStatus`: even without raids, must draw `level.random.nextInt(20)`
                // every tick so the level random sequence stays in sync.
                cx.level_next_int(20) == 0
            }
            Self::SetEntityLookTarget { filter, max_dist } => {
                let max_sqr = f64::from(*max_dist * *max_dist);
                let self_pos = cx.me.position;
                let passengers = cx.mob.passengers.clone();
                let target = sensor::find_closest_visible(cx, &mut |view| {
                    filter.matches(view)
                        && view.position.distance_to_sqr(self_pos) <= max_sqr
                        && !passengers.contains(&view.uid)
                });
                let Some(uid) = target else {
                    return false;
                };
                cx.memories.set(
                    LookTarget,
                    MemoryValue::Look(PositionTracker::Entity {
                        uid,
                        track_eye: true,
                        target_eye: false,
                    }),
                );
                true
            }
            Self::WakeUp => {
                if cx.is_active(Activity::Rest) || !cx.mob.villager.sleeping {
                    return false;
                }
                cx.mob.villager.sleeping = false;
                cx.memories.set(LastWoken, MemoryValue::Long(cx.time));
                true
            }
            Self::ReactToBell => {
                // 无袭击时听到钟声直接切到 `HIDE`。
                // Without an active raid, hearing the bell switches to `HIDE`.
                cx.set_active_activity_if_possible(Activity::Hide);
                true
            }
            Self::AcquirePoiStub {
                slot,
                next_scheduled_start,
            } => {
                if slot.only_if_adult() && cx.mob.is_baby() {
                    return false;
                }
                if *next_scheduled_start == 0 {
                    *next_scheduled_start = cx.time + i64::from(cx.level_next_int(20));
                    return false;
                }
                if cx.time < *next_scheduled_start {
                    return false;
                }
                *next_scheduled_start = cx.time + 20 + i64::from(cx.level_next_int(20));
                true
            }
            Self::PanicTrigger | Self::Unavailable(_) => false,
        }
    }
}

fn is_hurt(cx: &Cx) -> bool {
    cx.memories.has(HurtBy)
}

fn has_hostile(cx: &Cx) -> bool {
    cx.memories.has(NearestHostile)
}

fn section_of(pos: BlockPos) -> (i32, i32, i32) {
    (pos.x >> 4, pos.y >> 4, pos.z >> 4)
}

fn section_center((sx, sy, sz): (i32, i32, i32)) -> BlockPos {
    BlockPos::new((sx << 4) + 8, (sy << 4) + 8, (sz << 4) + 8)
}

/// `BehaviorUtils.findSectionClosestToVillage`：按 `SectionPos.cube`（`Cursor3D` 的
/// `z -> y -> x` 序）找比当前更靠近村庄、距离最小的区段；没有更近的就返回当前区段。
///
/// `BehaviorUtils.findSectionClosestToVillage`: walks `SectionPos.cube` in `Cursor3D`
/// order (`z -> y -> x`) for the section strictly closer to a village with minimum
/// distance, or returns the starting section.
fn find_section_closest_to_village(
    blocks: &dyn BlockView,
    origin: (i32, i32, i32),
    radius: i32,
) -> (i32, i32, i32) {
    let mut best_dist = blocks.poi_sections_to_village(section_center(origin));
    let mut best = origin;
    let (ox, oy, oz) = origin;
    for sz in (oz - radius)..=(oz + radius) {
        for sy in (oy - radius)..=(oy + radius) {
            for sx in (ox - radius)..=(ox + radius) {
                let candidate = (sx, sy, sz);
                let dist = blocks.poi_sections_to_village(section_center(candidate));
                if dist < best_dist {
                    best_dist = dist;
                    best = candidate;
                }
            }
        }
    }
    best
}

fn stub(which: Stub) -> Kind {
    Kind::Villager(Behavior::Unavailable(which))
}

fn look_target(filter: LookFilter, max_dist: f32) -> Kind {
    Kind::Villager(Behavior::SetEntityLookTarget { filter, max_dist })
}

fn full_look_behavior() -> (i32, Kind) {
    (
        5,
        Kind::Gate(Box::new(Gate::run_one(
            Vec::new(),
            vec![
                (look_target(LookFilter::Mob(MobType::Cat), 8.0), 8),
                (look_target(LookFilter::Mob(MobType::Villager), 8.0), 2),
                (look_target(LookFilter::Player, 8.0), 2),
                (
                    look_target(LookFilter::Category(MobCategory::Creature), 8.0),
                    1,
                ),
                (
                    look_target(LookFilter::Category(MobCategory::WaterCreature), 8.0),
                    1,
                ),
                (
                    look_target(LookFilter::Category(MobCategory::Axolotls), 8.0),
                    1,
                ),
                (
                    look_target(
                        LookFilter::Category(MobCategory::UndergroundWaterCreature),
                        8.0,
                    ),
                    1,
                ),
                (
                    look_target(LookFilter::Category(MobCategory::WaterAmbient), 8.0),
                    1,
                ),
                (
                    look_target(LookFilter::Category(MobCategory::Monster), 8.0),
                    1,
                ),
                (Kind::DoNothing { min: 30, max: 60 }, 2),
            ],
        ))),
    )
}

fn minimal_look_behavior() -> (i32, Kind) {
    (
        5,
        Kind::Gate(Box::new(Gate::run_one(
            Vec::new(),
            vec![
                (look_target(LookFilter::Mob(MobType::Villager), 8.0), 2),
                (look_target(LookFilter::Player, 8.0), 2),
                (Kind::DoNothing { min: 30, max: 60 }, 8),
            ],
        ))),
    )
}

fn core_package(speed: f32) -> ActivityData {
    let _ = speed;
    ActivityData::with_priorities(
        Activity::Core,
        vec![
            (0, Kind::Swim { chance: 0.8 }),
            (0, stub(Stub::InteractWithDoor)),
            (0, Kind::LookAtTargetSink { min: 45, max: 90 }),
            (0, Kind::Villager(Behavior::PanicTrigger)),
            (0, Kind::Villager(Behavior::WakeUp)),
            (0, Kind::Villager(Behavior::ReactToBell)),
            (0, Kind::Villager(Behavior::SetRaidStatus)),
            (0, stub(Stub::ValidateJobSite)),
            (0, stub(Stub::ValidatePotentialJobSite)),
            (1, Kind::MoveToTargetSink(MoveToTargetSink::new(false))),
            (2, stub(Stub::PoiCompetitorScan)),
            (3, stub(Stub::LookAndFollowTradingPlayerSink)),
            (5, stub(Stub::GoToWantedItem)),
            (
                6,
                Kind::Villager(Behavior::AcquirePoiStub {
                    slot: PoiAcquireSlot::JobSite,
                    next_scheduled_start: 0,
                }),
            ),
            (7, stub(Stub::GoToPotentialJobSite)),
            (8, stub(Stub::YieldJobSite)),
            (
                10,
                Kind::Villager(Behavior::AcquirePoiStub {
                    slot: PoiAcquireSlot::Home,
                    next_scheduled_start: 0,
                }),
            ),
            (
                10,
                Kind::Villager(Behavior::AcquirePoiStub {
                    slot: PoiAcquireSlot::MeetingPoint,
                    next_scheduled_start: 0,
                }),
            ),
            (10, stub(Stub::AssignProfessionFromJobSite)),
            (10, stub(Stub::ResetProfession)),
        ],
    )
}

fn work_package(is_farmer: bool, _speed: f32) -> ActivityData {
    let work_at_poi = if is_farmer {
        stub(Stub::WorkAtComposter)
    } else {
        stub(Stub::WorkAtPoi)
    };
    let harvest_weight = if is_farmer { 2 } else { 5 };
    let bonemeal_weight = if is_farmer { 4 } else { 7 };
    ActivityData::with_priorities(
        Activity::Work,
        vec![
            minimal_look_behavior(),
            (
                5,
                Kind::Gate(Box::new(Gate::run_one(
                    Vec::new(),
                    vec![
                        (work_at_poi, 7),
                        (stub(Stub::StrollAroundJobSite), 2),
                        (stub(Stub::StrollToJobSite), 5),
                        (stub(Stub::StrollToSecondaryJobSite), 5),
                        (stub(Stub::HarvestFarmland), harvest_weight),
                        (stub(Stub::UseBonemeal), bonemeal_weight),
                    ],
                ))),
            ),
            (10, stub(Stub::ShowTradesToPlayer)),
            (10, stub(Stub::SetLookAndInteractPlayer)),
            (2, stub(Stub::SetWalkTargetFromJobSite)),
            (3, stub(Stub::GiveGiftToHero)),
            (99, Kind::Villager(Behavior::UpdateActivityFromSchedule)),
        ],
    )
    .when(vec![(JobSite, ValuePresent)])
}

fn play_package(speed: f32) -> ActivityData {
    ActivityData::with_priorities(
        Activity::Play,
        vec![
            (
                0,
                Kind::MoveToTargetSink(MoveToTargetSink::with_duration(80, 120, false)),
            ),
            full_look_behavior(),
            (5, stub(Stub::PlayTagWithOtherKids)),
            (
                5,
                Kind::Gate(Box::new(Gate::run_one(
                    vec![(VisibleVillagerBabies, ValueAbsent)],
                    vec![
                        (stub(Stub::InteractWithVillager), 2),
                        (stub(Stub::InteractWithCat), 1),
                        (
                            Kind::Villager(Behavior::VillageBoundRandomStroll {
                                speed,
                                max_xz: 10,
                                max_y: 7,
                            }),
                            1,
                        ),
                        (
                            Kind::WalkTargetFromLook {
                                speed: Speed::fixed(speed),
                                close_enough: 2,
                            },
                            1,
                        ),
                        (stub(Stub::JumpOnBed), 2),
                        (Kind::DoNothing { min: 20, max: 40 }, 2),
                    ],
                ))),
            ),
            (99, Kind::Villager(Behavior::UpdateActivityFromSchedule)),
        ],
    )
}

fn rest_package(speed: f32) -> ActivityData {
    ActivityData::with_priorities(
        Activity::Rest,
        vec![
            (2, stub(Stub::SetWalkTargetFromHome)),
            (3, stub(Stub::ValidateHome)),
            (3, stub(Stub::SleepInBed)),
            (
                5,
                Kind::Gate(Box::new(Gate::run_one(
                    vec![(Home, ValueAbsent)],
                    vec![
                        (stub(Stub::SetClosestHomeAsWalkTarget), 1),
                        (stub(Stub::InsideBrownianWalk), 4),
                        (
                            Kind::Villager(Behavior::GoToClosestVillage {
                                speed,
                                close_enough: 4,
                            }),
                            2,
                        ),
                        (Kind::DoNothing { min: 20, max: 40 }, 2),
                    ],
                ))),
            ),
            minimal_look_behavior(),
            (99, Kind::Villager(Behavior::UpdateActivityFromSchedule)),
        ],
    )
}

fn meet_package(_speed: f32) -> ActivityData {
    ActivityData::with_priorities(
        Activity::Meet,
        vec![
            (2, stub(Stub::MeetTriggerGate)),
            (10, stub(Stub::ShowTradesToPlayer)),
            (10, stub(Stub::SetLookAndInteractPlayer)),
            (2, stub(Stub::SetWalkTargetFromMeetingPoint)),
            (3, stub(Stub::GiveGiftToHero)),
            (3, stub(Stub::ValidateMeetingPoint)),
            (
                3,
                Kind::Gate(Box::new(Gate::ordered_run_one(
                    Vec::new(),
                    vec![InteractionTarget],
                    vec![(stub(Stub::TradeWithVillager), 1)],
                ))),
            ),
            full_look_behavior(),
            (99, Kind::Villager(Behavior::UpdateActivityFromSchedule)),
        ],
    )
    .when(vec![(MeetingPoint, ValuePresent)])
}

fn idle_package(speed: f32) -> ActivityData {
    ActivityData::with_priorities(
        Activity::Idle,
        vec![
            (
                2,
                Kind::Gate(Box::new(Gate::run_one(
                    Vec::new(),
                    vec![
                        (stub(Stub::InteractWithVillager), 2),
                        (stub(Stub::InteractWithBreedVillager), 1),
                        (stub(Stub::InteractWithCat), 1),
                        (
                            Kind::Villager(Behavior::VillageBoundRandomStroll {
                                speed,
                                max_xz: 10,
                                max_y: 7,
                            }),
                            1,
                        ),
                        (
                            Kind::WalkTargetFromLook {
                                speed: Speed::fixed(speed),
                                close_enough: 2,
                            },
                            1,
                        ),
                        (stub(Stub::JumpOnBed), 1),
                        (Kind::DoNothing { min: 30, max: 60 }, 1),
                    ],
                ))),
            ),
            (3, stub(Stub::GiveGiftToHero)),
            (3, stub(Stub::SetLookAndInteractPlayer)),
            (3, stub(Stub::ShowTradesToPlayer)),
            (
                3,
                Kind::Gate(Box::new(Gate::ordered_run_one(
                    Vec::new(),
                    vec![InteractionTarget],
                    vec![(stub(Stub::TradeWithVillager), 1)],
                ))),
            ),
            (
                3,
                Kind::Gate(Box::new(Gate::ordered_run_one(
                    Vec::new(),
                    vec![BreedTarget],
                    vec![(stub(Stub::VillagerMakeLove), 1)],
                ))),
            ),
            full_look_behavior(),
            (99, Kind::Villager(Behavior::UpdateActivityFromSchedule)),
        ],
    )
}

fn panic_package(speed: f32) -> ActivityData {
    let panic_speed = speed * 1.5;
    ActivityData::with_priorities(
        Activity::Panic,
        vec![
            (0, Kind::Villager(Behavior::CalmDown)),
            (
                1,
                Kind::Villager(Behavior::WalkAwayFromEntity {
                    memory: NearestHostile,
                    speed: panic_speed,
                    desired_distance: 6,
                    check_no_walk_target: false,
                }),
            ),
            (
                1,
                Kind::Villager(Behavior::WalkAwayFromEntity {
                    memory: HurtByEntity,
                    speed: panic_speed,
                    desired_distance: 6,
                    check_no_walk_target: false,
                }),
            ),
            (
                3,
                Kind::Villager(Behavior::VillageBoundRandomStroll {
                    speed: panic_speed,
                    max_xz: 2,
                    max_y: 2,
                }),
            ),
            minimal_look_behavior(),
        ],
    )
}

fn pre_raid_package(_speed: f32) -> ActivityData {
    ActivityData::with_priorities(
        Activity::PreRaid,
        vec![
            (0, stub(Stub::RingBell)),
            (0, stub(Stub::PreRaidTriggerGate)),
            minimal_look_behavior(),
            (99, stub(Stub::ResetRaidStatus)),
        ],
    )
}

fn raid_package(_speed: f32) -> ActivityData {
    ActivityData::with_priorities(
        Activity::Raid,
        vec![
            (0, stub(Stub::RaidCelebrateTriggerGate)),
            (0, stub(Stub::CelebrateVillagersSurvivedRaid)),
            (2, stub(Stub::RaidLocateHidingPlace)),
            minimal_look_behavior(),
            (99, stub(Stub::ResetRaidStatus)),
        ],
    )
}

fn hide_package(_speed: f32) -> ActivityData {
    ActivityData::with_priorities(
        Activity::Hide,
        vec![
            (0, stub(Stub::SetHiddenState)),
            (1, stub(Stub::LocateHidingPlace)),
            minimal_look_behavior(),
        ],
    )
}

fn build_template(baby: bool, is_farmer: bool) -> BrainTemplate {
    let mut activities = Vec::with_capacity(9);
    if baby {
        activities.push(play_package(SPEED_MODIFIER));
    } else {
        activities.push(work_package(is_farmer, SPEED_MODIFIER));
    }
    activities.push(core_package(SPEED_MODIFIER));
    activities.push(meet_package(SPEED_MODIFIER));
    activities.push(rest_package(SPEED_MODIFIER));
    activities.push(idle_package(SPEED_MODIFIER));
    activities.push(panic_package(SPEED_MODIFIER));
    activities.push(pre_raid_package(SPEED_MODIFIER));
    activities.push(raid_package(SPEED_MODIFIER));
    activities.push(hide_package(SPEED_MODIFIER));
    BrainTemplate::new(&VILLAGER_SENSORS, activities)
}

fn is_farmer_profession(profession: i32) -> bool {
    facts::profession(profession).is_some_and(|entry| entry.name == "minecraft:farmer")
}

/// 构造村民大脑并绑上日程（`Villager` 或 `BabyVillager`）；传感器的随机起始延迟由调用方抽。
///
/// Builds a villager brain with its schedule attached (`Villager` or `BabyVillager`);
/// callers draw sensor start delays from the mob's random.
pub fn brain(baby: bool, profession: i32) -> Brain {
    let mut brain = if baby {
        Brain::cached(&BABY_TEMPLATE, || build_template(true, false))
    } else if is_farmer_profession(profession) {
        Brain::cached(&ADULT_FARMER_TEMPLATE, || build_template(false, true))
    } else {
        Brain::cached(&ADULT_DEFAULT_TEMPLATE, || build_template(false, false))
    };
    brain.set_schedule(Some(if baby {
        ActivitySchedule::BabyVillager
    } else {
        ActivitySchedule::Villager
    }));
    brain
}

/// `Villager.registerBrainGoals`：设置日程并立即按当前世界时钟更新一次活动。
///
/// `Villager.registerBrainGoals`: sets the schedule and immediately updates the active
/// activity from the current world clock.
pub fn register_brain_goals(mob: &mut Mob, blocks: &dyn BlockView, game_time: i64) {
    let schedule = if mob.is_baby() {
        ActivitySchedule::BabyVillager
    } else {
        ActivitySchedule::Villager
    };
    if let Some(brain) = mob.brain.as_mut() {
        brain.set_schedule(Some(schedule));
        brain.update_activity_from_schedule(blocks, game_time);
    }
}

/// `Villager.refreshBrain(ServerLevel)`：`stopAll` → `pack` → 按当前年龄和职业重建大脑 →
/// 拷贝可序列化记忆、播种闸门并抽 9 个传感器延迟 → `registerBrainGoals`。
///
/// `Villager.refreshBrain(ServerLevel)`: `stopAll` → `pack` → rebuild for current age
/// and profession → restore serialisable memories, seed gates and draw 9 sensor delays →
/// `registerBrainGoals`.
pub fn refresh_brain(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    let old = mob.brain.take();
    if let Some(mut old_brain) = old {
        old_brain.stop_all(me, mob, ctx);
        let mut fresh = brain(mob.is_baby(), mob.villager.profession);
        fresh.seed_shuffles(me.random.state());
        fresh.randomly_delay_start(&mut me.random);
        fresh.copy_packed_memories_from(&old_brain);
        mob.brain = Some(Box::new(fresh));
    } else {
        let mut fresh = brain(mob.is_baby(), mob.villager.profession);
        fresh.seed_shuffles(me.random.state());
        fresh.randomly_delay_start(&mut me.random);
        mob.brain = Some(Box::new(fresh));
    }
    let time = ctx.blocks().game_time();
    register_brain_goals(mob, ctx.blocks(), time);
}

/// 离线（无 `EntityCtx`）版本的 `refreshBrain`，给 NBT 读档和无上下文的年龄边界切换用。
///
/// Offline (no `EntityCtx`) `refreshBrain`, for NBT loading and context-free age transitions.
pub fn refresh_brain_offline(me: &mut Entity, mob: &mut Mob) {
    let old = mob.brain.take();
    let mut fresh = brain(mob.is_baby(), mob.villager.profession);
    fresh.seed_shuffles(me.random.state());
    fresh.randomly_delay_start(&mut me.random);
    if let Some(old_brain) = old {
        fresh.copy_packed_memories_from(&old_brain);
    }
    mob.brain = Some(Box::new(fresh));
}

/// `Villager.customServerAiStep`：推进大脑（支持降频跳刻 `decision_gap`），随后
/// `!isNoAi()` 时照官方抽 `this.random.nextInt(100)`（袭击冒汗粒子判定，保持随机序列一致）。
///
/// `Villager.customServerAiStep`: ticks the brain (advancing by `decision_gap` when
/// throttled), then draws `this.random.nextInt(100)` while `!isNoAi()` to keep the mob's
/// RNG sequence aligned with vanilla's raid-sweat check.
pub(super) fn custom_server_ai_step(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    decision_gap: i64,
) {
    if let Some(mut brain) = mob.brain.take() {
        brain.tick_by(me, mob, ctx, decision_gap.max(1));
        mob.brain = Some(brain);
    }
    if !mob.no_ai {
        let _ = me.random.next_int_bounded(100);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BlockTicks, World, entity::ctx::PreciseCtx, testing::TestLevel};
    use mc_bamboo_protocol::components::value::GlobalPos;

    /// 成年默认、成年农民、幼年三种模板的传感器表、活动表与进入条件和官方一致。
    #[test]
    fn villager_brain_templates_match_vanilla_structure() {
        let adult = brain(false, 0);
        let farmer = brain(false, facts::profession_id("minecraft:farmer").unwrap());
        let baby = brain(true, 0);

        // 9 个传感器严格按登记顺序。
        let sensor_names: Vec<&str> = adult
            .sensor_table()
            .into_iter()
            .map(|(name, _, _)| name)
            .collect();
        assert_eq!(
            sensor_names,
            vec![
                "NearestLivingEntitySensor",
                "PlayerSensor",
                "NearestItemSensor",
                "NearestBedSensor",
                "HurtBySensor",
                "VillagerHostilesSensor",
                "VillagerBabiesSensor",
                "SecondaryPoiSensor",
                "GolemSensor",
            ]
        );

        // 成年有 WORK，没有 PLAY；幼年有 PLAY，没有 WORK。
        assert!(adult.table_activities().contains(&Activity::Work));
        assert!(!adult.table_activities().contains(&Activity::Play));
        assert!(baby.table_activities().contains(&Activity::Play));
        assert!(!baby.table_activities().contains(&Activity::Work));

        // 农民用 WorkAtComposter，其余成年职业用 WorkAtPoi。
        assert!(
            farmer
                .behavior_table()
                .iter()
                .any(|(_, act, name)| *act == "work" && name.contains("WorkAtComposter"))
        );
        assert!(
            adult
                .behavior_table()
                .iter()
                .any(|(_, act, name)| *act == "work" && name.contains("WorkAtPoi"))
        );

        // 活动进入条件：成年 WORK 要 JOB_SITE，MEET 要 MEETING_POINT；幼年只有 MEET。
        assert_eq!(
            adult.requirement_table(),
            vec![
                ("work", vec![("job_site", "value_present")]),
                ("meet", vec![("meeting_point", "value_present")]),
            ]
        );
        assert_eq!(
            baby.requirement_table(),
            vec![("meet", vec![("meeting_point", "value_present")])]
        );
        assert_eq!(adult.schedule(), Some(ActivitySchedule::Villager));
        assert_eq!(baby.schedule(), Some(ActivitySchedule::BabyVillager));
    }

    /// `refresh_brain` 保留可序列化记忆（如 `HOME`、`LAST_SLEPT`），丢弃不可序列化记忆
    /// （如 `HURT_BY`、`WALK_TARGET`），并按新年龄切换日程。
    #[test]
    fn refresh_brain_preserves_serializable_memories_only() {
        let mut me = Entity::by_mob_type(MobType::Villager, Vec3::new(0.5, 64.0, 0.5));
        let mut mob = Mob::new(MobType::Villager);
        let home = GlobalPos {
            dimension: "minecraft:overworld".to_owned(),
            pos: BlockPos::new(1, 64, 2),
        };
        {
            let brain = mob.brain.as_mut().expect("villager has brain");
            brain
                .memories
                .set(Home, MemoryValue::GlobalPos(home.clone()));
            brain.memories.set(LastSlept, MemoryValue::Long(500));
            brain.memories.set(WalkTarget, MemoryValue::Unit);
        }

        mob.age = -24_000;
        let mut level = TestLevel::new();
        level.clock_ticks = 3000; // 幼年 PLAY 窗口
        let mut ticks = BlockTicks::new();
        ticks.set_time(50);
        {
            let mut world = World::new(&mut level, &mut ticks);
            let mut ctx = PreciseCtx::new(&mut world, me.id);
            refresh_brain(&mut me, &mut mob, &mut ctx);
        }

        let brain = mob.brain.as_ref().expect("villager still has brain");
        assert_eq!(brain.schedule(), Some(ActivitySchedule::BabyVillager));
        assert_eq!(brain.active_names(), vec!["core", "play"]);
        assert_eq!(brain.memories.global_pos(Home), Some(&home));
        assert_eq!(brain.memories.long(LastSlept), Some(500));
        assert!(!brain.memories.has(WalkTarget));
    }

    /// 受伤触发 `VillagerPanicTrigger` 立即切到 `PANIC` 并清理旧移动/注视记忆；
    /// 伤害消失后 `VillagerCalmDown` 切回日程活动。
    #[test]
    fn panic_trigger_and_calm_down_switch_activities() {
        let mut level = TestLevel::new();
        level.clock_ticks = 500; // 成年 IDLE 窗口
        let mut ticks = BlockTicks::new();
        ticks.set_time(100);
        let mut me = Entity::by_mob_type(MobType::Villager, Vec3::new(0.5, 64.0, 0.5));
        let mut mob = Mob::new(MobType::Villager);
        {
            let world = World::new(&mut level, &mut ticks);
            register_brain_goals(&mut mob, &world, 100);
        }
        assert_eq!(
            mob.brain.as_ref().unwrap().active_names(),
            vec!["core", "idle"]
        );

        // 记录最近受伤（`HurtBySensor` 写入 `HurtBy`）并放一个旧的 `LookTarget`。
        mob.last_damage_source = Some(super::super::DamageSource::environment(
            super::super::DamageKind::Generic,
        ));
        mob.last_damage_stamp = 100;
        mob.brain.as_mut().unwrap().memories.set(
            LookTarget,
            MemoryValue::Look(PositionTracker::at_block(BlockPos::new(5, 64, 5))),
        );
        ticks.set_time(101);
        {
            let mut world = World::new(&mut level, &mut ticks);
            let mut ctx = PreciseCtx::new(&mut world, me.id);
            custom_server_ai_step(&mut me, &mut mob, &mut ctx, 1);
        }
        let brain = mob.brain.as_ref().unwrap();
        assert_eq!(brain.active_names(), vec!["core", "panic"]);
        assert!(!brain.memories.has(LookTarget));

        // 伤害过期（>40 刻）且 `HurtBySensor` 再次扫描后擦除 `HurtBy`，`VillagerCalmDown` 切回 `IDLE`。
        for step in 1..=25 {
            ticks.set_time(140 + step);
            let mut world = World::new(&mut level, &mut ticks);
            let mut ctx = PreciseCtx::new(&mut world, me.id);
            custom_server_ai_step(&mut me, &mut mob, &mut ctx, 1);
        }
        assert_eq!(
            mob.brain.as_ref().unwrap().active_names(),
            vec!["core", "idle"]
        );
    }
}
