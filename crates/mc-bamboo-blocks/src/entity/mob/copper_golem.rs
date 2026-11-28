//! 铜傀儡：官方 `CopperGolem` / `CopperGolemAi`。大脑驱动，会氧化、打蜡、用斧刮锈，
//! 被雷击退一级氧化，完全氧化后有几率变成雕像方块。
//!
//! Copper golems: vanilla `CopperGolem` / `CopperGolemAi`. Brain-driven; they
//! oxidise over time, can be waxed, scraped with an axe, de-oxidised by lightning,
//! and may turn into a statue block once fully oxidised.

use mc_bamboo_core::{BlockPos, Direction, Vec3};
use mc_bamboo_data::{
    blocks, items,
    registries::attribute,
    sounds::{self, sound_event},
    tags::item_tags,
};
use mc_bamboo_protocol::{item::ItemStack, packet::play::EquipmentSlot};

use super::{
    Mob, MobType,
    attributes::Attributes,
    brain::{
        Activity, ActivityData, Brain, BrainTemplate, Cx, TemplateCache,
        behavior::{Class, Gate, Kind, MoveToTargetSink},
        memory::{
            MemoryStatus::{Registered, ValueAbsent, ValuePresent},
            MemoryType,
        },
        sensor::SensorKind,
        template_cache,
    },
    interact::{MobInteractRequest, MobInteraction},
    living,
};
mod transport;

use crate::{
    InteractionResult,
    entity::{
        Entity,
        ctx::{Command, EntityCtx},
    },
    state::{BlockState, COPPER_GOLEM_POSE, CopperGolemPose, HORIZONTAL_FACING, item_is_tagged},
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
/// `TransportItemsBetweenContainers` 的常量（`javap -constants`）。
pub const TARGET_INTERACTION_TIME: i32 = 60;
pub const TRANSPORTED_ITEM_MAX_STACK_SIZE: i32 = 16;
pub const IDLE_COOLDOWN: i32 = 140;
pub const MAX_VISITED_POSITIONS: usize = 10;
pub const MAX_UNREACHABLE_POSITIONS: usize = 50;
pub const VISITED_POSITIONS_MEMORY_TIME: i64 = 6000;
pub const HORIZONTAL_SEARCH_RADIUS: i32 = 32;
pub const VERTICAL_SEARCH_RADIUS: i32 = 8;

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

/// `CopperGolemOxidationLevels` 里 `UNAFFECTED` / `EXPOSED` 共用一组声音；
/// `WEATHERED` / `OXIDIZED` 各用自己的一组。
///
/// In `CopperGolemOxidationLevels`, `UNAFFECTED` / `EXPOSED` share one sound set;
/// `WEATHERED` / `OXIDIZED` each use their own.
pub const fn hurt_sound(weather: WeatherState) -> &'static str {
    match weather {
        WeatherState::Unaffected | WeatherState::Exposed => "minecraft:entity.copper_golem.hurt",
        WeatherState::Weathered => "minecraft:entity.copper_golem_weathered.hurt",
        WeatherState::Oxidized => "minecraft:entity.copper_golem_oxidized.hurt",
    }
}

pub const fn death_sound(weather: WeatherState) -> &'static str {
    match weather {
        WeatherState::Unaffected | WeatherState::Exposed => "minecraft:entity.copper_golem.death",
        WeatherState::Weathered => "minecraft:entity.copper_golem_weathered.death",
        WeatherState::Oxidized => "minecraft:entity.copper_golem_oxidized.death",
    }
}

pub const fn step_sound(weather: WeatherState) -> &'static str {
    match weather {
        WeatherState::Unaffected | WeatherState::Exposed => "minecraft:entity.copper_golem.step",
        WeatherState::Weathered => "minecraft:entity.copper_golem_weathered.step",
        WeatherState::Oxidized => "minecraft:entity.copper_golem_oxidized.step",
    }
}

pub const fn spin_sound(weather: WeatherState) -> &'static str {
    match weather {
        WeatherState::Unaffected | WeatherState::Exposed => "minecraft:entity.copper_golem.spin",
        WeatherState::Weathered => "minecraft:entity.copper_golem_weathered.spin",
        WeatherState::Oxidized => "minecraft:entity.copper_golem_oxidized.spin",
    }
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
            Kind::InteractWithDoor,
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
                Kind::CopperGolem(Behavior::Transport(transport::Transport::default())),
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
                        (
                            Kind::RandomStroll {
                                speed: 1.0,
                                horizontal: 2,
                                vertical: 2,
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

/// 铜傀儡大脑里的专属行为（目前只有 `TransportItemsBetweenContainers`）。
///
/// The copper golem's brain-only behaviour (`TransportItemsBetweenContainers`).
#[derive(Debug, Clone, PartialEq)]
pub enum Behavior {
    /// 在铜箱子与普通箱子之间搬运物品。
    Transport(transport::Transport),
}

impl Behavior {
    pub(super) fn class(&self) -> Class {
        // `Behavior(Map)` 的默认时长 60..60；官方覆写了 `timedOut` 恒假，
        // 所以这里只影响 `tryStart` 消耗的那一次世界随机数（见 behaviors.rs）。
        Class::Timed { min: 60, max: 60 }
    }

    /// 官方构造器的四项入口条件。
    pub(super) fn required(
        &self,
    ) -> &'static [(MemoryType, crate::entity::mob::brain::memory::MemoryStatus)] {
        &[
            (MemoryType::VisitedBlockPositions, Registered),
            (MemoryType::UnreachableTransportBlockPositions, Registered),
            (MemoryType::TransportItemsCooldownTicks, ValueAbsent),
            (MemoryType::IsPanicking, ValueAbsent),
        ]
    }

    pub(super) fn name(&self) -> &'static str {
        "TransportItemsBetweenContainers"
    }

    pub(super) fn trigger(&mut self, _cx: &mut Cx) -> bool {
        false
    }

    /// `checkExtraStartConditions`：`!isLeashed`（拴绳未建模）。
    pub(super) fn extra_start(&mut self, cx: &mut Cx) -> bool {
        cx.mob.mob_type == MobType::CopperGolem
    }

    /// `start`：`GroundPathNavigation.setCanPathToTargetsBelowSurface(true)`。
    /// 官方大脑在 `tryStart` 的同一刻紧接着跑一次 `tick`，这里把首刻的目标搜索
    /// 一起做掉，避免活动/记忆快照差一相位。
    pub(super) fn start(&mut self, cx: &mut Cx) {
        let Self::Transport(state) = self;
        state.target = None;
        state.transport_state = transport::TransportState::Travelling;
        state.interaction = None;
        state.interaction_tick = 0;
        cx.mob.navigation.flags.can_open_doors = true;
        state.tick(cx);
    }

    /// `canStillUse`：冷却无值、不恐慌、没被拴住。
    pub(super) fn can_still_use(&mut self, cx: &mut Cx) -> bool {
        let Self::Transport(state) = self;
        state.can_use(cx)
    }

    pub(super) fn tick(&mut self, cx: &mut Cx) {
        let Self::Transport(state) = self;
        state.tick(cx);
    }

    /// `stop`：`onStartTravelling` + 关掉地下寻路开关。
    pub(super) fn stop(&mut self, cx: &mut Cx) {
        let Self::Transport(state) = self;
        state.stop(cx);
        cx.mob.navigation.flags.can_open_doors = false;
    }
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

/// `CopperGolem.updateWeathering` 这一刻的结果。
///
/// What one tick of `CopperGolem.updateWeathering` decides.
enum WeatheringOutcome {
    None,
    Statue { pose: u8, facing: Direction },
}

/// `CopperGolem.tick` 在 `super.tick()` 之后：只在服务端推进氧化计时，
/// 完全氧化且脚下是空气、掷中几率时变成雕像。
///
/// The part of `CopperGolem.tick` after `super.tick()`: advance the weathering
/// clock on the server and turn into a statue when fully oxidised, standing on
/// air and the chance roll hits.
pub(super) fn tick_after(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    if mob.mob_type != MobType::CopperGolem || me.removed {
        return;
    }
    if let WeatheringOutcome::Statue { pose, facing } = update_weathering(me, mob, ctx) {
        let pos = me.block_position();
        ctx.emit(Command::CopperGolemStatue { pos, pose, facing }, me);
    }
}

fn update_weathering(me: &mut Entity, mob: &mut Mob, ctx: &dyn EntityCtx) -> WeatheringOutcome {
    let blocks = ctx.blocks();
    let game_time = blocks.game_time();
    let span = (WEATHERING_TICK_TO - WEATHERING_TICK_FROM + 1) as i32;
    let cg = &mut mob.copper_golem;
    if cg.next_weathering_tick == IGNORE_WEATHERING_TICK {
        return WeatheringOutcome::None;
    }
    if cg.next_weathering_tick == UNSET_WEATHERING_TICK {
        // `nextIntBetweenInclusive(504000, 552000)`：`nextInt(48001) + 504000`。
        cg.next_weathering_tick =
            game_time + i64::from(blocks.level_random_int(span)) + WEATHERING_TICK_FROM;
        return WeatheringOutcome::None;
    }
    let was_oxidized = cg.weather_state == WeatherState::Oxidized;
    if game_time >= cg.next_weathering_tick && !was_oxidized {
        let next = cg.weather_state.next();
        let next_is_oxidized = next == WeatherState::Oxidized;
        cg.weather_state = next;
        cg.next_weathering_tick = if next_is_oxidized {
            0
        } else {
            cg.next_weathering_tick
                + i64::from(blocks.level_random_int(span))
                + WEATHERING_TICK_FROM
        };
    }
    if was_oxidized {
        // `canTurnToStatue`：脚下方块是空气且 `level.random.nextFloat() <= 0.0058`。
        let pos = me.block_position();
        let air = blocks.block_state(pos).is_air();
        let roll = blocks.level_random_float();
        if air && roll <= TURN_TO_STATUE_CHANCE {
            let pose = me.random.next_int_bounded(4) as u8;
            let facing = Direction::from_yaw(f64::from(me.y_rot));
            return WeatheringOutcome::Statue { pose, facing };
        }
    }
    WeatheringOutcome::None
}

/// `CopperGolemStatueBlock.Pose.values()[id]`。
fn pose_from_id(id: u8) -> CopperGolemPose {
    match id {
        1 => CopperGolemPose::Sitting,
        2 => CopperGolemPose::Running,
        3 => CopperGolemPose::Star,
        _ => CopperGolemPose::Standing,
    }
}

/// `Command::CopperGolemStatue` 的落地：放氧化雕像、掉下携带物、播音效、移除实体。
///
/// Applying `Command::CopperGolemStatue`: place the oxidised statue, drop carried
/// items, play the sound and discard the golem.
pub(crate) fn apply_statue(
    world: &mut crate::world::World,
    me: &mut Entity,
    pos: BlockPos,
    pose: u8,
    facing: Direction,
) {
    let state = BlockState(blocks::OXIDIZED_COPPER_GOLEM_STATUE)
        .with(COPPER_GOLEM_POSE, pose_from_id(pose))
        .with(HORIZONTAL_FACING, facing);
    world.set_block_and_update(pos, state);
    // `CopperGolemStatueBlockEntity.createStatue`：把自定义名搬进雕像的组件表。
    // `createStatue` copies the golem's custom name into the statue's components.
    let custom_name = me.kind.mob().and_then(|mob| mob.custom_name.clone());
    world.set_statue_custom_name(pos, custom_name);
    if let Some(mob) = me.kind.mob_mut() {
        for slot in [EquipmentSlot::MainHand, EquipmentSlot::Saddle] {
            let stack = mob.equipment.get(slot).clone();
            if !stack.is_empty() {
                world.spawn_at_location(me.position, stack);
                mob.equipment.set(slot, ItemStack::empty());
            }
        }
    }
    world.play_sound(
        Vec3::new(
            f64::from(pos.x) + 0.5,
            f64::from(pos.y) + 0.05,
            f64::from(pos.z) + 0.5,
        ),
        sound_event("minecraft:entity.copper_golem_become_statue"),
        sounds::NEUTRAL,
        1.0,
        1.0,
        false,
    );
    me.discard();
}

/// `clearMemoriesAfterMatchingTargetFound`：清掉搬运用的两处位置集合记忆。
fn clear_transport_memories(mob: &mut Mob) {
    if let Some(brain) = mob.brain.as_mut() {
        brain.memories.erase(MemoryType::VisitedBlockPositions);
        brain
            .memories
            .erase(MemoryType::UnreachableTransportBlockPositions);
    }
}

/// `TransportItemsBetweenContainers.pickUpItems` 的落地：从箱子取最多 16 个进主手。
///
/// 只读查询决定要取之后才发这条命令；落地阶段按 uid 顺序串行应用，若另一只傀儡
/// 已经把东西取走，这里读到空箱子就什么都不做（官方「先到先得」）。
pub(crate) fn apply_pick_up(world: &mut crate::world::World, me: &mut Entity, pos: BlockPos) {
    let mut taken = ItemStack::empty();
    let found = world.with_container(pos, &mut |container| {
        for slot in 0..container.size() {
            let stack = container.get(slot);
            if !stack.is_empty() {
                taken =
                    container.remove_item(slot, stack.count.min(TRANSPORTED_ITEM_MAX_STACK_SIZE));
                break;
            }
        }
    });
    if !found || taken.is_empty() {
        return;
    }
    // `emit` 精确驱动当场落地，但此时 `me.kind` 已被 `living::tick` 借走换成占位；
    // 生物状态在 `Cx.mob` 上，由行为本体负责设置（见 transport.rs）。这里只补上
    // 从存档 / 其它命令路径直接调用时的装备写入。
    if let Some(mob) = me.kind.mob_mut() {
        mob.equipment.set(EquipmentSlot::MainHand, taken.clone());
        clear_transport_memories(mob);
    }
}

/// `TransportItemsBetweenContainers.addItemsToContainer` 的落地：主手物品进箱子，
/// 剩余留主手。逐槽合并 / 填空，和官方一样用「可容纳空间」而不是实际移动数去减主手。
pub(crate) fn apply_put_down(
    world: &mut crate::world::World,
    me: &mut Entity,
    pos: BlockPos,
    held_stack: &ItemStack,
) {
    let mut held = held_stack.clone();
    if held.is_empty() {
        return;
    }
    let mut remaining = held.clone();
    let found = world.with_container(pos, &mut |container| {
        for slot in 0..container.size() {
            let stack = container.get(slot);
            if stack.is_empty() {
                container.set(slot, held.clone());
                remaining = ItemStack::empty();
                return;
            }
            if crate::container::same_item_same_components(&stack, &held)
                && stack.count < container.max_stack_size_of(&stack)
            {
                let space = container.max_stack_size_of(&stack) - stack.count;
                let moved = space.min(held.count);
                let mut merged = stack.clone();
                merged.count += moved;
                container.set(slot, merged);
                // 官方写的是 `hand.count -= space`（不是 moved）。
                held.count -= space;
                if held.is_empty() {
                    remaining = ItemStack::empty();
                    return;
                }
            }
        }
        remaining = held.clone();
    });
    if !found {
        return;
    }
    if let Some(mob) = me.kind.mob_mut() {
        mob.equipment
            .set(EquipmentSlot::MainHand, remaining.clone());
        if remaining.is_empty() {
            clear_transport_memories(mob);
        }
    }
}

/// `CopperGolem.mobInteract`：空手扔回携带物、剪下天线、蜜脾打蜡、斧头去蜡 / 刮锈。
///
/// `CopperGolem.mobInteract`: empty hand throws the carried item, shears remove the
/// antenna, honeycomb waxes, an axe removes wax or scrapes one oxidation stage off.
pub(super) fn interact(me: &Entity, mob: &mut Mob, req: &MobInteractRequest) -> MobInteraction {
    let held = req.held.item;
    if held == items::AIR {
        let carried = mob.equipment.get(EquipmentSlot::MainHand).clone();
        if !carried.is_empty() {
            mob.equipment
                .set(EquipmentSlot::MainHand, ItemStack::empty());
            living::play_sound(me, mob, "minecraft:entity.copper_golem.item_drop", 1.0, 1.0);
            return MobInteraction {
                result: InteractionResult::Success,
                drop_stack: Some(carried),
                ..MobInteraction::pass()
            };
        }
    }
    if held == items::SHEARS {
        let antenna = mob.equipment.get(EquipmentSlot::Saddle).clone();
        if item_is_tagged(antenna.item, item_tags::SHEARABLE_FROM_COPPER_GOLEM) {
            mob.equipment.set(EquipmentSlot::Saddle, ItemStack::empty());
            living::play_sound(me, mob, "minecraft:entity.copper_golem.shear", 1.0, 1.0);
            return MobInteraction {
                result: InteractionResult::Success,
                drop_stack: Some(antenna),
                damage_held: true,
                ..MobInteraction::pass()
            };
        }
    }
    if held == items::HONEYCOMB && mob.copper_golem.next_weathering_tick != IGNORE_WEATHERING_TICK {
        mob.copper_golem.next_weathering_tick = IGNORE_WEATHERING_TICK;
        living::play_sound(me, mob, "minecraft:item.honeycomb.wax_on", 1.0, 1.0);
        return MobInteraction {
            result: InteractionResult::SuccessServer,
            shrink_held: !req.creative,
            // `level.levelEvent(this, 3003, blockPosition(), 0)`：打蜡粒子。
            level_events: vec![(3003, me.block_position(), 0)],
            ..MobInteraction::pass()
        };
    }
    let is_axe = item_is_tagged(held, item_tags::AXES);
    if is_axe && mob.copper_golem.next_weathering_tick == IGNORE_WEATHERING_TICK {
        mob.copper_golem.next_weathering_tick = UNSET_WEATHERING_TICK;
        living::play_sound(me, mob, "minecraft:item.axe.scrape", 1.0, 1.0);
        return MobInteraction {
            result: InteractionResult::SuccessServer,
            damage_held: true,
            // `level.levelEvent(this, 3004, blockPosition(), 0)`：去蜡粒子。
            level_events: vec![(3004, me.block_position(), 0)],
            ..MobInteraction::pass()
        };
    }
    if is_axe && mob.copper_golem.weather_state != WeatherState::Unaffected {
        mob.copper_golem.next_weathering_tick = UNSET_WEATHERING_TICK;
        mob.copper_golem.weather_state = mob.copper_golem.weather_state.previous();
        living::play_sound(me, mob, "minecraft:item.axe.scrape", 1.0, 1.0);
        return MobInteraction {
            result: InteractionResult::SuccessServer,
            damage_held: true,
            // `level.levelEvent(this, 3005, blockPosition(), 0)`：刮锈粒子。
            level_events: vec![(3005, me.block_position(), 0)],
            ..MobInteraction::pass()
        };
    }
    MobInteraction::pass()
}

/// `CopperGolem.thunderHit`：同一道闪电只处理一次，非 `UNAFFECTED` 时退一级氧化。
///
/// `CopperGolem.thunderHit`: one bolt is handled once; a non-`UNAFFECTED` golem
/// loses one oxidation stage.
pub fn thunder_hit(mob: &mut Mob, bolt: [u8; 16]) {
    if mob.copper_golem.last_lightning_uuid == Some(bolt) {
        return;
    }
    mob.copper_golem.last_lightning_uuid = Some(bolt);
    if mob.copper_golem.weather_state != WeatherState::Unaffected {
        mob.copper_golem.next_weathering_tick = UNSET_WEATHERING_TICK;
        mob.copper_golem.weather_state = mob.copper_golem.weather_state.previous();
    }
}

/// `CopperGolem.actuallyHurt` 在 `super` 之后把状态设回 `IDLE`。
///
/// `CopperGolem.actuallyHurt` resets the state to `IDLE` after `super`.
pub fn after_hurt(mob: &mut Mob) {
    if mob.mob_type == MobType::CopperGolem {
        mob.copper_golem.state = State::Idle;
    }
}

/// `CarvedPumpkinBlock` 生成铜傀儡后 `CopperGolem.spawn(WeatherState)` 的那一半：
/// 按图案里铜块的氧化等级设初始氧化状态（声音由方块动作负责）。
///
/// The `CopperGolem.spawn(WeatherState)` half after the pumpkin places a golem:
/// set the initial oxidation stage from the copper block (the sound belongs to the
/// block action).
pub fn spawn(mob: &mut Mob, weather: WeatherState) {
    mob.copper_golem.weather_state = weather;
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::Hand;
    use mc_bamboo_protocol::item::ItemStack;

    use super::*;
    use crate::entity::mob::nbt;

    fn request(item: ItemStack) -> MobInteractRequest {
        MobInteractRequest {
            player_uid: 1,
            player_uuid: [0; 16],
            hand: Hand::Main,
            held: item,
            creative: false,
            position: Vec3::default(),
            player_height: 1.8,
            sneaking: false,
            difficulty: 2,
            hit_pos: None,
            spectator: false,
        }
    }

    /// `living::play_sound` 只需要实体的位置，这里用一头牛当发声坐标。
    fn dummy() -> Entity {
        Entity::by_mob_type(MobType::Cow, Vec3::default())
    }

    #[test]
    fn weather_and_state_ids_match_vanilla() {
        assert_eq!(WeatherState::Unaffected.id(), 0);
        assert_eq!(WeatherState::Oxidized.id(), 3);
        assert_eq!(WeatherState::from_id(2), WeatherState::Weathered);
        assert_eq!(
            WeatherState::from_name("exposed"),
            Some(WeatherState::Exposed)
        );
        assert_eq!(State::from_id(4), State::DroppingNoItem);
        assert_eq!(State::DroppingItem.name(), "dropping_item");
    }

    #[test]
    fn oxidation_steps_and_stops_at_the_ends() {
        assert_eq!(WeatherState::Unaffected.next(), WeatherState::Exposed);
        assert_eq!(WeatherState::Weathered.next(), WeatherState::Oxidized);
        assert_eq!(WeatherState::Oxidized.next(), WeatherState::Oxidized);
        assert_eq!(
            WeatherState::Unaffected.previous(),
            WeatherState::Unaffected
        );
        assert_eq!(WeatherState::Oxidized.previous(), WeatherState::Weathered);
    }

    #[test]
    fn sounds_follow_the_oxidation_level() {
        assert_eq!(
            hurt_sound(WeatherState::Unaffected),
            "minecraft:entity.copper_golem.hurt"
        );
        assert_eq!(
            hurt_sound(WeatherState::Exposed),
            "minecraft:entity.copper_golem.hurt"
        );
        assert_eq!(
            death_sound(WeatherState::Weathered),
            "minecraft:entity.copper_golem_weathered.death"
        );
        assert_eq!(
            step_sound(WeatherState::Oxidized),
            "minecraft:entity.copper_golem_oxidized.step"
        );
    }

    #[test]
    fn honeycomb_waxes_and_axe_unwaxes() {
        let mut mob = Mob::new(MobType::CopperGolem);
        let me = dummy();
        assert_eq!(mob.copper_golem.next_weathering_tick, UNSET_WEATHERING_TICK);
        let out = interact(&me, &mut mob, &request(ItemStack::new(items::HONEYCOMB, 1)));
        assert_eq!(out.result, InteractionResult::SuccessServer);
        assert!(out.shrink_held);
        assert_eq!(out.level_events, vec![(3003, me.block_position(), 0)]);
        assert_eq!(
            mob.copper_golem.next_weathering_tick,
            IGNORE_WEATHERING_TICK
        );
        // 再打蜡一次无效（官方要求 `nextWeatheringTick != -2`）。
        interact(&me, &mut mob, &request(ItemStack::new(items::HONEYCOMB, 1)));
        assert_eq!(
            mob.copper_golem.next_weathering_tick,
            IGNORE_WEATHERING_TICK
        );
        let out = interact(
            &me,
            &mut mob,
            &request(ItemStack::new(items::DIAMOND_AXE, 1)),
        );
        assert_eq!(out.result, InteractionResult::SuccessServer);
        assert!(out.damage_held);
        assert_eq!(out.level_events, vec![(3004, me.block_position(), 0)]);
        assert_eq!(mob.copper_golem.next_weathering_tick, UNSET_WEATHERING_TICK);
    }

    #[test]
    fn axe_scrapes_one_oxidation_stage_off() {
        let mut mob = Mob::new(MobType::CopperGolem);
        mob.copper_golem.weather_state = WeatherState::Oxidized;
        let me = dummy();
        let out = interact(
            &me,
            &mut mob,
            &request(ItemStack::new(items::DIAMOND_AXE, 1)),
        );
        assert_eq!(out.result, InteractionResult::SuccessServer);
        assert_eq!(out.level_events, vec![(3005, me.block_position(), 0)]);
        assert_eq!(mob.copper_golem.weather_state, WeatherState::Weathered);
        assert_eq!(mob.copper_golem.next_weathering_tick, UNSET_WEATHERING_TICK);
    }

    #[test]
    fn thunder_deoxidises_once_per_bolt() {
        let mut mob = Mob::new(MobType::CopperGolem);
        mob.copper_golem.weather_state = WeatherState::Weathered;
        let bolt = [7; 16];
        thunder_hit(&mut mob, bolt);
        assert_eq!(mob.copper_golem.weather_state, WeatherState::Exposed);
        // 同一道闪电不再退。
        thunder_hit(&mut mob, bolt);
        assert_eq!(mob.copper_golem.weather_state, WeatherState::Exposed);
    }

    #[test]
    fn hurt_resets_the_interaction_state() {
        let mut mob = Mob::new(MobType::CopperGolem);
        mob.copper_golem.state = State::GettingItem;
        after_hurt(&mut mob);
        assert_eq!(mob.copper_golem.state, State::Idle);
    }

    #[test]
    fn save_round_trip_keeps_weather_state_and_clock() {
        let mut entity = Entity::by_mob_type(MobType::CopperGolem, Vec3::default());
        entity.with_mob(|_, mob| {
            mob.copper_golem.weather_state = WeatherState::Weathered;
            mob.copper_golem.next_weathering_tick = 123_456;
        });
        let tag = nbt::write_mob(&entity.split()).expect("write copper golem");
        let mut loaded = Entity::by_mob_type(MobType::CopperGolem, Vec3::default());
        nbt::load_into(&mut loaded, &tag).expect("load copper golem");
        loaded.with_mob(|_, mob| {
            assert_eq!(mob.copper_golem.weather_state, WeatherState::Weathered);
            assert_eq!(mob.copper_golem.next_weathering_tick, 123_456);
        });
    }
}

#[cfg(test)]
mod statue_tests {
    use mc_bamboo_nbt::Tag;

    use super::*;
    use crate::{testing::TestLevel, ticks::BlockTicks, world::World};

    /// `canTurnToStatue`：`level.random.nextFloat() <= 0.0058` 掷中时发
    /// `CopperGolemStatue` 命令，和官方在同一位置取一次世界随机数。
    /// `LegacyRandomSource(5125).nextFloat()` 约 0.0057137，必定掷中；
    /// 换个种子让 `nextFloat` 大于 0.0058 就必定不中。
    #[test]
    fn statue_roll_uses_one_level_random_float() {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        ticks.set_time(1000);
        let pos = BlockPos::new(1, 64, 2);
        let mut entity = Entity::by_mob_type(MobType::CopperGolem, Vec3::new(1.5, 64.0, 2.5));
        entity.with_mob(|_, mob| {
            mob.copper_golem.weather_state = WeatherState::Oxidized;
            mob.copper_golem.next_weathering_tick = 0;
        });
        let mut mob = entity.kind.mob_mut().unwrap().clone();
        {
            let mut world = World::new(&mut level, &mut ticks);
            // `LegacyRandomSource(5125)` 首个 `nextFloat()` 约 0.0057137，
            // 小于官方 `TURN_TO_STATUE_CHANCE` 的 0.0058。
            world.random().set_seed(5125);
            let seed_before = world.random().seed();
            let mut ctx = crate::entity::ctx::PreciseCtx::new(&mut world, entity.id);
            tick_after(&mut entity, &mut mob, &mut ctx);
            // 掷中后世界随机源恰好前进一格（只取了一个 `nextFloat()`）。
            assert_ne!(world.random().seed(), seed_before);
        }
        // 精确驱动的 `emit` 当场落地：雕像方块已放、实体已消失。
        eprintln!(
            "DBG state={:?} removed={} pos_block={:?}",
            level.get(pos),
            entity.removed,
            entity.block_position()
        );
        assert!(
            level
                .get(pos)
                .is(mc_bamboo_data::blocks::OXIDIZED_COPPER_GOLEM_STATUE)
        );
        assert!(entity.removed);
    }

    /// 同一段路径在 `nextFloat()` 大于 0.0058 时不发命令、不改方块。
    #[test]
    fn statue_roll_miss_leaves_the_golem() {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        ticks.set_time(1000);
        let mut entity = Entity::by_mob_type(MobType::CopperGolem, Vec3::new(1.5, 64.0, 2.5));
        entity.with_mob(|_, mob| {
            mob.copper_golem.weather_state = WeatherState::Oxidized;
            mob.copper_golem.next_weathering_tick = 0;
        });
        let mut mob = entity.kind.mob_mut().unwrap().clone();
        {
            let mut world = World::new(&mut level, &mut ticks);
            // 种子 42 的首个 `nextFloat()` 远大于 0.0058，必定不中。
            world.random().set_seed(42);
            let mut ctx = crate::entity::ctx::PreciseCtx::new(&mut world, entity.id);
            tick_after(&mut entity, &mut mob, &mut ctx);
        }
        assert!(!entity.removed);
        assert!(level.get(BlockPos::new(1, 64, 2)).is_air());
    }

    /// `createStatue` 把铜傀儡的自定义名写进雕像的组件表。
    #[test]
    fn statue_creation_copies_the_custom_name() {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        let pos = BlockPos::new(1, 64, 2);
        let name = Tag::String("Copper Nick".to_owned());
        let mut entity = Entity::by_mob_type(MobType::CopperGolem, Vec3::new(1.5, 64.0, 2.5));
        entity.with_mob(|_, mob| mob.custom_name = Some(name.clone()));
        {
            let mut world = World::new(&mut level, &mut ticks);
            apply_statue(&mut world, &mut entity, pos, 2, Direction::North);
        }
        assert_eq!(level.statue_names.get(&pos), Some(&name));
        assert!(entity.removed);
    }
}

#[cfg(test)]
mod transport_tests {
    use mc_bamboo_data::items;

    use super::*;
    use crate::{
        entity::ctx::{self, Command},
        level::Level,
        testing::TestLevel,
        ticks::BlockTicks,
        world::World,
    };

    fn golem(id: i32) -> Entity {
        let mut entity = Entity::by_mob_type(MobType::CopperGolem, Vec3::new(0.5, 64.0, 0.5));
        entity.id = id;
        entity
    }

    /// 两条取物命令冲突时按 uid 顺序落地：先到的拿走，后到的读到空箱子，什么都不做。
    #[test]
    fn conflicting_pickups_are_first_come_first_served() {
        let pos = BlockPos::new(2, 64, 3);
        let mut level = TestLevel::new();
        level.set_container(pos, vec![ItemStack::new(items::STONE, 3)]);
        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);

        let mut first = golem(1);
        let mut second = golem(2);
        // 落地阶段按 uid 升序串行应用，正是 `parallel.rs` 的循环语义。
        ctx::apply(Command::CopperGolemPickUp { pos }, &mut first, &mut world);
        ctx::apply(Command::CopperGolemPickUp { pos }, &mut second, &mut world);

        assert_eq!(
            first
                .kind
                .mob()
                .unwrap()
                .equipment
                .get(EquipmentSlot::MainHand),
            &ItemStack::new(items::STONE, 3)
        );
        assert!(
            second
                .kind
                .mob()
                .unwrap()
                .equipment
                .get(EquipmentSlot::MainHand)
                .is_empty()
        );
        assert!(level.slots_at(pos).iter().all(ItemStack::is_empty));
    }

    /// 一次最多取 16 个（`TRANSPORTED_ITEM_MAX_STACK_SIZE`）。
    #[test]
    fn pickup_is_capped_at_sixteen() {
        let pos = BlockPos::new(2, 64, 3);
        let mut level = TestLevel::new();
        level.set_container(pos, vec![ItemStack::new(items::STONE, 40)]);
        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        let mut golem = golem(1);
        ctx::apply(Command::CopperGolemPickUp { pos }, &mut golem, &mut world);
        assert_eq!(
            golem
                .kind
                .mob()
                .unwrap()
                .equipment
                .get(EquipmentSlot::MainHand),
            &ItemStack::new(items::STONE, 16)
        );
        assert_eq!(level.slots_at(pos)[0], ItemStack::new(items::STONE, 24));
    }
}
