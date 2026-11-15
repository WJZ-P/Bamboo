//! 组合类地物与递归放置 `PlacedFeature` 的框架。
//!
//! 官方 `Feature` 是 interface，组合类（`RandomSelectorFeature` 等）的 `place` 会再调
//! 一个子 `PlacedFeature.place(level, generator, random, origin)`，走 `FeaturePlacer.place`
//! （`placeWithBiomeCheck=false`，`topFeature` 为空）。子项可以是注册表引用，也可以是内联
//! 的 `{"feature":..,"placement":[..]}`；子 `Feature.place` 又可能是叶子或另一个组合类。
//! 这里把「叶子」和「组合类」统一成一个递归入口 [`place_placed`]，随机数全程共用同一个
//! `WorldgenRandom`，顺序照官方字节码。
//!
//! Composition features and recursive `PlacedFeature` placement. A composition's `place`
//! calls a child `PlacedFeature.place`, which re-runs the placement stack DFS from the same
//! origin with `placeWithBiomeCheck=false`. Children may be registry references or inline
//! configs; [`place_placed`] unifies leaves and compositions into one recursive entry point
//! sharing a single `WorldgenRandom` in official order.

use serde_json::Value;

use crate::random::WorldgenRandom;

use super::{
    dispatch::{parse_leaf, place_leaf},
    freeze_top_layer,
    placement::{Modifier, PlaceOutcome, PlacementEnv, place_candidates},
    registry::Registry,
    run::resolve_feature,
    world::FeatureWorld,
};

type Pos = (i32, i32, i32);

/// `Holder<PlacedFeature>`：注册表引用名，或内联的 `{"feature":..,"placement":[..]}`。
///
/// `Holder<PlacedFeature>`: a registry name or an inline placed-feature object.
pub(crate) enum Child<'a> {
    Registry(&'a str),
    Inline {
        feature: &'a Value,
        placement: &'a [Value],
    },
}

impl<'a> Child<'a> {
    pub(crate) fn parse(value: &'a Value) -> Option<Self> {
        match value {
            Value::String(name) => Some(Self::Registry(name.as_str())),
            Value::Object(object) => Some(Self::Inline {
                feature: object.get("feature")?,
                placement: object.get("placement")?.as_array()?.as_slice(),
            }),
            _ => None,
        }
    }

    fn resolve(&self, registry: &'a Registry) -> Option<(&'a Value, &'a [Value])> {
        match self {
            Self::Registry(name) => {
                let id = registry.placed_id(name)?;
                let def = registry.placed(id)?;
                Some((&def.feature, def.placement.as_slice()))
            }
            Self::Inline { feature, placement } => Some((*feature, *placement)),
        }
    }
}

/// `HolderSet<PlacedFeature>`：数组、单个条目，或（暂不支持的）`#tag`。
///
/// `HolderSet<PlacedFeature>`: an array, a single entry, or an (unsupported) `#tag`.
fn parse_holder_set(value: &Value) -> Option<Vec<Child<'_>>> {
    match value {
        Value::Array(items) => items.iter().map(Child::parse).collect(),
        _ => Child::parse(value).map(|child| vec![child]),
    }
}

/// 已解析的组合类地物。
///
/// A parsed composition feature.
enum Composition<'a> {
    RandomSelector {
        features: Vec<(f32, Child<'a>)>,
        default: Child<'a>,
    },
    WeightedRandomSelector {
        features: Vec<(i32, Child<'a>)>,
    },
    SimpleRandomSelector {
        features: Vec<Child<'a>>,
    },
    RandomBooleanSelector {
        feature_true: Child<'a>,
        feature_false: Child<'a>,
    },
    Sequence {
        features: Vec<Child<'a>>,
    },
}

impl<'a> Composition<'a> {
    fn parse(kind: &str, config: &'a Value) -> Option<Self> {
        let object = config.as_object()?;
        match kind {
            "minecraft:random_selector" => {
                let features = object
                    .get("features")?
                    .as_array()?
                    .iter()
                    .map(|entry| {
                        let chance = entry.get("chance")?.as_f64()? as f32;
                        Some((chance, Child::parse(entry.get("feature")?)?))
                    })
                    .collect::<Option<Vec<_>>>()?;
                Some(Self::RandomSelector {
                    features,
                    default: Child::parse(object.get("default")?)?,
                })
            }
            "minecraft:weighted_random_selector" => {
                let features = object
                    .get("features")?
                    .as_array()?
                    .iter()
                    .map(|entry| {
                        let weight = entry.get("weight")?.as_i64()? as i32;
                        Some((weight, Child::parse(entry.get("data")?)?))
                    })
                    .collect::<Option<Vec<_>>>()?;
                Some(Self::WeightedRandomSelector { features })
            }
            "minecraft:simple_random_selector" => Some(Self::SimpleRandomSelector {
                features: parse_holder_set(object.get("features")?)?,
            }),
            "minecraft:sequence" => Some(Self::Sequence {
                features: parse_holder_set(object.get("features")?)?,
            }),
            "minecraft:random_boolean_selector" => Some(Self::RandomBooleanSelector {
                feature_true: Child::parse(object.get("feature_true")?)?,
                feature_false: Child::parse(object.get("feature_false")?)?,
            }),
            _ => None,
        }
    }
}

/// 放置一个 `PlacedFeature`（顶层或组合类的子项）。`env.top_feature` 是 `Some(id)` 时
/// 对应官方 `placeWithBiomeCheck`（顶层）；组合类子项由 [`place_child`] 用 `None` 递归。
///
/// Place one `PlacedFeature` (top-level or a composition child). `env.top_feature=Some(id)`
/// is the official `placeWithBiomeCheck` (top level); composition children recurse with
/// `None` via [`place_child`].
pub fn place_placed<'a>(
    registry: &'a Registry,
    feature_value: &'a Value,
    placement: &'a [Value],
    world: &mut FeatureWorld<'_>,
    env: &mut PlacementEnv<'_>,
    random: &WorldgenRandom,
    origin: Pos,
) -> PlaceOutcome {
    let Some((kind, config)) = resolve_feature(registry, feature_value) else {
        return PlaceOutcome::unsupported();
    };
    let modifiers: Vec<Modifier> = match placement
        .iter()
        .map(Modifier::parse)
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(modifiers) => modifiers,
        Err(_) => return PlaceOutcome::unsupported(),
    };
    if modifiers.iter().any(|m| !m.is_supported()) {
        return PlaceOutcome::unsupported();
    }
    let kind = kind.as_str();
    let mut place = |world: &mut FeatureWorld<'_>, env: &mut PlacementEnv<'_>, pos: Pos| {
        place_feature(kind, config, registry, world, env, random, pos)
    };
    place_candidates(&modifiers, world, env, random, origin, &mut place)
}

/// 一个候选位置上的 `Feature.place`：先试叶子，再试组合类，都没有则不支持。
///
/// `Feature.place` at one candidate: try a leaf, then a composition, else unsupported.
fn place_feature<'a>(
    kind: &str,
    config: &'a Value,
    registry: &'a Registry,
    world: &mut FeatureWorld<'_>,
    env: &mut PlacementEnv<'_>,
    random: &WorldgenRandom,
    pos: Pos,
) -> PlaceOutcome {
    // `freeze_top_layer` 要逐点群系解析，只有主循环能提供 `BiomeManager` 回调，
    // 所以它不走 [`place_leaf`]，在这里直接接。
    // `freeze_top_layer` needs per-point biome resolution, which only the main loop can
    // supply via a `BiomeManager` callback, so it bypasses [`place_leaf`] here.
    if kind == "minecraft:freeze_top_layer" {
        let placed = freeze_top_layer::place(world, random, pos, &mut *env.biome_at);
        return PlaceOutcome {
            supported: true,
            placed,
        };
    }
    if let Some(leaf) = parse_leaf(kind, config, &|name| registry.state_provider(name)) {
        let placed = place_leaf(&leaf, world, random, env.region_random, pos);
        return PlaceOutcome {
            supported: true,
            placed,
        };
    }
    // `vegetation_patch` 需要注册表和 `PlacementEnv` 才能递归放子地物，不能当叶子。
    //
    // **只接 `vegetation_patch`**：`moss_patch`（floor）、`moss_patch_ceiling`
    // （ceiling，还有 10 处未解决的 `cave_vines` 错位）、`pale_moss_patch` 都跑过逐方块
    // 对拍；ceiling 的差异根因是 `JavaPosSet` 没实现 `HashMap` 树化，属已知限制。
    // `waterlogged_vegetation_patch`（`clay_pool_with_dripleaves`）**没有任何对拍**，
    // 所以不接，标 unsupported。
    //
    // `vegetation_patch` needs the registry and `PlacementEnv` to recurse into its child
    // feature, so it cannot be a leaf.
    //
    // **Only `vegetation_patch` is wired**: `moss_patch` (floor), `moss_patch_ceiling`
    // (ceiling, still 10 unresolved `cave_vines` offsets) and `pale_moss_patch` all went
    // through per-block comparison; the ceiling difference traces back to `JavaPosSet` not
    // implementing `HashMap` treeification, a documented limit.
    // `waterlogged_vegetation_patch` (`clay_pool_with_dripleaves`) has **no comparison at
    // all**, so it stays unsupported.
    if kind == "minecraft:vegetation_patch" || kind == "minecraft:waterlogged_vegetation_patch" {
        let Some(patch) = super::vegetation_patch::VegetationPatch::parse(kind, config) else {
            return PlaceOutcome::unsupported();
        };
        return patch.place(registry, world, env, random, pos);
    }
    // `root_system` 同样要注册表和 `PlacementEnv`（它递归调 `treeFeature.place`）。
    //
    // `root_system` likewise needs the registry and `PlacementEnv` because it recurses
    // into `treeFeature.place`.
    if kind == "minecraft:root_system" {
        let Some(system) =
            super::root_system::RootSystem::parse(config, &|name| registry.state_provider(name))
        else {
            return PlaceOutcome::unsupported();
        };
        return system.place(registry, world, env, random, pos);
    }
    let Some(comp) = Composition::parse(kind, config) else {
        return PlaceOutcome::unsupported();
    };
    place_composition(&comp, registry, world, env, random, pos)
}

/// 官方各种组合类的 `place`：选子项（或依次全部）并递归。
///
/// Official composition `place`: pick (or iterate) children and recurse.
fn place_composition<'a>(
    comp: &Composition<'a>,
    registry: &'a Registry,
    world: &mut FeatureWorld<'_>,
    env: &mut PlacementEnv<'_>,
    random: &WorldgenRandom,
    pos: Pos,
) -> PlaceOutcome {
    match comp {
        Composition::RandomSelector { features, default } => {
            let mut selected = default;
            for (chance, child) in features {
                if random.next_float() < *chance {
                    selected = child;
                    break;
                }
            }
            place_child(selected, registry, world, env, random, pos)
        }
        Composition::WeightedRandomSelector { features } => {
            let total: i32 = features.iter().map(|(weight, _)| *weight).sum();
            if total == 0 {
                // `WeightedList.getRandom` 在 selector 为空时返回 empty，不消费随机数。
                // `WeightedList.getRandom` returns empty and consumes no random when total==0.
                return PlaceOutcome {
                    supported: true,
                    placed: false,
                };
            }
            let mut selection = random.next_int(total);
            for (weight, child) in features {
                selection -= *weight;
                if selection < 0 {
                    return place_child(child, registry, world, env, random, pos);
                }
            }
            PlaceOutcome {
                supported: true,
                placed: false,
            }
        }
        Composition::SimpleRandomSelector { features } => {
            if features.is_empty() {
                return PlaceOutcome::unsupported();
            }
            let index = random.next_int(features.len() as i32) as usize;
            place_child(&features[index], registry, world, env, random, pos)
        }
        Composition::RandomBooleanSelector {
            feature_true,
            feature_false,
        } => {
            let child = if random.next_boolean() {
                feature_true
            } else {
                feature_false
            };
            place_child(child, registry, world, env, random, pos)
        }
        Composition::Sequence { features } => {
            let mut total = PlaceOutcome {
                supported: true,
                placed: true,
            };
            for child in features {
                let outcome = place_child(child, registry, world, env, random, pos);
                total.supported &= outcome.supported;
                // 官方 `SequenceFeature` 在某个 `placer.place` 返回 false 时立即停止。
                // Official `SequenceFeature` stops when a `placer.place` returns false.
                if !outcome.placed {
                    total.placed = false;
                    break;
                }
            }
            total
        }
    }
}

pub(crate) fn place_child<'a>(
    child: &Child<'a>,
    registry: &'a Registry,
    world: &mut FeatureWorld<'_>,
    env: &mut PlacementEnv<'_>,
    random: &WorldgenRandom,
    pos: Pos,
) -> PlaceOutcome {
    let Some((feature_value, placement)) = child.resolve(registry) else {
        return PlaceOutcome::unsupported();
    };
    // 官方 `PlacedFeature.place` 是 `placeWithBiomeCheck=false`，topFeature 为空。
    // Official `PlacedFeature.place` uses `placeWithBiomeCheck=false` (empty topFeature).
    let mut child_env = PlacementEnv {
        generation: env.generation,
        biome_at: &mut *env.biome_at,
        registry: env.registry,
        top_feature: None,
        region_random: env.region_random,
    };
    place_placed(
        registry,
        feature_value,
        placement,
        world,
        &mut child_env,
        random,
        pos,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_step_nine_composition_feature() {
        let root: Value = serde_json::from_str(mc_bamboo_data::worldgen::WORLDGEN).unwrap();
        let registry = Registry::parse(root.as_object().unwrap()).unwrap();
        let mut count = 0;
        for &id in &registry.steps()[9] {
            let placed = registry.placed(id).unwrap();
            let Some((kind, config)) = resolve_feature(&registry, &placed.feature) else {
                continue;
            };
            let is_composition = matches!(
                kind.as_str(),
                "minecraft:random_selector"
                    | "minecraft:weighted_random_selector"
                    | "minecraft:simple_random_selector"
                    | "minecraft:random_boolean_selector"
                    | "minecraft:sequence"
            );
            if is_composition {
                assert!(
                    Composition::parse(&kind, config).is_some(),
                    "{}",
                    placed.name
                );
                count += 1;
            }
        }
        assert!(count > 0);
    }
}
