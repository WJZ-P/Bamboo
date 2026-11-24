//! 大脑：官方 `Brain` 一套（记忆、传感器、活动、行为）的 Rust 版。山羊、犰狳这些
//! 新生物不用 `GoalSelector`，`customServerAiStep` 里跑 `getBrain().tick(level, this)`。
//!
//! 一刻的顺序照 `Brain.tick`：先让记忆过期，再按登记顺序跑传感器，再按优先级启动
//! 没在跑的行为，最后推进正在跑的行为。行为的排列顺序复刻官方那张
//! `TreeMap<优先级, HashMap<活动, LinkedHashSet<行为>>>` 的遍历顺序：优先级从小到大，
//! 同一优先级里按 `Activity.hashCode`（名字的 `String.hashCode`）落到 `HashMap` 的桶序，
//! 同桶按加入先后。行为的时长、`SetEntityLookTargetSometimes` 的间隔用世界随机源，
//! 其余（随机漫步、环顾、窥视计时）用生物自己的随机源，和官方一样。
//!
//! `RunOne` 的洗牌用 `ShufflingList` 自己的随机源。官方那个随机源用
//! `RandomSupport.generateUniqueSeed()` 播种，每次开服都不同；Bamboo 用生物出生时随机源的
//! 状态加上闸门序号算种子（不从任何随机源取数），对拍时两边按同一个公式重设。
//!
//! The brain: vanilla `Brain` (memories, sensors, activities, behaviours) in Rust.
//! Goats, armadillos and the other newer mobs run `getBrain().tick(level, this)` from
//! `customServerAiStep` instead of a `GoalSelector`.
//!
//! One tick follows `Brain.tick`: expire memories, run the sensors in registration
//! order, start the stopped behaviours by priority, then tick the running ones. The
//! behaviour order replays the iteration of vanilla's
//! `TreeMap<priority, HashMap<Activity, LinkedHashSet<behaviour>>>`: ascending
//! priority, then the `HashMap` bucket order of `Activity.hashCode` (the name's
//! `String.hashCode`), then insertion order. Behaviour durations and the
//! `SetEntityLookTargetSometimes` interval draw from the level random; strolling,
//! looking around and peeking draw from the mob's own random, as in vanilla.
//!
//! `RunOne` shuffles with its `ShufflingList`'s own random. Vanilla seeds it with
//! `RandomSupport.generateUniqueSeed()`, different on every start; Bamboo derives the
//! seed from the mob's random state at birth plus the gate index, without drawing, and
//! the oracle reseeds both sides with the same formula.

pub mod behavior;
mod behaviors;
pub mod memory;
pub mod nbt;
pub mod sensor;

use self::{
    behavior::{Kind, Node, Status},
    memory::{Memories, MemoryStatus, MemoryType},
    sensor::{Sensor, SensorKind},
};
use super::Mob;
use crate::entity::{Entity, ctx::EntityCtx};

use std::sync::{Arc, OnceLock};

pub use behavior::Cx;
pub(in crate::entity::mob) use behaviors::current_position;

/// 一份大脑模板缓存槽：每种生物一个 `OnceLock`，整张结构表只算一次、用 `Arc` 共享。
///
/// A brain-template cache slot: one `OnceLock` per mob kind, so the whole structure
/// table is computed once and shared through an `Arc`.
pub type TemplateCache = OnceLock<Arc<BrainTemplate>>;

/// 新建一个空的模板缓存槽（`const`，可以给 `static` 用）。
///
/// A fresh empty template cache slot (const, usable in a `static`).
pub const fn template_cache() -> TemplateCache {
    OnceLock::new()
}

/// 官方 `Activity` 里 Bamboo 用到的那几个。
///
/// The vanilla `Activity` values Bamboo uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Activity {
    Core,
    Idle,
    Panic,
    LongJump,
    Ram,
    /// 青蛙的游泳、产卵、卷舌活动（`Activity.SWIM / LAY_SPAWN / TONGUE`）。
    ///
    /// The frog's swim, lay-spawn and tongue activities (`Activity.SWIM / LAY_SPAWN / TONGUE`).
    Swim,
    LaySpawn,
    Tongue,
    /// 美西螈的装死和战斗活动（`Activity.PLAY_DEAD / FIGHT`）。
    ///
    /// The axolotl's play-dead and fight activities (`Activity.PLAY_DEAD / FIGHT`).
    PlayDead,
    Fight,
    /// 村民的日程和袭击活动（`Activity.WORK / PLAY / REST / MEET / RAID / PRE_RAID / HIDE / CELEBRATE`）。
    ///
    /// Villager schedule and raid activities (`Activity.WORK / PLAY / REST / MEET / RAID / PRE_RAID / HIDE / CELEBRATE`).
    Work,
    Play,
    Rest,
    Meet,
    Raid,
    PreRaid,
    Hide,
    Celebrate,
}

impl Activity {
    /// 注册名。
    ///
    /// The registry name.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Core => "core",
            Self::Idle => "idle",
            Self::Panic => "panic",
            Self::LongJump => "long_jump",
            Self::Ram => "ram",
            Self::Swim => "swim",
            Self::LaySpawn => "lay_spawn",
            Self::Tongue => "tongue",
            Self::PlayDead => "play_dead",
            Self::Fight => "fight",
            Self::Work => "work",
            Self::Play => "play",
            Self::Rest => "rest",
            Self::Meet => "meet",
            Self::Raid => "raid",
            Self::PreRaid => "pre_raid",
            Self::Hide => "hide",
            Self::Celebrate => "celebrate",
        }
    }

    /// 按注册名（可带 `minecraft:`）找活动。
    ///
    /// Finds an activity by registry name, with or without `minecraft:`.
    pub fn by_name(name: &str) -> Option<Self> {
        let path = name.strip_prefix("minecraft:").unwrap_or(name);
        match path {
            "core" => Some(Self::Core),
            "idle" => Some(Self::Idle),
            "panic" => Some(Self::Panic),
            "long_jump" => Some(Self::LongJump),
            "ram" => Some(Self::Ram),
            "swim" => Some(Self::Swim),
            "lay_spawn" => Some(Self::LaySpawn),
            "tongue" => Some(Self::Tongue),
            "play_dead" => Some(Self::PlayDead),
            "fight" => Some(Self::Fight),
            "work" => Some(Self::Work),
            "play" => Some(Self::Play),
            "rest" => Some(Self::Rest),
            "meet" => Some(Self::Meet),
            "raid" => Some(Self::Raid),
            "pre_raid" => Some(Self::PreRaid),
            "hide" => Some(Self::Hide),
            "celebrate" => Some(Self::Celebrate),
            _ => None,
        }
    }

    /// `Activity.hashCode()`：名字的 `String.hashCode()`。
    ///
    /// `Activity.hashCode()`: the name's `String.hashCode()`.
    fn java_hash(self) -> i32 {
        self.name().bytes().fold(0_i32, |hash, byte| {
            hash.wrapping_mul(31).wrapping_add(i32::from(byte))
        })
    }

    /// `HashMap.hash` 加桶下标：`(h ^ (h >>> 16)) & (capacity - 1)`。
    ///
    /// `HashMap.hash` plus the bucket index: `(h ^ (h >>> 16)) & (capacity - 1)`.
    fn bucket(self, capacity: usize) -> usize {
        let hash = self.java_hash() as u32;
        ((hash ^ (hash >> 16)) as usize) & (capacity - 1)
    }
}

/// `ActivityData`：一个活动的行为（优先级和行为）、进入条件和退出时要清的记忆。
///
/// `ActivityData`: an activity's behaviours with priorities, its entry conditions and
/// the memories erased when it stops.
#[derive(Debug, Clone)]
pub struct ActivityData {
    pub activity: Activity,
    pub behaviors: Vec<(i32, Kind)>,
    pub conditions: Vec<(MemoryType, MemoryStatus)>,
    pub erase_when_stopped: Vec<MemoryType>,
}

impl ActivityData {
    /// `ActivityData.create(activity, firstPriority, list)`：优先级从第一个起逐个加一。
    ///
    /// `ActivityData.create(activity, firstPriority, list)`: priorities count up from the first.
    pub fn sequential(activity: Activity, first: i32, behaviors: Vec<Kind>) -> Self {
        Self {
            activity,
            behaviors: behaviors
                .into_iter()
                .enumerate()
                .map(|(index, kind)| (first + index as i32, kind))
                .collect(),
            conditions: Vec::new(),
            erase_when_stopped: Vec::new(),
        }
    }

    /// `ActivityData.create(activity, pairs)`：优先级一个个给出。
    ///
    /// `ActivityData.create(activity, pairs)`: explicit priorities.
    pub fn with_priorities(activity: Activity, behaviors: Vec<(i32, Kind)>) -> Self {
        Self {
            activity,
            behaviors,
            conditions: Vec::new(),
            erase_when_stopped: Vec::new(),
        }
    }

    /// 加上进入条件。
    ///
    /// Adds the entry conditions.
    pub fn when(mut self, conditions: Vec<(MemoryType, MemoryStatus)>) -> Self {
        self.conditions = conditions;
        self
    }
}

/// 大脑里排好序的一个行为。
///
/// One behaviour of the brain, in iteration order.
#[derive(Debug, Clone, PartialEq)]
struct Entry {
    priority: i32,
    activity: Activity,
    node: Node,
}

/// 一种生物共用的、不可变的大脑结构，`Arc` 共享、每种生物只构造一次：传感器表、排好序的
/// 行为表（含闸门结构和每个行为预编译好的记忆前置条件掩码）、活动桶、活动进入条件 /
/// 退出清理记忆、以及登记好的记忆位掩码。运行时的记忆、传感器倒计时和每个行为的运行态
/// 留在 [`Brain`] 实例里。
///
/// The immutable brain structure shared by `Arc` across every mob of a kind, built once:
/// the sensor table, the sorted behaviour table (with gate structure and each behaviour's
/// precompiled memory precondition masks), activity buckets, activity entry conditions and
/// exit-erased memories, and the registered-memory mask. Runtime memories, sensor
/// countdowns and each behaviour's running state live in the [`Brain`] instance.
#[derive(Debug, PartialEq)]
pub struct BrainTemplate {
    sensors: Vec<SensorKind>,
    entries: Vec<Entry>,
    requirements: Vec<(Activity, Vec<(MemoryType, MemoryStatus)>)>,
    erase_when_stopped: Vec<(Activity, Vec<MemoryType>)>,
    default_activity: Activity,
    registered: u64,
    /// 活动 → 该活动的行为下标（按优先级升序）。`startEachNonRunning` 把核心桶和当前
    /// 活动桶拼成要尝试启动的下标表，不再全表过滤。
    ///
    /// Activity → its behaviour indices (ascending priority). `startEachNonRunning`
    /// merges the core bucket with the current activity bucket, no full-table filter.
    buckets: Vec<(Activity, Vec<usize>)>,
}

impl BrainTemplate {
    /// 按官方顺序排好行为、登记好所有记忆、建好活动桶；整份结构由调用方缓存复用。
    ///
    /// Orders behaviours like vanilla, registers every memory and builds the activity
    /// buckets; the caller caches the whole structure for reuse.
    pub fn new(sensors: &[SensorKind], activities: Vec<ActivityData>) -> Self {
        let mut memories = Memories::default();
        let sensors: Vec<SensorKind> = sensors.to_vec();
        for &kind in &sensors {
            for &memory in kind.requires() {
                memories.register(memory);
            }
        }
        let mut requirements = Vec::new();
        let mut erase_when_stopped = Vec::new();
        let mut entries = Vec::new();
        for data in activities {
            requirements.push((data.activity, data.conditions));
            if !data.erase_when_stopped.is_empty() {
                erase_when_stopped.push((data.activity, data.erase_when_stopped));
            }
            for (priority, kind) in data.behaviors {
                let node = Node::new(kind);
                node.register_memories(&mut memories);
                entries.push(Entry {
                    priority,
                    activity: data.activity,
                    node,
                });
            }
        }
        let entries = order_entries(entries);
        let registered = memories.registered_mask();
        let mut buckets: Vec<(Activity, Vec<usize>)> = Vec::new();
        for (index, entry) in entries.iter().enumerate() {
            match buckets
                .iter_mut()
                .find(|(owner, _)| *owner == entry.activity)
            {
                Some((_, list)) => list.push(index),
                None => buckets.push((entry.activity, vec![index])),
            }
        }
        Self {
            sensors,
            entries,
            requirements,
            erase_when_stopped,
            default_activity: Activity::Idle,
            registered,
            buckets,
        }
    }

    /// 复制一份原型行为运行态给实例。官方每只生物各有一份行为对象，这里也一样；排序、
    /// 登记、闸门结构、前置条件掩码这些大结构直接从共享模板拷贝，不再逐只重算。
    ///
    /// Clones the prototype behaviour nodes for one instance. Vanilla keeps one behaviour
    /// object per mob; so do we, but the ordering, registration, gate structure and
    /// precondition masks are copied from the shared template instead of rebuilt per mob.
    fn fresh_nodes(&self) -> Vec<Node> {
        self.entries
            .iter()
            .map(|entry| entry.node.clone())
            .collect()
    }
}

/// 官方 `Brain.schedule` 引用的环境属性日程：成年村民和幼年村民各一条。
///
/// The environment-attribute schedule referenced by `Brain.schedule`: adult and baby
/// villager tracks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivitySchedule {
    /// `EnvironmentAttributes.VILLAGER_ACTIVITY`。
    Villager,
    /// `EnvironmentAttributes.BABY_VILLAGER_ACTIVITY`。
    BabyVillager,
}

/// `Brain.lastScheduleUpdate` 的官方初始值（`Brain.java`）。
///
/// Vanilla's initial value for `Brain.lastScheduleUpdate` (`Brain.java`).
const INITIAL_LAST_SCHEDULE_UPDATE: i64 = -9999;

/// `Brain.SCHEDULE_UPDATE_DELAY`：每过 20 刻（严格大于 20）才重新按日程切活动。
///
/// `Brain.SCHEDULE_UPDATE_DELAY`: re-evaluates the schedule only after strictly more
/// than 20 ticks.
const SCHEDULE_UPDATE_DELAY: i64 = 20;

/// 行为在 `Brain.tick` 中途读写活动集合与日程的借用视图。
///
/// Borrowed view allowing behaviours running inside `Brain.tick` to query and switch
/// activities or trigger schedule updates.
pub(super) struct ActivityState<'a> {
    template: &'a BrainTemplate,
    active: &'a mut Vec<Activity>,
    start_bucket: &'a mut Vec<usize>,
    default_activity: &'a mut Activity,
    schedule: &'a mut Option<ActivitySchedule>,
    last_schedule_update: &'a mut i64,
    switched: bool,
}

impl ActivityState<'_> {
    pub(super) fn is_active(&self, activity: Activity) -> bool {
        self.active.contains(&activity)
    }

    pub(super) fn set_default_activity(&mut self, activity: Activity) {
        *self.default_activity = activity;
    }

    fn requirements_met(&self, memories: &Memories, activity: Activity) -> bool {
        self.template
            .requirements
            .iter()
            .find(|(owner, _)| *owner == activity)
            .is_some_and(|(_, conditions)| {
                conditions
                    .iter()
                    .all(|&(memory, status)| memories.check(memory, status))
            })
    }

    fn set_active_activity(&mut self, memories: &mut Memories, activity: Activity) {
        if self.is_active(activity) {
            return;
        }
        for old in self.active.clone() {
            if old == activity {
                continue;
            }
            if let Some((_, to_erase)) = self
                .template
                .erase_when_stopped
                .iter()
                .find(|(owner, _)| *owner == old)
            {
                for &memory in to_erase {
                    memories.erase(memory);
                }
            }
        }
        self.active.clear();
        self.active.push(Activity::Core);
        if activity != Activity::Core {
            self.active.push(activity);
        }
        rebuild_start_bucket(self.template, self.active, self.start_bucket);
        self.switched = true;
    }

    pub(super) fn set_active_activity_if_possible(
        &mut self,
        memories: &mut Memories,
        activity: Activity,
    ) {
        if self.requirements_met(memories, activity) {
            self.set_active_activity(memories, activity);
        } else {
            let default = *self.default_activity;
            self.set_active_activity(memories, default);
        }
    }

    pub(super) fn update_activity_from_schedule(
        &mut self,
        memories: &mut Memories,
        blocks: &dyn crate::entity::ctx::BlockView,
        game_time: i64,
    ) {
        if game_time - *self.last_schedule_update > SCHEDULE_UPDATE_DELAY {
            *self.last_schedule_update = game_time;
            let activity = match *self.schedule {
                Some(ActivitySchedule::Villager) => blocks.villager_activity(),
                Some(ActivitySchedule::BabyVillager) => blocks.baby_villager_activity(),
                None => Activity::Idle,
            };
            if !self.is_active(activity) {
                self.set_active_activity_if_possible(memories, activity);
            }
        }
    }
}

fn rebuild_start_bucket(
    template: &BrainTemplate,
    active: &[Activity],
    start_bucket: &mut Vec<usize>,
) {
    start_bucket.clear();
    for (activity, indices) in &template.buckets {
        if active.contains(activity) {
            start_bucket.extend_from_slice(indices);
        }
    }
    start_bucket.sort_unstable();
}

/// 一只生物的大脑：只存记忆、传感器倒计时、每个行为的运行态、活动集合和两个下标表；
/// 大结构放在 [`BrainTemplate`] 里用 `Arc` 共享。
///
/// One mob's brain: only memories, sensor countdowns, per-behaviour running state, the
/// active set and two index tables; the big structure lives in the `Arc`-shared
/// [`BrainTemplate`].
#[derive(Debug, Clone, PartialEq)]
pub struct Brain {
    template: Arc<BrainTemplate>,
    pub memories: Memories,
    pub sensors: Vec<Sensor>,
    nodes: Vec<Node>,
    /// `activeActivities`：官方是 `HashSet`，这里的顺序只用于对拍输出（排过序）。
    ///
    /// `activeActivities`; a `HashSet` in vanilla, the order here only feeds the sorted
    /// oracle dump.
    active: Vec<Activity>,
    /// 当前活动（核心 + 当前活动）里要尝试启动的行为下标，按官方优先级顺序排好。
    /// 只在切换活动时重建，`startEachNonRunning` 每刻只遍历它，不再全表过滤。
    ///
    /// Indices of behaviours to try starting, from the core and current activities, in
    /// vanilla priority order. Rebuilt only on an activity switch; `startEachNonRunning`
    /// walks just this list instead of filtering the whole table every tick.
    start_bucket: Vec<usize>,
    /// 正在跑的行为下标，按官方优先级顺序排好，跨刻保留。推进阶段直接遍历它，
    /// 不再每刻分配一个 `Vec` 去全表筛选。
    ///
    /// Indices of running behaviours, in priority order, persistent across ticks. The
    /// tick phase walks it directly instead of allocating a fresh `Vec` each tick.
    running: Vec<usize>,
    /// `Brain.defaultActivity`：默认 `Activity::Idle`，袭击时可被 `SetRaidStatus` 改写。
    ///
    /// `Brain.defaultActivity`: defaults to `Activity::Idle`; `SetRaidStatus` can override it.
    default_activity: Activity,
    /// `Brain.schedule`：生物绑定的日程（村民 / 幼年村民），其余生物为 `None`。
    ///
    /// `Brain.schedule`: the mob's activity schedule (villager / baby villager), or `None`.
    schedule: Option<ActivitySchedule>,
    /// `Brain.lastScheduleUpdate`：初始 `-9999`，按 20 刻节流。
    ///
    /// `Brain.lastScheduleUpdate`: starts at `-9999`, throttled to every > 20 ticks.
    last_schedule_update: i64,
}

impl Brain {
    /// `Brain.Provider.makeBrain` → `new Brain(...)`：登记传感器和行为要的记忆，排好行为，
    /// 核心活动是 `CORE`，然后切到默认活动 `IDLE`。传感器的随机起始延迟由调用方用
    /// [`Self::randomly_delay_start`] 另外取，因为官方在构造函数里用生物的随机源取。
    ///
    /// `Brain.Provider.makeBrain` → `new Brain(...)`: registers the memories the sensors
    /// and behaviours need, orders the behaviours, makes `CORE` the core activity and
    /// switches to the default `IDLE`. The callers draw the sensors' random start delays
    /// with [`Self::randomly_delay_start`], since vanilla draws them from the mob's random
    /// inside the constructor.
    pub fn new(sensors: &[SensorKind], activities: Vec<ActivityData>) -> Self {
        Self::from_template(Arc::new(BrainTemplate::new(sensors, activities)))
    }

    /// 每种生物一个缓存槽：整张模板只构造一次，之后每只生物只拷运行态。
    ///
    /// One cache slot per mob kind: the template is built once, each later mob only
    /// copies its running state.
    pub fn cached(slot: &'static TemplateCache, build: impl FnOnce() -> BrainTemplate) -> Self {
        let template = slot.get_or_init(|| Arc::new(build()));
        Self::from_template(Arc::clone(template))
    }

    fn from_template(template: Arc<BrainTemplate>) -> Self {
        let mut memories = Memories::default();
        memories.register_mask(template.registered);
        let sensors = template.sensors.iter().copied().map(Sensor::new).collect();
        let default_activity = template.default_activity;
        let mut brain = Self {
            nodes: template.fresh_nodes(),
            template,
            memories,
            sensors,
            active: Vec::new(),
            start_bucket: Vec::new(),
            running: Vec::new(),
            default_activity,
            schedule: None,
            last_schedule_update: INITIAL_LAST_SCHEDULE_UPDATE,
        };
        brain.set_active_activity(default_activity);
        brain
    }

    /// `Brain.setSchedule`。
    ///
    /// `Brain.setSchedule`.
    pub fn set_schedule(&mut self, schedule: Option<ActivitySchedule>) {
        self.schedule = schedule;
    }

    /// 当前绑定的日程。
    ///
    /// The currently configured activity schedule.
    pub fn schedule(&self) -> Option<ActivitySchedule> {
        self.schedule
    }

    /// `Brain.setDefaultActivity`。
    ///
    /// `Brain.setDefaultActivity`.
    pub fn set_default_activity(&mut self, activity: Activity) {
        self.default_activity = activity;
    }

    /// `Brain.useDefaultActivity`。
    ///
    /// `Brain.useDefaultActivity`.
    pub fn use_default_activity(&mut self) {
        self.set_active_activity(self.default_activity);
    }

    /// `Brain.setActiveActivityIfPossible`：条件满足就切过去，否则退回默认活动。
    ///
    /// `Brain.setActiveActivityIfPossible`: switches if requirements hold, else uses
    /// the default activity.
    pub fn set_active_activity_if_possible(&mut self, activity: Activity) {
        if self.requirements_met(activity) {
            self.set_active_activity(activity);
        } else {
            self.use_default_activity();
        }
    }

    /// `Brain.updateActivityFromSchedule(attrs, gameTime, pos)`：每过 20 刻（`game_time -
    /// last_schedule_update > 20`）才读日程并按需切换活动。
    ///
    /// `Brain.updateActivityFromSchedule(attrs, gameTime, pos)`: checks the schedule only
    /// when `game_time - last_schedule_update > 20` and switches if not already active.
    pub fn update_activity_from_schedule(
        &mut self,
        blocks: &dyn crate::entity::ctx::BlockView,
        game_time: i64,
    ) {
        if game_time - self.last_schedule_update > SCHEDULE_UPDATE_DELAY {
            self.last_schedule_update = game_time;
            let activity = match self.schedule {
                Some(ActivitySchedule::Villager) => blocks.villager_activity(),
                Some(ActivitySchedule::BabyVillager) => blocks.baby_villager_activity(),
                None => Activity::Idle,
            };
            if !self.is_active(activity) {
                self.set_active_activity_if_possible(activity);
            }
        }
    }

    /// 从旧大脑打包可保留的记忆并放进当前新大脑，对应官方 `Brain.pack()` → `new Brain(..., packed)`。
    /// 照官方 `Brain$1` 只拷 `canSerialize() == true` 的记忆，并按新大脑已登记的槽位过滤。
    ///
    /// Copies packed memories from an old brain into this fresh brain, matching vanilla's
    /// `Brain.pack()` → `new Brain(..., packed)`. Per `Brain$1`, only memories with
    /// `canSerialize() == true` are copied, filtered by the new brain's registered slots.
    pub fn copy_packed_memories_from(&mut self, old: &Brain) {
        for (kind, value, ttl) in old.memories.filled() {
            if !kind.serializable() {
                continue;
            }
            match ttl {
                Some(ttl) => self.memories.set_with_ttl(kind, value.clone(), ttl),
                None => self.memories.set(kind, value.clone()),
            }
        }
    }

    /// `Sensor.randomlyDelayStart`：每个传感器按登记顺序取一次 `nextInt(scanRate)`。
    ///
    /// `Sensor.randomlyDelayStart`: one `nextInt(scanRate)` per sensor in registration order.
    pub fn randomly_delay_start(&mut self, random: &mut crate::entity::JavaRandom) {
        for sensor in &mut self.sensors {
            sensor.time_to_tick = i64::from(random.next_int_bounded(sensor.kind.scan_rate()));
        }
    }

    /// 给每个 `RunOne` / `GateBehavior` 的洗牌随机源播种：第 k 个闸门（按行为遍历顺序、
    /// 深度优先）用 [`shuffle_seed`]`(base, k)`。
    ///
    /// Seeds the shuffle random of every `RunOne` / `GateBehavior`: gate k (in behaviour
    /// iteration order, depth first) uses [`shuffle_seed`]`(base, k)`.
    pub fn seed_shuffles(&mut self, base: i64) {
        let mut index = 0;
        for node in &mut self.nodes {
            node.seed_gates(base, &mut index);
        }
    }

    /// `Brain.isActive`。
    ///
    /// `Brain.isActive`.
    pub fn is_active(&self, activity: Activity) -> bool {
        self.active.contains(&activity)
    }

    /// `Brain.setActiveActivity`：没在跑时先清掉别的活动退出时要清的记忆，
    /// 再换成核心活动加这一个。
    ///
    /// `Brain.setActiveActivity`: if not active yet, erase the other activities'
    /// exit memories, then become the core activity plus this one.
    fn set_active_activity(&mut self, activity: Activity) {
        if self.is_active(activity) {
            return;
        }
        for old in self.active.clone() {
            if old == activity {
                continue;
            }
            if let Some((_, memories)) = self
                .template
                .erase_when_stopped
                .iter()
                .find(|(owner, _)| *owner == old)
            {
                for &memory in memories {
                    self.memories.erase(memory);
                }
            }
        }
        self.active.clear();
        self.active.push(Activity::Core);
        if activity != Activity::Core {
            self.active.push(activity);
        }
        self.rebuild_start_bucket();
    }

    /// 切换活动后重建 `start_bucket`：核心活动桶和当前活动桶拼起来、按优先级（下标）升序。
    /// 只在切换活动时调用，不在每刻的 `tick` 里。
    ///
    /// Rebuild `start_bucket` after an activity switch: the core bucket merged with the
    /// current activity bucket, ascending by priority (index). Called only on a switch,
    /// never per tick.
    fn rebuild_start_bucket(&mut self) {
        rebuild_start_bucket(&self.template, &self.active, &mut self.start_bucket);
    }

    /// `Brain.activityRequirementsAreMet`：没有登记过的活动不满足。
    ///
    /// `Brain.activityRequirementsAreMet`: unknown activities never qualify.
    fn requirements_met(&self, activity: Activity) -> bool {
        self.template
            .requirements
            .iter()
            .find(|(owner, _)| *owner == activity)
            .is_some_and(|(_, conditions)| {
                conditions
                    .iter()
                    .all(|&(memory, status)| self.memories.check(memory, status))
            })
    }

    /// `Brain.setActiveActivityToFirstValid`。
    ///
    /// `Brain.setActiveActivityToFirstValid`.
    pub fn set_active_activity_to_first_valid(&mut self, activities: &[Activity]) {
        if let Some(&activity) = activities
            .iter()
            .find(|&&activity| self.requirements_met(activity))
        {
            self.set_active_activity(activity);
        }
    }

    /// `Brain.tick`：记忆过期、传感器、启动、推进（精确单刻）。
    ///
    /// `Brain.tick`: expire memories, sensors, start, then tick (exact single tick).
    pub fn tick(&mut self, me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
        self.tick_by(me, mob, ctx, 1);
    }

    /// 降频档用的跳刻版 `Brain.tick`：记忆过期与传感器倒计时按 `gap`（至少 1）推进，
    /// 避免 20 刻传感器在降频 4 刻下拖慢成 80 刻。行为的结束时刻用的是绝对 `game_time`
    /// （`ctx.blocks().game_time()`），不受 `gap` 影响。精确档传 `gap = 1` 与
    /// [`Self::tick`] 逐位一致。
    ///
    /// Throttled `Brain.tick`: advances memory expiry and sensor countdowns by `gap`
    /// (clamped to >= 1) so 20-tick sensors do not slow down 4× under 4-tick throttling.
    /// Behaviour end timestamps use absolute `game_time` and are unaffected. Passing
    /// `gap = 1` (precise mode) is bit-identical to [`Self::tick`].
    pub fn tick_by(&mut self, me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx, gap: i64) {
        crate::entity::mob::timing::record_brain_tick!();
        let step = gap.max(1);
        self.memories.tick_by(step);
        let time = ctx.blocks().game_time();
        let Self {
            template,
            memories,
            sensors,
            nodes,
            active,
            start_bucket,
            running,
            default_activity,
            schedule,
            last_schedule_update,
        } = self;
        let mut cx = Cx {
            me,
            mob,
            ctx,
            memories,
            activity: ActivityState {
                template,
                active,
                start_bucket,
                default_activity,
                schedule,
                last_schedule_update,
                switched: false,
            },
            time,
        };
        {
            crate::entity::mob::timing::span!(BrainSensors);
            for sensor in sensors.iter_mut() {
                sensor.tick_by(&mut cx, step);
            }
        }
        // `startEachNonRunning`：只遍历核心 + 当前活动桶，按优先级顺序。新启动的行为
        // 按序插进持久的 running 表，保持官方遍历顺序；若某个行为在启动时切换了活动
        // （如 `VillagerPanicTrigger` / `VillagerCalmDown`），立刻按新桶定位到下一个
        // 排在当前下标之后的行为，与官方 `TreeMap`/`HashMap` 遍历一致。
        // `startEachNonRunning`: walk only the core + current activity buckets in priority
        // order. Newly started behaviours are inserted into the persistent running list in
        // order; if a behaviour switches activities during `tryStart` (e.g.
        // `VillagerPanicTrigger` / `VillagerCalmDown`), jump into the rebuilt bucket right
        // after `index`, matching vanilla's `TreeMap`/`HashMap` loop.
        {
            crate::entity::mob::timing::span!(BrainBehaviors);
            let mut pos = 0;
            while pos < cx.activity.start_bucket.len() {
                let index = cx.activity.start_bucket[pos];
                cx.activity.switched = false;
                let node = &mut nodes[index];
                if node.status == Status::Stopped && node.try_start(&mut cx) {
                    crate::entity::mob::timing::record_behavior_start!();
                    let at = running.partition_point(|&other| other < index);
                    running.insert(at, index);
                }
                if cx.activity.switched {
                    pos = cx
                        .activity
                        .start_bucket
                        .partition_point(|&other| other <= index);
                } else {
                    pos += 1;
                }
            }
            // `tickEachRunning`：直接遍历持久下标表，不再每刻分配 Vec 或全表筛选；停掉的移除。
            // `tickEachRunning`: walk the persistent index list directly — no per-tick Vec
            // allocation or full-table filter; stopped behaviours are removed.
            let mut at = 0;
            while at < running.len() {
                let index = running[at];
                nodes[index].tick_or_stop(&mut cx);
                if nodes[index].status == Status::Stopped {
                    running.remove(at);
                } else {
                    at += 1;
                }
            }
        }
    }

    /// `Brain.stopAll`：按下标顺序停掉所有正在跑的行为，并清空 running 表。
    ///
    /// `Brain.stopAll`: stop every running behaviour in index order and clear the list.
    pub fn stop_all(&mut self, me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
        let time = ctx.blocks().game_time();
        let Self {
            template,
            memories,
            nodes,
            active,
            start_bucket,
            default_activity,
            schedule,
            last_schedule_update,
            ..
        } = self;
        let mut cx = Cx {
            me,
            mob,
            ctx,
            memories,
            activity: ActivityState {
                template,
                active,
                start_bucket,
                default_activity,
                schedule,
                last_schedule_update,
                switched: false,
            },
            time,
        };
        for node in nodes.iter_mut() {
            if node.status == Status::Running {
                node.do_stop(&mut cx);
            }
        }
        self.running.clear();
    }

    /// 行为表里出现过的活动（去重前，按遍历顺序）。测试 / 对拍用。
    ///
    /// The activities present in the behaviour table (before dedup, in iteration
    /// order), for tests / oracles.
    pub fn table_activities(&self) -> Vec<Activity> {
        self.template
            .entries
            .iter()
            .map(|entry| entry.activity)
            .collect()
    }

    /// 对拍用：排过序的活动名。
    ///
    /// For oracles: the sorted activity names.
    pub fn active_names(&self) -> Vec<&'static str> {
        let mut names: Vec<_> = self.active.iter().map(|activity| activity.name()).collect();
        names.sort_unstable();
        names
    }

    /// 对拍用：`getRunningBehaviors` 的类名；闸门带上里面正在跑的子行为。
    ///
    /// For oracles: the class names of `getRunningBehaviors`; gates list their running
    /// children.
    pub fn running_names(&self) -> Vec<String> {
        self.running
            .iter()
            .map(|&index| self.nodes[index].debug_name())
            .collect()
    }

    /// 对拍用：传感器表（类名、扫描间隔、`requires()` 的记忆名）。
    ///
    /// For oracles: the sensor table (class name, scan rate, the `requires()` memories).
    pub fn sensor_table(&self) -> Vec<(&'static str, i32, Vec<&'static str>)> {
        self.sensors
            .iter()
            .map(|sensor| {
                let mut memories: Vec<&'static str> = sensor
                    .kind
                    .requires()
                    .iter()
                    .map(|kind| kind.name())
                    .collect();
                memories.sort_unstable();
                (sensor.kind.class_name(), sensor.kind.scan_rate(), memories)
            })
            .collect()
    }

    /// 对拍用：行为表（优先级、活动名、类名），按官方遍历顺序；闸门带装载顺序的子行为名。
    ///
    /// For oracles: the behaviour table (priority, activity name, class name) in the
    /// vanilla iteration order; gates list their children's names in load order.
    pub fn behavior_table(&self) -> Vec<(i32, &'static str, String)> {
        self.template
            .entries
            .iter()
            .map(|entry| {
                (
                    entry.priority,
                    entry.activity.name(),
                    entry.node.table_name(),
                )
            })
            .collect()
    }

    /// 对拍用：活动的进入条件（活动名、记忆名与状态名）；没有条件的活动不出现。
    ///
    /// For oracles: the activity entry conditions (activity name, memory name and
    /// status); activities without conditions are omitted.
    pub fn requirement_table(&self) -> Vec<(&'static str, Vec<(&'static str, &'static str)>)> {
        self.template
            .requirements
            .iter()
            .filter(|(_, conditions)| !conditions.is_empty())
            .map(|(activity, conditions)| {
                let mut rows: Vec<(&'static str, &'static str)> = conditions
                    .iter()
                    .map(|&(memory, status)| (memory.name(), status.name()))
                    .collect();
                rows.sort_unstable();
                (activity.name(), rows)
            })
            .collect()
    }

    /// 对拍用：活动退出时清的记忆（活动名、记忆名）。
    ///
    /// For oracles: the memories erased when an activity stops (activity, memory name).
    pub fn erase_table(&self) -> Vec<(&'static str, Vec<&'static str>)> {
        self.template
            .erase_when_stopped
            .iter()
            .map(|(activity, memories)| {
                let mut names: Vec<&'static str> =
                    memories.iter().map(|kind| kind.name()).collect();
                names.sort_unstable();
                (activity.name(), names)
            })
            .collect()
    }
}

/// 第 k 个闸门的洗牌种子：`base` 和 `k` 用 SplitMix64 的终混合拌一下。
///
/// The shuffle seed of gate k: `base` and `k` mixed with SplitMix64's finaliser.
pub fn shuffle_seed(base: i64, index: i64) -> i64 {
    let mut z = (base as u64).wrapping_add((index as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    (z ^ (z >> 31)) as i64
}

/// 按官方 `TreeMap` / `HashMap` / `LinkedHashSet` 的遍历顺序排好行为。
///
/// Orders the behaviours like vanilla's `TreeMap` / `HashMap` / `LinkedHashSet` iteration.
fn order_entries(entries: Vec<Entry>) -> Vec<Entry> {
    let mut priorities: Vec<i32> = entries.iter().map(|entry| entry.priority).collect();
    priorities.sort_unstable();
    priorities.dedup();
    let mut ordered = Vec::with_capacity(entries.len());
    let mut remaining: Vec<Option<Entry>> = entries.into_iter().map(Some).collect();
    for priority in priorities {
        // 这一优先级的 `HashMap` 里活动按第一次出现的先后插入。
        // Activities enter this priority's `HashMap` in first-seen order.
        let mut activities: Vec<Activity> = Vec::new();
        for entry in remaining.iter().flatten() {
            if entry.priority == priority && !activities.contains(&entry.activity) {
                activities.push(entry.activity);
            }
        }
        let capacity = hash_map_capacity(activities.len());
        let mut keyed: Vec<(usize, usize, Activity)> = activities
            .iter()
            .enumerate()
            .map(|(order, &activity)| (activity.bucket(capacity), order, activity))
            .collect();
        // OpenJDK `HashMap.computeIfAbsent` 碰撞时插在桶链表头部（`tab[i] = newNode(..., first)`），
        // 所以同一桶内后插入的活动排在前面。
        // OpenJDK `HashMap.computeIfAbsent` prepends collisions to the bucket head
        // (`tab[i] = newNode(..., first)`), so later-inserted activities iterate first.
        keyed.sort_unstable_by_key(|&(bucket, order, _)| (bucket, std::cmp::Reverse(order)));
        for (_, _, activity) in keyed {
            for slot in &mut remaining {
                if slot
                    .as_ref()
                    .is_some_and(|entry| entry.priority == priority && entry.activity == activity)
                {
                    ordered.extend(slot.take());
                }
            }
        }
    }
    ordered
}

/// `HashMap` 默认容量 16、装载因子 0.75：放 n 个键之后的桶数。
///
/// A default `HashMap` (16 buckets, load factor 0.75): the bucket count after n keys.
fn hash_map_capacity(keys: usize) -> usize {
    let mut capacity = 16;
    while keys * 4 > capacity * 3 {
        capacity *= 2;
    }
    capacity
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 钉住关键结构的大小。优化前 `Brain=6528`、`Memories=6400`（32 个槽每个都内联
    /// 184 字节的 `MemoryValue`）；把值装箱、把整张结构表挪进 `Arc<BrainTemplate>` 后
    /// 32 槽时 `Brain=664`、`Memories=536`，期 3 扩到 49 槽（+17×16 = +272）加日程字段后
    /// `Memories=808`、`Brain=952`。这些数字变了就说明有人又把大对象塞回了实例。
    ///
    /// Pins the key struct sizes. After boxing values and moving the structure table into
    /// `Arc<BrainTemplate>`: 32 slots had `Brain=664`, `Memories=536`; phase 3 grows to 49
    /// slots (+272) plus schedule fields (`Memories=808`, `Brain=952`).
    #[test]
    fn brain_and_memory_sizes_stay_small() {
        use crate::entity::mob::brain::behavior::Node;
        use crate::entity::mob::brain::memory::{Memories, MemoryValue};
        assert_eq!(std::mem::size_of::<MemoryValue>(), 184);
        assert_eq!(std::mem::size_of::<Memories>(), 808);
        assert_eq!(std::mem::size_of::<Brain>(), 952);
        assert_eq!(std::mem::size_of::<Node>(), 136);
        assert_eq!(std::mem::size_of::<Sensor>(), 16);
        assert_eq!(std::mem::size_of::<Entry>(), 144);
    }

    /// 同一缓存槽构造两只大脑，它们共享同一份模板（`Arc::ptr_eq`），但记忆和运行态
    /// 各自独立。这就是「模板按种类缓存、实例只留小状态」。
    ///
    /// Two brains from one cache slot share the same template (`Arc::ptr_eq`) but own
    /// separate memories and running state: "template cached per kind, instance holds
    /// only small state".
    #[test]
    fn cached_brains_share_the_template_but_not_state() {
        static TEMPLATE: TemplateCache = template_cache();
        let build = || {
            BrainTemplate::new(
                &[],
                vec![ActivityData::sequential(
                    Activity::Core,
                    0,
                    vec![Kind::DoNothing { min: 30, max: 60 }],
                )],
            )
        };
        let mut first = Brain::cached(&TEMPLATE, build);
        let second = Brain::cached(&TEMPLATE, build);
        assert!(Arc::ptr_eq(&first.template, &second.template));

        first.memories.register(MemoryType::WalkTarget);
        first
            .memories
            .set(MemoryType::WalkTarget, memory::MemoryValue::Unit);
        assert!(!second.memories.has(MemoryType::WalkTarget));

        // 结构表来自共享模板，和官方遍历顺序一致；运行态表初始为空。
        // Structure tables come from the shared template in vanilla order; running is empty.
        assert_eq!(first.behavior_table().len(), 1);
        assert!(first.running_names().is_empty());
        assert_eq!(first.table_activities(), vec![Activity::Core]);
    }

    /// 官方 `"core".hashCode()` 等几个值，和桶序：16 个桶时 idle、core、panic 的顺序。
    /// 新增的青蛙活动也按官方注册名逐个核过 `String.hashCode()`。
    ///
    /// Vanilla `String.hashCode` values and the bucket order with 16 buckets. The new
    /// frog activities are checked against the vanilla registry names too.
    #[test]
    fn activity_hashes_match_java() {
        assert_eq!(Activity::Core.java_hash(), 3_059_615);
        assert_eq!(Activity::Idle.java_hash(), 3_227_604);
        assert_eq!(Activity::Panic.java_hash(), 106_433_143);
        assert_eq!(Activity::LongJump.java_hash(), 127_833_105);
        assert_eq!(Activity::Ram.java_hash(), 112_670);
        assert_eq!(Activity::Swim.java_hash(), 3_543_688);
        assert_eq!(Activity::LaySpawn.java_hash(), -1_834_472_160);
        assert_eq!(Activity::Tongue.java_hash(), -868_095_228);
        assert_eq!(Activity::Work.java_hash(), 3_655_441);
        assert_eq!(Activity::Play.java_hash(), 3_443_508);
        assert_eq!(Activity::Rest.java_hash(), 3_496_916);
        assert_eq!(Activity::Meet.java_hash(), 3_347_527);
        assert_eq!(Activity::Raid.java_hash(), 3_492_746);
        assert_eq!(Activity::PreRaid.java_hash(), -1_294_762_074);
        assert_eq!(Activity::Hide.java_hash(), 3_202_370);
        assert_eq!(Activity::Celebrate.java_hash(), 1_432_511_431);
        assert_eq!(Activity::Swim.name(), "swim");
        assert_eq!(Activity::LaySpawn.name(), "lay_spawn");
        assert_eq!(Activity::Tongue.name(), "tongue");
        for activity in [
            Activity::Work,
            Activity::Play,
            Activity::Rest,
            Activity::Meet,
            Activity::Raid,
            Activity::PreRaid,
            Activity::Hide,
            Activity::Celebrate,
        ] {
            assert_eq!(Activity::by_name(activity.name()), Some(activity));
            assert_eq!(
                Activity::by_name(&format!("minecraft:{}", activity.name())),
                Some(activity)
            );
        }
        assert_eq!(hash_map_capacity(12), 16);
        assert_eq!(hash_map_capacity(13), 32);
    }

    /// `Brain::update_activity_from_schedule` 遵循 20 刻节流（`time - last > 20`，严格大于），
    /// 且目标活动不满足进入条件时退回默认活动 `IDLE`。
    #[test]
    fn schedule_update_throttles_every_20_ticks_and_falls_back_to_idle() {
        use crate::{BlockTicks, World, testing::TestLevel};

        let mut brain = Brain::new(
            &[],
            vec![
                ActivityData::sequential(
                    Activity::Core,
                    0,
                    vec![Kind::DoNothing { min: 10, max: 10 }],
                ),
                ActivityData::sequential(
                    Activity::Idle,
                    0,
                    vec![Kind::DoNothing { min: 10, max: 10 }],
                ),
                ActivityData::sequential(
                    Activity::Work,
                    0,
                    vec![Kind::DoNothing { min: 10, max: 10 }],
                )
                .when(vec![(MemoryType::JobSite, MemoryStatus::ValuePresent)]),
                ActivityData::sequential(
                    Activity::Rest,
                    0,
                    vec![Kind::DoNothing { min: 10, max: 10 }],
                ),
            ],
        );
        brain.memories.register(MemoryType::JobSite);
        brain.set_schedule(Some(ActivitySchedule::Villager));

        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();

        // 0 刻：lastScheduleUpdate = -9999，0 - (-9999) > 20 → 更新为 Rest。
        level.clock_ticks = 0;
        {
            let world = World::new(&mut level, &mut ticks);
            brain.update_activity_from_schedule(&world, 0);
        }
        assert_eq!(brain.active_names(), vec!["core", "rest"]);

        // 20 刻（差值 == 20，不满足严格大于）：仍在节流内，不切到 Idle。
        level.clock_ticks = 20;
        {
            let world = World::new(&mut level, &mut ticks);
            brain.update_activity_from_schedule(&world, 20);
        }
        assert_eq!(brain.active_names(), vec!["core", "rest"]);

        // 21 刻（差值 == 21 > 20）：切到 Idle。
        level.clock_ticks = 21;
        {
            let world = World::new(&mut level, &mut ticks);
            brain.update_activity_from_schedule(&world, 21);
        }
        assert_eq!(brain.active_names(), vec!["core", "idle"]);

        // 2000 刻（Work 窗口），但没有 JOB_SITE → 退回默认活动 Idle。
        level.clock_ticks = 2000;
        {
            let world = World::new(&mut level, &mut ticks);
            brain.update_activity_from_schedule(&world, 100);
        }
        assert_eq!(brain.active_names(), vec!["core", "idle"]);

        // 写上 JOB_SITE 后过 21 刻：成功切到 Work。
        brain.memories.set(
            MemoryType::JobSite,
            memory::MemoryValue::GlobalPos(mc_bamboo_protocol::components::value::GlobalPos {
                dimension: "minecraft:overworld".to_owned(),
                pos: mc_bamboo_core::BlockPos::new(1, 64, 1),
            }),
        );
        {
            let world = World::new(&mut level, &mut ticks);
            brain.update_activity_from_schedule(&world, 121);
        }
        assert_eq!(brain.active_names(), vec!["core", "work"]);
    }

    /// 洗牌种子不取数、可复现，不同闸门不同。
    ///
    /// Shuffle seeds are reproducible and differ per gate.
    #[test]
    fn shuffle_seeds_differ_per_gate() {
        assert_eq!(shuffle_seed(7, 0), shuffle_seed(7, 0));
        assert_ne!(shuffle_seed(7, 0), shuffle_seed(7, 1));
    }

    /// 青蛙用到的新活动能进 `order_entries` 的桶序、按优先级排好，初始活动仍是
    /// core + idle。六个活动时青蛙也会走同一套排序机制。
    ///
    /// The new frog activities enter `order_entries`' bucket order, sort by priority and
    /// leave the initial activity at core + idle. The frog's six activities reuse the
    /// same ordering mechanism.
    #[test]
    fn frog_activities_sort_into_the_brain_table() {
        let nothing = Kind::DoNothing { min: 30, max: 60 };
        let brain = Brain::new(
            &[],
            vec![
                ActivityData::with_priorities(Activity::Swim, vec![(1, nothing.clone())]),
                ActivityData::sequential(Activity::Core, 0, vec![nothing.clone()]),
                ActivityData::sequential(Activity::Idle, 0, vec![nothing]),
            ],
        );
        assert_eq!(brain.active_names(), vec!["core", "idle"]);
        let activities: Vec<Activity> = brain.table_activities();
        assert_eq!(activities.len(), 3);
        // 优先级 0 的 core/idle 排在优先级 1 的 swim 之前。
        // Priority-0 core/idle precede priority-1 swim.
        assert_eq!(activities.last(), Some(&Activity::Swim));
    }

    /// 官方 `Brain`（`Brain.java:78-93, 350-366`）只从 `memoryTypes`、`Sensor.requires()`
    /// 和行为的 `getRequiredMemories()` 登记记忆，活动 `.when()` 条件本身不登记；
    /// `checkMemory`（`Brain.java:242-249`）对未登记种类连 `VALUE_ABSENT` 都返回 false。
    /// 所以青蛙活动条件引用的 `IS_IN_WATER / IS_PREGNANT / LONG_JUMP_MID_JUMP` 等记忆
    /// 必须由某个传感器 / 行为登记，忠实照搬它们的 `requires()` / `required()` 即可，
    /// `Brain::new` 无需为活动条件改动（旧汇报疑点 5）。这条测试把语义锁住。
    ///
    /// Vanilla `Brain` registers memories only from `memoryTypes`, `Sensor.requires()`
    /// and behaviours' `getRequiredMemories()`; activity `.when()` conditions register
    /// nothing, and `checkMemory` returns false for unregistered types even for
    /// `VALUE_ABSENT`. Frog activity-condition memories must therefore be required by
    /// some sensor/behaviour; faithful `requires()`/`required()` porting suffices and
    /// `Brain::new` needs no change. This test locks that semantics (old report doubt 5).
    #[test]
    fn frog_activity_conditions_need_registered_memories_like_vanilla() {
        let mut memories = Memories::default();
        // 未登记的记忆：与官方 `checkMemory` 一致，连 VALUE_ABSENT 都不满足。
        // An unregistered memory: even VALUE_ABSENT fails, as in vanilla checkMemory.
        assert!(!memories.check(MemoryType::LongJumpMidJump, MemoryStatus::ValueAbsent));
        assert!(!memories.check(MemoryType::LongJumpMidJump, MemoryStatus::ValuePresent));
        memories.register(MemoryType::LongJumpMidJump);
        assert!(memories.check(MemoryType::LongJumpMidJump, MemoryStatus::ValueAbsent));
        assert!(memories.check(MemoryType::LongJumpMidJump, MemoryStatus::Registered));

        // `LookTarget` 由 `LookAtTargetSink` 的 `required()` 登记，`IsInWater` 没人登记。
        // `LookTarget` is registered by `LookAtTargetSink::required()`; `IsInWater` is not.
        let brain = Brain::new(
            &[],
            vec![
                ActivityData::sequential(
                    Activity::Core,
                    0,
                    vec![Kind::LookAtTargetSink { min: 45, max: 90 }],
                ),
                ActivityData::sequential(
                    Activity::Idle,
                    0,
                    vec![Kind::DoNothing { min: 30, max: 60 }],
                )
                .when(vec![(MemoryType::LookTarget, MemoryStatus::ValueAbsent)]),
                ActivityData::sequential(
                    Activity::Swim,
                    0,
                    vec![Kind::DoNothing { min: 30, max: 60 }],
                )
                .when(vec![(MemoryType::IsInWater, MemoryStatus::ValuePresent)]),
            ],
        );
        assert!(brain.memories.is_registered(MemoryType::LookTarget));
        assert!(!brain.memories.is_registered(MemoryType::IsInWater));
        // 登记过的空槽满足 VALUE_ABSENT；未登记的条件记忆让活动永远选不中。
        // A registered empty slot satisfies VALUE_ABSENT; an unregistered one never does.
        assert!(brain.requirements_met(Activity::Idle));
        assert!(!brain.requirements_met(Activity::Swim));
    }

    /// `Memories::tick_by(gap)` 与连续 `gap` 次 `Memories::tick()` 逐槽逐位一致
    /// （含 `ttl = 0`、`ttl = gap - 1`、`ttl = gap`、`ttl > gap` 与不过期槽）。
    #[test]
    fn memory_tick_by_matches_repeated_single_ticks() {
        for gap in [1_i64, 2, 3, 4, 20] {
            for ttl in [0_i64, 1, 2, 3, 4, 5, 19, 20, 21] {
                let mut stepped = Memories::default();
                let mut looped = Memories::default();
                for m in [&mut stepped, &mut looped] {
                    m.register(MemoryType::IsPanicking);
                    m.register(MemoryType::WalkTarget);
                    m.set_with_ttl(
                        MemoryType::IsPanicking,
                        memory::MemoryValue::Bool(true),
                        ttl,
                    );
                    m.set(MemoryType::WalkTarget, memory::MemoryValue::Unit);
                }
                stepped.tick_by(gap);
                for _ in 0..gap {
                    looped.tick();
                }
                assert_eq!(stepped, looped, "gap={gap} ttl={ttl}");
            }
        }
    }

    /// `Brain::tick_by(gap)` 按 `gap` 推进记忆过期和传感器倒计时，行为的结束时刻仍读
    /// 绝对 `game_time` 不受 `gap` 影响；精确档 `gap = 1` 与 `Brain::tick()` 一致。
    #[test]
    fn brain_tick_by_advances_memories_and_sensors_while_behaviors_use_game_time() {
        use crate::{
            BlockTicks, World, entity::ctx::PreciseCtx, entity::mob::MobType, testing::TestLevel,
        };
        use mc_bamboo_core::Vec3;

        let mut brain = Brain::new(
            &[SensorKind::IsInWater],
            vec![ActivityData::sequential(
                Activity::Core,
                0,
                vec![Kind::DoNothing { min: 10, max: 10 }],
            )],
        );
        brain.memories.register(MemoryType::DangerDetectedRecently);
        brain.memories.set_with_ttl(
            MemoryType::DangerDetectedRecently,
            memory::MemoryValue::Bool(true),
            10,
        );
        brain.sensors[0].time_to_tick = 10;

        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        let mut me = Entity::by_mob_type(MobType::Frog, Vec3::new(0.5, 64.0, 0.5));
        let mut mob = Mob::new(MobType::Frog);

        // t = 100, gap = 1：启动 DoNothing (end = 110)，记忆 ttl 10→9，传感器 10→9。
        ticks.set_time(100);
        {
            let mut world = World::new(&mut level, &mut ticks);
            let mut ctx = PreciseCtx::new(&mut world, me.id);
            brain.tick_by(&mut me, &mut mob, &mut ctx, 1);
        }
        assert_eq!(
            brain
                .memories
                .time_until_expiry(MemoryType::DangerDetectedRecently),
            9
        );
        assert_eq!(brain.sensors[0].time_to_tick, 9);
        assert_eq!(brain.running_names(), vec!["DoNothing"]);

        // t = 104, gap = 4：记忆 ttl 9→5，传感器 9→5；行为按 game_time=104 < 110 仍在跑。
        ticks.set_time(104);
        {
            let mut world = World::new(&mut level, &mut ticks);
            let mut ctx = PreciseCtx::new(&mut world, me.id);
            brain.tick_by(&mut me, &mut mob, &mut ctx, 4);
        }
        assert_eq!(
            brain
                .memories
                .time_until_expiry(MemoryType::DangerDetectedRecently),
            5
        );
        assert_eq!(brain.sensors[0].time_to_tick, 5);
        assert_eq!(brain.running_names(), vec!["DoNothing"]);

        // t = 108, gap = 4：记忆 ttl 5→1，传感器 5→1；行为按 game_time=108 < 110 仍在跑。
        ticks.set_time(108);
        {
            let mut world = World::new(&mut level, &mut ticks);
            let mut ctx = PreciseCtx::new(&mut world, me.id);
            brain.tick_by(&mut me, &mut mob, &mut ctx, 4);
        }
        assert_eq!(
            brain
                .memories
                .time_until_expiry(MemoryType::DangerDetectedRecently),
            1
        );
        assert_eq!(brain.sensors[0].time_to_tick, 1);
        assert_eq!(brain.running_names(), vec!["DoNothing"]);

        // t = 112, gap = 4：记忆 ttl 1 < 4 到期清空；传感器 1 - 4 <= 0 触发并重置为 scan_rate(20)；
        // DoNothing 按 game_time = 112 > 110 停止。
        ticks.set_time(112);
        {
            let mut world = World::new(&mut level, &mut ticks);
            let mut ctx = PreciseCtx::new(&mut world, me.id);
            brain.tick_by(&mut me, &mut mob, &mut ctx, 4);
        }
        assert!(!brain.memories.has(MemoryType::DangerDetectedRecently));
        assert_eq!(
            brain.sensors[0].time_to_tick,
            i64::from(SensorKind::IsInWater.scan_rate())
        );
        assert!(brain.running_names().is_empty());
    }

    /// 降频档里的带大脑生物（山羊站在 48 格处，`throttle=32 / sleep=64`）每次决策时拿到
    /// `decision_gap > 1`，大脑的记忆过期和传感器倒计时按 `decision_gap` 推进；关闭降频时
    /// `decision_gap` 恒为 1，精确行为不变。
    #[test]
    fn throttled_brain_mob_advances_brain_by_decision_gap() {
        use crate::{
            BlockGameRules, BlockState, BlockTicks, World,
            entity::mob::{MobType, throttle},
            testing::TestLevel,
        };
        use mc_bamboo_core::{BlockPos, Vec3};
        use mc_bamboo_data::blocks;

        let mut level = TestLevel::new();
        let mut rules = BlockGameRules::vanilla();
        rules.mob_throttle_distance = 32;
        rules.mob_sleep_distance = 64;
        level.rules = rules;
        level
            .players
            .push(throttle::test_player(1, Vec3::new(0.5, 64.0, 0.5)));

        let mut ticks = BlockTicks::new();
        let uid = {
            let mut world = World::new(&mut level, &mut ticks);
            for x in 44..=52 {
                for z in -4..=4 {
                    world.set_block(BlockPos::new(x, 63, z), BlockState(blocks::GRASS_BLOCK), 0);
                }
            }
            let uid = world.spawn_entity(Entity::by_mob_type(
                MobType::Armadillo,
                Vec3::new(48.5, 64.0, 0.5),
            ));
            world.with_entity_mut(uid, &mut |entity| {
                entity.on_ground = true;
                entity.with_mob(|_, mob| {
                    let brain = mob.brain.as_mut().expect("armadillo has a brain");
                    assert!(
                        brain
                            .memories
                            .is_registered(MemoryType::DangerDetectedRecently)
                    );
                    brain.memories.set_with_ttl(
                        MemoryType::DangerDetectedRecently,
                        memory::MemoryValue::Bool(true),
                        30,
                    );
                    brain.sensors[0].time_to_tick = 18;
                });
            });
            uid
        };

        // 跑 12 刻（距离 48 在 [32,64] 中点，period = 3），记忆与传感器应总共推进约 10..=12 刻
        // 而不是降频后只推进 4 刻。
        for tick in 1_i64..=12 {
            ticks.set_time(tick);
            let mut world = World::new(&mut level, &mut ticks);
            world.tick_entities();
        }
        let mut world = World::new(&mut level, &mut ticks);
        let mut remaining_ttl = 0;
        let mut sensor_left = 0;
        world.with_entity_mut(uid, &mut |entity| {
            entity.with_mob(|_, mob| {
                let brain = mob.brain.as_ref().expect("armadillo has a brain");
                remaining_ttl = brain
                    .memories
                    .time_until_expiry(MemoryType::DangerDetectedRecently);
                sensor_left = brain.sensors[0].time_to_tick;
            });
        });
        // 30 刻的记忆过了 10..=12 刻后剩下 18..=20，而不是只扣了 4 刻剩 26。
        assert!(
            (18..=21).contains(&remaining_ttl),
            "throttled brain memory should advance by decision_gap, got ttl={remaining_ttl}"
        );
        // 初始 18 刻的传感器过了 10..=12 刻后剩下 6..=8，而不是只扣 4 刻剩 14。
        assert!(
            (6..=9).contains(&sensor_left),
            "throttled brain sensor should advance by decision_gap, got time_to_tick={sensor_left}"
        );
    }
}
