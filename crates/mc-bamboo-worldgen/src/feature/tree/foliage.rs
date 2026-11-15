//! `FoliagePlacer` 的各个子类，各自照官方 26.3 的 `createFoliage` /
//! `shouldSkipLocation` 逐行翻译。`poplar_foliage_placer` 还没移植。
//!
//! The `FoliagePlacer` subclasses, each translated line by line from official 26.3
//! `createFoliage` / `shouldSkipLocation`. `poplar_foliage_placer` is not ported yet.

use serde_json::Value;

use mc_bamboo_data::{registries, state_fluid};

use crate::{
    feature::{provider::IntProvider, state::with_property},
    random::WorldgenRandom,
};

use super::{
    FoliageAttachment, HORIZONTAL, Pos, TreeConfig, TreePlace, floor_f32, has_property,
    property_value, valid_tree_pos,
};

/// `FoliagePlacer` 的公共字段加上各子类自己的参数。
///
/// The shared `FoliagePlacer` fields plus each subclass's own parameters.
pub struct FoliagePlacer {
    radius: IntProvider,
    offset: IntProvider,
    kind: Kind,
}

enum Kind {
    Blob {
        height: i32,
    },
    Fancy {
        height: i32,
    },
    Bush {
        height: i32,
    },
    Acacia,
    MegaJungle {
        height: i32,
    },
    Pine {
        height: IntProvider,
    },
    Spruce {
        trunk_height: IntProvider,
    },
    MegaPine {
        crown_height: IntProvider,
    },
    DarkOak,
    RandomSpread {
        foliage_height: IntProvider,
        leaf_placement_attempts: i32,
    },
    Poplar {
        height: IntProvider,
        side_hole_chance: f32,
    },
    Cherry {
        height: IntProvider,
        wide_bottom_layer_hole_chance: f32,
        corner_hole_chance: f32,
        hanging_leaves_chance: f32,
        hanging_leaves_extension_chance: f32,
    },
}

impl FoliagePlacer {
    pub fn parse(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let int = |name: &str| object.get(name).and_then(Value::as_i64).map(|n| n as i32);
        let float = |name: &str| object.get(name).and_then(Value::as_f64).map(|n| n as f32);
        let provider = |name: &str| IntProvider::parse(object.get(name)?).ok();
        let kind = match object.get("type")?.as_str()? {
            "minecraft:blob_foliage_placer" => Kind::Blob {
                height: int("height")?,
            },
            "minecraft:fancy_foliage_placer" => Kind::Fancy {
                height: int("height")?,
            },
            "minecraft:bush_foliage_placer" => Kind::Bush {
                height: int("height")?,
            },
            "minecraft:acacia_foliage_placer" => Kind::Acacia,
            "minecraft:jungle_foliage_placer" => Kind::MegaJungle {
                height: int("height")?,
            },
            "minecraft:pine_foliage_placer" => Kind::Pine {
                height: provider("height")?,
            },
            "minecraft:spruce_foliage_placer" => Kind::Spruce {
                trunk_height: provider("trunk_height")?,
            },
            "minecraft:mega_pine_foliage_placer" => Kind::MegaPine {
                crown_height: provider("crown_height")?,
            },
            "minecraft:dark_oak_foliage_placer" => Kind::DarkOak,
            "minecraft:random_spread_foliage_placer" => Kind::RandomSpread {
                foliage_height: provider("foliage_height")?,
                leaf_placement_attempts: int("leaf_placement_attempts")?,
            },
            "minecraft:poplar_foliage_placer" => Kind::Poplar {
                height: provider("height")?,
                side_hole_chance: float("side_hole_chance")?,
            },
            "minecraft:cherry_foliage_placer" => Kind::Cherry {
                height: provider("height")?,
                wide_bottom_layer_hole_chance: float("wide_bottom_layer_hole_chance")?,
                corner_hole_chance: float("corner_hole_chance")?,
                hanging_leaves_chance: float("hanging_leaves_chance")?,
                hanging_leaves_extension_chance: float("hanging_leaves_extension_chance")?,
            },
            _ => return None,
        };
        Some(Self {
            radius: provider("radius")?,
            offset: provider("offset")?,
            kind,
        })
    }

    /// 官方 `foliageHeight(random, treeHeight, tree)`。
    ///
    /// Official `foliageHeight(random, treeHeight, tree)`.
    pub fn foliage_height(&self, random: &WorldgenRandom, tree_height: i32) -> i32 {
        match &self.kind {
            Kind::Blob { height }
            | Kind::Fancy { height }
            | Kind::Bush { height }
            | Kind::MegaJungle { height } => *height,
            Kind::Acacia => 0,
            Kind::Pine { height } | Kind::Cherry { height, .. } | Kind::Poplar { height, .. } => {
                height.sample(random)
            }
            Kind::Spruce { trunk_height } => 4.max(tree_height - trunk_height.sample(random)),
            Kind::MegaPine { crown_height } => crown_height.sample(random),
            Kind::DarkOak => 4,
            Kind::RandomSpread { foliage_height, .. } => foliage_height.sample(random),
        }
    }

    /// 官方 `foliageRadius(random, trunkHeight)`；松树多取一次随机数。
    ///
    /// Official `foliageRadius(random, trunkHeight)`; pine draws one more random.
    pub fn foliage_radius(&self, random: &WorldgenRandom, trunk_height: i32) -> i32 {
        let radius = self.radius.sample(random);
        match &self.kind {
            Kind::Pine { .. } => radius + random.next_int((trunk_height + 1).max(1)),
            _ => radius,
        }
    }

    /// 官方公开的 `createFoliage`：先取 `offset`，再进各子类的实现。
    ///
    /// The public official `createFoliage`: sample `offset`, then run the subclass.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn create_foliage(
        &self,
        place: &mut TreePlace<'_, '_>,
        config: &TreeConfig,
        random: &WorldgenRandom,
        _tree_height: i32,
        attachment: &FoliageAttachment,
        foliage_height: i32,
        leaf_radius: i32,
    ) {
        let offset = self.offset.sample(random);
        let mut run = Run {
            placer: self,
            place,
            config,
            random,
        };
        let a = attachment;
        let double_trunk = a.double_trunk();
        let height_with_offset = foliage_height + a.foliage_height_offset;
        match &self.kind {
            Kind::Blob { .. } => {
                for yo in (offset - height_with_offset..=offset).rev() {
                    let radius = (leaf_radius + a.radius_offset_xz - 1 - yo / 2).max(0);
                    run.leaves_row(a.pos, radius, yo, double_trunk);
                }
            }
            Kind::Fancy { .. } => {
                for yo in (offset - foliage_height..=offset).rev() {
                    let middle = yo != offset && yo != offset - foliage_height;
                    run.leaves_row(a.pos, leaf_radius + i32::from(middle), yo, double_trunk);
                }
            }
            Kind::Bush { .. } => {
                for yo in (offset - height_with_offset..=offset).rev() {
                    let radius = leaf_radius + a.radius_offset_xz - 1 - yo;
                    run.leaves_row(a.pos, radius, yo, double_trunk);
                }
            }
            Kind::Acacia => {
                let pos = (a.pos.0, a.pos.1 + offset, a.pos.2);
                run.leaves_row(
                    pos,
                    leaf_radius + a.radius_offset_xz,
                    -1 - height_with_offset,
                    double_trunk,
                );
                run.leaves_row(pos, leaf_radius - 1, -height_with_offset, double_trunk);
                run.leaves_row(pos, leaf_radius + a.radius_offset_xz - 1, 0, double_trunk);
            }
            Kind::MegaJungle { .. } => {
                let leaf_height = if double_trunk {
                    foliage_height
                } else {
                    1 + random.next_int(2)
                } + a.foliage_height_offset;
                for yo in (offset - leaf_height..=offset).rev() {
                    let radius = leaf_radius + a.radius_offset_xz + 1 - yo;
                    run.leaves_row(a.pos, radius, yo, double_trunk);
                }
            }
            Kind::Pine { .. } => {
                let mut radius = 0;
                for yo in (offset - height_with_offset..=offset).rev() {
                    run.leaves_row(a.pos, radius, yo, double_trunk);
                    if radius >= 1 && yo == offset - height_with_offset + 1 {
                        radius -= 1;
                    } else if radius < leaf_radius + a.radius_offset_xz {
                        radius += 1;
                    }
                }
            }
            Kind::Spruce { .. } => {
                let mut radius = random.next_int(2);
                let mut max_radius = 1;
                let mut min_radius = 0;
                // 下界是 `-foliageHeightWithOffset`，不带 offset。
                // The lower bound is `-foliageHeightWithOffset`, without the offset.
                for yo in (-height_with_offset..=offset).rev() {
                    run.leaves_row(a.pos, radius, yo, double_trunk);
                    if radius >= max_radius {
                        radius = min_radius;
                        min_radius = 1;
                        max_radius = (max_radius + 1).min(leaf_radius + a.radius_offset_xz);
                    } else {
                        radius += 1;
                    }
                }
            }
            Kind::MegaPine { .. } => {
                let mut prev_radius = 0;
                for yy in a.pos.1 - height_with_offset + offset..=a.pos.1 + offset {
                    let yo = a.pos.1 - yy;
                    let smooth = leaf_radius
                        + a.radius_offset_xz
                        + floor_f32(yo as f32 / height_with_offset as f32 * 3.5);
                    let jagged = if yo > 0 && smooth == prev_radius && (yy & 1) == 0 {
                        smooth + 1
                    } else {
                        smooth
                    };
                    run.leaves_row((a.pos.0, yy, a.pos.2), jagged, 0, double_trunk);
                    prev_radius = smooth;
                }
            }
            Kind::DarkOak => {
                let pos = (a.pos.0, a.pos.1 + offset, a.pos.2);
                if double_trunk {
                    run.leaves_row(pos, leaf_radius + 2, -1, double_trunk);
                    run.leaves_row(pos, leaf_radius + 3, 0, double_trunk);
                    run.leaves_row(pos, leaf_radius + 2, 1, double_trunk);
                    if random.next_boolean() {
                        run.leaves_row(pos, leaf_radius, 2, double_trunk);
                    }
                } else {
                    run.leaves_row(pos, leaf_radius + 2, -1, double_trunk);
                    run.leaves_row(pos, leaf_radius + 1, 0, double_trunk);
                }
            }
            Kind::RandomSpread {
                leaf_placement_attempts,
                ..
            } => {
                for _ in 0..*leaf_placement_attempts {
                    let dx = random.next_int(leaf_radius) - random.next_int(leaf_radius);
                    let dy = random.next_int(foliage_height) - random.next_int(foliage_height);
                    let dz = random.next_int(leaf_radius) - random.next_int(leaf_radius);
                    run.try_place_leaf((a.pos.0 + dx, a.pos.1 + dy, a.pos.2 + dz));
                }
            }
            Kind::Poplar {
                side_hole_chance, ..
            } => {
                let pos = (a.pos.0, a.pos.1 + offset, a.pos.2);
                let radius = leaf_radius + a.radius_offset_xz - 1;
                let flip = random.next_boolean();
                let poplar = Poplar {
                    side_hole_chance: *side_hole_chance,
                    foliage_height: height_with_offset,
                    flip,
                    double_trunk,
                };
                run.poplar_row(&poplar, pos, radius - 2, height_with_offset - 1);
                run.poplar_row(&poplar, pos, radius - 1, height_with_offset - 2);
                run.poplar_row(&poplar, pos, radius - 1, height_with_offset - 3);
                for y in (1..=height_with_offset - 4).rev() {
                    run.poplar_row(&poplar, pos, radius, y);
                }
                run.poplar_logs(&poplar, pos, radius, height_with_offset - 4);
                run.poplar_row(&poplar, pos, radius - 1, 0);
                run.poplar_row(&poplar, pos, (radius - 2).clamp(1, 2), -1);
            }
            Kind::Cherry {
                hanging_leaves_chance,
                hanging_leaves_extension_chance,
                ..
            } => {
                let pos = (a.pos.0, a.pos.1 + offset, a.pos.2);
                let radius = leaf_radius + a.radius_offset_xz - 1;
                run.leaves_row(pos, radius - 2, height_with_offset - 3, double_trunk);
                run.leaves_row(pos, radius - 1, height_with_offset - 4, double_trunk);
                for y in (0..=height_with_offset - 5).rev() {
                    run.leaves_row(pos, radius, y, double_trunk);
                }
                let chances = (*hanging_leaves_chance, *hanging_leaves_extension_chance);
                run.leaves_row_with_hanging(pos, radius, -1, double_trunk, chances);
                run.leaves_row_with_hanging(pos, radius - 1, -2, double_trunk, chances);
            }
        }
    }

    /// 官方 `shouldSkipLocation`（参数已是绝对值）。
    ///
    /// Official `shouldSkipLocation` (arguments are already absolute values).
    fn should_skip_location(
        &self,
        random: &WorldgenRandom,
        dx: i32,
        y: i32,
        dz: i32,
        radius: i32,
        double_trunk: bool,
    ) -> bool {
        match &self.kind {
            Kind::Blob { .. } => {
                dx == radius && dz == radius && (random.next_int(2) == 0 || y == 0)
            }
            Kind::Fancy { .. } => {
                let square = |v: f32| v * v;
                square(dx as f32 + 0.5) + square(dz as f32 + 0.5) > (radius * radius) as f32
            }
            Kind::Bush { .. } => dx == radius && dz == radius && random.next_int(2) == 0,
            Kind::Acacia => {
                if y == 0 {
                    (dx > 1 || dz > 1) && dx != 0 && dz != 0
                } else {
                    dx == radius && dz == radius && radius > 0
                }
            }
            Kind::MegaJungle { .. } | Kind::MegaPine { .. } => {
                dx + dz >= 7 || dx * dx + dz * dz > radius * radius
            }
            Kind::Pine { .. } | Kind::Spruce { .. } => dx == radius && dz == radius && radius > 0,
            Kind::DarkOak => {
                if y == -1 && !double_trunk {
                    dx == radius && dz == radius
                } else if y == 1 {
                    dx + dz > radius * 2 - 2
                } else {
                    false
                }
            }
            // 白杨有自己的 `shouldSkipLocation`，官方这个重载直接抛异常。
            // Poplar has its own `shouldSkipLocation`; the official overload throws.
            Kind::RandomSpread { .. } | Kind::Poplar { .. } => false,
            Kind::Cherry {
                wide_bottom_layer_hole_chance,
                corner_hole_chance,
                ..
            } => {
                if y == -1
                    && (dx == radius || dz == radius)
                    && random.next_float() < *wide_bottom_layer_hole_chance
                {
                    return true;
                }
                let corner = dx == radius && dz == radius;
                if radius > 2 {
                    corner || dx + dz > radius * 2 - 2 && random.next_float() < *corner_hole_chance
                } else {
                    corner && random.next_float() < *corner_hole_chance
                }
            }
        }
    }

    /// 官方 `shouldSkipLocationSigned`；深色橡木在双树干的最宽一层先裁掉四条边。
    ///
    /// Official `shouldSkipLocationSigned`; dark oak first trims the four edges of its
    /// widest double-trunk layer.
    fn should_skip_location_signed(
        &self,
        random: &WorldgenRandom,
        dx: i32,
        y: i32,
        dz: i32,
        radius: i32,
        double_trunk: bool,
    ) -> bool {
        if matches!(self.kind, Kind::DarkOak) {
            let inside = y != 0
                || !double_trunk
                || dx != -radius && dx < radius
                || dz != -radius && dz < radius;
            if !inside {
                return true;
            }
        }
        let (min_dx, min_dz) = if double_trunk {
            (dx.abs().min((dx - 1).abs()), dz.abs().min((dz - 1).abs()))
        } else {
            (dx.abs(), dz.abs())
        };
        self.should_skip_location(random, min_dx, y, min_dz, radius, double_trunk)
    }
}

/// 一次 `createFoliage` 调用的参数。
///
/// The arguments of one `createFoliage` call.
struct Run<'r, 'a, 'w> {
    placer: &'r FoliagePlacer,
    place: &'r mut TreePlace<'a, 'w>,
    config: &'r TreeConfig,
    random: &'r WorldgenRandom,
}

impl Run<'_, '_, '_> {
    /// 官方 `FoliagePlacer.placeLeavesRow`。
    ///
    /// Official `FoliagePlacer.placeLeavesRow`.
    fn leaves_row(&mut self, origin: Pos, radius: i32, y: i32, double_trunk: bool) {
        let offset = i32::from(double_trunk);
        for dx in -radius..=radius + offset {
            for dz in -radius..=radius + offset {
                let skip = self.placer.should_skip_location_signed(
                    self.random,
                    dx,
                    y,
                    dz,
                    radius,
                    double_trunk,
                );
                if !skip {
                    self.try_place_leaf((origin.0 + dx, origin.1 + y, origin.2 + dz));
                }
            }
        }
    }

    /// 官方 `PoplarFoliagePlacer.placeLeavesRow`。
    ///
    /// Official `PoplarFoliagePlacer.placeLeavesRow`.
    fn poplar_row(&mut self, poplar: &Poplar, origin: Pos, radius: i32, y: i32) {
        let offset = i32::from(poplar.double_trunk);
        for dx in -radius..=radius + offset {
            for dz in -radius..=radius + offset {
                if !poplar.should_skip_location(self.random, dx, y, dz, radius) {
                    self.try_place_leaf((origin.0 + dx, origin.1 + y, origin.2 + dz));
                }
            }
        }
    }

    /// 官方 `PoplarFoliagePlacer.replaceLeavesWithLog`：把伸得够远的那几片树叶换成
    /// 横放的原木。换上去的原木走的是 `foliageSetter`，所以记在树叶集合里。
    ///
    /// Official `PoplarFoliagePlacer.replaceLeavesWithLog`: swaps the leaves that reach
    /// far enough for sideways logs. The logs go through `foliageSetter`, so they are
    /// recorded in the foliage set.
    fn poplar_logs(&mut self, poplar: &Poplar, origin: Pos, radius: i32, y: i32) {
        let offset = i32::from(poplar.double_trunk);
        for dx in -radius..=radius + offset {
            for dz in -radius..=radius + offset {
                let (abs_dx, abs_dz) = (dx.abs(), dz.abs());
                let partial = poplar.partial_row(y);
                let corner = poplar.corner_blocks_to_cut(dx, dz, radius, partial);
                let far =
                    abs_dz == 0 && radius - abs_dx >= 4 || abs_dx == 0 && radius - abs_dz >= 4;
                if !(within_rhombus(radius, abs_dx, abs_dz, corner, 2) && far) {
                    continue;
                }
                let pos = (origin.0 + dx, origin.1 + y, origin.2 + dz);
                // `tryPlaceLog`：这一格正好是 foliage provider 给出的状态才换。
                // `tryPlaceLog`: only when the block equals the state the foliage
                // provider gives.
                let foliage = self.config.foliage_provider.get_optional_state(
                    self.place.world,
                    self.random,
                    pos,
                );
                if foliage != Some(self.place.get_block(pos)) {
                    continue;
                }
                if let Some(log) = self.config.trunk_provider.get_optional_state(
                    self.place.world,
                    self.random,
                    pos,
                ) {
                    let axis = if abs_dz == 0 { "x" } else { "z" };
                    let log = with_property(log, "axis", axis).unwrap_or(log);
                    self.place.set_foliage(pos, log);
                }
            }
        }
    }

    /// 官方 `FoliagePlacer.placeLeavesRowWithHangingLeavesBelow`。
    ///
    /// Official `FoliagePlacer.placeLeavesRowWithHangingLeavesBelow`.
    fn leaves_row_with_hanging(
        &mut self,
        origin: Pos,
        radius: i32,
        y: i32,
        double_trunk: bool,
        (hanging_chance, extension_chance): (f32, f32),
    ) {
        self.leaves_row(origin, radius, y, double_trunk);
        let offset = i32::from(double_trunk);
        let log_pos = (origin.0, origin.1 - 1, origin.2);
        for along in 0..4 {
            let along_edge = HORIZONTAL[along];
            // `getClockWise`：水平方向表里的下一个。
            // `getClockWise`: the next entry of the horizontal table.
            let to_edge = HORIZONTAL[(along + 1) % 4];
            // EAST 和 SOUTH 是正方向。
            // EAST and SOUTH are the positive directions.
            let positive = to_edge.0 + to_edge.1 > 0;
            let offset_to_edge = if positive { radius + offset } else { radius };
            let mut pos = (
                origin.0 + to_edge.0 * offset_to_edge - along_edge.0 * radius,
                origin.1 + y - 1,
                origin.2 + to_edge.1 * offset_to_edge - along_edge.1 * radius,
            );
            for _ in -radius..radius + offset {
                let leaves_above = self.place.foliage.contains((pos.0, pos.1 + 1, pos.2));
                if leaves_above && self.try_place_extension(hanging_chance, log_pos, pos) {
                    let below = (pos.0, pos.1 - 1, pos.2);
                    self.try_place_extension(extension_chance, log_pos, below);
                }
                pos.0 += along_edge.0;
                pos.2 += along_edge.1;
            }
        }
    }

    /// 官方 `FoliagePlacer.tryPlaceExtension`。
    ///
    /// Official `FoliagePlacer.tryPlaceExtension`.
    fn try_place_extension(&mut self, chance: f32, log_pos: Pos, pos: Pos) -> bool {
        let distance =
            (pos.0 - log_pos.0).abs() + (pos.1 - log_pos.1).abs() + (pos.2 - log_pos.2).abs();
        if distance >= 7 {
            return false;
        }
        if self.random.next_float() > chance {
            return false;
        }
        self.try_place_leaf(pos)
    }

    /// 官方 `FoliagePlacer.tryPlaceLeaf`。
    ///
    /// Official `FoliagePlacer.tryPlaceLeaf`.
    fn try_place_leaf(&mut self, pos: Pos) -> bool {
        let current = self.place.get_block(pos);
        let persistent = property_value(current, "persistent") == Some("true");
        if persistent || !valid_tree_pos(self.place.world, pos) {
            return false;
        }
        let Some(mut state) =
            self.config
                .foliage_provider
                .get_optional_state(self.place.world, self.random, pos)
        else {
            return false;
        };
        if has_property(state, "waterlogged") {
            let water = is_water_source(self.place.get_block(pos));
            state = with_property(state, "waterlogged", if water { "true" } else { "false" })
                .unwrap_or(state);
        }
        self.place.set_foliage(pos, state);
        true
    }
}

/// `fluidState.isSourceOfType(Fluids.WATER)`。
fn is_water_source(state: u32) -> bool {
    let fluid = state_fluid(state);
    fluid.source && fluid.fluid == registries::fluid::by_name("minecraft:water").unwrap_or(-1)
}

/// 白杨一次 `createFoliage` 里不变的参数。
///
/// The parameters that stay fixed during one poplar `createFoliage`.
struct Poplar {
    side_hole_chance: f32,
    foliage_height: i32,
    flip: bool,
    double_trunk: bool,
}

impl Poplar {
    /// `shouldRowBePartialRhombusShape`。
    fn partial_row(&self, y: i32) -> bool {
        self.foliage_height - 1 == y || self.foliage_height - 2 == y
    }

    /// `getCornerBlocksToCutForRhombusShape`。
    fn corner_blocks_to_cut(&self, dx: i32, dz: i32, radius: i32, partial: bool) -> i32 {
        let small_corner = if self.flip {
            dx > 0 && dz > 0 || dz < 0 && dx < 0
        } else {
            dx > 0 && dz < 0 || dz > 0 && dx < 0
        };
        if small_corner {
            radius - 1
        } else if partial {
            radius + 1
        } else {
            radius
        }
    }

    /// 官方 `PoplarFoliagePlacer.shouldSkipLocation`（带上下文的那个重载）。
    ///
    /// Official `PoplarFoliagePlacer.shouldSkipLocation` (the overload with context).
    fn should_skip_location(
        &self,
        random: &WorldgenRandom,
        dx: i32,
        y: i32,
        dz: i32,
        radius: i32,
    ) -> bool {
        let partial = self.partial_row(y);
        let corner = self.corner_blocks_to_cut(dx, dz, radius, partial);
        let (abs_dx, abs_dz) = (dx.abs(), dz.abs());
        if partial && (abs_dx == radius || abs_dz == radius) {
            return true;
        }
        let side_removal = i32::from(random.next_float() <= self.side_hole_chance);
        !within_rhombus(radius, abs_dx, abs_dz, corner, side_removal)
    }
}

/// `PoplarFoliagePlacer.isWithinRhombusShape`。
fn within_rhombus(radius: i32, abs_dx: i32, abs_dz: i32, corner: i32, side_removal: i32) -> bool {
    abs_dx + abs_dz <= radius * 2 - (corner + side_removal)
}
