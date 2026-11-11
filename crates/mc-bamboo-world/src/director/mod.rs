//! 每个维度一个 Director：区块平面的纯状态机。
//!
//! 它只收事件、发效果，自己不干重活：不碰文件、不起线程、不认识区块包和 NBT。
//! 宿主把 [`Effect::Submit`] 的任务交给执行器（精确模式是内联执行器），
//! 把结果变成 [`Event::TaskDone`] 送回来；`mc-bamboo-game` 负责把
//! [`Effect::ToRegion`] 的消息交给 region。
//!
//! 区块束的类型是泛型参数 `C: ChunkBundle`：Director 只搬运、比较代数，
//! 不打开盒子。纯状态机的测试可以用一个数字当盒子。
//!
//! 所有表用 `BTreeMap`，任务按 `TaskId` 单调发号，输出按坐标排序；
//! 同样的初始状态和事件序列一定得到逐字节相同的效果序列。

mod pyramid;
mod state;

#[cfg(test)]
mod bench_tests;
#[cfg(test)]
mod tests;

pub use pyramid::{GENERATION, LOADING, PipelineKind, Step, loading_step, stage_index};
pub use state::{
    ChunkBundle, DebugRecord, DebugState, Director, DirectorConfig, Effect, Event,
    LevelClassification, Owner, RegionId, RegionMessage, SaveSeq, Task, TaskId, TaskResult,
    TicketOp, classify,
};
