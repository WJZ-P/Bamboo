//! `TreeDecorator` 的各个子类，各自照官方 26.3 的 `place` 逐行翻译。
//!
//! 装饰器看到的原木、树叶、树根列表，是把对应的 `HashSet` 拷成列表后按 Y 稳定排序
//! 得到的（`TreeDecorator.Context` 的构造器），所以同一高度内的顺序就是 `HashSet` 的
//! 迭代顺序，见 [`super::java_set`]。注意 `trunkSetter` 也记下了树干正下方那块泥土，
//! 它在原木列表里排第一个，官方的 `logs.getFirst()` 取到的就是它。
//!
//! The `TreeDecorator` subclasses, each translated line by line from official 26.3
//! `place`.
//!
//! The log, leaf and root lists a decorator sees are the matching `HashSet`s copied
//! into lists and stable-sorted by Y (the `TreeDecorator.Context` constructor), so the
//! order within one height is the `HashSet` iteration order, see [`super::java_set`].
//! Note that `trunkSetter` also records the dirt block below the trunk; it comes first
//! in the log list and is what official `logs.getFirst()` returns.
//!

use std::{collections::HashSet, sync::OnceLock};

use serde_json::Value;

use mc_bamboo_data::{block_by_name, block_id_of_state, block_tag, facts, state_facts};

use crate::{
    feature::{
        composition::place_placed,
        placement::{PlaceOutcome, PlacementEnv},
        provider::{IntProvider, WorldGenerationContext},
        registry::Registry,
        state::with_property,
        state_provider::StateProvider,
        world::HeightmapKind,
    },
    random::{Random, WorldgenRandom},
};

use super::{HORIZONTAL, Pos, TreePlace, is_air, is_log, random_horizontal};

/// `Direction.Plane.HORIZONTAL` 各方向的名字，和 [`HORIZONTAL`] 同序。
///
/// The names of `Direction.Plane.HORIZONTAL`, in the order of [`HORIZONTAL`].
const HORIZONTAL_NAMES: [&str; 4] = ["north", "east", "south", "west"];

/// `Direction.values()` 的顺序：DOWN、UP、NORTH、SOUTH、WEST、EAST。
///
/// The order of `Direction.values()`: DOWN, UP, NORTH, SOUTH, WEST, EAST.
const ALL_DIRECTIONS: [Pos; 6] = [
    (0, -1, 0),
    (0, 1, 0),
    (0, 0, -1),
    (0, 0, 1),
    (-1, 0, 0),
    (1, 0, 0),
];

/// 一个已解析的装饰器。要放的方块状态都在解析时算好，属性名对不上就解析失败。
///
/// A parsed decorator. Every state it places is built at parse time, so a wrong
/// property name fails the parse instead of placing a wrong block.
pub enum Decorator {
    TrunkVine {
        /// 下标和 [`HORIZONTAL`] 一致：藤蔓贴向这个方向时的状态。
        ///
        /// Indexed like [`HORIZONTAL`]: the vine state attached towards that direction.
        vines: [u32; 4],
    },
    LeaveVine {
        probability: f32,
        vines: [u32; 4],
    },
    Cocoa {
        probability: f32,
        /// `[facing][age]`。
        states: [[u32; 3]; 4],
    },
    Beehive {
        probability: f32,
        nest: u32,
    },
    AlterGround {
        provider: GroundProvider,
    },
    AttachedToLeaves {
        probability: f32,
        exclusion_radius_xz: i32,
        exclusion_radius_y: i32,
        provider: StateProvider,
        required_empty_blocks: i32,
        directions: Vec<Pos>,
    },
    AttachedToLogs {
        probability: f32,
        provider: StateProvider,
        directions: Vec<Pos>,
    },
    PlaceOnGround {
        tries: i32,
        radius: i32,
        height: i32,
        provider: StateProvider,
    },
    ShelfMushroom {
        probability: f32,
        /// `[facing][age]`。
        states: [[u32; 2]; 4],
        mushroom: usize,
    },
    CreakingHeart {
        probability: f32,
        heart: u32,
    },
    PaleMoss {
        leaves_probability: f32,
        trunk_probability: f32,
        ground_probability: f32,
    },
}

/// `alter_ground` 的 provider。数据里只有注册表引用 `minecraft:podzol_beneath_tree`：
/// 一条规则的 `RuleBasedStateProvider`，方块在 `beneath_tree_podzol_replaceable` 标签里
/// 就给灰化土，否则不给。
///
/// The `alter_ground` provider. The data only uses the registry reference
/// `minecraft:podzol_beneath_tree`: a one-rule `RuleBasedStateProvider` giving podzol
/// when the block is in the `beneath_tree_podzol_replaceable` tag and nothing otherwise.
pub enum GroundProvider {
    PodzolBeneathTree { podzol: u32 },
}

impl Decorator {
    pub fn parse(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let float = |name: &str| object.get(name).and_then(Value::as_f64).map(|n| n as f32);
        let int = |name: &str, default: i32| {
            object
                .get(name)
                .and_then(Value::as_i64)
                .map_or(default, |n| n as i32)
        };
        Some(match object.get("type")?.as_str()? {
            "minecraft:trunk_vine" => Self::TrunkVine {
                vines: vine_states()?,
            },
            "minecraft:leave_vine" => Self::LeaveVine {
                probability: float("probability")?,
                vines: vine_states()?,
            },
            "minecraft:cocoa" => Self::Cocoa {
                probability: float("probability")?,
                states: facing_age_states("minecraft:cocoa")?,
            },
            "minecraft:beehive" => Self::Beehive {
                probability: float("probability")?,
                nest: with_property(default_state("minecraft:bee_nest")?, "facing", "south")?,
            },
            "minecraft:alter_ground" => Self::AlterGround {
                provider: match object.get("provider")?.as_str()? {
                    "minecraft:podzol_beneath_tree" => GroundProvider::PodzolBeneathTree {
                        podzol: default_state("minecraft:podzol")?,
                    },
                    _ => return None,
                },
            },
            "minecraft:attached_to_leaves" => Self::AttachedToLeaves {
                probability: float("probability")?,
                exclusion_radius_xz: int("exclusion_radius_xz", 0),
                exclusion_radius_y: int("exclusion_radius_y", 0),
                provider: block_provider(object.get("block_provider")?)?,
                required_empty_blocks: int("required_empty_blocks", 1),
                directions: object
                    .get("directions")?
                    .as_array()?
                    .iter()
                    .map(|name| direction_step(name.as_str()?))
                    .collect::<Option<Vec<_>>>()?,
            },
            "minecraft:attached_to_logs" => Self::AttachedToLogs {
                probability: float("probability")?,
                provider: block_provider(object.get("block_provider")?)?,
                directions: object
                    .get("directions")?
                    .as_array()?
                    .iter()
                    .map(|name| direction_step(name.as_str()?))
                    .collect::<Option<Vec<_>>>()?,
            },
            "minecraft:place_on_ground" => Self::PlaceOnGround {
                tries: int("tries", 128),
                radius: int("radius", 2),
                height: int("height", 1),
                provider: StateProvider::parse(object.get("block_state_provider")?)?,
            },
            "minecraft:shelf_mushroom" => Self::ShelfMushroom {
                probability: float("probability")?,
                states: facing_age_states("minecraft:shelf_mushroom")?,
                mushroom: block_id_of_state(default_state("minecraft:shelf_mushroom")?)?,
            },
            "minecraft:pale_moss" => Self::PaleMoss {
                leaves_probability: float("leaves_probability")?,
                trunk_probability: float("trunk_probability")?,
                ground_probability: float("ground_probability")?,
            },
            "minecraft:creaking_heart" => {
                let heart = default_state("minecraft:creaking_heart")?;
                let heart = with_property(heart, "creaking_heart_state", "dormant")?;
                Self::CreakingHeart {
                    probability: float("probability")?,
                    heart: with_property(heart, "natural", "true")?,
                }
            }
            _ => return None,
        })
    }

    /// 官方 `TreeDecorator.place(context)`。
    ///
    /// Official `TreeDecorator.place(context)`.
    pub(crate) fn place<'r, 'a, 'w>(
        &self,
        place: &'r mut TreePlace<'a, 'w>,
        random: &'r WorldgenRandom,
        biome_at: &'r mut dyn FnMut(i32, i32, i32) -> u32,
        recursion: Option<&'r Recursion<'r>>,
    ) -> PlaceOutcome {
        let mut context = Context::new(place, random, biome_at, recursion);
        match self {
            Self::TrunkVine { vines } => context.trunk_vine(vines),
            Self::LeaveVine { probability, vines } => context.leave_vine(*probability, vines),
            Self::Cocoa {
                probability,
                states,
            } => context.cocoa(*probability, states),
            Self::Beehive { probability, nest } => context.beehive(*probability, *nest),
            Self::AlterGround { provider } => context.alter_ground(provider),
            Self::AttachedToLeaves {
                probability,
                exclusion_radius_xz,
                exclusion_radius_y,
                provider,
                required_empty_blocks,
                directions,
            } => context.attached_to_leaves(
                *probability,
                (*exclusion_radius_xz, *exclusion_radius_y),
                provider,
                *required_empty_blocks,
                directions,
            ),
            Self::AttachedToLogs {
                probability,
                provider,
                directions,
            } => context.attached_to_logs(*probability, provider, directions),
            Self::PlaceOnGround {
                tries,
                radius,
                height,
                provider,
            } => context.place_on_ground(*tries, *radius, *height, provider),
            Self::ShelfMushroom {
                probability,
                states,
                mushroom,
            } => context.shelf_mushroom(*probability, states, *mushroom),
            Self::CreakingHeart { probability, heart } => {
                context.creaking_heart(*probability, *heart);
            }
            Self::PaleMoss {
                leaves_probability,
                trunk_probability,
                ground_probability,
            } => context.pale_moss(*leaves_probability, *trunk_probability, *ground_probability),
        }
        PlaceOutcome {
            supported: context.supported,
            placed: true,
        }
    }
}

/// `pale_moss` 装饰器递归放 `minecraft:pale_moss_patch` 时需要的只读上下文。
/// 注册表、世界随机源和高度参数来自主循环，群系回调单独以可变借用传入；
/// 缺了这份上下文就把整棵树标 unsupported，避免静默跳过的假绿。
///
/// Read-only context for the `pale_moss` decorator's recursive
/// `minecraft:pale_moss_patch` placement. The registry, region random and height
/// parameters come from the main loop; the biome callback is passed as a separate
/// mutable borrow. Without it the tree is marked unsupported rather than silently
/// skipping the recursion.
pub struct Recursion<'a> {
    /// `FEATURE` / `PLACED_FEATURE` 注册表。
    /// The `FEATURE` / `PLACED_FEATURE` registry.
    pub registry: &'a Registry,
    /// `WorldGenRegion.getRandom()`。
    /// `WorldGenRegion.getRandom()`.
    pub region_random: &'a Random,
    /// `WorldGenerationContext` 的最小高度、高度和海平面。
    /// The `WorldGenerationContext` min Y, height and sea level.
    pub generation: WorldGenerationContext,
}

fn default_state(name: &str) -> Option<u32> {
    Some(block_by_name(name)?.default_state)
}

/// 四个方向各一个 `VINE` 状态：`Blocks.VINE.defaultBlockState().setValue(dir, true)`。
///
/// One `VINE` state per direction: `Blocks.VINE.defaultBlockState().setValue(dir, true)`.
fn vine_states() -> Option<[u32; 4]> {
    let vine = default_state("minecraft:vine")?;
    let mut states = [0; 4];
    for (state, name) in states.iter_mut().zip(HORIZONTAL_NAMES) {
        *state = with_property(vine, name, "true")?;
    }
    Some(states)
}

/// `[facing][age]` 的状态表，`age` 从 0 到 `N - 1`。
///
/// A `[facing][age]` state table with `age` from 0 to `N - 1`.
fn facing_age_states<const N: usize>(block: &str) -> Option<[[u32; N]; 4]> {
    let base = default_state(block)?;
    let mut states = [[0; N]; 4];
    for (row, facing) in states.iter_mut().zip(HORIZONTAL_NAMES) {
        let facing = with_property(base, "facing", facing)?;
        for (age, state) in row.iter_mut().enumerate() {
            *state = with_property(facing, "age", &age.to_string())?;
        }
    }
    Some(states)
}

/// `attached_to_leaves` 的 `block_provider`。数据里是注册表引用
/// `minecraft:mangrove_propagule`：`RandomizedIntStateProvider`，source 是
/// `hanging=true` 的红树胎生苗，`age` 取 `UniformInt(0, 4)`。
///
/// The `block_provider` of `attached_to_leaves`. The data uses the registry reference
/// `minecraft:mangrove_propagule`: a `RandomizedIntStateProvider` over a
/// `hanging=true` propagule with `age` drawn from `UniformInt(0, 4)`.
fn block_provider(value: &Value) -> Option<StateProvider> {
    match value {
        Value::String(name) if name == "minecraft:mangrove_propagule" => {
            let propagule = default_state("minecraft:mangrove_propagule")?;
            Some(StateProvider::RandomizedInt {
                source: Box::new(StateProvider::Simple(with_property(
                    propagule, "hanging", "true",
                )?)),
                property: "age".to_owned(),
                values: IntProvider::Uniform { min: 0, max: 4 },
            })
        }
        Value::String(_) => None,
        _ => StateProvider::parse(value),
    }
}

fn direction_step(name: &str) -> Option<Pos> {
    Some(match name {
        "down" => (0, -1, 0),
        "up" => (0, 1, 0),
        "north" => (0, 0, -1),
        "south" => (0, 0, 1),
        "west" => (-1, 0, 0),
        "east" => (1, 0, 0),
        _ => return None,
    })
}

fn tagged(cell: &'static OnceLock<&'static [i32]>, name: &str, state: u32) -> bool {
    let ids = cell.get_or_init(|| block_tag(name).unwrap_or(&[]));
    block_id_of_state(state).is_some_and(|id| ids.contains(&(id as i32)))
}

/// `Util.shuffle`：从后往前，每步 `nextInt(i)` 后交换。
///
/// `Util.shuffle`: walks backwards, swapping after each `nextInt(i)`.
fn shuffle(list: &mut [Pos], random: &WorldgenRandom) {
    for i in (2..=list.len()).rev() {
        let swap_to = random.next_int(i as i32) as usize;
        list.swap(i - 1, swap_to);
    }
}

fn offset(pos: Pos, step: Pos) -> Pos {
    (pos.0 + step.0, pos.1 + step.1, pos.2 + step.2)
}

fn horizontal(pos: Pos, direction: usize) -> Pos {
    (
        pos.0 + HORIZONTAL[direction].0,
        pos.1,
        pos.2 + HORIZONTAL[direction].1,
    )
}

/// 官方 `TreeDecorator.Context`。
///
/// Official `TreeDecorator.Context`.
struct Context<'r, 'a, 'w> {
    place: &'r mut TreePlace<'a, 'w>,
    random: &'r WorldgenRandom,
    biome_at: &'r mut dyn FnMut(i32, i32, i32) -> u32,
    recursion: Option<&'r Recursion<'r>>,
    supported: bool,
    logs: Vec<Pos>,
    leaves: Vec<Pos>,
    roots: Vec<Pos>,
}

impl<'r, 'a, 'w> Context<'r, 'a, 'w> {
    fn new(
        place: &'r mut TreePlace<'a, 'w>,
        random: &'r WorldgenRandom,
        biome_at: &'r mut dyn FnMut(i32, i32, i32) -> u32,
        recursion: Option<&'r Recursion<'r>>,
    ) -> Self {
        // `sort_by_key` 是稳定排序，和 fastutil `ObjectArrayList.sort` 一样。
        // `sort_by_key` is stable, like fastutil's `ObjectArrayList.sort`.
        let sorted = |set: &super::java_set::JavaHashSet| {
            let mut list: Vec<Pos> = set.iter().collect();
            list.sort_by_key(|pos| pos.1);
            list
        };
        Self {
            logs: sorted(&place.trunks),
            leaves: sorted(&place.foliage),
            roots: sorted(&place.roots),
            place,
            random,
            biome_at,
            recursion,
            supported: true,
        }
    }

    fn block(&self, pos: Pos) -> u32 {
        self.place.get_block(pos)
    }

    fn is_air(&self, pos: Pos) -> bool {
        is_air(self.block(pos))
    }

    /// `Context.setBlock`：走 `decorationSetter`。
    fn set(&mut self, pos: Pos, state: u32) {
        self.place.decorations.insert(pos);
        self.place.world.set_tree_block(pos.0, pos.1, pos.2, state);
    }

    /// `TrunkVineDecorator.place`。
    fn trunk_vine(&mut self, vines: &[u32; 4]) {
        for pos in self.logs.clone() {
            // 官方的顺序：西、东、北、南；藤蔓贴向原木那一面。
            // Official order: west, east, north, south; the vine faces the log.
            for (side, facing) in [(3, 1), (1, 3), (0, 2), (2, 0)] {
                if self.random.next_int(3) > 0 {
                    let at = horizontal(pos, side);
                    if self.is_air(at) {
                        self.set(at, vines[facing]);
                    }
                }
            }
        }
    }

    /// `LeaveVineDecorator.place`。
    fn leave_vine(&mut self, probability: f32, vines: &[u32; 4]) {
        for pos in self.leaves.clone() {
            for (side, facing) in [(3, 1), (1, 3), (0, 2), (2, 0)] {
                if self.random.next_float() < probability {
                    let at = horizontal(pos, side);
                    if self.is_air(at) {
                        self.hanging_vine(at, vines[facing]);
                    }
                }
            }
        }
    }

    /// `LeaveVineDecorator.addHangingVine`：往下最多再挂 4 格。
    ///
    /// `LeaveVineDecorator.addHangingVine`: hangs at most four more blocks down.
    fn hanging_vine(&mut self, pos: Pos, vine: u32) {
        self.set(pos, vine);
        let mut below = (pos.0, pos.1 - 1, pos.2);
        let mut remaining = 4;
        while self.is_air(below) && remaining > 0 {
            self.set(below, vine);
            below.1 -= 1;
            remaining -= 1;
        }
    }

    /// `CocoaDecorator.place`。
    fn cocoa(&mut self, probability: f32, states: &[[u32; 3]; 4]) {
        if self.random.next_float() >= probability {
            return;
        }
        let Some(&first) = self.logs.first() else {
            return;
        };
        let tree_y = first.1;
        for pos in self.logs.clone() {
            if pos.1 - tree_y > 2 {
                continue;
            }
            for (direction, ages) in states.iter().enumerate() {
                if self.random.next_float() <= 0.25 {
                    let cocoa_pos = horizontal(pos, (direction + 2) % 4);
                    if self.is_air(cocoa_pos) {
                        let age = self.random.next_int(3) as usize;
                        self.set(cocoa_pos, ages[age]);
                    }
                }
            }
        }
    }

    /// `BeehiveDecorator.place`。
    fn beehive(&mut self, probability: f32, nest: u32) {
        let (Some(&first_log), Some(&last_log)) = (self.logs.first(), self.logs.last()) else {
            return;
        };
        if self.random.next_float() >= probability {
            return;
        }
        let hive_y = match self.leaves.first() {
            Some(leaf) => (leaf.1 - 1).max(first_log.1 + 1),
            None => (first_log.1 + 1 + self.random.next_int(3)).min(last_log.1),
        };
        // `SPAWN_DIRECTIONS`：水平方向里去掉 NORTH，剩 EAST、SOUTH、WEST。
        // `SPAWN_DIRECTIONS`: the horizontal directions without NORTH: EAST, SOUTH, WEST.
        let mut placements: Vec<Pos> = self
            .logs
            .iter()
            .filter(|pos| pos.1 == hive_y)
            .flat_map(|&pos| [1, 2, 3].map(|direction| horizontal(pos, direction)))
            .collect();
        if placements.is_empty() {
            return;
        }
        shuffle(&mut placements, self.random);
        let hive = placements
            .into_iter()
            .find(|&pos| self.is_air(pos) && self.is_air(horizontal(pos, 2)));
        if let Some(hive) = hive {
            self.set(hive, nest);
            // 方块实体里存蜜蜂：只比方块，但随机数要照取。
            // Bees go into the block entity: only blocks are compared, but the random
            // draws still have to happen.
            let bees = 2 + self.random.next_int(2);
            for _ in 0..bees {
                self.random.next_int(599);
            }
        }
    }

    /// `TreeFeature.getLowestTrunkOrRootOfTree`。
    fn lowest_trunk_or_root(&self) -> Vec<Pos> {
        if self.roots.is_empty() {
            return self.logs.clone();
        }
        match (self.roots.first(), self.logs.first()) {
            (Some(root), Some(log)) if root.1 == log.1 => {
                let mut all = self.logs.clone();
                all.extend(&self.roots);
                all
            }
            _ => self.roots.clone(),
        }
    }

    /// `AlterGroundDecorator.place`。
    fn alter_ground(&mut self, provider: &GroundProvider) {
        let positions = self.lowest_trunk_or_root();
        let Some(&first) = positions.first() else {
            return;
        };
        for pos in positions.into_iter().filter(|pos| pos.1 == first.1) {
            for (dx, dz) in [(-1, -1), (2, -1), (-1, 2), (2, 2)] {
                self.ground_circle(provider, (pos.0 + dx, pos.1, pos.2 + dz));
            }
            for _ in 0..5 {
                let placement = self.random.next_int(64);
                let (xx, zz) = (placement % 8, placement / 8);
                if xx == 0 || xx == 7 || zz == 0 || zz == 7 {
                    self.ground_circle(provider, (pos.0 - 3 + xx, pos.1, pos.2 - 3 + zz));
                }
            }
        }
    }

    /// `AlterGroundDecorator.placeCircle`。
    fn ground_circle(&mut self, provider: &GroundProvider, pos: Pos) {
        for xx in -2_i32..=2 {
            for zz in -2_i32..=2 {
                if xx.abs() != 2 || zz.abs() != 2 {
                    self.ground_block(provider, (pos.0 + xx, pos.1, pos.2 + zz));
                }
            }
        }
    }

    /// `AlterGroundDecorator.placeBlockAt`。
    fn ground_block(&mut self, provider: &GroundProvider, pos: Pos) {
        static REPLACEABLE: OnceLock<&'static [i32]> = OnceLock::new();
        for dy in (-3..=2).rev() {
            let cursor = (pos.0, pos.1 + dy, pos.2);
            let GroundProvider::PodzolBeneathTree { podzol } = provider;
            let replaceable = tagged(
                &REPLACEABLE,
                "minecraft:beneath_tree_podzol_replaceable",
                self.block(cursor),
            );
            if replaceable {
                self.set(cursor, *podzol);
                break;
            }
            if !self.is_air(cursor) && dy < 0 {
                break;
            }
        }
    }

    /// `AttachedToLeavesDecorator.place`。
    fn attached_to_leaves(
        &mut self,
        probability: f32,
        (radius_xz, radius_y): (i32, i32),
        provider: &StateProvider,
        required_empty_blocks: i32,
        directions: &[Pos],
    ) {
        let mut blacklist: HashSet<Pos> = HashSet::new();
        let mut leaves = self.leaves.clone();
        shuffle(&mut leaves, self.random);
        for leaf in leaves {
            let direction = directions[self.random.next_int(directions.len() as i32) as usize];
            let placement = offset(leaf, direction);
            if blacklist.contains(&placement) || self.random.next_float() >= probability {
                continue;
            }
            let empty = (1..=required_empty_blocks).all(|i| {
                self.is_air((
                    leaf.0 + direction.0 * i,
                    leaf.1 + direction.1 * i,
                    leaf.2 + direction.2 * i,
                ))
            });
            if !empty {
                continue;
            }
            for x in -radius_xz..=radius_xz {
                for y in -radius_y..=radius_y {
                    for z in -radius_xz..=radius_xz {
                        blacklist.insert((placement.0 + x, placement.1 + y, placement.2 + z));
                    }
                }
            }
            if let Some(state) = provider.get_state(self.random) {
                self.set(placement, state);
            }
        }
    }

    /// `AttachedToLogsDecorator.place`。
    fn attached_to_logs(&mut self, probability: f32, provider: &StateProvider, directions: &[Pos]) {
        let mut logs = self.logs.clone();
        shuffle(&mut logs, self.random);
        for log in logs {
            let direction = directions[self.random.next_int(directions.len() as i32) as usize];
            let placement = offset(log, direction);
            if self.random.next_float() <= probability
                && self.is_air(placement)
                && let Some(state) = provider.get_state(self.random)
            {
                self.set(placement, state);
            }
        }
    }

    /// `PlaceOnGroundDecorator.place`。
    fn place_on_ground(&mut self, tries: i32, radius: i32, height: i32, provider: &StateProvider) {
        let positions = self.lowest_trunk_or_root();
        let Some(&origin) = positions.first() else {
            return;
        };
        let min_y = origin.1;
        let (mut min_x, mut max_x, mut min_z, mut max_z) = (origin.0, origin.0, origin.2, origin.2);
        for pos in positions.iter().filter(|pos| pos.1 == min_y) {
            min_x = min_x.min(pos.0);
            max_x = max_x.max(pos.0);
            min_z = min_z.min(pos.2);
            max_z = max_z.max(pos.2);
        }
        for _ in 0..tries {
            let x = self
                .random
                .next_int_between_inclusive(min_x - radius, max_x + radius);
            let y = self
                .random
                .next_int_between_inclusive(min_y - height, min_y + height);
            let z = self
                .random
                .next_int_between_inclusive(min_z - radius, max_z + radius);
            self.place_above((x, y, z), provider);
        }
    }

    /// `PlaceOnGroundDecorator.attemptToPlaceBlockAbove`。
    fn place_above(&mut self, pos: Pos, provider: &StateProvider) {
        static VINE: OnceLock<Option<usize>> = OnceLock::new();
        let vine = *VINE.get_or_init(|| {
            block_by_name("minecraft:vine").and_then(|b| block_id_of_state(b.default_state))
        });
        let above = (pos.0, pos.1 + 1, pos.2);
        let above_state = self.block(above);
        let open =
            is_air(above_state) || (vine.is_some() && block_id_of_state(above_state) == vine);
        if !open || state_facts(self.block(pos)) & facts::SOLID_RENDER == 0 {
            return;
        }
        let surface =
            self.place
                .world
                .get_height(HeightmapKind::MotionBlockingNoLeaves, pos.0, pos.2);
        if surface > above.1 {
            return;
        }
        if let Some(state) = provider.get_state(self.random) {
            self.set(above, state);
        }
    }

    /// `ShelfMushroomDecorator.place`。
    fn shelf_mushroom(&mut self, probability: f32, states: &[[u32; 2]; 4], mushroom: usize) {
        if self.random.next_float() >= probability {
            return;
        }
        let (Some(&first), Some(&last)) = (self.logs.first(), self.logs.last()) else {
            return;
        };
        let logs = self.logs.clone();
        if first.1 == last.1 {
            // `placeOnFallenLog`：倒木，朝向垂直于原木的走向。
            // `placeOnFallenLog`: a fallen log; facings are perpendicular to its run.
            let directions = if first.0 != last.0 { [0, 2] } else { [1, 3] };
            for log in logs {
                for facing in directions {
                    if self.random.next_float() <= 0.25 {
                        let at = horizontal(log, facing);
                        if self.mushroom_can_replace(at)
                            && !self.mushroom_beside(at, mushroom)
                            && !self.mushroom_beside(log, mushroom)
                        {
                            self.put_mushroom(at, facing, states);
                        }
                    }
                }
            }
            return;
        }
        // `placeOnStandingTree`：随机一个方向和它顺时针的下一个。
        // `placeOnStandingTree`: a random direction and its clockwise neighbour.
        let first_direction = random_horizontal(self.random);
        let directions = [first_direction, (first_direction + 1) % 4];
        for log in logs {
            let dy = log.1 - first.1;
            if !(1..=4).contains(&dy) {
                continue;
            }
            for facing in directions {
                if self.random.next_float() > 0.25 {
                    continue;
                }
                let at = horizontal(log, facing);
                let below = (at.0, at.1 - 1, at.2);
                if self.mushroom_can_replace(at) && !self.is_block(below, mushroom) {
                    self.put_mushroom(at, facing, states);
                    break;
                }
            }
        }
    }

    fn put_mushroom(&mut self, pos: Pos, facing: usize, states: &[[u32; 2]; 4]) {
        let age = self.random.next_int(2) as usize;
        self.set(pos, states[facing][age]);
    }

    fn is_block(&self, pos: Pos, block: usize) -> bool {
        block_id_of_state(self.block(pos)) == Some(block)
    }

    /// `hasHorizontallyAdjacentShelfMushroom`。
    fn mushroom_beside(&self, pos: Pos, mushroom: usize) -> bool {
        (0..4).any(|direction| self.is_block(horizontal(pos, direction), mushroom))
    }

    /// `isBlockReplaceableWithShelfMushroom`：可替换，且自己和四周都不是水方块。
    ///
    /// `isBlockReplaceableWithShelfMushroom`: replaceable, with no water block at the
    /// position or beside it.
    fn mushroom_can_replace(&self, pos: Pos) -> bool {
        static WATER: OnceLock<Option<usize>> = OnceLock::new();
        let water = *WATER.get_or_init(|| {
            block_by_name("minecraft:water").and_then(|b| block_id_of_state(b.default_state))
        });
        let Some(water) = water else {
            return false;
        };
        state_facts(self.block(pos)) & facts::REPLACEABLE != 0
            && !self.is_block(pos, water)
            && !(0..4).any(|direction| self.is_block(horizontal(pos, direction), water))
    }

    /// `CreakingHeartDecorator.place`。
    fn creaking_heart(&mut self, probability: f32, heart: u32) {
        if self.logs.is_empty() || self.random.next_float() >= probability {
            return;
        }
        let mut placements = self.logs.clone();
        shuffle(&mut placements, self.random);
        let target = placements.into_iter().find(|&pos| {
            ALL_DIRECTIONS
                .iter()
                .all(|&step| is_log(self.block(offset(pos, step))))
        });
        if let Some(target) = target {
            self.set(target, heart);
        }
    }

    /// `PaleMossDecorator.place`：洗牌原木列表、取最低的一根，按三个概率分别
    /// 放 `pale_moss_patch`、给树叶和树干挂苍白悬挂苔藓。
    ///
    /// `PaleMossDecorator.place`: shuffle the logs, take the lowest one, then
    /// place `pale_moss_patch` and hang pale moss below leaves and logs with the
    /// three configured probabilities.
    fn pale_moss(
        &mut self,
        leaves_probability: f32,
        trunk_probability: f32,
        ground_probability: f32,
    ) {
        let mut logs = self.logs.clone();
        shuffle(&mut logs, self.random);
        // `Collections.min` 和 Rust 的 `min_by_key` 都保留迭代顺序里第一个
        // 最小值；洗牌后同高度原木的先后会影响这根。
        // Both `Collections.min` and Rust's `min_by_key` keep the first minimum
        // in iteration order; the shuffled order among equal heights matters.
        let Some(&lowest) = logs.iter().min_by_key(|pos| pos.1) else {
            return;
        };
        if self.random.next_float() < ground_probability {
            let Some(context) = self.recursion else {
                self.supported = false;
                return;
            };
            // 官方 `lambda$place$1` 直接调 `Feature.place`（不是 PlacedFeature.place）：
            // 注册表里的 configured feature 没有修饰器链，起点是最低原木的上一格。
            // Official `lambda$place$1` calls `Feature.place` directly, not
            // `PlacedFeature.place`; the configured feature has no placement
            // modifiers and starts one block above the lowest log.
            let feature_name = serde_json::Value::String("minecraft:pale_moss_patch".to_owned());
            let mut env = PlacementEnv {
                generation: context.generation,
                biome_at: self.biome_at,
                registry: context.registry,
                top_feature: None,
                region_random: context.region_random,
            };
            let outcome = place_placed(
                context.registry,
                &feature_name,
                &[],
                self.place.world,
                &mut env,
                self.random,
                (lowest.0, lowest.1 + 1, lowest.2),
            );
            self.supported &= outcome.supported;
        }
        let hanging = hanging_moss_states();
        let Some((body, tip)) = hanging else {
            self.supported = false;
            return;
        };
        for pos in self.logs.clone() {
            if self.random.next_float() < trunk_probability {
                let below = (pos.0, pos.1 - 1, pos.2);
                if self.is_air(below) {
                    self.add_moss_hanger(below, body, tip);
                }
            }
        }
        for pos in self.leaves.clone() {
            if self.random.next_float() < leaves_probability {
                let below = (pos.0, pos.1 - 1, pos.2);
                if self.is_air(below) {
                    self.add_moss_hanger(below, body, tip);
                }
            }
        }
    }

    /// `PaleMossDecorator.addMossHanger`：检查的是当前格的**下面一格**是不是空气，
    /// 是则抽 `nextFloat()`；`< 0.5` 就停，否则在当前格写 `tip=false` 并下移，
    /// 循环结束后在当前格写 `tip=true`。
    ///
    /// `PaleMossDecorator.addMossHanger`: test the cell **below** the current one
    /// for air, draw `nextFloat()` and stop when `< 0.5`; otherwise write
    /// `tip=false` at the current cell and move down. The final cell gets `tip=true`.
    fn add_moss_hanger(&mut self, start: Pos, body: u32, tip: u32) {
        let mut pos = start;
        while self.is_air((pos.0, pos.1 - 1, pos.2)) {
            if self.random.next_float() < 0.5 {
                break;
            }
            self.set(pos, body);
            pos = (pos.0, pos.1 - 1, pos.2);
        }
        self.set(pos, tip);
    }
}

/// `pale_hanging_moss` 的两个状态：TIP=false 是挂着的苔藓段，TIP=true 是末端。
///
/// The two `pale_hanging_moss` states: TIP=false is a body segment, TIP=true the tip.
fn hanging_moss_states() -> Option<(u32, u32)> {
    let base = default_state("minecraft:pale_hanging_moss")?;
    Some((
        with_property(base, "tip", "false")?,
        with_property(base, "tip", "true")?,
    ))
}
