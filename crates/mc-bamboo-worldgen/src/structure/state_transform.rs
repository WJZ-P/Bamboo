//! 结构件放置时的方块状态 `mirror` / `rotate` 变换（`StructurePiece.placeBlock` 和
//! `StructureTemplate.placeInWorld` 的那一步）。
//!
//! 官方先 `blockState.mirror(mirror)` 再 `blockState.rotate(rotation)`；默认实现是恒等
//! （`BlockBehaviour.java:251-257`），重写了的方块类按做法分成 [`Rule`] 的几类，按方块类
//! 和祖先类查出来。没重写的方块原样返回。
//!
//! The `mirror` / `rotate` state transforms of `StructurePiece.placeBlock` and
//! `StructureTemplate.placeInWorld`. Vanilla applies `blockState.mirror(mirror)` then
//! `blockState.rotate(rotation)`; the defaults are identity (`BlockBehaviour.java:251-257`)
//! and the overriding block classes are grouped by behaviour into [`Rule`], looked up from
//! the block class and its ancestors. Blocks without an override pass through.

use mc_bamboo_data::{block_id_of_state, block_of_state};

use crate::structure::piece_place::with_property;

/// 官方 `Mirror`。
///
/// Vanilla `Mirror`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mirror {
    None,
    LeftRight,
    FrontBack,
}

/// 官方 `Rotation`。
///
/// Vanilla `Rotation`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rotation {
    None,
    Clockwise90,
    Clockwise180,
    CounterClockwise90,
}

impl Mirror {
    /// `Mirror.getRotation(direction)`：把某个水平朝向按镜像得到对应的旋转。
    ///
    /// `Mirror.getRotation(direction)`: the rotation a mirrored horizontal facing implies.
    pub fn get_rotation(self, facing: &str) -> Rotation {
        match (self, facing) {
            (Self::None, _) => Rotation::None,
            (Self::LeftRight, "north" | "south") => Rotation::Clockwise180,
            (Self::FrontBack, "west" | "east") => Rotation::Clockwise180,
            _ => Rotation::None,
        }
    }

    /// `Direction.getSerializedName()` 级别的镜像：水平四方向。
    ///
    /// Mirror one horizontal facing name.
    pub fn mirror_facing(self, facing: &str) -> &'static str {
        match (self, facing) {
            (Self::LeftRight, "north") => "south",
            (Self::LeftRight, "south") => "north",
            (Self::FrontBack, "west") => "east",
            (Self::FrontBack, "east") => "west",
            (_, "north") => "north",
            (_, "south") => "south",
            (_, "west") => "west",
            _ => "east",
        }
    }

    /// `Mirror.mirror(Direction)`：六方向，镜像轴上的方向取反，其余不变。
    ///
    /// `Mirror.mirror(Direction)` over all six directions: directions on the mirrored
    /// axis flip, the rest stay.
    pub fn mirror_direction(self, direction: &str) -> &'static str {
        match direction {
            "up" => "up",
            "down" => "down",
            other => self.mirror_facing(other),
        }
    }
}

impl Rotation {
    /// `Rotation.rotate(Direction)`：水平四方向顺时针转，`up` / `down` 不变。
    ///
    /// `Rotation.rotate(Direction)`: the four horizontal facings turn clockwise, `up` /
    /// `down` stay.
    pub fn rotate_facing(self, facing: &str) -> &'static str {
        match (self, facing) {
            (_, "up") => "up",
            (_, "down") => "down",
            (Self::None, "north") => "north",
            (Self::None, "south") => "south",
            (Self::None, "west") => "west",
            (Self::None, "east") => "east",
            (Self::Clockwise90, "north") => "east",
            (Self::Clockwise90, "east") => "south",
            (Self::Clockwise90, "south") => "west",
            (Self::Clockwise90, "west") => "north",
            (Self::Clockwise180, "north") => "south",
            (Self::Clockwise180, "south") => "north",
            (Self::Clockwise180, "west") => "east",
            (Self::Clockwise180, "east") => "west",
            (Self::CounterClockwise90, "north") => "west",
            (Self::CounterClockwise90, "west") => "south",
            (Self::CounterClockwise90, "south") => "east",
            _ => "north",
        }
    }
}

/// `BlockState.mirror(mirror)` 再 `BlockState.rotate(rotation)`。
///
/// `BlockState.mirror(mirror)` followed by `BlockState.rotate(rotation)`.
pub fn transform(state: u32, mirror: Mirror, rotation: Rotation) -> u32 {
    let mirrored = apply_mirror(state, mirror);
    apply_rotation(mirrored, rotation)
}

/// 官方重写了 `rotate` / `mirror` 的方块按做法分成的几类。
///
/// The ways vanilla blocks override `rotate` / `mirror`, grouped by behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rule {
    /// `FACING` 按 `rotation.rotate` 转；镜像是 `state.rotate(mirror.getRotation(FACING))`。
    ///
    /// `FACING` turns by `rotation.rotate`; mirroring is
    /// `state.rotate(mirror.getRotation(FACING))`.
    Facing,
    /// `AnvilBlock`：只重写了 `rotate`，镜像恒等。
    ///
    /// `AnvilBlock`: only `rotate` is overridden, mirroring is identity.
    FacingRotateOnly,
    /// `RodBlock`：转 `FACING`，镜像是 `mirror.mirror(FACING)`。
    ///
    /// `RodBlock`: turns `FACING`, mirroring is `mirror.mirror(FACING)`.
    Rod,
    /// `DoorBlock`：同 [`Rule::Facing`]，镜像之后再 `cycle(HINGE)`。
    ///
    /// `DoorBlock`: like [`Rule::Facing`], then `cycle(HINGE)` after mirroring.
    Door,
    /// `StairBlock`。
    ///
    /// `StairBlock`.
    Stair,
    /// 三种铁轨的 `shape`。
    ///
    /// The `shape` of the three rails.
    Rail,
    /// 四面属性（`CrossCollisionBlock`、`WallBlock`、`VineBlock`、`TripWireBlock`、
    /// `RedstoneWireBlock`、`MossyCarpetBlock`）。
    ///
    /// Four side properties (`CrossCollisionBlock`, `WallBlock`, `VineBlock`,
    /// `TripWireBlock`, `RedstoneWireBlock`, `MossyCarpetBlock`).
    Sides,
    /// 六面布尔属性按方向映射（`HugeMushroomBlock`、`MultifaceBlock`）。
    ///
    /// Six face booleans remapped by direction (`HugeMushroomBlock`, `MultifaceBlock`).
    Faces,
    /// `RotatedPillarBlock.rotatePillar`（以及同样写法的下界传送门），镜像恒等。
    ///
    /// `RotatedPillarBlock.rotatePillar` (and the nether portal's equivalent), mirroring
    /// is identity.
    Pillar,
    /// 16 档的 `ROTATION`（旗帜、立着的告示牌和悬挂告示牌、头颅）。
    ///
    /// The 16-step `ROTATION` (banners, standing and ceiling hanging signs, skulls).
    Rotation16,
    /// `ORIENTATION`（`FrontAndTop`：拼图方块、合成器）。
    ///
    /// `ORIENTATION` (`FrontAndTop`: jigsaw, crafter).
    Orientation,
    /// 默认实现：恒等。
    ///
    /// The default: identity.
    Identity,
}

/// 按方块类和它的祖先类找出官方用的是哪一种重写。
///
/// 祖先表是无序的，所以先查会被子类再次重写的那些（门、楼梯、铁砧……），最后才查
/// `HorizontalDirectionalBlock` 这类最宽的。
///
/// Find which vanilla override applies from the block class and its ancestors. The
/// ancestor table is unordered, so the narrower overrides (door, stairs, anvil, ...) are
/// checked before broad ones such as `HorizontalDirectionalBlock`.
fn rule_of(state: u32) -> Rule {
    let Some(block) = block_of_state(state) else {
        return Rule::Identity;
    };
    let class = format!("{:?}", block.class);
    let names: Vec<String> = std::iter::once(class)
        .chain(block.class.ancestors().iter().map(|a| format!("{a:?}")))
        .collect();
    let has = |wanted: &[&str]| names.iter().any(|name| wanted.contains(&name.as_str()));
    if has(&["DoorBlock"]) {
        Rule::Door
    } else if has(&["StairBlock"]) {
        Rule::Stair
    } else if has(&["RailBlock", "PoweredRailBlock", "DetectorRailBlock"]) {
        Rule::Rail
    } else if has(&[
        "CrossCollisionBlock",
        "WallBlock",
        "VineBlock",
        "TripWireBlock",
        "RedstoneWireBlock",
        "MossyCarpetBlock",
    ]) {
        Rule::Sides
    } else if has(&["HugeMushroomBlock", "MultifaceBlock"]) {
        Rule::Faces
    } else if has(&[
        "RotatedPillarBlock",
        "InfestedRotatedPillarBlock",
        "CreakingHeartBlock",
        "NetherPortalBlock",
    ]) {
        Rule::Pillar
    } else if has(&[
        "BannerBlock",
        "StandingSignBlock",
        "CeilingHangingSignBlock",
        "SkullBlock",
    ]) {
        Rule::Rotation16
    } else if has(&["JigsawBlock", "CrafterBlock"]) {
        Rule::Orientation
    } else if has(&["RodBlock"]) {
        Rule::Rod
    } else if has(&["AnvilBlock"]) {
        Rule::FacingRotateOnly
    } else if has(&[
        "HorizontalDirectionalBlock",
        "AbstractFurnaceBlock",
        "AmethystClusterBlock",
        "AttachedStemBlock",
        "BarrelBlock",
        "BaseCoralWallFanBlock",
        "BeehiveBlock",
        "BellBlock",
        "CalibratedSculkSensorBlock",
        "CampfireBlock",
        "ChestBlock",
        "ChiseledBookShelfBlock",
        "CommandBlock",
        "CopperGolemStatueBlock",
        "DecoratedPotBlock",
        "DispenserBlock",
        "EnderChestBlock",
        "EndPortalFrameBlock",
        "FlowerBedBlock",
        "GrindstoneBlock",
        "HopperBlock",
        "LadderBlock",
        "LeafLitterBlock",
        "LecternBlock",
        "ObserverBlock",
        "RedstoneWallTorchBlock",
        "ShelfBlock",
        "ShulkerBoxBlock",
        "SmallDripleafBlock",
        "StonecutterBlock",
        "TripWireHookBlock",
        "VaultBlock",
        "WallBannerBlock",
        "WallHangingSignBlock",
        "WallSignBlock",
        "WallSkullBlock",
        "WallTorchBlock",
    ]) {
        Rule::Facing
    } else {
        Rule::Identity
    }
}

/// `BlockState.mirror(mirror)`。
///
/// `BlockState.mirror(mirror)`.
pub fn apply_mirror(state: u32, mirror: Mirror) -> u32 {
    if mirror == Mirror::None {
        return state;
    }
    match rule_of(state) {
        Rule::Facing => mirror_facing(state, mirror),
        // `DoorBlock.mirror`：先按 `mirror.getRotation(FACING)` 旋转，再 `cycle(HINGE)`。
        // `DoorBlock.mirror`: rotate by `mirror.getRotation(FACING)` then `cycle(HINGE)`.
        Rule::Door => cycle_hinge(mirror_facing(state, mirror)),
        Rule::Rod => {
            let Some(facing) = property_of(state, "facing") else {
                return state;
            };
            with_property(state, "facing", mirror.mirror_direction(facing)).unwrap_or(state)
        }
        // `StairBlock.mirror`：Z 轴方向（LEFT_RIGHT）或 X 轴方向（FRONT_BACK）时整体转 180°
        // 并按镜像翻转 `shape`。
        // `StairBlock.mirror`: turn 180° and flip `shape` when the axis matches.
        Rule::Stair => mirror_stairs(state, mirror),
        // LEFT_RIGHT 交换 north/south，FRONT_BACK 交换 east/west。
        // LEFT_RIGHT swaps north/south, FRONT_BACK east/west.
        Rule::Sides => {
            let pairs: &[(&str, &str)] = match mirror {
                Mirror::LeftRight => &[("north", "south"), ("south", "north")],
                Mirror::FrontBack => &[("east", "west"), ("west", "east")],
                Mirror::None => &[],
            };
            swap_properties(state, pairs)
        }
        Rule::Faces => map_faces(state, |d| mirror.mirror_direction(d)),
        // `BaseRailBlock.mirror`：按 `LEFT_RIGHT` / `FRONT_BACK` 翻转 `shape`。
        // `BaseRailBlock.mirror`: flip `shape` for `LEFT_RIGHT` / `FRONT_BACK`.
        Rule::Rail => {
            let Some(shape) = property_of(state, "shape") else {
                return state;
            };
            let new_shape = mirror_rail(shape, mirror);
            with_property(state, "shape", new_shape).unwrap_or(state)
        }
        // `Mirror.mirror(rotation, 16)`。
        // `Mirror.mirror(rotation, 16)`.
        Rule::Rotation16 => map_rotation16(state, |r| {
            let corrected = if r > 8 { r - 16 } else { r };
            match mirror {
                Mirror::LeftRight => (8 - corrected + 16) % 16,
                Mirror::FrontBack => (16 - corrected) % 16,
                Mirror::None => r,
            }
        }),
        // `mirror.rotation()` 是 `INVERT_Z` / `INVERT_X`，作用在方向上就是 `mirror.mirror`。
        // `mirror.rotation()` is `INVERT_Z` / `INVERT_X`, which acts on a direction like
        // `mirror.mirror`.
        Rule::Orientation => map_orientation(state, |d| mirror.mirror_direction(d)),
        Rule::FacingRotateOnly | Rule::Pillar | Rule::Identity => state,
    }
}

/// `BlockState.rotate(rotation)`。
///
/// `BlockState.rotate(rotation)`.
pub fn apply_rotation(state: u32, rotation: Rotation) -> u32 {
    if rotation == Rotation::None {
        return state;
    }
    match rule_of(state) {
        // 这几类都只转 `facing`（`DoorBlock.rotate` 和 `StairBlock.rotate` 不动 `hinge` /
        // `shape`）。
        // These only turn `facing` (`DoorBlock.rotate` and `StairBlock.rotate` leave
        // `hinge` / `shape` alone).
        Rule::Facing | Rule::FacingRotateOnly | Rule::Rod | Rule::Door | Rule::Stair => {
            rotate_facing_state(state, rotation)
        }
        // 四面属性的四个分支照抄。
        // The four side-property branches, transcribed.
        Rule::Sides => {
            let pairs: &[(&str, &str)] = match rotation {
                Rotation::Clockwise180 => &[
                    ("north", "south"),
                    ("east", "west"),
                    ("south", "north"),
                    ("west", "east"),
                ],
                Rotation::CounterClockwise90 => &[
                    ("north", "east"),
                    ("east", "south"),
                    ("south", "west"),
                    ("west", "north"),
                ],
                Rotation::Clockwise90 => &[
                    ("north", "west"),
                    ("east", "north"),
                    ("south", "east"),
                    ("west", "south"),
                ],
                Rotation::None => &[],
            };
            swap_properties(state, pairs)
        }
        Rule::Faces => map_faces(state, |d| rotation.rotate_facing(d)),
        // `BaseRailBlock.rotate`：转 `shape`。
        // `BaseRailBlock.rotate` turns `shape`.
        Rule::Rail => {
            let Some(shape) = property_of(state, "shape") else {
                return state;
            };
            let new_shape = rotate_rail(shape, rotation);
            with_property(state, "shape", new_shape).unwrap_or(state)
        }
        // 90 度时 `axis` 的 X↔Z 互换，180 度与不转时不变。
        // 90 degrees swaps axis X and Z; 180 and none do not.
        Rule::Pillar => rotate_pillar(state, rotation),
        // `Rotation.rotate(rotation, 16)`。
        // `Rotation.rotate(rotation, 16)`.
        Rule::Rotation16 => map_rotation16(state, |r| {
            let steps = match rotation {
                Rotation::Clockwise90 => 4,
                Rotation::Clockwise180 => 8,
                Rotation::CounterClockwise90 => 12,
                Rotation::None => 0,
            };
            (r + steps) % 16
        }),
        Rule::Orientation => map_orientation(state, |d| rotation.rotate_facing(d)),
        Rule::Identity => state,
    }
}

/// `BaseRailBlock.mirror(RailShape, Mirror)`。
///
/// `BaseRailBlock.mirror(RailShape, Mirror)`.
fn mirror_rail(shape: &str, mirror: Mirror) -> &'static str {
    match mirror {
        Mirror::None => keep_shape(shape),
        // `LEFT_RIGHT`：南北翻成相反，对角线的南北分量互换。
        // `LEFT_RIGHT`: north/south flip, and the diagonal shapes swap their north/south.
        Mirror::LeftRight => match shape {
            "ascending_north" => "ascending_south",
            "ascending_south" => "ascending_north",
            "south_east" => "north_east",
            "south_west" => "north_west",
            "north_west" => "south_west",
            "north_east" => "south_east",
            other => keep_shape(other),
        },
        // `FRONT_BACK`：东西翻成相反，对角线的东西分量互换。
        // `FRONT_BACK`: east/west flip, and the diagonal shapes swap their east/west.
        Mirror::FrontBack => match shape {
            "ascending_east" => "ascending_west",
            "ascending_west" => "ascending_east",
            "south_east" => "south_west",
            "south_west" => "south_east",
            "north_west" => "north_east",
            "north_east" => "north_west",
            other => keep_shape(other),
        },
    }
}

/// 形状名到同名静态串。
///
/// Map a shape name to the same static string.
fn keep_shape(shape: &str) -> &'static str {
    match shape {
        "north_south" => "north_south",
        "east_west" => "east_west",
        "ascending_east" => "ascending_east",
        "ascending_west" => "ascending_west",
        "ascending_north" => "ascending_north",
        "ascending_south" => "ascending_south",
        "south_east" => "south_east",
        "south_west" => "south_west",
        "north_west" => "north_west",
        _ => "north_east",
    }
}
///
/// `state.rotate(mirror.getRotation(FACING))`: the single-`facing` mirror path.
fn mirror_facing(state: u32, mirror: Mirror) -> u32 {
    let Some(facing) = property_of(state, "facing") else {
        return state;
    };
    let implied = mirror.get_rotation(facing);
    with_property(state, "facing", implied.rotate_facing(facing)).unwrap_or(state)
}

/// `StairBlock.mirror`：`LEFT_RIGHT` 且朝 Z 轴、或 `FRONT_BACK` 且朝 X 轴时，
/// 整体 `rotate(CLOCKWISE_180)` 并按 `shape` 翻转；其它情况走父类（恒等）。
///
/// `StairBlock.mirror`: with `LEFT_RIGHT` on a Z facing or `FRONT_BACK` on an X facing,
/// rotate 180° and flip `shape`; otherwise the parent (identity).
fn mirror_stairs(state: u32, mirror: Mirror) -> u32 {
    let Some(facing) = property_of(state, "facing") else {
        return state;
    };
    let axis_z = matches!(facing, "north" | "south");
    let axis_x = matches!(facing, "east" | "west");
    let applies = match mirror {
        Mirror::LeftRight => axis_z,
        Mirror::FrontBack => axis_x,
        Mirror::None => false,
    };
    if !applies {
        return state;
    }
    let shape = property_of(state, "shape").unwrap_or("straight");
    // `LEFT_RIGHT`（Z 轴）：outer/inner 的左右互换；`FRONT_BACK`（X 轴）：
    // straight / inner 不变，outer 左右互换。
    // `LEFT_RIGHT` (Z axis) swaps the left/right of outer and inner; `FRONT_BACK`
    // (X axis) keeps straight/inner and swaps outer.
    let new_shape = match (mirror, shape) {
        (Mirror::LeftRight, "outer_left") => "outer_right",
        (Mirror::LeftRight, "outer_right") => "outer_left",
        (Mirror::LeftRight, "inner_left") => "inner_right",
        (Mirror::LeftRight, "inner_right") => "inner_left",
        (Mirror::FrontBack, "outer_left") => "outer_right",
        (Mirror::FrontBack, "outer_right") => "outer_left",
        _ => shape,
    };
    let rotated = rotate_facing_state(state, Rotation::Clockwise180);
    with_property(rotated, "shape", new_shape).unwrap_or(rotated)
}

/// 只旋转 `facing` 的那一半（`StairBlock.rotate` 的行为）。
///
/// Just the `facing` half (`StairBlock.rotate`'s behaviour).
fn rotate_facing_state(state: u32, rotation: Rotation) -> u32 {
    let Some(facing) = property_of(state, "facing") else {
        return state;
    };
    with_property(state, "facing", rotation.rotate_facing(facing)).unwrap_or(state)
}

/// `MultifaceBlock.mapDirections` / `HugeMushroomBlock` 的六面映射：新的 `map(d)` 面取旧的
/// `d` 面，所有面同时写。
///
/// The six-face remap of `MultifaceBlock.mapDirections` / `HugeMushroomBlock`: the new
/// `map(d)` face takes the old `d` face, all written at once.
fn map_faces(state: u32, map: impl Fn(&str) -> &'static str) -> u32 {
    let mut pairs: Vec<(&str, &str)> = Vec::with_capacity(6);
    for direction in ["down", "up", "north", "south", "west", "east"] {
        if property_of(state, direction).is_some() {
            pairs.push((map(direction), direction));
        }
    }
    swap_properties(state, &pairs)
}

/// 改写 0..16 的 `rotation` 属性。
///
/// Rewrite the 0..16 `rotation` property.
fn map_rotation16(state: u32, map: impl Fn(i32) -> i32) -> u32 {
    let Some(value) = property_of(state, "rotation").and_then(|v| v.parse::<i32>().ok()) else {
        return state;
    };
    with_property(state, "rotation", &map(value).to_string()).unwrap_or(state)
}

/// 改写 `FrontAndTop` 的 `orientation`（形如 `north_up`）：前、上两个方向各自映射。
///
/// Rewrite a `FrontAndTop` `orientation` (like `north_up`): map the front and the top
/// direction each.
fn map_orientation(state: u32, map: impl Fn(&str) -> &'static str) -> u32 {
    let Some((front, top)) = property_of(state, "orientation").and_then(|v| v.split_once('_'))
    else {
        return state;
    };
    let value = format!("{}_{}", map(front), map(top));
    with_property(state, "orientation", &value).unwrap_or(state)
}

/// `RotatedPillarBlock.rotatePillar`。
///
/// `RotatedPillarBlock.rotatePillar`.
fn rotate_pillar(state: u32, rotation: Rotation) -> u32 {
    if !matches!(
        rotation,
        Rotation::Clockwise90 | Rotation::CounterClockwise90
    ) {
        return state;
    }
    let Some(axis) = property_of(state, "axis") else {
        return state;
    };
    let swapped = match axis {
        "x" => "z",
        "z" => "x",
        _ => return state,
    };
    with_property(state, "axis", swapped).unwrap_or(state)
}

/// `BaseRailBlock.rotate(RailShape, Rotation)`。
///
/// `BaseRailBlock.rotate(RailShape, Rotation)`.
fn rotate_rail(shape: &str, rotation: Rotation) -> &'static str {
    match rotation {
        Rotation::None => keep(shape),
        Rotation::Clockwise180 => match shape {
            "ascending_east" => "ascending_west",
            "ascending_west" => "ascending_east",
            "ascending_north" => "ascending_south",
            "ascending_south" => "ascending_north",
            "south_east" => "north_west",
            "south_west" => "north_east",
            "north_west" => "south_east",
            "north_east" => "south_west",
            _ => keep(shape),
        },
        Rotation::CounterClockwise90 => match shape {
            "ascending_east" => "ascending_north",
            "ascending_west" => "ascending_south",
            "ascending_north" => "ascending_west",
            "ascending_south" => "ascending_east",
            "north_south" => "east_west",
            "east_west" => "north_south",
            "south_east" => "north_east",
            "south_west" => "south_east",
            "north_west" => "south_west",
            "north_east" => "north_west",
            _ => keep(shape),
        },
        Rotation::Clockwise90 => match shape {
            "ascending_east" => "ascending_south",
            "ascending_west" => "ascending_north",
            "ascending_north" => "ascending_east",
            "ascending_south" => "ascending_west",
            "north_south" => "east_west",
            "east_west" => "north_south",
            "south_east" => "south_west",
            "south_west" => "north_west",
            "north_west" => "north_east",
            "north_east" => "south_east",
            _ => keep(shape),
        },
    }
}

/// 形状名到同名静态串（`keep` 只处理已知的十个）。
///
/// Map a shape name to the same static string (only the ten known shapes).
fn keep(shape: &str) -> &'static str {
    match shape {
        "north_south" => "north_south",
        "east_west" => "east_west",
        "ascending_east" => "ascending_east",
        "ascending_west" => "ascending_west",
        "ascending_north" => "ascending_north",
        "ascending_south" => "ascending_south",
        "south_east" => "south_east",
        "south_west" => "south_west",
        "north_west" => "north_west",
        _ => "north_east",
    }
}

/// `with_property` 的多属性版：按给定顺序逐个设置，任一失败就保留原状态。
///
/// Multi-property `with_property`: apply each pair in order, keeping the state as-is if
/// any one fails.
pub fn with_props(state: u32, pairs: &[(&str, &str)]) -> u32 {
    let mut current = state;
    for (key, value) in pairs {
        current = with_property(current, key, value).unwrap_or(current);
    }
    current
}

/// `BlockState.cycle(HINGE)`：上下半扇互换。
///
/// `BlockState.cycle(HINGE)`: swap the two halves of the hinge.
fn cycle_hinge(state: u32) -> u32 {
    let Some(hinge) = property_of(state, "hinge") else {
        return state;
    };
    let flipped = if hinge == "left" { "right" } else { "left" };
    with_property(state, "hinge", flipped).unwrap_or(state)
}

/// 读一个属性值。
///
/// Read one property value.
pub fn property_of(state: u32, name: &str) -> Option<&'static str> {
    let block = block_of_state(state)?;
    let mut stride = 1_u32;
    for property in block.properties.iter().rev() {
        let count = property.values.len() as u32;
        if property.name == name {
            let index = (state - block.first_state) / stride % count;
            return Some(property.values[index as usize]);
        }
        stride *= count;
    }
    None
}

/// 一次性交换若干属性对（官方 `setValue` 链是同时的，不是顺序的）。
///
/// Swap several property pairs at once (vanilla's `setValue` chain is simultaneous).
fn swap_properties(state: u32, pairs: &[(&str, &str)]) -> u32 {
    if pairs.is_empty() {
        return state;
    }
    let Some(block) = block_of_state(state) else {
        return state;
    };
    // 先把原值全读出来，再一起写，避免链式覆盖。
    // Read every source value first, then write them together to avoid chain clobbering.
    let mut updates: Vec<(String, String)> = Vec::with_capacity(pairs.len());
    for (target, source) in pairs {
        if let Some(value) = property_of(state, source) {
            updates.push(((*target).to_owned(), value.to_owned()));
        }
    }
    let mut current = state;
    let _ = block;
    for (key, value) in updates {
        current = with_property(current, &key, &value).unwrap_or(current);
    }
    current
}

/// 从 `setOrientation` 的方向推出 `(mirror, rotation)`。
///
/// The `(mirror, rotation)` pair `setOrientation` derives from an orientation.
pub fn from_orientation(orientation: Option<crate::structure::pieces::HDir>) -> (Mirror, Rotation) {
    use crate::structure::pieces::HDir;
    match orientation {
        None => (Mirror::None, Rotation::None),
        Some(HDir::South) => (Mirror::LeftRight, Rotation::None),
        Some(HDir::West) => (Mirror::LeftRight, Rotation::Clockwise90),
        Some(HDir::East) => (Mirror::None, Rotation::Clockwise90),
        Some(HDir::North) => (Mirror::None, Rotation::None),
    }
}

/// 状态所属方块名；查不到返回 `None`。
///
/// Block name of a state; `None` when unknown.
pub fn name_of(state: u32) -> Option<&'static str> {
    let id = block_id_of_state(state)?;
    Some(mc_bamboo_data::blocks::BLOCKS[id].name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structure::piece_place::block;

    #[test]
    fn set_orientation_pairs_match_vanilla() {
        use crate::structure::pieces::HDir;
        assert_eq!(
            from_orientation(Some(HDir::North)),
            (Mirror::None, Rotation::None)
        );
        assert_eq!(
            from_orientation(Some(HDir::South)),
            (Mirror::LeftRight, Rotation::None)
        );
        assert_eq!(
            from_orientation(Some(HDir::East)),
            (Mirror::None, Rotation::Clockwise90)
        );
        assert_eq!(
            from_orientation(Some(HDir::West)),
            (Mirror::LeftRight, Rotation::Clockwise90)
        );
        assert_eq!(from_orientation(None), (Mirror::None, Rotation::None));
    }

    #[test]
    fn wall_torch_facing_rotates_and_mirrors() {
        let torch = with_property(block("minecraft:wall_torch"), "facing", "north").unwrap();
        // 顺时针 90 度：north -> east。
        // Clockwise 90: north -> east.
        assert_eq!(
            property_of(apply_rotation(torch, Rotation::Clockwise90), "facing"),
            Some("east")
        );
        // LEFT_RIGHT 镜像 north -> south（`getRotation` 给 180 度）。
        // LEFT_RIGHT mirror turns north into south via a 180-degree rotation.
        assert_eq!(
            property_of(apply_mirror(torch, Mirror::LeftRight), "facing"),
            Some("south")
        );
        // FRONT_BACK 对 north/south 无影响。
        // FRONT_BACK leaves north/south alone.
        assert_eq!(
            property_of(apply_mirror(torch, Mirror::FrontBack), "facing"),
            Some("north")
        );
    }

    #[test]
    fn fence_sides_swap_when_mirrored() {
        let fence = block("minecraft:oak_fence");
        let north = with_property(fence, "north", "true").unwrap();
        // LEFT_RIGHT 交换 north / south。
        // LEFT_RIGHT swaps north and south.
        let mirrored = apply_mirror(north, Mirror::LeftRight);
        assert_eq!(property_of(mirrored, "south"), Some("true"));
        assert_eq!(property_of(mirrored, "north"), Some("false"));

        // CLOCKWISE_90 里 `north` 的新值来自旧 `west`，所以旧 `north` 转到 `east`。
        // In CLOCKWISE_90 the new `north` comes from the old `west`, so an old `north`
        // becomes `east`.
        let rotated = apply_rotation(north, Rotation::Clockwise90);
        assert_eq!(property_of(rotated, "east"), Some("true"));
        assert_eq!(property_of(rotated, "north"), Some("false"));
    }

    #[test]
    fn rail_shapes_rotate_like_vanilla() {
        let rail = block("minecraft:rail");
        let east_west = with_property(rail, "shape", "east_west").unwrap();
        // 90 度旋转交换南北 / 东西。
        // A 90-degree rotation swaps north_south and east_west.
        assert_eq!(
            property_of(apply_rotation(east_west, Rotation::Clockwise90), "shape"),
            Some("north_south")
        );
        assert_eq!(
            property_of(apply_rotation(east_west, Rotation::Clockwise180), "shape"),
            Some("east_west")
        );
    }

    #[test]
    fn blocks_without_overrides_pass_through() {
        let planks = block("minecraft:oak_planks");
        assert_eq!(apply_mirror(planks, Mirror::LeftRight), planks);
        assert_eq!(apply_rotation(planks, Rotation::Clockwise90), planks);
    }
}
