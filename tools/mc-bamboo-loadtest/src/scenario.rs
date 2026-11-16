//! 重负载场景：把 `mc-bamboo-hit-rate` 的九台机器复刻进正式的服务端世界。
//!
//! 世界内容完全由固定的 `machines::all()` 和网格坐标决定，不用手工存档、不用真实客户端。
//! `setup` 先给整片区域加 `/forceload` 票，等 Director 交付完再摆方块和方块实体；
//! `tick` 每刻按机器的 `Input` 翻红石块 / 补物品，等价于玩家在拉杆。
//!
//! Heavy-load scenarios: copies the nine `mc-bamboo-hit-rate` machines into the
//! real server world. The layout is derived from fixed constructors and grid
//! coordinates. `setup` force-loads the area first; `tick` drives the periodic
//! inputs on the world thread.

use std::sync::Mutex;

use mc_bamboo_blocks::{BlockState, HopperEntity, behaviour::machines::FurnaceEntity};
use mc_bamboo_core::{BlockPos, ChunkPos};
use mc_bamboo_data::{CookingKind, blocks, items};
use mc_bamboo_game::{BenchContainer, BenchHooks, BenchWorld};
use mc_bamboo_hit_rate::{machines, world::Input, world::Machine};
use mc_bamboo_protocol::item::ItemStack;

/// 分散布局里相邻机器的间距（区块）。机器最宽 19 格，占 2 个区块；
/// 6 - 2 = 4 个区块的空隙，满足设计稿「相互隔开至少 4 个区块」。
const SPREAD_STRIDE: i32 = 6;
/// 连成一片时的间距（格）。比最宽的机器多 5 格，机器之间不重叠但都在同一片区块里。
const BIG_STRIDE: i32 = 24;
/// 一台机器最多占的格数，用来算要强制加载的区块框。
const MACHINE_EXTENT: i32 = 20;

/// 场景种类，由 `serve --scenario <名字>` 选。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScenarioKind {
    /// N 台机器分散，机器之间隔 4 个区块以上。
    Spread4,
    Spread16,
    Spread64,
    /// 同样的台数但连成一片，分不出单元。
    Big16,
    Big64,
    /// 分散机器再加 50 名假玩家走动。
    Spread16Players,
    /// 真实地形（`level-type=normal`），没有机器。
    Normal20Walk,
    Normal20Fly,
    Normal50Walk,
    Normal50Fly,
}

impl ScenarioKind {
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "spread-4" => Self::Spread4,
            "spread-16" => Self::Spread16,
            "spread-64" => Self::Spread64,
            "big-16" => Self::Big16,
            "big-64" => Self::Big64,
            "spread-16-players" => Self::Spread16Players,
            "normal-20-walk" => Self::Normal20Walk,
            "normal-20-fly" => Self::Normal20Fly,
            "normal-50-walk" => Self::Normal50Walk,
            "normal-50-fly" => Self::Normal50Fly,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Spread4 => "spread-4",
            Self::Spread16 => "spread-16",
            Self::Spread64 => "spread-64",
            Self::Big16 => "big-16",
            Self::Big64 => "big-64",
            Self::Spread16Players => "spread-16-players",
            Self::Normal20Walk => "normal-20-walk",
            Self::Normal20Fly => "normal-20-fly",
            Self::Normal50Walk => "normal-50-walk",
            Self::Normal50Fly => "normal-50-fly",
        }
    }

    /// 假玩家场景的参数：`(人数, 移动方式, 布局, 时长秒)`。
    pub fn bots(self) -> (usize, &'static str, &'static str, u32) {
        match self {
            Self::Spread4 | Self::Big16 | Self::Spread16 | Self::Big64 | Self::Spread64 => {
                (1, "stand", "cluster", 60)
            }
            Self::Spread16Players => (50, "wander", "cluster", 60),
            Self::Normal20Walk | Self::Normal50Walk => {
                let n = if self == Self::Normal20Walk { 20 } else { 50 };
                (n, "walk", "spread", 60)
            }
            Self::Normal20Fly | Self::Normal50Fly => {
                let n = if self == Self::Normal20Fly { 20 } else { 50 };
                (n, "fly", "spread", 60)
            }
        }
    }

    fn machines(self) -> usize {
        match self {
            Self::Spread4 => 4,
            Self::Spread16 | Self::Big16 | Self::Spread16Players => 16,
            Self::Spread64 | Self::Big64 => 64,
            _ => 0,
        }
    }

    fn big(self) -> bool {
        matches!(self, Self::Big16 | Self::Big64)
    }

    /// 真实地形场景用 `level-type=normal` 和固定种子。
    pub fn normal(self) -> bool {
        matches!(
            self,
            Self::Normal20Walk | Self::Normal20Fly | Self::Normal50Walk | Self::Normal50Fly
        )
    }
}

/// 一条每刻要做的输入。
#[derive(Clone)]
enum Action {
    Clock {
        pos: BlockPos,
        period: i64,
        offset: i64,
    },
    Refill {
        pos: BlockPos,
        stack: ItemStack,
        period: i64,
        offset: i64,
    },
}

/// 一台摆好的机器。
struct Placement {
    build: usize,
    origin: (i32, i32),
}

struct Plan {
    placements: Vec<Placement>,
    chunks: Vec<ChunkPos>,
    actions: Vec<Action>,
    forced: bool,
    installed: bool,
}

/// 一个 `serve` 进程只跑一个场景；`setup` / `tick` 是普通函数指针，状态放这里。
static PLAN: Mutex<Option<Plan>> = Mutex::new(None);

/// 给 `serve` 用的钩子。没有机器时返回 `None`（真实地形场景不需要搭场景）。
pub fn configure(kind: ScenarioKind) -> Option<BenchHooks> {
    let builds = machines::all();
    let placements: Vec<Placement> = (0..kind.machines())
        .map(|i| Placement {
            build: i % builds.len(),
            origin: origin_of(kind, i),
        })
        .collect();
    let chunks = forced_chunks(&placements);
    let actions: Vec<Action> = placements
        .iter()
        .enumerate()
        .flat_map(|(index, p)| {
            let input = builds[p.build]().input;
            translate_input(input, p.origin, index as i64)
        })
        .collect();
    *PLAN.lock().expect("scenario plan lock") = Some(Plan {
        placements,
        chunks,
        actions,
        forced: false,
        installed: false,
    });
    if kind.machines() == 0 {
        return None;
    }
    Some(BenchHooks {
        setup: setup_scene,
        tick: tick_scene,
    })
}

fn translate_input(input: Input, origin: (i32, i32), index: i64) -> Vec<Action> {
    let shift = |pos: BlockPos| pos.offset(origin.0, 0, origin.1);
    // 每台机器给一个错开的相位：同一种机器如果都在同一刻翻转，
    // 尖峰是场景自己造出来的，不是真实服务器的行为。
    let offset = index.wrapping_mul(7).wrapping_add(1);
    match input {
        Input::None => Vec::new(),
        Input::Clock { pos, period } => vec![Action::Clock {
            pos: shift(pos),
            period,
            offset,
        }],
        Input::Clocks(clocks) => clocks
            .into_iter()
            .enumerate()
            .map(|(i, (pos, period))| Action::Clock {
                pos: shift(pos),
                period,
                offset: offset.wrapping_add(i as i64 * 3),
            })
            .collect(),
        Input::Refill { pos, stack, period } => vec![Action::Refill {
            pos: shift(pos),
            stack,
            period,
            offset,
        }],
    }
}

/// 网格坐标。`n` 台机器按 ceil(sqrt(n)) 列排；分散时一列 6 个区块，连片时 24 格。
fn origin_of(kind: ScenarioKind, index: usize) -> (i32, i32) {
    let count = kind.machines();
    let cols = (count as f64).sqrt().ceil() as i32;
    let col = index as i32 % cols;
    let row = index as i32 / cols;
    if kind.big() {
        (col * BIG_STRIDE, row * BIG_STRIDE)
    } else {
        (col * SPREAD_STRIDE * 16, row * SPREAD_STRIDE * 16)
    }
}

/// 机器所在的整片矩形都强制加载，保证是一片连通的已加载区域、
/// 每台机器周围 3×3 都在（方块 tick 的前提）。
fn forced_chunks(placements: &[Placement]) -> Vec<ChunkPos> {
    if placements.is_empty() {
        return Vec::new();
    }
    let mut min_x = i32::MAX;
    let mut max_x = i32::MIN;
    let mut min_z = i32::MAX;
    let mut max_z = i32::MIN;
    for p in placements {
        let (ox, oz) = p.origin;
        min_x = min_x.min(ox);
        max_x = max_x.max(ox + MACHINE_EXTENT);
        min_z = min_z.min(oz);
        max_z = max_z.max(oz + MACHINE_EXTENT);
    }
    // 出生点在 (8, 8)，把玩家周围也并进这片区域。
    min_x = min_x.min(0);
    min_z = min_z.min(0);
    let from_x = (min_x >> 4) - 1;
    let to_x = (max_x >> 4) + 1;
    let from_z = (min_z >> 4) - 1;
    let to_z = (max_z >> 4) + 1;
    let mut out = Vec::new();
    for x in from_x..=to_x {
        for z in from_z..=to_z {
            out.push(ChunkPos::new(x, z));
        }
    }
    out
}

/// 每刻开头调一次，直到所有区块交付完并把机器摆好。
fn setup_scene(world: &mut BenchWorld<'_>) -> bool {
    let mut guard = PLAN.lock().expect("scenario plan lock");
    let Some(plan) = guard.as_mut() else {
        return true;
    };
    if !plan.forced {
        world.force_chunks(&plan.chunks);
        plan.forced = true;
        return false;
    }
    if !world.all_loaded(&plan.chunks) {
        return false;
    }
    if !plan.installed {
        let builds = machines::all();
        for placement in &plan.placements {
            install_machine(world, builds[placement.build], placement.origin);
        }
        plan.installed = true;
    }
    true
}

fn install_machine(world: &mut BenchWorld<'_>, build: fn() -> Machine, origin: (i32, i32)) {
    let m = build();
    let shift = |pos: BlockPos| pos.offset(origin.0, 0, origin.1);
    for (pos, state) in &m.level.blocks {
        world.set_block(shift(*pos), state.0);
    }
    for (pos, slots) in &m.level.containers {
        let state = m.level.blocks.get(pos).copied().unwrap_or(BlockState::AIR);
        let kind = if state.is(blocks::HOPPER) {
            BenchContainer::Hopper
        } else {
            BenchContainer::Chest
        };
        world.set_container(shift(*pos), kind, slots.clone());
    }
    // 方块实体按机器里的方块状态重建：漏斗的冷却状态，熔炉的原料和燃料。
    let entities: Vec<(BlockPos, u32)> = m
        .level
        .block_entities
        .ticker_positions()
        .map(|(pos, _)| {
            (
                pos,
                m.level
                    .blocks
                    .get(&pos)
                    .copied()
                    .unwrap_or(BlockState::AIR)
                    .0,
            )
        })
        .collect();
    for (pos, state) in entities {
        let state = BlockState(state);
        let at = shift(pos);
        if state.is(blocks::HOPPER) {
            world.insert_block_entity(Box::new(HopperEntity::new(at)));
        } else if state.is(blocks::FURNACE) {
            let mut furnace = FurnaceEntity::new(at, CookingKind::Smelting);
            furnace.set_item(0, ItemStack::new(items::RAW_IRON, 32));
            furnace.set_item(1, ItemStack::new(items::COAL, 64));
            world.insert_block_entity(Box::new(furnace));
        }
    }
    let pending: Vec<_> = m.ticks.pending().collect();
    for tick in pending {
        let delay = tick.trigger_tick.clamp(1, i32::MAX as i64) as i32;
        world.schedule_tick(shift(tick.pos), tick.block, delay, tick.priority);
    }
    // `connect_wires` 在假世界里摆好的状态已经带属性，不需要重跑。
}

/// `setup` 成功之后的每刻输入。
fn tick_scene(world: &mut BenchWorld<'_>) {
    let now = world.game_time();
    let actions = {
        let guard = PLAN.lock().expect("scenario plan lock");
        guard
            .as_ref()
            .map(|plan| plan.actions.clone())
            .unwrap_or_default()
    };
    for action in actions {
        match action {
            Action::Clock {
                pos,
                period,
                offset,
            } if period > 0 && (now + offset) % period == 0 => {
                let on = BlockState(world.block_state(pos)).is(blocks::REDSTONE_BLOCK);
                let next = if on {
                    BlockState::AIR.0
                } else {
                    blocks::REDSTONE_BLOCK
                };
                world.set_block_and_update(pos, next);
            }
            Action::Refill {
                pos,
                stack,
                period,
                offset,
            } if period > 0 && (now + offset) % period == 0 => {
                world.refill(pos, stack);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spread_machines_are_four_chunks_apart() {
        let a = origin_of(ScenarioKind::Spread16, 0);
        let b = origin_of(ScenarioKind::Spread16, 1);
        assert_eq!((b.0 - a.0) / 16, SPREAD_STRIDE);
        assert!((b.0 - a.0) / 16 - 2 >= 4);
    }

    #[test]
    fn every_spread_size_rotates_through_the_nine_builders() {
        for kind in [
            ScenarioKind::Spread4,
            ScenarioKind::Spread16,
            ScenarioKind::Spread64,
        ] {
            let builds = machines::all().len();
            let placements: Vec<usize> = (0..kind.machines()).map(|i| i % builds).collect();
            assert_eq!(placements.len(), kind.machines());
            assert!(placements.contains(&0));
            // 16 / 64 台都覆盖到九种机器，4 台只取前四种。
            assert_eq!(
                placements.contains(&(builds - 1)),
                kind.machines() >= builds
            );
        }
    }

    #[test]
    fn big_layout_is_connected_but_not_overlapping() {
        let a = origin_of(ScenarioKind::Big16, 0);
        let b = origin_of(ScenarioKind::Big16, 1);
        assert!(b.0 - a.0 > MACHINE_EXTENT);
        assert!((b.0 - a.0) / 16 <= 2);
    }

    #[test]
    fn forced_rectangle_covers_every_machine() {
        let builds = machines::all().len();
        let placements: Vec<Placement> = (0..16)
            .map(|i| Placement {
                build: i % builds,
                origin: origin_of(ScenarioKind::Spread16, i),
            })
            .collect();
        let chunks = forced_chunks(&placements);
        assert!(!chunks.is_empty());
        for p in &placements {
            let (ox, oz) = p.origin;
            for dx in 0..=MACHINE_EXTENT {
                for dz in 0..=MACHINE_EXTENT {
                    let pos = ChunkPos::new((ox + dx) >> 4, (oz + dz) >> 4);
                    assert!(chunks.contains(&pos), "{pos:?} missing");
                }
            }
        }
    }
}
