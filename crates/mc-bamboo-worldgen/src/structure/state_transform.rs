//! 结构件放置时的方块状态 `mirror` / `rotate` 变换（`StructurePiece.placeBlock` 的那一步）。
//!
//! 官方 `StructurePiece.placeBlock` 在 `setBlock` 之前对状态依次做
//! `blockState.mirror(this.mirror)` 和 `blockState.rotate(this.rotation)`，而
//! `mirror` / `rotate` 的默认实现是恒等（`BlockBehaviour.java:251-257`），只有少数方块重写。
//! 废弃矿井用到的三类会变：栅栏（`CrossCollisionBlock` 的四面）、墙上的火把（`facing`）、
//! 铁轨（`shape`）。其它方块（原木、木板、蜘蛛网、刷怪笼、铁链）恒等。
//!
//! 这里只实现这三类；遇到没重写的方块原样返回，和官方默认行为一致。
//!
//! The `mirror` / `rotate` state transforms of `StructurePiece.placeBlock`. Vanilla applies
//! `blockState.mirror(this.mirror)` then `blockState.rotate(this.rotation)` before
//! `setBlock`; the defaults are identity (`BlockBehaviour.java:251-257`) and only a few
//! blocks override them. Three matter for the mineshaft: fences (`CrossCollisionBlock`'s
//! four sides), wall torches (`facing`) and rails (`shape`). Everything else (logs,
//! planks, cobweb, spawner, iron chain) is identity. Only those three are implemented;
//! unhandled blocks pass through, matching the vanilla default.

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
}

impl Rotation {
    /// `Rotation.rotate(Direction)`：水平四方向顺时针转。
    ///
    /// `Rotation.rotate(Direction)` for the four horizontal facings.
    pub fn rotate_facing(self, facing: &str) -> &'static str {
        match (self, facing) {
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

/// `BlockState.mirror(mirror)`。
///
/// `BlockState.mirror(mirror)`.
pub fn apply_mirror(state: u32, mirror: Mirror) -> u32 {
    if mirror == Mirror::None {
        return state;
    }
    let Some(block) = block_of_state(state) else {
        return state;
    };
    match block.name {
        // `WallTorchBlock.mirror`：`state.rotate(mirror.getRotation(FACING))`。
        // `ButtonBlock` 继承 `HorizontalDirectionalBlock`，实现相同。
        // `WallTorchBlock.mirror`, and `ButtonBlock` via `HorizontalDirectionalBlock`.
        n if is_horizontal_facing(n) => {
            let Some(facing) = property_of(state, "facing") else {
                return state;
            };
            let implied = mirror.get_rotation(facing);
            with_property(state, "facing", implied.rotate_facing(facing)).unwrap_or(state)
        }
        // `DoorBlock.mirror`：先按 `mirror.getRotation(FACING)` 旋转，再 `cycle(HINGE)`。
        // `DoorBlock.mirror`: rotate by `mirror.getRotation(FACING)` then `cycle(HINGE)`.
        n if is_door(n) => {
            let Some(facing) = property_of(state, "facing") else {
                return state;
            };
            let implied = mirror.get_rotation(facing);
            let rotated =
                with_property(state, "facing", implied.rotate_facing(facing)).unwrap_or(state);
            cycle_hinge(rotated)
        }
        // `CrossCollisionBlock.mirror`：LEFT_RIGHT 交换 north/south，FRONT_BACK 交换 east/west。
        // `CrossCollisionBlock.mirror`: LEFT_RIGHT swaps north/south, FRONT_BACK east/west.
        name if is_cross_collision(name) => {
            let pairs: &[(&str, &str)] = match mirror {
                Mirror::LeftRight => &[("north", "south"), ("south", "north")],
                Mirror::FrontBack => &[("east", "west"), ("west", "east")],
                Mirror::None => &[],
            };
            swap_properties(state, pairs)
        }
        _ => state,
    }
}

/// `BlockState.rotate(rotation)`。
///
/// `BlockState.rotate(rotation)`.
pub fn apply_rotation(state: u32, rotation: Rotation) -> u32 {
    if rotation == Rotation::None {
        return state;
    }
    let Some(block) = block_of_state(state) else {
        return state;
    };
    match block.name {
        // `WallTorchBlock.rotate`：`state.setValue(FACING, rotation.rotate(FACING))`。
        // `ButtonBlock` 继承 `HorizontalDirectionalBlock`，实现相同。
        // `WallTorchBlock.rotate`, and `ButtonBlock` via `HorizontalDirectionalBlock`.
        n if is_horizontal_facing(n) => {
            let Some(facing) = property_of(state, "facing") else {
                return state;
            };
            with_property(state, "facing", rotation.rotate_facing(facing)).unwrap_or(state)
        }
        // `DoorBlock.rotate`：只旋转 `facing`，不动 `hinge`。
        // `DoorBlock.rotate`: only `facing` turns; `hinge` stays.
        n if is_door(n) => {
            let Some(facing) = property_of(state, "facing") else {
                return state;
            };
            with_property(state, "facing", rotation.rotate_facing(facing)).unwrap_or(state)
        }
        // `CrossCollisionBlock.rotate` 的四个分支照抄。
        // The four `CrossCollisionBlock.rotate` branches, transcribed.
        name if is_cross_collision(name) => {
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
        name if is_rail(name) => {
            let Some(shape) = property_of(state, "shape") else {
                return state;
            };
            let new_shape = rotate_rail(shape, rotation);
            with_property(state, "shape", new_shape).unwrap_or(state)
        }
        _ => state,
    }
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

/// 是不是 `CrossCollisionBlock` 的子类（栅栏、墙、玻璃板、铁栏杆……）。
///
/// Whether the block is a `CrossCollisionBlock` subclass (fences, walls, panes, bars).
fn is_cross_collision(name: &str) -> bool {
    name.ends_with("_fence")
        || name.ends_with("_fence_gate")
        || name.ends_with("_wall")
        || name.ends_with("_pane")
        || name == "minecraft:iron_bars"
}

/// 是不是 `HorizontalDirectionalBlock` 的子类、且只有 `facing` 一个朝向属性
/// （墙上火把、按钮、箱子、熔炉……）。要塞用到的是墙上火把和石按钮。
///
/// Whether the block is a `HorizontalDirectionalBlock` subclass whose only direction
/// property is `facing` (wall torches, buttons, chests, furnaces, ...). The stronghold
/// uses wall torches and stone buttons.
fn is_horizontal_facing(name: &str) -> bool {
    matches!(
        name,
        "minecraft:wall_torch"
            | "minecraft:stone_button"
            | "minecraft:oak_button"
            | "minecraft:spruce_button"
            | "minecraft:birch_button"
            | "minecraft:jungle_button"
            | "minecraft:acacia_button"
            | "minecraft:dark_oak_button"
            | "minecraft:cherry_button"
            | "minecraft:pale_oak_button"
            | "minecraft:mangrove_button"
            | "minecraft:bamboo_button"
            | "minecraft:crimson_button"
            | "minecraft:warped_button"
            | "minecraft:polished_blackstone_button"
    )
}

/// 是不是 `DoorBlock`（需要额外的 `cycle(HINGE)`）。
///
/// Whether the block is a `DoorBlock` (needs the extra `cycle(HINGE)`).
fn is_door(name: &str) -> bool {
    name.ends_with("_door") && name != "minecraft:iron_trapdoor"
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

/// 是不是 `BaseRailBlock` 的子类。
///
/// Whether the block is a `BaseRailBlock` subclass.
fn is_rail(name: &str) -> bool {
    matches!(
        name,
        "minecraft:rail"
            | "minecraft:powered_rail"
            | "minecraft:detector_rail"
            | "minecraft:activator_rail"
    )
}

/// 读一个属性值。
///
/// Read one property value.
fn property_of(state: u32, name: &str) -> Option<&'static str> {
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
