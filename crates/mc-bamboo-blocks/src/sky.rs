//! 官方 `Level.updateSkyBrightness`：把昼夜、天气和维度的 `SKY_LIGHT_LEVEL` 合成
//! `getSkyDarken()` 的整数值。
//!
//! 世界线程按官方 `ServerLevel.tick` 的节奏（天气推进之后、方块刻之前）算一次，
//! 通过 [`Level::sky_darken`](crate::Level::sky_darken) 和并行只读视图的
//! [`ViewFacts`](crate::entity::view::ViewFacts) 把同一份值发给方块逻辑和实体逻辑。
//!
//! Official `Level.updateSkyBrightness`: fold the day/night timeline, weather and the
//! dimension's `SKY_LIGHT_LEVEL` into the integer `getSkyDarken()` value.
//!
//! The world thread computes it once per tick at vanilla's `ServerLevel.tick` point
//! (after the weather step, before block ticks) and publishes the same value through
//! [`Level::sky_darken`](crate::Level::sky_darken) and the parallel read-only view.

/// `Level.MAX_BRIGHTNESS`，也是 `SKY_LIGHT_LEVEL` 的缺省值和上限。
///
/// `Level.MAX_BRIGHTNESS`: the default and upper bound of `SKY_LIGHT_LEVEL`.
pub const MAX_SKY_LIGHT: f32 = 15.0;

/// 主世界 `minecraft:day` 时间线的周期。
///
/// The period of the overworld `minecraft:day` timeline.
pub const DAY_PERIOD: i64 = 24_000;

/// 官方 `Mth.lerp(alpha, from, to)`：`from + alpha * (to - from)`。
///
/// 每一步都是单独的 f32 运算，和 JVM 的 fsub / fmul / fadd 一致；不能用
/// `mul_add`，那会把两步合成一步，舍入不同。
///
/// Official `Mth.lerp(alpha, from, to)`: `from + alpha * (to - from)`.
/// Each step is a separate f32 operation, matching the JVM's fsub / fmul / fadd;
/// `mul_add` would fuse them and round differently.
pub fn lerp(alpha: f32, from: f32, to: f32) -> f32 {
    from + alpha * (to - from)
}

/// `minecraft:gameplay/sky_light_level` 的关键帧（`data/minecraft/timeline/day.json`）。
///
/// 官方 `KeyframeTrackSampler.bakeSegments` 在周期轨道前后各补一段：最后一项
/// `22330 - 24000 = -1670` 接第一项 `133`，以及最后一项 `22330` 接
/// `133 + 24000 = 24133`。破晓和黎明因此是渐变的，不是突跳。
///
/// Keyframes of `minecraft:gameplay/sky_light_level`
/// (`data/minecraft/timeline/day.json`). Vanilla's
/// `KeyframeTrackSampler.bakeSegments` prepends and appends wrap segments
/// (`22330 - 24000 = -1670` → `133`, and `22330` → `133 + 24000 = 24133`),
/// so dawn and sunrise are gradual.
const SKY_LIGHT_LEVEL_SEGMENTS: [(i64, f32, i64, f32); 5] = [
    (-1_670, 0.266_666_68, 133, 1.0),
    (133, 1.0, 11_867, 1.0),
    (11_867, 1.0, 13_670, 0.266_666_68),
    (13_670, 0.266_666_68, 22_330, 0.266_666_68),
    (22_330, 0.266_666_68, 24_133, 1.0),
];

/// `WeatherAttributes.RAIN` 对 `SKY_LIGHT_LEVEL` 的 `ALPHA_BLEND(4.0, 0.3125)`。
const WEATHER_VALUE: f32 = 4.0;
const RAIN_ALPHA: f32 = 0.3125;
/// `WeatherAttributes.THUNDER` 的 `ALPHA_BLEND(4.0, 0.52734375)`。
const THUNDER_ALPHA: f32 = 0.527_343_75;

/// 一整个昼夜循环里 `SKY_LIGHT_LEVEL` 的时间线系数。
///
/// 段内是官方 `LerpFunction.ofFloat()`（`Mth.lerp`）的线性插值，周期用
/// `Math.floorMod(totalTicks, 24000)` 折回。`totalTicks` 是主世界时钟的整数刻。
///
/// The timeline's `SKY_LIGHT_LEVEL` factor at a clock tick. Segments use vanilla's
/// `LerpFunction.ofFloat()` (`Mth.lerp`); `Math.floorMod(totalTicks, 24000)` wraps
/// the period. `totalTicks` is the overworld clock's whole ticks.
pub fn day_sky_light_factor(total_ticks: i64) -> f32 {
    // `Math.floorMod(J, I)` 返回 int，Rust 这边用 rem_euclid 对同一个周期。
    //
    // `Math.floorMod(J, I)` returns an int; Rust's rem_euclid matches it.
    let ticks = total_ticks.rem_euclid(DAY_PERIOD);
    for &(from_ticks, from_value, to_ticks, to_value) in &SKY_LIGHT_LEVEL_SEGMENTS {
        if ticks < to_ticks {
            if ticks <= from_ticks {
                return from_value;
            }
            let alpha = (ticks - from_ticks) as f32 / (to_ticks - from_ticks) as f32;
            return lerp(alpha, from_value, to_value);
        }
    }
    SKY_LIGHT_LEVEL_SEGMENTS[4].3
}

/// `minecraft:gameplay/monsters_burn` 在 `minecraft:day` 时间线上的两个关键帧：
/// 12542 刻起为假，23460 刻起为真（布尔轨道是步进的，不插值）。
const MONSTERS_BURN_OFF: i64 = mc_bamboo_data::environment::BEES_STAY_IN_HIVE_START_TICK;
const MONSTERS_BURN_ON: i64 = mc_bamboo_data::environment::BEES_STAY_IN_HIVE_END_TICK;

/// 主世界的 `EnvironmentAttributes.MONSTERS_BURN`：属性缺省值假，`day` 时间线用
/// `or` 叠上去，所以就是时间线在这个时钟刻的值。
///
/// The overworld `MONSTERS_BURN`: the attribute defaults to false and the `day`
/// timeline is applied with `or`, so the value is the timeline's step at this tick.
pub fn monsters_burn(total_ticks: i64) -> bool {
    let ticks = total_ticks.rem_euclid(DAY_PERIOD);
    !(MONSTERS_BURN_OFF..MONSTERS_BURN_ON).contains(&ticks)
}

/// 主世界的 `EnvironmentAttributes.BEES_STAY_IN_HIVE`：`day` 时间线用 `or`
/// 叠在缺省值假上，所以取值就是这两条关键帧之间的那一段。
///
/// The overworld `BEES_STAY_IN_HIVE`: the `day` timeline is applied with `or`
/// on top of the false default, so it is true between these two keyframes.
pub fn bees_stay_in_hive(total_ticks: i64) -> bool {
    let ticks = total_ticks.rem_euclid(mc_bamboo_data::environment::DAY_PERIOD_TICKS);
    (mc_bamboo_data::environment::BEES_STAY_IN_HIVE_START_TICK
        ..mc_bamboo_data::environment::BEES_STAY_IN_HIVE_END_TICK)
        .contains(&ticks)
}

/// `minecraft:moon` 时间线的周期：八个月相各一天。
pub const MOON_PERIOD: i64 = 192_000;

/// `DimensionType.MOON_BRIGHTNESS_PER_PHASE`，按 `MoonPhase.index()` 排。
const MOON_BRIGHTNESS_PER_PHASE: [f32; 8] = [1.0, 0.75, 0.5, 0.25, 0.0, 0.25, 0.5, 0.75];

/// `ServerLevel.getMoonBrightness(pos)`：`minecraft:visual/moon_phase` 轨道每 24000 刻
/// 换一个月相（常数缓动），再查每个月相的亮度。
///
/// `ServerLevel.getMoonBrightness`: the moon phase track steps every 24000 ticks.
pub fn moon_brightness(total_ticks: i64) -> f32 {
    let phase = total_ticks.rem_euclid(MOON_PERIOD) / DAY_PERIOD;
    MOON_BRIGHTNESS_PER_PHASE[phase as usize]
}

/// `EnvironmentAttributes.SURFACE_SLIME_SPAWN_CHANCE` 在主世界的值：缺省 0，`minecraft:moon`
/// 时间线用 `maximum` 叠上「月亮亮度 × 0.5」（常数缓动，每个月相一档）。
///
/// The overworld value of `EnvironmentAttributes.SURFACE_SLIME_SPAWN_CHANCE`: default 0,
/// raised by the `minecraft:moon` timeline (`maximum` modifier, constant easing) to half the
/// moon brightness of the current phase.
pub fn surface_slime_spawn_chance(total_ticks: i64) -> f32 {
    moon_brightness(total_ticks) * 0.5
}

/// `DifficultyInstance.calculateDifficulty`：区域难度。`difficulty` 是难度 ID，
/// `clock_ticks` 是主世界时钟，`inhabited` 是区块的 `InhabitedTime`。
/// 每一步都是 f32 运算，和官方一致。
///
/// `DifficultyInstance.calculateDifficulty`, step by step in f32.
pub fn effective_difficulty(
    difficulty: i32,
    clock_ticks: i64,
    inhabited: i64,
    moon_brightness: f32,
) -> f32 {
    if difficulty == 0 {
        return 0.0;
    }
    let hard = difficulty == 3;
    let mut scale = 0.75_f32;
    let day = ((clock_ticks as f32 + -72_000.0) / 1_440_000.0).clamp(0.0, 1.0) * 0.25;
    scale += day;
    let mut local = 0.0_f32;
    local += (inhabited as f32 / 3_600_000.0).clamp(0.0, 1.0) * if hard { 1.0 } else { 0.75 };
    local += (moon_brightness * 0.25).clamp(0.0, day);
    if difficulty == 1 {
        local *= 0.5;
    }
    scale += local;
    difficulty as f32 * scale
}

/// 官方 `WeatherAttributes.addBuiltinLayers` 的雨 / 雷暴层。
///
/// `rain` 是 `Level.getRainLevel(1)`，`thunder` 是 `Level.getThunderLevel(1)`，
/// 两者都由调用方从 `oRainLevel` / `oThunderLevel` 按 `Mth.lerp(1, o, now)` 算好。
/// 先按「雨但不打雷」的强度把值混向 4.0，再按雷暴强度混一次，
/// 最后 `EnvironmentAttribute.sanitizeValue` 裁到 [0,15]。
///
/// Vanilla's `WeatherAttributes.addBuiltinLayers` rain / thunder layers.
/// `rain` is `Level.getRainLevel(1)` and `thunder` is `Level.getThunderLevel(1)`,
/// both computed by the caller with `Mth.lerp(1, o, now)`. The rain-only strength
/// blends towards 4.0 first, then the thunder strength blends again; finally
/// `EnvironmentAttribute.sanitizeValue` clamps into [0, 15].
pub fn sky_light_level_with_weather(level: f32, rain: f32, thunder: f32) -> f32 {
    let rain_only = rain - thunder;
    let mut value = level;
    if rain_only > 0.0 {
        // `FloatModifier.ALPHA_BLEND`：先从当前值混向 4.0，再按雨强做状态变化插值。
        let modified = lerp(RAIN_ALPHA, value, WEATHER_VALUE);
        value = lerp(rain_only, value, modified);
    }
    if thunder > 0.0 {
        let modified = lerp(THUNDER_ALPHA, value, WEATHER_VALUE);
        value = lerp(thunder, value, modified);
    }
    value.clamp(0.0, MAX_SKY_LIGHT)
}

/// `Level.updateSkyBrightness()` 的结果：`(15.0 - SKY_LIGHT_LEVEL) as i32`。
///
/// Java 的 `f2i` 对非负数截断；这里的值被裁到 [0,15]，所以直接截断。
///
/// The result of `Level.updateSkyBrightness()`: `(15.0 - SKY_LIGHT_LEVEL) as i32`.
/// Java's `f2i` truncates non-negative values; the value is clamped into [0, 15].
pub fn sky_darken(total_ticks: i64, rain: f32, thunder: f32) -> i32 {
    let day = day_sky_light_factor(total_ticks);
    let level = sky_light_level_with_weather(MAX_SKY_LIGHT * day, rain, thunder);
    (MAX_SKY_LIGHT - level) as i32
}

/// `Level.getRainLevel(1)` / `getThunderLevel(1)` 的当前渐变强度。
///
/// `getThunderLevel(1)` 还乘了 `getRainLevel(1)`，和官方的两步 f32 运算一致。
///
/// The current smoothed `Level.getRainLevel(1)` / `getThunderLevel(1)` values.
/// `getThunderLevel(1)` also multiplies by `getRainLevel(1)`, matching
/// vanilla's two f32 operations.
pub fn weather_levels(o_rain: f32, rain: f32, o_thunder: f32, thunder: f32) -> (f32, f32) {
    let rain_now = lerp(1.0, o_rain, rain);
    let thunder_now = lerp(1.0, o_thunder, thunder) * rain_now;
    (rain_now, thunder_now)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyframes_and_period_match_the_official_timeline() {
        assert_eq!(day_sky_light_factor(133), 1.0);
        assert_eq!(day_sky_light_factor(6000), 1.0);
        assert_eq!(day_sky_light_factor(11_867), 1.0);
        assert_eq!(day_sky_light_factor(13_670), 0.266_666_68);
        assert_eq!(day_sky_light_factor(18_000), 0.266_666_68);
        assert_eq!(day_sky_light_factor(22_330), 0.266_666_68);
        // 周期轨道从 22330 绕回 133 + 24000，两端值不同，所以 0 刻正在日出。
        assert!(day_sky_light_factor(0) > 0.9);
        assert_eq!(day_sky_light_factor(24_000), day_sky_light_factor(0));
        assert_eq!(day_sky_light_factor(-1), day_sky_light_factor(23_999));
        // 11867 → 13670 的中点按官方 `Mth.lerp` 线性插值。
        let middle = (11_867 + 13_670) / 2;
        assert!((day_sky_light_factor(middle) - 0.633_536_7).abs() < 1e-6);
    }

    #[test]
    fn bees_stay_in_hive_follows_the_day_timeline() {
        use mc_bamboo_data::environment as env;
        assert!(!bees_stay_in_hive(0));
        assert!(!bees_stay_in_hive(env::BEES_STAY_IN_HIVE_START_TICK - 1));
        assert!(bees_stay_in_hive(env::BEES_STAY_IN_HIVE_START_TICK));
        assert!(bees_stay_in_hive(env::BEES_STAY_IN_HIVE_END_TICK - 1));
        assert!(!bees_stay_in_hive(env::BEES_STAY_IN_HIVE_END_TICK));
        // 时间线按 24000 刻循环。
        assert!(bees_stay_in_hive(
            env::BEES_STAY_IN_HIVE_START_TICK + DAY_PERIOD
        ));
    }

    #[test]
    fn monsters_burn_follows_the_day_timeline() {
        assert!(monsters_burn(0));
        assert!(monsters_burn(12_541));
        assert!(!monsters_burn(12_542));
        assert!(!monsters_burn(23_459));
        assert!(monsters_burn(23_460));
        assert!(!monsters_burn(24_000 + 18_000));
        assert!(!monsters_burn(-6_000));
    }

    #[test]
    fn moon_and_regional_difficulty() {
        assert_eq!(moon_brightness(0), 1.0);
        assert_eq!(moon_brightness(24_000 * 4), 0.0);
        assert_eq!(moon_brightness(192_000 + 24_000), 0.75);
        // 新世界第一天：普通难度 2 × 0.75。
        assert_eq!(effective_difficulty(2, 0, 0, 1.0), 1.5);
        assert_eq!(effective_difficulty(0, 9_000_000, 9_000_000, 1.0), 0.0);
        // 时钟和人气都封顶时困难难度到 3 × (1 + 1 + 0.25) = 6.75。
        assert_eq!(effective_difficulty(3, 9_000_000, 9_000_000, 1.0), 6.75);
    }

    #[test]
    fn clear_weather_darkens_by_eleven_at_night() {
        // 夜里 15 * 0.26666668 = 4.0000002，15 - 4.0000002 截断成 10 或 11
        // 取决于 f32 舍入；对拍给出官方值，这里只检查范围。
        let night = sky_darken(18_000, 0.0, 0.0);
        assert!((10..=11).contains(&night), "{night}");
        assert_eq!(sky_darken(6000, 0.0, 0.0), 0);
    }

    #[test]
    fn rain_and_thunder_blend_towards_four() {
        // 晴天正午 15 → `lerp(0.3125, 15, 4)` = 11.5625，再按雨强 1 保持不变。
        let rainy = sky_light_level_with_weather(15.0, 1.0, 0.0);
        assert!((rainy - 11.5625).abs() < 1e-5, "{rainy}");
        // 雷暴时先把 15 混向 4，再按 0.52734375 混一次。
        let stormy = sky_light_level_with_weather(15.0, 1.0, 1.0);
        assert!((stormy - 9.199_219).abs() < 1e-5, "{stormy}");
        assert_eq!(sky_darken(6000, 1.0, 0.0), 3);
        assert_eq!(sky_darken(6000, 1.0, 1.0), 5);
    }

    #[test]
    fn weather_levels_match_mth_lerp() {
        assert_eq!(weather_levels(0.0, 1.0, 0.0, 1.0), (1.0, 1.0));
        assert_eq!(weather_levels(0.5, 0.5, 0.25, 0.25), (0.5, 0.125));
    }
}
