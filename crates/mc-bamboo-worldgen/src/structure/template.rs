//! 从导出的 gzip NBT 解析 `StructureTemplate`：尺寸、调色板、方块、方块实体和拼图方块。
//!
//! 字段照官方 `StructureTemplate.load` / `JigsawBlockInfo.parse`：`size` 是三个 int 的
//! 列表，`palette`（或 `palettes` 的第一个）是方块状态表，`blocks` 每项含
//! `pos`/`state`/可选 `nbt`，`entities` 每项含 `pos`/`blockPos`/`nbt`。拼图方块是调色板
//! 名为 `minecraft:jigsaw` 的 block，`orientation` 在方块状态属性里，其余字段在 `nbt` 里。
//! 字节来自 `mc-bamboo-data::structures`，只解析、不改字节。
//!
//! Parse `StructureTemplate` from the exported gzip NBT: size, palettes, blocks, block
//! entities and jigsaw blocks. Fields follow vanilla `StructureTemplate.load` /
//! `JigsawBlockInfo.parse`: `size` is a list of three ints, `palette` (or the first of
//! `palettes`) is the block-state table, each `blocks` entry holds `pos`/`state` and
//! optional `nbt`, each `entities` entry holds `pos`/`blockPos`/`nbt`. A jigsaw block is
//! one whose palette name is `minecraft:jigsaw`; `orientation` is a block-state property
//! and the rest live in `nbt`. Bytes come from `mc-bamboo-data::structures` and are
//! parsed, never rewritten.

use mc_bamboo_data::structures as blob;
use mc_bamboo_nbt::{Accounter, Compression, Tag, read_compressed};

type Compound = [(String, Tag)];

/// 官方 `net.minecraft.core.Direction` 的六个值，序号就是 `get3DDataValue()`。
///
/// The six values of vanilla `net.minecraft.core.Direction`, in `get3DDataValue()` order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Down,
    Up,
    North,
    South,
    West,
    East,
}

impl Direction {
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "down" => Self::Down,
            "up" => Self::Up,
            "north" => Self::North,
            "south" => Self::South,
            "west" => Self::West,
            "east" => Self::East,
            _ => return None,
        })
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Down => "down",
            Self::Up => "up",
            Self::North => "north",
            Self::South => "south",
            Self::West => "west",
            Self::East => "east",
        }
    }

    /// `Direction.getOpposite()`。
    pub const fn opposite(self) -> Self {
        match self {
            Self::Down => Self::Up,
            Self::Up => Self::Down,
            Self::North => Self::South,
            Self::South => Self::North,
            Self::West => Self::East,
            Self::East => Self::West,
        }
    }

    /// `Direction.getStepY()`。
    pub const fn step_y(self) -> i32 {
        match self {
            Self::Down => -1,
            Self::Up => 1,
            _ => 0,
        }
    }

    /// `Direction.getAxis().isHorizontal()`。
    pub const fn axis_is_horizontal(self) -> bool {
        matches!(self, Self::North | Self::South | Self::West | Self::East)
    }

    /// `Direction.getClockWise()`（绕 Y 轴）。
    pub const fn clock_wise(self) -> Self {
        match self {
            Self::North => Self::East,
            Self::East => Self::South,
            Self::South => Self::West,
            Self::West => Self::North,
            _ => self,
        }
    }

    /// `Direction.getCounterClockWise()`（绕 Y 轴）。
    pub const fn counter_clock_wise(self) -> Self {
        match self {
            Self::North => Self::West,
            Self::West => Self::South,
            Self::South => Self::East,
            Self::East => Self::North,
            _ => self,
        }
    }

    /// `Direction.getNormal()` 的 X/Y/Z 步长。
    pub const fn step(self) -> (i32, i32, i32) {
        match self {
            Self::Down => (0, -1, 0),
            Self::Up => (0, 1, 0),
            Self::North => (0, 0, -1),
            Self::South => (0, 0, 1),
            Self::West => (-1, 0, 0),
            Self::East => (1, 0, 0),
        }
    }
}

/// `JigsawBlockEntity.JointType`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JointType {
    Rollable,
    Aligned,
}

/// 调色板的一项：`Name` 和可选的 `Properties`。
///
/// One palette entry: `Name` plus optional `Properties`.
#[derive(Debug, Clone, PartialEq)]
pub struct PaletteEntry {
    pub name: String,
    pub properties: Vec<(String, String)>,
}

impl PaletteEntry {
    pub fn property(&self, key: &str) -> Option<&str> {
        self.properties
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }
}

/// `blocks` 里的一项。
///
/// One entry of `blocks`.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockEntry {
    pub pos: (i32, i32, i32),
    pub state: usize,
    pub nbt: Option<Tag>,
}

/// `entities` 里的一项。
///
/// One entry of `entities`.
#[derive(Debug, Clone, PartialEq)]
pub struct EntityEntry {
    pub pos: (f64, f64, f64),
    pub block_pos: (i32, i32, i32),
    pub nbt: Tag,
}

/// 一个拼图方块，照 `JigsawBlockInfo.parse`。
///
/// One jigsaw block, following `JigsawBlockInfo.parse`.
#[derive(Debug, Clone, PartialEq)]
pub struct JigsawEntry {
    pub pos: (i32, i32, i32),
    pub orientation: String,
    pub name: String,
    pub target: String,
    pub pool: String,
    pub final_state: String,
    pub joint: JointType,
    pub placement_priority: i32,
    pub selection_priority: i32,
}

impl JigsawEntry {
    /// `FrontAndTop.front()`：orientation 名字的第一段。
    pub fn front(&self) -> Option<Direction> {
        self.orientation
            .split('_')
            .next()
            .and_then(Direction::from_name)
    }

    /// `FrontAndTop.top()`：orientation 名字的第二段。
    pub fn top(&self) -> Option<Direction> {
        self.orientation
            .split('_')
            .nth(1)
            .and_then(Direction::from_name)
    }
}

/// 一块结构模板。
///
/// One parsed structure template.
#[derive(Debug, Clone, PartialEq)]
pub struct StructureTemplate {
    pub size: (i32, i32, i32),
    pub palettes: Vec<Vec<PaletteEntry>>,
    pub blocks: Vec<BlockEntry>,
    pub entities: Vec<EntityEntry>,
    pub jigsaws: Vec<JigsawEntry>,
}

impl StructureTemplate {
    /// 按注册名（`minecraft:...`）加载模板。
    ///
    /// 官方 `StructureTemplateManager.getOrCreate(id)` 对数据包里找不到的模板返回一个
    /// 空 `StructureTemplate`（size 0、无 palette），不是错误；这里照做。
    ///
    /// Load a template by its id (`minecraft:...`). Vanilla `getOrCreate(id)`
    /// returns an empty `StructureTemplate` for a missing id instead of an error.
    pub fn load(name: &str) -> Result<Self, String> {
        let Some(bytes) = blob::get(name) else {
            return Ok(Self::empty());
        };
        Self::parse(bytes)
    }

    /// `new StructureTemplate()`：size 0、无 palette、无方块、无实体、无拼图方块。
    ///
    /// `new StructureTemplate()`: size 0, no palettes, blocks, entities or jigsaws.
    pub fn empty() -> Self {
        Self {
            size: (0, 0, 0),
            palettes: Vec::new(),
            blocks: Vec::new(),
            entities: Vec::new(),
            jigsaws: Vec::new(),
        }
    }

    /// 从官方 `NbtIo.writeCompressed` 的 gzip 字节解析。
    ///
    /// Parse the gzip bytes produced by vanilla `NbtIo.writeCompressed`.
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let (_, root) = read_compressed(bytes, Compression::Gzip, Accounter::unlimited())
            .map_err(|error| format!("cannot read structure template NBT: {error}"))?;
        let root: &Compound = &root;
        let size = int_triple(field(root, "size").ok_or("structure template has no size")?)?;
        let palettes = parse_palettes(root)?;
        let blocks = parse_blocks(root)?;
        let entities = parse_entities(root)?;
        let jigsaws = parse_jigsaws(&palettes, &blocks)?;
        Ok(Self {
            size,
            palettes,
            blocks,
            entities,
            jigsaws,
        })
    }
}

fn parse_palettes(root: &Compound) -> Result<Vec<Vec<PaletteEntry>>, String> {
    if let Some(palettes) = field(root, "palettes") {
        let lists = as_list(palettes).ok_or("palettes is not a list")?;
        lists.iter().map(parse_palette).collect()
    } else {
        let palette = field(root, "palette").ok_or("structure template has no palette")?;
        Ok(vec![parse_palette(palette)?])
    }
}

fn parse_palette(tag: &Tag) -> Result<Vec<PaletteEntry>, String> {
    as_list(tag)
        .ok_or("palette is not a list")?
        .iter()
        .map(|entry| {
            let compound = as_compound(entry).ok_or("palette entry is not a compound")?;
            // 26.3 的 `NbtUtils.writeBlockState` 写 `id` / `properties`；旧版写
            // `Name` / `Properties`，两种都认，导出的数据里都是新格式。
            // 26.3 `NbtUtils.writeBlockState` writes `id` / `properties`; older
            // versions wrote `Name` / `Properties`. Accept both; exports are all new.
            let name = field(compound, "id")
                .and_then(as_string)
                .or_else(|| field(compound, "Name").and_then(as_string))
                .ok_or("palette entry has no id/Name")?;
            let properties = field(compound, "properties")
                .or_else(|| field(compound, "Properties"))
                .map(parse_properties)
                .unwrap_or_default();
            Ok(PaletteEntry {
                name: name.to_owned(),
                properties,
            })
        })
        .collect()
}

fn parse_properties(tag: &Tag) -> Vec<(String, String)> {
    as_compound(tag).map_or_else(Vec::new, |compound| {
        compound
            .iter()
            .filter_map(|(key, value)| as_string(value).map(|text| (key.clone(), text.to_owned())))
            .collect()
    })
}

fn parse_blocks(root: &Compound) -> Result<Vec<BlockEntry>, String> {
    as_list(field(root, "blocks").ok_or("structure template has no blocks")?)
        .ok_or("blocks is not a list")?
        .iter()
        .map(|tag| {
            let compound = as_compound(tag).ok_or("block entry is not a compound")?;
            let pos = int_triple(field(compound, "pos").ok_or("block has no pos")?)?;
            let state = field(compound, "state").and_then(as_int).unwrap_or(0);
            Ok(BlockEntry {
                pos,
                state: usize::try_from(state).unwrap_or(0),
                nbt: field(compound, "nbt").cloned(),
            })
        })
        .collect()
}

fn parse_entities(root: &Compound) -> Result<Vec<EntityEntry>, String> {
    let Some(list) = field(root, "entities").and_then(as_list) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for tag in list {
        let compound = as_compound(tag).ok_or("entity entry is not a compound")?;
        // 官方 `getCompound("nbt").ifPresent(...)`：没有 nbt 的实体直接跳过。
        // Vanilla `getCompound("nbt").ifPresent(...)`: entities without nbt are skipped.
        let Some(nbt) = field(compound, "nbt") else {
            continue;
        };
        let pos = double_triple(field(compound, "pos").ok_or("entity has no pos")?)?;
        let block_pos = int_triple(field(compound, "blockPos").ok_or("entity has no blockPos")?)?;
        out.push(EntityEntry {
            pos,
            block_pos,
            nbt: nbt.clone(),
        });
    }
    Ok(out)
}

fn parse_jigsaws(
    palettes: &[Vec<PaletteEntry>],
    blocks: &[BlockEntry],
) -> Result<Vec<JigsawEntry>, String> {
    let palette = palettes
        .first()
        .ok_or("structure template has no palette")?;
    let mut out = Vec::new();
    for block in blocks {
        let state = palette
            .get(block.state)
            .ok_or("block state index out of palette")?;
        if state.name != "minecraft:jigsaw" {
            continue;
        }
        // `JigsawBlockInfo.parse` 对拼图方块强制要求 nbt。
        // `JigsawBlockInfo.parse` requires the jigsaw block to have nbt.
        let nbt = block
            .nbt
            .as_ref()
            .and_then(as_compound)
            .ok_or("jigsaw block has no nbt")?;
        let orientation = state.property("orientation").unwrap_or("north_up");
        let front = orientation.split('_').next().and_then(Direction::from_name);
        let joint = match field(nbt, "joint").and_then(as_string) {
            Some("aligned") => JointType::Aligned,
            Some("rollable") => JointType::Rollable,
            // `getDefaultJointType`：front 水平是 ALIGNED，否则 ROLLABLE。
            // `getDefaultJointType`: horizontal front is ALIGNED, otherwise ROLLABLE.
            _ if front.is_some_and(Direction::axis_is_horizontal) => JointType::Aligned,
            _ => JointType::Rollable,
        };
        out.push(JigsawEntry {
            pos: block.pos,
            orientation: orientation.to_owned(),
            name: string_or(nbt, "name", "minecraft:empty"),
            target: string_or(nbt, "target", "minecraft:empty"),
            pool: string_or(nbt, "pool", "minecraft:empty"),
            final_state: string_or(nbt, "final_state", "minecraft:air"),
            joint,
            placement_priority: int_or(nbt, "placement_priority", 0),
            selection_priority: int_or(nbt, "selection_priority", 0),
        });
    }
    Ok(out)
}

fn field<'a>(compound: &'a Compound, key: &str) -> Option<&'a Tag> {
    compound
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
}

fn as_compound(tag: &Tag) -> Option<&Compound> {
    match tag {
        Tag::Compound(entries) => Some(entries),
        _ => None,
    }
}

fn as_list(tag: &Tag) -> Option<&[Tag]> {
    match tag {
        Tag::List(entries) => Some(entries),
        _ => None,
    }
}

fn as_string(tag: &Tag) -> Option<&str> {
    match tag {
        Tag::String(text) => Some(text),
        _ => None,
    }
}

fn as_int(tag: &Tag) -> Option<i32> {
    match tag {
        Tag::Byte(value) => Some(i32::from(*value)),
        Tag::Short(value) => Some(i32::from(*value)),
        Tag::Int(value) => Some(*value),
        _ => None,
    }
}

fn string_or(compound: &Compound, key: &str, default: &str) -> String {
    field(compound, key)
        .and_then(as_string)
        .unwrap_or(default)
        .to_owned()
}

fn int_or(compound: &Compound, key: &str, default: i32) -> i32 {
    field(compound, key).and_then(as_int).unwrap_or(default)
}

fn int_triple(tag: &Tag) -> Result<(i32, i32, i32), String> {
    let values: Vec<i32> = match tag {
        Tag::List(entries) => entries
            .iter()
            .map(as_int)
            .collect::<Option<Vec<_>>>()
            .ok_or("expected a list of three ints")?,
        Tag::IntArray(values) => values.clone(),
        _ => return Err("expected a list of three ints".to_owned()),
    };
    Ok((
        values.first().copied().unwrap_or(0),
        values.get(1).copied().unwrap_or(0),
        values.get(2).copied().unwrap_or(0),
    ))
}

fn double_triple(tag: &Tag) -> Result<(f64, f64, f64), String> {
    fn as_double(value: &Tag) -> Option<f64> {
        match value {
            Tag::Double(v) => Some(*v),
            Tag::Float(v) => Some(f64::from(*v)),
            _ => None,
        }
    }
    let values = as_list(tag).ok_or("expected a list of three doubles")?;
    let parsed: Vec<f64> = values
        .iter()
        .map(as_double)
        .collect::<Option<Vec<_>>>()
        .ok_or("expected a list of three doubles")?;
    Ok((
        parsed.first().copied().unwrap_or(0.0),
        parsed.get(1).copied().unwrap_or(0.0),
        parsed.get(2).copied().unwrap_or(0.0),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 全部导出的模板都要能被 NBT 解析，调色板索引和拼图方块字段都要合法。
    ///
    /// Every exported template must parse, with valid palette indices and jigsaw fields.
    #[test]
    fn every_exported_template_parses() {
        let mut with_jigsaws = 0;
        let mut with_block_entities = 0;
        let mut multi_palette = 0;
        let mut jigsaw_blocks = 0;
        for &(name, _, _) in blob::ENTRIES {
            let template =
                StructureTemplate::load(name).unwrap_or_else(|error| panic!("{name}: {error}"));
            assert!(!template.palettes.is_empty(), "{name}");
            if template.palettes.len() > 1 {
                multi_palette += 1;
            }
            jigsaw_blocks += template.jigsaws.len();
            for block in &template.blocks {
                assert!(
                    block.state < template.palettes[0].len(),
                    "{name}: state {} out of palette",
                    block.state
                );
            }
            for jigsaw in &template.jigsaws {
                assert!(jigsaw.front().is_some(), "{name}: bad jigsaw front");
                assert!(jigsaw.top().is_some(), "{name}: bad jigsaw top");
            }
            if !template.jigsaws.is_empty() {
                with_jigsaws += 1;
            }
            if template.blocks.iter().any(|block| block.nbt.is_some()) {
                with_block_entities += 1;
            }
        }
        assert!(
            with_jigsaws > 100,
            "only {with_jigsaws} templates have jigsaw blocks"
        );
        assert!(
            with_block_entities > 10,
            "only {with_block_entities} templates have block entities"
        );
        eprintln!(
            "templates={} with_jigsaws={with_jigsaws} jigsaw_blocks={jigsaw_blocks} \
             with_block_entities={with_block_entities} multi_palette={multi_palette}",
            blob::ENTRIES.len()
        );
    }

    /// `empty.nbt` 是最小的模板，能读出来且尺寸非负。
    ///
    /// `empty.nbt` is the smallest template; it loads with a non-negative size.
    #[test]
    fn empty_template_loads() {
        let template = StructureTemplate::load("minecraft:empty").unwrap();
        assert!(template.size.0 >= 0 && template.size.1 >= 0 && template.size.2 >= 0);
    }
}
