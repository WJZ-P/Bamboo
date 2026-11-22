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

pub use behavior::Cx;
pub(in crate::entity::mob) use behaviors::current_position;

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

/// 一只生物的大脑。
///
/// One mob's brain.
#[derive(Debug, Clone, PartialEq)]
pub struct Brain {
    pub memories: Memories,
    pub sensors: Vec<Sensor>,
    entries: Vec<Entry>,
    requirements: Vec<(Activity, Vec<(MemoryType, MemoryStatus)>)>,
    erase_when_stopped: Vec<(Activity, Vec<MemoryType>)>,
    /// `activeActivities`：官方是 `HashSet`，这里的顺序只用于对拍输出（排过序）。
    ///
    /// `activeActivities`; a `HashSet` in vanilla, the order here only feeds the sorted
    /// oracle dump.
    active: Vec<Activity>,
    default_activity: Activity,
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
        let mut memories = Memories::default();
        let sensors: Vec<Sensor> = sensors.iter().map(|&kind| Sensor::new(kind)).collect();
        for sensor in &sensors {
            for &memory in sensor.kind.requires() {
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
        let mut brain = Self {
            memories,
            sensors,
            entries: order_entries(entries),
            requirements,
            erase_when_stopped,
            active: Vec::new(),
            default_activity: Activity::Idle,
        };
        brain.set_active_activity(brain.default_activity);
        brain
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
        for entry in &mut self.entries {
            entry.node.seed_gates(base, &mut index);
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
    }

    /// `Brain.activityRequirementsAreMet`：没有登记过的活动不满足。
    ///
    /// `Brain.activityRequirementsAreMet`: unknown activities never qualify.
    fn requirements_met(&self, activity: Activity) -> bool {
        self.requirements
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

    /// `Brain.tick`：记忆过期、传感器、启动、推进。
    ///
    /// `Brain.tick`: expire memories, sensors, start, then tick.
    pub fn tick(&mut self, me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
        self.memories.tick();
        let time = ctx.blocks().game_time();
        let Self {
            memories,
            sensors,
            entries,
            active,
            ..
        } = self;
        let mut cx = Cx {
            me,
            mob,
            ctx,
            memories,
            time,
        };
        for sensor in sensors.iter_mut() {
            sensor.tick(&mut cx);
        }
        for entry in entries.iter_mut() {
            if active.contains(&entry.activity) && entry.node.status == Status::Stopped {
                entry.node.try_start(&mut cx);
            }
        }
        let running: Vec<usize> = entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.node.status == Status::Running)
            .map(|(index, _)| index)
            .collect();
        for index in running {
            entries[index].node.tick_or_stop(&mut cx);
        }
    }

    /// `Brain.stopAll`：按遍历顺序停掉所有正在跑的行为。
    ///
    /// `Brain.stopAll`: stop every running behaviour in iteration order.
    pub fn stop_all(&mut self, me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
        let time = ctx.blocks().game_time();
        let Self {
            memories, entries, ..
        } = self;
        let mut cx = Cx {
            me,
            mob,
            ctx,
            memories,
            time,
        };
        for entry in entries.iter_mut() {
            if entry.node.status == Status::Running {
                entry.node.do_stop(&mut cx);
            }
        }
    }

    /// 行为表里出现过的活动（去重前，按遍历顺序）。测试 / 对拍用。
    ///
    /// The activities present in the behaviour table (before dedup, in iteration
    /// order), for tests / oracles.
    pub fn table_activities(&self) -> Vec<Activity> {
        self.entries.iter().map(|entry| entry.activity).collect()
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
        self.entries
            .iter()
            .filter(|entry| entry.node.status == Status::Running)
            .map(|entry| entry.node.debug_name())
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
        keyed.sort_unstable_by_key(|&(bucket, order, _)| (bucket, order));
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
        assert_eq!(Activity::Swim.name(), "swim");
        assert_eq!(Activity::LaySpawn.name(), "lay_spawn");
        assert_eq!(Activity::Tongue.name(), "tongue");
        assert_eq!(hash_map_capacity(12), 16);
        assert_eq!(hash_map_capacity(13), 32);
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
        let activities: Vec<Activity> = brain.entries.iter().map(|entry| entry.activity).collect();
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
}
