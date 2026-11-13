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

/// `IntProvider.sample(random)`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntProvider {
    Constant(i32),
    Uniform { min: i32, max: i32 },
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
        match kind {
            "minecraft:constant" => Ok(Self::Constant(integer("value")?)),
            "minecraft:uniform" => Ok(Self::Uniform {
                min: integer("min_inclusive")?,
                max: integer("max_inclusive")?,
            }),
            _ => Err(format!("unsupported int provider {kind}")),
        }
    }

    pub fn sample(self, random: &WorldgenRandom) -> i32 {
        match self {
            Self::Constant(value) => value,
            Self::Uniform { min, max } => random.next_int_between_inclusive(min, max),
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
}
