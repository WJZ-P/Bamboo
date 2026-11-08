//! 官方的两种群系源：`MultiNoiseBiomeSource`（主世界和下界）和 `TheEndBiomeSource`。
//!
//! 这里只做「给定 quart 坐标算出群系」这一步，`BiomeResolver` 的成批版本和区块里的
//! 群系容器都归区块流程，不在这个 crate 里。

use crate::{
    climate::{ParameterList, TargetPoint},
    math::{block_to_section, quart_to_block},
    sampler::{Context, Path, Sampler},
    settings::SeededRouter,
};

/// `MultiNoiseBiomeSource`：按六个气候参数在参数表里找最近的一条。
#[derive(Debug)]
pub struct MultiNoiseBiomeSource {
    parameters: ParameterList<String>,
}

impl MultiNoiseBiomeSource {
    pub fn new(parameters: ParameterList<String>) -> Self {
        Self { parameters }
    }

    /// `MultiNoiseBiomeSource.getNoiseBiome(target)`。
    pub fn biome(&self, target: TargetPoint) -> &str {
        self.parameters.find_value(target)
    }

    /// `createResolver` 那条路：先采气候再查表。
    pub fn biome_at(&self, router: &mut SeededRouter, quart: (i32, i32, i32)) -> &str {
        let target = router.climate(quart.0, quart.1, quart.2);
        self.biome(target)
    }

    /// 清掉 R 树的「上一次结果」。
    pub fn reset(&self) {
        self.parameters.reset();
    }

    pub fn parameters(&self) -> &ParameterList<String> {
        &self.parameters
    }
}

/// `TheEndBiomeSource` 的五个群系，顺序和官方构造参数一致。
#[derive(Debug, Clone)]
pub struct TheEndBiomeSource {
    pub end: String,
    pub highlands: String,
    pub midlands: String,
    pub islands: String,
    pub barrens: String,
}

impl TheEndBiomeSource {
    /// `TheEndBiomeSource.getNoiseBiome(quartX, quartY, quartZ, sampler)`。
    ///
    /// 官方问的是 `Climate.Sampler.erosion()`，也就是末地路由里的 `erosion` 那一项。
    pub fn biome(&self, erosion: &Sampler, context: &mut Context, quart: (i32, i32, i32)) -> &str {
        let block_x = quart_to_block(quart.0);
        let block_y = quart_to_block(quart.1);
        let block_z = quart_to_block(quart.2);
        let chunk_x = block_to_section(block_x);
        let chunk_z = block_to_section(block_z);
        if i64::from(chunk_x) * i64::from(chunk_x) + i64::from(chunk_z) * i64::from(chunk_z) <= 4096
        {
            return &self.end;
        }
        let weird_x = (chunk_x.wrapping_mul(2).wrapping_add(1)).wrapping_mul(8);
        let weird_z = (chunk_z.wrapping_mul(2).wrapping_add(1)).wrapping_mul(8);
        let height = f64::from(erosion.sample(context, Path::Point, weird_x, block_y, weird_z));
        if height > 0.25 {
            &self.highlands
        } else if height >= -0.0625 {
            &self.midlands
        } else if height < -0.21875 {
            &self.islands
        } else {
            &self.barrens
        }
    }

    /// 按末地路由直接算，省掉手动取 `erosion`。
    pub fn biome_at(&self, router: &mut SeededRouter, quart: (i32, i32, i32)) -> &str {
        let erosion = &router.samplers.erosion;
        self.biome(erosion, &mut router.context, quart)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::climate::ParameterPoint;

    fn point(temperature: i64) -> ParameterPoint {
        ParameterPoint::from_quantized([temperature, temperature, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
    }

    #[test]
    fn multi_noise_picks_the_closest_entry() {
        let source = MultiNoiseBiomeSource::new(ParameterList::new(vec![
            (point(-10_000), "cold".to_owned()),
            (point(10_000), "warm".to_owned()),
        ]));
        let cold = TargetPoint {
            temperature: -5_000,
            humidity: 0,
            continentalness: 0,
            erosion: 0,
            depth: 0,
            weirdness: 0,
        };
        assert_eq!(source.biome(cold), "cold");
        let warm = TargetPoint {
            temperature: 9_000,
            ..cold
        };
        assert_eq!(source.biome(warm), "warm");
    }

    /// 中心 ±64 个区块以内一律是 `the_end`，不问噪声。
    #[test]
    fn the_end_centre_is_always_the_end() {
        let source = TheEndBiomeSource {
            end: "the_end".to_owned(),
            highlands: "highlands".to_owned(),
            midlands: "midlands".to_owned(),
            islands: "islands".to_owned(),
            barrens: "barrens".to_owned(),
        };
        let erosion = Sampler::Constant(1.0);
        let mut context = Context::default();
        assert_eq!(source.biome(&erosion, &mut context, (0, 0, 0)), "the_end");
        // 64 个区块处 64*64 + 0 = 4096，还算中心。
        assert_eq!(source.biome(&erosion, &mut context, (256, 0, 0)), "the_end");
        // 再往外就按 erosion 分，常量 1.0 落在 highlands。
        assert_eq!(
            source.biome(&erosion, &mut context, (260, 0, 0)),
            "highlands"
        );
    }

    #[test]
    fn the_end_outer_bands_follow_the_thresholds() {
        let source = TheEndBiomeSource {
            end: "the_end".to_owned(),
            highlands: "highlands".to_owned(),
            midlands: "midlands".to_owned(),
            islands: "islands".to_owned(),
            barrens: "barrens".to_owned(),
        };
        let mut context = Context::default();
        let far = (1_000, 0, 1_000);
        let biome = |value: f32, context: &mut Context| {
            source
                .biome(&Sampler::Constant(value), context, far)
                .to_owned()
        };
        assert_eq!(biome(0.5, &mut context), "highlands");
        assert_eq!(biome(0.0, &mut context), "midlands");
        assert_eq!(biome(-0.1, &mut context), "barrens");
        assert_eq!(biome(-0.5, &mut context), "islands");
    }
}
