//! 方块逻辑要读的天气。
//!
//! 权威副本在世界线程的 `Global` 上。这里只有方块代码用得到的快照：两个渐变强度，以及
//! 按位置算好的降水类型。温度的世界生成数学在 `mc-bamboo-worldgen`，由调用方算好后
//! 经 [`Level`](crate::Level) 传进来。默认是晴天、平原，测试和假世界不改它们时行为与原来相同。
//!
//! The weather snapshot block logic reads.
//!
//! The authoritative copy lives on the world thread's `Global`. This is only the snapshot
//! block code needs: two smoothed levels and the world queries precipitation depends on.
//! Defaults are clear weather over a plains biome; tests and fake worlds keep the old behaviour.

/// 官方 `Level.rainLevel` 和 `thunderLevel`，每刻向 0 或 1 靠近 0.01。
///
/// `isRaining()` 是 `rainLevel > 0.2`，`isThundering()` 还要把两个强度乘起来和 0.9 比。
///
/// `Level.rainLevel` and `thunderLevel`; each tick they move 0.01 towards 0 or 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlockWeather {
    pub rain_level: f32,
    pub thunder_level: f32,
}

impl Default for BlockWeather {
    fn default() -> Self {
        Self::CLEAR
    }
}

impl BlockWeather {
    /// 晴天，两个强度都是 0。
    pub const CLEAR: Self = Self {
        rain_level: 0.0,
        thunder_level: 0.0,
    };

    /// `Level.isRaining()`。
    pub const fn is_raining(self) -> bool {
        self.rain_level > 0.2
    }

    /// `Level.isThundering()`：`getThunderLevel(1)` 乘了 `getRainLevel(1)`。
    pub const fn is_thundering(self) -> bool {
        self.is_raining() && self.thunder_level * self.rain_level > 0.9
    }
}

/// 官方 `Biome.Precipitation`。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Precipitation {
    None,
    #[default]
    Rain,
    Snow,
}

/// 一格的降水条件，官方 `Biome.getPrecipitationAt(pos, seaLevel)` 的结果。
///
/// `Biome.shouldFreeze` / `shouldSnow` 也要看方块光和水，方块逻辑自己从世界读。
/// 温度按位置和群系算好后放在 `precipitation`，不由 `mc-bamboo-blocks` 计算。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PrecipitationFacts {
    /// 这一格所在群系的降水类型，`NONE` 表示群系根本不降水。
    pub precipitation: Precipitation,
    /// `Biome.warmEnoughToRain`：`shouldFreeze` 先看它，暖到能下雨就不结冰。
    pub warm_enough_to_rain: bool,
}

impl PrecipitationFacts {
    /// 官方平原在世界常规高度上的值。
    pub const PLAINS: Self = Self {
        precipitation: Precipitation::Rain,
        warm_enough_to_rain: true,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_official_rain_and_thunder_thresholds() {
        let mut weather = BlockWeather {
            rain_level: 0.2,
            thunder_level: 1.0,
        };
        assert!(!weather.is_raining());
        weather.rain_level = 0.21;
        assert!(weather.is_raining());
        assert!(!weather.is_thundering());
        weather.rain_level = 1.0;
        assert!(weather.is_thundering());
        weather.thunder_level = 0.9;
        assert!(!weather.is_thundering());
    }
}
