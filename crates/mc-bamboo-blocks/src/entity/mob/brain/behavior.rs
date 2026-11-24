//! 行为的公共骨架：官方 `BehaviorControl` 的四种实现——`Behavior`（有时长）、
//! `OneShot`（`BehaviorBuilder` 拼出来的一次性触发）、`DoNothing` 和 `GateBehavior`
//! （`RunOne` 是它的洗牌、只跑一个的版本）。每种具体行为的逻辑在 [`super::behaviors`]。
//!
//! The behaviour skeleton: vanilla's four `BehaviorControl` implementations —
//! `Behavior` (timed), `OneShot` (the one-shot triggers `BehaviorBuilder` builds),
//! `DoNothing` and `GateBehavior` (`RunOne` is its shuffled run-one form). The logic of
//! each concrete behaviour lives in [`super::behaviors`].

use super::{
    super::Mob,
    Activity, ActivityState,
    memory::{Memories, MemoryMasks, MemoryStatus, MemoryType},
};
use crate::entity::{Entity, JavaRandom, ctx::EntityCtx};

pub use super::behaviors::{BodyPredicate, Kind, MoveToTargetSink, Speed, Trigger};

/// 行为跑一刻时能碰到的一切：自己（实体和生物，大脑已经取出来）、上下文、记忆、
/// 活动切换控制、世界时钟（`level.getGameTime()`，也就是行为的 `timestamp`）。
///
/// Everything a behaviour can touch in one tick: itself (entity and mob, with the brain
/// taken out), the context, the memories, the activity controller and the game time
/// (`level.getGameTime()`, the behaviours' `timestamp`).
pub struct Cx<'a> {
    pub me: &'a mut Entity,
    pub mob: &'a mut Mob,
    pub ctx: &'a mut dyn EntityCtx,
    pub memories: &'a mut Memories,
    pub(super) activity: ActivityState<'a>,
    pub time: i64,
}

impl Cx<'_> {
    /// `level.getRandom().nextInt(bound)`。
    ///
    /// `level.getRandom().nextInt(bound)`.
    pub fn level_next_int(&self, bound: i32) -> i32 {
        self.ctx.blocks().level_random_int(bound)
    }

    /// `Brain.isActive`。
    ///
    /// `Brain.isActive`.
    pub fn is_active(&self, activity: Activity) -> bool {
        self.activity.is_active(activity)
    }

    /// `Brain.setDefaultActivity`。
    ///
    /// `Brain.setDefaultActivity`.
    pub fn set_default_activity(&mut self, activity: Activity) {
        self.activity.set_default_activity(activity);
    }

    /// `Brain.setActiveActivityIfPossible`：条件满足则切过去，否则退回默认活动。
    ///
    /// `Brain.setActiveActivityIfPossible`: switches if requirements hold, else uses
    /// the default activity.
    pub fn set_active_activity_if_possible(&mut self, activity: Activity) {
        self.activity
            .set_active_activity_if_possible(self.memories, activity);
    }

    /// `Brain.updateActivityFromSchedule`：20 刻节流（`time - last > 20`）。
    ///
    /// `Brain.updateActivityFromSchedule`: 20-tick throttle (`time - last > 20`).
    pub fn update_activity_from_schedule(&mut self) {
        let blocks = self.ctx.blocks();
        self.activity
            .update_activity_from_schedule(self.memories, blocks, self.time);
    }
}

/// `Behavior.Status`。
///
/// `Behavior.Status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Status {
    #[default]
    Stopped,
    Running,
}

/// 行为属于官方哪一种 `BehaviorControl` 实现。
///
/// Which vanilla `BehaviorControl` implementation a behaviour is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// `Behavior`：`tryStart` 用世界随机源取 `[min, max]` 的时长。
    ///
    /// `Behavior`: `tryStart` draws a duration in `[min, max]` from the level random.
    Timed { min: i32, max: i32 },
    /// `OneShot`：触发成功就算跑起来，同一刻的推进阶段就停。
    ///
    /// `OneShot`: running once triggered, stopped in the same tick's tick phase.
    OneShot,
    /// `DoNothing`：总能启动，到时就停。
    ///
    /// `DoNothing`: always starts, stops when the time is up.
    DoNothing { min: i32, max: i32 },
    /// `GateBehavior`。
    ///
    /// `GateBehavior`.
    Gate,
}

/// 一个行为入口条件预编译成的三组位掩码，判定只做位运算。
///
/// A behaviour's entry conditions precompiled into three bit masks; checking is bit-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Preconditions {
    pub present: u64,
    pub absent: u64,
    pub registered: u64,
}

impl Preconditions {
    fn of(required: &[(MemoryType, MemoryStatus)]) -> Self {
        let mut out = Self::default();
        for &(memory, status) in required {
            match status {
                MemoryStatus::ValuePresent => out.present |= memory.bit(),
                MemoryStatus::ValueAbsent => out.absent |= memory.bit(),
                MemoryStatus::Registered => out.registered |= memory.bit(),
            }
        }
        out
    }

    /// 全部入口条件满足：`tryStart` 一进门就只做这三次按位与。
    ///
    /// Every entry condition holds: `tryStart` begins with just these three bit-ands.
    pub fn holds(&self, masks: &MemoryMasks) -> bool {
        masks.present & self.present == self.present
            && masks.absent & self.absent == self.absent
            && masks.registered & self.registered == self.registered
    }
}

/// 一个行为：状态、结束时刻、预编译的入口条件、具体逻辑。
///
/// One behaviour: status, end timestamp, precompiled preconditions and logic.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub status: Status,
    end_timestamp: i64,
    pub required: Preconditions,
    pub kind: Kind,
}

impl Node {
    pub fn new(kind: Kind) -> Self {
        let required = Preconditions::of(kind.required());
        Self {
            status: Status::Stopped,
            end_timestamp: 0,
            required,
            kind,
        }
    }

    /// `getRequiredMemories()` 全部登记进记忆表；闸门再加上子行为的。
    ///
    /// Registers `getRequiredMemories()`; gates add their children's.
    pub fn register_memories(&self, memories: &mut Memories) {
        for &(memory, _) in self.kind.required() {
            memories.register(memory);
        }
        if let Kind::Gate(gate) = &self.kind {
            for child in &gate.children {
                child.node.register_memories(memories);
            }
        }
    }

    pub(super) fn seed_gates(&mut self, base: i64, index: &mut i64) {
        // 长跳的角度洗牌和闸门一样不带种子，按同一个序号接着播种。
        // The long-jump angle shuffle is unseeded like the gates and takes the next index.
        if let Kind::Goat(super::super::goat::Behavior::LongJump(jump)) = &mut self.kind {
            jump.angle_random = JavaRandom::new(super::shuffle_seed(base, *index));
            *index += 1;
        }
        if let Kind::Frog(super::super::frog::Behavior::LongJump(jump)) = &mut self.kind {
            jump.angle_random = JavaRandom::new(super::shuffle_seed(base, *index));
            *index += 1;
        }
        if let Kind::Gate(gate) = &mut self.kind {
            gate.random = JavaRandom::new(super::shuffle_seed(base, *index));
            *index += 1;
            for child in &mut gate.children {
                child.node.seed_gates(base, index);
            }
        }
    }

    /// 入口条件全部满足（`Behavior.hasRequiredMemories` / `BehaviorBuilder` 的记忆条件）。
    /// 官方这些判定没有副作用、不取随机数，所以用预编译的掩码做纯位运算，取数顺序不变。
    ///
    /// Every entry condition holds (`Behavior.hasRequiredMemories` / the
    /// `BehaviorBuilder` memory conditions). Vanilla's checks have no side effects and
    /// draw no random, so a precompiled mask check is bit-only and keeps the draw order.
    fn has_required_memories(&self, memories: &Memories) -> bool {
        self.required.holds(&memories.masks())
    }

    /// `BehaviorControl.tryStart`。
    ///
    /// `BehaviorControl.tryStart`.
    pub fn try_start(&mut self, cx: &mut Cx) -> bool {
        match self.kind.class() {
            Class::Timed { min, max } => {
                if !self.has_required_memories(cx.memories) || !self.kind.extra_start(cx) {
                    return false;
                }
                self.status = Status::Running;
                let duration = min + cx.level_next_int(max + 1 - min);
                self.end_timestamp = cx.time + i64::from(duration);
                self.kind.start(cx);
                true
            }
            Class::OneShot => {
                if !self.has_required_memories(cx.memories) || !self.kind.trigger(cx) {
                    return false;
                }
                self.status = Status::Running;
                true
            }
            Class::DoNothing { min, max } => {
                self.status = Status::Running;
                let duration = min + cx.level_next_int(max + 1 - min);
                self.end_timestamp = cx.time + i64::from(duration);
                true
            }
            Class::Gate => {
                if !self.has_required_memories(cx.memories) {
                    return false;
                }
                self.status = Status::Running;
                let Kind::Gate(gate) = &mut self.kind else {
                    unreachable!("闸门类的行为一定是闸门");
                };
                gate.start_children(cx);
                true
            }
        }
    }

    /// `BehaviorControl.tickOrStop`。
    ///
    /// `BehaviorControl.tickOrStop`.
    pub fn tick_or_stop(&mut self, cx: &mut Cx) {
        match self.kind.class() {
            Class::Timed { .. } => {
                let timed_out = !self.kind.never_times_out() && cx.time > self.end_timestamp;
                if !timed_out && self.kind.can_still_use(cx) {
                    self.kind.tick(cx);
                } else {
                    self.do_stop(cx);
                }
            }
            Class::OneShot => self.do_stop(cx),
            Class::DoNothing { .. } => {
                if cx.time > self.end_timestamp {
                    self.do_stop(cx);
                }
            }
            Class::Gate => {
                let Kind::Gate(gate) = &mut self.kind else {
                    unreachable!("闸门类的行为一定是闸门");
                };
                for child in &mut gate.children {
                    if child.node.status == Status::Running {
                        child.node.tick_or_stop(cx);
                    }
                }
                if gate
                    .children
                    .iter()
                    .all(|child| child.node.status != Status::Running)
                {
                    self.do_stop(cx);
                }
            }
        }
    }

    /// `BehaviorControl.doStop`。
    ///
    /// `BehaviorControl.doStop`.
    pub fn do_stop(&mut self, cx: &mut Cx) {
        self.status = Status::Stopped;
        match self.kind.class() {
            Class::Timed { .. } => self.kind.stop(cx),
            Class::OneShot | Class::DoNothing { .. } => {}
            Class::Gate => {
                let Kind::Gate(gate) = &mut self.kind else {
                    unreachable!("闸门类的行为一定是闸门");
                };
                for child in &mut gate.children {
                    if child.node.status == Status::Running {
                        child.node.do_stop(cx);
                    }
                }
                for &memory in &gate.exit_erased {
                    cx.memories.erase(memory);
                }
            }
        }
    }

    /// 对拍输出用的类名：闸门带上正在跑的子行为。
    ///
    /// The class name for oracle dumps; gates append their running children.
    pub fn debug_name(&self) -> String {
        match &self.kind {
            Kind::Gate(gate) => {
                let running: Vec<String> = gate
                    .children
                    .iter()
                    .filter(|child| child.node.status == Status::Running)
                    .map(|child| child.node.debug_name())
                    .collect();
                format!("{}[{}]", gate.name(), running.join(","))
            }
            kind => kind.name().to_owned(),
        }
    }

    /// 对拍输出用的静态类名：闸门带上装载顺序的全部子行为（不管在不在跑）。
    ///
    /// The static class name for oracle dumps; gates list every child in load order,
    /// running or not.
    pub fn table_name(&self) -> String {
        match &self.kind {
            Kind::Gate(gate) => {
                let children: Vec<String> = gate
                    .children
                    .iter()
                    .map(|child| child.node.table_name())
                    .collect();
                format!("{}[{}]", gate.name(), children.join(","))
            }
            kind => kind.name().to_owned(),
        }
    }
}

/// `ShufflingList.WeightedEntry`。
///
/// `ShufflingList.WeightedEntry`.
#[derive(Debug, Clone, PartialEq)]
pub struct GateChild {
    pub node: Node,
    weight: i32,
    rand_weight: f64,
}

/// `GateBehavior`：入口条件、退出时清的记忆、排序和运行策略、子行为和洗牌随机源。
///
/// `GateBehavior`: entry conditions, exit-erased memories, order and running policy,
/// the children and the shuffle random.
#[derive(Debug, Clone, PartialEq)]
pub struct Gate {
    pub entry: Vec<(MemoryType, MemoryStatus)>,
    pub exit_erased: Vec<MemoryType>,
    pub shuffled: bool,
    pub run_one: bool,
    pub children: Vec<GateChild>,
    random: JavaRandom,
}

impl Gate {
    /// `new RunOne(entryCondition, weightedBehaviors)`：洗牌、只跑一个、不清记忆。
    ///
    /// `new RunOne(entryCondition, weightedBehaviors)`: shuffled, run one, erases nothing.
    pub fn run_one(entry: Vec<(MemoryType, MemoryStatus)>, children: Vec<(Kind, i32)>) -> Self {
        Self::new_gate(entry, Vec::new(), true, true, children)
    }

    /// `new GateBehavior(ORDERED, TRY_ALL, ...)`：不洗牌、按登记顺序尝试启动全部停着的
    /// 子行为（青蛙 `SWIM` 活动用）。和 `RunOne` 共用 `start_children` 骨架，只是两个策略
    /// 开关不同；`OrderPolicy.ORDERED` 是空操作，`RunningPolicy.TRY_ALL` 对每个都 `tryStart`。
    ///
    /// `new GateBehavior(ORDERED, TRY_ALL, ...)`: unshuffled, tries to start every stopped
    /// child in registration order (the frog `SWIM` activity). It shares `start_children`
    /// with `RunOne`; only the two policy flags differ. `OrderPolicy.ORDERED` is a no-op,
    /// `RunningPolicy.TRY_ALL` calls `tryStart` on each one.
    pub fn ordered_try_all(
        entry: Vec<(MemoryType, MemoryStatus)>,
        children: Vec<(Kind, i32)>,
    ) -> Self {
        Self::new_gate(entry, Vec::new(), false, false, children)
    }

    fn new_gate(
        entry: Vec<(MemoryType, MemoryStatus)>,
        exit_erased: Vec<MemoryType>,
        shuffled: bool,
        run_one: bool,
        children: Vec<(Kind, i32)>,
    ) -> Self {
        Self {
            entry,
            exit_erased,
            shuffled,
            run_one,
            children: children
                .into_iter()
                .map(|(kind, weight)| GateChild {
                    node: Node::new(kind),
                    weight,
                    rand_weight: 0.0,
                })
                .collect(),
            random: JavaRandom::new(0),
        }
    }

    pub fn name(&self) -> &'static str {
        if self.shuffled && self.run_one && self.exit_erased.is_empty() {
            "RunOne"
        } else {
            "GateBehavior"
        }
    }

    /// `ShufflingList.shuffle`：每项一个 `nextFloat`，权重是 `-pow(r, 1 / weight)`，
    /// 按升序稳定排序（Java 的 `List.sort` 是稳定的）。权重为 1 时 `pow(r, 1) == r`，
    /// 省掉 `powf`，但 `nextFloat` 照样每项取一次，取数次数不变。
    ///
    /// `ShufflingList.shuffle`: one `nextFloat` per entry, weight `-pow(r, 1 / weight)`,
    /// then a stable ascending sort. For weight 1, `pow(r, 1) == r` so `powf` is skipped,
    /// but `nextFloat` is still drawn once per entry, keeping the draw count unchanged.
    fn shuffle(&mut self) {
        for child in &mut self.children {
            let random = self.random.next_float();
            child.rand_weight = if child.weight == 1 {
                -f64::from(random)
            } else {
                let exponent = 1.0_f32 / child.weight as f32;
                -f64::from(random).powf(f64::from(exponent))
            };
        }
        self.children
            .sort_by(|a, b| a.rand_weight.total_cmp(&b.rand_weight));
    }

    /// `GateBehavior.tryStart` 的后半：排序策略，然后按运行策略逐个尝试停着的子行为。
    ///
    /// The second half of `GateBehavior.tryStart`: apply the order policy, then try
    /// the stopped children by the running policy.
    fn start_children(&mut self, cx: &mut Cx) {
        if self.shuffled {
            self.shuffle();
        }
        for child in &mut self.children {
            if child.node.status != Status::Stopped {
                continue;
            }
            if child.node.try_start(cx) && self.run_one {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 入口条件的三组掩码是纯位运算：present / absent / registered 各自都要齐。
    ///
    /// The three precondition masks are pure bit ops; present / absent / registered
    /// must each be fully satisfied.
    #[test]
    fn preconditions_are_pure_bit_ops() {
        let req = Preconditions::of(&[
            (MemoryType::LookTarget, MemoryStatus::ValuePresent),
            (MemoryType::WalkTarget, MemoryStatus::ValueAbsent),
            (MemoryType::Path, MemoryStatus::Registered),
        ]);
        let masks = MemoryMasks {
            present: MemoryType::LookTarget.bit(),
            absent: MemoryType::WalkTarget.bit(),
            registered: MemoryType::LookTarget.bit()
                | MemoryType::WalkTarget.bit()
                | MemoryType::Path.bit(),
        };
        assert!(req.holds(&masks));
        assert!(!req.holds(&MemoryMasks {
            present: 0,
            ..masks
        }));
        assert!(!req.holds(&MemoryMasks { absent: 0, ..masks }));
        assert!(!req.holds(&MemoryMasks {
            registered: 0,
            ..masks
        }));
    }

    /// 权重 1 时 `pow(r, 1)` 与 `r` 逐位相同，洗牌略过 `powf` 不会改变排序值。
    ///
    /// For weight 1, `pow(r, 1)` equals `r` bit-for-bit, so skipping `powf` in the
    /// shuffle changes no sorted value.
    #[test]
    fn weight_one_powf_is_the_identity() {
        let mut random = JavaRandom::new(0x5eed_1234);
        for _ in 0..100_000 {
            let r = f64::from(random.next_float());
            assert_eq!(r.powf(1.0).to_bits(), r.to_bits());
        }
    }

    /// `RunOne` 洗牌、只跑一个、对拍类名 `RunOne`；`ORDERED + TRY_ALL` 的 `GateBehavior`
    /// 不洗牌、不提前停、类名 `GateBehavior`。两个策略开关正好对应官方两个枚举。
    ///
    /// `RunOne` shuffles, runs one and dumps as `RunOne`; the `ORDERED + TRY_ALL`
    /// `GateBehavior` neither shuffles nor stops early and dumps as `GateBehavior`. The
    /// two flags map one-to-one onto vanilla's two enums.
    #[test]
    fn gate_policies_match_vanilla() {
        let children = vec![
            (Kind::DoNothing { min: 30, max: 60 }, 1),
            (Kind::DoNothing { min: 30, max: 60 }, 1),
        ];
        let run_one = Gate::run_one(Vec::new(), children.clone());
        assert!(run_one.shuffled);
        assert!(run_one.run_one);
        assert_eq!(run_one.name(), "RunOne");

        let ordered = Gate::ordered_try_all(Vec::new(), children);
        assert!(!ordered.shuffled);
        assert!(!ordered.run_one);
        assert_eq!(ordered.name(), "GateBehavior");
        assert_eq!(ordered.children.len(), 2);
    }
}
