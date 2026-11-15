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
    /// `OverlayFeature`：把所有子项都放一遍，任一成功就算成功（**不短路**）。
    ///
    /// `OverlayFeature`: place every child, succeed if any did (no short-circuit).
    Overlay {
        features: Vec<Child<'a>>,
    },
    /// `CoralTreeFeature`：竖直长主干，再从顶部水平分枝。
    ///
    /// `CoralTreeFeature`: a vertical trunk, then horizontal branches from the top.
    CoralTree {
        feature: Child<'a>,
    },
    /// `CoralClawFeature`：一个主爪方向加两段侧枝。
    ///
    /// `CoralClawFeature`: one claw direction plus two side branches.
    CoralClaw {
        feature: Child<'a>,
    },
}

impl<'a> Composition<'a> {
    /// 依次访问所有子项（诊断和测试用）。
    ///
    /// Visit every child (for diagnostics and tests).
    #[cfg(test)]
    fn for_each_child(&self, mut visit: impl FnMut(&Child<'a>)) {
        match self {
            Self::RandomSelector { features, default } => {
                for (_, child) in features {
                    visit(child);
                }
                visit(default);
            }
            Self::WeightedRandomSelector { features } => {
                for (_, child) in features {
                    visit(child);
                }
            }
            Self::SimpleRandomSelector { features }
            | Self::Sequence { features }
            | Self::Overlay { features } => {
                for child in features {
                    visit(child);
                }
            }
            Self::RandomBooleanSelector {
                feature_true,
                feature_false,
            } => {
                visit(feature_true);
                visit(feature_false);
            }
            Self::CoralTree { feature } | Self::CoralClaw { feature } => visit(feature),
        }
    }

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
            "minecraft:overlay" => Some(Self::Overlay {
                features: parse_holder_set(object.get("features")?)?,
            }),
            "minecraft:coral_tree" => Some(Self::CoralTree {
                feature: Child::parse(object.get("feature")?)?,
            }),
            "minecraft:coral_claw" => Some(Self::CoralClaw {
                feature: Child::parse(object.get("feature")?)?,
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
        let placed = place_leaf(&leaf, world, random, pos);
        return PlaceOutcome {
            supported: true,
            placed,
        };
    }
    // `vegetation_patch` / `waterlogged_vegetation_patch` 需要注册表和 `PlacementEnv`
    // 才能递归放子地物，不能当叶子。
    //
    // `moss_patch`（floor）、`moss_patch_ceiling`（ceiling，还有 10 处未解决的
    // `cave_vines` 错位）、`pale_moss_patch` 都跑过逐方块对拍；ceiling 的差异根因是
    // `JavaPosSet` 没实现 `HashMap` 树化，属已知限制。
    // `waterlogged_vegetation_patch`（`clay_pool_with_dripleaves`）的 `waterlogged` 分支
    // 由 `VegetationPatch::parse` 按 kind 打开，这里一起放行。
    //
    // `vegetation_patch` / `waterlogged_vegetation_patch` need the registry and a
    // `PlacementEnv` to recurse into the child feature, so they cannot be leaves.
    // `VegetationPatch::parse` turns on the waterlogged branch by kind, so both kinds
    // are dispatched here.
    if kind == "minecraft:vegetation_patch" || kind == "minecraft:waterlogged_vegetation_patch" {
        let Some(patch) = super::vegetation_patch::VegetationPatch::parse(kind, config) else {
            return PlaceOutcome::unsupported();
        };
        return patch.place(registry, world, env, random, pos);
    }
    // `root_system` 要先递归放一株树，同样需要注册表和 `PlacementEnv`。
    //
    // `root_system` recurses into a tree feature, so it also needs the registry and a
    // `PlacementEnv` and cannot be a leaf.
    if kind == "minecraft:root_system" {
        let lookup = |name: &str| registry.state_provider(name);
        let Some(feature) = super::root_system::RootSystemFeature::parse(config, &lookup) else {
            return PlaceOutcome::unsupported();
        };
        return feature.place(registry, world, env, random, pos);
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
        // 官方 `CoralTreeFeature.place`：主干 `nextInt(3)+1` 高，每格放一次子项，放不下
        // 就直接返回 true；然后从顶部朝 `nextInt(3)+2` 个水平方向分枝，每枝
        // `nextInt(5)+2` 段，`j == 0 || (segmentLength >= 2 && nextFloat() < 0.25)` 时
        // 再往外挪一格。
        // Official `CoralTreeFeature.place`: a trunk of `nextInt(3)+1`, one child placement
        // per block (returning true as soon as one fails), then `nextInt(3)+2` horizontal
        // branches from the top, each up to `nextInt(5)+2` segments.
        Composition::CoralTree { feature } => {
            let mut pos = pos;
            let trunk_height = random.next_int(3) + 1;
            for _ in 0..trunk_height {
                let outcome = place_child(feature, registry, world, env, random, pos);
                if !outcome.supported {
                    return outcome;
                }
                if !outcome.placed {
                    return PlaceOutcome {
                        supported: true,
                        placed: true,
                    };
                }
                pos = (pos.0, pos.1 + 1, pos.2);
            }
            let trunk_top = pos;
            let branches = random.next_int(3) + 2;
            let directions = shuffle_horizontal(random);
            let mut total = PlaceOutcome {
                supported: true,
                placed: false,
            };
            for branch in directions.into_iter().take(branches as usize) {
                let mut pos = step(trunk_top, branch);
                let branch_height = random.next_int(5) + 2;
                let mut segment_length = 0;
                for j in 0..branch_height {
                    let outcome = place_child(feature, registry, world, env, random, pos);
                    total.supported &= outcome.supported;
                    if !outcome.placed {
                        break;
                    }
                    total.placed = true;
                    segment_length += 1;
                    pos = (pos.0, pos.1 + 1, pos.2);
                    if j == 0 || (segment_length >= 2 && random.next_float() < 0.25) {
                        pos = step(pos, branch);
                        segment_length = 0;
                    }
                }
            }
            total
        }
        // 官方 `CoralClawFeature.place`：先随机一个爪方向，再在
        // [爪方向, 顺时针, 逆时针] 里洗牌取 `nextInt(2)+2` 条枝。
        // Official `CoralClawFeature.place`: pick a claw direction, shuffle
        // [it, clockwise, counter-clockwise] and take `nextInt(2)+2` branches.
        Composition::CoralClaw { feature } => {
            let claw = horizontal_direction(random);
            let branches = random.next_int(2) + 2;
            let candidates = [claw, clockwise(claw), counter_clockwise(claw)];
            let mut directions = candidates;
            shuffle_slice(&mut directions, random);
            let mut total = PlaceOutcome {
                supported: true,
                placed: false,
            };
            for branch in directions.into_iter().take(branches as usize) {
                let mut pos = step(pos, branch);
                let sideway_length = random.next_int(2) + 1;
                let (segment_direction, inway_length) = if branch == claw {
                    (claw, random.next_int(3) + 2)
                } else {
                    pos = (pos.0, pos.1 + 1, pos.2);
                    let pick = if random.next_int(2) == 0 {
                        branch
                    } else {
                        "up"
                    };
                    (pick, random.next_int(3) + 3)
                };
                for _ in 0..sideway_length {
                    let outcome = place_child(feature, registry, world, env, random, pos);
                    total.supported &= outcome.supported;
                    if !outcome.placed {
                        break;
                    }
                    total.placed = true;
                    pos = step(pos, segment_direction);
                }
                pos = step(pos, opposite(segment_direction));
                pos = (pos.0, pos.1 + 1, pos.2);
                for _ in 0..inway_length {
                    pos = step(pos, claw);
                    let outcome = place_child(feature, registry, world, env, random, pos);
                    total.supported &= outcome.supported;
                    if !outcome.placed {
                        break;
                    }
                    total.placed = true;
                    if random.next_float() < 0.25 {
                        pos = (pos.0, pos.1 + 1, pos.2);
                    }
                }
            }
            total
        }
        // `OverlayFeature.place`：`placedAny |= placer.place(...)`，**循环到底**，
        // 不因为某个子项失败就停。
        // Official `OverlayFeature.place`: `placedAny |= placer.place(...)` over every
        // child; it never stops early.
        Composition::Overlay { features } => {
            let mut total = PlaceOutcome {
                supported: true,
                placed: false,
            };
            for child in features {
                let outcome = place_child(child, registry, world, env, random, pos);
                total.supported &= outcome.supported;
                total.placed |= outcome.placed;
            }
            total
        }
    }
}

/// `Direction.Plane.HORIZONTAL.shuffledCopy(random)` 的方向集（NORTH, EAST, SOUTH, WEST）。
const HORIZONTAL: [&str; 4] = ["north", "east", "south", "west"];

/// 洗牌后的水平方向。
fn shuffle_horizontal(random: &WorldgenRandom) -> [&'static str; 4] {
    let mut directions = HORIZONTAL;
    shuffle_slice(&mut directions, random);
    directions
}

/// `Direction.Plane.HORIZONTAL.getRandomDirection(random)`。
fn horizontal_direction(random: &WorldgenRandom) -> &'static str {
    HORIZONTAL[random.next_int(4) as usize]
}

/// `Util.shuffle`：从尾部往前，每步 `nextInt(i)`。
fn shuffle_slice<T>(items: &mut [T], random: &WorldgenRandom) {
    for i in (2..=items.len()).rev() {
        let swap_to = random.next_int(i as i32) as usize;
        items.swap(i - 1, swap_to);
    }
}

/// `Direction.getClockWise()`（水平面上）。
fn clockwise(direction: &'static str) -> &'static str {
    match direction {
        "north" => "east",
        "east" => "south",
        "south" => "west",
        _ => "north",
    }
}

/// `Direction.getCounterClockWise()`（水平面上）。
fn counter_clockwise(direction: &'static str) -> &'static str {
    match direction {
        "north" => "west",
        "west" => "south",
        "south" => "east",
        _ => "north",
    }
}

/// `Direction.getOpposite()`。
fn opposite(direction: &'static str) -> &'static str {
    match direction {
        "up" => "down",
        "down" => "up",
        "north" => "south",
        "south" => "north",
        "west" => "east",
        _ => "west",
    }
}

/// 沿方向走一格。
fn step(pos: Pos, direction: &str) -> Pos {
    match direction {
        "up" => (pos.0, pos.1 + 1, pos.2),
        "down" => (pos.0, pos.1 - 1, pos.2),
        "north" => (pos.0, pos.1, pos.2 - 1),
        "south" => (pos.0, pos.1, pos.2 + 1),
        "west" => (pos.0 - 1, pos.1, pos.2),
        _ => (pos.0 + 1, pos.1, pos.2),
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

    /// 诊断：逐层打印 `warm_ocean_vegetation` 子树里每个节点的解析结果。
    ///
    /// Diagnostic: print how every node under `warm_ocean_vegetation` parses.
    #[test]
    fn audits_the_coral_subtree() {
        fn walk(registry: &Registry, child: &Child<'_>, depth: usize, path: &str) {
            let pad = "  ".repeat(depth);
            let Some((feature, placement)) = child.resolve(registry) else {
                println!("{pad}{path}: RESOLVE FAIL");
                return;
            };
            let Some((kind, config)) = crate::feature::run::resolve_feature(registry, feature)
            else {
                println!("{pad}{path}: KIND FAIL");
                return;
            };
            for entry in placement {
                match Modifier::parse(entry) {
                    Ok(m) if !m.is_supported() => println!("{pad}{path}: BAD modifier {entry}"),
                    Err(error) => println!("{pad}{path}: modifier parse error {error}"),
                    _ => {}
                }
            }
            if parse_leaf(&kind, config, &|_| None).is_some() {
                println!("{pad}{path}: leaf {kind}");
                return;
            }
            let Some(comp) = Composition::parse(&kind, config) else {
                let text = config.to_string();
                println!(
                    "{pad}{path}: UNSUPPORTED {kind} config={}",
                    &text[..160.min(text.len())]
                );
                return;
            };
            println!("{pad}{path}: comp {kind}");
            let mut index = 0;
            comp.for_each_child(|child| {
                walk(registry, child, depth + 1, &format!("{path}.{index}"));
                index += 1;
            });
        }

        let root: Value = serde_json::from_str(mc_bamboo_data::worldgen::WORLDGEN).unwrap();
        let registry = Registry::parse(root.as_object().unwrap()).unwrap();
        let placed = registry
            .placed_id("minecraft:warm_ocean_vegetation")
            .unwrap();
        let def = registry.placed(placed).unwrap();
        // 顶层：先看 placement 链，再把 feature 当成一个内联 child。
        for entry in &def.placement {
            match Modifier::parse(entry) {
                Ok(m) if !m.is_supported() => println!("root: BAD modifier {entry}"),
                Err(error) => println!("root: modifier parse error {error}"),
                _ => {}
            }
        }
        let Some((kind, config)) = crate::feature::run::resolve_feature(&registry, &def.feature)
        else {
            panic!("root kind");
        };
        println!("root: comp {kind}");
        let comp = Composition::parse(&kind, config).expect("root composition");
        let mut index = 0;
        comp.for_each_child(|child| {
            walk(&registry, child, 1, &format!("root.{index}"));
            index += 1;
        });

        for name in [
            "minecraft:tube_coral_block",
            "minecraft:sea_pickle",
            "minecraft:tube_coral",
            "minecraft:tube_coral_fan",
        ] {
            let state = mc_bamboo_data::block_by_name(name).unwrap().default_state;
            println!(
                "survival {name} -> {}",
                super::super::survival::supports(state)
            );
        }
    }

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
                    | "minecraft:overlay"
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
