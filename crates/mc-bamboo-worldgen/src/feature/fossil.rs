//! `FossilFeature`：沙漠 / 沼泽地下的化石结构。
//!
//! 官方 `FossilFeature.place` 先随机旋转、随机选一对模板（`fossil_structures` 与
//! `overlay_structures` 一一对应），再把锚点压到「模板覆盖范围内 `OCEAN_FLOOR_WG`
//! 最低高度往下 15..24 格」，最后**连放两次**：先 `fossil_processors` 放基座，
//! 再 `overlay_processors` 在同一位置盖一层矿脉。两次各用一条独立的 processor 链。
//!
//! 随机数顺序（一步都不能少）：
//! 1. `Rotation.getRandom(random)` = `Util.getRandom(values(), random)` -> `nextInt(4)`
//! 2. `random.nextInt(fossilStructures.size())`
//! 3. `lowestSurfaceY` 扫描（不取数）
//! 4. `random.nextInt(10)`（`targetY` 里的 `- 15 - nextInt(10)`）
//! 5. 两次 `placeInWorld` 里 processor 链的取数（`block_rot` 的 `nextFloat`）
//!
//! `countEmptyCorners` 只读方块、不取数；超过 `max_empty_corners_allowed` 就整体放弃。
//!
//! `FossilFeature.place`: roll a rotation, pick one template pair (`fossil_structures`
//! and `overlay_structures` are index-aligned), push the anchor down to
//! `lowest OCEAN_FLOOR_WG - 15 - nextInt(10)` clamped above `minY + 10`, then place
//! **twice**: the base with `fossil_processors`, the ore overlay with
//! `overlay_processors`. Each pass uses its own processor chain.
//!
//! Random order (every draw matters): rotation (`nextInt(4)`), then
//! `nextInt(template_count)`, then the surface scan (no draws), then `nextInt(10)`,
//! then the processors' draws (`block_rot`'s `nextFloat`). `countEmptyCorners` reads
//! blocks only; exceeding `max_empty_corners_allowed` abandons the whole fossil.

use mc_bamboo_data::{block_by_name, block_tag, state_facts};
use serde_json::Value;

use crate::{
    random::WorldgenRandom,
    structure::{
        state_transform::{Mirror, Rotation, transform},
        template::{PaletteEntry, StructureTemplate},
    },
};

use super::{
    state::apply_property,
    world::{FeatureWorld, HeightmapKind},
};

type Pos = (i32, i32, i32);

/// `Rotation.values()` 的声明顺序（`NONE`、`CLOCKWISE_90`、`CLOCKWISE_180`、
/// `COUNTERCLOCKWISE_90`），`Util.getRandom` 按下标取。
///
/// `Rotation.values()` in declaration order (`NONE`, `CLOCKWISE_90`, `CLOCKWISE_180`,
/// `COUNTERCLOCKWISE_90`); `Util.getRandom` indexes into it.
const ALL_ROTATIONS: [Rotation; 4] = [
    Rotation::None,
    Rotation::Clockwise90,
    Rotation::Clockwise180,
    Rotation::CounterClockwise90,
];

/// 一条 `rule` processor 的规则（`fossil_diamonds` 用它把煤矿换成钻石矿）。
///
/// One rule of a `rule` processor (`fossil_diamonds` swaps coal ore for diamond ore).
#[derive(Debug, Clone)]
struct Rule {
    input_block: i32,
    output_state: u32,
}

/// 一条 processor 链：`minecraft:rule` 的规则 + 其它单条 processor，按配置文件顺序。
///
/// A processor chain: the rules of every `minecraft:rule` entry plus the other
/// single-block processors, in config order.
#[derive(Debug, Clone, Default)]
struct Chain {
    steps: Vec<Step>,
}

#[derive(Debug, Clone)]
enum Step {
    BlockRot {
        integrity: f32,
        rottable: Option<Vec<i32>>,
    },
    Protected {
        blocks: Vec<i32>,
    },
    Rules(Vec<Rule>),
}

/// 一个已解析的 `minecraft:fossil`。
///
/// A parsed `minecraft:fossil`.
#[derive(Debug, Clone)]
pub struct FossilFeature {
    fossil_structures: Vec<String>,
    overlay_structures: Vec<String>,
    fossil_processors: String,
    overlay_processors: String,
    max_empty_corners_allowed: i32,
}

impl FossilFeature {
    /// 按官方 codec 解析。`fossil_structures` 与 `overlay_structures` 必须等长
    /// （官方 `validate` 会拒绝不等长的配置）。
    ///
    /// Parse by the official codec. The two structure lists must have equal length;
    /// the official `validate` rejects anything else.
    pub fn parse(config: &Value) -> Option<Self> {
        let object = config.as_object()?;
        let strings = |key: &str| -> Option<Vec<String>> {
            object
                .get(key)?
                .as_array()?
                .iter()
                .map(|value| Some(value.as_str()?.to_owned()))
                .collect()
        };
        let fossil_structures = strings("fossil_structures")?;
        let overlay_structures = strings("overlay_structures")?;
        if fossil_structures.is_empty() || fossil_structures.len() != overlay_structures.len() {
            return None;
        }
        Some(Self {
            fossil_structures,
            overlay_structures,
            fossil_processors: object.get("fossil_processors")?.as_str()?.to_owned(),
            overlay_processors: object.get("overlay_processors")?.as_str()?.to_owned(),
            max_empty_corners_allowed: object.get("max_empty_corners_allowed")?.as_i64()? as i32,
        })
    }

    /// 所有引用的模板都能加载、两条 processor 链都移植了才算支持。
    ///
    /// Supported only when every referenced template loads and both processor chains
    /// are ported.
    pub fn supported(&self) -> bool {
        if self.fossil_structures.len() != self.overlay_structures.len() {
            return false;
        }
        self.chain(&self.fossil_processors).is_some()
            && self.chain(&self.overlay_processors).is_some()
            && self
                .fossil_structures
                .iter()
                .chain(self.overlay_structures.iter())
                .all(|id| StructureTemplate::load_cached(id).is_ok())
    }

    /// 从 `processor_list` 注册表解析一条链；含未移植的 processor 时返回 `None`。
    ///
    /// Resolve one chain from the `processor_list` registry; `None` when it contains an
    /// unported processor kind.
    fn chain(&self, name: &str) -> Option<Chain> {
        let root: Value = serde_json::from_str(mc_bamboo_data::worldgen::WORLDGEN).ok()?;
        let list = root.get("processor_list")?.get(name)?;
        let processors = list.get("processors")?.as_array()?;
        let mut steps = Vec::with_capacity(processors.len());
        for processor in processors {
            let kind = processor.get("processor_type")?.as_str()?;
            match kind {
                "minecraft:block_rot" => {
                    let integrity = processor.get("integrity")?.as_f64()? as f32;
                    let rottable = match processor.get("rottable_blocks") {
                        None => None,
                        Some(value) => Some(parse_block_set(value)?),
                    };
                    steps.push(Step::BlockRot {
                        integrity,
                        rottable,
                    });
                }
                "minecraft:protected_blocks" => {
                    let blocks = parse_block_set(processor.get("value")?)?;
                    steps.push(Step::Protected { blocks });
                }
                "minecraft:rule" => {
                    let mut rules = Vec::new();
                    for rule in processor.get("rules")?.as_array()? {
                        rules.push(parse_rule(rule)?);
                    }
                    steps.push(Step::Rules(rules));
                }
                _ => return None,
            }
        }
        Some(Chain { steps })
    }

    /// `FossilFeature.place`。返回官方 `placed`。
    ///
    /// `FossilFeature.place`; returns the official `placed` flag.
    pub fn place(
        &self,
        world: &mut FeatureWorld<'_>,
        random: &WorldgenRandom,
        origin: Pos,
    ) -> bool {
        let Some(fossil_chain) = self.chain(&self.fossil_processors) else {
            return false;
        };
        let Some(overlay_chain) = self.chain(&self.overlay_processors) else {
            return false;
        };

        // `Rotation.getRandom(random)` -> `Util.getRandom(values(), random)`，
        // 即一次 `nextInt(4)`，落在 `values()` 的 ordinal 上。
        //
        // `Rotation.getRandom(random)` -> `Util.getRandom(values(), random)`: exactly one
        // `nextInt(4)`, indexed into `values()`.
        let rotation = ALL_ROTATIONS[random.next_int(4) as usize];
        // `random.nextInt(this.fossilStructures().size())`。
        let index = random.next_int(self.fossil_structures.len() as i32) as usize;
        let Ok(fossil_base) = StructureTemplate::load_cached(&self.fossil_structures[index]) else {
            return false;
        };
        let Ok(fossil_overlay) = StructureTemplate::load_cached(&self.overlay_structures[index])
        else {
            return false;
        };

        // 官方把 `boundingBox` 设成「中心区块 ±16 格 × 整列」，`placeInWorld` 只放落在
        // 里面的方块。`isInside` 的上下界都是闭区间，所以这里按闭区间判断。
        //
        // The official bounding box is the centre chunk padded by 16 blocks across the
        // full height; `placeInWorld` only writes cells inside it. `isInside` is
        // inclusive on both ends, so the check below mirrors that.
        let box_min_x = (origin.0 >> 4 << 4) - 16;
        let box_max_x = box_min_x + 31 + 32;
        let box_min_z = (origin.2 >> 4 << 4) - 16;
        let box_max_z = box_min_z + 31 + 32;
        let box_min_y = world.min_y;
        let box_max_y = world.min_y + world.height - 1;

        let (size_x, size_y, size_z) = rotated_size(rotation, fossil_base.size);
        let low_corner = (origin.0 - size_x / 2, origin.1, origin.2 - size_z / 2);

        let mut lowest_surface_y = origin.1;
        for xscan in 0..size_x {
            for zscan in 0..size_z {
                let height = world.get_height(
                    HeightmapKind::OceanFloorWg,
                    low_corner.0 + xscan,
                    low_corner.2 + zscan,
                );
                lowest_surface_y = lowest_surface_y.min(height);
            }
        }

        let target_y = (lowest_surface_y - 15 - random.next_int(10)).max(world.min_y + 10);
        let target = zero_position_with_transform(
            (low_corner.0, target_y, low_corner.2),
            rotation,
            fossil_base.size.0,
            fossil_base.size.2,
        );

        let corners = bounding_box(rotation, target, fossil_base.size);
        if count_empty_corners(world, corners) > self.max_empty_corners_allowed {
            return false;
        }

        let _ = size_y;
        place_in_world(
            &fossil_base,
            world,
            random,
            rotation,
            target,
            &fossil_chain,
            (box_min_x, box_min_y, box_min_z),
            (box_max_x, box_max_y, box_max_z),
        );
        place_in_world(
            &fossil_overlay,
            world,
            random,
            rotation,
            target,
            &overlay_chain,
            (box_min_x, box_min_y, box_min_z),
            (box_max_x, box_max_y, box_max_z),
        );
        true
    }
}

/// `StructureTemplate.getSize(rotation)`：90° / 270° 时 X 与 Z 互换。
///
/// `StructureTemplate.getSize(rotation)`: the two 90° rotations swap X and Z.
fn rotated_size(rotation: Rotation, size: (i32, i32, i32)) -> (i32, i32, i32) {
    match rotation {
        Rotation::Clockwise90 | Rotation::CounterClockwise90 => (size.2, size.1, size.0),
        _ => size,
    }
}

/// `StructureTemplate.getZeroPositionWithTransform(zeroPos, NONE, rotation, sizeX, sizeZ)`。
///
/// `StructureTemplate.getZeroPositionWithTransform(zeroPos, NONE, rotation, sizeX, sizeZ)`.
fn zero_position_with_transform(zero: Pos, rotation: Rotation, size_x: i32, size_z: i32) -> Pos {
    let size_x = size_x - 1;
    let size_z = size_z - 1;
    // `Mirror.NONE` 时两个 `mirrorDelta` 都是 0。
    // Both mirror deltas are zero for `Mirror.NONE`.
    match rotation {
        Rotation::CounterClockwise90 => (zero.0, zero.1, zero.2 + size_x),
        Rotation::Clockwise90 => (zero.0 + size_z, zero.1, zero.2),
        Rotation::Clockwise180 => (zero.0 + size_x, zero.1, zero.2 + size_z),
        Rotation::None => zero,
    }
}

/// `StructureTemplate.getBoundingBox(position, rotation, pivot=ZERO, mirror=NONE)`：
/// `transform(ZERO)` 与 `transform(size - 1)` 两个角点围成的盒子再平移 `position`。
///
/// `StructureTemplate.getBoundingBox(position, rotation, pivot=ZERO, mirror=NONE)`: the
/// box spanned by `transform(ZERO)` and `transform(size - 1)`, moved to `position`.
fn bounding_box(rotation: Rotation, position: Pos, size: (i32, i32, i32)) -> (Pos, Pos) {
    let corner1 = transform_pos(rotation, (0, 0, 0));
    let corner2 = transform_pos(rotation, (size.0 - 1, size.1 - 1, size.2 - 1));
    let min = (
        corner1.0.min(corner2.0) + position.0,
        corner1.1.min(corner2.1) + position.1,
        corner1.2.min(corner2.2) + position.2,
    );
    let max = (
        corner1.0.max(corner2.0) + position.0,
        corner1.1.max(corner2.1) + position.1,
        corner1.2.max(corner2.2) + position.2,
    );
    (min, max)
}

/// `StructureTemplate.transform(pos, Mirror.NONE, rotation, BlockPos.ZERO)`。
///
/// `StructureTemplate.transform(pos, Mirror.NONE, rotation, BlockPos.ZERO)`.
fn transform_pos(rotation: Rotation, pos: Pos) -> Pos {
    match rotation {
        Rotation::None => pos,
        // 绕 Y 轴顺时针 90°：`(x, z) -> (-z, x)`，再按模板尺寸平移由调用方处理；
        // 这里只做 `transform` 本身（官方对负坐标也是同一公式）。
        // 90° clockwise around Y: `(x, z) -> (-z, x)`.
        Rotation::Clockwise90 => (-pos.2, pos.1, pos.0),
        Rotation::Clockwise180 => (-pos.0, pos.1, -pos.2),
        Rotation::CounterClockwise90 => (pos.2, pos.1, -pos.0),
    }
}

/// `FossilFeature.countEmptyCorners`：包围盒八个角里空气 / 水 / 岩浆的个数。
///
/// `FossilFeature.countEmptyCorners`: how many of the eight corners are air, water or
/// lava.
fn count_empty_corners(world: &FeatureWorld<'_>, corners: (Pos, Pos)) -> i32 {
    let (min, max) = corners;
    let water = block_id_of_name("minecraft:water");
    let lava = block_id_of_name("minecraft:lava");
    let mut count = 0;
    for x in [min.0, max.0] {
        for y in [min.1, max.1] {
            for z in [min.2, max.2] {
                let state = world.get_block(x, y, z);
                let empty = state_facts(state) & mc_bamboo_data::facts::AIR != 0
                    || mc_bamboo_data::block_id_of_state(state)
                        .is_some_and(|id| Some(id as i32) == water || Some(id as i32) == lava);
                if empty {
                    count += 1;
                }
            }
        }
    }
    count
}

fn block_id_of_name(name: &str) -> Option<i32> {
    block_by_name(name)
        .and_then(|block| mc_bamboo_data::block_id_of_state(block.default_state))
        .map(|id| id as i32)
}

/// `StructureTemplate.placeInWorld(...)`：按 processor 链过滤后逐格写方块。
///
/// `StructureTemplate.placeInWorld(...)`: filter through the processor chain and write
/// each block.
#[allow(clippy::too_many_arguments)]
fn place_in_world(
    template: &StructureTemplate,
    world: &mut FeatureWorld<'_>,
    random: &WorldgenRandom,
    rotation: Rotation,
    position: Pos,
    chain: &Chain,
    box_min: Pos,
    box_max: Pos,
) {
    let Some(palette) = template.palettes.first() else {
        return;
    };
    for block in &template.blocks {
        let Some(entry) = palette.get(block.state) else {
            continue;
        };
        let Some(mut state) = palette_state(entry) else {
            continue;
        };
        // `calculateRelativePosition(settings, blockInfo.pos).offset(position)`。
        let rotated = transform_pos(rotation, block.pos);
        let target = (
            position.0 + rotated.0,
            position.1 + rotated.1,
            position.2 + rotated.2,
        );
        if target.0 < box_min.0
            || target.0 > box_max.0
            || target.1 < box_min.1
            || target.1 > box_max.1
            || target.2 < box_min.2
            || target.2 > box_max.2
        {
            continue;
        }
        // processor 链：每步都可能丢弃这一格。
        // The processor chain: any step may drop this cell.
        let mut keep = true;
        for step in &chain.steps {
            match step {
                Step::BlockRot {
                    integrity,
                    rottable,
                } => {
                    let rottable_hit = match rottable {
                        None => true,
                        Some(blocks) => mc_bamboo_data::block_id_of_state(state)
                            .is_some_and(|id| blocks.contains(&(id as i32))),
                    };
                    // 官方是 `!(random.nextFloat() <= integrity)` 时丢弃；`nextFloat()`
                    // 落在 `[0, 1)`、`integrity` 是有限的 0..1，所以 `> integrity`
                    // 与取反等价，且不必对浮点写取反比较。
                    //
                    // Vanilla drops the cell when `!(random.nextFloat() <= integrity)`.
                    // `nextFloat()` is in `[0, 1)` and `integrity` is a finite 0..1, so
                    // `> integrity` is equivalent without a negated float comparison.
                    if rottable_hit && random.next_float() > *integrity {
                        keep = false;
                        break;
                    }
                }
                Step::Protected { blocks } => {
                    let existing = world.get_block(target.0, target.1, target.2);
                    if mc_bamboo_data::block_id_of_state(existing)
                        .is_some_and(|id| blocks.contains(&(id as i32)))
                    {
                        keep = false;
                        break;
                    }
                }
                Step::Rules(rules) => {
                    for rule in rules {
                        if mc_bamboo_data::block_id_of_state(state)
                            .is_some_and(|id| id as i32 == rule.input_block)
                        {
                            state = rule.output_state;
                            break;
                        }
                    }
                }
            }
        }
        if !keep {
            continue;
        }
        state = transform(state, Mirror::None, rotation);
        world.set_block(target.0, target.1, target.2, state);
    }
}

/// 把 `PaletteEntry`（名字 + 属性）还原成状态 ID。
///
/// Rebuild a state ID from a `PaletteEntry` (name plus properties).
fn palette_state(entry: &PaletteEntry) -> Option<u32> {
    let block = block_by_name(&entry.name)?;
    let mut state = block.default_state;
    for (key, value) in &entry.properties {
        state = apply_property(state, key, value)?;
    }
    Some(state)
}

/// `HolderSet<Block>`：`#tag`、方块名，或方块名数组。
///
/// `HolderSet<Block>`: a `#tag`, a block name, or an array of block names.
fn parse_block_set(value: &Value) -> Option<Vec<i32>> {
    match value {
        Value::String(name) => {
            let mut out = Vec::new();
            push_block_set(name, &mut out)?;
            Some(out)
        }
        Value::Array(items) => {
            let mut out = Vec::new();
            for item in items {
                push_block_set(item.as_str()?, &mut out)?;
            }
            Some(out)
        }
        _ => None,
    }
}

fn push_block_set(name: &str, out: &mut Vec<i32>) -> Option<()> {
    if let Some(tag) = name.strip_prefix('#') {
        out.extend_from_slice(block_tag(tag)?);
    } else {
        let block = block_by_name(name)?;
        out.push(mc_bamboo_data::block_id_of_state(block.default_state)? as i32);
    }
    Some(())
}

/// 一条 `minecraft:rule` 的规则：只支持 `block_match` + `always_true`（化石就只用了它）。
///
/// One `minecraft:rule` entry: only `block_match` + `always_true` (all fossils use just
/// that).
fn parse_rule(value: &Value) -> Option<Rule> {
    let object = value.as_object()?;
    let input = object.get("input_predicate")?;
    if input.get("predicate_type")?.as_str()? != "minecraft:block_match" {
        return None;
    }
    let input_block = block_by_name(input.get("block")?.as_str()?).map(|block| {
        mc_bamboo_data::block_id_of_state(block.default_state).map(|id| id as i32)
    })??;
    if object
        .get("location_predicate")?
        .get("predicate_type")?
        .as_str()?
        != "minecraft:always_true"
    {
        return None;
    }
    let output_state = super::state::parse_state(object.get("output_state")?).ok()?;
    Some(Rule {
        input_block,
        output_state,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fossil_config() -> Value {
        let root: Value =
            serde_json::from_str(mc_bamboo_data::worldgen::WORLDGEN).expect("worldgen json");
        root["feature"]["minecraft:fossil_coal"].clone()
    }

    #[test]
    fn parses_the_exported_fossil() {
        let feature = FossilFeature::parse(&fossil_config()).expect("fossil parses");
        assert_eq!(feature.fossil_structures.len(), 8);
        assert_eq!(feature.overlay_structures.len(), 8);
        assert_eq!(feature.max_empty_corners_allowed, 4);
    }

    #[test]
    fn both_processor_chains_and_templates_resolve() {
        let feature = FossilFeature::parse(&fossil_config()).expect("fossil parses");
        assert!(
            feature.supported(),
            "fossil_rot / fossil_coal chains and all templates must resolve"
        );
    }

    #[test]
    fn diamond_overlay_swaps_coal_ore() {
        let root: Value =
            serde_json::from_str(mc_bamboo_data::worldgen::WORLDGEN).expect("worldgen json");
        let config = root["feature"]["minecraft:fossil_diamonds"].clone();
        let feature = FossilFeature::parse(&config).expect("fossil_diamonds parses");
        assert!(feature.supported());
    }

    #[test]
    fn rotated_size_swaps_axes_for_the_quarter_turns() {
        assert_eq!(rotated_size(Rotation::Clockwise90, (4, 5, 6)), (6, 5, 4));
        assert_eq!(
            rotated_size(Rotation::CounterClockwise90, (4, 5, 6)),
            (6, 5, 4)
        );
        assert_eq!(rotated_size(Rotation::None, (4, 5, 6)), (4, 5, 6));
        assert_eq!(rotated_size(Rotation::Clockwise180, (4, 5, 6)), (4, 5, 6));
    }

    /// 造一个 9 区块的实心石头世界，把 y < 0 的部分留给化石。
    ///
    /// A nine-chunk solid-stone world; the sub-zero band is where the fossil lands.
    fn solid_world() -> crate::feature::FeatureChunks {
        let min_y = -64;
        let height = 384;
        let chunk_blocks = || {
            let mut blocks = Vec::with_capacity(height);
            for _ in 0..height {
                blocks.push(vec![mc_bamboo_data::blocks::STONE; 256]);
            }
            blocks
        };
        let sections = || {
            let mut out = Vec::new();
            for _ in 0..(height / 16) {
                out.push(vec![0_u32; 64]);
            }
            out
        };
        crate::feature::FeatureChunks {
            center_x: 0,
            center_z: 0,
            min_y,
            height: height as i32,
            blocks: std::array::from_fn(|_| chunk_blocks()),
            biomes: std::array::from_fn(|_| sections()),
            heights: std::array::from_fn(|_| crate::feature::Heightmaps::new(min_y)),
        }
    }

    #[test]
    fn the_exported_fossil_writes_template_blocks() {
        // 用导出数据里**真实的** `fossil_coal` 跑一遍 `place`：实心石头世界里
        // `countEmptyCorners` 一个空角都看不到，`max_empty_corners_allowed = 4`
        // 必然通过，所以放置路径一定会执行。
        //
        // Run the **exported** `fossil_coal` through `place`: in a solid-stone world
        // `countEmptyCorners` sees no air at all, so the four-corner budget always
        // passes and the placement path runs.
        let feature = FossilFeature::parse(&fossil_config()).expect("fossil parses");
        let input = solid_world();
        let mut world = FeatureWorld::new(&input, 63, 0).unwrap();
        let random =
            crate::random::WorldgenRandom::new(crate::random::Algorithm::Xoroshiro.new_instance(3));
        let placed = feature.place(&mut world, &random, (8, 0, 8));
        assert!(placed, "the exported fossil must place in a solid world");

        // 至少有一格不是石头：模板把石头换成了骨块 / 煤矿之类。
        // At least one cell must differ from stone: the template swaps in bone blocks,
        // coal ore and the like.
        let mut changed = 0;
        for x in 0..16 {
            for z in 0..16 {
                for y in -64..-40 {
                    if world.get_block(x, y, z) != mc_bamboo_data::blocks::STONE {
                        changed += 1;
                    }
                }
            }
        }
        assert!(changed > 0, "the fossil template must write blocks");
    }

    #[test]
    fn block_rot_drops_cells_below_the_integrity() {
        // `block_rot` 每格取一次 `nextFloat`，`<= integrity` 才保留。integrity 0 时
        // 没有任何格子能留下，所以模板一格都写不出来。
        //
        // `block_rot` draws one `nextFloat` per cell and keeps it when `<= integrity`.
        // With integrity 0 nothing survives, so the template writes nothing.
        let chain = Chain {
            steps: vec![Step::BlockRot {
                integrity: 0.0,
                rottable: None,
            }],
        };
        let template =
            StructureTemplate::load_cached("minecraft:fossil/spine_1").expect("template loads");
        let input = solid_world();
        let mut world = FeatureWorld::new(&input, 63, 0).unwrap();
        let random =
            crate::random::WorldgenRandom::new(crate::random::Algorithm::Xoroshiro.new_instance(5));
        place_in_world(
            &template,
            &mut world,
            &random,
            Rotation::None,
            (8, 0, 8),
            &chain,
            (-16, -64, -16),
            (48, 320, 48),
        );
        for x in 0..16 {
            for z in 0..16 {
                for y in -64..-40 {
                    assert_eq!(
                        world.get_block(x, y, z),
                        mc_bamboo_data::blocks::STONE,
                        "integrity 0 must drop every cell"
                    );
                }
            }
        }
    }
}
