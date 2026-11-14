//! 十种 `TrunkPlacer`，各自照官方 26.3 的 `placeTrunk` 逐行翻译。
//!
//! 每个函数里随机数的取数次数和顺序都和官方一致，包括那些结果没用上的取数。
//!
//! The ten `TrunkPlacer`s, each translated line by line from official 26.3 `placeTrunk`.
//! Every function draws random numbers the same number of times and in the same order
//! as the official code, including draws whose result goes unused.

use serde_json::Value;

use mc_bamboo_data::{block_by_name, block_id_of_state, block_tag};

use crate::{
    carver::{mth_cos, mth_sin},
    feature::{provider::IntProvider, state::with_property, world::FeatureWorld},
    random::WorldgenRandom,
};

use super::{
    FoliageAttachment, HORIZONTAL, Pos, TreeConfig, TreePlace, axis_of, floor_f32, floor_f64,
    is_air_or_leaves, is_log, random_horizontal, valid_tree_pos,
};

/// `TrunkPlacer` 的公共字段加上各子类自己的参数。
///
/// The shared `TrunkPlacer` fields plus each subclass's own parameters.
pub struct TrunkPlacer {
    base_height: i32,
    height_rand_a: i32,
    height_rand_b: i32,
    kind: Kind,
}

enum Kind {
    Straight,
    Forking,
    Giant,
    MegaJungle,
    DarkOak,
    Fancy,
    Bending {
        min_height_for_leaves: i32,
        bend_length: IntProvider,
    },
    UpwardsBranching {
        extra_branch_steps: IntProvider,
        place_branch_per_log_probability: f32,
        extra_branch_length: IntProvider,
        /// `can_grow_through` 里的方块 ID。
        ///
        /// The block IDs in `can_grow_through`.
        can_grow_through: Vec<i32>,
    },
    Cherry {
        branch_count: IntProvider,
        branch_horizontal_length: IntProvider,
        /// `branch_start_offset_from_top` 的 `(min, max)`，官方要求是 `UniformInt`。
        ///
        /// `(min, max)` of `branch_start_offset_from_top`, which must be a `UniformInt`.
        branch_start: (i32, i32),
        branch_end_offset_from_top: IntProvider,
    },
    Poplar {
        trunk_height_above_branches: IntProvider,
        branch_amount: IntProvider,
    },
}

/// 原木状态的修改：不改，或者 `trySetValue(AXIS, axis)`。
///
/// How a log state is modified: unchanged, or `trySetValue(AXIS, axis)`.
#[derive(Clone, Copy)]
enum LogAxis {
    Keep,
    Set(&'static str),
}

impl TrunkPlacer {
    pub fn parse(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let int = |name: &str| object.get(name).and_then(Value::as_i64).map(|n| n as i32);
        let provider = |name: &str| IntProvider::parse(object.get(name)?).ok();
        let kind = match object.get("type")?.as_str()? {
            "minecraft:straight_trunk_placer" => Kind::Straight,
            "minecraft:forking_trunk_placer" => Kind::Forking,
            "minecraft:giant_trunk_placer" => Kind::Giant,
            "minecraft:mega_jungle_trunk_placer" => Kind::MegaJungle,
            "minecraft:dark_oak_trunk_placer" => Kind::DarkOak,
            "minecraft:fancy_trunk_placer" => Kind::Fancy,
            "minecraft:bending_trunk_placer" => Kind::Bending {
                min_height_for_leaves: int("min_height_for_leaves").unwrap_or(1),
                bend_length: provider("bend_length")?,
            },
            "minecraft:upwards_branching_trunk_placer" => Kind::UpwardsBranching {
                extra_branch_steps: provider("extra_branch_steps")?,
                place_branch_per_log_probability: object
                    .get("place_branch_per_log_probability")?
                    .as_f64()? as f32,
                extra_branch_length: provider("extra_branch_length")?,
                can_grow_through: holder_set(object.get("can_grow_through")?)?,
            },
            "minecraft:cherry_trunk_placer" => Kind::Cherry {
                branch_count: provider("branch_count")?,
                branch_horizontal_length: provider("branch_horizontal_length")?,
                branch_start: match provider("branch_start_offset_from_top")? {
                    IntProvider::Uniform { min, max } => (min, max),
                    _ => return None,
                },
                branch_end_offset_from_top: provider("branch_end_offset_from_top")?,
            },
            "minecraft:poplar_trunk_placer" => Kind::Poplar {
                trunk_height_above_branches: provider("trunk_height_above_branches")?,
                branch_amount: provider("branch_amount")?,
            },
            _ => return None,
        };
        Some(Self {
            base_height: int("base_height")?,
            height_rand_a: int("height_rand_a")?,
            height_rand_b: int("height_rand_b")?,
            kind,
        })
    }

    /// `TrunkPlacer.getTreeHeight`：两次 `nextInt` 都要取，包括 `nextInt(1)`。
    ///
    /// Official `TrunkPlacer.getTreeHeight`: both `nextInt` calls happen, `nextInt(1)` too.
    pub fn tree_height(&self, random: &WorldgenRandom) -> i32 {
        self.base_height
            + random.next_int(self.height_rand_a + 1)
            + random.next_int(self.height_rand_b + 1)
    }

    /// `TrunkPlacer.validTreePos`；`UpwardsBranchingTrunkPlacer` 多放行
    /// `can_grow_through` 里的方块。
    ///
    /// `TrunkPlacer.validTreePos`; `UpwardsBranchingTrunkPlacer` also accepts the
    /// blocks in `can_grow_through`.
    fn valid_tree_pos(&self, world: &FeatureWorld<'_>, pos: Pos) -> bool {
        if valid_tree_pos(world, pos) {
            return true;
        }
        match &self.kind {
            Kind::UpwardsBranching {
                can_grow_through, ..
            } => block_id_of_state(world.get_block(pos.0, pos.1, pos.2))
                .is_some_and(|id| can_grow_through.contains(&(id as i32))),
            _ => false,
        }
    }

    /// `TrunkPlacer.isFree`：`validTreePos` 或 `LOGS`。
    ///
    /// Official `TrunkPlacer.isFree`: `validTreePos` or `LOGS`.
    pub(crate) fn is_free(&self, world: &FeatureWorld<'_>, pos: Pos) -> bool {
        self.valid_tree_pos(world, pos) || is_log(world.get_block(pos.0, pos.1, pos.2))
    }

    /// `TrunkPlacer.placeLog`。先判位置，再向 trunk provider 要状态。
    ///
    /// Official `TrunkPlacer.placeLog`: test the position, then ask the trunk provider.
    fn place_log(
        &self,
        place: &mut TreePlace<'_, '_>,
        config: &TreeConfig,
        random: &WorldgenRandom,
        pos: Pos,
        axis: LogAxis,
    ) -> bool {
        if !self.valid_tree_pos(place.world, pos) {
            return false;
        }
        let Some(mut state) = config.trunk_provider.get_state_without_position(random) else {
            return false;
        };
        if let LogAxis::Set(axis) = axis {
            state = with_property(state, "axis", axis).unwrap_or(state);
        }
        place.set_trunk(pos, state);
        true
    }

    /// `TrunkPlacer.placeLogIfFree`。
    fn place_log_if_free(
        &self,
        place: &mut TreePlace<'_, '_>,
        config: &TreeConfig,
        random: &WorldgenRandom,
        pos: Pos,
    ) {
        if self.is_free(place.world, pos) {
            self.place_log(place, config, random, pos, LogAxis::Keep);
        }
    }

    /// 官方 `placeTrunk`，返回树叶挂点。
    ///
    /// Official `placeTrunk`; returns the foliage attachments.
    pub(crate) fn place_trunk(
        &self,
        place: &mut TreePlace<'_, '_>,
        config: &TreeConfig,
        random: &WorldgenRandom,
        tree_height: i32,
        origin: Pos,
    ) -> Vec<FoliageAttachment> {
        let mut run = Run {
            placer: self,
            place,
            config,
            random,
            tree_height,
            origin,
        };
        match &self.kind {
            Kind::Straight => run.straight(),
            Kind::Forking => run.forking(),
            Kind::Giant => run.giant(),
            Kind::MegaJungle => run.mega_jungle(),
            Kind::DarkOak => run.dark_oak(),
            Kind::Fancy => run.fancy(),
            Kind::Bending {
                min_height_for_leaves,
                bend_length,
            } => run.bending(*min_height_for_leaves, bend_length),
            Kind::UpwardsBranching {
                extra_branch_steps,
                place_branch_per_log_probability,
                extra_branch_length,
                ..
            } => run.upwards_branching(
                extra_branch_steps,
                *place_branch_per_log_probability,
                extra_branch_length,
            ),
            Kind::Cherry {
                branch_count,
                branch_horizontal_length,
                branch_start,
                branch_end_offset_from_top,
            } => run.cherry(
                branch_count,
                branch_horizontal_length,
                *branch_start,
                branch_end_offset_from_top,
            ),
            Kind::Poplar {
                trunk_height_above_branches,
                branch_amount,
            } => run.poplar(trunk_height_above_branches, branch_amount),
        }
    }
}

/// `HolderSet<Block>`：`#标签`、单个方块 ID，或方块 ID 列表。
///
/// `HolderSet<Block>`: a `#tag`, one block ID, or a list of block IDs.
pub(crate) fn holder_set(value: &Value) -> Option<Vec<i32>> {
    let block = |name: &str| {
        let block = block_by_name(name)?;
        block_id_of_state(block.default_state).map(|id| id as i32)
    };
    match value {
        Value::String(name) => match name.strip_prefix('#') {
            Some(tag) => Some(block_tag(tag)?.to_vec()),
            None => Some(vec![block(name)?]),
        },
        Value::Array(names) => names.iter().map(|name| block(name.as_str()?)).collect(),
        _ => None,
    }
}

/// 一次 `placeTrunk` 调用的参数，省得每个辅助函数都传一遍。
///
/// The arguments of one `placeTrunk` call, so helpers need not pass them around.
struct Run<'r, 'a, 'w> {
    placer: &'r TrunkPlacer,
    place: &'r mut TreePlace<'a, 'w>,
    config: &'r TreeConfig,
    random: &'r WorldgenRandom,
    tree_height: i32,
    origin: Pos,
}

impl Run<'_, '_, '_> {
    fn log(&mut self, pos: Pos) -> bool {
        self.log_with(pos, LogAxis::Keep)
    }

    fn log_with(&mut self, pos: Pos, axis: LogAxis) -> bool {
        self.placer
            .place_log(self.place, self.config, self.random, pos, axis)
    }

    fn log_if_free(&mut self, pos: Pos) {
        self.placer
            .place_log_if_free(self.place, self.config, self.random, pos);
    }

    fn below_trunk(&mut self, pos: Pos) {
        self.config.below_trunk.place(self.place, self.random, pos);
    }

    fn at(&self, dx: i32, dy: i32, dz: i32) -> Pos {
        (self.origin.0 + dx, self.origin.1 + dy, self.origin.2 + dz)
    }

    /// `StraightTrunkPlacer.placeTrunk`。
    fn straight(&mut self) -> Vec<FoliageAttachment> {
        self.below_trunk(self.at(0, -1, 0));
        for y in 0..self.tree_height {
            self.log(self.at(0, y, 0));
        }
        vec![FoliageAttachment::new(
            self.at(0, self.tree_height, 0),
            0,
            false,
        )]
    }

    /// `ForkingTrunkPlacer.placeTrunk`。
    fn forking(&mut self) -> Vec<FoliageAttachment> {
        self.below_trunk(self.at(0, -1, 0));
        let mut attachments = Vec::new();
        let lean_direction = random_horizontal(self.random);
        let lean_height = self.tree_height - self.random.next_int(4) - 1;
        let mut lean_steps = 3 - self.random.next_int(3);
        let (mut tx, mut tz) = (self.origin.0, self.origin.2);
        let mut ey = None;
        for yo in 0..self.tree_height {
            let yy = self.origin.1 + yo;
            if yo >= lean_height && lean_steps > 0 {
                tx += HORIZONTAL[lean_direction].0;
                tz += HORIZONTAL[lean_direction].1;
                lean_steps -= 1;
            }
            if self.log((tx, yy, tz)) {
                ey = Some(yy + 1);
            }
        }
        if let Some(ey) = ey {
            attachments.push(FoliageAttachment::new((tx, ey, tz), 1, false));
        }
        (tx, tz) = (self.origin.0, self.origin.2);
        let branch_direction = random_horizontal(self.random);
        if branch_direction != lean_direction {
            let branch_pos = lean_height - self.random.next_int(2) - 1;
            let mut branch_steps = 1 + self.random.next_int(3);
            let mut ey = None;
            let mut yo = branch_pos;
            while yo < self.tree_height && branch_steps > 0 {
                if yo >= 1 {
                    let yy = self.origin.1 + yo;
                    tx += HORIZONTAL[branch_direction].0;
                    tz += HORIZONTAL[branch_direction].1;
                    if self.log((tx, yy, tz)) {
                        ey = Some(yy + 1);
                    }
                }
                yo += 1;
                branch_steps -= 1;
            }
            if let Some(ey) = ey {
                attachments.push(FoliageAttachment::new((tx, ey, tz), 0, false));
            }
        }
        attachments
    }

    /// `GiantTrunkPlacer.placeTrunk`：2×2 的树干。
    ///
    /// `GiantTrunkPlacer.placeTrunk`: a 2×2 trunk.
    fn giant(&mut self) -> Vec<FoliageAttachment> {
        for (dx, dz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            self.below_trunk(self.at(dx, -1, dz));
        }
        for hh in 0..self.tree_height {
            self.log_if_free(self.at(0, hh, 0));
            if hh < self.tree_height - 1 {
                self.log_if_free(self.at(1, hh, 0));
                self.log_if_free(self.at(1, hh, 1));
                self.log_if_free(self.at(0, hh, 1));
            }
        }
        vec![FoliageAttachment::new(
            self.at(0, self.tree_height, 0),
            0,
            true,
        )]
    }

    /// `MegaJungleTrunkPlacer.placeTrunk`：先按 `GiantTrunkPlacer` 放，再加侧枝。
    ///
    /// `MegaJungleTrunkPlacer.placeTrunk`: the giant trunk, then side branches.
    fn mega_jungle(&mut self) -> Vec<FoliageAttachment> {
        let mut attachments = self.giant();
        let mut branch_height = self.tree_height - 2 - self.random.next_int(4);
        while branch_height > self.tree_height / 2 {
            let angle = self.random.next_float() * (std::f64::consts::PI * 2.0) as f32;
            let (mut bx, mut bz) = (0, 0);
            for b in 0..5 {
                bx = (1.5_f32 + mth_cos(f64::from(angle)) * b as f32) as i32;
                bz = (1.5_f32 + mth_sin(f64::from(angle)) * b as f32) as i32;
                self.log(self.at(bx, branch_height - 3 + b / 2, bz));
            }
            attachments.push(FoliageAttachment::new(
                self.at(bx, branch_height, bz),
                -2,
                false,
            ));
            branch_height -= 2 + self.random.next_int(4);
        }
        attachments
    }

    /// `DarkOakTrunkPlacer.placeTrunk`。
    fn dark_oak(&mut self) -> Vec<FoliageAttachment> {
        let mut attachments = Vec::new();
        for (dx, dz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            self.below_trunk(self.at(dx, -1, dz));
        }
        let lean_direction = random_horizontal(self.random);
        let lean_height = self.tree_height - self.random.next_int(4);
        let mut lean_steps = 2 - self.random.next_int(3);
        let (x, y, z) = self.origin;
        let (mut tx, mut tz) = (x, z);
        let ey = y + self.tree_height - 1;
        for dy in 0..self.tree_height {
            if dy >= lean_height && lean_steps > 0 {
                tx += HORIZONTAL[lean_direction].0;
                tz += HORIZONTAL[lean_direction].1;
                lean_steps -= 1;
            }
            let yy = y + dy;
            if is_air_or_leaves(self.place.world, (tx, yy, tz)) {
                self.log((tx, yy, tz));
                self.log((tx + 1, yy, tz));
                self.log((tx, yy, tz + 1));
                self.log((tx + 1, yy, tz + 1));
            }
        }
        attachments.push(FoliageAttachment::new((tx, ey, tz), 0, true));
        for ox in -1..=2 {
            for oz in -1..=2 {
                let outside = !(0..=1).contains(&ox) || !(0..=1).contains(&oz);
                if outside && self.random.next_int(3) <= 0 {
                    let length = self.random.next_int(3) + 2;
                    for branch_y in 0..length {
                        self.log((x + ox, ey - branch_y - 1, z + oz));
                    }
                    attachments.push(FoliageAttachment::new((x + ox, ey, z + oz), 0, false));
                }
            }
        }
        attachments
    }

    /// `BendingTrunkPlacer.placeTrunk`。
    fn bending(
        &mut self,
        min_height_for_leaves: i32,
        bend_length: &IntProvider,
    ) -> Vec<FoliageAttachment> {
        let direction = HORIZONTAL[random_horizontal(self.random)];
        let log_height = self.tree_height - 1;
        let mut pos = self.origin;
        self.below_trunk(self.at(0, -1, 0));
        let mut foliage_points = Vec::new();
        for i in 0..=log_height {
            if i + 1 >= log_height + self.random.next_int(2) {
                pos.0 += direction.0;
                pos.2 += direction.1;
            }
            if valid_tree_pos(self.place.world, pos) {
                self.log(pos);
            }
            if i >= min_height_for_leaves {
                foliage_points.push(FoliageAttachment::new(pos, 0, false));
            }
            pos.1 += 1;
        }
        let dir_length = bend_length.sample(self.random);
        for _ in 0..=dir_length {
            if valid_tree_pos(self.place.world, pos) {
                self.log(pos);
            }
            foliage_points.push(FoliageAttachment::new(pos, 0, false));
            pos.0 += direction.0;
            pos.2 += direction.1;
        }
        foliage_points
    }

    /// `UpwardsBranchingTrunkPlacer.placeTrunk`。
    fn upwards_branching(
        &mut self,
        extra_branch_steps: &IntProvider,
        probability: f32,
        extra_branch_length: &IntProvider,
    ) -> Vec<FoliageAttachment> {
        let mut attachments = Vec::new();
        for height_pos in 0..self.tree_height {
            let current_height = self.origin.1 + height_pos;
            let log_pos = (self.origin.0, current_height, self.origin.2);
            if self.log(log_pos)
                && height_pos < self.tree_height - 1
                && self.random.next_float() < probability
            {
                let branch_dir = random_horizontal(self.random);
                let branch_len = extra_branch_length.sample(self.random);
                let branch_pos = (branch_len - extra_branch_length.sample(self.random) - 1).max(0);
                let branch_steps = extra_branch_steps.sample(self.random);
                self.place_branch(
                    &mut attachments,
                    log_pos,
                    HORIZONTAL[branch_dir],
                    branch_pos,
                    branch_steps,
                );
            }
            if height_pos == self.tree_height - 1 {
                attachments.push(FoliageAttachment::new(
                    (self.origin.0, current_height + 1, self.origin.2),
                    0,
                    false,
                ));
            }
        }
        attachments
    }

    /// `UpwardsBranchingTrunkPlacer.placeBranch`。
    fn place_branch(
        &mut self,
        attachments: &mut Vec<FoliageAttachment>,
        log_pos: Pos,
        branch_dir: (i32, i32),
        branch_pos: i32,
        mut branch_steps: i32,
    ) {
        let current_height = log_pos.1;
        let mut height_along_branch = current_height + branch_pos;
        let (mut log_x, mut log_z) = (log_pos.0, log_pos.2);
        let mut index = branch_pos;
        while index < self.tree_height && branch_steps > 0 {
            if index >= 1 {
                let placement_height = current_height + index;
                log_x += branch_dir.0;
                log_z += branch_dir.1;
                height_along_branch = placement_height;
                if self.log((log_x, placement_height, log_z)) {
                    height_along_branch += 1;
                }
                attachments.push(FoliageAttachment::new(
                    (log_x, placement_height, log_z),
                    0,
                    false,
                ));
            }
            index += 1;
            branch_steps -= 1;
        }
        if height_along_branch - current_height > 1 {
            let foliage_pos = (log_x, height_along_branch, log_z);
            attachments.push(FoliageAttachment::new(foliage_pos, 0, false));
            attachments.push(FoliageAttachment::new(
                (log_x, height_along_branch - 2, log_z),
                0,
                false,
            ));
        }
    }

    /// `CherryTrunkPlacer.placeTrunk`。
    fn cherry(
        &mut self,
        branch_count: &IntProvider,
        branch_horizontal_length: &IntProvider,
        branch_start: (i32, i32),
        branch_end_offset_from_top: &IntProvider,
    ) -> Vec<FoliageAttachment> {
        self.below_trunk(self.at(0, -1, 0));
        let first_start = IntProvider::Uniform {
            min: branch_start.0,
            max: branch_start.1,
        };
        // 官方构造器里的 `secondBranchStartOffsetFromTop`：上界少 1。
        // `secondBranchStartOffsetFromTop` from the official constructor: max minus one.
        let second_start = IntProvider::Uniform {
            min: branch_start.0,
            max: branch_start.1 - 1,
        };
        let first_offset = (self.tree_height - 1 + first_start.sample(self.random)).max(0);
        let mut second_offset = (self.tree_height - 1 + second_start.sample(self.random)).max(0);
        if second_offset >= first_offset {
            second_offset += 1;
        }
        let branches = branch_count.sample(self.random);
        let has_middle_branch = branches == 3;
        let has_both_side_branches = branches >= 2;
        let trunk_height = if has_middle_branch {
            self.tree_height
        } else if has_both_side_branches {
            first_offset.max(second_offset) + 1
        } else {
            first_offset + 1
        };
        for y in 0..trunk_height {
            self.log(self.at(0, y, 0));
        }
        let mut attachments = Vec::new();
        if has_middle_branch {
            attachments.push(FoliageAttachment::new(
                self.at(0, trunk_height, 0),
                0,
                false,
            ));
        }
        let tree_direction = random_horizontal(self.random);
        let sideways = LogAxis::Set(axis_of(tree_direction));
        attachments.push(self.cherry_branch(
            branch_horizontal_length,
            branch_end_offset_from_top,
            sideways,
            HORIZONTAL[tree_direction],
            first_offset,
            first_offset < trunk_height - 1,
        ));
        if has_both_side_branches {
            // `getOpposite`：水平方向表里隔两个。
            // `getOpposite`: two entries further in the horizontal table.
            let opposite = HORIZONTAL[(tree_direction + 2) % 4];
            attachments.push(self.cherry_branch(
                branch_horizontal_length,
                branch_end_offset_from_top,
                sideways,
                opposite,
                second_offset,
                second_offset < trunk_height - 1,
            ));
        }
        attachments
    }

    /// `CherryTrunkPlacer.generateBranch`。
    fn cherry_branch(
        &mut self,
        branch_horizontal_length: &IntProvider,
        branch_end_offset_from_top: &IntProvider,
        sideways: LogAxis,
        branch_direction: (i32, i32),
        offset_from_origin: i32,
        middle_continues_upwards: bool,
    ) -> FoliageAttachment {
        let mut log_pos = self.at(0, offset_from_origin, 0);
        let end_offset = self.tree_height - 1 + branch_end_offset_from_top.sample(self.random);
        let extend = middle_continues_upwards || end_offset < offset_from_origin;
        let distance_to_trunk = branch_horizontal_length.sample(self.random) + i32::from(extend);
        let branch_end = (
            self.origin.0 + branch_direction.0 * distance_to_trunk,
            self.origin.1 + end_offset,
            self.origin.2 + branch_direction.1 * distance_to_trunk,
        );
        let steps_horizontally = if extend { 2 } else { 1 };
        for _ in 0..steps_horizontally {
            log_pos.0 += branch_direction.0;
            log_pos.2 += branch_direction.1;
            self.log_with(log_pos, sideways);
        }
        let vertical = if branch_end.1 > log_pos.1 { 1 } else { -1 };
        loop {
            let distance = (log_pos.0 - branch_end.0).abs()
                + (log_pos.1 - branch_end.1).abs()
                + (log_pos.2 - branch_end.2).abs();
            if distance == 0 {
                return FoliageAttachment::new(
                    (branch_end.0, branch_end.1 + 1, branch_end.2),
                    0,
                    false,
                );
            }
            let chance = (branch_end.1 - log_pos.1).abs() as f32 / distance as f32;
            let grow_vertically = self.random.next_float() < chance;
            if grow_vertically {
                log_pos.1 += vertical;
                self.log_with(log_pos, LogAxis::Keep);
            } else {
                log_pos.0 += branch_direction.0;
                log_pos.2 += branch_direction.1;
                self.log_with(log_pos, sideways);
            }
        }
    }

    /// `PoplarTrunkPlacer.placeTrunk`。
    fn poplar(
        &mut self,
        trunk_height_above_branches: &IntProvider,
        branch_amount: &IntProvider,
    ) -> Vec<FoliageAttachment> {
        self.below_trunk(self.at(0, -1, 0));
        let up_to_branches = self.tree_height - trunk_height_above_branches.sample(self.random);
        for y in 0..self.tree_height {
            self.log(self.at(0, y, 0));
            // 每一层都洗一次牌，哪怕这一层不长侧枝。
            // The shuffle happens on every layer, branches or not.
            let directions = shuffled_horizontal(self.random);
            if up_to_branches - 1 == y {
                let branches = branch_amount.sample(self.random);
                for &direction in directions.iter().take(branches.max(0) as usize) {
                    let step = HORIZONTAL[direction];
                    self.log_with(self.at(step.0, y, step.1), LogAxis::Set(axis_of(direction)));
                }
            }
        }
        vec![FoliageAttachment::new(
            self.at(0, up_to_branches, 0),
            0,
            false,
        )]
    }

    /// `FancyTrunkPlacer.placeTrunk`。
    fn fancy(&mut self) -> Vec<FoliageAttachment> {
        let height = self.tree_height + 2;
        let trunk_height = floor_f64(f64::from(height) * 0.618);
        self.below_trunk(self.at(0, -1, 0));
        let clusters_per_y = 1.min(floor_f64(
            1.382 + (1.0 * f64::from(height) / 13.0).powf(2.0),
        ));
        let trunk_top = self.origin.1 + trunk_height;
        let mut relative_y = height - 5;
        // `FoliageCoords`：挂点位置和它的 `branchBase`。
        // `FoliageCoords`: the attachment position and its `branchBase`.
        let mut foliage_coords: Vec<(Pos, i32)> = vec![(self.at(0, relative_y, 0), trunk_top)];
        while relative_y >= 0 {
            let tree_shape = fancy_tree_shape(height, relative_y);
            if tree_shape >= 0.0 {
                for _ in 0..clusters_per_y {
                    let radius =
                        f64::from(tree_shape) * (f64::from(self.random.next_float()) + 0.328);
                    let angle =
                        f64::from(self.random.next_float() * 2.0_f32) * std::f64::consts::PI;
                    let x = radius * angle.sin() + 0.5;
                    let z = radius * angle.cos() + 0.5;
                    let check_start = self.at(floor_f64(x), relative_y - 1, floor_f64(z));
                    let check_end = (check_start.0, check_start.1 + 5, check_start.2);
                    if self.fancy_limb(check_start, check_end, false) {
                        let dx = self.origin.0 - check_start.0;
                        let dz = self.origin.2 - check_start.2;
                        let branch_height =
                            f64::from(check_start.1) - f64::from(dx * dx + dz * dz).sqrt() * 0.381;
                        let branch_top = if branch_height > f64::from(trunk_top) {
                            trunk_top
                        } else {
                            branch_height as i32
                        };
                        let branch_base = (self.origin.0, branch_top, self.origin.2);
                        if self.fancy_limb(branch_base, check_start, false) {
                            foliage_coords.push((check_start, branch_base.1));
                        }
                    }
                }
            }
            relative_y -= 1;
        }
        self.fancy_limb(self.origin, self.at(0, trunk_height, 0), true);
        // `makeBranches`。
        for &(end, branch_base) in &foliage_coords {
            let base = (self.origin.0, branch_base, self.origin.2);
            if base != end && fancy_trim_branches(height, branch_base - self.origin.1) {
                self.fancy_limb(base, end, true);
            }
        }
        foliage_coords
            .into_iter()
            .filter(|&(_, branch_base)| fancy_trim_branches(height, branch_base - self.origin.1))
            .map(|(pos, _)| FoliageAttachment::new(pos, 0, false))
            .collect()
    }

    /// `FancyTrunkPlacer.makeLimb`：`do_place` 为假时只检查这条线上是否都空。
    ///
    /// `FancyTrunkPlacer.makeLimb`: with `do_place` false it only checks the line is free.
    fn fancy_limb(&mut self, start: Pos, end: Pos, do_place: bool) -> bool {
        if !do_place && start == end {
            return true;
        }
        let delta = (end.0 - start.0, end.1 - start.1, end.2 - start.2);
        let steps = delta.0.abs().max(delta.1.abs()).max(delta.2.abs());
        // steps 为 0 时官方也是浮点除零得 NaN，`Mth.floor(NaN)` 是 0，结果就是起点。
        // With zero steps the official code also divides by zero; `Mth.floor(NaN)` is 0,
        // which yields the start position.
        let dx = delta.0 as f32 / steps as f32;
        let dy = delta.1 as f32 / steps as f32;
        let dz = delta.2 as f32 / steps as f32;
        for i in 0..=steps {
            let pos = (
                start.0 + floor_f32(0.5 + i as f32 * dx),
                start.1 + floor_f32(0.5 + i as f32 * dy),
                start.2 + floor_f32(0.5 + i as f32 * dz),
            );
            if do_place {
                self.log_with(pos, LogAxis::Set(fancy_log_axis(start, pos)));
            } else if !self.placer.is_free(self.place.world, pos) {
                return false;
            }
        }
        true
    }
}

/// `FancyTrunkPlacer.getLogAxis`。
fn fancy_log_axis(start: Pos, pos: Pos) -> &'static str {
    let x_diff = (pos.0 - start.0).abs();
    let z_diff = (pos.2 - start.2).abs();
    let max_diff = x_diff.max(z_diff);
    if max_diff == 0 {
        "y"
    } else if x_diff == max_diff {
        "x"
    } else {
        "z"
    }
}

/// `FancyTrunkPlacer.trimBranches`。
fn fancy_trim_branches(height: i32, local_y: i32) -> bool {
    f64::from(local_y) >= f64::from(height) * 0.2
}

/// `FancyTrunkPlacer.treeShape`，全程 `float` 运算。
///
/// `FancyTrunkPlacer.treeShape`, in `float` arithmetic throughout.
fn fancy_tree_shape(height: i32, y: i32) -> f32 {
    if (y as f32) < height as f32 * 0.3 {
        return -1.0;
    }
    let radius = height as f32 / 2.0;
    let adjacent = radius - y as f32;
    let mut distance = (radius * radius - adjacent * adjacent).sqrt();
    if adjacent == 0.0 {
        distance = radius;
    } else if adjacent.abs() >= radius {
        return 0.0;
    }
    distance * 0.5
}

/// `Direction.allShuffled(random)` 再滤掉竖直方向，返回 [`HORIZONTAL`] 的下标。
/// `Util.shuffle` 从后往前，每步 `nextInt(i)` 后交换。
///
/// `Direction.allShuffled(random)` with the vertical directions filtered out, as
/// indexes into [`HORIZONTAL`]. `Util.shuffle` walks backwards, swapping after each
/// `nextInt(i)`.
fn shuffled_horizontal(random: &WorldgenRandom) -> Vec<usize> {
    // `Direction.values()`：DOWN、UP、NORTH、SOUTH、WEST、EAST；`None` 是竖直方向。
    // `Direction.values()`: DOWN, UP, NORTH, SOUTH, WEST, EAST; `None` is vertical.
    let mut all = [None, None, Some(0), Some(2), Some(3), Some(1)];
    for i in (2..=all.len()).rev() {
        let swap_to = random.next_int(i as i32) as usize;
        all.swap(i - 1, swap_to);
    }
    all.into_iter().flatten().collect()
}
