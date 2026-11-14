//! 世界生成的纯计算部分：种子加坐标到数值，照官方 `net.minecraft.world.level.levelgen` 移植。
//!
//! 这个 crate 是纯函数，**还没有接进游戏**：不认识世界线程，也不改世界状态。
//! 已有的 [`chunk`] 能把一个主世界区块生成到噪声阶段（群系 + 方块），见 `docs/worldgen.md`；
//! 地表规则、雕刻器、地物、结构、光照和区块流水线是以后的里程碑。
//!
//! 包含：
//!
//! * [`random`]：`XoroshiroRandomSource`、`RandomSupport` 的种子混合、两种
//!   `PositionalRandomFactory` 和 `WorldgenRandom` 的种子设置。
//!   旧的 `LegacyRandomSource` 在 `mc-bamboo-blocks` 里（`LegacyRandom`），这里直接用。
//! * [`noise`]：`synth` 下的全部噪声。
//! * [`density`] 和 [`sampler`]：密度函数的解析、范围、编译和求值。
//! * [`spline`]：三次样条。
//! * [`climate`] 和 [`biome`]：气候参数、R 树查找和三个维度的群系源。
//! * [`settings`]：`NoiseGeneratorSettings`、`NoiseRouter` 和 `RandomState`。
//! * [`volume`]、`[`aquifer`]` 和 [`chunk`]：成批求值的体积、含水层和区块的两个生成阶段。
//!
//! 数据（`worldgen/noise`、`worldgen/density_function`、`worldgen/noise_settings` 和
//! 两个多噪声群系预设）由 `mc-bamboo-vanilla` 的生成器从官方导出，存在 `mc-bamboo-data` 里，
//! 这里用 [`Worldgen::load`] 读。
//!
//! ## 两条求值路径
//!
//! 官方的密度函数有两套实现：`sampleValue` 单点求值，`sampleVolume` 成批求值。
//! 两条路径算的是同一个数学函数，但浮点运算的顺序不同，最后一位会差，所以两条都移植了，
//! 见 [`sampler::Path`]。`interpolated` 在单点求值时读的是输入的成批路径，两条都用得到。
//!
//! 官方在编译前还跑一遍 `DfRewriteRule.SLICE_UNIFORM_AXES`，把用不到某个轴的子树包进
//! `slice` 里。`domainAxes` 是保守的，那一步不改单点求值的值，但会改变成批求值时体积的
//! 形状、进而改变 `cache` 记住的体积，所以 [`sampler::Compiler::compile`] 照做了
//! （定义域本身见 [`density::Function::axes`]）。

pub mod aquifer;
pub mod biome;
pub mod biome_manager;
pub mod biome_temperature;
pub mod carver;
pub mod chunk;
pub mod climate;
pub mod density;
pub mod feature;
pub mod interval;
pub mod material;
pub mod math;
pub mod noise;
pub mod random;
pub mod sampler;
pub mod settings;
pub mod spline;
pub mod volume;

use std::collections::HashMap;

use serde_json::Value;

use crate::{
    biome::{MultiNoiseBiomeSource, TheEndBiomeSource},
    climate::{ParameterList, ParameterPoint},
    density::{FunctionRegistry, NoiseRegistry},
    settings::Settings,
};

/// `worldgen` 数据里一条多噪声预设的 13 个定点数：六个区间加一个 `offset`。
const PRESET_PARAMETER_COUNT: usize = 13;

/// 官方数据包里世界生成那几个注册表，解析好的样子。
#[derive(Debug)]
pub struct Worldgen {
    noises: NoiseRegistry,
    functions: FunctionRegistry,
    settings: HashMap<String, Settings>,
    presets: HashMap<String, MultiNoiseBiomeSource>,
    end_biomes: TheEndBiomeSource,
    /// 地表规则和雕刻器要用的原始注册表，官方 codec 的 JSON。
    raw: serde_json::Map<String, Value>,
}

impl Worldgen {
    /// 读 `mc-bamboo-data` 里嵌着的那份官方数据。
    pub fn load() -> Result<Self, String> {
        Self::parse(mc_bamboo_data::worldgen::WORLDGEN)
    }

    /// 从一份 JSON 文本解析，测试和工具用。
    pub fn parse(text: &str) -> Result<Self, String> {
        let root: Value =
            serde_json::from_str(text).map_err(|error| format!("bad worldgen data: {error}"))?;
        let root = root.as_object().ok_or("worldgen data must be an object")?;

        let raw_noises = object_field(root, "noise")?;
        let noises = NoiseRegistry::load(raw_noises)?;
        let raw_functions = object_field(root, "density_function")?;
        let functions = FunctionRegistry::load(raw_functions, &noises)?;

        let mut settings = HashMap::new();
        for (id, value) in object_field(root, "noise_settings")? {
            let parsed = Settings::parse(value, raw_functions, &noises, &functions)
                .map_err(|error| format!("noise settings {id}: {error}"))?;
            settings.insert(id.clone(), parsed);
        }

        let mut presets = HashMap::new();
        for (id, value) in object_field(root, "multi_noise_presets")? {
            let parsed = parse_preset(value).map_err(|error| format!("preset {id}: {error}"))?;
            presets.insert(id.clone(), MultiNoiseBiomeSource::new(parsed));
        }

        // 地表规则、条件和雕刻器是新加的字段；缺了明确报错，别静默生成错的世界。
        // 地物那四个键同理，feature / placed_feature / biome_features 是对象，
        // features_per_step 是数组，形状由 object_field / array_field 一并核对。
        for name in [
            "material_rule",
            "material_condition",
            "carver",
            "biome_carvers",
            "feature",
            "placed_feature",
            "biome_features",
        ] {
            object_field(root, name)?;
        }
        let steps = root
            .get("features_per_step")
            .and_then(Value::as_array)
            .ok_or("missing features_per_step")?;
        if steps.len() != crate::feature::DECORATION_STEPS {
            return Err(format!(
                "features_per_step has {} steps, expected {}",
                steps.len(),
                crate::feature::DECORATION_STEPS
            ));
        }
        for (index, step) in steps.iter().enumerate() {
            step.as_array()
                .ok_or_else(|| format!("features_per_step[{index}] is not an array"))?;
        }

        let end = object_field(root, "end_biomes")?;
        let end_biome = |name: &str| -> Result<String, String> {
            end.get(name)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| format!("missing end biome {name}"))
        };
        let end_biomes = TheEndBiomeSource {
            end: end_biome("end")?,
            highlands: end_biome("highlands")?,
            midlands: end_biome("midlands")?,
            islands: end_biome("islands")?,
            barrens: end_biome("barrens")?,
        };

        Ok(Self {
            noises,
            functions,
            settings,
            presets,
            end_biomes,
            raw: root.clone(),
        })
    }

    pub fn noises(&self) -> &NoiseRegistry {
        &self.noises
    }

    pub fn functions(&self) -> &FunctionRegistry {
        &self.functions
    }

    /// `worldgen/noise_settings` 里的一条，比如 `minecraft:overworld`。
    pub fn settings(&self, id: &str) -> Option<&Settings> {
        self.settings.get(id)
    }

    pub fn settings_ids(&self) -> impl Iterator<Item = &str> {
        self.settings.keys().map(String::as_str)
    }

    /// `worldgen/multi_noise_biome_source_parameter_list` 里的一条。
    pub fn preset(&self, id: &str) -> Option<&MultiNoiseBiomeSource> {
        self.presets.get(id)
    }

    pub fn end_biomes(&self) -> &TheEndBiomeSource {
        &self.end_biomes
    }

    /// 导出数据里还没解析成 Rust 结构的原始注册表。
    pub fn raw(&self) -> &serde_json::Map<String, Value> {
        &self.raw
    }
}

fn object_field<'a>(
    object: &'a serde_json::Map<String, Value>,
    name: &str,
) -> Result<&'a serde_json::Map<String, Value>, String> {
    object
        .get(name)
        .and_then(Value::as_object)
        .ok_or_else(|| format!("missing field {name}"))
}

/// 一条预设：每项是 `{"biome": ID, "parameters": [13 个定点数]}`。
fn parse_preset(value: &Value) -> Result<ParameterList<String>, String> {
    let entries = value.as_array().ok_or("expected a list")?;
    let mut parsed = Vec::with_capacity(entries.len());
    for entry in entries {
        let entry = entry.as_object().ok_or("expected an object")?;
        let biome = entry
            .get("biome")
            .and_then(Value::as_str)
            .ok_or("missing field biome")?;
        let numbers = entry
            .get("parameters")
            .and_then(Value::as_array)
            .ok_or("missing field parameters")?;
        if numbers.len() != PRESET_PARAMETER_COUNT {
            return Err(format!(
                "expected {PRESET_PARAMETER_COUNT} parameters, got {}",
                numbers.len()
            ));
        }
        let mut quantized = [0_i64; PRESET_PARAMETER_COUNT];
        for (slot, number) in quantized.iter_mut().zip(numbers) {
            *slot = number.as_i64().ok_or("parameters must be integers")?;
        }
        parsed.push((ParameterPoint::from_quantized(quantized), biome.to_owned()));
    }
    Ok(ParameterList::new(parsed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        climate::TargetPoint,
        settings::{RouterOutput, SeededRouter},
    };

    fn worldgen() -> Worldgen {
        Worldgen::load().expect("嵌着的官方数据能解析")
    }

    #[test]
    fn the_official_registries_are_complete() {
        let worldgen = worldgen();
        // 官方 26.3 的 worldgen/noise 有 64 条。
        assert_eq!(worldgen.noises().ids().count(), 64);
        assert!(
            worldgen
                .functions()
                .get("minecraft:overworld/offset")
                .is_some()
        );
        assert!(worldgen.functions().get("minecraft:zero").is_some());
        let mut ids: Vec<&str> = worldgen.settings_ids().collect();
        ids.sort_unstable();
        assert_eq!(
            ids,
            [
                "minecraft:amplified",
                "minecraft:caves",
                "minecraft:end",
                "minecraft:floating_islands",
                "minecraft:large_biomes",
                "minecraft:nether",
                "minecraft:overworld",
            ]
        );
        assert!(worldgen.preset("minecraft:overworld").is_some());
        assert!(worldgen.preset("minecraft:nether").is_some());
        assert_eq!(worldgen.end_biomes().end, "minecraft:the_end");
    }

    /// 六条 17 位有效数字的噪声振幅必须和官方运行时的 double 逐位相同。
    ///
    /// 官方内存里的位模式由 `Double.doubleToRawLongBits` 读出；`serde_json` 不做正确
    /// 舍入的快路径会把这些十进制串全部读成低 1 ulp 的值，于是对拍会悄悄失准。
    #[test]
    fn noise_amplitudes_match_the_official_bits() {
        let worldgen = worldgen();
        for (id, bits) in [
            ("minecraft:jagged", 0x3ff0_9ceb_74ac_70d7),
            ("minecraft:nether/temperature", 0x3fee_6215_6c09_de3a),
            ("minecraft:nether/vegetation", 0x3fee_6215_6c09_de3a),
            ("minecraft:offset", 0x3fee_0583_ed0d_7735),
            ("minecraft:pillar", 0x3fee_6215_6c09_de3a),
            ("minecraft:ridge", 0x3fed_4558_d7b1_d486),
        ] {
            let noise = worldgen.noises().get(id).expect("噪声在注册表里");
            assert_eq!(
                noise.parameters().base_amplitude.to_bits(),
                bits,
                "{id} 的振幅变了"
            );
        }
    }

    #[test]
    fn dimension_settings_match_the_official_numbers() {
        let worldgen = worldgen();
        let overworld = worldgen.settings("minecraft:overworld").expect("有主世界");
        assert_eq!((overworld.min_y, overworld.height), (-64, 384));
        assert_eq!(overworld.sea_level, 63);
        assert!(!overworld.legacy_random_source);

        let nether = worldgen.settings("minecraft:nether").expect("有下界");
        assert_eq!((nether.min_y, nether.height), (0, 128));
        assert_eq!(nether.sea_level, 32);
        assert!(nether.legacy_random_source);

        let end = worldgen.settings("minecraft:end").expect("有末地");
        assert_eq!((end.min_y, end.height), (0, 128));
        assert!(end.legacy_random_source);
    }

    /// 三个维度的路由都能编译，也都能在一个坐标上求出有限的值。
    #[test]
    fn every_dimension_router_compiles_and_samples() {
        let worldgen = worldgen();
        for id in ["minecraft:overworld", "minecraft:nether", "minecraft:end"] {
            let settings = worldgen.settings(id).expect("有这个维度");
            let mut router = SeededRouter::new(settings, 1);
            for output in RouterOutput::ALL {
                let value = router.sample(output, 100, 64, -200);
                assert!(value.is_finite(), "{id} {output:?} 给了 {value}");
            }
        }
    }

    /// 主世界在同一个坐标上算两次要给出同一个值（`cache` 不能把结果弄脏）。
    #[test]
    fn sampling_is_repeatable() {
        let worldgen = worldgen();
        let settings = worldgen.settings("minecraft:overworld").expect("有主世界");
        let mut router = SeededRouter::new(settings, 12_345);
        let first = router.sample(RouterOutput::FinalDensity, 37, 11, -59);
        let _ = router.sample(RouterOutput::FinalDensity, 1_000, 11, 1_000);
        let again = router.sample(RouterOutput::FinalDensity, 37, 11, -59);
        assert_eq!(first.to_bits(), again.to_bits());
    }

    /// 真实数据上，R 树找到的群系就是 fitness 最小的那一条。
    ///
    /// 不直接和暴力查找比名字：平局时两边挑的不是同一条，官方也是这样。
    #[test]
    fn overworld_biomes_are_a_nearest_entry() {
        let worldgen = worldgen();
        let settings = worldgen.settings("minecraft:overworld").expect("有主世界");
        let source = worldgen.preset("minecraft:overworld").expect("有预设");
        let mut router = SeededRouter::new(settings, 7);
        for index in 0..16 {
            let quart = (index * 37 - 200, 16, index * -53 + 100);
            let target = router.climate(quart.0, quart.1, quart.2);
            let best = source
                .parameters()
                .entries()
                .map(|(point, _)| point.fitness(target))
                .min()
                .expect("预设非空");
            let found = source.biome(target).to_owned();
            let found_fitness = source
                .parameters()
                .entries()
                .filter(|(_, biome)| **biome == found)
                .map(|(point, _)| point.fitness(target))
                .min()
                .expect("找到的群系在表里");
            assert_eq!(found_fitness, best, "{quart:?} {found}");
        }
    }

    /// 下界的温度和植被噪声走老的 fbm，种子偏移不同，两项不会一样。
    #[test]
    fn nether_temperature_and_vegetation_differ() {
        let worldgen = worldgen();
        let settings = worldgen.settings("minecraft:nether").expect("有下界");
        let mut router = SeededRouter::new(settings, 99);
        let temperature = router.sample(RouterOutput::Temperature, 24, 40, -8);
        let vegetation = router.sample(RouterOutput::Vegetation, 24, 40, -8);
        assert_ne!(temperature.to_bits(), vegetation.to_bits());
    }

    /// 末地中心是 `the_end`，远处按 `erosion` 分带。
    #[test]
    fn the_end_biomes_use_the_erosion_router() {
        let worldgen = worldgen();
        let settings = worldgen.settings("minecraft:end").expect("有末地");
        let mut router = SeededRouter::new(settings, 3);
        let end = worldgen.end_biomes();
        assert_eq!(end.biome_at(&mut router, (0, 0, 0)), "minecraft:the_end");
        let far = end.biome_at(&mut router, (4_000, 0, 4_000)).to_owned();
        assert!(
            [
                "minecraft:end_highlands",
                "minecraft:end_midlands",
                "minecraft:small_end_islands",
                "minecraft:end_barrens",
            ]
            .contains(&far.as_str()),
            "{far}"
        );
    }

    /// 噪声的两条求值路径在几个已知点上和官方逐位相同。
    ///
    /// 期望值是 `mc-bamboo-vanilla worldgen` 里官方那一侧打出来的 `floatToRawIntBits`，
    /// 坐标的系数也和那边一样（xz 是 0.3125，y 是 1.25）。
    #[test]
    fn noise_values_match_official() {
        const XZ: f64 = 0.3125;
        const Y: f64 = 1.25;
        /// 设置 ID、噪声 ID、x、y、z、`get` 的位、`addToVolume` 的位。
        const CASES: [(&str, &str, i32, i32, i32, i32, i32); 6] = [
            (
                "minecraft:overworld",
                "minecraft:offset",
                -411,
                -51,
                -474,
                -1_099_372_801,
                -1_099_372_801,
            ),
            (
                "minecraft:overworld",
                "minecraft:offset",
                -15_949_877,
                7_241,
                8_647_937,
                -1_095_081_083,
                -1_095_081_083,
            ),
            (
                "minecraft:overworld",
                "minecraft:temperature",
                -434,
                102,
                232,
                1_047_519_842,
                1_047_519_842,
            ),
            (
                "minecraft:overworld",
                "minecraft:temperature",
                -42_003,
                1_783,
                27_526,
                -1_101_225_124,
                -1_101_225_124,
            ),
            (
                "minecraft:nether",
                "minecraft:offset",
                -411,
                -51,
                -474,
                -1_087_587_695,
                -1_087_587_695,
            ),
            (
                "minecraft:nether",
                "minecraft:temperature",
                -2_913_569,
                78_506,
                14_030_031,
                -1_117_112_024,
                -1_117_112_024,
            ),
        ];

        let worldgen = worldgen();
        for (settings, id, x, y, z, direct, volume) in CASES {
            let parameters = worldgen.noises().get(id).expect("有这条噪声").clone();
            let mut router = SeededRouter::new(worldgen.settings(settings).expect("有这个维度"), 0);
            let noise = router.noise(id, &parameters);
            let ours = noise.get(f64::from(x) * XZ, f64::from(y) * Y, f64::from(z) * XZ);
            assert_eq!(ours.to_bits() as i32, direct, "{settings} {id} get");
            let ours = noise.add_to_point((x, y, z), XZ, Y, 1.0);
            assert_eq!(ours.to_bits() as i32, volume, "{settings} {id} addToVolume");
        }
    }

    /// 噪声路由的输出在几个已知点上和官方逐位相同。
    #[test]
    fn router_values_match_official() {
        /// 设置 ID、输出、x、y、z、`sampleValue` 的位。
        const CASES: [(&str, RouterOutput, i32, i32, i32, i32); 6] = [
            (
                "minecraft:overworld",
                RouterOutput::Temperature,
                -133,
                61,
                -410,
                -1_131_075_292,
            ),
            (
                "minecraft:overworld",
                RouterOutput::Temperature,
                12_115_775,
                -39_575,
                -13_200_574,
                1_053_550_547,
            ),
            (
                "minecraft:overworld",
                RouterOutput::FinalDensity,
                -225,
                66,
                456,
                1_000_017_732,
            ),
            (
                "minecraft:overworld",
                RouterOutput::FinalDensity,
                61_602,
                -615,
                68_335,
                1_025_083_178,
            ),
            (
                "minecraft:nether",
                RouterOutput::FinalDensity,
                -225,
                66,
                456,
                -1_149_835_541,
            ),
            (
                "minecraft:end",
                RouterOutput::FinalDensity,
                -225,
                66,
                456,
                -1_091_917_141,
            ),
        ];

        let worldgen = worldgen();
        for (settings, output, x, y, z, expected) in CASES {
            let mut router = SeededRouter::new(worldgen.settings(settings).expect("有这个维度"), 0);
            let ours = router.sample(output, x, y, z);
            assert_eq!(ours.to_bits() as i32, expected, "{settings} {output:?}");
        }
    }

    /// 群系在几个已知的 quart 坐标上和官方一样。
    #[test]
    fn biome_names_match_official() {
        let worldgen = worldgen();
        let overworld = worldgen.preset("minecraft:overworld").expect("有预设");
        let mut router = SeededRouter::new(
            worldgen.settings("minecraft:overworld").expect("有主世界"),
            0,
        );
        overworld.reset();
        assert_eq!(
            overworld.biome_at(&mut router, (119, -3, -57)),
            "minecraft:forest"
        );
        assert_eq!(
            overworld.biome_at(&mut router, (-11_688, 14, -19_224)),
            "minecraft:forest"
        );
        assert_eq!(
            overworld.biome_at(&mut router, (1_088_096, 4_637, -2_263_635)),
            "minecraft:taiga"
        );

        let nether = worldgen.preset("minecraft:nether").expect("有预设");
        let mut router =
            SeededRouter::new(worldgen.settings("minecraft:nether").expect("有下界"), 0);
        nether.reset();
        assert_eq!(
            nether.biome_at(&mut router, (43, 23, 122)),
            "minecraft:nether_wastes"
        );
        assert_eq!(
            nether.biome_at(&mut router, (-1_222_944, 13_347, -916_587)),
            "minecraft:crimson_forest"
        );

        let mut router = SeededRouter::new(worldgen.settings("minecraft:end").expect("有末地"), 0);
        let end = worldgen.end_biomes();
        assert_eq!(end.biome_at(&mut router, (2, 66, -60)), "minecraft:the_end");
        assert_eq!(
            end.biome_at(&mut router, (-9_086, -85, 13_552)),
            "minecraft:small_end_islands"
        );
    }

    /// 同一个种子算出来的气候参数不随调用顺序变。
    #[test]
    fn climate_targets_are_deterministic() {
        let worldgen = worldgen();
        let settings = worldgen.settings("minecraft:overworld").expect("有主世界");
        let mut router = SeededRouter::new(settings, 42);
        let first: Vec<TargetPoint> = (0..4).map(|i| router.climate(i, 4, -i)).collect();
        let second: Vec<TargetPoint> = (0..4).map(|i| router.climate(i, 4, -i)).collect();
        assert_eq!(first, second);
    }
}
