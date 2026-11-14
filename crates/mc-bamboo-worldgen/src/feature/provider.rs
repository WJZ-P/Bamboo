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
#[derive(Debug, Clone, PartialEq)]
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
    /// `ClampedNormalInt`：`clamp(mean + (float)nextGaussian()*deviation, min, max)` 后截断。
    ClampedNormal {
        mean: f32,
        deviation: f32,
        min: i32,
        max: i32,
    },
    /// `WeightedListInt`：`distribution.getRandomOrThrow(random).sample(random)`。
    /// 每个 `data` 是递归的 `IntProvider`（导出里是裸数字，即 `ConstantInt`）。
    ///
    /// `WeightedListInt`: `distribution.getRandomOrThrow(random).sample(random)`.
    /// Each `data` is a recursive `IntProvider` (bare numbers in the export are
    /// `ConstantInt`).
    WeightedList {
        entries: Vec<(i32, Box<IntProvider>)>,
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
        let float = |name: &str| -> Result<f32, String> {
            object
                .get(name)
                .and_then(Value::as_f64)
                .map(|n| n as f32)
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
            // `ClampedNormalInt`：mean / deviation 是 f32（官方 Codec.FLOAT）。
            "minecraft:clamped_normal" => Self::ClampedNormal {
                mean: float("mean")?,
                deviation: float("deviation")?,
                min: integer("min_inclusive")?,
                max: integer("max_inclusive")?,
            },
            // `WeightedListInt`：distribution 是 WeightedList<IntProvider>。
            // `WeightedListInt`: distribution is a WeightedList<IntProvider>.
            "minecraft:weighted_list" => {
                let entries = object
                    .get("distribution")
                    .ok_or("weighted list has no distribution")?
                    .as_array()
                    .ok_or("weighted list distribution is not an array")?
                    .iter()
                    .map(|entry| {
                        let weight = entry
                            .get("weight")
                            .and_then(Value::as_i64)
                            .ok_or("weighted list entry has no weight")?
                            as i32;
                        let data = entry.get("data").ok_or("weighted list entry has no data")?;
                        Ok((weight, Box::new(IntProvider::parse(data)?)))
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                let total = entries.iter().map(|(weight, _)| *weight).sum();
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
            // ClampedNormalInt.sample = (int)clamp(Mth.normal(random, mean, deviation), min, max)。
            // Mth.normal = mean + (float)nextGaussian() * deviation（均为 f32 运算）。
            Self::ClampedNormal {
                mean,
                deviation,
                min,
                max,
            } => {
                let normal = *mean + (random.next_gaussian() as f32) * *deviation;
                normal.clamp(*min as f32, *max as f32) as i32
            }
            // WeightedList.getRandomOrThrow：nextInt(totalWeight) + 累计权重轮盘，
            // 选中的 IntProvider 再 sample（ConstantInt 不额外取数）。
            // WeightedList.getRandomOrThrow: nextInt(totalWeight) + cumulative wheel,
            // then sample the chosen IntProvider (ConstantInt takes no extra random).
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
    fn clamped_normal_matches_official_mth_normal() {
        // 官方 `ClampedNormalInt.sample` = (int)clamp(Mth.normal(random, mean, dev), min, max)。
        // `Mth.normal` = mean + (float)nextGaussian() * deviation。
        let provider = IntProvider::ClampedNormal {
            mean: 5.0,
            deviation: 2.0,
            min: 0,
            max: 10,
        };
        let a = random();
        let b = random();
        for _ in 0..1000 {
            let expected = {
                let normal = 5.0_f32 + (b.next_gaussian() as f32) * 2.0_f32;
                normal.clamp(0.0, 10.0) as i32
            };
            assert_eq!(provider.sample(&a), expected);
        }
    }

    #[test]
    fn weighted_list_matches_official_get_random_or_throw() {
        // `trees_plains` 的 count：weighted_list[{data:0,w:19},{data:1,w:1}]。
        // 官方：nextInt(totalWeight) 选条目，再 sample 选中的 IntProvider。
        let value: Value = serde_json::from_str(
            r#"{"distribution":[{"data":0,"weight":19},{"data":{"type":"minecraft:uniform","min_inclusive":1,"max_inclusive":3},"weight":1}],"type":"minecraft:weighted_list"}"#,
        )
        .unwrap();
        let provider = IntProvider::parse(&value).unwrap();
        let a = random();
        let b = random();
        for _ in 0..1000 {
            let selection = b.next_int(20);
            let expected = if selection < 19 {
                0
            } else {
                b.next_int_between_inclusive(1, 3)
            };
            assert_eq!(provider.sample(&a), expected);
        }
    }
}
