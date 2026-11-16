//! 生成耗时的测量钩子：默认构建里全是空操作，行为和结果都不变。
//!
//! 阶段计时用 [`begin`] / [`mark`] / [`pause`] / [`finish`]；地物内部另有一组独立的钩子
//! （[`feature_begin`]、[`candidate`]、[`place_begin`]），按 decoration step 和 placed
//! feature 记候选位置的高度分布与耗时，不碰阶段计时。
//!
//! 只有开了 `perf` 特性的构建才记东西；没开时所有函数是空实现，调用点不求值、不改
//! 求值顺序，也不碰线程本地状态。[`Floors`] 是三个「只生成切割线以上」的对照实验用的
//! 下界，默认全 `None`（等价于原来的行为），显式设过之后生成结果会变，只用来测时间。
//!
//! Measurement hooks for generation timing; every function is a no-op in builds
//! without the `perf` feature, so call sites keep the exact same behaviour and
//! evaluation order. [`begin`] / [`mark`] / [`pause`] / [`finish`] time whole phases;
//! the feature hooks ([`feature_begin`], [`candidate`], [`place_begin`]) record
//! per-step and per-placed-feature candidate heights and durations without touching
//! the phase clock. [`Floors`] holds the lower bounds used by the "generate above the
//! cut line" experiments: all `None` by default (the original behaviour), and setting
//! one changes the generated blocks, so it is only for timing.

/// 一个阶段。`mark` 记的是「当前阶段」的墙钟。
///
/// One phase. `mark` attributes the elapsed wall clock to the current phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// `ChunkStatus.STRUCTURE_STARTS`：判定起点并生成结构件。
    /// `ChunkStatus.STRUCTURE_STARTS`: decide starts and generate pieces.
    StructureStarts,
    /// `ChunkStatus.STRUCTURE_REFERENCES`：17×17 邻域登记起点引用。
    /// `ChunkStatus.STRUCTURE_REFERENCES`: register starts over the 17x17 ring.
    StructureReferences,
    /// `ChunkStatus.BIOMES`：一个区块的 quart 群系格。
    /// `ChunkStatus.BIOMES`: one chunk's quart biome cells.
    Biomes,
    /// `doFill` 进来到含水层建好之前的准备（体积、`Context`、缓冲）。
    /// `doFill` setup before the aquifer is built (volume, contexts, buffers).
    TerrainSetup,
    /// `NoiseBasedAquifer` 构造器（含 `maxSurfaceLevel` 那次成批求值）。
    /// The `NoiseBasedAquifer` constructor, including the `maxSurfaceLevel` batch.
    AquiferInit,
    /// `finalDensity.sampleVolume` 整块成批求值。
    /// The whole-volume `finalDensity.sampleVolume`.
    NoiseDensity,
    /// `computeSubstance` 和填方块的逐方块循环。
    /// The per-block `computeSubstance` / layer fill loop.
    Substance,
    /// `MaterialSystem.buildSurface`。
    /// `MaterialSystem.buildSurface`.
    Surface,
    /// `generateCarvers` 造 `CarvingMask`（17×17 邻域 + 隧道）。
    /// `generateCarvers` building the `CarvingMask` (17x17 neighbourhood + tunnels).
    CarverMask,
    /// `applyCarvingMask`。
    /// `applyCarvingMask`.
    CarverApply,
    /// 拼 3×3 邻居视图（`features` / `light` 的输入）。
    /// Assembling the 3x3 neighbour view (the input of `features` / `light`).
    View,
    /// `applyBiomeDecoration` 本身。
    /// `applyBiomeDecoration` itself.
    Features,
    /// `light` 阶段把 3×3 方块装成光引擎的 `Chunk`。
    /// The `light` stage converting the 3x3 blocks into light-engine `Chunk`s.
    LightView,
    /// `generate_light` 本身。
    /// `generate_light` itself.
    Light,
    /// `build_region_column` 组装。
    /// `build_region_column` assembly.
    Assembly,
}

/// [`Phase`] 的项数。 The number of [`Phase`] variants.
pub const PHASE_COUNT: usize = 15;

impl Phase {
    /// 所有阶段，按 [`Phase::index`] 的顺序。
    /// Every phase, in [`Phase::index`] order.
    pub const ALL: [Self; PHASE_COUNT] = [
        Self::StructureStarts,
        Self::StructureReferences,
        Self::Biomes,
        Self::TerrainSetup,
        Self::AquiferInit,
        Self::NoiseDensity,
        Self::Substance,
        Self::Surface,
        Self::CarverMask,
        Self::CarverApply,
        Self::View,
        Self::Features,
        Self::LightView,
        Self::Light,
        Self::Assembly,
    ];

    /// 报告里用的名字。 The name used in reports.
    pub fn name(self) -> &'static str {
        match self {
            Self::StructureStarts => "structure_starts",
            Self::StructureReferences => "structure_references",
            Self::Biomes => "biomes",
            Self::TerrainSetup => "terrain_setup",
            Self::AquiferInit => "aquifer_init",
            Self::NoiseDensity => "noise_density",
            Self::Substance => "substance",
            Self::Surface => "surface",
            Self::CarverMask => "carver_mask",
            Self::CarverApply => "carver_apply",
            Self::View => "view",
            Self::Features => "features",
            Self::LightView => "light_view",
            Self::Light => "light",
            Self::Assembly => "assembly",
        }
    }

    /// 数组下标。 The array index.
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// 高度分层数：y=-64..320 每 8 格一层。
/// Height bands: y=-64..320 in steps of 8.
pub const Y_BANDS: usize = 48;

/// 实验用的四个下界，`None` 表示按原来的下界生成。
///
/// * `noise`：`doFill` 的密度只算这个 y 以上（含水层照旧，低处直接当固体）；
/// * `surface`：地表规则的最深扫描行；
/// * `carver`：`applyCarvingMask` 换方块的最低行；
/// * `feature`：`Feature.place` 的候选位置低于它就跳过。
///
/// The four experimental lower bounds; `None` keeps the original behaviour. `noise`
/// limits the `doFill` density volume, `surface` the deepest surface-rule row,
/// `carver` the lowest row `applyCarvingMask` rewrites, and `feature` skips
/// candidates below it. Setting any of them changes the generated blocks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Floors {
    pub noise: Option<i32>,
    pub surface: Option<i32>,
    pub carver: Option<i32>,
    pub feature: Option<i32>,
}

/// 一个 placed feature 在一次 `features` 里的记录。
///
/// One placed feature's record inside a single `features` run.
#[derive(Debug, Clone)]
pub struct FeatureRecord {
    /// `Registry::placed` 的下标。
    /// Index into `Registry::placed`.
    pub placed: u32,
    /// decoration step（0..11）。
    /// The decoration step (0..11).
    pub step: usize,
    /// 候选位置个数。 Number of candidate positions.
    pub candidates: u64,
    /// 候选位置落在切割线以上的个数。 Candidates above the cut line.
    pub above_cut: u64,
    /// 候选位置的最低 / 最高 y；没有候选时都是 0。
    /// Lowest / highest candidate y; both 0 when there is no candidate.
    pub min_y: i32,
    pub max_y: i32,
    /// 每个 8 格高度带里的候选个数，下标 `(y - min_y) / 8`。
    /// Candidates per 8-block band, indexed `(y - min_y) / 8`.
    pub y_counts: [u64; Y_BANDS],
    /// `Feature.place` 加起来的时间。 Summed `Feature.place` time.
    pub place_ms: f64,
    /// 修饰器链（含候选位置的计算）的时间。
    /// Time in the modifier chain (including producing the candidates).
    pub modifier_ms: f64,
}

impl FeatureRecord {
    /// 一条空记录。 An empty record.
    pub fn empty(placed: u32, step: usize) -> Self {
        Self {
            placed,
            step,
            candidates: 0,
            above_cut: 0,
            min_y: 0,
            max_y: 0,
            y_counts: [0; Y_BANDS],
            place_ms: 0.0,
            modifier_ms: 0.0,
        }
    }
}

/// 一次记录的完整结果。
/// The full result of one recording.
#[derive(Debug, Clone, Default)]
pub struct Record {
    /// 各阶段墙钟（毫秒）。 Per-phase wall clock in milliseconds.
    pub phases_ms: [f64; PHASE_COUNT],
    /// 各 decoration step 的时间。 Per-decoration-step time.
    pub steps_ms: [f64; crate::feature::DECORATION_STEPS],
    /// 各 decoration step 的候选总数。 Candidates per decoration step.
    pub steps_candidates: [u64; crate::feature::DECORATION_STEPS],
    /// 逐个 placed feature。 Per placed feature.
    pub features: Vec<FeatureRecord>,
    /// 记录的方块范围下界（`settings.min_y`）。 Recorded lower bound (`settings.min_y`).
    pub min_y: i32,
    /// 本次记录的切割线。 The cut line of this recording.
    pub cut: i32,
    /// 含水层「这之上不采样」的高度，由 `Aquifer::new` 填。
    /// The aquifer's "no sampling above" height, filled in by `Aquifer::new`.
    pub skip_above_y: Option<i32>,
    /// 记录的三个实验下界。 The three experimental floors of this recording.
    pub floors: Floors,
    /// 只算某个 y 以上的密度时，那次求值用的下界；`None` 表示整个高度都算了。
    /// The lower bound used when only sampling above some y; `None` means the full column.
    pub density_floor: Option<i32>,
}

impl Record {
    /// 一组阶段的和。 The sum of a set of phases.
    pub fn sum(&self, phases: &[Phase]) -> f64 {
        phases
            .iter()
            .map(|phase| self.phases_ms[phase.index()])
            .sum()
    }

    /// `doFill` 的墙钟（准备 + 含水层 + 密度 + 填方块）。
    /// The `doFill` wall clock (setup + aquifer + density + fill loop).
    pub fn do_fill(&self) -> f64 {
        self.sum(&[
            Phase::TerrainSetup,
            Phase::AquiferInit,
            Phase::NoiseDensity,
            Phase::Substance,
        ])
    }

    /// 地形三个阶段的和，即 `terrain` 状态的墙钟。
    /// The three terrain steps, i.e. the `terrain` wall clock.
    pub fn terrain(&self) -> f64 {
        self.do_fill() + self.sum(&[Phase::Surface, Phase::CarverMask, Phase::CarverApply])
    }

    /// 一个区块从头到尾的墙钟（群系、地形、地物、光照、组装）。
    /// One chunk end to end: biomes, terrain, features, light, assembly.
    pub fn total(&self) -> f64 {
        self.phases_ms.iter().sum()
    }
}

#[cfg(feature = "perf")]
mod imp {
    use std::{
        cell::RefCell,
        time::{Duration, Instant},
    };

    use super::{FeatureRecord, Floors, Phase, Record, Y_BANDS};

    struct State {
        last: Instant,
        current: Option<Phase>,
        record: Record,
        step: usize,
        feature: Option<usize>,
        feature_start: Instant,
        suspended: bool,
    }

    thread_local! {
        static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
        /// 实验下界独立于记录：不开记录时也能设，用来测「半成品」的整块耗时。
        /// The floors live outside the recording so they can be set with no recording
        /// running, to time a half-built chunk end to end.
        static FLOORS: RefCell<Floors> = const {
            RefCell::new(Floors {
                noise: None,
                surface: None,
                carver: None,
                feature: None,
            })
        };
    }

    impl State {
        fn close(&mut self, now: Instant) {
            if let Some(phase) = self.current.take() {
                let elapsed = now.duration_since(self.last).as_secs_f64() * 1000.0;
                self.record.phases_ms[phase.index()] += elapsed;
            }
            self.last = now;
        }
    }

    /// 开始一次记录，当前阶段是 `phase`。
    /// Start a recording; the current phase is `phase`.
    pub fn begin(min_y: i32, cut: i32, phase: Phase) {
        STATE.with(|state| {
            let now = Instant::now();
            *state.borrow_mut() = Some(State {
                last: now,
                current: Some(phase),
                record: Record {
                    min_y,
                    cut,
                    floors: floors(),
                    ..Record::default()
                },
                step: 0,
                feature: None,
                feature_start: now,
                suspended: false,
            });
        });
    }

    /// 结束当前阶段、开始 `next`。
    /// Close the current phase and start `next`.
    pub fn mark(next: Phase) {
        STATE.with(|state| {
            let mut slot = state.borrow_mut();
            let Some(state) = slot.as_mut() else {
                return;
            };
            if state.suspended {
                return;
            }
            let now = Instant::now();
            state.close(now);
            state.current = Some(next);
        });
    }

    /// 结束当前阶段，不再计时。
    /// Close the current phase without starting another.
    pub fn pause() {
        STATE.with(|state| {
            let mut slot = state.borrow_mut();
            let Some(state) = slot.as_mut() else {
                return;
            };
            if state.suspended {
                return;
            }
            let now = Instant::now();
            state.close(now);
        });
    }

    /// 暂停计时：之后的 `mark` / `pause` 都忽略，直到 [`resume`]。
    ///
    /// 用在「外层工具已经单独计时了这一段」的地方（比如只测某一个阶段时，其余时间不该
    /// 再记一遍）。`mark` 在暂停期间不切换当前阶段，`resume` 会从这里继续。
    ///
    /// Suspend timing: subsequent `mark` / `pause` calls are ignored until [`resume`].
    /// Used where the outer tool times a span itself; `mark` does not switch phases while
    /// suspended, and `resume` continues from the same phase.
    pub fn suspend() {
        STATE.with(|state| {
            let mut slot = state.borrow_mut();
            let Some(state) = slot.as_mut() else {
                return;
            };
            if state.suspended {
                return;
            }
            let now = Instant::now();
            state.close(now);
            state.suspended = true;
        });
    }

    /// 恢复计时，从当前阶段继续。
    /// Resume timing in the current phase.
    pub fn resume() {
        STATE.with(|state| {
            let mut slot = state.borrow_mut();
            let Some(state) = slot.as_mut() else {
                return;
            };
            if !state.suspended {
                return;
            }
            state.suspended = false;
            state.last = Instant::now();
        });
    }

    /// 结束记录并取出结果。
    /// Finish the recording and take the result.
    pub fn finish() -> Option<Record> {
        pause();
        STATE.with(|state| state.borrow_mut().take().map(|state| state.record))
    }

    /// 开始一个 placed feature。
    /// Begin one placed feature.
    pub fn feature_begin(step: usize, placed: u32) {
        STATE.with(|state| {
            let mut slot = state.borrow_mut();
            let Some(state) = slot.as_mut() else {
                return;
            };
            let now = Instant::now();
            state.record.features.push(FeatureRecord {
                placed,
                step,
                min_y: i32::MAX,
                max_y: i32::MIN,
                ..FeatureRecord::empty(placed, step)
            });
            state.feature = Some(state.record.features.len() - 1);
            state.feature_start = now;
            state.step = step;
        });
    }

    /// 结束当前 placed feature：把剩下的时间记进它的 `modifier_ms`。
    /// End the current placed feature, charging the remaining time to `modifier_ms`.
    pub fn feature_end() {
        STATE.with(|state| {
            let mut slot = state.borrow_mut();
            let Some(state) = slot.as_mut() else {
                return;
            };
            let Some(index) = state.feature.take() else {
                return;
            };
            let now = Instant::now();
            let total = now.duration_since(state.feature_start).as_secs_f64() * 1000.0;
            let step = state.step;
            let record = &mut state.record.features[index];
            record.modifier_ms += (total - record.place_ms).max(0.0);
            if record.candidates == 0 {
                record.min_y = 0;
                record.max_y = 0;
            }
            state.record.steps_ms[step] += total;
        });
    }

    /// 记一个候选位置。 Record one candidate position.
    pub fn candidate(y: i32) {
        STATE.with(|state| {
            let mut slot = state.borrow_mut();
            let Some(state) = slot.as_mut() else {
                return;
            };
            let Some(index) = state.feature else {
                return;
            };
            let band = (y - state.record.min_y) / 8;
            let above = y >= state.record.cut;
            let step = state.step;
            let record = &mut state.record.features[index];
            record.candidates += 1;
            if above {
                record.above_cut += 1;
            }
            record.min_y = record.min_y.min(y);
            record.max_y = record.max_y.max(y);
            if (0..Y_BANDS as i32).contains(&band) {
                record.y_counts[band as usize] += 1;
            }
            state.record.steps_candidates[step] += 1;
        });
    }

    /// 开始一次 `Feature.place`；没有记录时返回 `None`（`place_end` 会直接返回）。
    /// Begin one `Feature.place`; `None` when nothing is recorded, which `place_end`
    /// then ignores.
    #[inline]
    pub fn place_begin() -> Option<Instant> {
        STATE.with(|state| {
            if state.borrow().is_some() {
                Some(Instant::now())
            } else {
                None
            }
        })
    }

    /// 结束一次 `Feature.place`。 End one `Feature.place`.
    pub fn place_end(started: Option<Instant>) {
        let Some(started) = started else {
            return;
        };
        let elapsed = started.elapsed();
        STATE.with(|state| {
            let mut slot = state.borrow_mut();
            let Some(state) = slot.as_mut() else {
                return;
            };
            let Some(index) = state.feature else {
                return;
            };
            state.record.features[index].place_ms += elapsed.as_secs_f64() * 1000.0;
        });
    }

    /// 含水层「这之上不采样」的高度。 The aquifer's "no sampling above" height.
    pub fn note_skip_above(y: i32) {
        STATE.with(|state| {
            if let Some(state) = state.borrow_mut().as_mut() {
                state.record.skip_above_y = Some(y);
            }
        });
    }

    /// 记下这次密度求值用的下界（只算某个 y 以上时）。
    /// Record the lower bound of this density sampling (when only above some y).
    pub fn note_density_floor(y: i32) {
        STATE.with(|state| {
            if let Some(state) = state.borrow_mut().as_mut() {
                state.record.density_floor = Some(y);
            }
        });
    }

    /// 设置实验下界。 Set the experimental floors.
    pub fn set_floors(next: Floors) {
        FLOORS.with(|floors| *floors.borrow_mut() = next);
    }

    /// 取实验下界。 The experimental floors.
    #[inline]
    pub fn floors() -> Floors {
        FLOORS.with(|floors| *floors.borrow())
    }

    /// 把一段时长累加进一个阶段，供调用方自带计时器时用。
    /// Add a duration to one phase, for callers that time it themselves.
    pub fn add(phase: Phase, elapsed: Duration) {
        STATE.with(|state| {
            if let Some(state) = state.borrow_mut().as_mut() {
                state.record.phases_ms[phase.index()] += elapsed.as_secs_f64() * 1000.0;
            }
        });
    }
}

#[cfg(not(feature = "perf"))]
mod imp {
    use std::time::{Duration, Instant};

    use super::{Floors, Phase, Record};

    #[inline]
    pub fn begin(_min_y: i32, _cut: i32, _phase: Phase) {}

    #[inline]
    pub fn mark(_next: Phase) {}

    #[inline]
    pub fn pause() {}

    #[inline]
    pub fn suspend() {}

    #[inline]
    pub fn resume() {}

    #[inline]
    pub fn finish() -> Option<Record> {
        None
    }

    #[inline]
    pub fn feature_begin(_step: usize, _placed: u32) {}

    #[inline]
    pub fn feature_end() {}

    #[inline]
    pub fn candidate(_y: i32) {}

    #[inline]
    pub fn place_begin() -> Option<Instant> {
        None
    }

    #[inline]
    pub fn place_end(_started: Option<Instant>) {}

    #[inline]
    pub fn note_skip_above(_y: i32) {}

    #[inline]
    pub fn note_density_floor(_y: i32) {}

    #[inline]
    pub fn set_floors(_floors: Floors) {}

    #[inline]
    pub fn floors() -> Floors {
        Floors::default()
    }

    #[inline]
    pub fn add(_phase: Phase, _elapsed: Duration) {}
}

pub use imp::{
    add, begin, candidate, feature_begin, feature_end, finish, floors, mark, note_density_floor,
    note_skip_above, pause, place_begin, place_end, resume, set_floors, suspend,
};
