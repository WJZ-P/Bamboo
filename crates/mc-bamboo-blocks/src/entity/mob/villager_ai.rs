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

use std::collections::BTreeMap;

use mc_bamboo_core::{Aabb, BlockPos, Vec3};
use mc_bamboo_data::{
    blocks, items,
    spawning::MobCategory,
    tags::{block_tags, item_tags},
    villager as facts,
};
use mc_bamboo_protocol::{
    components::value::GlobalPos, item::ItemStack, packet::play::EquipmentSlot,
};

use super::{
    Mob, MobEvent, MobType,
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
    goals, living,
    navigation::find_surface_position,
    path::{self, EvaluatorFlags, Malus, Path, PathMob, PathRequest},
    villager::{self, VillagerInventory},
};
use crate::{
    behaviour::{
        common::{block_by_id, default_state, item_of},
        plants::{is_crop_block, is_crop_max_age},
    },
    entity::{
        Entity, EntityPublic, EntityUid, KindPublic,
        ctx::{BlockView, Command, EntityCtx},
    },
    shape::CollisionContext,
    state::{OCCUPIED, item_is_tagged},
};

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

/// `AcquirePoi` 三个实例的槽位种类。
///
/// Slot kind for the three `AcquirePoi` instances in `getCorePackage`.
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

    const fn acquiring_memory(self) -> MemoryType {
        match self {
            Self::JobSite => PotentialJobSite,
            Self::Home => Home,
            Self::MeetingPoint => MeetingPoint,
        }
    }

    const fn event_id(self) -> Option<u8> {
        match self {
            Self::JobSite => None,
            Self::Home | Self::MeetingPoint => Some(14),
        }
    }

    fn type_mask(self, profession: i32) -> u32 {
        match self {
            Self::JobSite => villager::acquirable_job_site_mask(profession),
            Self::Home => villager::poi_name_mask("minecraft:home"),
            Self::MeetingPoint => villager::poi_name_mask("minecraft:meeting"),
        }
    }

    fn valid_poi(self, blocks: &dyn BlockView, pos: BlockPos) -> bool {
        match self {
            Self::Home => validate_bed_poi(blocks, pos),
            Self::JobSite | Self::MeetingPoint => true,
        }
    }
}

/// `AcquirePoi$JitteredLinearRetry`：失败候选的抖动线性退避。
///
/// `AcquirePoi$JitteredLinearRetry`: jittered linear backoff for unreachable candidates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JitteredLinearRetry {
    previous_attempt: i64,
    next_scheduled_attempt: i64,
    current_delay: i32,
}

impl JitteredLinearRetry {
    fn new(now: i64, random_40: i32) -> Self {
        let mut retry = Self {
            previous_attempt: 0,
            next_scheduled_attempt: 0,
            current_delay: 0,
        };
        retry.mark_attempt(now, random_40);
        retry
    }

    fn mark_attempt(&mut self, now: i64, random_40: i32) {
        self.previous_attempt = now;
        let next = self.current_delay + random_40 + 40;
        self.current_delay = next.min(400);
        self.next_scheduled_attempt = now + i64::from(self.current_delay);
    }

    const fn is_still_valid(&self, now: i64) -> bool {
        now - self.previous_attempt < 400
    }

    const fn should_retry(&self, now: i64) -> bool {
        now >= self.next_scheduled_attempt
    }
}

/// `AcquirePoi` 运行态（装箱存放，保持 `Node` 136 字节不增长）。
///
/// Boxed runtime state for `AcquirePoi` (keeps `Node` at 136 bytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcquirePoiState {
    pub slot: PoiAcquireSlot,
    pub next_scheduled_start: i64,
    pub retry_cache: BTreeMap<BlockPos, JitteredLinearRetry>,
}

impl AcquirePoiState {
    fn new(slot: PoiAcquireSlot) -> Self {
        Self {
            slot,
            next_scheduled_start: 0,
            retry_cache: BTreeMap::new(),
        }
    }
}

/// `HarvestFarmland` 运行态（装箱存放，保持 `Node` 136 字节不增长）。
///
/// Boxed runtime state for `HarvestFarmland` (keeps `Node` at 136 bytes).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HarvestFarmlandState {
    pub above_farmland: Option<BlockPos>,
    pub next_ok_start: i64,
    pub time_worked: i32,
    pub valid_farmland: Vec<BlockPos>,
}

/// `UseBonemeal` 运行态（装箱存放，保持 `Node` 136 字节不增长）。
///
/// Boxed runtime state for `UseBonemeal` (keeps `Node` at 136 bytes).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UseBonemealState {
    pub next_work_cycle: i64,
    pub last_session: i64,
    pub time_worked: i32,
    pub crop_pos: Option<BlockPos>,
}

/// `ValidateNearbyPoi` 的四种槽位。
///
/// The four `ValidateNearbyPoi` slots across core / rest / meet packages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidatePoiSlot {
    JobSite,
    PotentialJobSite,
    Home,
    MeetingPoint,
}

impl ValidatePoiSlot {
    const fn memory(self) -> MemoryType {
        match self {
            Self::JobSite => JobSite,
            Self::PotentialJobSite => PotentialJobSite,
            Self::Home => Home,
            Self::MeetingPoint => MeetingPoint,
        }
    }

    const fn required(self) -> &'static [(MemoryType, MemoryStatus)] {
        match self {
            Self::JobSite => REQ_JOB_PRESENT,
            Self::PotentialJobSite => REQ_POTENTIAL_JOB_PRESENT,
            Self::Home => REQ_HOME_PRESENT,
            Self::MeetingPoint => REQ_MEETING_PRESENT,
        }
    }

    fn type_mask(self, profession: i32) -> u32 {
        match self {
            Self::JobSite => villager::held_job_site_mask(profession),
            Self::PotentialJobSite => villager::acquirable_job_site_mask(profession),
            Self::Home => villager::poi_name_mask("minecraft:home"),
            Self::MeetingPoint => villager::poi_name_mask("minecraft:meeting"),
        }
    }
}

/// 期 4 以后再实现的行为占位：保持官方的 `Class`、必需记忆表与对拍类名，永不启动。
///
/// Placeholders for behaviours deferred to later phases: preserve vanilla's `Class`,
/// required memories and oracle class name, and never start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stub {
    InteractWithDoor,
    GoToWantedItem,
    WorkAtComposter,
    WorkAtPoi,
    StrollAroundJobSite,
    StrollToJobSite,
    StrollToSecondaryJobSite,
    HarvestFarmland,
    UseBonemeal,
    ShowTradesToPlayer,
    SetLookAndInteractPlayer,
    GiveGiftToHero,
    PlayTagWithOtherKids,
    InteractWithVillager,
    InteractWithBreedVillager,
    InteractWithCat,
    JumpOnBed,
    SleepInBed,
    SetClosestHomeAsWalkTarget,
    InsideBrownianWalk,
    TradeWithVillager,
    VillagerMakeLove,
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
const REQ_STROLL_MEETING: &[(MemoryType, MemoryStatus)] =
    &[(WalkTarget, Registered), (MeetingPoint, ValuePresent)];
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
            _ => Class::OneShot,
        }
    }

    const fn required(self) -> &'static [(MemoryType, MemoryStatus)] {
        match self {
            Self::InteractWithDoor => REQ_DOOR,
            Self::GoToWantedItem => REQ_WANTED_ITEM,
            Self::WorkAtComposter | Self::WorkAtPoi => REQ_WORK_AT_POI,
            Self::StrollAroundJobSite | Self::StrollToJobSite => REQ_STROLL_JOB,
            Self::StrollToSecondaryJobSite => REQ_STROLL_SECONDARY,
            Self::HarvestFarmland => REQ_HARVEST_FARMLAND,
            Self::UseBonemeal => REQ_USE_BONEMEAL,
            Self::ShowTradesToPlayer => REQ_INTERACTION_PRESENT,
            Self::SetLookAndInteractPlayer => REQ_LOOK_INTERACT,
            Self::GiveGiftToHero => REQ_GIVE_GIFT,
            Self::PlayTagWithOtherKids => REQ_PLAY_TAG,
            Self::InteractWithVillager | Self::InteractWithCat => REQ_INTERACT_TARGET,
            Self::InteractWithBreedVillager => REQ_INTERACT_BREED,
            Self::JumpOnBed => REQ_JUMP_ON_BED,
            Self::SleepInBed => REQ_SLEEP_IN_BED,
            Self::SetClosestHomeAsWalkTarget => REQ_CLOSEST_HOME,
            Self::InsideBrownianWalk => REQ_WALK_ABSENT,
            Self::TradeWithVillager => REQ_TRADE_VILLAGER,
            Self::VillagerMakeLove => REQ_MAKE_LOVE,
        }
    }

    const fn name(self) -> &'static str {
        match self {
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
            _ => "OneShot",
        }
    }
}

/// `MEET` 优先级 2 的 `TriggerGate(StrollAroundPoi, SocializeAtBell)` 状态。
///
/// State for the `MEET` priority-2 `TriggerGate(StrollAroundPoi, SocializeAtBell)`.
#[derive(Debug, Clone, PartialEq)]
pub struct MeetGateState {
    pub stroll_next_ok: i64,
    pub flip: bool,
}

/// `PRE_RAID` 优先级 0 的 `TriggerGate.triggerOneShuffled([SetWalkTargetFromBlockMemory,
/// VillageBoundRandomStroll])` 状态。官方 `ShufflingList` 用无种子的随机源，对拍场景里
/// 两个触发只有一个能成、顺序不影响取数，这里和 `MEET` 闸门一样用翻转近似。
///
/// State for `PRE_RAID`'s priority-0 `TriggerGate.triggerOneShuffled([SetWalkTargetFromBlockMemory,
/// VillageBoundRandomStroll])`. Vanilla seeds the `ShufflingList` from an unseeded random; in the
/// oracle scenes only one trigger can fire and the order never changes the draws, so like the
/// `MEET` gate this approximates the shuffle with a flip.
#[derive(Debug, Clone, PartialEq)]
pub struct PreRaidGateState {
    pub speed: f32,
    pub flip: bool,
}

/// `SetClosestHomeAsWalkTarget` 的重试缓存（装箱以保持 `Node` 紧凑）。
///
/// Boxed retry cache for `SetClosestHomeAsWalkTarget`.
#[derive(Debug, Clone, PartialEq)]
pub struct ClosestHomeState {
    pub speed: f32,
    pub last_update: i64,
    pub batch_cache: Vec<(BlockPos, i64)>,
}

/// 村民专属行为与未接通占位。
///
/// Villager-specific behaviours and remaining placeholders.
#[derive(Debug, Clone, PartialEq)]
pub enum Behavior {
    /// `VillagerPanicTrigger`：`Behavior(60, 60)`，受伤或身边有敌对时切到 `PANIC`。
    PanicTrigger,
    /// `VillagerCalmDown`：`OneShot`，无伤害、无敌对且攻击者超出 6 格时清伤害记忆并回日程。
    CalmDown,
    /// `UpdateActivityFromSchedule`：`OneShot`，按 20 刻节流刷新日程活动。
    UpdateActivityFromSchedule,
    /// `LookAndFollowTradingPlayerSink`：`Behavior(MAX, MAX)`，交易中的村民停下看着玩家。
    LookAndFollowTradingPlayerSink { speed: f32 },
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
    /// `RingBell`：`OneShot`，每刻抽 `level.random.nextFloat()`，> 0.95 且钟在 3 格内时敲钟。
    RingBell,
    /// `ResetRaidStatus`：`OneShot`，每刻抽 `level.random.nextInt(20)`，命中且袭击
    /// 已停 / 判负 / 不在时回 IDLE。
    ResetRaidStatus,
    /// `CelebrateVillagersSurvivedRaid(600, 600)`：袭击胜利后的庆祝，放烟花、播庆祝声。
    CelebrateVillagersSurvivedRaid,
    /// `RaidLocateHidingPlace`：`LocateHidingPlace.create(24, speed, 1)` 带「袭击进行中」
    /// 的前置条件。
    RaidLocateHidingPlace {
        radius: i32,
        speed: f32,
        close_enough: i32,
    },
    /// `RaidCelebrateTriggerGate`：袭击胜利后 `triggerOneShuffled([MoveToSkySeeingSpot,
    /// VillageBoundRandomStroll])`。`MoveToSkySeeingSpot` 未建模，先按村内闲逛近似。
    RaidCelebrateTriggerGate { speed: f32 },
    /// `SetEntityLookTarget.create(filter, maxDist)`。
    SetEntityLookTarget { filter: LookFilter, max_dist: f32 },
    /// `InteractWith.of(type, maxDist, INTERACTION_TARGET, speed, closeEnough)`：`OneShot`。
    InteractWith {
        kind: MobType,
        max_dist: i32,
        speed: f32,
        close_enough: i32,
    },
    /// `WakeUp`：`OneShot`，非 `REST` 活动时叫醒正在睡觉的村民。
    WakeUp,
    /// `ReactToBell`：`OneShot`，听到钟声且无袭击时切到 `HIDE`。
    ReactToBell,
    /// `SetWalkTargetFromBlockMemory.create(memory, speed, closeEnough, tooFar, tooLong)`。
    WalkTargetFromBlockMemory {
        memory: MemoryType,
        speed: f32,
        close_enough: u16,
        too_far_distance: u16,
        too_long_unreachable: u16,
    },
    /// `MEET` 优先级 2 的 `TriggerGate(StrollAroundPoi, SocializeAtBell)`。
    MeetTriggerGate(Box<MeetGateState>),
    /// `PRE_RAID` 优先级 0 的 `TriggerGate.triggerOneShuffled([SetWalkTargetFromBlockMemory,
    /// VillageBoundRandomStroll])`：`OneShot`，走向钟（集合点）或在村里闲逛。
    PreRaidTriggerGate(Box<PreRaidGateState>),
    /// `SleepInBed`：`Behavior(60, 60)` 且 `timedOut` 恒假，在床上入睡直到活动不再是 `REST`。
    SleepInBed { next_ok_start_time: i64 },
    /// `SetClosestHomeAsWalkTarget`：`OneShot`，无 `HOME` 记忆时找最近 5 张床并走去。
    SetClosestHomeAsWalkTarget(Box<ClosestHomeState>),
    /// `InsideBrownianWalk`：`OneShot`，看不到天空时在 3×3×3 范围内随机走一格。
    InsideBrownianWalk { speed: f32 },
    /// `LocateHidingPlace.create(radius, speed, closeEnough)`：`OneShot`，找藏身床位并走去。
    LocateHidingPlace {
        radius: i32,
        speed: f32,
        close_enough: i32,
    },
    /// `SetHiddenState.create(stayHiddenSeconds, closeEnough)`：`OneShot`，藏身计时与钟声超时退出 `HIDE`。
    SetHiddenState {
        stay_hidden_ticks: i32,
        close_enough_dist: i32,
        ticks_hidden: i32,
    },
    /// `AcquirePoi`：最近 5 候选 + 一次多目标 A\* + 失败按候选建 `JitteredLinearRetry`。
    AcquirePoi(Box<AcquirePoiState>),
    /// `GoToPotentialJobSite`：`Behavior(1200, 1200)`，走向潜在工作站，停止时若未认领则归还票。
    GoToPotentialJobSite { speed: f32 },
    /// `AssignProfessionFromJobSite`：`OneShot`，2 格内认领工作站、发事件 14、设职业并重建大脑。
    AssignProfessionFromJobSite,
    /// `ValidateNearbyPoi`：`OneShot`，16 格内校验 POI 仍有效且床未被他人占用。
    ValidateNearbyPoi(ValidatePoiSlot),
    /// `YieldJobSite`：`OneShot`，无业村民把潜在工作站让给附近已有对应职业的同伴。
    YieldJobSite { speed: f32 },
    /// `ResetProfession`：`OneShot`，失去工作站且无经验、1 级时变回无业并重建大脑。
    ResetProfession,
    /// `PoiCompetitorScan`：`OneShot`，多个同职业村民共享同一工作站时高经验者胜出。
    PoiCompetitorScan,
    /// `GoToWantedItem.create(speed, false, maxDist)`：`OneShot`，走向 4 格内想捡的掉落物。
    GoToWantedItem { speed: f32, max_dist: i32 },
    /// `WorkAtPoi` / `WorkAtComposter`：`Behavior(60, 60)`，300 刻冷却、1/2 概率、1.73 格内工作。
    WorkAtPoi { is_composter: bool, last_check: i64 },
    /// `StrollAroundPoi.create(memory, speed, maxDist)`：`OneShot`，在 POI 周围 8×6 闲逛（180 刻冷却）。
    StrollAroundPoi {
        memory: MemoryType,
        speed: f32,
        max_dist: i32,
        next_ok_start: i64,
    },
    /// `StrollToPoi.create(memory, speed, closeEnough, maxDist)`：`OneShot`，走向 POI（80 刻冷却）。
    StrollToPoi {
        memory: MemoryType,
        speed: f32,
        close_enough: i32,
        max_dist: i32,
        next_ok_start: i64,
    },
    /// `StrollToPoiList.create(listMemory, speed, closeEnough, maxDist, primaryMemory)`：`OneShot`，
    /// 随机选一处副 POI（耕地）走去（100 刻冷却）。
    StrollToPoiList {
        list_memory: MemoryType,
        primary_memory: MemoryType,
        speed: f32,
        close_enough: i32,
        max_dist: i32,
        next_ok_start: i64,
    },
    /// `HarvestFarmland`：`Behavior(60, 60)`，农民收割成熟作物并播种。
    HarvestFarmland(Box<HarvestFarmlandState>),
    /// `UseBonemeal`：`Behavior(60, 60)`，农民对未成熟作物施骨粉。
    UseBonemeal(Box<UseBonemealState>),
    /// 尚未实现的行为占位（永不启动）。
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
            Self::PanicTrigger
            | Self::WorkAtPoi { .. }
            | Self::HarvestFarmland(_)
            | Self::UseBonemeal(_)
            | Self::SleepInBed { .. } => Class::Timed { min: 60, max: 60 },
            Self::GoToPotentialJobSite { .. } => Class::Timed {
                min: 1200,
                max: 1200,
            },
            Self::LookAndFollowTradingPlayerSink { .. } => Class::Timed {
                min: i32::MAX,
                max: i32::MAX,
            },
            Self::CelebrateVillagersSurvivedRaid => Class::Timed { min: 600, max: 600 },
            Self::Unavailable(stub) => stub.class(),
            _ => Class::OneShot,
        }
    }

    pub(crate) fn required(&self) -> &[(MemoryType, MemoryStatus)] {
        match self {
            Self::PanicTrigger
            | Self::UpdateActivityFromSchedule
            | Self::SetRaidStatus
            | Self::WakeUp
            | Self::ResetRaidStatus
            | Self::CelebrateVillagersSurvivedRaid
            | Self::RaidCelebrateTriggerGate { .. }
            | Self::MeetTriggerGate(_)
            | Self::PreRaidTriggerGate(_) => &[],
            Self::SleepInBed { .. } => REQ_SLEEP_IN_BED,
            Self::SetClosestHomeAsWalkTarget(_) => REQ_CLOSEST_HOME,
            Self::InsideBrownianWalk { .. } => REQ_WALK_ABSENT,
            Self::LocateHidingPlace { .. } | Self::RaidLocateHidingPlace { .. } => {
                REQ_LOCATE_HIDING
            }
            Self::RingBell => REQ_MEETING_PRESENT,
            Self::SetHiddenState { .. } => REQ_SET_HIDDEN,
            Self::CalmDown => REQ_CALM_DOWN,
            Self::WalkAwayFromEntity { memory, .. } => match memory {
                NearestHostile => REQ_AWAY_HOSTILE,
                _ => REQ_AWAY_HURT_BY,
            },
            Self::VillageBoundRandomStroll { .. } | Self::GoToClosestVillage { .. } => {
                REQ_WALK_ABSENT
            }
            Self::SetEntityLookTarget { .. } => REQ_LOOK_ENTITY,
            Self::InteractWith { .. } => REQ_INTERACT_TARGET,
            Self::ReactToBell => REQ_HEARD_BELL,
            Self::WalkTargetFromBlockMemory { memory, .. } => match memory {
                JobSite => REQ_WALK_FROM_JOB,
                Home => REQ_WALK_FROM_HOME,
                _ => REQ_WALK_FROM_MEETING,
            },
            Self::AcquirePoi(state) => state.slot.required(),
            Self::GoToPotentialJobSite { .. } => REQ_POTENTIAL_JOB_PRESENT,
            Self::AssignProfessionFromJobSite => REQ_ASSIGN_PROF,
            Self::ValidateNearbyPoi(slot) => slot.required(),
            Self::YieldJobSite { .. } => REQ_YIELD_JOB,
            Self::ResetProfession => REQ_RESET_PROF,
            Self::PoiCompetitorScan => REQ_POI_COMPETITOR,
            Self::GoToWantedItem { .. } => REQ_WANTED_ITEM,
            Self::WorkAtPoi { .. } => REQ_WORK_AT_POI,
            Self::StrollAroundPoi { memory, .. } | Self::StrollToPoi { memory, .. } => match memory
            {
                JobSite => REQ_STROLL_JOB,
                _ => REQ_STROLL_MEETING,
            },
            Self::StrollToPoiList { .. } => REQ_STROLL_SECONDARY,
            Self::HarvestFarmland(_) => REQ_HARVEST_FARMLAND,
            Self::UseBonemeal(_) => REQ_USE_BONEMEAL,
            Self::LookAndFollowTradingPlayerSink { .. } => REQ_TRADING_SINK,
            Self::Unavailable(stub) => stub.required(),
        }
    }

    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::PanicTrigger => "VillagerPanicTrigger",
            Self::GoToPotentialJobSite { .. } => "GoToPotentialJobSite",
            Self::WorkAtPoi {
                is_composter: true, ..
            } => "WorkAtComposter",
            Self::WorkAtPoi {
                is_composter: false,
                ..
            } => "WorkAtPoi",
            Self::HarvestFarmland(_) => "HarvestFarmland",
            Self::UseBonemeal(_) => "UseBonemeal",
            Self::SleepInBed { .. } => "SleepInBed",
            Self::LookAndFollowTradingPlayerSink { .. } => "LookAndFollowTradingPlayerSink",
            Self::CelebrateVillagersSurvivedRaid => "CelebrateVillagersSurvivedRaid",
            Self::Unavailable(stub) => stub.name(),
            _ => "OneShot",
        }
    }

    pub(crate) fn extra_start(&mut self, cx: &mut Cx) -> bool {
        match self {
            Self::PanicTrigger => true,
            Self::LookAndFollowTradingPlayerSink { .. } => trading_sink_can_use(cx),
            Self::GoToPotentialJobSite { .. } => matches!(
                cx.active_non_core_activity(),
                None | Some(Activity::Idle | Activity::Work | Activity::Play)
            ),
            Self::WorkAtPoi { last_check, .. } => {
                if cx.time - *last_check < 300 {
                    return false;
                }
                if cx.level_next_int(2) != 0 {
                    return false;
                }
                *last_check = cx.time;
                cx.memories.global_pos(JobSite).is_some_and(|gp| {
                    gp.dimension == "minecraft:overworld"
                        && closer_to_center_than(gp.pos, cx.me.position, 1.73)
                })
            }
            Self::HarvestFarmland(state) => harvest_extra_start(state, cx),
            Self::UseBonemeal(state) => bonemeal_extra_start(state, cx),
            Self::SleepInBed { .. } => sleep_extra_start(cx),
            Self::CelebrateVillagersSurvivedRaid => celebrate_can_use(cx),
            Self::Unavailable(_) => false,
            _ => false,
        }
    }

    pub(crate) fn start(&mut self, cx: &mut Cx) {
        match self {
            Self::LookAndFollowTradingPlayerSink { speed } => follow_trading_player(cx, *speed),
            Self::PanicTrigger => {
                if is_hurt(cx) || has_hostile(cx) {
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
            Self::WorkAtPoi { is_composter, .. } => {
                cx.memories.set(LastWorkedAtPoi, MemoryValue::Long(cx.time));
                if let Some(gp) = cx.memories.global_pos(JobSite).cloned() {
                    cx.memories.set(
                        LookTarget,
                        MemoryValue::Look(PositionTracker::at_block(gp.pos)),
                    );
                }
                if let Some(sound) = villager::work_sound(cx.mob.villager.profession) {
                    living::make_sound(cx.me, cx.mob, sound);
                }
                if *is_composter {
                    use_composter_workstation(cx);
                }
                // `Villager.shouldRestock(ServerLevel)` 的 `day` 来自 OVERWORLD_DAY
                // 时间线的周期数（`clockTicks / 24000`），与 game_time 不是同一个时钟。
                // `Villager.shouldRestock(ServerLevel)`'s `day` is the OVERWORLD_DAY
                // timeline period count (`clockTicks / 24000`), a different clock from
                // game_time.
                let day = cx.ctx.blocks().clock_ticks() / 24_000;
                if cx.mob.villager.should_restock(cx.time, day) {
                    cx.mob.villager.restock(cx.time);
                }
            }
            Self::HarvestFarmland(state) => {
                if cx.time > state.next_ok_start
                    && let Some(pos) = state.above_farmland
                {
                    cx.memories.set(
                        LookTarget,
                        MemoryValue::Look(PositionTracker::at_block(pos)),
                    );
                    cx.memories.set(
                        WalkTarget,
                        MemoryValue::Walk(WalkTarget {
                            target: PositionTracker::at_block(pos),
                            speed: 0.5,
                            close_enough: 1,
                        }),
                    );
                }
            }
            Self::UseBonemeal(state) => {
                set_bonemeal_crop_target(state.crop_pos, cx);
                cx.mob
                    .equipment
                    .set(EquipmentSlot::MainHand, ItemStack::new(items::BONE_MEAL, 1));
                state.next_work_cycle = cx.time;
                state.time_worked = 0;
            }
            Self::SleepInBed { next_ok_start_time } => {
                if cx.time > *next_ok_start_time
                    && let Some(gp) = cx.memories.global_pos(Home).cloned()
                {
                    if villager::start_sleeping(cx.me, cx.mob, cx.ctx, gp.pos) {
                        cx.memories.set(LastSlept, MemoryValue::Long(cx.time));
                    }
                    cx.memories.erase(WalkTarget);
                    cx.memories.erase(CantReachWalkTargetSince);
                }
            }
            _ => {}
        }
    }

    pub(crate) fn can_still_use(&mut self, cx: &mut Cx) -> bool {
        match self {
            Self::LookAndFollowTradingPlayerSink { .. } => trading_sink_can_use(cx),
            Self::PanicTrigger => is_hurt(cx) || has_hostile(cx),
            Self::GoToPotentialJobSite { .. } => cx.memories.has(PotentialJobSite),
            Self::WorkAtPoi { .. } => cx.memories.global_pos(JobSite).is_some_and(|gp| {
                gp.dimension == "minecraft:overworld"
                    && closer_to_center_than(gp.pos, cx.me.position, 1.73)
            }),
            Self::HarvestFarmland(state) => state.time_worked < 200,
            Self::UseBonemeal(state) => state.time_worked < 80 && state.crop_pos.is_some(),
            Self::SleepInBed { .. } => cx.memories.global_pos(Home).is_some_and(|gp| {
                cx.is_active(Activity::Rest)
                    && cx.me.position.y > f64::from(gp.pos.y) + 0.4
                    && closer_to_center_than(gp.pos, cx.me.position, 1.14)
            }),
            Self::CelebrateVillagersSurvivedRaid => celebrate_can_continue(cx),
            _ => false,
        }
    }

    pub(crate) fn tick(&mut self, cx: &mut Cx) {
        match self {
            Self::LookAndFollowTradingPlayerSink { speed } => follow_trading_player(cx, *speed),
            Self::PanicTrigger => {
                // `VillagerPanicTrigger.tick`：每 100 刻调 `spawnGolemIfNeeded(3)`，铁傀儡生成在期 6。
            }
            Self::GoToPotentialJobSite { speed } => {
                if let Some(gp) = cx.memories.global_pos(PotentialJobSite).cloned() {
                    set_walk_and_look_target_block(cx, gp.pos, *speed, 1);
                }
            }
            Self::HarvestFarmland(state) => harvest_tick(state, cx),
            Self::UseBonemeal(state) => bonemeal_tick(state, cx),
            Self::CelebrateVillagersSurvivedRaid => celebrate_tick(cx),
            _ => {}
        }
    }

    pub(crate) fn stop(&mut self, cx: &mut Cx) {
        match self {
            Self::LookAndFollowTradingPlayerSink { .. } => {
                cx.memories.erase(WalkTarget);
                cx.memories.erase(LookTarget);
            }
            Self::GoToPotentialJobSite { .. } => {
                if let Some(gp) = cx.memories.global_pos(PotentialJobSite).cloned()
                    && gp.dimension == "minecraft:overworld"
                    && cx.ctx.blocks().poi_type_at(gp.pos).is_some()
                {
                    cx.ctx.emit(Command::PoiRelease { pos: gp.pos }, cx.me);
                }
                cx.memories.erase(PotentialJobSite);
            }
            Self::HarvestFarmland(state) => {
                cx.memories.erase(LookTarget);
                cx.memories.erase(WalkTarget);
                state.time_worked = 0;
                state.next_ok_start = cx.time + 40;
            }
            Self::UseBonemeal(state) => {
                cx.mob
                    .equipment
                    .set(EquipmentSlot::MainHand, ItemStack::empty());
                state.last_session = i64::from(cx.me.tick_count);
            }
            Self::SleepInBed { next_ok_start_time } if cx.mob.villager.sleeping => {
                villager::stop_sleeping(cx.me, cx.mob, Some(cx.memories), cx.ctx);
                *next_ok_start_time = cx.time + 40;
            }
            Self::CelebrateVillagersSurvivedRaid => {
                cx.update_activity_from_schedule();
            }
            _ => {}
        }
    }

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
            } => trigger_village_bound_random_stroll(*speed, *max_xz, *max_y, cx),
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
                // `SetRaidStatus`：每刻抽 `level.random.nextInt(20)` 保持世界随机序列
                // 对齐；命中时按 `ServerLevel.getRaidAt(pos)` 切 PRE_RAID / RAID。
                //
                // `SetRaidStatus`: draw `level.random.nextInt(20)` every tick to keep the
                // level RNG in step; on a hit, switch to PRE_RAID / RAID per
                // `ServerLevel.getRaidAt(pos)`.
                if cx.level_next_int(20) != 0 {
                    return false;
                }
                let pos = BlockPos::containing(cx.me.position);
                if let Some(raid) = cx.ctx.blocks().raid_at(pos) {
                    let activity = if !raid.has_first_wave_spawned || raid.is_between_waves {
                        Activity::PreRaid
                    } else {
                        Activity::Raid
                    };
                    cx.set_default_activity(activity);
                    cx.set_active_activity_if_possible(activity);
                }
                true
            }
            Self::RingBell => {
                // `RingBell`：每刻先抽 `level.random.nextFloat()`（不管距离和钟都取数），
                // 大于 0.95 且 MEETING_POINT 在 3 格内且那一格是钟才敲。
                //
                // `RingBell`: draw `level.random.nextFloat()` every tick, then ring only
                // past 0.95 when the meeting point is within 3 blocks and holds a bell.
                if cx.ctx.blocks().level_random_float() <= 0.95 {
                    return true;
                }
                let Some(gp) = cx.memories.global_pos(MeetingPoint).cloned() else {
                    return true;
                };
                if gp.dimension != "minecraft:overworld"
                    || !closer_to_center_than(gp.pos, cx.me.position, 3.0)
                {
                    return true;
                }
                if cx.ctx.blocks().block_state(gp.pos).is(blocks::BELL) {
                    cx.ctx.emit(Command::RingBell { pos: gp.pos }, cx.me);
                }
                true
            }
            Self::ResetRaidStatus => {
                // `ResetRaidStatus`：每刻抽 `level.random.nextInt(20)`，命中且袭击
                // 不在 / 已停 / 判负时回 IDLE 并按日程刷新。
                //
                // `ResetRaidStatus`: draw `nextInt(20)`; on a hit, reset to IDLE when
                // the raid is absent, stopped or lost.
                if cx.level_next_int(20) != 0 {
                    return false;
                }
                let pos = BlockPos::containing(cx.me.position);
                let gone = cx
                    .ctx
                    .blocks()
                    .raid_at(pos)
                    .is_none_or(|raid| raid.is_stopped || raid.is_loss);
                if gone {
                    cx.set_default_activity(Activity::Idle);
                    cx.update_activity_from_schedule();
                }
                true
            }
            Self::RaidLocateHidingPlace {
                radius,
                speed,
                close_enough,
            } => {
                // `LocateHidingPlace` 带「袭击进行中」的前置：只在袭击活跃且未定胜负时躲。
                let pos = BlockPos::containing(cx.me.position);
                let active = cx
                    .ctx
                    .blocks()
                    .raid_at(pos)
                    .is_some_and(|raid| raid.active && !raid.is_victory && !raid.is_loss);
                if !active {
                    return false;
                }
                trigger_locate_hiding_place(*radius, *speed, *close_enough, cx)
            }
            Self::RaidCelebrateTriggerGate { speed } => {
                // 袭击胜利后往空旷处走；`MoveToSkySeeingSpot` 未建模，先按村内闲逛近似。
                let victory = cx
                    .ctx
                    .blocks()
                    .raid_at(cx.me.block_position())
                    .is_some_and(|raid| raid.is_victory);
                if !victory {
                    return false;
                }
                trigger_village_bound_random_stroll(*speed, 10, 7, cx)
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
            Self::InteractWith {
                kind,
                max_dist,
                speed,
                close_enough,
            } => {
                let kind = *kind;
                // 官方先用 contains 判断是否有可见的该类型实体，没有就不触发。
                if sensor::find_closest_visible(cx, &mut |view| view.mob_type == Some(kind))
                    .is_none()
                {
                    return false;
                }
                let max_sqr = f64::from(*max_dist * *max_dist);
                let self_pos = cx.me.position;
                let target = sensor::find_closest_visible(cx, &mut |view| {
                    view.mob_type == Some(kind)
                        && view.position.distance_to_sqr(self_pos) <= max_sqr
                });
                if let Some(uid) = target {
                    cx.memories.set(InteractionTarget, MemoryValue::Entity(uid));
                    cx.memories.set(
                        LookTarget,
                        MemoryValue::Look(PositionTracker::Entity {
                            uid,
                            track_eye: true,
                            target_eye: false,
                        }),
                    );
                    cx.memories.set(
                        WalkTarget,
                        MemoryValue::Walk(WalkTarget {
                            target: PositionTracker::Entity {
                                uid,
                                track_eye: false,
                                target_eye: false,
                            },
                            speed: *speed,
                            close_enough: *close_enough,
                        }),
                    );
                }
                true
            }
            Self::WakeUp => {
                if cx.is_active(Activity::Rest) || !cx.mob.villager.sleeping {
                    return false;
                }
                villager::stop_sleeping(cx.me, cx.mob, Some(cx.memories), cx.ctx);
                true
            }
            Self::ReactToBell => {
                // 官方 `ReactToBell`：有活跃袭击时不切 HIDE（钟声由 PRE_RAID 的
                // `RingBell` 处理）；只在平时听到钟声才躲。
                //
                // Official `ReactToBell`: an active raid suppresses the HIDE switch
                // (PRE_RAID's `RingBell` handles bells); only hide outside raids.
                let pos = BlockPos::containing(cx.me.position);
                if cx.ctx.blocks().raid_at(pos).is_none() {
                    cx.set_active_activity_if_possible(Activity::Hide);
                }
                true
            }
            Self::WalkTargetFromBlockMemory {
                memory,
                speed,
                close_enough,
                too_far_distance,
                too_long_unreachable,
            } => trigger_walk_target_from_block_memory(
                *memory,
                *speed,
                *close_enough,
                *too_far_distance,
                *too_long_unreachable,
                cx,
            ),
            Self::PreRaidTriggerGate(state) => trigger_pre_raid_gate(state, cx),
            Self::MeetTriggerGate(state) => trigger_meet_gate(state, cx),
            Self::SetClosestHomeAsWalkTarget(state) => trigger_closest_home(state, cx),
            Self::InsideBrownianWalk { speed } => trigger_inside_brownian_walk(*speed, cx),
            Self::LocateHidingPlace {
                radius,
                speed,
                close_enough,
            } => trigger_locate_hiding_place(*radius, *speed, *close_enough, cx),
            Self::SetHiddenState {
                stay_hidden_ticks,
                close_enough_dist,
                ticks_hidden,
            } => trigger_set_hidden_state(*stay_hidden_ticks, *close_enough_dist, ticks_hidden, cx),
            Self::AcquirePoi(state) => trigger_acquire_poi(state, cx),
            Self::AssignProfessionFromJobSite => trigger_assign_profession(cx),
            Self::ValidateNearbyPoi(slot) => trigger_validate_nearby_poi(*slot, cx),
            Self::YieldJobSite { speed } => trigger_yield_job_site(*speed, cx),
            Self::ResetProfession => trigger_reset_profession(cx),
            Self::PoiCompetitorScan => trigger_poi_competitor_scan(cx),
            Self::GoToWantedItem { speed, max_dist } => {
                trigger_go_to_wanted_item(*speed, *max_dist, cx)
            }
            Self::StrollAroundPoi {
                memory,
                speed,
                max_dist,
                next_ok_start,
            } => trigger_stroll_around_poi(*memory, *speed, *max_dist, next_ok_start, cx),
            Self::StrollToPoi {
                memory,
                speed,
                close_enough,
                max_dist,
                next_ok_start,
            } => {
                trigger_stroll_to_poi(*memory, *speed, *close_enough, *max_dist, next_ok_start, cx)
            }
            Self::StrollToPoiList {
                list_memory,
                primary_memory,
                speed,
                close_enough,
                max_dist,
                next_ok_start,
            } => trigger_stroll_to_poi_list(
                *list_memory,
                *primary_memory,
                *speed,
                *close_enough,
                *max_dist,
                next_ok_start,
                cx,
            ),
            Self::PanicTrigger
            | Self::GoToPotentialJobSite { .. }
            | Self::WorkAtPoi { .. }
            | Self::HarvestFarmland(_)
            | Self::UseBonemeal(_)
            | Self::SleepInBed { .. }
            | Self::LookAndFollowTradingPlayerSink { .. }
            | Self::CelebrateVillagersSurvivedRaid
            | Self::Unavailable(_) => false,
        }
    }
}

fn trigger_go_to_wanted_item(speed: f32, max_dist: i32, cx: &mut Cx) -> bool {
    if cx.memories.has(ItemPickupCooldownTicks) || !cx.mob.can_pick_up_loot {
        return false;
    }
    // 官方记忆里持有 ItemEntity 对象本体；被捡走后它只是 isRemoved，位置和距离仍可读。
    // Vanilla holds the ItemEntity object itself: after pickup it is merely removed,
    // while position and distance remain readable.
    let Some((item_uid, remembered_pos)) = cx.memories.wanted_item() else {
        return false;
    };
    let mut item_pos = remembered_pos;
    let mut in_world = false;
    cx.ctx.with_other(item_uid, &mut |public| {
        if !public.removed {
            item_pos = public.position;
            in_world = true;
        }
    });
    let max_d = f64::from(max_dist);
    if item_pos.distance_to_sqr(cx.me.position) >= max_d * max_d {
        return false;
    }
    cx.memories.set(
        LookTarget,
        MemoryValue::Look(PositionTracker::Item {
            uid: item_uid,
            pos: item_pos,
            track_eye: true,
            target_eye: false,
        }),
    );
    cx.memories.set(
        WalkTarget,
        MemoryValue::Walk(WalkTarget {
            target: PositionTracker::Item {
                uid: item_uid,
                pos: item_pos,
                track_eye: false,
                target_eye: false,
            },
            speed,
            close_enough: 0,
        }),
    );
    let _ = in_world;
    true
}

fn trigger_stroll_around_poi(
    memory: MemoryType,
    speed: f32,
    max_dist: i32,
    next_ok_start: &mut i64,
    cx: &mut Cx,
) -> bool {
    let Some(gp) = cx.memories.global_pos(memory).cloned() else {
        return false;
    };
    if gp.dimension != "minecraft:overworld"
        || !closer_to_center_than(gp.pos, cx.me.position, f64::from(max_dist))
    {
        return false;
    }
    if cx.time <= *next_ok_start {
        return true;
    }
    let pos = goals::land_random_pos(cx.me, cx.mob, cx.ctx.blocks(), 8, 6);
    cx.memories.set_or_erase(
        WalkTarget,
        pos.map(|at| MemoryValue::Walk(WalkTarget::to_point(at, speed, 1))),
    );
    *next_ok_start = cx.time + 180;
    true
}

fn trigger_stroll_to_poi(
    memory: MemoryType,
    speed: f32,
    close_enough: i32,
    max_dist: i32,
    next_ok_start: &mut i64,
    cx: &mut Cx,
) -> bool {
    let Some(gp) = cx.memories.global_pos(memory).cloned() else {
        return false;
    };
    if gp.dimension != "minecraft:overworld"
        || !closer_to_center_than(gp.pos, cx.me.position, f64::from(max_dist))
    {
        return false;
    }
    if cx.time <= *next_ok_start {
        return true;
    }
    cx.memories.set(
        WalkTarget,
        MemoryValue::Walk(WalkTarget {
            target: PositionTracker::at_block(gp.pos),
            speed,
            close_enough,
        }),
    );
    *next_ok_start = cx.time + 80;
    true
}

fn trigger_stroll_to_poi_list(
    list_memory: MemoryType,
    primary_memory: MemoryType,
    speed: f32,
    close_enough: i32,
    max_dist: i32,
    next_ok_start: &mut i64,
    cx: &mut Cx,
) -> bool {
    let list_len = match cx.memories.get(list_memory) {
        Some(MemoryValue::GlobalPosList(list)) => list.len(),
        _ => 0,
    };
    if list_len == 0 {
        return false;
    }
    let idx = cx.level_next_int(list_len as i32) as usize;
    let picked = match cx.memories.get(list_memory) {
        Some(MemoryValue::GlobalPosList(list)) => list.get(idx).cloned(),
        _ => None,
    };
    let Some(picked) = picked else {
        return false;
    };
    let Some(primary) = cx.memories.global_pos(primary_memory).cloned() else {
        return false;
    };
    if picked.dimension != "minecraft:overworld"
        || !closer_to_center_than(primary.pos, cx.me.position, f64::from(max_dist))
    {
        return false;
    }
    if cx.time > *next_ok_start {
        cx.memories.set(
            WalkTarget,
            MemoryValue::Walk(WalkTarget {
                target: PositionTracker::at_block(picked.pos),
                speed,
                close_enough,
            }),
        );
        *next_ok_start = cx.time + 100;
    }
    true
}

/// `WorkAtComposter.useWorkstation`：磨面包 + 堆肥。
fn use_composter_workstation(cx: &mut Cx) {
    let Some(gp) = cx.memories.global_pos(JobSite).cloned() else {
        return;
    };
    if !cx.ctx.blocks().block_state(gp.pos).is(blocks::COMPOSTER) {
        return;
    }
    // `makeBread`
    if cx.mob.villager.inventory.count_item(items::BREAD) <= 36 {
        let wheat = cx.mob.villager.inventory.count_item(items::WHEAT);
        let loaves = (wheat / 3).min(3);
        if loaves > 0 {
            let _ = cx
                .mob
                .villager
                .inventory
                .remove_item_type(items::WHEAT, loaves * 3);
            let leftover = cx
                .mob
                .villager
                .inventory
                .add_item(ItemStack::new(items::BREAD, loaves));
            if !leftover.is_empty() {
                let drop_pos =
                    Vec3::new(cx.me.position.x, cx.me.position.y + 0.5, cx.me.position.z);
                cx.ctx.emit(
                    Command::DropAtLocation {
                        pos: drop_pos,
                        stack: leftover,
                    },
                    cx.me,
                );
            }
        }
    }
    // `compostItems`
    compost_items_at_workstation(gp.pos, cx);
}

fn compost_items_at_workstation(pos: BlockPos, cx: &mut Cx) {
    use crate::behaviour::common::item_of;
    use crate::behaviour::misc::{COMPOST_MAX_LEVEL, COMPOST_READY, roll_compost_layers};
    use crate::state::LEVEL_COMPOSTER;

    const COMPOSTABLE_ITEMS: [i32; 2] = [items::WHEAT_SEEDS, items::BEETROOT_SEEDS];
    let mut initial_state = cx.ctx.blocks().block_state(pos);
    if !initial_state.is_a(mc_bamboo_data::BlockAncestor::ComposterBlock) {
        return;
    }
    if initial_state.get(LEVEL_COMPOSTER) == COMPOST_READY {
        cx.ctx.emit(Command::VillagerCompostExtract { pos }, cx.me);
        initial_state = initial_state.with(LEVEL_COMPOSTER, 0);
    }
    let initial_level = initial_state.get(LEVEL_COMPOSTER);
    let mut level = initial_level;
    let mut remaining = 20_i32;
    let mut seen = [0_i32; 2];
    for slot_idx in (0..villager::VillagerInventory::SIZE).rev() {
        if remaining <= 0 {
            break;
        }
        let stack = cx.mob.villager.inventory.get(slot_idx);
        let Some(kind_idx) = COMPOSTABLE_ITEMS.iter().position(|&id| id == stack.item) else {
            continue;
        };
        let count = stack.count;
        let total = seen[kind_idx] + count;
        seen[kind_idx] = total;
        let to_insert = (total - 10).min(remaining).min(count);
        if to_insert <= 0 {
            continue;
        }
        remaining -= to_insert;
        let Some(layers) = item_of(stack.item).and_then(|info| info.compostable) else {
            continue;
        };
        for _ in 0..to_insert {
            if level < COMPOST_MAX_LEVEL {
                let added = roll_compost_layers(layers, level, |b| cx.level_next_int(b));
                if added > 0 {
                    level = (level + added).clamp(0, COMPOST_MAX_LEVEL);
                }
                cx.mob.villager.inventory.shrink_slot(slot_idx, 1);
            }
            if level == COMPOST_MAX_LEVEL {
                cx.ctx.emit(
                    Command::VillagerCompostFinish {
                        pos,
                        level,
                        success: level != initial_level,
                    },
                    cx.me,
                );
                return;
            }
        }
    }
    cx.ctx.emit(
        Command::VillagerCompostFinish {
            pos,
            level,
            success: level != initial_level,
        },
        cx.me,
    );
}

fn harvest_valid_pos(blocks: &dyn BlockView, pos: BlockPos) -> bool {
    let state = blocks.block_state(pos);
    let below = blocks.block_state(pos.below());
    is_crop_max_age(state) || (state.is_air() && below.is(blocks::FARMLAND))
}

fn pick_valid_farmland(state: &HarvestFarmlandState, cx: &mut Cx) -> Option<BlockPos> {
    if state.valid_farmland.is_empty() {
        None
    } else {
        let idx = cx.level_next_int(state.valid_farmland.len() as i32) as usize;
        state.valid_farmland.get(idx).copied()
    }
}

fn harvest_extra_start(state: &mut HarvestFarmlandState, cx: &mut Cx) -> bool {
    if !cx.ctx.blocks().block_game_rules().mob_griefing {
        return false;
    }
    if cx.mob.villager.profession != facts::profession_id("minecraft:farmer").unwrap_or(-1) {
        return false;
    }
    state.valid_farmland.clear();
    let pos = cx.me.position;
    let blocks = cx.ctx.blocks();
    for dx in -1..=1 {
        for dy in -1..=1 {
            for dz in -1..=1 {
                let target = BlockPos::containing(Vec3::new(
                    pos.x + f64::from(dx),
                    pos.y + f64::from(dy),
                    pos.z + f64::from(dz),
                ));
                if harvest_valid_pos(blocks, target) {
                    state.valid_farmland.push(target);
                }
            }
        }
    }
    state.above_farmland = pick_valid_farmland(state, cx);
    state.above_farmland.is_some()
}

fn harvest_tick(state: &mut HarvestFarmlandState, cx: &mut Cx) {
    if let Some(target) = state.above_farmland
        && !closer_to_center_than(target, cx.me.position, 1.0)
    {
        return;
    }
    if let Some(target) = state.above_farmland
        && cx.time > state.next_ok_start
    {
        let block_state = cx.ctx.blocks().block_state(target);
        let below_state = cx.ctx.blocks().block_state(target.below());
        if is_crop_max_age(block_state) {
            cx.ctx.emit(
                Command::DestroyBlock {
                    pos: target,
                    drop: true,
                },
                cx.me,
            );
        }
        if block_state.is_air()
            && below_state.is(blocks::FARMLAND)
            && cx.mob.villager.inventory.has_farm_seeds()
        {
            for slot_idx in 0..VillagerInventory::SIZE {
                let stack = cx.mob.villager.inventory.get(slot_idx);
                if stack.is_empty()
                    || !item_is_tagged(stack.item, item_tags::VILLAGER_PLANTABLE_SEEDS)
                {
                    continue;
                }
                let Some(block_id) = item_of(stack.item).and_then(|info| info.block) else {
                    continue;
                };
                let planted = default_state(block_by_id(block_id));
                cx.ctx.emit(
                    Command::PlantCrop {
                        pos: target,
                        state: planted,
                    },
                    cx.me,
                );
                cx.mob.villager.inventory.shrink_slot(slot_idx, 1);
                break;
            }
        }
        if is_crop_block(block_state) && !is_crop_max_age(block_state) {
            state.valid_farmland.retain(|&p| p != target);
            state.above_farmland = pick_valid_farmland(state, cx);
            if let Some(next) = state.above_farmland {
                state.next_ok_start = cx.time + 20;
                cx.memories.set(
                    WalkTarget,
                    MemoryValue::Walk(WalkTarget {
                        target: PositionTracker::at_block(next),
                        speed: 0.5,
                        close_enough: 1,
                    }),
                );
                cx.memories.set(
                    LookTarget,
                    MemoryValue::Look(PositionTracker::at_block(next)),
                );
            }
        }
    }
    state.time_worked += 1;
}

fn bonemeal_valid_pos(blocks: &dyn BlockView, pos: BlockPos) -> bool {
    let state = blocks.block_state(pos);
    is_crop_block(state) && !is_crop_max_age(state)
}

fn pick_bonemeal_target(cx: &mut Cx) -> Option<BlockPos> {
    let base = cx.me.block_position();
    let mut picked = None;
    let mut seen = 0_i32;
    for dx in -1..=1 {
        for dy in -1..=1 {
            for dz in -1..=1 {
                let pos = base.offset(dx, dy, dz);
                if bonemeal_valid_pos(cx.ctx.blocks(), pos) {
                    seen += 1;
                    if cx.level_next_int(seen) == 0 {
                        picked = Some(pos);
                    }
                }
            }
        }
    }
    picked
}

fn set_bonemeal_crop_target(crop_pos: Option<BlockPos>, cx: &mut Cx) {
    let Some(pos) = crop_pos else {
        return;
    };
    let tracker = PositionTracker::at_block(pos);
    cx.memories.set(LookTarget, MemoryValue::Look(tracker));
    cx.memories.set(
        WalkTarget,
        MemoryValue::Walk(WalkTarget {
            target: tracker,
            speed: 0.5,
            close_enough: 1,
        }),
    );
}

fn bonemeal_extra_start(state: &mut UseBonemealState, cx: &mut Cx) -> bool {
    let tick = i64::from(cx.me.tick_count);
    if tick % 10 != 0 || (state.last_session != 0 && state.last_session + 160 > tick) {
        return false;
    }
    if cx.mob.villager.inventory.count_item(items::BONE_MEAL) <= 0 {
        return false;
    }
    state.crop_pos = pick_bonemeal_target(cx);
    state.crop_pos.is_some()
}

fn bonemeal_tick(state: &mut UseBonemealState, cx: &mut Cx) {
    let Some(pos) = state.crop_pos else {
        return;
    };
    if cx.time < state.next_work_cycle || !closer_to_center_than(pos, cx.me.position, 1.0) {
        return;
    }
    let bonemeal_slot = (0..VillagerInventory::SIZE).find(|&i| {
        let s = cx.mob.villager.inventory.get(i);
        !s.is_empty() && s.item == items::BONE_MEAL
    });
    if let Some(slot_idx) = bonemeal_slot
        && bonemeal_valid_pos(cx.ctx.blocks(), pos)
    {
        cx.mob.villager.inventory.shrink_slot(slot_idx, 1);
        cx.ctx.emit(Command::GrowCrop { pos }, cx.me);
        state.crop_pos = pick_bonemeal_target(cx);
        set_bonemeal_crop_target(state.crop_pos, cx);
        state.next_work_cycle = cx.time + 40;
    }
    state.time_worked += 1;
}

/// `AcquirePoi.findPathToPois`：把候选 `(PoiType, BlockPos)` 集合交给一次多目标 A\* 寻路。
///
/// `AcquirePoi.findPathToPois`: runs one multi-target A\* search across all candidate POIs.
pub(crate) fn find_path_to_pois(cx: &mut Cx, candidates: &[(u8, BlockPos)]) -> Option<Path> {
    if candidates.is_empty() {
        return None;
    }
    let mut reach = 1_i32;
    let mut positions: Vec<BlockPos> = Vec::with_capacity(candidates.len());
    for &(kind, pos) in candidates {
        reach = reach.max(villager::poi_valid_range(kind));
        if !positions.contains(&pos) {
            positions.push(pos);
        }
    }
    let blocks = cx.ctx.blocks();
    let nav = living::nav_mob(cx.me, cx.mob, blocks);
    cx.mob
        .navigation
        .create_path_to_targets(blocks, &nav, &positions, reach)
}

fn validate_bed_poi(blocks: &dyn BlockView, pos: BlockPos) -> bool {
    let state = blocks.block_state(pos);
    state.is_tagged(block_tags::VILLAGERS_CAN_SLEEP_ON_BED) && !state.get(OCCUPIED)
}

fn set_walk_and_look_target_block(cx: &mut Cx, pos: BlockPos, speed: f32, close_enough: i32) {
    cx.memories.set(
        WalkTarget,
        MemoryValue::Walk(WalkTarget {
            target: PositionTracker::at_block(pos),
            speed,
            close_enough,
        }),
    );
    cx.memories.set(
        LookTarget,
        MemoryValue::Look(PositionTracker::at_block(pos)),
    );
}

fn closer_to_center_than(block: BlockPos, pos: Vec3, radius: f64) -> bool {
    let dx = f64::from(block.x) + 0.5 - pos.x;
    let dy = f64::from(block.y) + 0.5 - pos.y;
    let dz = f64::from(block.z) + 0.5 - pos.z;
    dx * dx + dy * dy + dz * dz < radius * radius
}

fn block_dist_sqr(a: BlockPos, b: BlockPos) -> i64 {
    let dx = i64::from(a.x - b.x);
    let dy = i64::from(a.y - b.y);
    let dz = i64::from(a.z - b.z);
    dx * dx + dy * dy + dz * dz
}

fn sleep_extra_start(cx: &Cx) -> bool {
    if cx.me.vehicle.is_some() {
        return false;
    }
    let Some(gp) = cx.memories.global_pos(Home) else {
        return false;
    };
    if gp.dimension != "minecraft:overworld" {
        return false;
    }
    if let Some(last_woken) = cx.memories.long(LastWoken) {
        let diff = cx.time - last_woken;
        if diff > 0 && diff < 100 {
            return false;
        }
    }
    let state = cx.ctx.blocks().block_state(gp.pos);
    closer_to_center_than(gp.pos, cx.me.position, 2.0)
        && state.is_tagged(block_tags::VILLAGERS_CAN_SLEEP_ON_BED)
        && !state.get(OCCUPIED)
}

fn trigger_closest_home(state: &mut ClosestHomeState, cx: &mut Cx) -> bool {
    if cx.time - state.last_update < 20 {
        return false;
    }
    let Some(home_kind) = mc_bamboo_data::poi::poi_type_id("home") else {
        return false;
    };
    let origin = cx.me.block_position();
    let Some(closest) = cx
        .ctx
        .blocks()
        .poi_find_closest(origin, 48, home_kind, 2, &|_| true)
    else {
        return false;
    };
    if dist_sqr_block(closest, origin) <= 4.0 {
        return false;
    }
    state.last_update = cx.time + i64::from(cx.level_next_int(20));
    let mut tried = 0_i32;
    let home_mask = 1_u32 << home_kind;
    let mut targets = Vec::new();
    for (kind, pos) in cx.ctx.blocks().poi_in_range(origin, 48, home_mask, 2) {
        if state.batch_cache.iter().any(|(cached, _)| *cached == pos) {
            continue;
        }
        tried += 1;
        if tried >= 5 {
            continue;
        }
        state.batch_cache.push((pos, state.last_update + 40));
        targets.push((kind, pos));
    }
    if let Some(path) = find_path_to_pois(cx, &targets)
        && path.reached
        && cx.ctx.blocks().poi_type_at(path.target).is_some()
    {
        let target = path.target;
        cx.memories.set(
            WalkTarget,
            MemoryValue::Walk(WalkTarget {
                target: PositionTracker::at_block(target),
                speed: state.speed,
                close_enough: 1,
            }),
        );
    } else if tried < 5 {
        let cutoff = state.last_update;
        state.batch_cache.retain(|(_, expiry)| *expiry >= cutoff);
    }
    true
}

fn trigger_inside_brownian_walk(speed: f32, cx: &mut Cx) -> bool {
    let origin = cx.me.block_position();
    if cx.ctx.blocks().can_see_sky(origin) {
        return false;
    }
    let mut candidates = Vec::with_capacity(27);
    for dx in -1..=1 {
        for dy in -1..=1 {
            for dz in -1..=1 {
                candidates.push(BlockPos::new(origin.x + dx, origin.y + dy, origin.z + dz));
            }
        }
    }
    // `Collections.shuffle` 用独立随机源，避免污染实体或世界的随机序列。
    let mut rng = crate::entity::JavaRandom::new(cx.time ^ i64::from(cx.me.id));
    for i in (1..candidates.len()).rev() {
        let j = rng.next_int_bounded((i + 1) as i32) as usize;
        candidates.swap(i, j);
    }
    let blocks = cx.ctx.blocks();
    let entity_box = cx.me.bounding_box();
    let no_collision = {
        let min_x = mc_bamboo_core::mth::floor(entity_box.min_x - 1.0e-7) - 1;
        let max_x = mc_bamboo_core::mth::floor(entity_box.max_x + 1.0e-7) + 1;
        let min_y = mc_bamboo_core::mth::floor(entity_box.min_y - 1.0e-7) - 1;
        let max_y = mc_bamboo_core::mth::floor(entity_box.max_y + 1.0e-7) + 1;
        let min_z = mc_bamboo_core::mth::floor(entity_box.min_z - 1.0e-7) - 1;
        let max_z = mc_bamboo_core::mth::floor(entity_box.max_z + 1.0e-7) + 1;
        let ctx_c = crate::shape::CollisionContext::Empty;
        let mut hit = false;
        'outer: for x in min_x..=max_x {
            for y in min_y..=max_y {
                for z in min_z..=max_z {
                    let p = BlockPos::new(x, y, z);
                    let st = blocks.block_state(p);
                    if st.is_air() {
                        continue;
                    }
                    for b in crate::shape::collision_boxes(st, p, ctx_c).iter() {
                        if b.moved(f64::from(x), f64::from(y), f64::from(z))
                            .intersects(entity_box)
                        {
                            hit = true;
                            break 'outer;
                        }
                    }
                }
            }
        }
        !hit
    };
    if no_collision {
        for pos in candidates {
            if !blocks.can_see_sky(pos)
                && blocks.is_loaded(pos)
                && !crate::shape::collision_boxes(
                    blocks.block_state(pos.below()),
                    pos.below(),
                    crate::shape::CollisionContext::Empty,
                )
                .is_empty()
            {
                cx.memories.set(
                    WalkTarget,
                    MemoryValue::Walk(WalkTarget {
                        target: PositionTracker::at_block(pos),
                        speed,
                        close_enough: 0,
                    }),
                );
                break;
            }
        }
    }
    true
}

fn trigger_meet_gate(state: &mut MeetGateState, cx: &mut Cx) -> bool {
    state.flip = !state.flip;
    if state.flip {
        if trigger_socialize_at_bell(cx) {
            return true;
        }
        if !cx.memories.has(WalkTarget) && cx.memories.has(MeetingPoint) {
            let _ = trigger_stroll_around_poi(MeetingPoint, 0.4, 40, &mut state.stroll_next_ok, cx);
        }
    } else {
        if !cx.memories.has(WalkTarget)
            && cx.memories.has(MeetingPoint)
            && trigger_stroll_around_poi(MeetingPoint, 0.4, 40, &mut state.stroll_next_ok, cx)
        {
            return true;
        }
        let _ = trigger_socialize_at_bell(cx);
    }
    true
}

/// `SetWalkTargetFromBlockMemory.create(memory, speed, closeEnough, tooFar, tooLong)`：
/// 走向记忆里的方块；太远时往那个方向随机挑一格，走不通太久则放弃并释放 POI。
///
/// `SetWalkTargetFromBlockMemory.create(memory, speed, closeEnough, tooFar, tooLong)`:
/// walk to the remembered block; when too far pick a random point towards it, and on a
/// long stall give up and release the POI.
fn trigger_walk_target_from_block_memory(
    memory: MemoryType,
    speed: f32,
    close_enough: u16,
    too_far_distance: u16,
    too_long_unreachable: u16,
    cx: &mut Cx,
) -> bool {
    let Some(MemoryValue::GlobalPos(global_pos)) = cx.memories.get(memory).cloned() else {
        return false;
    };
    let cant_reach = match cx.memories.get(CantReachWalkTargetSince) {
        Some(MemoryValue::Long(since)) => Some(*since),
        _ => None,
    };
    let wrong_dim = global_pos.dimension != "minecraft:overworld";
    let timed_out = cant_reach
        .is_some_and(|since| cx.time.saturating_sub(since) > i64::from(too_long_unreachable));
    if wrong_dim || timed_out {
        villager::release_poi_from_memories(
            cx.me,
            cx.mob.villager.profession,
            cx.memories,
            cx.ctx,
            memory,
        );
        cx.memories.erase(memory);
        cx.memories
            .set(CantReachWalkTargetSince, MemoryValue::Long(cx.time));
        return true;
    }
    let mob_pos = cx.me.block_position();
    let dist = manhattan(global_pos.pos, mob_pos);
    let too_far = i32::from(too_far_distance);
    let close = i32::from(close_enough);
    if dist > too_far {
        let towards = Vec3::new(
            f64::from(global_pos.pos.x) + 0.5,
            f64::from(global_pos.pos.y),
            f64::from(global_pos.pos.z) + 0.5,
        );
        let blocks = cx.ctx.blocks();
        let mut picked: Option<Vec3> = None;
        let mut tries = 0;
        while picked.is_none_or(|p| manhattan(BlockPos::containing(p), mob_pos) > too_far) {
            picked = goals::default_random_pos_towards(
                cx.me,
                cx.mob,
                blocks,
                15,
                7,
                towards,
                std::f32::consts::FRAC_PI_2,
            );
            tries += 1;
            if tries == 1000 {
                villager::release_poi_from_memories(
                    cx.me,
                    cx.mob.villager.profession,
                    cx.memories,
                    cx.ctx,
                    memory,
                );
                cx.memories.erase(memory);
                cx.memories
                    .set(CantReachWalkTargetSince, MemoryValue::Long(cx.time));
                return true;
            }
        }
        if let Some(target) = picked {
            cx.memories.set(
                WalkTarget,
                MemoryValue::Walk(WalkTarget::to_point(target, speed, close)),
            );
        }
    } else if dist > close {
        cx.memories.set(
            WalkTarget,
            MemoryValue::Walk(WalkTarget {
                target: PositionTracker::at_block(global_pos.pos),
                speed,
                close_enough: close,
            }),
        );
    }
    true
}

/// `PreRaidTriggerGate`：`TriggerGate.triggerOneShuffled` 的两个触发翻转各试一遍，
/// 先命中（成功设下步行目标）的那个停止，和 `MEET` 闸门用同一套确定性近似。
///
/// `PreRaidTriggerGate`: try the two `TriggerGate.triggerOneShuffled` triggers in the
/// two flip orders and stop at the first that fires (sets a walk target), using the same
/// deterministic approximation as the `MEET` gate.
fn trigger_pre_raid_gate(state: &mut PreRaidGateState, cx: &mut Cx) -> bool {
    state.flip = !state.flip;
    let speed = state.speed * 1.5;
    if state.flip {
        if trigger_walk_target_from_block_memory(MeetingPoint, speed, 2, 150, 200, cx) {
            return true;
        }
        let _ = trigger_village_bound_random_stroll(speed, 10, 7, cx);
    } else {
        if trigger_village_bound_random_stroll(speed, 10, 7, cx) {
            return true;
        }
        let _ = trigger_walk_target_from_block_memory(MeetingPoint, speed, 2, 150, 200, cx);
    }
    true
}

fn trigger_socialize_at_bell(cx: &mut Cx) -> bool {
    if !cx.memories.has(MeetingPoint)
        || !cx.memories.has(NearestVisibleLivingEntities)
        || cx.memories.has(InteractionTarget)
    {
        return false;
    }
    let Some(gp) = cx.memories.global_pos(MeetingPoint).cloned() else {
        return false;
    };
    if cx.level_next_int(100) != 0 {
        return false;
    }
    if gp.dimension != "minecraft:overworld" || !closer_to_center_than(gp.pos, cx.me.position, 4.0)
    {
        return false;
    }
    let has_villager =
        sensor::find_closest_visible(cx, &mut |view| view.mob_type == Some(MobType::Villager))
            .is_some();
    if !has_villager {
        return false;
    }
    let self_pos = cx.me.position;
    let target_uid = sensor::find_closest_visible(cx, &mut |view| {
        view.mob_type == Some(MobType::Villager) && view.position.distance_to_sqr(self_pos) <= 32.0
    });
    if let Some(uid) = target_uid {
        cx.memories.set(InteractionTarget, MemoryValue::Entity(uid));
        cx.memories.set(
            LookTarget,
            MemoryValue::Look(PositionTracker::Entity {
                uid,
                track_eye: true,
                target_eye: false,
            }),
        );
        cx.memories.set(
            WalkTarget,
            MemoryValue::Walk(WalkTarget {
                target: PositionTracker::Entity {
                    uid,
                    track_eye: false,
                    target_eye: false,
                },
                speed: 0.3,
                close_enough: 1,
            }),
        );
    }
    true
}

fn trigger_village_bound_random_stroll(speed: f32, max_xz: i32, max_y: i32, cx: &mut Cx) -> bool {
    let block_pos = cx.me.block_position();
    let blocks = cx.ctx.blocks();
    let pos = if blocks.poi_is_close_to_village(block_pos, 1) {
        goals::land_random_pos(cx.me, cx.mob, blocks, max_xz, max_y)
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
                max_xz,
                max_y,
                towards,
                std::f32::consts::FRAC_PI_2,
            )
        } else {
            goals::land_random_pos(cx.me, cx.mob, blocks, max_xz, max_y)
        }
    };
    cx.memories.set_or_erase(
        WalkTarget,
        pos.map(|at| MemoryValue::Walk(WalkTarget::to_point(at, speed, 0))),
    );
    true
}

fn trigger_locate_hiding_place(radius: i32, speed: f32, close_enough: i32, cx: &mut Cx) -> bool {
    let origin = cx.me.block_position();
    let home_mask = villager::poi_name_mask("home");
    let found = cx
        .ctx
        .blocks()
        .poi_in_range(origin, close_enough + 1, home_mask, 2)
        .into_iter()
        .next()
        .map(|(_, pos)| pos)
        .filter(|&pos| closer_to_center_than(pos, cx.me.position, f64::from(close_enough)))
        .or_else(|| {
            let mut list: Vec<BlockPos> = cx
                .ctx
                .blocks()
                .poi_in_range(origin, radius, home_mask, 2)
                .into_iter()
                .map(|(_, pos)| pos)
                .collect();
            if list.is_empty() {
                return None;
            }
            for i in (1..list.len()).rev() {
                let j = cx.me.random.next_int_bounded((i + 1) as i32) as usize;
                list.swap(i, j);
            }
            Some(list[0])
        })
        .or_else(|| cx.memories.global_pos(Home).map(|gp| gp.pos));

    if let Some(pos) = found {
        cx.memories.erase(Path);
        cx.memories.erase(LookTarget);
        cx.memories.erase(BreedTarget);
        cx.memories.erase(InteractionTarget);
        cx.memories.set(
            HidingPlace,
            MemoryValue::GlobalPos(GlobalPos {
                dimension: "minecraft:overworld".to_owned(),
                pos,
            }),
        );
        if !closer_to_center_than(pos, cx.me.position, f64::from(close_enough)) {
            cx.memories.set(
                WalkTarget,
                MemoryValue::Walk(WalkTarget {
                    target: PositionTracker::at_block(pos),
                    speed,
                    close_enough,
                }),
            );
        }
    }
    true
}

fn trigger_set_hidden_state(
    stay_hidden_ticks: i32,
    close_enough_dist: i32,
    ticks_hidden: &mut i32,
    cx: &mut Cx,
) -> bool {
    let Some(heard) = cx.memories.long(HeardBellTime) else {
        return false;
    };
    let bell_timed_out = heard + 300 <= cx.time;
    if *ticks_hidden > stay_hidden_ticks || bell_timed_out {
        cx.memories.erase(HeardBellTime);
        cx.memories.erase(HidingPlace);
        cx.update_activity_from_schedule();
        *ticks_hidden = 0;
        return true;
    }
    if let Some(gp) = cx.memories.global_pos(HidingPlace)
        && dist_sqr_block(gp.pos, cx.me.block_position())
            < f64::from(close_enough_dist * close_enough_dist)
    {
        *ticks_hidden += 1;
    }
    true
}

fn dist_sqr_block(a: BlockPos, b: BlockPos) -> f64 {
    let dx = f64::from(a.x - b.x);
    let dy = f64::from(a.y - b.y);
    let dz = f64::from(a.z - b.z);
    dx * dx + dy * dy + dz * dz
}

/// `MoveToSkySeeingSpot.hasNoBlocksAbove`：看得见天，且 MOTION_BLOCKING 高度图
/// 不超过村民的脚。
///
/// `MoveToSkySeeingSpot.hasNoBlocksAbove`: open sky and the MOTION_BLOCKING
/// heightmap does not rise above the villager's feet.
fn has_no_blocks_above(cx: &Cx) -> bool {
    let pos = cx.me.block_position();
    let view = cx.ctx.blocks();
    view.can_see_sky(pos) && f64::from(view.motion_blocking_y(pos)) <= cx.me.position.y
}

/// `CelebrateVillagersSurvivedRaid.checkExtraStartConditions`：袭击胜利且头顶无方块。
fn celebrate_can_use(cx: &mut Cx) -> bool {
    cx.ctx
        .blocks()
        .raid_at(cx.me.block_position())
        .is_some_and(|raid| raid.is_victory && has_no_blocks_above(cx))
}

/// `CelebrateVillagersSurvivedRaid.canStillUse`：袭击还在、还没停。
fn celebrate_can_continue(cx: &mut Cx) -> bool {
    cx.ctx
        .blocks()
        .raid_at(cx.me.block_position())
        .is_some_and(|raid| !raid.is_stopped)
}

/// `CelebrateVillagersSurvivedRaid.tick`：每刻 `nextInt(100)==0` 播庆祝声，
/// `nextInt(200)==0` 且头顶无方块时放烟花（随机颜色 + 0..3 飞行时长；烟花实体
/// 本身没建模，只保持取数次数对齐）。
fn celebrate_tick(cx: &mut Cx) {
    if cx.me.random.next_int_bounded(100) == 0 {
        living::make_sound(cx.me, cx.mob, "minecraft:entity.villager.celebrate");
    }
    if cx.me.random.next_int_bounded(200) == 0 && has_no_blocks_above(cx) {
        let _ = cx.me.random.next_int_bounded(16);
        let _ = cx.me.random.next_int_bounded(3);
    }
}

fn trigger_acquire_poi(state: &mut AcquirePoiState, cx: &mut Cx) -> bool {
    let slot = state.slot;
    if slot.only_if_adult() && cx.mob.is_baby() {
        return false;
    }
    if state.next_scheduled_start == 0 {
        state.next_scheduled_start = cx.time + i64::from(cx.level_next_int(20));
        return false;
    }
    if cx.time < state.next_scheduled_start {
        return false;
    }
    state.next_scheduled_start = cx.time + 20 + i64::from(cx.level_next_int(20));

    let now = cx.time;
    state
        .retry_cache
        .retain(|_, retry| retry.is_still_valid(now));

    let type_mask = slot.type_mask(cx.mob.villager.profession);
    if type_mask == 0 {
        return true;
    }

    const OCCUPANCY_HAS_SPACE: u8 = 0;
    let origin = cx.me.block_position();
    let in_range = cx
        .ctx
        .blocks()
        .poi_in_range(origin, 48, type_mask, OCCUPANCY_HAS_SPACE);

    // 精确复刻 `findAllClosestFirstWithType(..., retryFilter, origin, 48, HAS_SPACE).limit(5).filter(validPoi)`：
    // `retryFilter` 作用在排序之前，到期条目会立刻调用 `markAttempt` 抽一次 `level.random.nextInt(40)`。
    let mut filtered: Vec<(u8, BlockPos)> = Vec::new();
    for (kind, pos) in in_range {
        let pass = match state.retry_cache.get_mut(&pos) {
            None => true,
            Some(retry) => {
                if !retry.should_retry(now) {
                    false
                } else {
                    let r = cx.level_next_int(40);
                    retry.mark_attempt(now, r);
                    true
                }
            }
        };
        if pass {
            filtered.push((kind, pos));
        }
    }
    filtered.sort_by_key(|&(_, pos)| block_dist_sqr(origin, pos));
    filtered.truncate(5);
    let candidates: Vec<(u8, BlockPos)> = filtered
        .into_iter()
        .filter(|&(_, pos)| slot.valid_poi(cx.ctx.blocks(), pos))
        .collect();

    let path = find_path_to_pois(cx, &candidates);
    if let Some(path) = path
        && path.reached
    {
        let target = path.target;
        if cx.ctx.blocks().poi_type_at(target).is_some() {
            cx.ctx.emit(
                Command::PoiTake {
                    pos: target,
                    type_mask,
                },
                cx.me,
            );
            cx.memories.set(
                slot.acquiring_memory(),
                MemoryValue::GlobalPos(GlobalPos {
                    dimension: "minecraft:overworld".to_owned(),
                    pos: target,
                }),
            );
            if let Some(event) = slot.event_id() {
                cx.mob.events.push(MobEvent::EntityEvent { event });
            }
            state.retry_cache.clear();
        }
    } else {
        for &(_, pos) in &candidates {
            if let std::collections::btree_map::Entry::Vacant(slot) = state.retry_cache.entry(pos) {
                let r = cx.level_next_int(40);
                slot.insert(JitteredLinearRetry::new(now, r));
            }
        }
    }
    true
}

fn trigger_assign_profession(cx: &mut Cx) -> bool {
    let Some(gp) = cx.memories.global_pos(PotentialJobSite).cloned() else {
        return false;
    };
    if !closer_to_center_than(gp.pos, cx.me.position, 2.0) {
        return false;
    }
    cx.memories.erase(PotentialJobSite);
    cx.memories.set(JobSite, MemoryValue::GlobalPos(gp.clone()));
    cx.mob.events.push(MobEvent::EntityEvent { event: 14 });
    let none_id = facts::profession_id(facts::NONE_PROFESSION).unwrap_or(0);
    if cx.mob.villager.profession != none_id {
        return true;
    }
    if gp.dimension == "minecraft:overworld"
        && let Some(poi_kind) = cx.ctx.blocks().poi_type_at(gp.pos)
        && let Some(new_prof) = villager::profession_for_poi(poi_kind)
    {
        cx.mob.villager.set_profession(new_prof);
        cx.request_brain_refresh();
    }
    true
}

fn trigger_validate_nearby_poi(slot: ValidatePoiSlot, cx: &mut Cx) -> bool {
    let mem = slot.memory();
    let Some(gp) = cx.memories.global_pos(mem).cloned() else {
        return false;
    };
    if gp.dimension != "minecraft:overworld" || !closer_to_center_than(gp.pos, cx.me.position, 16.0)
    {
        return false;
    }
    let mask = slot.type_mask(cx.mob.villager.profession);
    let exists = cx
        .ctx
        .blocks()
        .poi_type_at(gp.pos)
        .is_some_and(|kind| (mask & (1_u32 << kind)) != 0);
    if !exists {
        cx.memories.erase(mem);
    } else if bed_is_occupied_by_other(cx.ctx.blocks(), gp.pos, cx.mob.villager.sleeping) {
        cx.memories.erase(mem);
        if !bed_is_occupied_by_villager(cx, gp.pos) {
            cx.ctx.emit(Command::PoiRelease { pos: gp.pos }, cx.me);
        }
    }
    true
}

fn bed_is_occupied_by_other(blocks: &dyn BlockView, pos: BlockPos, self_sleeping: bool) -> bool {
    let state = blocks.block_state(pos);
    state.is_tagged(block_tags::VILLAGERS_CAN_SLEEP_ON_BED) && state.get(OCCUPIED) && !self_sleeping
}

fn bed_is_occupied_by_villager(cx: &Cx, pos: BlockPos) -> bool {
    let area = Aabb::new(
        f64::from(pos.x),
        f64::from(pos.y),
        f64::from(pos.z),
        f64::from(pos.x + 1),
        f64::from(pos.y + 1),
        f64::from(pos.z + 1),
    );
    let mut found = false;
    cx.ctx.for_each_other_in(area, &mut |other| {
        if found || other.removed {
            return;
        }
        if let KindPublic::Mob(mob) = &other.kind
            && mob.mob_type == MobType::Villager
            && mob.is_alive()
        {
            found = true;
        }
    });
    found
}

fn trigger_yield_job_site(speed: f32, cx: &mut Cx) -> bool {
    if cx.mob.is_baby() {
        return false;
    }
    let none_id = facts::profession_id(facts::NONE_PROFESSION).unwrap_or(0);
    if cx.mob.villager.profession != none_id {
        return false;
    }
    let Some(gp) = cx.memories.global_pos(PotentialJobSite).cloned() else {
        return false;
    };
    let pos = gp.pos;
    let Some(poi_kind) = cx.ctx.blocks().poi_type_at(pos) else {
        return true;
    };
    let Some(living_ids) = cx.memories.entities(NearestLivingEntities) else {
        return true;
    };
    let ids: Vec<EntityUid> = living_ids.to_vec();
    let valid_range = villager::poi_valid_range(poi_kind);
    let mut chosen: Option<(EntityUid, bool)> = None;
    for uid in ids {
        if uid == cx.me.id {
            continue;
        }
        let Some(other) = cx.ctx.other(uid) else {
            continue;
        };
        if other.removed {
            continue;
        }
        let KindPublic::Mob(mob_pub) = &other.kind else {
            continue;
        };
        if mob_pub.mob_type != MobType::Villager || !mob_pub.is_alive() {
            continue;
        }
        let Some(vp) = mob_pub.villager.as_deref() else {
            continue;
        };
        if vp.potential_job_site.is_some() {
            continue;
        }
        let held_mask = villager::held_job_site_mask(vp.profession);
        if (held_mask & (1_u32 << poi_kind)) == 0 {
            continue;
        }
        let wants = match &vp.job_site {
            None => {
                can_other_villager_reach(cx.ctx.blocks(), &other, mob_pub.baby, pos, valid_range)
            }
            Some(existing) => existing.pos == pos,
        };
        if wants {
            chosen = Some((uid, vp.job_site.is_none()));
            break;
        }
    }
    if let Some((target_uid, had_no_job_site)) = chosen {
        cx.memories.erase(WalkTarget);
        cx.memories.erase(LookTarget);
        cx.memories.erase(PotentialJobSite);
        if had_no_job_site {
            sensor::set_partner_memory(
                cx,
                target_uid,
                WalkTarget,
                Some(MemoryValue::Walk(WalkTarget {
                    target: PositionTracker::at_block(pos),
                    speed,
                    close_enough: 1,
                })),
            );
            sensor::set_partner_memory(
                cx,
                target_uid,
                LookTarget,
                Some(MemoryValue::Look(PositionTracker::at_block(pos))),
            );
            sensor::set_partner_memory(
                cx,
                target_uid,
                PotentialJobSite,
                Some(MemoryValue::GlobalPos(GlobalPos {
                    dimension: "minecraft:overworld".to_owned(),
                    pos,
                })),
            );
        }
    }
    true
}

/// 替附近另一只村民评估 `YieldJobSite.canReachPos`（`GroundPathNavigation.createPath(pos, validRange)`）。
fn can_other_villager_reach(
    blocks: &dyn BlockView,
    other: &EntityPublic,
    baby: bool,
    pos: BlockPos,
    reach: i32,
) -> bool {
    if !blocks.is_loaded(pos) || other.position.y < f64::from(blocks.min_y()) {
        return false;
    }
    let in_liquid = blocks.fluid_state(other.block_position()).is_water()
        || blocks.fluid_state(other.block_position()).is_lava();
    if !other.on_ground && !in_liquid {
        return false;
    }
    let surface = find_surface_position(blocks, pos);
    let (width, height, _) = MobType::Villager.dimensions(baby);
    let flags = EvaluatorFlags {
        can_pass_doors: true,
        can_open_doors: true,
        can_float: true,
        ..EvaluatorFlags::default()
    };
    let request = PathRequest {
        mob: PathMob {
            position: other.position,
            on_ground: other.on_ground,
            in_floatable_fluid: in_liquid,
            in_water: blocks.fluid_state(other.block_position()).is_water(),
            width,
            height,
            max_up_step: 0.6,
            max_fall_distance: 3,
            malus: Malus::default(),
            context: CollisionContext::Empty,
        },
        flags,
        target: surface,
        max_distance_from_start: 48.0,
        reach_range: reach,
        max_visited_nodes: 48 * 16,
        max_visited_nodes_multiplier: 1.0,
    };
    path::find_path(blocks, &request).is_some_and(|p| p.reached)
}

fn trigger_reset_profession(cx: &mut Cx) -> bool {
    let none_id = facts::profession_id(facts::NONE_PROFESSION).unwrap_or(0);
    let nitwit_id = facts::profession_id("minecraft:nitwit").unwrap_or(-1);
    let prof = cx.mob.villager.profession;
    if prof != none_id && prof != nitwit_id && cx.mob.villager.xp == 0 && cx.mob.villager.level <= 1
    {
        cx.mob.villager.set_profession(none_id);
        cx.request_brain_refresh();
        true
    } else {
        false
    }
}

fn trigger_poi_competitor_scan(cx: &mut Cx) -> bool {
    let Some(gp) = cx.memories.global_pos(JobSite).cloned() else {
        return false;
    };
    let Some(poi_kind) = cx.ctx.blocks().poi_type_at(gp.pos) else {
        return true;
    };
    let Some(living_ids) = cx.memories.entities(NearestLivingEntities) else {
        return true;
    };
    let ids: Vec<EntityUid> = living_ids.to_vec();
    let mut winner_uid = cx.me.id;
    let mut winner_xp = cx.mob.villager.xp;
    for uid in ids {
        if uid == cx.me.id {
            continue;
        }
        let Some(other) = cx.ctx.other(uid) else {
            continue;
        };
        if other.removed {
            continue;
        }
        let KindPublic::Mob(mob_pub) = &other.kind else {
            continue;
        };
        if mob_pub.mob_type != MobType::Villager || !mob_pub.is_alive() {
            continue;
        }
        let Some(vp) = mob_pub.villager.as_deref() else {
            continue;
        };
        let same_site = vp.job_site.as_ref().is_some_and(|site| site == &gp);
        let matches_prof = (villager::held_job_site_mask(vp.profession) & (1_u32 << poi_kind)) != 0;
        if !(same_site && matches_prof) {
            continue;
        }
        // `PoiCompetitorScan.selectWinner(winner, other)`：严格大于胜出，否则后者胜出。
        if winner_xp > vp.experience {
            sensor::set_partner_memory(cx, uid, JobSite, None);
        } else {
            if winner_uid == cx.me.id {
                cx.memories.erase(JobSite);
            } else {
                sensor::set_partner_memory(cx, winner_uid, JobSite, None);
            }
            winner_uid = uid;
            winner_xp = vp.experience;
        }
    }
    true
}

fn is_hurt(cx: &Cx) -> bool {
    cx.memories.has(HurtBy)
}

fn has_hostile(cx: &Cx) -> bool {
    cx.memories.has(NearestHostile)
}

fn manhattan(a: BlockPos, b: BlockPos) -> i32 {
    (a.x - b.x).abs() + (a.y - b.y).abs() + (a.z - b.z).abs()
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

/// `LookAndFollowTradingPlayerSink.checkExtraStartConditions` / `canStillUse`：
/// 活着、有交易玩家、不在水里、没刚受伤、且离玩家不超过 4 格（距离平方 <= 16）。
///
/// `LookAndFollowTradingPlayerSink.checkExtraStartConditions` / `canStillUse`: alive, has a
/// trading player, not in water, not recently hurt, and within 4 blocks (squared <= 16) of
/// the player.
fn trading_sink_can_use(cx: &Cx) -> bool {
    let Some(uid) = cx.mob.villager.trading_player() else {
        return false;
    };
    if cx.me.removed || cx.mob.health <= 0.0 || cx.me.is_in_water() || cx.mob.hurt_time > 0 {
        return false;
    }
    let mut player_pos = None;
    cx.ctx.blocks().for_each_player(&mut |player| {
        if player.uid == uid {
            player_pos = Some(player.position);
        }
    });
    player_pos.is_some_and(|pos| cx.me.position.distance_to_sqr(pos) <= 16.0)
}

/// `LookAndFollowTradingPlayerSink.followPlayer`：把 WALK_TARGET / LOOK_TARGET 指到交易玩家。
///
/// `LookAndFollowTradingPlayerSink.followPlayer`: points WALK_TARGET / LOOK_TARGET at the
/// trading player.
fn follow_trading_player(cx: &mut Cx, speed: f32) {
    let Some(uid) = cx.mob.villager.trading_player() else {
        return;
    };
    cx.memories.set(
        WalkTarget,
        MemoryValue::Walk(WalkTarget {
            target: PositionTracker::Entity {
                uid,
                track_eye: false,
                target_eye: false,
            },
            speed,
            close_enough: 2,
        }),
    );
    cx.memories.set(
        LookTarget,
        MemoryValue::Look(PositionTracker::Entity {
            uid,
            track_eye: true,
            target_eye: false,
        }),
    );
}

fn interact_with(kind: MobType, speed: f32) -> Kind {
    Kind::Villager(Behavior::InteractWith {
        kind,
        max_dist: 8,
        speed,
        close_enough: 2,
    })
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
            (
                0,
                Kind::Villager(Behavior::ValidateNearbyPoi(ValidatePoiSlot::JobSite)),
            ),
            (
                0,
                Kind::Villager(Behavior::ValidateNearbyPoi(
                    ValidatePoiSlot::PotentialJobSite,
                )),
            ),
            (1, Kind::MoveToTargetSink(MoveToTargetSink::new(false))),
            (2, Kind::Villager(Behavior::PoiCompetitorScan)),
            (
                3,
                Kind::Villager(Behavior::LookAndFollowTradingPlayerSink { speed }),
            ),
            (
                5,
                Kind::Villager(Behavior::GoToWantedItem { speed, max_dist: 4 }),
            ),
            (
                6,
                Kind::Villager(Behavior::AcquirePoi(Box::new(AcquirePoiState::new(
                    PoiAcquireSlot::JobSite,
                )))),
            ),
            (7, Kind::Villager(Behavior::GoToPotentialJobSite { speed })),
            (8, Kind::Villager(Behavior::YieldJobSite { speed })),
            (
                10,
                Kind::Villager(Behavior::AcquirePoi(Box::new(AcquirePoiState::new(
                    PoiAcquireSlot::Home,
                )))),
            ),
            (
                10,
                Kind::Villager(Behavior::AcquirePoi(Box::new(AcquirePoiState::new(
                    PoiAcquireSlot::MeetingPoint,
                )))),
            ),
            (10, Kind::Villager(Behavior::AssignProfessionFromJobSite)),
            (10, Kind::Villager(Behavior::ResetProfession)),
        ],
    )
}

fn work_package(is_farmer: bool, speed: f32) -> ActivityData {
    let work_at_poi = Kind::Villager(Behavior::WorkAtPoi {
        is_composter: is_farmer,
        last_check: 0,
    });
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
                        (
                            Kind::Villager(Behavior::StrollAroundPoi {
                                memory: JobSite,
                                speed: 0.4,
                                max_dist: 4,
                                next_ok_start: 0,
                            }),
                            2,
                        ),
                        (
                            Kind::Villager(Behavior::StrollToPoi {
                                memory: JobSite,
                                speed: 0.4,
                                close_enough: 1,
                                max_dist: 10,
                                next_ok_start: 0,
                            }),
                            5,
                        ),
                        (
                            Kind::Villager(Behavior::StrollToPoiList {
                                list_memory: SecondaryJobSite,
                                primary_memory: JobSite,
                                speed,
                                close_enough: 1,
                                max_dist: 6,
                                next_ok_start: 0,
                            }),
                            5,
                        ),
                        (
                            Kind::Villager(Behavior::HarvestFarmland(Box::default())),
                            harvest_weight,
                        ),
                        (
                            Kind::Villager(Behavior::UseBonemeal(Box::default())),
                            bonemeal_weight,
                        ),
                    ],
                ))),
            ),
            (10, stub(Stub::ShowTradesToPlayer)),
            (10, stub(Stub::SetLookAndInteractPlayer)),
            (
                2,
                Kind::Villager(Behavior::WalkTargetFromBlockMemory {
                    memory: JobSite,
                    speed,
                    close_enough: 9,
                    too_far_distance: 100,
                    too_long_unreachable: 1200,
                }),
            ),
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
                        (interact_with(MobType::Villager, speed), 2),
                        (interact_with(MobType::Cat, speed), 1),
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
            (
                2,
                Kind::Villager(Behavior::WalkTargetFromBlockMemory {
                    memory: Home,
                    speed,
                    close_enough: 1,
                    too_far_distance: 150,
                    too_long_unreachable: 1200,
                }),
            ),
            (
                3,
                Kind::Villager(Behavior::ValidateNearbyPoi(ValidatePoiSlot::Home)),
            ),
            (
                3,
                Kind::Villager(Behavior::SleepInBed {
                    next_ok_start_time: 0,
                }),
            ),
            (
                5,
                Kind::Gate(Box::new(Gate::run_one(
                    vec![(Home, ValueAbsent)],
                    vec![
                        (
                            Kind::Villager(Behavior::SetClosestHomeAsWalkTarget(Box::new(
                                ClosestHomeState {
                                    speed,
                                    last_update: 0,
                                    batch_cache: Vec::new(),
                                },
                            ))),
                            1,
                        ),
                        (Kind::Villager(Behavior::InsideBrownianWalk { speed }), 4),
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

fn meet_package(speed: f32) -> ActivityData {
    ActivityData::with_priorities(
        Activity::Meet,
        vec![
            (
                2,
                Kind::Villager(Behavior::MeetTriggerGate(Box::new(MeetGateState {
                    stroll_next_ok: 0,
                    flip: false,
                }))),
            ),
            (10, stub(Stub::ShowTradesToPlayer)),
            (10, stub(Stub::SetLookAndInteractPlayer)),
            (
                2,
                Kind::Villager(Behavior::WalkTargetFromBlockMemory {
                    memory: MeetingPoint,
                    speed,
                    close_enough: 6,
                    too_far_distance: 100,
                    too_long_unreachable: 200,
                }),
            ),
            (3, stub(Stub::GiveGiftToHero)),
            (
                3,
                Kind::Villager(Behavior::ValidateNearbyPoi(ValidatePoiSlot::MeetingPoint)),
            ),
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
                        (interact_with(MobType::Villager, speed), 2),
                        (stub(Stub::InteractWithBreedVillager), 1),
                        (interact_with(MobType::Cat, speed), 1),
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

fn pre_raid_package(speed: f32) -> ActivityData {
    ActivityData::with_priorities(
        Activity::PreRaid,
        vec![
            (0, Kind::Villager(Behavior::RingBell)),
            (
                0,
                Kind::Villager(Behavior::PreRaidTriggerGate(Box::new(PreRaidGateState {
                    speed,
                    flip: false,
                }))),
            ),
            minimal_look_behavior(),
            (99, Kind::Villager(Behavior::ResetRaidStatus)),
        ],
    )
}

fn raid_package(speed: f32) -> ActivityData {
    ActivityData::with_priorities(
        Activity::Raid,
        vec![
            (
                0,
                Kind::Villager(Behavior::RaidCelebrateTriggerGate { speed: speed * 1.1 }),
            ),
            (0, Kind::Villager(Behavior::CelebrateVillagersSurvivedRaid)),
            (
                2,
                Kind::Villager(Behavior::RaidLocateHidingPlace {
                    radius: 24,
                    speed: speed * 1.4,
                    close_enough: 1,
                }),
            ),
            minimal_look_behavior(),
            (99, Kind::Villager(Behavior::ResetRaidStatus)),
        ],
    )
}

fn hide_package(speed: f32) -> ActivityData {
    ActivityData::with_priorities(
        Activity::Hide,
        vec![
            (
                0,
                Kind::Villager(Behavior::SetHiddenState {
                    stay_hidden_ticks: 15 * 20,
                    close_enough_dist: 3,
                    ticks_hidden: 0,
                }),
            ),
            (
                1,
                Kind::Villager(Behavior::LocateHidingPlace {
                    radius: 32,
                    speed: speed * 1.25,
                    close_enough: 2,
                }),
            ),
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

/// 判据（对照 Purpur `Villager.canTravelFrom` / `canTravelTo`）：
/// 检查村民脚部所在格的东、西、北、南四个水平邻格。任一邻格可水平进入（脚部格与成年村民的
/// 头部格都不是阻挡型 `PathType`，且下方 3 格内有可落脚方块或水）即视为未受困；
/// 四个水平邻格全部不可通行时判为被困（例如 1×1 石墙加工作站的交易隔间）。
///
/// Stuck criterion (matching Purpur `Villager.canTravelFrom` / `canTravelTo`):
/// checks the four horizontal neighbours (E/W/N/S) of the villager's feet block. If any
/// neighbour can be stepped into horizontally (passable `PathType` at feet and, for adults,
/// head, with solid footing or water within 3 blocks down), the villager is free; otherwise
/// all four neighbours are impassable and the villager is stuck (e.g. a 1×1 trading stall).
pub fn is_villager_stuck(me: &Entity, mob: &Mob, blocks: &dyn BlockView) -> bool {
    let feet = BlockPos::containing(Vec3::new(
        me.position.x,
        me.position.y + 0.0625,
        me.position.z,
    ));
    let adult = !mob.is_baby();
    const DIRS: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
    !DIRS
        .into_iter()
        .any(|(dx, dz)| can_step_into_neighbor(blocks, feet.offset(dx, 0, dz), adult))
}

fn can_step_into_neighbor(blocks: &dyn BlockView, pos: BlockPos, adult: bool) -> bool {
    use super::path;
    use mc_bamboo_data::PathType;

    let passable = |pt: PathType| {
        matches!(
            pt,
            PathType::Open
                | PathType::Walkable
                | PathType::WalkableDoor
                | PathType::DoorOpen
                | PathType::DoorWoodClosed
                | PathType::Rail
                | PathType::Water
                | PathType::WaterBorder
                | PathType::Trapdoor
                | PathType::OnTopOfTrapdoor
        )
    };
    if !passable(path::state_path_type(blocks.block_state(pos))) {
        return false;
    }
    if adult && !passable(path::state_path_type(blocks.block_state(pos.above()))) {
        return false;
    }
    // 下方 3 格内有可落脚方块或水即可（避免把悬空 1×1 柱子 / 岩浆沟误判成未受困）。
    // Require safe footing or water within 3 blocks down (so a 1×1 pillar over a pit/lava counts as stuck).
    for dy in 1..=3 {
        let below = path::state_path_type(blocks.block_state(pos.offset(0, -dy, 0)));
        if matches!(
            below,
            PathType::Lava | PathType::Fire | PathType::Damaging | PathType::DamageCautious
        ) {
            return false;
        }
        if below != PathType::Open {
            return true;
        }
    }
    false
}

/// `Villager.customServerAiStep`：推进大脑（支持三档降频、被困村民降频与睡觉村民降频，
/// 跳过刻由 `Brain::tick_by(gap)` 补齐），随后 `!isNoAi()` 时照官方抽
/// `this.random.nextInt(100)`（袭击冒汗粒子判定，保持随机序列一致）。
///
/// `Villager.customServerAiStep`: ticks the brain (supporting three-tier, stuck-villager
/// lobotomize, and sleeping-villager throttling via `Brain::tick_by(gap)`), then draws
/// `this.random.nextInt(100)` while `!isNoAi()` to keep the RNG sequence aligned with vanilla.
pub(super) fn custom_server_ai_step(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    decision_gap: i64,
) {
    let blocks = ctx.blocks();
    let rules = blocks.block_game_rules();
    let now = blocks.game_time();

    let lobotomize_on = rules.villager_lobotomize;
    let sleep_interval = i64::from(rules.villager_sleep_brain_interval.max(0));

    let effective_gap = if !lobotomize_on && sleep_interval <= 1 {
        // 精确模式快速路径：两项村民降频开关都关着，与官方逐位一致。
        // Exact-mode fast path: both villager throttling switches are off.
        mob.throttle.villager_last_brain = now;
        Some(decision_gap.max(1))
    } else {
        let general_exempt = super::throttle::villager_throttle_exempt(mob, now);
        let hostile_near = mob
            .brain
            .as_deref()
            .is_some_and(|brain| brain.memories.has(NearestHostile));
        let hurt_now = mob.hurt_time > 0
            || (mob.last_damage_source.is_some()
                && now.saturating_sub(mob.last_damage_stamp) <= 40)
            || mob
                .brain
                .as_deref()
                .is_some_and(|brain| brain.memories.has(HurtBy));

        let mut target_interval: i64 = 1;

        // 睡觉村民降频：钟声 / 敌对生物 / 恐慌（及交易、袭击、隐藏）豁免。
        if sleep_interval > 1 && mob.villager.sleeping && !general_exempt && !hostile_near {
            target_interval = target_interval.max(sleep_interval);
        }

        // 被困村民降频（照 Purpur lobotomize）：被打 / 敌对生物 / 恐慌 / 交易 / 钟声立刻回每刻。
        if lobotomize_on {
            let interval = i64::from(rules.villager_lobotomize_interval.max(1));
            let eligible = !general_exempt
                && !hurt_now
                && !hostile_near
                && (!rules.villager_lobotomize_wait_trade_lock || mob.villager.xp > 0);
            if !eligible {
                mob.throttle.villager_lobotomized = false;
                mob.throttle.villager_lobotomize_checked = i64::MIN;
            } else {
                let recheck = mob.throttle.villager_lobotomize_checked == i64::MIN
                    || now.saturating_sub(mob.throttle.villager_lobotomize_checked) >= interval;
                if recheck {
                    mob.throttle.villager_lobotomized = is_villager_stuck(me, mob, blocks);
                    mob.throttle.villager_lobotomize_checked = now;
                }
                if mob.throttle.villager_lobotomized {
                    target_interval = target_interval.max(interval);
                }
            }
        } else {
            mob.throttle.villager_lobotomized = false;
            mob.throttle.villager_lobotomize_checked = i64::MIN;
        }

        let last = mob.throttle.villager_last_brain;
        let run_now = if target_interval <= 1 || last == i64::MIN {
            true
        } else {
            let elapsed = now.saturating_sub(last);
            let phase = super::throttle::decision_phase(now, me.id);
            elapsed >= target_interval || phase.rem_euclid(target_interval) == 0
        };
        if run_now {
            let gap = if last == i64::MIN {
                decision_gap.max(1)
            } else {
                now.saturating_sub(last).max(decision_gap).max(1)
            };
            mob.throttle.villager_last_brain = now;
            Some(gap)
        } else {
            None
        }
    };

    if let Some(gap) = effective_gap {
        if let Some(mut brain) = mob.brain.take() {
            let replaced = brain.tick_by(me, mob, ctx, gap);
            mob.brain = Some(Box::new(replaced.unwrap_or(*brain)));
        }
        if !mob.no_ai {
            let _ = me.random.next_int_bounded(100);
        }
    }

    // `Villager.customServerAiStep` 末尾：上一笔交易玩家用于声望事件（期 6 接，这里只清），
    // 交易表为空却仍在交易时按官方 `stopTrading()`。
    // End of `Villager.customServerAiStep`: the last traded player feeds the reputation
    // event (phase 6; cleared here), and an empty offer list while trading triggers the
    // official `stopTrading()`.
    mob.villager.last_traded_player = None;
    let offers_empty = mob
        .villager
        .offers
        .as_ref()
        .is_none_or(|offers| offers.is_empty());
    if offers_empty && mob.villager.is_trading() {
        mob.villager.stop_trading();
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

    /// 被困判据：1×1 石墙 + 工作站隔间判被困，空地 / 木门 / 5×5 房间不困，
    /// 1 格净空成年困、幼年不困，四面深坑判被困。
    #[test]
    fn stuck_villager_detection_matches_four_neighbor_criterion() {
        use crate::BlockState;
        use mc_bamboo_data::blocks;

        let mut level = TestLevel::new();
        for x in -8..=8 {
            for z in -8..=8 {
                level.set(BlockPos::new(x, 63, z), BlockState(blocks::STONE));
            }
        }
        let mut ticks = BlockTicks::new();
        let adult_me = Entity::by_mob_type(MobType::Villager, Vec3::new(0.5, 64.0, 0.5));
        let adult_mob = Mob::new(MobType::Villager);
        let mut baby_mob = Mob::new(MobType::Villager);
        baby_mob.age = -24_000;

        // 空地：不困。
        {
            let world = World::new(&mut level, &mut ticks);
            assert!(!is_villager_stuck(&adult_me, &adult_mob, &world));
        }

        // 1×1 交易隔间：三面两格高石墙 + 前面工作站（木桶）。
        for y in 64..66 {
            level.set(BlockPos::new(-1, y, 0), BlockState(blocks::STONE));
            level.set(BlockPos::new(1, y, 0), BlockState(blocks::STONE));
            level.set(BlockPos::new(0, y, -1), BlockState(blocks::STONE));
        }
        level.set(BlockPos::new(0, 64, 1), BlockState(blocks::BARREL));
        {
            let world = World::new(&mut level, &mut ticks);
            assert!(is_villager_stuck(&adult_me, &adult_mob, &world));
        }

        // 把工作站换成橡木门：村民会开门，不算被困。
        level.set(BlockPos::new(0, 64, 1), BlockState(blocks::OAK_DOOR));
        {
            let world = World::new(&mut level, &mut ticks);
            assert!(!is_villager_stuck(&adult_me, &adult_mob, &world));
        }

        // 把前面留 1 格高矮缝（y=64 空气，y=65 石头）：成年被困，幼年不困。
        level.set(BlockPos::new(0, 64, 1), BlockState(blocks::AIR));
        level.set(BlockPos::new(0, 65, 1), BlockState(blocks::STONE));
        {
            let world = World::new(&mut level, &mut ticks);
            assert!(is_villager_stuck(&adult_me, &adult_mob, &world));
            assert!(!is_villager_stuck(&adult_me, &baby_mob, &world));
        }

        // 四面 4 格深坑的 1×1 孤柱：判被困。
        level.set(BlockPos::new(0, 65, 1), BlockState(blocks::AIR));
        let pillar_me = Entity::by_mob_type(MobType::Villager, Vec3::new(6.5, 64.0, 6.5));
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            level.set(BlockPos::new(6 + dx, 63, 6 + dz), BlockState(blocks::AIR));
        }
        {
            let world = World::new(&mut level, &mut ticks);
            assert!(is_villager_stuck(&pillar_me, &adult_mob, &world));
        }
    }

    /// 被困村民降频：`wait_trade_lock` 守门、按 `interval` 跳刻走 `Brain::tick_by(gap)`，
    /// 受伤 / 遇敌 / 恐慌 / 交易 / 钟声时立刻回到每刻运行。
    #[test]
    fn lobotomize_throttles_stuck_villager_and_wakes_immediately_on_danger() {
        use crate::BlockState;
        use mc_bamboo_data::blocks;

        let mut level = TestLevel::new();
        for x in -4..=4 {
            for z in -4..=4 {
                level.set(BlockPos::new(x, 63, z), BlockState(blocks::STONE));
            }
        }
        for y in 64..66 {
            level.set(BlockPos::new(-1, y, 0), BlockState(blocks::STONE));
            level.set(BlockPos::new(1, y, 0), BlockState(blocks::STONE));
            level.set(BlockPos::new(0, y, -1), BlockState(blocks::STONE));
        }
        level.set(BlockPos::new(0, 64, 1), BlockState(blocks::LECTERN));
        level.rules.villager_lobotomize = true;
        level.rules.villager_lobotomize_interval = 20;
        level.rules.villager_lobotomize_wait_trade_lock = true;

        let mut ticks = BlockTicks::new();
        let mut me = Entity::by_mob_type(MobType::Villager, Vec3::new(0.5, 64.0, 0.5));
        let mut mob = Mob::new(MobType::Villager);

        // xp == 0 且 wait_trade_lock == true：不降频，每刻都跑大脑。
        for t in 1..=5 {
            ticks.set_time(t);
            let mut world = World::new(&mut level, &mut ticks);
            let mut ctx = PreciseCtx::new(&mut world, me.id);
            custom_server_ai_step(&mut me, &mut mob, &mut ctx, 1);
            assert_eq!(mob.throttle.villager_last_brain, t);
            assert!(!mob.throttle.villager_lobotomized);
        }

        // 交易锁定（xp > 0）后进入被困降频：20 刻内只跑 1–2 次大脑，且带 TTL 的记忆在醒来时按 gap 扣减。
        mob.villager.xp = 1;
        mob.brain.as_mut().unwrap().memories.set_with_ttl(
            GolemDetectedRecently,
            MemoryValue::Bool(true),
            100,
        );
        let mut brain_runs = 0;
        for t in 6..=45 {
            ticks.set_time(t);
            let prev = mob.throttle.villager_last_brain;
            let mut world = World::new(&mut level, &mut ticks);
            let mut ctx = PreciseCtx::new(&mut world, me.id);
            custom_server_ai_step(&mut me, &mut mob, &mut ctx, 1);
            if mob.throttle.villager_last_brain != prev {
                brain_runs += 1;
            }
        }
        assert!(mob.throttle.villager_lobotomized);
        assert!(
            brain_runs <= 3,
            "expected <= 3 brain ticks across 40 ticks at interval=20, got {brain_runs}"
        );
        // 过期记忆已按跳过刻数推进（不会停留在 97+）。
        let ttl = mob
            .brain
            .as_ref()
            .unwrap()
            .memories
            .time_until_expiry(GolemDetectedRecently);
        assert!(ttl <= 70, "expected TTL to advance by gap, got {ttl}");

        // 被打时立刻回到每刻运行。
        mob.hurt_time = 10;
        for t in 46..=49 {
            ticks.set_time(t);
            let mut world = World::new(&mut level, &mut ticks);
            let mut ctx = PreciseCtx::new(&mut world, me.id);
            custom_server_ai_step(&mut me, &mut mob, &mut ctx, 1);
            assert_eq!(mob.throttle.villager_last_brain, t);
            assert!(!mob.throttle.villager_lobotomized);
        }
        mob.hurt_time = 0;

        // 身边有敌对生物时立刻回到每刻运行。
        mob.brain
            .as_mut()
            .unwrap()
            .memories
            .set(NearestHostile, MemoryValue::Entity(99));
        ticks.set_time(50);
        {
            let mut world = World::new(&mut level, &mut ticks);
            let mut ctx = PreciseCtx::new(&mut world, me.id);
            custom_server_ai_step(&mut me, &mut mob, &mut ctx, 1);
        }
        assert_eq!(mob.throttle.villager_last_brain, 50);
        assert!(!mob.throttle.villager_lobotomized);
    }

    /// 睡觉村民降频：睡着时每 N 刻跑大脑；听到钟声、敌对生物或恐慌时豁免，立刻回每刻。
    #[test]
    fn sleeping_villager_throttles_and_exempts_bell_hostile_panic() {
        let mut level = TestLevel::new();
        level.rules.villager_sleep_brain_interval = 5;
        let mut ticks = BlockTicks::new();
        let mut me = Entity::by_mob_type(MobType::Villager, Vec3::new(0.5, 64.0, 0.5));
        let mut mob = Mob::new(MobType::Villager);
        mob.villager.sleeping = true;
        // 把活动切到 REST，避免 `WakeUp` 立刻叫醒。
        mob.brain
            .as_mut()
            .unwrap()
            .set_active_activity_if_possible(Activity::Rest);

        let mut runs = 0;
        for t in 1..=20 {
            ticks.set_time(t);
            let prev = mob.throttle.villager_last_brain;
            let mut world = World::new(&mut level, &mut ticks);
            let mut ctx = PreciseCtx::new(&mut world, me.id);
            custom_server_ai_step(&mut me, &mut mob, &mut ctx, 1);
            if mob.throttle.villager_last_brain != prev {
                runs += 1;
            }
        }
        assert!(
            (4..=6).contains(&runs),
            "expected ~4-5 brain runs in 20 ticks at interval=5, got {runs}"
        );

        // 钟声有效期内豁免：每刻都跑。
        mob.brain
            .as_mut()
            .unwrap()
            .memories
            .set(HeardBellTime, MemoryValue::Long(20));
        for t in 21..=25 {
            ticks.set_time(t);
            let mut world = World::new(&mut level, &mut ticks);
            let mut ctx = PreciseCtx::new(&mut world, me.id);
            custom_server_ai_step(&mut me, &mut mob, &mut ctx, 1);
            assert_eq!(mob.throttle.villager_last_brain, t);
        }
    }

    /// 三档降频里的村民豁免：PANIC / RAID / PRE_RAID / HIDE、正在交易、HEARD_BELL_TIME 300 刻内。
    #[test]
    fn three_tier_throttle_exempts_villager_emergencies_trading_and_bell() {
        use super::super::throttle;

        let mut mob = Mob::new(MobType::Villager);
        assert!(!throttle::exempt(&mob, 1000, true, false, 0));

        // 正在交易。
        mob.villager.set_trading_player(Some(7));
        assert!(throttle::exempt(&mob, 1000, true, false, 0));
        mob.villager.stop_trading();

        // 四种紧急活动。
        for act in [
            Activity::Panic,
            Activity::Raid,
            Activity::PreRaid,
            Activity::Hide,
        ] {
            mob.brain
                .as_mut()
                .unwrap()
                .set_active_activity_if_possible(act);
            assert!(
                throttle::exempt(&mob, 1000, true, false, 0),
                "{act:?} must exempt villager from throttling"
            );
        }
        mob.brain
            .as_mut()
            .unwrap()
            .set_active_activity_if_possible(Activity::Idle);
        assert!(!throttle::exempt(&mob, 1000, true, false, 0));

        // HEARD_BELL_TIME：300 刻内豁免，超过 300 刻不再豁免。
        mob.brain
            .as_mut()
            .unwrap()
            .memories
            .set(HeardBellTime, MemoryValue::Long(1000));
        assert!(throttle::exempt(&mob, 1000, true, false, 0));
        assert!(throttle::exempt(&mob, 1299, true, false, 0));
        assert!(!throttle::exempt(&mob, 1300, true, false, 0));
    }
}
