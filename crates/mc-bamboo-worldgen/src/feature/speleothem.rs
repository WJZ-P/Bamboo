//! `SpeleothemFeature`：`pointed_dripstone` / `sulfur_spike` 的叶子地物。
//!
//! 官方 26.3 把旧的 `PointedDripstoneFeature` 换成了通用的 `SpeleothemFeature`：
//! 先按上下相邻格决定尖端方向，再在根部铺一小片 `base_block`，最后用
//! `SpeleothemUtils.growSpeleothem` 长出一列 pointed 方块。随机数的取数顺序照
//! `getTipDirection` → `createPatchOfBaseBlocks` → `place` 里的 `nextFloat`。
//!
//! `SpeleothemFeature`: the `pointed_dripstone` / `sulfur_spike` leaf. 26.3 replaced
//! the old `PointedDripstoneFeature` with this generic one: pick the tip direction from
//! the neighbours above/below, scatter a patch of `base_block` around the root, then
//! grow a pointed column via `SpeleothemUtils.growSpeleothem`. Random consumption
//! follows `getTipDirection` → `createPatchOfBaseBlocks` → the `nextFloat` in `place`.

use serde_json::Value;

use crate::random::WorldgenRandom;

use super::{
    dripstone_utils::{
        HORIZONTAL, Materials, grow_speleothem, is_base, is_empty_or_water,
        place_base_block_if_possible,
    },
    state::parse_state,
    world::FeatureWorld,
};

type Pos = (i32, i32, i32);

/// `SpeleothemFeature` 的配置。
///
/// The `SpeleothemFeature` config.
#[derive(Debug, Clone)]
pub struct SpeleothemFeature {
    pub base_block: u32,
    pub pointed_default: u32,
    pub replaceable: String,
    pub chance_of_taller_generation: f32,
    pub chance_of_directional_spread: f32,
    pub chance_of_spread_radius2: f32,
    pub chance_of_spread_radius3: f32,
}

impl SpeleothemFeature {
    pub fn parse(config: &Value) -> Result<Self, String> {
        let object = config.as_object().ok_or("speleothem must be an object")?;
        let chance = |name: &str, fallback: f32| -> f32 {
            object
                .get(name)
                .and_then(Value::as_f64)
                .map(|value| value as f32)
                .unwrap_or(fallback)
        };
        Ok(Self {
            base_block: parse_state(
                object
                    .get("base_block")
                    .ok_or("speleothem has no base_block")?,
            )?,
            pointed_default: parse_state(
                object
                    .get("pointed_block")
                    .ok_or("speleothem has no pointed_block")?,
            )?,
            replaceable: parse_replaceable(
                object
                    .get("replaceable_blocks")
                    .ok_or("speleothem has no replaceable_blocks")?,
            )?,
            chance_of_taller_generation: chance("chance_of_taller_generation", 0.2),
            chance_of_directional_spread: chance("chance_of_directional_spread", 0.7),
            chance_of_spread_radius2: chance("chance_of_spread_radius2", 0.5),
            chance_of_spread_radius3: chance("chance_of_spread_radius3", 0.5),
        })
    }
}

/// `HolderSet<Block>` 只支持单标签（`"#minecraft:dripstone_replaceable_blocks"`），
/// 导出的两个配置都是这个形态。
///
/// `HolderSet<Block>` only supports a single tag; both exported configs are that shape.
fn parse_replaceable(value: &Value) -> Result<String, String> {
    let tag = value
        .as_str()
        .ok_or("speleothem replaceable_blocks must be a tag string")?;
    tag.strip_prefix('#')
        .map(str::to_owned)
        .ok_or_else(|| format!("speleothem replaceable_blocks is not a tag: {tag}"))
}

/// `SpeleothemFeature.place`。`origin` 是尖端那一格。
///
/// `SpeleothemFeature.place`; `origin` is the tip cell.
pub fn place(
    feature: &SpeleothemFeature,
    world: &mut FeatureWorld<'_>,
    random: &WorldgenRandom,
    origin: Pos,
) -> bool {
    let Some(tip) = tip_direction(feature, world, origin, random) else {
        return false;
    };
    // `origin.relative(tipDirection.getOpposite())`：根部在尖端方向的反向一格。
    // `origin.relative(tipDirection.getOpposite())`: the root is one step back.
    let root = (origin.0, origin.1 - tip, origin.2);
    create_patch_of_base_blocks(feature, world, random, root);
    let taller = random.next_float() < feature.chance_of_taller_generation
        && is_empty_or_water(world.get_block(origin.0, origin.1 + tip, origin.2));
    grow_speleothem(
        world,
        origin,
        tip == 1,
        if taller { 2 } else { 1 },
        false,
        Materials {
            base_block: feature.base_block,
            pointed_default: feature.pointed_default,
            replaceable: &feature.replaceable,
        },
    );
    true
}

/// `SpeleothemFeature.getTipDirection`：上下都可能是根部时取一次 `nextBoolean`；
/// 只有一边可以时直接返回，都不行返回 `None`（不消费随机数）。
///
/// `SpeleothemFeature.getTipDirection`: when both sides can host a base it consumes one
/// `nextBoolean`; when only one can it returns directly; `None` consumes nothing.
fn tip_direction(
    feature: &SpeleothemFeature,
    world: &FeatureWorld<'_>,
    pos: Pos,
    random: &WorldgenRandom,
) -> Option<i32> {
    let above = is_base(
        world.get_block(pos.0, pos.1 + 1, pos.2),
        feature.base_block,
        &feature.replaceable,
    );
    let below = is_base(
        world.get_block(pos.0, pos.1 - 1, pos.2),
        feature.base_block,
        &feature.replaceable,
    );
    if above && below {
        // `Direction.DOWN` 是 -1，`Direction.UP` 是 +1。
        // `Direction.DOWN` is -1, `Direction.UP` is +1.
        return Some(if random.next_boolean() { -1 } else { 1 });
    }
    if above {
        return Some(-1);
    }
    below.then_some(1)
}

/// `SpeleothemFeature.createPatchOfBaseBlocks`：四个水平方向各试一次，命中后
/// 再叠两层随机方向的扩散。三层 `nextFloat` 的消费顺序照官方短路结构。
///
/// `SpeleothemFeature.createPatchOfBaseBlocks`: try each of the four horizontal
/// directions, then optionally spread twice in random directions. The three
/// `nextFloat` calls follow the official short-circuit nesting.
fn create_patch_of_base_blocks(
    feature: &SpeleothemFeature,
    world: &mut FeatureWorld<'_>,
    random: &WorldgenRandom,
    pos: Pos,
) {
    place_base_block_if_possible(world, pos, feature.base_block, &feature.replaceable);
    for &ordinal in &HORIZONTAL {
        if random.next_float() > feature.chance_of_directional_spread {
            continue;
        }
        let pos1 = offset(pos, ordinal);
        place_base_block_if_possible(world, pos1, feature.base_block, &feature.replaceable);
        if random.next_float() > feature.chance_of_spread_radius2 {
            continue;
        }
        let pos2 = offset(pos1, random.next_int(6) as usize);
        place_base_block_if_possible(world, pos2, feature.base_block, &feature.replaceable);
        if random.next_float() > feature.chance_of_spread_radius3 {
            continue;
        }
        let pos3 = offset(pos2, random.next_int(6) as usize);
        place_base_block_if_possible(world, pos3, feature.base_block, &feature.replaceable);
    }
}

/// `pos.relative(direction)`，`direction` 是 `Direction.values()` 的序号
/// （DOWN, UP, NORTH, SOUTH, WEST, EAST）。
///
/// `pos.relative(direction)` where `direction` indexes `Direction.values()`
/// (DOWN, UP, NORTH, SOUTH, WEST, EAST).
fn offset(pos: Pos, ordinal: usize) -> Pos {
    let step = super::dripstone_utils::DIRECTION_OFFSETS[ordinal];
    (pos.0 + step.0, pos.1 + step.1, pos.2 + step.2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_bamboo_data::blocks;

    fn parse(text: &str) -> SpeleothemFeature {
        let value: Value = serde_json::from_str(text).unwrap();
        SpeleothemFeature::parse(&value).unwrap()
    }

    #[test]
    fn parses_both_exported_speleothem_configs() {
        let dripstone = parse(
            r##"{
                "base_block": "minecraft:dripstone_block",
                "pointed_block": "minecraft:pointed_dripstone",
                "replaceable_blocks": "#minecraft:dripstone_replaceable_blocks",
                "type": "minecraft:speleothem"
            }"##,
        );
        assert_eq!(dripstone.base_block, blocks::DRIPSTONE_BLOCK);
        assert_eq!(dripstone.pointed_default, blocks::POINTED_DRIPSTONE);
        assert_eq!(
            dripstone.replaceable,
            "minecraft:dripstone_replaceable_blocks"
        );
        // 四个概率字段缺省时按官方默认值。
        // The four chance fields fall back to the official defaults.
        assert_eq!(dripstone.chance_of_taller_generation, 0.2);
        assert_eq!(dripstone.chance_of_directional_spread, 0.7);
        assert_eq!(dripstone.chance_of_spread_radius2, 0.5);
        assert_eq!(dripstone.chance_of_spread_radius3, 0.5);

        let sulfur = parse(
            r##"{
                "base_block": "minecraft:sulfur",
                "pointed_block": "minecraft:sulfur_spike",
                "replaceable_blocks": "#minecraft:sulfur_spike_replaceable_blocks",
                "type": "minecraft:speleothem"
            }"##,
        );
        assert_eq!(
            sulfur.replaceable,
            "minecraft:sulfur_spike_replaceable_blocks"
        );
    }

    #[test]
    fn horizontal_ordinals_are_north_east_south_west() {
        // `Direction.Plane.HORIZONTAL` 不是 `values()` 的前四个。
        // `Direction.Plane.HORIZONTAL` is not the first four of `values()`.
        let offsets: Vec<(i32, i32, i32)> = HORIZONTAL
            .iter()
            .map(|&ordinal| super::super::dripstone_utils::DIRECTION_OFFSETS[ordinal])
            .collect();
        assert_eq!(offsets, vec![(0, 0, -1), (1, 0, 0), (0, 0, 1), (-1, 0, 0)]);
    }
}
