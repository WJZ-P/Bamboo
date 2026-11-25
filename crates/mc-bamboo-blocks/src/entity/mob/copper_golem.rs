//! 铜傀儡：官方 `CopperGolem` / `CopperGolemAi`。大脑驱动，会氧化、打蜡、用斧刮锈，
//! 被雷击退一级氧化，完全氧化后有几率变成雕像方块。
//!
//! Copper golems: vanilla `CopperGolem` / `CopperGolemAi`. Brain-driven; they
//! oxidise over time, can be waxed, scraped with an axe, de-oxidised by lightning,
//! and may turn into a statue block once fully oxidised.

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::registries::attribute;

use super::{
    Mob, MobType,
    attributes::Attributes,
    brain::{
        Activity, ActivityData, Brain, BrainTemplate, Cx, TemplateCache,
        behavior::{Class, Gate, Kind, MoveToTargetSink},
        memory::{
            MemoryStatus::{ValueAbsent, ValuePresent},
            MemoryType,
        },
        sensor::SensorKind,
        template_cache,
    },
};

/// `WeatheringCopper.WeatherState`：四个氧化等级，存档名和网络 id 都是这个顺序。
///
/// `WeatheringCopper.WeatherState`: the four oxidation stages; saved names and
/// network ids follow this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WeatherState {
    #[default]
    Unaffected,
    Exposed,
    Weathered,
    Oxidized,
}

impl WeatherState {
    /// `ByIdMap` 网络 id。
    ///
    /// The `ByIdMap` network id.
    pub const fn id(self) -> i32 {
        match self {
            Self::Unaffected => 0,
            Self::Exposed => 1,
            Self::Weathered => 2,
            Self::Oxidized => 3,
        }
    }

    pub const fn from_id(id: i32) -> Self {
        match id {
            1 => Self::Exposed,
            2 => Self::Weathered,
            3 => Self::Oxidized,
            _ => Self::Unaffected,
        }
    }

    /// `getSerializedName()`，也是存档里的字符串。
    ///
    /// `getSerializedName()`, also the saved string.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Unaffected => "unaffected",
            Self::Exposed => "exposed",
            Self::Weathered => "weathered",
            Self::Oxidized => "oxidized",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        [
            Self::Unaffected,
            Self::Exposed,
            Self::Weathered,
            Self::Oxidized,
        ]
        .into_iter()
        .find(|state| state.name() == name)
    }

    /// 氧化下一级（到 `OXIDIZED` 为止）。
    ///
    /// The next oxidation stage (stops at `OXIDIZED`).
    pub const fn next(self) -> Self {
        match self {
            Self::Unaffected => Self::Exposed,
            Self::Exposed => Self::Weathered,
            Self::Weathered | Self::Oxidized => Self::Oxidized,
        }
    }

    /// 退一级氧化（到 `UNAFFECTED` 为止）。
    ///
    /// The previous oxidation stage (stops at `UNAFFECTED`).
    pub const fn previous(self) -> Self {
        match self {
            Self::Oxidized => Self::Weathered,
            Self::Weathered => Self::Exposed,
            Self::Exposed | Self::Unaffected => Self::Unaffected,
        }
    }
}

/// `CopperGolemState`：五种交互动画状态，存档名和网络 id 都是这个顺序。
///
/// `CopperGolemState`: the five interaction animation states; saved names and
/// network ids follow this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum State {
    #[default]
    Idle,
    GettingItem,
    GettingNoItem,
    DroppingItem,
    DroppingNoItem,
}

impl State {
    pub const fn id(self) -> i32 {
        match self {
            Self::Idle => 0,
            Self::GettingItem => 1,
            Self::GettingNoItem => 2,
            Self::DroppingItem => 3,
            Self::DroppingNoItem => 4,
        }
    }

    pub const fn from_id(id: i32) -> Self {
        match id {
            1 => Self::GettingItem,
            2 => Self::GettingNoItem,
            3 => Self::DroppingItem,
            4 => Self::DroppingNoItem,
            _ => Self::Idle,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::GettingItem => "getting_item",
            Self::GettingNoItem => "getting_no_item",
            Self::DroppingItem => "dropping_item",
            Self::DroppingNoItem => "dropping_no_item",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        [
            Self::Idle,
            Self::GettingItem,
            Self::GettingNoItem,
            Self::DroppingItem,
            Self::DroppingNoItem,
        ]
        .into_iter()
        .find(|state| state.name() == name)
    }
}

/// `CopperGolem.nextWeatheringTick` 的两个哨兵值。
///
/// The two sentinel values of `CopperGolem.nextWeatheringTick`.
pub const UNSET_WEATHERING_TICK: i64 = -1;
pub const IGNORE_WEATHERING_TICK: i64 = -2;
/// `nextIntBetweenInclusive(504000, 552000)`。
pub const WEATHERING_TICK_FROM: i64 = 504_000;
pub const WEATHERING_TICK_TO: i64 = 552_000;
/// `TURN_TO_STATUE_CHANCE`。
pub const TURN_TO_STATUE_CHANCE: f32 = 0.0058;
/// 构造时 `TRANSPORT_ITEMS_COOLDOWN_TICKS` 的 `[60, 100)`。
pub const SPAWN_COOLDOWN_MIN: i32 = 60;
pub const SPAWN_COOLDOWN_MAX: i32 = 100;

/// 铜傀儡自己的字段，别的生物恒为缺省值。
///
/// The copper golem's own fields; defaults for every other mob.
#[derive(Debug, Clone, PartialEq)]
pub struct CopperGolem {
    /// `DATA_WEATHER_STATE`（元数据 index 16）。
    pub weather_state: WeatherState,
    /// `COPPER_GOLEM_STATE`（元数据 index 17）。
    pub state: State,
    /// 绝对时刻（岛的时钟）；-1 未排程、-2 已打蜡不氧化。
    ///
    /// Absolute game time; -1 unscheduled, -2 waxed (no weathering).
    pub next_weathering_tick: i64,
    /// 上一道处理过的闪电 UUID，避免同一道闪电重复退锈。
    pub last_lightning_uuid: Option<[u8; 16]>,
    /// 正在开的箱子位置（`ContainerUser`）。
    pub opened_chest_pos: Option<BlockPos>,
    /// 客户端闲置/转头动画的起始刻（只在客户端算，这里仅为存档不建模）。
    pub idle_animation_start_tick: i32,
}

impl Default for CopperGolem {
    fn default() -> Self {
        Self {
            weather_state: WeatherState::Unaffected,
            state: State::Idle,
            next_weathering_tick: UNSET_WEATHERING_TICK,
            last_lightning_uuid: None,
            opened_chest_pos: None,
            idle_animation_start_tick: 0,
        }
    }
}

/// `EntityTypes.COPPER_GOLEM`：0.49×0.98，眼高 `0.98 * 0.75`。
///
/// `EntityTypes.COPPER_GOLEM`: 0.49 by 0.98, eye height `0.98 * 0.75`.
pub(super) const fn dimensions(baby: bool) -> (f32, f32, f32) {
    let _ = baby; // 铜傀儡没有幼年形态。Copper golems have no baby form.
    (0.49, 0.98, 0.735)
}

/// `CopperGolem.createAttributes`：生命 12、速度 0.2、台阶高度 1.0。
///
/// `CopperGolem.createAttributes`: 12 health, 0.2 speed and 1.0 step height.
pub(super) fn attributes() -> Attributes {
    let mut attributes = Attributes::cow();
    attributes.set_base(attribute::MAX_HEALTH, 12.0);
    attributes.set_base(attribute::MOVEMENT_SPEED, 0.2);
    attributes.set_base(attribute::STEP_HEIGHT, 1.0);
    attributes
}

/// `CopperGolemOxidationLevels` 里 `UNAFFECTED` / `EXPOSED` 共用一组声音。
///
/// `UNAFFECTED` / `EXPOSED` in `CopperGolemOxidationLevels` share one sound set.
const fn sound_suffix(weather: WeatherState) -> &'static str {
    match weather {
        WeatherState::Unaffected | WeatherState::Exposed => "",
        WeatherState::Weathered => "_weathered",
        WeatherState::Oxidized => "_oxidized",
    }
}

/// `getHurtSound(DamageSource)`。
///
/// `getHurtSound(DamageSource)`.
pub fn hurt_sound(weather: WeatherState) -> String {
    format!(
        "minecraft:entity.copper_golem{}.hurt",
        sound_suffix(weather)
    )
}

/// `getDeathSound()`。
pub fn death_sound(weather: WeatherState) -> String {
    format!(
        "minecraft:entity.copper_golem{}.death",
        sound_suffix(weather)
    )
}

/// `playStepSound` 的声音名。
///
/// The `playStepSound` sound name.
pub fn step_sound(weather: WeatherState) -> String {
    format!(
        "minecraft:entity.copper_golem{}.step",
        sound_suffix(weather)
    )
}

/// `getSpinHeadSound()`（转头动画的声音）。
///
/// `getSpinHeadSound()` (the head-spin animation sound).
pub fn spin_sound(weather: WeatherState) -> String {
    format!(
        "minecraft:entity.copper_golem{}.spin",
        sound_suffix(weather)
    )
}

/// `CopperGolem.BRAIN_PROVIDER`：两个传感器、核心与闲逛两个活动。
///
/// `CopperGolem.BRAIN_PROVIDER`: two sensors and the core / idle activities.
pub(super) fn brain() -> Brain {
    static TEMPLATE: TemplateCache = template_cache();
    Brain::cached(&TEMPLATE, || {
        BrainTemplate::new(
            &[SensorKind::NearestLivingEntities, SensorKind::HurtBy],
            vec![core_activity(), idle_activity()],
        )
    })
}

/// `initCoreActivity`（优先级 0）。
fn core_activity() -> ActivityData {
    ActivityData::sequential(
        Activity::Core,
        0,
        vec![
            Kind::AnimalPanic {
                speed: 1.5,
                environmental: false,
                armadillo: false,
                camel: false,
            },
            Kind::LookAtTargetSink { min: 45, max: 90 },
            Kind::MoveToTargetSink(MoveToTargetSink::new(false)),
            Kind::CountDownCooldown(MemoryType::GazeCooldownTicks),
            Kind::CountDownCooldown(MemoryType::TransportItemsCooldownTicks),
        ],
    )
}

/// `initIdleActivity`（优先级 0..2）。
fn idle_activity() -> ActivityData {
    ActivityData::with_priorities(
        Activity::Idle,
        vec![
            (
                0,
                Kind::CopperGolem(Behavior::Transport {
                    phase: TransportPhase::Searching,
                    interaction_tick: 0,
                }),
            ),
            (
                1,
                Kind::LookAtPlayerSometimes {
                    max_dist: 6.0,
                    min_interval: 40,
                    max_interval: 80,
                    ticks_until_next_start: 0,
                },
            ),
            (
                2,
                Kind::Gate(Box::new(Gate::run_one(
                    vec![
                        (MemoryType::WalkTarget, ValueAbsent),
                        (MemoryType::TransportItemsCooldownTicks, ValuePresent),
                    ],
                    vec![
                        (Kind::RandomStroll { speed: 1.0 }, 1),
                        (Kind::DoNothing { min: 30, max: 60 }, 1),
                    ],
                ))),
            ),
        ],
    )
}

/// `TransportItemsBetweenContainers` 的一次搬运所处的阶段。
///
/// The phase of one `TransportItemsBetweenContainers` run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransportPhase {
    /// 找源铜箱子 / 目标箱子。
    #[default]
    Searching,
    /// 走向目标箱子。
    Travelling,
    /// 到达后开箱、取/放物品的 60 刻动画。
    Interacting,
}

/// 铜傀儡大脑里的专属行为（目前只有 `TransportItemsBetweenContainers`）。
///
/// The copper golem's brain-only behaviour (`TransportItemsBetweenContainers`).
#[derive(Debug, Clone, PartialEq)]
pub enum Behavior {
    /// 在铜箱子与普通箱子之间搬运物品。
    Transport {
        phase: TransportPhase,
        interaction_tick: i32,
    },
}

impl Behavior {
    pub(super) fn class(&self) -> Class {
        // `TransportItemsBetweenContainers` 是 `Behavior`，时长取自世界随机源；
        // 这里用一个足够长的固定区间，由 `can_still_use` 决定何时真正停止。
        Class::Timed { min: 200, max: 200 }
    }

    pub(super) fn required(
        &self,
    ) -> &'static [(MemoryType, crate::entity::mob::brain::memory::MemoryStatus)] {
        &[]
    }

    pub(super) fn name(&self) -> &'static str {
        "TransportItemsBetweenContainers"
    }

    pub(super) fn trigger(&mut self, _cx: &mut Cx) -> bool {
        false
    }

    pub(super) fn extra_start(&mut self, cx: &mut Cx) -> bool {
        cx.mob.mob_type == MobType::CopperGolem
            && !cx.memories.has(MemoryType::IsPanicking)
            && cx
                .memories
                .int(MemoryType::TransportItemsCooldownTicks)
                .unwrap_or(0)
                == 0
    }

    pub(super) fn start(&mut self, _cx: &mut Cx) {}

    pub(super) fn can_still_use(&mut self, cx: &mut Cx) -> bool {
        let Self::Transport { phase, .. } = self;
        *phase != TransportPhase::Searching
            || cx
                .memories
                .int(MemoryType::TransportItemsCooldownTicks)
                .unwrap_or(0)
                == 0
    }

    pub(super) fn tick(&mut self, _cx: &mut Cx) {}

    pub(super) fn stop(&mut self, _cx: &mut Cx) {}
}

/// `customServerAiStep` 里铜傀儡的部分：跑大脑、把活动切到 `IDLE`。
///
/// The copper golem part of `customServerAiStep`: tick the brain and set the
/// active activity to the first valid one (`IDLE`).
pub(super) fn custom_server_ai_step(
    me: &mut crate::entity::Entity,
    mob: &mut Mob,
    ctx: &mut dyn crate::entity::ctx::EntityCtx,
) {
    if let Some(mut brain) = mob.brain.take() {
        let gap = ctx.throttle_plan().decision_gap;
        brain.tick_by(me, mob, ctx, gap);
        brain.set_active_activity_to_first_valid(&[Activity::Idle]);
        mob.brain = Some(brain);
    }
}
