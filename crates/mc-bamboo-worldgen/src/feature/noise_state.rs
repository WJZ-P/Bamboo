//! `NoiseProvider` / `DualNoiseProvider` / `NoiseThresholdProvider`：
//! `SimpleBlockFeature.to_place` 的 holder 引用指向 `BLOCK_STATE_PROVIDER` 注册表，
//! 里面这三类用 `NormalNoise`（seed 固定的 `WorldgenRandom(LegacyRandomSource)`）按坐标选方块。
//!
//! The three noise-based `BlockStateProvider`s referenced by flower `to_place` holders.
//! They pick a block from a `NormalNoise` sampled at the placement position; the noise is
//! built from a fixed `WorldgenRandom(LegacyRandomSource(seed))`.

use serde_json::Value;

use crate::{
    noise::{Noise, NormalNoise, NormalNoiseParameters, Normalization},
    random::{Algorithm, WorldgenRandom},
};

use super::state::parse_state;

pub type Pos = (i32, i32, i32);

/// `NormalNoise.DIRECT_CODEC` 的参数 + 固定 seed 造一份噪声。
fn build_noise(seed: i64, value: &Value) -> Option<Noise> {
    let object = value.as_object()?;
    let base_amplitude = object
        .get("base_amplitude")
        .and_then(Value::as_f64)
        .unwrap_or(1.0);
    let base_octave = object.get("base_octave")?.as_i64()? as i32;
    let octave_count = object
        .get("octave_count")
        .and_then(Value::as_i64)
        .unwrap_or(1) as i32;
    let normalize = match object.get("normalize") {
        Some(Value::Bool(false)) => Normalization::Disabled,
        Some(Value::String(name)) if name == "legacy" => Normalization::Legacy,
        _ => Normalization::Enabled,
    };
    let amplitude_modifiers = object
        .get("amplitude_modifiers")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_f64).collect())
        .unwrap_or_default();
    let parameters = NormalNoiseParameters {
        base_amplitude,
        base_octave,
        octave_count,
        normalize,
        amplitude_modifiers,
    };
    // 官方 `NoiseBasedStateProvider`：parameters.create(new WorldgenRandom(new LegacyRandomSource(seed)))。
    let random = Algorithm::Legacy.new_instance(seed);
    Some(NormalNoise::new(parameters).create(&random))
}

fn parse_states(value: Option<&Value>) -> Option<Vec<u32>> {
    value?
        .as_array()?
        .iter()
        .map(parse_state)
        .collect::<Result<Vec<_>, _>>()
        .ok()
}

fn f32_field(object: &serde_json::Map<String, Value>, name: &str) -> Option<f32> {
    object.get(name).and_then(Value::as_f64).map(|v| v as f32)
}

fn i64_field(object: &serde_json::Map<String, Value>, name: &str) -> Option<i64> {
    object.get(name).and_then(Value::as_i64)
}

/// 一个已解析的 noise-based `BlockStateProvider`。
#[derive(Debug, Clone)]
pub enum NoiseStateProvider {
    /// `NoiseProvider`：按主噪声值从 states 选一个。
    Noise {
        noise: Box<Noise>,
        scale: f64,
        states: Vec<u32>,
    },
    /// `DualNoiseProvider`：先用 slow noise 定 variety，再从子列表按主噪声选。
    DualNoise {
        noise: Box<Noise>,
        scale: f64,
        states: Vec<u32>,
        slow_noise: Box<Noise>,
        slow_scale: f64,
        variety_min: f64,
        variety_max: f64,
    },
    /// `NoiseThresholdProvider`：阈值分 low/high，high 还要过 high_chance。
    NoiseThreshold {
        noise: Box<Noise>,
        scale: f64,
        threshold: f32,
        high_chance: f32,
        default_state: u32,
        low_states: Vec<u32>,
        high_states: Vec<u32>,
    },
}

impl NoiseStateProvider {
    pub fn parse(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let kind = object.get("type")?.as_str()?;
        let seed = i64_field(object, "seed")?;
        let scale = f64::from(f32_field(object, "scale")?);
        let noise = build_noise(seed, object.get("noise")?)?;
        match kind {
            "minecraft:noise" => Some(Self::Noise {
                noise: Box::new(noise),
                scale,
                states: parse_states(object.get("states"))?,
            }),
            "minecraft:dual_noise" => {
                let variety = object.get("variety")?.as_array()?;
                let variety_min = variety.first()?.as_i64()? as f64;
                let variety_max = variety.get(1)?.as_i64()? as f64;
                Some(Self::DualNoise {
                    noise: Box::new(noise),
                    scale,
                    states: parse_states(object.get("states"))?,
                    slow_noise: Box::new(build_noise(seed, object.get("slow_noise")?)?),
                    slow_scale: f64::from(f32_field(object, "slow_scale")?),
                    variety_min,
                    variety_max,
                })
            }
            "minecraft:noise_threshold" => Some(Self::NoiseThreshold {
                noise: Box::new(noise),
                scale,
                threshold: f32_field(object, "threshold")?,
                high_chance: f32_field(object, "high_chance")?,
                default_state: parse_state(object.get("default_state")?).ok()?,
                low_states: parse_states(object.get("low_states"))?,
                high_states: parse_states(object.get("high_states"))?,
            }),
            _ => None,
        }
    }

    /// 所有可能放出的状态（存活判定白名单检查用）。
    pub fn possible_states(&self) -> Vec<u32> {
        match self {
            Self::Noise { states, .. } | Self::DualNoise { states, .. } => states.clone(),
            Self::NoiseThreshold {
                default_state,
                low_states,
                high_states,
                ..
            } => {
                let mut all = Vec::with_capacity(1 + low_states.len() + high_states.len());
                all.push(*default_state);
                all.extend_from_slice(low_states);
                all.extend_from_slice(high_states);
                all
            }
        }
    }

    /// 官方 `getState(level, random, pos)`。
    pub fn get_state(&self, random: &WorldgenRandom, pos: Pos) -> Option<u32> {
        let (x, y, z) = pos;
        match self {
            Self::Noise {
                noise,
                scale,
                states,
            } => pick_state(states, noise_value(noise, x, y, z, *scale)),
            Self::DualNoise {
                noise,
                scale,
                states,
                slow_noise,
                slow_scale,
                variety_min,
                variety_max,
            } => {
                let variety = noise_value(slow_noise, x, y, z, *slow_scale) as f64;
                let local_variety =
                    clamped_map(variety, -1.0, 1.0, *variety_min, *variety_max + 1.0) as i32;
                let mut possible = Vec::with_capacity(local_variety.max(0) as usize);
                for i in 0..local_variety {
                    let dx = i.wrapping_mul(54545);
                    let dz = i.wrapping_mul(34234);
                    let value = noise_value(
                        slow_noise,
                        x.wrapping_add(dx),
                        y,
                        z.wrapping_add(dz),
                        *slow_scale,
                    );
                    if let Some(state) = pick_state(states, value) {
                        possible.push(state);
                    }
                }
                pick_state(&possible, noise_value(noise, x, y, z, *scale))
            }
            Self::NoiseThreshold {
                noise,
                scale,
                threshold,
                high_chance,
                default_state,
                low_states,
                high_states,
            } => {
                let local_value = noise_value(noise, x, y, z, *scale) as f64;
                if local_value < f64::from(*threshold) {
                    random_state(low_states, random)
                } else if random.next_float() < *high_chance {
                    random_state(high_states, random)
                } else {
                    Some(*default_state)
                }
            }
        }
    }
}

/// `NoiseBasedStateProvider.getNoiseValue(pos, scale)` = noise.get(x*scale, y*scale, z*scale)。
fn noise_value(noise: &Noise, x: i32, y: i32, z: i32, scale: f64) -> f32 {
    noise.get(
        f64::from(x) * scale,
        f64::from(y) * scale,
        f64::from(z) * scale,
    )
}

/// `NoiseProvider.getRandomState(states, noiseValue)` = `clamp((1+v)/2, 0, 0.9999)` 映射到下标。
fn pick_state(states: &[u32], noise_value: f32) -> Option<u32> {
    let placement = ((1.0_f32 + noise_value) / 2.0_f32).clamp(0.0, 0.9999);
    let index = (placement * states.len() as f32) as usize;
    states.get(index).copied()
}

/// `Util.getRandom(list, random)` = list[nextInt(size)]。
fn random_state(states: &[u32], random: &WorldgenRandom) -> Option<u32> {
    if states.is_empty() {
        return None;
    }
    states
        .get(random.next_int(states.len() as i32) as usize)
        .copied()
}

/// `Mth.clampedMap(double, ...)` = clampedLerp(inverseLerp(value, fromMin, fromMax), toMin, toMax)。
fn clamped_map(value: f64, from_min: f64, from_max: f64, to_min: f64, to_max: f64) -> f64 {
    let t = ((value - from_min) / (from_max - from_min)).clamp(0.0, 1.0);
    to_min + t * (to_max - to_min)
}
