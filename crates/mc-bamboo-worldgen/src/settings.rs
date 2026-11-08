//! 官方 `NoiseGeneratorSettings`、`NoiseSettings`、`NoiseRouter` 和 `RandomState`。
//!
//! 一个维度的设置是数据（由生成器从官方导出），加上一个种子才变成能求值的采样器。

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::{
    density::{FunctionRegistry, NoiseRegistry, Ref, parse},
    sampler::{Compiler, Context, Path, Sampler},
};

/// `NoiseRouter` 的八个输出。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouterOutput {
    Temperature,
    Vegetation,
    Continents,
    Erosion,
    Depth,
    Ridges,
    ChunkSurfaceLevel,
    FinalDensity,
}

impl RouterOutput {
    /// 官方 codec 里的字段顺序。
    pub const ALL: [Self; 8] = [
        Self::Temperature,
        Self::Vegetation,
        Self::Continents,
        Self::Erosion,
        Self::Depth,
        Self::Ridges,
        Self::ChunkSurfaceLevel,
        Self::FinalDensity,
    ];

    /// JSON 里的字段名。
    pub fn field(self) -> &'static str {
        match self {
            Self::Temperature => "temperature",
            Self::Vegetation => "vegetation",
            Self::Continents => "continents",
            Self::Erosion => "erosion",
            Self::Depth => "depth",
            Self::Ridges => "ridges",
            Self::ChunkSurfaceLevel => "chunk_surface_level",
            Self::FinalDensity => "final_density",
        }
    }
}

/// `NoiseRouter`：八个密度函数。
#[derive(Debug)]
pub struct RouterFunctions {
    pub temperature: Ref,
    pub vegetation: Ref,
    pub continents: Ref,
    pub erosion: Ref,
    pub depth: Ref,
    pub ridges: Ref,
    pub chunk_surface_level: Ref,
    pub final_density: Ref,
}

impl RouterFunctions {
    pub fn get(&self, output: RouterOutput) -> &Ref {
        match output {
            RouterOutput::Temperature => &self.temperature,
            RouterOutput::Vegetation => &self.vegetation,
            RouterOutput::Continents => &self.continents,
            RouterOutput::Erosion => &self.erosion,
            RouterOutput::Depth => &self.depth,
            RouterOutput::Ridges => &self.ridges,
            RouterOutput::ChunkSurfaceLevel => &self.chunk_surface_level,
            RouterOutput::FinalDensity => &self.final_density,
        }
    }
}

/// `NoiseGeneratorSettings` 里世界生成的纯计算部分要用的几项。
///
/// 含水层、表面规则、出生点和调试函数不在这里：那几块还没做。
#[derive(Debug)]
pub struct Settings {
    pub min_y: i32,
    pub height: i32,
    pub sea_level: i32,
    /// `legacy_random_source`：决定 `RandomState` 用旧随机源还是 xoroshiro。
    pub legacy_random_source: bool,
    pub router: RouterFunctions,
}

impl Settings {
    /// 按官方 `NoiseGeneratorSettings.DIRECT_CODEC` 里我们要的那几个字段解析。
    pub fn parse(
        value: &Value,
        raw_functions: &serde_json::Map<String, Value>,
        noises: &NoiseRegistry,
        functions: &FunctionRegistry,
    ) -> Result<Self, String> {
        let object = value.as_object().ok_or("expected an object")?;
        let noise = object
            .get("noise")
            .and_then(Value::as_object)
            .ok_or("missing field noise")?;
        let min_y = int_field(noise, "min_y")?;
        let height = int_field(noise, "height")?;
        if min_y % 16 != 0 || height % 16 != 0 {
            return Err("min_y and height have to be multiples of 16".to_owned());
        }
        let sea_level = int_field(object, "sea_level")?;
        let legacy_random_source = object
            .get("legacy_random_source")
            .and_then(Value::as_bool)
            .ok_or("missing field legacy_random_source")?;

        let raw_router = object
            .get("noise_router")
            .and_then(Value::as_object)
            .ok_or("missing field noise_router")?;
        let mut entries: HashMap<String, Ref> = functions
            .ids()
            .map(|id| {
                (
                    id.to_owned(),
                    Ref::clone(functions.get(id).expect("刚问过")),
                )
            })
            .collect();
        let mut resolving = HashSet::new();
        let mut router_entry = |output: RouterOutput| -> Result<Ref, String> {
            let field = output.field();
            let value = raw_router
                .get(field)
                .ok_or_else(|| format!("missing noise router field {field}"))?;
            parse(value, raw_functions, noises, &mut entries, &mut resolving)
                .map_err(|error| format!("noise router field {field}: {error}"))
        };
        let router = RouterFunctions {
            temperature: router_entry(RouterOutput::Temperature)?,
            vegetation: router_entry(RouterOutput::Vegetation)?,
            continents: router_entry(RouterOutput::Continents)?,
            erosion: router_entry(RouterOutput::Erosion)?,
            depth: router_entry(RouterOutput::Depth)?,
            ridges: router_entry(RouterOutput::Ridges)?,
            chunk_surface_level: router_entry(RouterOutput::ChunkSurfaceLevel)?,
            final_density: router_entry(RouterOutput::FinalDensity)?,
        };
        Ok(Self {
            min_y,
            height,
            sea_level,
            legacy_random_source,
            router,
        })
    }
}

fn int_field(object: &serde_json::Map<String, Value>, name: &str) -> Result<i32, String> {
    object
        .get(name)
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .ok_or_else(|| format!("missing field {name}"))
}

/// 编译好的八个采样器。字段分开放，这样借 `context` 和借采样器不冲突。
#[derive(Debug)]
pub struct Samplers {
    pub temperature: Sampler,
    pub vegetation: Sampler,
    pub continents: Sampler,
    pub erosion: Sampler,
    pub depth: Sampler,
    pub ridges: Sampler,
    pub chunk_surface_level: Sampler,
    pub final_density: Sampler,
}

impl Samplers {
    pub fn get(&self, output: RouterOutput) -> &Sampler {
        match output {
            RouterOutput::Temperature => &self.temperature,
            RouterOutput::Vegetation => &self.vegetation,
            RouterOutput::Continents => &self.continents,
            RouterOutput::Erosion => &self.erosion,
            RouterOutput::Depth => &self.depth,
            RouterOutput::Ridges => &self.ridges,
            RouterOutput::ChunkSurfaceLevel => &self.chunk_surface_level,
            RouterOutput::FinalDensity => &self.final_density,
        }
    }
}

/// 官方的 `RandomState`：一个维度的设置配上一个种子。
#[derive(Debug)]
pub struct SeededRouter {
    compiler: Compiler,
    pub samplers: Samplers,
    pub context: Context,
}

impl SeededRouter {
    /// `RandomState.create(noises, seed, settings)`。
    pub fn new(settings: &Settings, seed: i64) -> Self {
        let mut compiler = Compiler::new(seed, settings.legacy_random_source);
        let router = &settings.router;
        let samplers = Samplers {
            temperature: compiler.compile(&router.temperature),
            vegetation: compiler.compile(&router.vegetation),
            continents: compiler.compile(&router.continents),
            erosion: compiler.compile(&router.erosion),
            depth: compiler.compile(&router.depth),
            ridges: compiler.compile(&router.ridges),
            chunk_surface_level: compiler.compile(&router.chunk_surface_level),
            final_density: compiler.compile(&router.final_density),
        };
        let context = Context::new(compiler.cache_count());
        Self {
            compiler,
            samplers,
            context,
        }
    }

    pub fn seed(&self) -> i64 {
        self.compiler.seed()
    }

    /// 再编译一个注册表里的密度函数，和路由共用同一套 `cache` 格子。
    pub fn compile(&mut self, function: &Ref) -> Sampler {
        self.compiler.compile(function)
    }

    /// `RandomState.getOrCreateNoise(key)`。
    ///
    /// 注意这条路不管下界温度和植被那两条老 fbm，密度函数引用它们时走的是另一条。
    pub fn noise(
        &mut self,
        id: &str,
        parameters: &std::sync::Arc<crate::noise::NormalNoise>,
    ) -> std::sync::Arc<crate::noise::Noise> {
        self.compiler.get_or_create_noise(id, parameters)
    }

    /// 用这个维度的上下文求一个另外编译出来的采样器。
    pub fn sample_with(&mut self, sampler: &Sampler, path: Path, x: i32, y: i32, z: i32) -> f32 {
        sampler.sample(&mut self.context, path, x, y, z)
    }

    /// `RandomState.sampleBlockValueUncached` 对应的单点求值。
    pub fn sample(&mut self, output: RouterOutput, x: i32, y: i32, z: i32) -> f32 {
        self.sample_router(output, Path::Point, x, y, z)
    }

    /// 路由的一个输出，自己挑求值路径。
    pub fn sample_router(
        &mut self,
        output: RouterOutput,
        path: Path,
        x: i32,
        y: i32,
        z: i32,
    ) -> f32 {
        self.samplers
            .get(output)
            .sample(&mut self.context, path, x, y, z)
    }

    /// `Climate.Sampler.sample(quartX, quartY, quartZ)`。
    pub fn climate(
        &mut self,
        quart_x: i32,
        quart_y: i32,
        quart_z: i32,
    ) -> crate::climate::TargetPoint {
        let sampler = crate::climate::Sampler {
            temperature: &self.samplers.temperature,
            humidity: &self.samplers.vegetation,
            continentalness: &self.samplers.continents,
            erosion: &self.samplers.erosion,
            depth: &self.samplers.depth,
            weirdness: &self.samplers.ridges,
        };
        sampler.sample(&mut self.context, quart_x, quart_y, quart_z)
    }
}
