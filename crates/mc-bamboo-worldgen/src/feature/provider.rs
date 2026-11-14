//! `IntProvider` / `HeightProvider` / `VerticalAnchor` 的解析与求值。
//!
//! step 6 只用到 constant / uniform 两种 IntProvider 和 uniform / trapezoid 两种
//! HeightProvider；其它类型明确报错，避免静默算错。

use serde_json::Value;

use crate::random::WorldgenRandom;

/// `WorldGenerationContext` 的三个数，主世界是 `(-64, 384, 63)`。
#[derive(Debug, Clone, Copy)]
pub struct WorldGenerationContext {
    pub min_y: i32,
    pub height: i32,
    pub sea_level: i32,
}

impl WorldGenerationContext {
    fn min_gen_y(self) -> i32 {
        self.min_y
    }

    fn gen_depth(self) -> i32 {
        self.height
    }
}

/// `VerticalAnchor`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerticalAnchor {
    Absolute(i32),
    AboveBottom(i32),
    BelowTop(i32),
}

impl VerticalAnchor {
    pub fn parse(value: &Value) -> Result<Self, String> {
        let object = value
            .as_object()
            .ok_or("vertical anchor must be an object")?;
        let one = |name: &str| -> Result<Option<i32>, String> {
            object
                .get(name)
                .map(|v| {
                    v.as_i64()
                        .map(|n| n as i32)
                        .ok_or_else(|| format!("bad {name} in vertical anchor"))
                })
                .transpose()
        };
        if let Some(y) = one("absolute")? {
            return Ok(Self::Absolute(y));
        }
        if let Some(offset) = one("above_bottom")? {
            return Ok(Self::AboveBottom(offset));
        }
        if let Some(offset) = one("below_top")? {
            return Ok(Self::BelowTop(offset));
        }
        Err("unknown vertical anchor".to_owned())
    }

    /// `resolveY(WorldGenerationContext)`。
    pub fn resolve(self, context: WorldGenerationContext) -> i32 {
        match self {
            Self::Absolute(y) => y,
            Self::AboveBottom(offset) => context.min_gen_y() + offset,
            Self::BelowTop(offset) => context.gen_depth() - 1 + context.min_gen_y() - offset,
        }
    }
}

/// `IntProvider.sample(random)`。26.3 step 9 的 `count` / `offset` 用到 constant、
/// uniform、trapezoid、biased_to_bottom、very_biased_to_bottom；递归的 clamped /
/// weighted_list / clamped_normal 暂不支持，解析时报错让该 placed feature 标 unsupported。
///
/// `IntProvider.sample(random)`: step 9 uses constant, uniform, trapezoid, and the
/// two biased variants for `count` / `offset`. Recursive clamped / weighted_list /
/// clamped_normal stay unsupported for now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntProvider {
    Constant(i32),
    Uniform {
        min: i32,
        max: i32,
    },
    Trapezoid {
        min: i32,
        max: i32,
        plateau: i32,
    },
    BiasedToBottom {
        min: i32,
        max: i32,
    },
    VeryBiasedToBottom {
        min: i32,
        max: i32,
    },
    /// `ClampedInt`：`clamp(source.sample(random), min, max)`，source 递归。
    ///
    /// `ClampedInt`: `clamp(source.sample(random), min, max)` with a recursive source.
    Clamped {
        source: Box<IntProvider>,
        min: i32,
        max: i32,
    },
    /// `WeightedListInt`：`WeightedList.getRandomOrThrow` 选一项，再 sample 那一项。
    ///
    /// `WeightedListInt`: pick an entry via `WeightedList.getRandomOrThrow`, then sample it.
    WeightedList {
        entries: Vec<(i32, IntProvider)>,
        total: i32,
    },
}

impl IntProvider {
    pub fn parse(value: &Value) -> Result<Self, String> {
        if let Some(number) = value.as_i64() {
            return Ok(Self::Constant(number as i32));
        }
        let object = value
            .as_object()
            .ok_or("int provider must be an object or int")?;
        let kind = object
            .get("type")
            .and_then(Value::as_str)
            .ok_or("int provider has no type")?;
        let integer = |name: &str| -> Result<i32, String> {
            object
                .get(name)
                .and_then(Value::as_i64)
                .map(|n| n as i32)
                .ok_or_else(|| format!("int provider {kind} has no {name}"))
        };
        Ok(match kind {
            "minecraft:constant" => Self::Constant(integer("value")?),
            "minecraft:uniform" => Self::Uniform {
                min: integer("min_inclusive")?,
                max: integer("max_inclusive")?,
            },
            // TrapezoidInt 的字段名是 min / max / plateau（不是 min_inclusive）。
            // TrapezoidInt uses min / max / plateau (not min_inclusive).
            "minecraft:trapezoid" => Self::Trapezoid {
                min: integer("min")?,
                max: integer("max")?,
                plateau: integer("plateau")?,
            },
            "minecraft:biased_to_bottom" => Self::BiasedToBottom {
                min: integer("min_inclusive")?,
                max: integer("max_inclusive")?,
            },
            "minecraft:very_biased_to_bottom" => Self::VeryBiasedToBottom {
                min: integer("min_inclusive")?,
                max: integer("max_inclusive")?,
            },
            // `ClampedInt`：先 sample source（消费随机数），再 clamp。
            // `ClampedInt`: sample the source (consuming random), then clamp.
            "minecraft:clamped" => Self::Clamped {
                source: Box::new(IntProvider::parse(
                    object
                        .get("source")
                        .ok_or("clamped int provider has no source")?,
                )?),
                min: integer("min_inclusive")?,
                max: integer("max_inclusive")?,
            },
            // `WeightedListInt`：`distribution` 是 `{data: IntProvider, weight}` 数组，
            // `data` 可以是裸整数（Constant）或嵌套的 IntProvider。
            // `WeightedListInt`: `distribution` is a list of `{data, weight}`;
            // `data` may be a bare int (Constant) or a nested IntProvider.
            "minecraft:weighted_list" => {
                let distribution = object
                    .get("distribution")
                    .and_then(Value::as_array)
                    .ok_or("weighted_list has no distribution")?;
                let mut entries = Vec::with_capacity(distribution.len());
                let mut total: i32 = 0;
                for item in distribution {
                    let weight = item
                        .get("weight")
                        .and_then(Value::as_i64)
                        .ok_or("weighted_list entry has no weight")?
                        as i32;
                    let provider = IntProvider::parse(
                        item.get("data").ok_or("weighted_list entry has no data")?,
                    )?;
                    total += weight;
                    entries.push((weight, provider));
                }
                Self::WeightedList { entries, total }
            }
            _ => return Err(format!("unsupported int provider {kind}")),
        })
    }

    pub fn sample(&self, random: &WorldgenRandom) -> i32 {
        match self {
            Self::Constant(value) => *value,
            Self::Uniform { min, max } => random.next_int_between_inclusive(*min, *max),
            // TrapezoidInt.sample：对称且 plateau==0 时走官方的快路径，两次 nextInt。
            // TrapezoidInt.sample: official symmetric fast path, two nextInt calls.
            Self::Trapezoid { min, max, plateau } => {
                if *plateau == 0 && *max == -*min {
                    return random.next_int(*max + 1) - random.next_int(*max + 1);
                }
                let range = *max - *min;
                if *plateau == range {
                    return random.next_int_between_inclusive(*min, *max);
                }
                let plateau_start = (range - *plateau) / 2;
                let plateau_end = range - plateau_start;
                *min + random.next_int_between_inclusive(0, plateau_end)
                    + random.next_int_between_inclusive(0, plateau_start)
            }
            // BiasedToBottomInt：min + nextInt(nextInt(max-min+1)+1)。
            // BiasedToBottomInt: min + nextInt(nextInt(max-min+1)+1).
            Self::BiasedToBottom { min, max } => {
                *min + random.next_int(random.next_int(*max - *min + 1) + 1)
            }
            // VeryBiasedToBottomInt：再套一层 nextInt。
            // VeryBiasedToBottomInt: one more nested nextInt.
            Self::VeryBiasedToBottom { min, max } => {
                *min + random.next_int(random.next_int(random.next_int(*max - *min + 1) + 1) + 1)
            }
            // ClampedInt.sample：先 sample source（消费随机数），再 clamp。
            // ClampedInt.sample: sample the source (consuming random), then clamp.
            Self::Clamped { source, min, max } => source.sample(random).clamp(*min, *max),
            // `WeightedList.getRandomOrThrow`：nextInt(totalWeight) 先消费一次，
            // 再按累计权重选中的那一项 sample（可能再消费随机数）。Flat / Compact
            // 两种 selector 的累计权重结果一致，用循环复现即可。
            // `WeightedList.getRandomOrThrow`: consume nextInt(totalWeight) first,
            // then sample the selected entry (which may consume more). Flat and
            // Compact selectors agree on the cumulative-weight mapping.
            Self::WeightedList { entries, total } => {
                if *total == 0 {
                    return 0;
                }
                let mut selection = random.next_int(*total);
                for (weight, provider) in entries {
                    selection -= *weight;
                    if selection < 0 {
                        return provider.sample(random);
                    }
                }
                0
            }
        }
    }
}

/// `HeightProvider.sample(random, WorldGenerationContext)`。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HeightProvider {
    Uniform {
        min: VerticalAnchor,
        max: VerticalAnchor,
    },
    Trapezoid {
        min: VerticalAnchor,
        max: VerticalAnchor,
        plateau: i32,
    },
}

impl HeightProvider {
    pub fn parse(value: &Value) -> Result<Self, String> {
        let object = value
            .as_object()
            .ok_or("height provider must be an object")?;
        let kind = object
            .get("type")
            .and_then(Value::as_str)
            .ok_or("height provider has no type")?;
        let anchor = |name: &str| -> Result<VerticalAnchor, String> {
            VerticalAnchor::parse(
                object
                    .get(name)
                    .ok_or_else(|| format!("height provider {kind} has no {name}"))?,
            )
        };
        match kind {
            "minecraft:uniform" => Ok(Self::Uniform {
                min: anchor("min_inclusive")?,
                max: anchor("max_inclusive")?,
            }),
            "minecraft:trapezoid" => {
                let plateau = object
                    .get("plateau")
                    .and_then(Value::as_i64)
                    .map(|n| n as i32)
                    .unwrap_or(0);
                Ok(Self::Trapezoid {
                    min: anchor("min_inclusive")?,
                    max: anchor("max_inclusive")?,
                    plateau,
                })
            }
            "minecraft:constant" => {
                let value = VerticalAnchor::parse(
                    object
                        .get("value")
                        .ok_or("constant height provider has no value")?,
                )?;
                Ok(Self::Uniform {
                    min: value,
                    max: value,
                })
            }
            _ => Err(format!("unsupported height provider {kind}")),
        }
    }

    pub fn sample(self, random: &WorldgenRandom, context: WorldGenerationContext) -> i32 {
        match self {
            Self::Uniform { min, max } => {
                let low = min.resolve(context);
                let high = max.resolve(context);
                if low > high {
                    // 官方打一行 warn 然后返回 min（UniformHeight.sample 23-62）。
                    low
                } else {
                    random.next_int_between_inclusive(low, high)
                }
            }
            Self::Trapezoid { min, max, plateau } => {
                let low = min.resolve(context);
                let high = max.resolve(context);
                if low > high {
                    return low;
                }
                let range = high - low;
                if plateau >= range {
                    return random.next_int_between_inclusive(low, high);
                }
                let slope = (range - plateau) / 2;
                low + random.next_int_between_inclusive(0, range - slope)
                    + random.next_int_between_inclusive(0, slope)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::random::Algorithm;

    fn random() -> WorldgenRandom {
        WorldgenRandom::new(Algorithm::Xoroshiro.new_instance(42))
    }

    #[test]
    fn anchors_resolve_against_the_dimension() {
        let context = WorldGenerationContext {
            min_y: -64,
            height: 384,
            sea_level: 63,
        };
        assert_eq!(VerticalAnchor::Absolute(136).resolve(context), 136);
        assert_eq!(VerticalAnchor::AboveBottom(80).resolve(context), 16);
        assert_eq!(VerticalAnchor::BelowTop(0).resolve(context), 319);
        assert_eq!(VerticalAnchor::BelowTop(-1).resolve(context), 320);
    }

    #[test]
    fn providers_parse_the_exported_shapes() {
        let a: Value = serde_json::from_str(r#"{"absolute":136}"#).unwrap();
        assert_eq!(
            VerticalAnchor::parse(&a).unwrap(),
            VerticalAnchor::Absolute(136)
        );
        let p: Value = serde_json::from_str(r#"30"#).unwrap();
        assert_eq!(IntProvider::parse(&p).unwrap(), IntProvider::Constant(30));
        let p: Value = serde_json::from_str(
            r#"{"min_inclusive":44,"max_inclusive":52,"type":"minecraft:uniform"}"#,
        )
        .unwrap();
        assert_eq!(
            IntProvider::parse(&p).unwrap(),
            IntProvider::Uniform { min: 44, max: 52 }
        );
        let h: Value = serde_json::from_str(
            r#"{"min_inclusive":{"absolute":0},"max_inclusive":{"below_top":0},"type":"minecraft:uniform"}"#,
        )
        .unwrap();
        assert!(matches!(
            HeightProvider::parse(&h).unwrap(),
            HeightProvider::Uniform { .. }
        ));
    }

    #[test]
    fn uniform_height_stays_in_range() {
        let context = WorldGenerationContext {
            min_y: -64,
            height: 384,
            sea_level: 63,
        };
        let random = random();
        let provider = HeightProvider::Uniform {
            min: VerticalAnchor::AboveBottom(0),
            max: VerticalAnchor::BelowTop(0),
        };
        for _ in 0..1000 {
            let y = provider.sample(&random, context);
            assert!((-64..=319).contains(&y), "y={y}");
        }
    }

    #[test]
    fn trapezoid_symmetric_fast_path_matches_two_next_int() {
        // `offset` 的 x/z 是 trapezoid(-7..7, plateau=0)，官方走对称快路径：
        // nextInt(8) - nextInt(8)。用两个同种子随机源验证取数序列完全一致。
        // The symmetric fast path must consume and produce exactly
        // nextInt(8) - nextInt(8) against a fresh random source.
        let provider = IntProvider::Trapezoid {
            min: -7,
            max: 7,
            plateau: 0,
        };
        let a = random();
        let b = random();
        for _ in 0..1000 {
            let expected = b.next_int(8) - b.next_int(8);
            assert_eq!(provider.sample(&a), expected);
        }
    }

    #[test]
    fn trapezoid_general_formula_matches_official() {
        // 非对称：min=2, max=11, plateau=2 → range=9, start=3, end=6。
        // min + nextIntBetweenInclusive(0,6) + nextIntBetweenInclusive(0,3)。
        // Non-symmetric path: min + nextIntBetweenInclusive(0,6) + (0,3).
        let provider = IntProvider::Trapezoid {
            min: 2,
            max: 11,
            plateau: 2,
        };
        let a = random();
        let b = random();
        for _ in 0..1000 {
            let expected =
                2 + b.next_int_between_inclusive(0, 6) + b.next_int_between_inclusive(0, 3);
            assert_eq!(provider.sample(&a), expected);
        }
    }

    #[test]
    fn biased_variants_match_official_nesting() {
        let biased = IntProvider::BiasedToBottom { min: 5, max: 9 };
        let very = IntProvider::VeryBiasedToBottom { min: 5, max: 9 };
        let a = random();
        let b = random();
        let c = random();
        let d = random();
        for _ in 0..1000 {
            let span = 9 - 5 + 1;
            assert_eq!(biased.sample(&a), 5 + b.next_int(b.next_int(span) + 1));
            assert_eq!(
                very.sample(&c),
                5 + d.next_int(d.next_int(d.next_int(span) + 1) + 1)
            );
        }
    }

    #[test]
    fn clamped_provider_matches_official() {
        // `forest_flowers` 的 count：clamp(uniform(-3,1), 0, 1)。
        // `forest_flowers` count: clamp(uniform(-3,1), 0, 1).
        let value: Value = serde_json::from_str(
            r#"{"min_inclusive":0,"max_inclusive":1,"source":{"min_inclusive":-3,"max_inclusive":1,"type":"minecraft:uniform"},"type":"minecraft:clamped"}"#,
        )
        .unwrap();
        let provider = IntProvider::parse(&value).unwrap();
        assert_eq!(
            provider,
            IntProvider::Clamped {
                source: Box::new(IntProvider::Uniform { min: -3, max: 1 }),
                min: 0,
                max: 1,
            }
        );
        let a = random();
        let b = random();
        for _ in 0..1000 {
            let expected = b.next_int_between_inclusive(-3, 1).clamp(0, 1);
            assert_eq!(provider.sample(&a), expected);
        }
    }

    #[test]
    fn weighted_list_selects_then_samples_like_official() {
        // `cactus` 第二层花：distribution [{data:0,w:3},{data:1,w:1}]，total=4。
        // getRandomOrThrow 先 nextInt(4)，选中项再 sample（这里都是 Constant，不取数）。
        // `cactus` flower layer: distribution [{data:0,w:3},{data:1,w:1}], total=4.
        // getRandomOrThrow consumes nextInt(4), then samples the selected Constant.
        let value: Value = serde_json::from_str(
            r#"{"distribution":[{"data":0,"weight":3},{"data":1,"weight":1}],"type":"minecraft:weighted_list"}"#,
        )
        .unwrap();
        let provider = IntProvider::parse(&value).unwrap();
        let a = random();
        let b = random();
        for _ in 0..1000 {
            let selection = b.next_int(4);
            let expected = if selection < 3 { 0 } else { 1 };
            assert_eq!(provider.sample(&a), expected);
        }
    }

    #[test]
    fn weighted_list_with_nested_provider_consumes_both_draws() {
        // `cave_vine` 高度：distribution 里是 uniform(0,2)，weight 单项为 1。
        // nextInt(1) 仍消费一次，再 sample uniform。两个同种子源应完全一致。
        // `cave_vine` height: distribution holds uniform(0,2), single weight 1.
        // nextInt(1) still consumes one draw, then the uniform samples.
        let value: Value = serde_json::from_str(
            r#"{"distribution":[{"data":{"min_inclusive":0,"max_inclusive":2,"type":"minecraft:uniform"},"weight":1}],"type":"minecraft:weighted_list"}"#,
        )
        .unwrap();
        let provider = IntProvider::parse(&value).unwrap();
        let a = random();
        let b = random();
        for _ in 0..1000 {
            let _ = b.next_int(1);
            let expected = b.next_int_between_inclusive(0, 2);
            assert_eq!(provider.sample(&a), expected);
        }
    }
}
