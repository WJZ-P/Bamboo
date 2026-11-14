//! `GeodeFeature`（紫水晶晶洞）。
//!
//! 照 `GeodeFeature.place`：先在 `distribution_points` 个随机点上取 `outer_wall_distance`
//! 的三轴线，任一格的方块是空气或是 `invalid_blocks` 时记一次「坏点」，超过
//! `invalid_blocks_threshold` 整个地物放弃；再按 3×3×3 大盒子的 `BlockPos.betweenClosed`
//! 顺序逐格算「到各分布点的距离倒数和 + 噪声偏移」，落在哪一层就写哪一层的方块。
//! 噪声是 `NormalNoise.createParity(-4, 1.0)` 用世界种子造的固定噪声，不消费放置随机源。
//! 最后从 `inner_placements` 里随机挑紫水晶芽，朝六个方向找第一个能长的地方放。
//!
//! `GeodeFeature.place`: sample `distribution_points` positions with three
//! `outer_wall_distance` draws each (counting invalid cells against
//! `invalid_blocks_threshold`), then walk the `min_gen_offset..=max_gen_offset` box in
//! `BlockPos.betweenClosed` order and pick a layer from the inverse-distance sum plus a
//! parity-noise offset. The noise is `NormalNoise.createParity(-4, 1.0)` seeded from the
//! world seed and consumes no placement randomness. Finally random `inner_placements`
//! entries are placed towards the first direction `canClusterGrowAtState` accepts.

use mc_bamboo_data::{blocks, facts, state_facts, state_fluid, tags};
use serde_json::Value;

use crate::{
    noise::NormalNoise,
    random::{Algorithm, Random, WorldgenRandom},
};

use super::{
    provider::IntProvider,
    state::{apply_property, parse_state},
    state_provider::StateProvider,
    world::FeatureWorld,
};

/// `GeodeFeature.NOISE_PARAMETERS`：`NormalNoise.createParity(-4, 1.0)`。
const NOISE_FIRST_OCTAVE: i32 = -4;
const NOISE_AMPLITUDES: [f64; 1] = [1.0];

/// `GeodeLayerSettings` 的四个厚度，缺省值照 `GeodeLayerSettings.CODEC`。
#[derive(Debug, Clone, Copy)]
pub struct GeodeLayerSettings {
    pub filling: f64,
    pub inner_layer: f64,
    pub middle_layer: f64,
    pub outer_layer: f64,
}

/// `GeodeCrackSettings`。
#[derive(Debug, Clone, Copy)]
pub struct GeodeCrackSettings {
    pub generate_crack_chance: f64,
    pub base_crack_size: f64,
    pub crack_point_offset: i32,
}

/// `GeodeBlockSettings`：五个状态提供者、内层芽列表、两个方块标签。
#[derive(Debug, Clone)]
pub struct GeodeBlockSettings {
    pub filling_provider: StateProvider,
    pub inner_layer_provider: StateProvider,
    pub alternate_inner_layer_provider: StateProvider,
    pub middle_layer_provider: StateProvider,
    pub outer_layer_provider: StateProvider,
    pub inner_placements: Vec<u32>,
    pub cannot_replace: Vec<i32>,
    pub invalid_blocks: Vec<i32>,
}

/// `GeodeFeature` 的配置。
#[derive(Debug, Clone)]
pub struct GeodeFeature {
    pub blocks: GeodeBlockSettings,
    pub layers: GeodeLayerSettings,
    pub crack: GeodeCrackSettings,
    pub use_potential_placements_chance: f64,
    pub use_alternate_layer0_chance: f64,
    pub placements_require_layer0_alternate: bool,
    pub outer_wall_distance: IntProvider,
    pub distribution_points: IntProvider,
    pub point_offset: IntProvider,
    pub min_gen_offset: i32,
    pub max_gen_offset: i32,
    pub noise_multiplier: f64,
    pub invalid_blocks_threshold: i32,
}

impl GeodeFeature {
    pub fn parse(config: &Value) -> Result<Self, String> {
        let object = config
            .as_object()
            .ok_or("geode feature must be an object")?;
        let field = |name: &str| -> Result<&Value, String> {
            object
                .get(name)
                .ok_or_else(|| format!("geode has no {name}"))
        };
        let blocks = field("blocks")?
            .as_object()
            .ok_or("geode blocks is not an object")?;
        let block_field = |name: &str| -> Result<&Value, String> {
            blocks
                .get(name)
                .ok_or_else(|| format!("geode blocks has no {name}"))
        };
        let layers = object
            .get("layers")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let layer = |name: &str, default: f64| -> f64 {
            layers.get(name).and_then(Value::as_f64).unwrap_or(default)
        };
        let crack = object
            .get("crack")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let crack_value = |name: &str, default: f64| -> f64 {
            crack.get(name).and_then(Value::as_f64).unwrap_or(default)
        };
        Ok(Self {
            blocks: GeodeBlockSettings {
                filling_provider: StateProvider::parse(block_field("filling_provider")?)?,
                inner_layer_provider: StateProvider::parse(block_field("inner_layer_provider")?)?,
                alternate_inner_layer_provider: StateProvider::parse(block_field(
                    "alternate_inner_layer_provider",
                )?)?,
                middle_layer_provider: StateProvider::parse(block_field("middle_layer_provider")?)?,
                outer_layer_provider: StateProvider::parse(block_field("outer_layer_provider")?)?,
                inner_placements: block_field("inner_placements")?
                    .as_array()
                    .ok_or("geode inner_placements is not an array")?
                    .iter()
                    .map(parse_state)
                    .collect::<Result<Vec<_>, String>>()?,
                cannot_replace: block_tag_list(block_field("cannot_replace")?)?,
                invalid_blocks: block_tag_list(block_field("invalid_blocks")?)?,
            },
            layers: GeodeLayerSettings {
                filling: layer("filling", 1.7),
                inner_layer: layer("inner_layer", 2.2),
                middle_layer: layer("middle_layer", 3.2),
                outer_layer: layer("outer_layer", 4.2),
            },
            crack: GeodeCrackSettings {
                generate_crack_chance: crack_value("generate_crack_chance", 1.0),
                base_crack_size: crack_value("base_crack_size", 2.0),
                crack_point_offset: crack
                    .get("crack_point_offset")
                    .and_then(Value::as_i64)
                    .unwrap_or(2) as i32,
            },
            use_potential_placements_chance: number(
                object,
                "use_potential_placements_chance",
                0.35,
            ),
            use_alternate_layer0_chance: number(object, "use_alternate_layer0_chance", 0.0),
            placements_require_layer0_alternate: object
                .get("placements_require_layer0_alternate")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            outer_wall_distance: match object.get("outer_wall_distance") {
                Some(value) => IntProvider::parse(value)?,
                None => IntProvider::Uniform { min: 4, max: 5 },
            },
            distribution_points: match object.get("distribution_points") {
                Some(value) => IntProvider::parse(value)?,
                None => IntProvider::Uniform { min: 3, max: 4 },
            },
            point_offset: match object.get("point_offset") {
                Some(value) => IntProvider::parse(value)?,
                None => IntProvider::Uniform { min: 1, max: 2 },
            },
            min_gen_offset: object
                .get("min_gen_offset")
                .and_then(Value::as_i64)
                .unwrap_or(-16) as i32,
            max_gen_offset: object
                .get("max_gen_offset")
                .and_then(Value::as_i64)
                .unwrap_or(16) as i32,
            noise_multiplier: number(object, "noise_multiplier", 0.05),
            invalid_blocks_threshold: object
                .get("invalid_blocks_threshold")
                .and_then(Value::as_i64)
                .ok_or("geode has no invalid_blocks_threshold")?
                as i32,
        })
    }

    /// 配置里的状态提供者是否都已移植。
    ///
    /// Whether every state provider in the config is ported.
    pub fn supported(&self) -> bool {
        self.blocks.filling_provider.supported()
            && self.blocks.inner_layer_provider.supported()
            && self.blocks.alternate_inner_layer_provider.supported()
            && self.blocks.middle_layer_provider.supported()
            && self.blocks.outer_layer_provider.supported()
    }
}

fn number(object: &serde_json::Map<String, Value>, name: &str, default: f64) -> f64 {
    object.get(name).and_then(Value::as_f64).unwrap_or(default)
}

/// `HolderSet<Block>`：单个 `#tag`、方块名或它们的数组，解析成方块注册表 ID。
///
/// `HolderSet<Block>`: a `#tag`, a block name, or an array of them, as block registry IDs.
fn block_tag_list(value: &Value) -> Result<Vec<i32>, String> {
    let entries: Vec<String> = match value {
        Value::String(name) => vec![name.clone()],
        Value::Array(items) => items
            .iter()
            .map(|item| {
                item.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| "block holder entry is not a string".to_owned())
            })
            .collect::<Result<Vec<_>, String>>()?,
        _ => return Err("block holder set is neither a string nor an array".to_owned()),
    };
    let mut ids = Vec::new();
    for entry in entries {
        match entry.strip_prefix('#') {
            Some(tag) => {
                let ids_in_tag = mc_bamboo_data::block_tag(tag)
                    .ok_or_else(|| format!("unknown block tag {tag}"))?;
                ids.extend_from_slice(ids_in_tag);
            }
            None => {
                let block = mc_bamboo_data::block_by_name(&entry)
                    .ok_or_else(|| format!("unknown block {entry}"))?;
                let id = mc_bamboo_data::block_id_of_state(block.first_state)
                    .ok_or_else(|| format!("unknown block {entry}"))?;
                ids.push(id as i32);
            }
        }
    }
    Ok(ids)
}

/// 晶洞用的 `NormalNoise`：种子取 `new WorldgenRandom(new LegacyRandomSource(level.getSeed()))`。
///
/// The geode `NormalNoise`, seeded by `new WorldgenRandom(new LegacyRandomSource(seed))`.
fn geode_noise(seed: i64) -> crate::noise::Noise {
    let parameters = NormalNoise::create_parity(NOISE_FIRST_OCTAVE, &NOISE_AMPLITUDES);
    parameters.create(&Random::Legacy(mc_bamboo_blocks::LegacyRandom::new(seed)))
}

/// `GeodeFeature.place`。
///
/// `GeodeFeature.place`.
pub fn place(
    geode: &GeodeFeature,
    world: &mut FeatureWorld<'_>,
    random: &WorldgenRandom,
    origin: (i32, i32, i32),
) -> bool {
    let (ox, oy, oz) = origin;
    let noise = geode_noise(world.seed);
    let num_points = geode.distribution_points.sample(random);
    let crack_size_adjustment =
        f64::from(num_points) / f64::from(geode.outer_wall_distance.max_value());
    // 1/sqrt 的各层半径，顺序照官方局部变量。
    // The per-layer 1/sqrt radii, in the official local-variable order.
    let inner_air = 1.0 / geode.layers.filling.sqrt();
    let innermost_block_layer = 1.0 / (geode.layers.inner_layer + crack_size_adjustment).sqrt();
    let inner_crust = 1.0 / (geode.layers.middle_layer + crack_size_adjustment).sqrt();
    let outer_crust = 1.0 / (geode.layers.outer_layer + crack_size_adjustment).sqrt();
    let crack_extra = if num_points > 3 {
        crack_size_adjustment
    } else {
        0.0
    };
    let crack_size =
        1.0 / (geode.crack.base_crack_size + random.next_double() / 2.0 + crack_extra).sqrt();
    let should_generate_crack = random.next_float() < geode.crack.generate_crack_chance as f32;
    let mut invalid_points = 0;

    // 每个分布点：(坐标, point_offset 采样的平方偏移量)。
    // Each distribution point: its position and the squared `point_offset` sample.
    let mut points: Vec<((i32, i32, i32), i32)> = Vec::with_capacity(num_points.max(0) as usize);
    for _ in 0..num_points {
        let dx = geode.outer_wall_distance.sample(random);
        let dy = geode.outer_wall_distance.sample(random);
        let dz = geode.outer_wall_distance.sample(random);
        let pos = (ox + dx, oy + dy, oz + dz);
        let state = world.get_block(pos.0, pos.1, pos.2);
        if state_facts(state) & facts::AIR != 0 || is_block_in(state, &geode.blocks.invalid_blocks)
        {
            invalid_points += 1;
            if invalid_points > geode.invalid_blocks_threshold {
                return false;
            }
        }
        points.push((pos, geode.point_offset.sample(random)));
    }

    let mut crack_points: Vec<(i32, i32, i32)> = Vec::new();
    if should_generate_crack {
        let offset_index = random.next_int(4);
        let crack_offset = num_points * 2 + 1;
        match offset_index {
            0 => {
                crack_points.push((ox + crack_offset, oy + 7, oz));
                crack_points.push((ox + crack_offset, oy + 5, oz));
                crack_points.push((ox + crack_offset, oy + 1, oz));
            }
            1 => {
                crack_points.push((ox, oy + 7, oz + crack_offset));
                crack_points.push((ox, oy + 5, oz + crack_offset));
                crack_points.push((ox, oy + 1, oz + crack_offset));
            }
            2 => {
                crack_points.push((ox + crack_offset, oy + 7, oz + crack_offset));
                crack_points.push((ox + crack_offset, oy + 5, oz + crack_offset));
                crack_points.push((ox + crack_offset, oy + 1, oz + crack_offset));
            }
            _ => {
                crack_points.push((ox, oy + 7, oz));
                crack_points.push((ox, oy + 5, oz));
                crack_points.push((ox, oy + 1, oz));
            }
        }
    }

    let mut potential_crystal_placements: Vec<(i32, i32, i32)> = Vec::new();
    let min = geode.min_gen_offset;
    let max = geode.max_gen_offset;
    // `BlockPos.betweenClosed`：x 变化最快，其次 y，最后 z。
    // `BlockPos.betweenClosed`: x varies fastest, then y, z slowest.
    for dz in min..=max {
        for dy in min..=max {
            for dx in min..=max {
                let pos = (ox + dx, oy + dy, oz + dz);
                let noise_offset =
                    f64::from(noise.get(f64::from(pos.0), f64::from(pos.1), f64::from(pos.2)))
                        * geode.noise_multiplier;
                let mut dist_sum_shell = 0.0;
                for (point, offset) in &points {
                    dist_sum_shell +=
                        inv_sqrt(dist_sqr(pos, *point) + f64::from(*offset)) + noise_offset;
                }
                if dist_sum_shell < outer_crust {
                    continue;
                }
                if dist_sum_shell >= inner_air {
                    place_provider(
                        world,
                        random,
                        pos,
                        &geode.blocks.filling_provider,
                        &geode.blocks.cannot_replace,
                    );
                    continue;
                }
                let mut dist_sum_crack = 0.0;
                for point in &crack_points {
                    dist_sum_crack +=
                        inv_sqrt(dist_sqr(pos, *point) + f64::from(geode.crack.crack_point_offset))
                            + noise_offset;
                }
                if should_generate_crack && dist_sum_crack >= crack_size {
                    place_if_replaceable(world, pos, blocks::AIR, &geode.blocks.cannot_replace);
                    // 官方对六个相邻格的流体排一次 tick，不改方块也不消费随机数。
                    // The official code schedules a tick for adjacent fluids; no blocks and
                    // no randomness are involved.
                } else if dist_sum_shell >= innermost_block_layer {
                    let use_alternate =
                        random.next_float() < geode.use_alternate_layer0_chance as f32;
                    let provider = if use_alternate {
                        &geode.blocks.alternate_inner_layer_provider
                    } else {
                        &geode.blocks.inner_layer_provider
                    };
                    place_provider(world, random, pos, provider, &geode.blocks.cannot_replace);
                    if (!geode.placements_require_layer0_alternate || use_alternate)
                        && random.next_float() < geode.use_potential_placements_chance as f32
                    {
                        potential_crystal_placements.push(pos);
                    }
                } else if dist_sum_shell >= inner_crust {
                    place_provider(
                        world,
                        random,
                        pos,
                        &geode.blocks.middle_layer_provider,
                        &geode.blocks.cannot_replace,
                    );
                } else if dist_sum_shell >= outer_crust {
                    place_provider(
                        world,
                        random,
                        pos,
                        &geode.blocks.outer_layer_provider,
                        &geode.blocks.cannot_replace,
                    );
                }
            }
        }
    }

    for crystal_pos in potential_crystal_placements {
        // `Util.getRandom(innerPlacements, random)`。
        // `Util.getRandom(innerPlacements, random)`.
        let index = random.next_int(geode.blocks.inner_placements.len() as i32) as usize;
        let mut block_state = geode.blocks.inner_placements[index];
        for direction in DIRECTIONS {
            if has_property(block_state, "facing") {
                block_state = apply_property(block_state, "facing", direction_name(direction))
                    .unwrap_or(block_state);
            }
            let place_pos = offset(crystal_pos, direction);
            let place_state = world.get_block(place_pos.0, place_pos.1, place_pos.2);
            if has_property(block_state, "waterlogged") {
                // `placeState.getFluidState().isSource()`。
                // `placeState.getFluidState().isSource()`.
                let source = state_fluid(place_state).source;
                block_state = apply_property(
                    block_state,
                    "waterlogged",
                    if source { "true" } else { "false" },
                )
                .unwrap_or(block_state);
            }
            if can_cluster_grow_at_state(place_state) {
                place_if_replaceable(world, place_pos, block_state, &geode.blocks.cannot_replace);
                break;
            }
        }
    }
    true
}

/// `Direction.values()` 的顺序：DOWN, UP, NORTH, SOUTH, WEST, EAST。
const DIRECTIONS: [(i32, i32, i32); 6] = [
    (0, -1, 0),
    (0, 1, 0),
    (0, 0, -1),
    (0, 0, 1),
    (-1, 0, 0),
    (1, 0, 0),
];

fn direction_name(direction: (i32, i32, i32)) -> &'static str {
    match direction {
        (0, -1, 0) => "down",
        (0, 1, 0) => "up",
        (0, 0, -1) => "north",
        (0, 0, 1) => "south",
        (-1, 0, 0) => "west",
        _ => "east",
    }
}

fn offset(pos: (i32, i32, i32), direction: (i32, i32, i32)) -> (i32, i32, i32) {
    (
        pos.0 + direction.0,
        pos.1 + direction.1,
        pos.2 + direction.2,
    )
}

/// `Vec3i.distSqr`：两格中心到中心的距离平方（`distToLowCornerSqr` 的整数版）。
///
/// `Vec3i.distSqr`: squared distance between the two block corners.
fn dist_sqr(a: (i32, i32, i32), b: (i32, i32, i32)) -> f64 {
    let dx = f64::from(a.0 - b.0);
    let dy = f64::from(a.1 - b.1);
    let dz = f64::from(a.2 - b.2);
    dx * dx + dy * dy + dz * dz
}

/// `Mth.invSqrt(double)`：joml 的 `1.0 / Math.sqrt(x)`。
///
/// `Mth.invSqrt(double)`: joml's `1.0 / Math.sqrt(x)`.
fn inv_sqrt(x: f64) -> f64 {
    1.0 / x.sqrt()
}

/// `state.is(HolderSet<Block>)`。
fn is_block_in(state: u32, ids: &[i32]) -> bool {
    mc_bamboo_data::block_id_of_state(state).is_some_and(|id| ids.contains(&(id as i32)))
}

/// 状态所属方块是否有这个属性。
fn has_property(state: u32, name: &str) -> bool {
    mc_bamboo_data::block_of_state(state).is_some_and(|block| {
        block
            .properties
            .iter()
            .any(|property| property.name == name)
    })
}

/// `BuddingAmethystBlock.canClusterGrowAtState`：空气，或满格的水。
///
/// `BuddingAmethystBlock.canClusterGrowAtState`: air, or full water.
fn can_cluster_grow_at_state(state: u32) -> bool {
    state_facts(state) & facts::AIR != 0
        || (mc_bamboo_data::block_of_state(state)
            .is_some_and(|block| block.name == "minecraft:water")
            && state_fluid(state).amount >= 8)
}
/// `safeSetBlock(level, pos, state, canReplace)`。
fn place_if_replaceable(
    world: &mut FeatureWorld<'_>,
    pos: (i32, i32, i32),
    state: u32,
    cannot_replace: &[i32],
) {
    let current = world.get_block(pos.0, pos.1, pos.2);
    if !is_block_in(current, cannot_replace) {
        world.set_block_with_heightmaps(pos.0, pos.1, pos.2, state);
    }
}

/// `safeSetBlock(level, pos, provider.getState(level, random, pos), canReplace)`。
fn place_provider(
    world: &mut FeatureWorld<'_>,
    random: &WorldgenRandom,
    pos: (i32, i32, i32),
    provider: &StateProvider,
    cannot_replace: &[i32],
) {
    let Some(state) = provider.get_optional_state(world, random, pos) else {
        return;
    };
    place_if_replaceable(world, pos, state, cannot_replace);
}

/// 引用的标签都导出过，缺失时说明数据不完整。
const _: () = {
    let _ = tags::block_tags::GEODE_INVALID_BLOCKS;
    let _ = tags::block_tags::FEATURES_CANNOT_REPLACE;
    let _ = Algorithm::Xoroshiro;
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_with_codec_defaults() {
        let root: Value =
            serde_json::from_str(mc_bamboo_data::worldgen::WORLDGEN).expect("worldgen json");
        let features = root
            .get("feature")
            .and_then(Value::as_object)
            .expect("feature registry");
        let config = features
            .get("minecraft:amethyst_geode")
            .expect("amethyst_geode config");
        let geode = GeodeFeature::parse(config).expect("geode parses");
        // 导出的配置省略了 distribution_points / point_offset，走 `optionalFieldOf` 默认值。
        // The exported config omits distribution_points / point_offset: codec defaults apply.
        assert_eq!(
            geode.distribution_points,
            IntProvider::Uniform { min: 3, max: 4 }
        );
        assert_eq!(geode.point_offset, IntProvider::Uniform { min: 1, max: 2 });
        assert_eq!(
            geode.outer_wall_distance,
            IntProvider::Uniform { min: 4, max: 6 }
        );
        assert_eq!(geode.layers.filling, 1.7);
        assert_eq!(geode.crack.crack_point_offset, 2);
        assert!(geode.supported());
    }

    #[test]
    fn parity_noise_matches_the_official_parameters() {
        // `NormalNoise.createParity(-4, 1.0)`：单八度，奇偶归一化后的振幅是 1.0。
        // `NormalNoise.createParity(-4, 1.0)`: one octave, parity-normalized to amplitude 1.
        let noise = NormalNoise::create_parity(NOISE_FIRST_OCTAVE, &NOISE_AMPLITUDES);
        assert_eq!(noise.parameters().base_octave, -4);
        assert_eq!(noise.parameters().octave_count, 1);
        assert!(noise.parameters().base_amplitude > 0.0);
    }
}
