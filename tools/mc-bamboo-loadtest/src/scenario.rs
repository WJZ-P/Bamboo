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

use std::{collections::BTreeSet, sync::Mutex};

use mc_bamboo_blocks::{BlockState, HopperEntity, behaviour::machines::FurnaceEntity};
use mc_bamboo_core::{BlockPos, ChunkPos, Vec3};
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

/// 「岛」场景里相邻两组的间距（区块）。按总设计稿 6.1，视距 10 时两名玩家的区块
/// 坐标差到 36 一定不合并；一组自己约占 21 个区块（玩家视野加机器和掉落物），
/// 64 让组间至少空出 40 多个区块，视距到 16 也仍是不同的岛。
///
/// Spacing between groups in the island scenarios, in chunks. Design 6.1 says two
/// players 36 chunks apart never share a region at view distance 10; one group spans
/// about 21 chunks, so 64 leaves a gap of 40+ chunks.
const GROUP_STRIDE_CHUNKS: i32 = 64;
/// 组内九台机器按 3×3 摆，间距 48 格（3 个区块），机器之间至少隔 1 个区块。
const GROUP_MACHINE_STRIDE: i32 = 48;
/// 每组的假玩家数。
const GROUP_BOTS: usize = 10;
/// 每组的掉落物按 16×16 摆，间距 2 格，不会互相合并（合并范围水平 0.5 格）。
const GROUP_ITEM_SIDE: i32 = 16;
/// 掉落物方阵的西北角，相对组原点。离玩家站的地方约 70 格，假玩家捡不到。
const GROUP_ITEM_CORNER: (i32, i32) = (-48, -48);
/// 玩家站的地方，相对组原点：机器方阵南边的中间，离最远的机器约 9 个区块。
const GROUP_PLAYER_SPOT: (i32, i32) = (58, -24);
/// 超平坦的站立高度，和 `flat::surface_y()` / 假玩家的 `SPAWN` 一致。
const FLAT_SURFACE_Y: f64 = -60.0;

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
    /// 「岛」的基线：K 组，每组 10 名假玩家、九台机器、256 个掉落物，组间隔
    /// 64 个区块，以后一定是 K 个不同的岛。每组内容逐位相同，`islands-1` 就是
    /// 「一组单独跑」的对照。
    ///
    /// Island baselines: K identical groups (10 bots, nine machines, 256 dropped
    /// items each) 64 chunks apart, so they will be K separate islands.
    Islands1,
    Islands2,
    Islands4,
    Islands8,
    Islands16,
    /// 同 `islands-K`，但第 0 组另外多放一大批不能堆叠的掉落物（个数由
    /// `BAMBOO_LOADTEST_HEAVY_ITEMS` 给，缺省 [`DEFAULT_HEAVY_ITEMS`]），让这一组单独就跑不满
    /// 20 TPS：量「一个岛过载只拖慢它自己」。
    ///
    /// Like `islands-K`, but group 0 also gets a large batch of unstackable
    /// dropped items (`BAMBOO_LOADTEST_HEAVY_ITEMS`, default
    /// [`DEFAULT_HEAVY_ITEMS`]) so that group alone cannot keep 20 TPS: to
    /// measure whether one overloaded island only slows itself.
    IslandsHeavy4,
    IslandsHeavy8,
    /// 拆合：8 名站着的假玩家排成一列（相邻两人隔 30 个区块，已加载区域相接，并成一个岛）；每
    /// [`SHUTTLE_PERIOD`] 刻单数号的人被送到很远的地方、再送回来。他们一走，这一列断成
    /// 几段，迟滞过后拆开；他们一回来，各段又并起来。量拆合的次数和每次耗时。
    ///
    /// Splits and merges: 8 standing bots in a row (30 chunks apart, loaded
    /// areas nearly touching, one island); every [`SHUTTLE_PERIOD`] ticks the odd-numbered
    /// bots are sent far away and then back. When they leave the row breaks
    /// into pieces that split after the hysteresis; when they return the
    /// pieces merge again. Measures how often and how long these take.
    IslandsShuttle,
    /// 长时间压力（岛的实施计划第 11 步）：[`ROAM_CAMPS`] 个营地（每个是一组 `islands-K` 的
    /// 内容，强制加载）排成网格；[`ROAM_BOTS`] 名假玩家在营地之间飞来飞去、在营地里游走，
    /// 有时飞到野外，自己定时断线重连、聊天、发命令（`roam` 移动方式）。场景每隔一段时间
    /// 把人直接放到别的营地、从控制台发 `/tp`、`/say`、`/time`、`/weather`、`/gamerule`、
    /// 碰方块的 `/setblock`。拆合在真实的聚散轨迹上发生。
    ///
    /// Long-run stress (island plan step 11): [`ROAM_CAMPS`] camps (each one
    /// `islands-K` group, force-loaded) on a grid; [`ROAM_BOTS`] bots fly
    /// between camps, wander inside them, sometimes head into the wild, and
    /// reconnect, chat and run commands on their own (`roam` pattern). The
    /// scene periodically places bots at other camps and sends console
    /// `/tp`, `/say`, `/time`, `/weather`, `/gamerule` and a block-touching
    /// `/setblock`. Splits and merges follow real gathering and scattering.
    IslandsRoam,
}

/// `islands-roam` 的营地数，排成 [`ROAM_COLUMNS`] 列。
pub const ROAM_CAMPS: usize = 6;
const ROAM_COLUMNS: usize = 3;
/// 相邻营地隔几个区块：飞（33 格/秒）过去约 47 秒。一个营地连同在里面游走的人约占 20 个
/// 区块宽，隔 96 个区块时两营地之间空出 70 多个区块，路上的人才有机会和出发的营地拆开、再
/// 并进要去的营地；隔 48 个区块时 40 个人在路上和野外的视野把六个营地连成一个岛，试跑的
/// 7 分钟里一直只有一个岛（`docs/profiling.md` 第 17 节）。
///
/// Camps are this many chunks apart (about 47 s of flying). A camp with the
/// bots wandering in it is about 20 chunks wide; 96 leaves 70+ chunks
/// between camps, so travellers get a chance to split off the camp they left
/// and merge into the one they reach. At 48 the views of 40 bots travelling
/// and in the wild joined all six camps into one island for the whole
/// 7-minute trial.
const ROAM_CAMP_STRIDE_CHUNKS: i32 = 96;
/// `islands-roam` 的假玩家数。
pub const ROAM_BOTS: usize = 40;
/// `islands-roam` 的控制台动作的周期（刻）。
const ROAM_PLACE_EVERY: i64 = 600;
const ROAM_TP_EVERY: i64 = 900;
const ROAM_SAY_EVERY: i64 = 400;
const ROAM_TIME_EVERY: i64 = 1200;
const ROAM_WEATHER_EVERY: i64 = 2400;
const ROAM_SETBLOCK_EVERY: i64 = 1800;
const ROAM_GAMERULE_EVERY: i64 = 3000;
/// `islands-roam` 从控制台改一名假玩家的游戏模式、给他加效果的周期（刻）。
///
/// How often `islands-roam` changes a bot's game mode and gives it an
/// effect from the console (ticks).
const ROAM_GAMEMODE_EVERY: i64 = 700;
const ROAM_EFFECT_EVERY: i64 = 500;
/// `islands-roam` 每隔这么多刻从控制台发一次 `defaultgamemode creative`（要整个世界的命令）。
/// 第一刻也发一次：假玩家进服时就是创造模式，飞着赶路不会因为「悬空太久」被踢。
///
/// Every this many ticks `islands-roam` sends `defaultgamemode creative`
/// (a whole-world command) from the console; also on its first tick, so bots
/// join in creative mode and are not kicked for floating while they fly.
const ROAM_WORLD_EVERY: i64 = 1200;

/// 第 `camp` 个营地里假玩家游走的中心（方块坐标的 x、z）。离营地原点不远，在强制加载的范围里。
///
/// Where bots wander inside camp `camp` (block x, z).
pub fn roam_camp_spot(camp: usize) -> (f64, f64) {
    let (ox, oz) = camp_origin(camp);
    (
        f64::from(ox + GROUP_PLAYER_SPOT.0) + 0.5,
        f64::from(oz + GROUP_PLAYER_SPOT.1) + 0.5,
    )
}

/// 第 `camp` 个营地的原点（方块坐标），网格排开。
fn camp_origin(camp: usize) -> (i32, i32) {
    let stride = ROAM_CAMP_STRIDE_CHUNKS * 16;
    (
        (camp % ROAM_COLUMNS) as i32 * stride,
        (camp / ROAM_COLUMNS) as i32 * stride,
    )
}

/// 拆合场景里单数号的人每隔这么多刻换一次位置（走 / 回）。拆分有 200 刻迟滞，这里给足。
///
/// Every this many ticks the odd-numbered bots of the shuttle scene switch
/// place (away / back); splits wait 200 ticks, so this leaves room.
pub const SHUTTLE_PERIOD: i64 = 400;
/// 拆合场景的人数。
const SHUTTLE_BOTS: usize = 8;
/// 拆合场景一列里相邻两人隔几个区块。视距 8 时一名玩家加载的范围约 27 个区块宽：隔 30 个
/// 区块时相邻两人的已加载区域只空 3 个区块，并成一个岛；单数号的人走开以后剩下的人隔
/// 60 个区块，空出 30 多个区块，够拆分的距离。
const SHUTTLE_STRIDE_CHUNKS: i32 = 30;
/// 拆合场景里走开的人去的地方离那一列多远（区块）。
const SHUTTLE_AWAY_CHUNKS: i32 = 200;

/// 重的那一组额外的掉落物个数（缺省）。
///
/// Extra dropped items in the heavy group (default).
pub const DEFAULT_HEAVY_ITEMS: usize = 20_000;
/// 改重的那一组掉落物个数的环境变量。
///
/// The environment variable overriding the heavy group's item count.
pub const HEAVY_ITEMS_ENV: &str = "BAMBOO_LOADTEST_HEAVY_ITEMS";
/// 每隔这么多刻从控制台发一条要整个世界的命令（`defaultgamemode survival`），量多岛时
/// 把全部岛拼起来再拆回去的耗时。缺省 0（不发）。
///
/// Every this many ticks, send a whole-world command from the console
/// (`defaultgamemode survival`) to time joining and splitting every island.
/// Default 0 (never).
pub const WORLD_COMMAND_ENV: &str = "BAMBOO_LOADTEST_WORLD_COMMAND_EVERY";

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
            "islands-1" => Self::Islands1,
            "islands-2" => Self::Islands2,
            "islands-4" => Self::Islands4,
            "islands-8" => Self::Islands8,
            "islands-16" => Self::Islands16,
            "islands-heavy-4" => Self::IslandsHeavy4,
            "islands-heavy-8" => Self::IslandsHeavy8,
            "islands-shuttle" => Self::IslandsShuttle,
            "islands-roam" => Self::IslandsRoam,
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
            Self::Islands1 => "islands-1",
            Self::Islands2 => "islands-2",
            Self::Islands4 => "islands-4",
            Self::Islands8 => "islands-8",
            Self::Islands16 => "islands-16",
            Self::IslandsHeavy4 => "islands-heavy-4",
            Self::IslandsHeavy8 => "islands-heavy-8",
            Self::IslandsShuttle => "islands-shuttle",
            Self::IslandsRoam => "islands-roam",
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
            Self::Islands1
            | Self::Islands2
            | Self::Islands4
            | Self::Islands8
            | Self::Islands16
            | Self::IslandsHeavy4
            | Self::IslandsHeavy8 => (GROUP_BOTS * self.groups(), "wander", "cluster", 60),
            Self::IslandsShuttle => (SHUTTLE_BOTS, "stand", "cluster", 90),
            Self::IslandsRoam => (ROAM_BOTS, "roam", "cluster", 3600),
        }
    }

    /// 「岛」场景的组数；别的场景是 0。
    pub fn groups(self) -> usize {
        match self {
            Self::Islands1 => 1,
            Self::Islands2 => 2,
            Self::Islands4 => 4,
            Self::Islands8 | Self::IslandsHeavy8 => 8,
            Self::Islands16 => 16,
            Self::IslandsHeavy4 => 4,
            Self::IslandsRoam => ROAM_CAMPS,
            _ => 0,
        }
    }

    /// 第 0 组额外的掉落物个数；不是重场景时是 0。
    ///
    /// Extra dropped items in group 0; 0 unless a heavy scenario.
    fn heavy_items(self) -> usize {
        if !matches!(self, Self::IslandsHeavy4 | Self::IslandsHeavy8) {
            return 0;
        }
        std::env::var(HEAVY_ITEMS_ENV)
            .ok()
            .and_then(|value| value.trim().parse().ok())
            .unwrap_or(DEFAULT_HEAVY_ITEMS)
    }

    fn machines(self) -> usize {
        match self {
            Self::Spread4 => 4,
            Self::Spread16 | Self::Big16 | Self::Spread16Players => 16,
            Self::Spread64 | Self::Big64 => 64,
            Self::Islands1
            | Self::Islands2
            | Self::Islands4
            | Self::Islands8
            | Self::Islands16
            | Self::IslandsHeavy4
            | Self::IslandsHeavy8
            | Self::IslandsRoam => machines::all().len() * self.groups(),
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
    kind: ScenarioKind,
    placements: Vec<Placement>,
    chunks: Vec<ChunkPos>,
    actions: Vec<Action>,
    /// 「岛」场景的组数，别的场景是 0。
    groups: usize,
    /// 第 0 组额外的掉落物个数（重场景）。
    heavy_items: usize,
    /// 拆合场景：每名假玩家上次被送到的是「在列里」还是「走开」。
    shuttle: Option<std::collections::BTreeMap<String, bool>>,
    /// 每隔几刻发一条要整个世界的命令，0 = 不发。
    world_command_every: i64,
    /// 已经送到自己那一组的假玩家。
    placed: BTreeSet<String>,
    /// `islands-roam` 已经发过第一条 `defaultgamemode creative`。
    roam_started: bool,
    /// `islands-roam` 已经从控制台 `op` 过的假玩家。
    opped: BTreeSet<String>,
    forced: bool,
    installed: bool,
}

/// 一个 `serve` 进程只跑一个场景；`setup` / `tick` 是普通函数指针，状态放这里。
static PLAN: Mutex<Option<Plan>> = Mutex::new(None);

/// 给 `serve` 用的钩子。所有场景都用同一个 `setup`：先把固定区域强制加载好，
/// 再（有机器时）摆方块；真实地形场景只做前一步。
pub fn configure(kind: ScenarioKind) -> BenchHooks {
    let builds = machines::all();
    let placements: Vec<Placement> = (0..kind.machines())
        .map(|i| Placement {
            build: i % builds.len(),
            origin: origin_of(kind, i),
        })
        .collect();
    let groups = kind.groups();
    let chunks = if kind == ScenarioKind::IslandsShuttle {
        Vec::new()
    } else if groups > 0 {
        group_chunks(kind)
    } else {
        forced_chunks(&placements)
    };
    let actions: Vec<Action> = placements
        .iter()
        .enumerate()
        .flat_map(|(index, p)| {
            let input = builds[p.build]().input;
            // 岛场景每组的相位相同，组和组的内容逐刻一样。
            let phase = if groups > 0 {
                index % builds.len()
            } else {
                index
            };
            translate_input(input, p.origin, phase as i64)
        })
        .collect();
    *PLAN.lock().expect("scenario plan lock") = Some(Plan {
        kind,
        placements,
        chunks,
        actions,
        groups,
        heavy_items: kind.heavy_items(),
        shuttle: (kind == ScenarioKind::IslandsShuttle).then(std::collections::BTreeMap::new),
        world_command_every: std::env::var(WORLD_COMMAND_ENV)
            .ok()
            .and_then(|value| value.trim().parse().ok())
            .unwrap_or(0),
        placed: BTreeSet::new(),
        roam_started: false,
        opped: BTreeSet::new(),
        forced: false,
        installed: false,
    });
    BenchHooks {
        setup: setup_scene,
        tick: tick_scene,
    }
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
    if kind.groups() > 0 {
        let per_group = machines::all().len();
        let (gx, gz) = group_origin(kind, index / per_group);
        let local = (index % per_group) as i32;
        return (
            gx + local % 3 * GROUP_MACHINE_STRIDE,
            gz + local / 3 * GROUP_MACHINE_STRIDE,
        );
    }
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

/// 第 `group` 组的原点（方块坐标）。`islands-K` 的组沿 x 轴排开，`islands-roam` 的营地排成网格。
fn group_origin(kind: ScenarioKind, group: usize) -> (i32, i32) {
    if kind == ScenarioKind::IslandsRoam {
        return camp_origin(group);
    }
    (group as i32 * GROUP_STRIDE_CHUNKS * 16, 0)
}

/// 每组各自强制加载一个矩形：九台机器、掉落物方阵和玩家站的地方。
/// 组和组之间不加载，所以是 K 片互不相连的已加载区域。
///
/// Each group force-loads its own rectangle; nothing between groups is loaded.
fn group_chunks(kind: ScenarioKind) -> Vec<ChunkPos> {
    let mut out = Vec::new();
    for group in 0..kind.groups() {
        let (ox, oz) = group_origin(kind, group);
        let min_x = ox + GROUP_ITEM_CORNER.0.min(GROUP_PLAYER_SPOT.0);
        let min_z = oz + GROUP_ITEM_CORNER.1.min(GROUP_PLAYER_SPOT.1);
        let max_x = ox + 2 * GROUP_MACHINE_STRIDE + MACHINE_EXTENT;
        let max_z = oz + 2 * GROUP_MACHINE_STRIDE + MACHINE_EXTENT;
        for x in (min_x >> 4) - 1..=(max_x >> 4) + 1 {
            for z in (min_z >> 4) - 1..=(max_z >> 4) + 1 {
                out.push(ChunkPos::new(x, z));
            }
        }
    }
    out
}

/// 第 `group` 组的掉落物：16×16、间距 2 格，物品种类轮换，落在地表上。
fn group_items(kind: ScenarioKind, group: usize) -> Vec<(Vec3, ItemStack)> {
    const KINDS: [i32; 4] = [
        items::COBBLESTONE,
        items::DIRT,
        items::OAK_PLANKS,
        items::STICK,
    ];
    let (ox, oz) = group_origin(kind, group);
    let mut out = Vec::new();
    for i in 0..GROUP_ITEM_SIDE {
        for j in 0..GROUP_ITEM_SIDE {
            let x = ox + GROUP_ITEM_CORNER.0 + 2 * i;
            let z = oz + GROUP_ITEM_CORNER.1 + 2 * j;
            let kind = KINDS[((i + j) % KINDS.len() as i32) as usize];
            out.push((
                Vec3::new(f64::from(x) + 0.5, FLAT_SURFACE_Y, f64::from(z) + 0.5),
                ItemStack::new(kind, 1),
            ));
        }
    }
    out
}

/// 重的那一组（第 0 组）额外的掉落物：不能堆叠的钻石剑，不会互相合并，摆在机器方阵西边
/// 那条空地上（x 在 −46..−2，z 在 −46..114），每格放几把、错开一点。
///
/// The heavy group's (group 0's) extra items: unstackable diamond swords that
/// never merge, on the free strip west of the machines (x −46..−2, z
/// −46..114), a few per block, slightly offset.
fn heavy_items(count: usize) -> Vec<(Vec3, ItemStack)> {
    const WIDTH: usize = 45;
    const LENGTH: usize = 161;
    let mut out = Vec::with_capacity(count);
    for index in 0..count {
        let spot = index % (WIDTH * LENGTH);
        let layer = index / (WIDTH * LENGTH);
        let x = -46 + (spot % WIDTH) as i32;
        let z = -46 + (spot / WIDTH) as i32;
        let shift = 0.2 + 0.15 * (layer % 4) as f64;
        out.push((
            Vec3::new(f64::from(x) + shift, FLAT_SURFACE_Y, f64::from(z) + shift),
            ItemStack::new(items::DIAMOND_SWORD, 1),
        ));
    }
    out
}

/// 假玩家名字末尾的编号（`Bot0007` → 7）。
fn bot_index(name: &str) -> Option<usize> {
    let digits = name.trim_start_matches(|c: char| !c.is_ascii_digit());
    digits.parse().ok()
}

/// 机器所在的整片矩形都强制加载，保证是一片连通的已加载区域、
/// 每台机器周围 3×3 都在（方块 tick 的前提）。
fn forced_chunks(placements: &[Placement]) -> Vec<ChunkPos> {
    // 真实地形没有机器：改成进程内由 `setup` 按出生点铺开的固定方框。
    // 这里先给一个以原点为中心的 21×21 区块，`setup` 会按 `spawn_chunk()` 平移。
    if placements.is_empty() {
        return (-10..=10)
            .flat_map(|x| (-10..=10).map(move |z| ChunkPos::new(x, z)))
            .collect();
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
        // 真实地形的出生点由种子决定，启动时才知道；把方框平移到出生点。
        if plan.placements.is_empty() {
            let spawn = world.spawn_chunk();
            for chunk in &mut plan.chunks {
                *chunk = ChunkPos::new(chunk.x + spawn.x, chunk.z + spawn.z);
            }
        }
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
        for group in 0..plan.groups {
            for (position, stack) in group_items(plan.kind, group) {
                world.spawn_item(position, stack);
            }
        }
        for (position, stack) in heavy_items(plan.heavy_items) {
            world.spawn_item(position, stack);
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

/// `setup` 成功之后的每刻输入。多岛时每台机器按它所在的岛的时钟翻（落后的岛慢），那个岛
/// 这一刻不在主控手里就跳过。
///
/// Each tick's inputs once `setup` succeeded. With several islands each
/// machine follows its own island's clock (a lagging island is behind) and
/// is skipped while that island is away.
fn tick_scene(world: &mut BenchWorld<'_>) {
    let (actions, world_command_every) = {
        let mut guard = PLAN.lock().expect("scenario plan lock");
        if let Some(plan) = guard.as_mut() {
            place_new_bots(world, plan);
            shuttle_bots(world, plan);
            roam_actions(world, plan);
        }
        guard
            .as_ref()
            .map(|plan| (plan.actions.clone(), plan.world_command_every))
            .unwrap_or_default()
    };
    if world_command_every > 0 && world.game_time() % world_command_every == 0 {
        world.console_command("defaultgamemode survival");
    }
    for action in actions {
        match action {
            Action::Clock {
                pos,
                period,
                offset,
            } if period > 0
                && world
                    .game_time_at(pos)
                    .is_some_and(|now| (now + offset) % period == 0) =>
            {
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
            } if period > 0
                && world
                    .game_time_at(pos)
                    .is_some_and(|now| (now + offset) % period == 0) =>
            {
                world.refill(pos, stack);
            }
            _ => {}
        }
    }
}

/// 拆合场景：按家岛的时钟，前半个周期全部在列里，后半个周期单数号的人走开；每名假玩家
/// 位置变了才送一次，送不成（他的岛不在手里）下一刻再试。
///
/// The shuttle scene: by the home clock, everyone is in the row for the
/// first half of each period and the odd-numbered bots are away for the
/// second half; a bot is sent only when its place changes, retried next tick
/// while its island is away.
fn shuttle_bots(world: &mut BenchWorld<'_>, plan: &mut Plan) {
    let Some(sent) = plan.shuttle.as_mut() else {
        return;
    };
    let away_phase = world.game_time().div_euclid(SHUTTLE_PERIOD) % 2 == 1;
    for name in world.player_names() {
        let index = bot_index(&name).unwrap_or(0);
        let away = away_phase && index % 2 == 1;
        if sent.get(&name) == Some(&away) {
            continue;
        }
        let x = index as i32 * SHUTTLE_STRIDE_CHUNKS * 16 + 8;
        let z = if away {
            SHUTTLE_AWAY_CHUNKS * 16 + 8
        } else {
            8
        };
        let at = Vec3::new(f64::from(x) + 0.5, FLAT_SURFACE_Y, f64::from(z) + 0.5);
        if world.teleport_player(&name, at) {
            sent.insert(name, away);
        }
    }
}

/// `islands-roam` 的控制台动作和直接放置，按全局游戏时间的固定周期；挑人、挑营地用一个由
/// 时间派生的伪随机数，同一次运行可重复。控制台命令和连接层送来的一样排队（有岛不在手里时
/// 等它回来）；直接放置送不成（那人的岛不在手里）就跳过这一次。
///
/// The console actions and direct placements of `islands-roam`, on fixed
/// periods of the global game time; bots and camps are picked by a
/// pseudo-random number derived from the time, so a run repeats. Console
/// lines queue like those from the connection layer; a placement whose
/// island is away is skipped this time.
fn roam_actions(world: &mut BenchWorld<'_>, plan: &mut Plan) {
    if plan.kind != ScenarioKind::IslandsRoam {
        return;
    }
    let now = world.game_time();
    if now <= 0 {
        return;
    }
    let pick = |salt: u64, below: usize| -> usize {
        let mut x = (now as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ salt;
        x ^= x >> 31;
        x = x.wrapping_mul(0xbf58_476d_1ce4_e5b9);
        x ^= x >> 29;
        (x % below.max(1) as u64) as usize
    };
    if !plan.roam_started || now % ROAM_WORLD_EVERY == 0 {
        world.console_command("defaultgamemode creative");
        plan.roam_started = true;
    }
    let names = world.player_names();
    if now % ROAM_PLACE_EVERY == 0 && !names.is_empty() {
        let name = &names[pick(1, names.len())];
        let (x, z) = roam_camp_spot(pick(2, ROAM_CAMPS));
        let _ = world.teleport_player(name, Vec3::new(x, FLAT_SURFACE_Y, z));
    }
    // 假玩家自己发的 `gamemode` / `kill @s` / `effect` 要管理员：新名字各 `op` 一次。
    // Bots' own `gamemode` / `kill @s` / `effect` need operators: op each new name once.
    for name in &names {
        if plan.opped.insert(name.clone()) {
            world.console_command(&format!("op {name}"));
        }
    }
    if now % ROAM_GAMEMODE_EVERY == 0 && !names.is_empty() {
        // 改别的岛上的人的模式（他本来就是创造，不改状态）：借他脚下那格的岛。
        // Another island's player (already creative, so nothing changes):
        // borrows the island under their feet.
        let name = &names[pick(6, names.len())];
        world.console_command(&format!("gamemode creative {name}"));
    }
    if now % ROAM_EFFECT_EVERY == 0 && !names.is_empty() {
        let name = &names[pick(7, names.len())];
        world.console_command(&format!("effect give {name} minecraft:speed 5"));
    }
    if now % ROAM_TP_EVERY == 0 && names.len() >= 2 {
        let a = pick(3, names.len());
        let b = (a + 1 + pick(4, names.len() - 1)) % names.len();
        world.console_command(&format!("tp {} {}", names[a], names[b]));
    }
    if now % ROAM_SAY_EVERY == 0 {
        world.console_command(&format!("say roam {now}"));
    }
    if now % ROAM_TIME_EVERY == 0 {
        world.console_command("time add 1000");
    }
    if now % ROAM_WEATHER_EVERY == 0 {
        let weather = if (now / ROAM_WEATHER_EVERY) % 2 == 0 {
            "weather rain 1200"
        } else {
            "weather clear"
        };
        world.console_command(weather);
    }
    if now % ROAM_SETBLOCK_EVERY == 0 {
        // 碰方块的命令：在一个营地掉落物方阵旁边的空地上来回放、拆一块石头。
        // A block-touching command: place and remove a stone on free ground
        // next to a camp's item square.
        let (ox, oz) = camp_origin(pick(5, ROAM_CAMPS));
        let block = if (now / ROAM_SETBLOCK_EVERY) % 2 == 0 {
            "minecraft:stone"
        } else {
            "minecraft:air"
        };
        world.console_command(&format!(
            "setblock {} -60 {} {block}",
            ox + GROUP_ITEM_CORNER.0 - 4,
            oz + GROUP_ITEM_CORNER.1 - 4
        ));
    }
    if now % ROAM_GAMERULE_EVERY == 0 {
        let speed = if (now / ROAM_GAMERULE_EVERY) % 2 == 0 {
            4
        } else {
            3
        };
        world.console_command(&format!("gamerule random_tick_speed {speed}"));
    }
}

/// 岛场景：新进服的假玩家按编号分到 `编号 % K` 组，直接放过去（控制台 `/tp` 要等全部岛停放，
/// 见 `command::place_player`）。他所在的岛这一刻不在手里就下一刻再送。只在有人没送到的时候
/// 查一次在线名单。
///
/// Island scenarios: each new bot goes to group `index % K`, placed directly
/// (a console `/tp` waits for every island to park, see
/// `command::place_player`); retried next tick while their island is away.
fn place_new_bots(world: &mut BenchWorld<'_>, plan: &mut Plan) {
    // `islands-roam` 的人自己飞，不按组送。
    // `islands-roam` bots fly on their own and are not sent to groups.
    if plan.groups == 0 || plan.kind == ScenarioKind::IslandsRoam {
        return;
    }
    for name in world.player_names() {
        if plan.placed.contains(&name) {
            continue;
        }
        let index = bot_index(&name).unwrap_or(plan.placed.len());
        let (ox, oz) = group_origin(plan.kind, index % plan.groups);
        // 同组的人错开几格，免得全挤在一个点上。
        let spread = (index / plan.groups % 5) as f64 * 2.0 - 4.0;
        let x = f64::from(ox + GROUP_PLAYER_SPOT.0) + 0.5 + spread;
        let z = f64::from(oz + GROUP_PLAYER_SPOT.1) + 0.5;
        if world.teleport_player(&name, Vec3::new(x, FLAT_SURFACE_Y, z)) {
            plan.placed.insert(name);
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
    fn island_groups_are_far_apart_and_cover_their_content() {
        let kind = ScenarioKind::Islands8;
        let chunks = group_chunks(kind);
        // 每组一个矩形，组和组的已加载区块之间至少隔 36 个区块（设计稿 6.1 的「一定不合并」）。
        let group_of = |c: &ChunkPos| (c.x + 16).div_euclid(GROUP_STRIDE_CHUNKS);
        for a in &chunks {
            for b in &chunks {
                if group_of(a) != group_of(b) {
                    assert!((a.x - b.x).abs().max((a.z - b.z).abs()) > 36);
                }
            }
        }
        for index in 0..kind.machines() {
            let (ox, oz) = origin_of(kind, index);
            for (dx, dz) in [(0, 0), (MACHINE_EXTENT, MACHINE_EXTENT)] {
                let pos = ChunkPos::new((ox + dx) >> 4, (oz + dz) >> 4);
                assert!(chunks.contains(&pos), "machine {index} {pos:?} missing");
            }
        }
        for group in 0..kind.groups() {
            for (position, _) in group_items(kind, group) {
                let pos = ChunkPos::new(
                    (position.x.floor() as i32) >> 4,
                    (position.z.floor() as i32) >> 4,
                );
                assert!(chunks.contains(&pos), "item {pos:?} missing");
            }
        }
        assert_eq!(kind.bots().0, 80);
        assert_eq!(bot_index("Bot0007"), Some(7));
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
