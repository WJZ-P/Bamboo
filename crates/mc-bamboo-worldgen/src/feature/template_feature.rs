//! `TemplateFeature`（`minecraft:template`）：把一份结构模板直接放进世界。
//!
//! 照 `TemplateFeature.place`：先 `templates.getRandomOrThrow(random)` 选模板，
//! 再 `Util.getRandom(entry.rotations, random)` 选旋转；位置按
//! `getRotatedOffset` 取「旋转后负轴单位向量 × 该轴尺寸的一半」把模板居中；
//! 然后按 `StructurePlaceSettings`（rotation + processor 链）逐格放方块。
//!
//! 本轮只支持 `desert_well` 用到的 processor 子集：`minecraft:rule` 配
//! `block_match` / `always_true` 谓词与 `append_loot` 方块实体修饰器。其余 processor
//! 类型解析成 unsupported，整个 placed feature 标 `unsupported`（不假装支持）。
//!
//! 已知简化：模板的方块实体 NBT 只保留 `LootTable` / `LootTableSeed`（`append_loot`
//! 就是写这两个），别的方块实体字段不写；`FeatureResult` 本来也只比较方块。
//!
//! `TemplateFeature.place`: pick a template with `templates.getRandomOrThrow(random)`,
//! then a rotation with `Util.getRandom(entry.rotations, random)`; the placement position
//! is centred by `getRotatedOffset` (rotated negative axis unit vector times half the
//! template size), and the blocks are laid down through `StructurePlaceSettings`
//! (rotation plus a processor chain).
//!
//! Only the processor subset used by `desert_well` is ported: `minecraft:rule` with
//! `block_match` / `always_true` predicates and the `append_loot` block-entity modifier.
//! Other processor kinds parse to unsupported so the whole placed feature is skipped
//! rather than silently mis-ported.

use mc_bamboo_data::{block_by_name, facts, state_facts};
use serde_json::Value;

use crate::{
    random::WorldgenRandom,
    structure::{
        state_transform::{Mirror, Rotation, transform},
        template::StructureTemplate,
    },
};

use super::{state::apply_property, world::FeatureWorld};

/// `TemplateFeature.TemplateEntry` 的旋转列表缺省值：`Rotation.values()`。
///
/// The default `rotations` list of `TemplateFeature.TemplateEntry`: `Rotation.values()`.
const ALL_ROTATIONS: [Rotation; 4] = [
    Rotation::None,
    Rotation::Clockwise90,
    Rotation::Clockwise180,
    Rotation::CounterClockwise90,
];

/// 模板里的一条加权条目。
///
/// One weighted template entry.
#[derive(Debug, Clone)]
struct TemplateEntry {
    id: String,
    rotations: Vec<Rotation>,
}

/// 一个方块实体的修饰器。
///
/// A block-entity modifier.
#[derive(Debug, Clone)]
enum BlockEntityModifier {
    /// `AppendLoot`：给方块的 `LootTable` / `LootTableSeed` 赋值。
    ///
    /// `AppendLoot`: writes the block's `LootTable` / `LootTableSeed`.
    AppendLoot { loot_table: String },
}

/// 一条 `rule` processor 的规则。
///
/// One rule of a `minecraft:rule` processor.
#[derive(Debug, Clone)]
struct Rule {
    input_blocks: Option<Vec<i32>>,
    always_true: bool,
    output_state: u32,
    block_entity_modifier: Option<BlockEntityModifier>,
}

/// `TemplateFeature` 的配置。
#[derive(Debug, Clone)]
pub struct TemplateFeature {
    entries: Vec<(i32, TemplateEntry)>,
    total_weight: i32,
    rules: Vec<Rule>,
    supported: bool,
}

impl TemplateFeature {
    pub fn parse(config: &Value) -> Result<Self, String> {
        let object = config
            .as_object()
            .ok_or("template feature must be an object")?;
        let templates = object
            .get("templates")
            .and_then(Value::as_array)
            .ok_or("template feature has no templates array")?;
        let mut entries = Vec::with_capacity(templates.len());
        let mut total_weight = 0;
        for entry in templates {
            let weight = entry.get("weight").and_then(Value::as_i64).unwrap_or(1) as i32;
            let data = entry.get("data").ok_or("template entry has no data")?;
            let id = data
                .get("id")
                .and_then(Value::as_str)
                .ok_or("template entry has no id")?
                .to_owned();
            let rotations = match data.get("rotations").and_then(Value::as_array) {
                Some(list) => list
                    .iter()
                    .filter_map(|value| value.as_str())
                    .filter_map(parse_rotation)
                    .collect::<Vec<_>>(),
                None => ALL_ROTATIONS.to_vec(),
            };
            total_weight += weight;
            entries.push((
                weight,
                TemplateEntry {
                    id,
                    rotations: if rotations.is_empty() {
                        ALL_ROTATIONS.to_vec()
                    } else {
                        rotations
                    },
                },
            ));
        }
        if entries.is_empty() || total_weight <= 0 {
            return Err("template feature needs at least one weighted template".to_owned());
        }

        let mut rules = Vec::new();
        let mut supported = true;
        if let Some(processors) = object.get("processors") {
            let list = processors
                .get("processors")
                .and_then(Value::as_array)
                .ok_or("processors has no processors array")?;
            for processor in list {
                let kind = processor
                    .get("processor_type")
                    .and_then(Value::as_str)
                    .ok_or("processor has no processor_type")?;
                if kind != "minecraft:rule" {
                    supported = false;
                    continue;
                }
                let Some(rule_list) = processor.get("rules").and_then(Value::as_array) else {
                    supported = false;
                    continue;
                };
                for rule in rule_list {
                    let Some(parsed) = Rule::parse(rule) else {
                        supported = false;
                        continue;
                    };
                    rules.push(parsed);
                }
            }
        }

        Ok(Self {
            entries,
            total_weight,
            rules,
            supported,
        })
    }

    /// 配置里的 processor 子集是否都已移植。
    ///
    /// Whether every processor in the config is ported.
    pub fn supported(&self) -> bool {
        self.supported
    }
}

/// `Rotation` 的 JSON 名字。
///
/// The JSON names of `Rotation`.
fn parse_rotation(name: &str) -> Option<Rotation> {
    Some(match name {
        "none" => Rotation::None,
        "clockwise_90" => Rotation::Clockwise90,
        "180" => Rotation::Clockwise180,
        "counterclockwise_90" => Rotation::CounterClockwise90,
        _ => return None,
    })
}

impl Rule {
    /// `RuleProcessor` 的一条规则。
    ///
    /// One rule of `RuleProcessor`.
    fn parse(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let input = object.get("input_predicate")?;
        let input_kind = input.get("predicate_type")?.as_str()?;
        let (input_blocks, always_true_input) = match input_kind {
            "minecraft:block_match" => {
                let name = input.get("block")?.as_str()?;
                let block = block_by_name(name)?;
                (
                    Some(vec![
                        mc_bamboo_data::block_id_of_state(block.first_state)? as i32
                    ]),
                    false,
                )
            }
            "minecraft:always_true" => (None, true),
            _ => return None,
        };
        // `location_predicate` 只支持 `always_true`（`desert_well` 就只用了它）。
        // Only `always_true` is handled for `location_predicate` (all `desert_well` uses).
        let location_kind = object
            .get("location_predicate")?
            .get("predicate_type")?
            .as_str()?;
        if location_kind != "minecraft:always_true" {
            return None;
        }
        let state = mc_bamboo_worldgen_state_from_json(object.get("output_state")?)?;
        let block_entity_modifier = match object.get("block_entity_modifier") {
            Some(modifier) => {
                let kind = modifier.get("type")?.as_str()?;
                if kind != "minecraft:append_loot" {
                    return None;
                }
                Some(BlockEntityModifier::AppendLoot {
                    loot_table: modifier.get("loot_table")?.as_str()?.to_owned(),
                })
            }
            None => None,
        };
        Some(Self {
            input_blocks,
            always_true: always_true_input,
            output_state: state,
            block_entity_modifier,
        })
    }

    /// 这条规则是否命中。
    ///
    /// Whether this rule matches.
    fn matches(&self, state: u32) -> bool {
        if self.always_true {
            return true;
        }
        let Some(blocks) = &self.input_blocks else {
            return false;
        };
        mc_bamboo_data::block_id_of_state(state).is_some_and(|id| blocks.contains(&(id as i32)))
    }
}

/// 解析一个方块状态 JSON（复用 `state::parse_state`，这里只是包一层避免名字冲突）。
///
/// Parse a block-state JSON (a thin wrapper over `state::parse_state`).
fn mc_bamboo_worldgen_state_from_json(value: &Value) -> Option<u32> {
    super::state::parse_state(value).ok()
}

/// `Rotation.rotate(axis.getNegative()).getUnitVec3i().multiply(size / 2)`。
///
/// `Rotation.rotate(axis.getNegative()).getUnitVec3i().multiply(size / 2)`.
fn rotated_offset(rotation: Rotation, axis: usize, size: i32) -> (i32, i32, i32) {
    // `Axis.X.getNegative()` = WEST，`Axis.Z.getNegative()` = NORTH。
    // `Axis.X.getNegative()` is WEST and `Axis.Z.getNegative()` is NORTH.
    let negative = if axis == 0 { (-1, 0, 0) } else { (0, 0, -1) };
    let rotated = rotate_direction(rotation, negative);
    let half = size / 2;
    (rotated.0 * half, rotated.1 * half, rotated.2 * half)
}

/// `Rotation.rotate(Direction)`。
///
/// `Rotation.rotate(Direction)`.
fn rotate_direction(rotation: Rotation, direction: (i32, i32, i32)) -> (i32, i32, i32) {
    match rotation {
        Rotation::None => direction,
        Rotation::Clockwise90 => (-direction.2, direction.1, direction.0),
        Rotation::Clockwise180 => (-direction.0, direction.1, -direction.2),
        Rotation::CounterClockwise90 => (direction.2, direction.1, -direction.0),
    }
}

/// `TemplateFeature.place`。
///
/// `TemplateFeature.place`.
pub fn place(
    feature: &TemplateFeature,
    world: &mut FeatureWorld<'_>,
    random: &WorldgenRandom,
    origin: (i32, i32, i32),
) -> bool {
    if !feature.supported {
        return false;
    }
    // `templates.getRandomOrThrow(random)`。
    // `templates.getRandomOrThrow(random)`.
    let mut selection = random.next_int(feature.total_weight);
    let mut chosen = &feature.entries[0].1;
    for (weight, entry) in &feature.entries {
        selection -= *weight;
        if selection < 0 {
            chosen = entry;
            break;
        }
    }
    // `Util.getRandom(entry.rotations(), random)`。
    // `Util.getRandom(entry.rotations(), random)`.
    let rotation_index = random.next_int(chosen.rotations.len() as i32) as usize;
    let rotation = chosen.rotations[rotation_index];

    let Ok(template) = StructureTemplate::load(&chosen.id) else {
        return false;
    };
    let offset_x = rotated_offset(rotation, 0, template.size.0);
    let offset_z = rotated_offset(rotation, 2, template.size.2);
    let pos = (
        origin.0 + offset_x.0 + offset_z.0,
        origin.1 + offset_x.1 + offset_z.1,
        origin.2 + offset_x.2 + offset_z.2,
    );

    // `settings.getRandomPalette(palettes, position)`：`TemplateFeature` 调了
    // `setRandom(random)`，所以 `getRandom(pos)` 直接返回地物自己的随机源，
    // 只有一套调色板也要取一次 `nextInt(1)`。少取这一次，`overlay` 后面的
    // `randomly_selected` 就会选错可疑的沙子的位置。
    // `settings.getRandomPalette(palettes, position)`: `TemplateFeature` called
    // `setRandom(random)`, so `getRandom(pos)` returns the feature random itself and a
    // single palette still draws `nextInt(1)`. Skipping it shifts the later
    // `randomly_selected` pick of the suspicious sand offset in the `overlay`.
    if template.palettes.is_empty() {
        return false;
    }
    let palette_index = random.next_int(template.palettes.len() as i32) as usize;
    let palette = &template.palettes[palette_index];
    let mut placed_any = false;
    // `placeInWorld` 按 `Palette.blocks()`（`buildInfoList` 的顺序）逐格放置。
    // `placeInWorld` walks `Palette.blocks()` in `buildInfoList` order.
    for index in crate::structure::template_place::ordered_blocks(&template, palette_index) {
        let block = &template.blocks[index];
        let Some(entry) = palette.get(block.state) else {
            continue;
        };
        let Some(mut state) = palette_state(entry) else {
            continue;
        };
        // `blockInfo.state.mirror(settings.getMirror()).rotate(settings.getRotation())`。
        // `blockInfo.state.mirror(settings.getMirror()).rotate(settings.getRotation())`.
        state = transform(state, Mirror::None, rotation);
        // processor 链：按顺序逐条试，第一条命中的改写方块。
        // The processor chain: try each rule in order; the first match rewrites the block.
        let mut loot: Option<&str> = None;
        for rule in &feature.rules {
            if rule.matches(state) {
                state = rule.output_state;
                if let Some(BlockEntityModifier::AppendLoot { loot_table }) =
                    &rule.block_entity_modifier
                {
                    loot = Some(loot_table);
                }
                break;
            }
        }
        let rotated = rotate_pos(rotation, block.pos);
        let target = (pos.0 + rotated.0, pos.1 + rotated.1, pos.2 + rotated.2);
        // `if (blockInfo.nbt != null) level.setBlock(pos, BARRIER, 820)`：先写屏障，
        // 把这一格原来的方块实体清掉，再写真正的状态。
        // `if (blockInfo.nbt != null) level.setBlock(pos, BARRIER, 820)`: the barrier
        // clears any previous block entity before the real state goes in.
        if loot.is_some() || block.nbt.is_some() {
            world.set_block_with_heightmaps(target.0, target.1, target.2, barrier_state());
        }
        world.set_block_with_heightmaps(target.0, target.1, target.2, state);
        placed_any = true;
        // `AppendLoot.apply` 用 `settings.getRandom(pos)`（按位置派生的
        // `RandomSource.create(Mth.getSeed(pos))`）取一次 `nextLong()` 当种子，
        // 和模板里的方块实体 nbt 一起交给 `loadWithComponents`。
        //
        // `AppendLoot.apply` draws one `nextLong()` from `settings.getRandom(pos)`
        // (a per-position `RandomSource.create(Mth.getSeed(pos))`) and hands the tag to
        // `loadWithComponents` along with the template's block-entity nbt.
        if let Some(table) = loot {
            // `RuleProcessor.processBlock` 的种子来自第 5 个参数 `processedBlockInfo.pos()`：
            // `processBlockInfos` 已经把它换成旋转并平移后的**世界坐标**。
            // `RuleProcessor.processBlock` seeds from its fifth argument
            // `processedBlockInfo.pos()`, which `processBlockInfos` has already turned into
            // the rotated and offset **world** position.
            let seed = mc_bamboo_blocks::LegacyRandom::new(crate::random::positional_seed(
                target.0, target.1, target.2,
            ))
            .next_long();
            let nbt = mc_bamboo_nbt::Tag::Compound(vec![
                (
                    "LootTable".to_owned(),
                    mc_bamboo_nbt::Tag::String(table.to_owned()),
                ),
                ("LootTableSeed".to_owned(), mc_bamboo_nbt::Tag::Long(seed)),
            ]);
            world.set_block_entity(target.0, target.1, target.2, &nbt);
        } else if let Some(tag) = &block.nbt {
            world.set_block_entity(target.0, target.1, target.2, tag);
        }
    }
    placed_any
}

/// `Blocks.BARRIER.defaultBlockState()`。
///
/// `Blocks.BARRIER.defaultBlockState()`.
fn barrier_state() -> u32 {
    block_by_name("minecraft:barrier").map_or(mc_bamboo_data::blocks::AIR, |b| b.default_state)
}

/// `Rotation.rotate(BlockPos)`：模板内的相对坐标跟着旋转。
///
/// `Rotation.rotate(BlockPos)`: the in-template relative position rotates with it.
fn rotate_pos(rotation: Rotation, pos: (i32, i32, i32)) -> (i32, i32, i32) {
    match rotation {
        Rotation::None => pos,
        // 绕 Y 轴顺时针 90°：`(x, z) -> (-z, x)`（方块坐标系里 x 向东、z 向南）。
        // 90° clockwise around Y: `(x, z) -> (-z, x)`.
        Rotation::Clockwise90 => (-pos.2, pos.1, pos.0),
        Rotation::Clockwise180 => (-pos.0, pos.1, -pos.2),
        Rotation::CounterClockwise90 => (pos.2, pos.1, -pos.0),
    }
}

/// 把 `PaletteEntry`（名字 + 属性）还原成状态 ID。
///
/// Rebuild a state ID from a `PaletteEntry` (name plus properties).
fn palette_state(entry: &super::super::structure::template::PaletteEntry) -> Option<u32> {
    let block = block_by_name(&entry.name)?;
    let mut state = block.default_state;
    for (key, value) in &entry.properties {
        state = apply_property(state, key, value)?;
    }
    Some(state)
}

/// 判断一个状态是否是空气（`append_loot` 之类的修饰器会跳过空气）。
///
/// Whether a state is air (modifiers such as `append_loot` skip air).
#[allow(dead_code)]
fn is_air(state: u32) -> bool {
    state_facts(state) & facts::AIR != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_desert_well_templates() {
        let root: Value =
            serde_json::from_str(mc_bamboo_data::worldgen::WORLDGEN).expect("worldgen json");
        let features = root
            .get("feature")
            .and_then(Value::as_object)
            .expect("feature registry");
        let config = features
            .get("minecraft:desert_well")
            .expect("desert_well feature");
        // `desert_well` 是 `overlay`，它内嵌的才是 `template`；直接测内嵌那两条。
        let children = config
            .get("features")
            .and_then(Value::as_array)
            .expect("desert_well children");
        let mut parsed_templates = 0;
        for child in children {
            let feature = child.get("feature").expect("child feature");
            if feature.get("type").and_then(Value::as_str) == Some("minecraft:template") {
                let template = TemplateFeature::parse(feature).expect("template parses");
                assert!(template.supported, "desert_well template must be supported");
                parsed_templates += 1;
            }
        }
        assert!(parsed_templates >= 2, "expected both desert_well templates");
    }

    #[test]
    fn rotation_offsets_centre_the_template() {
        // 顺时针 90° 时 X 轴负方向（WEST）转到 NORTH。
        // A 90° clockwise rotation maps WEST to NORTH.
        assert_eq!(rotated_offset(Rotation::Clockwise90, 0, 8).2, -4);
        assert_eq!(rotated_offset(Rotation::None, 0, 8).0, -4);
    }
}
