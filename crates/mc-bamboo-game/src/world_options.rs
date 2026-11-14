//! `server.properties` 里决定世界怎么生成的四项，以及把它们解析成官方语义。
//!
//! 对应 26.3 `DedicatedServerProperties.WorldDimensionData` 和 `WorldOptions`：
//! `level-seed` 空则随机、数字按 `Long.parseLong`、字符串按 `String.hashCode`；
//! `level-type` 是世界预设的注册名；`generator-settings` 只在 flat 预设里生效；
//! `generate-structures` 先存着，结构接上后生效。
//!
//! The four `server.properties` keys that decide world generation, parsed with
//! vanilla semantics. Mirrors 26.3 `DedicatedServerProperties.WorldDimensionData`
//! and `WorldOptions`: an empty `level-seed` is random, a numeric one uses
//! `Long.parseLong`, any other string uses `String.hashCode`; `level-type` names
//! a world preset; `generator-settings` only applies to the flat preset;
//! `generate-structures` is stored until structures are wired up.

use std::{
    sync::OnceLock,
    time::{SystemTime, UNIX_EPOCH},
};

use mc_bamboo_data::{block_by_name, blocks, registries};
use serde_json::Value;

/// `level-type` 认得的四个主世界预设。
///
/// 26.3 里 `normal` / `large_biomes` / `amplified` 都是 `NoiseBasedChunkGenerator`，
/// 只是 `NoiseGeneratorSettings` 不同，群系源都是 `MultiNoiseBiomeSource` 的
/// `minecraft:overworld` 预设；`flat` 走 `FlatLevelSource`（`WorldPresets.bootstrap`）。
///
/// The four overworld presets `level-type` accepts. In 26.3 `normal`,
/// `large_biomes` and `amplified` are all `NoiseBasedChunkGenerator`s differing
/// only in `NoiseGeneratorSettings`, sharing the `minecraft:overworld` biome
/// preset; `flat` uses `FlatLevelSource` (`WorldPresets.bootstrap`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LevelType {
    #[default]
    Normal,
    Flat,
    LargeBiomes,
    Amplified,
}

impl LevelType {
    /// 官方注册名。
    pub const fn id(self) -> &'static str {
        match self {
            Self::Normal => "minecraft:normal",
            Self::Flat => "minecraft:flat",
            Self::LargeBiomes => "minecraft:large_biomes",
            Self::Amplified => "minecraft:amplified",
        }
    }

    /// 解析 `level-type`。官方先 `toLowerCase(Locale.ROOT)`，再按 `Identifier.tryParse`
    /// 查预设表；`default` / `largebiomes` 是旧名字；查不到就警告并回退 `normal`。
    /// 带不带 `minecraft:` 前缀都收。
    ///
    /// Parse `level-type`: vanilla lowercases, resolves via `Identifier.tryParse`,
    /// keeps the legacy `default` / `largebiomes` aliases, and falls back to
    /// `normal` with a warning. Accepts values with or without the `minecraft:` prefix.
    pub fn from_level_id(raw: &str) -> Option<Self> {
        let lowered = raw.to_ascii_lowercase();
        let bare = lowered.strip_prefix("minecraft:").unwrap_or(&lowered);
        match bare {
            "normal" | "default" => Some(Self::Normal),
            "flat" | "flat_all_dimensions" => Some(Self::Flat),
            "large_biomes" | "largebiomes" => Some(Self::LargeBiomes),
            "amplified" => Some(Self::Amplified),
            _ => None,
        }
    }

    pub const fn is_flat(self) -> bool {
        matches!(self, Self::Flat)
    }

    /// `NoiseGeneratorSettings` 的注册名；`flat` 没有，返回 `None`。
    ///
    /// The `NoiseGeneratorSettings` id; `None` for `flat`.
    pub const fn settings_id(self) -> Option<&'static str> {
        match self {
            Self::Normal => Some("minecraft:overworld"),
            Self::LargeBiomes => Some("minecraft:large_biomes"),
            Self::Amplified => Some("minecraft:amplified"),
            Self::Flat => None,
        }
    }
}

/// 超平坦预设，从 `generator-settings` 的 JSON 解析。
///
/// 官方 `FlatLevelGeneratorSettings.CODEC` 认 `layers`、`biome`、`features`、
/// `lakes` 等；空 JSON（`{}`）得到默认预设。这里只需要地层和群系：其余字段
/// 只影响地物和结构，本轮都不接。
///
/// Flat preset parsed from `generator-settings` JSON. Vanilla's
/// `FlatLevelGeneratorSettings.CODEC` reads `layers`, `biome`, `features`,
/// `lakes`, etc.; an empty `{}` yields the default preset. Only layers and biome
/// matter here: the other fields drive features and structures, not wired yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlatSettings {
    /// 从世界底部往上的 `(默认状态 ID, 层数)`。
    pub layers: Vec<(u32, i32)>,
    /// 每个 quart 格填的群系注册表 ID。
    pub biome: u32,
}

impl Default for FlatSettings {
    /// 官方默认：`bedrock ×1`、`dirt ×2`、`grass_block ×1`，群系 `plains`。
    /// 和 `mc_bamboo_world::flat::generate_chunk` / 导出的 `WORLD_GEN_SETTINGS` 一致。
    ///
    /// Official default: `bedrock ×1`, `dirt ×2`, `grass_block ×1`, biome `plains`.
    /// Matches `mc_bamboo_world::flat::generate_chunk` and the exported `WORLD_GEN_SETTINGS`.
    fn default() -> Self {
        Self {
            layers: vec![
                (blocks::BEDROCK, 1),
                (blocks::DIRT, 2),
                (blocks::GRASS_BLOCK, 1),
            ],
            biome: registries::biome::PLAINS as u32,
        }
    }
}

impl FlatSettings {
    /// 解析 `generator-settings`。解析失败返回错误，让调用方回落默认预设。
    /// 只认 `layers`（`block` + `height`）和 `biome`；缺字段按官方默认补齐。
    ///
    /// Parse `generator-settings`; on failure the caller falls back to the
    /// default. Reads `layers` (`block` + `height`) and `biome`, defaulting the
    /// rest like vanilla.
    pub fn parse(json: &str) -> Result<Self, String> {
        let text = if json.trim().is_empty() { "{}" } else { json };
        let root: Value = serde_json::from_str(text)
            .map_err(|error| format!("bad generator-settings: {error}"))?;
        if !root.is_object() {
            return Err("generator-settings must be a JSON object".to_owned());
        }
        let mut settings = Self::default();
        if let Some(layers) = root.get("layers") {
            let list = layers
                .as_array()
                .ok_or("generator-settings layers must be an array")?;
            let mut parsed = Vec::with_capacity(list.len());
            for entry in list {
                let name = entry
                    .get("block")
                    .and_then(Value::as_str)
                    .ok_or("flat layer is missing block")?;
                let height = entry
                    .get("height")
                    .and_then(Value::as_i64)
                    .ok_or("flat layer is missing height")?;
                let state = block_by_name(name)
                    .ok_or_else(|| format!("unknown flat layer block {name}"))?
                    .default_state;
                let height = i32::try_from(height).map_err(|_| "flat layer height too large")?;
                if height <= 0 {
                    return Err("flat layer height must be positive".to_owned());
                }
                parsed.push((state, height));
            }
            settings.layers = parsed;
        }
        if let Some(biome) = root.get("biome").and_then(Value::as_str) {
            settings.biome = registries::biome::NAMES
                .iter()
                .position(|&name| name == biome)
                .ok_or_else(|| format!("unknown flat biome {biome}"))?
                as u32;
        }
        Ok(settings)
    }
}

/// 一局世界的生成参数。配置和存档都解析成这个值，生成器只读它。
///
/// World generation parameters for one session. Both config files and the save
/// resolve to this value; the generator only reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldGenConfig {
    pub seed: i64,
    pub level_type: LevelType,
    pub generate_structures: bool,
    pub flat: FlatSettings,
}

impl Default for WorldGenConfig {
    /// 嵌入式用法（测试、oracle、压测）默认是超平坦、种子 0：不改现有行为。
    /// 从 `server.properties` 新建的世界默认在 `ConfigFiles` 里改成 `normal`。
    ///
    /// Embedded use (tests, oracle, load tests) defaults to superflat, seed 0,
    /// keeping prior behaviour. A world created from `server.properties` is
    /// switched to `normal` by default in `ConfigFiles`.
    fn default() -> Self {
        Self {
            seed: 0,
            level_type: LevelType::Flat,
            generate_structures: true,
            flat: FlatSettings::default(),
        }
    }
}

impl WorldGenConfig {
    /// 按官方 `DedicatedServerProperties` 组装。空种子用 `random_seed()`；
    /// 坏的 `level-type` / `generator-settings` 回退并交给调用方记日志。
    ///
    /// Assemble like `DedicatedServerProperties`: empty seed uses `random_seed()`;
    /// invalid `level-type` / `generator-settings` fall back (caller logs).
    pub fn from_properties(
        level_seed: &str,
        level_type: &str,
        generator_settings: &str,
        generate_structures: bool,
    ) -> (Self, Vec<String>) {
        let mut warnings = Vec::new();
        let seed = parse_seed(level_seed).unwrap_or_else(random_seed);
        let resolved_type = LevelType::from_level_id(level_type).unwrap_or_else(|| {
            warnings.push(format!(
                "unknown level-type {level_type}, using {}",
                LevelType::Normal.id()
            ));
            LevelType::Normal
        });
        let flat = FlatSettings::parse(generator_settings).unwrap_or_else(|error| {
            warnings.push(format!(
                "bad generator-settings, using default flat preset: {error}"
            ));
            FlatSettings::default()
        });
        (
            Self {
                seed,
                level_type: resolved_type,
                generate_structures,
                flat,
            },
            warnings,
        )
    }
}

/// 官方 `WorldOptions.parseSeed`：空串 `OptionalLong.empty()`，能 `Long.parseLong`
/// 就是那个值，否则 `String.hashCode()`（按 UTF-16 码元、`int` 溢出、`int` 符号扩展成
/// `long`）。返回 `None` 表示空串，调用方再取随机种子。
///
/// Vanilla `WorldOptions.parseSeed`: empty string gives `OptionalLong.empty()`,
/// a `Long.parseLong`-able string gives that value, otherwise `String.hashCode()`
/// (over UTF-16 code units, wrapping `int`, sign-extended to `long`). `None`
/// means empty; the caller then picks a random seed.
pub fn parse_seed(raw: &str) -> Option<i64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(value) = trimmed.parse::<i64>() {
        return Some(value);
    }
    Some(java_string_hash_code(trimmed))
}

/// Java `String.hashCode()`：`h = 31*h + char`，按 UTF-16 码元、`i32` 环绕。
/// Java String.hashCode(): `h = 31*h + char`, over UTF-16 units, wrapping i32.
fn java_string_hash_code(value: &str) -> i64 {
    let mut hash: i32 = 0;
    for unit in value.encode_utf16() {
        hash = hash.wrapping_mul(31).wrapping_add(i32::from(unit));
    }
    i64::from(hash)
}

/// 空 `level-seed` 的随机种子。
///
/// 官方 `WorldOptions.randomSeed()` 是 `RandomSource.create().nextLong()`，靠
/// `nanoTime ^ uniquifier` 播种；Bamboo 没有把这套随机数接进世界线程，这里用
/// 系统纳秒加一个自增常量给一个 long。整个进程只算一次（`OnceLock`），因为
/// `ConfigFiles::to_server_config` 一局里可能被校验、保存、启动各调用一次，
/// 每次都换种子会让「同一局」拿到不同的世界。随机值本来就不和官方逐位比较，
/// 写进存档后即固定。
///
/// Random seed for an empty `level-seed`. Vanilla's `WorldOptions.randomSeed()`
/// is `RandomSource.create().nextLong()` seeded by `nanoTime ^ uniquifier`;
/// Bamboo does not wire that RNG into the world thread, so this uses system
/// nanos plus an incrementing constant. It is computed once per process
/// (`OnceLock`) because `ConfigFiles::to_server_config` may be called several
/// times (validate / save / start); re-rolling would give one session different
/// worlds. A random value is never bit-compared with vanilla; it is fixed once saved.
pub fn random_seed() -> i64 {
    static SEED: OnceLock<i64> = OnceLock::new();
    *SEED.get_or_init(|| {
        use std::sync::atomic::{AtomicU64, Ordering};
        static UNIQUIFIER: AtomicU64 = AtomicU64::new(8_682_522_807_148_012);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        let unique = UNIQUIFIER.fetch_add(1_181_783_497_276_652_981, Ordering::Relaxed);
        (nanos ^ unique) as i64
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_seed_matches_world_options() {
        // 数字按 Long.parseLong。/ Numeric strings use Long.parseLong.
        assert_eq!(parse_seed("0"), Some(0));
        assert_eq!(parse_seed("-1"), Some(-1));
        assert_eq!(parse_seed("+123456789"), Some(123456789));
        assert_eq!(parse_seed("9223372036854775807"), Some(i64::MAX));
        // 空串（含只含空白）返回 None。/ Empty / whitespace-only returns None.
        assert_eq!(parse_seed(""), None);
        assert_eq!(parse_seed("   "), None);
        // 超范围的字符串走 hashCode。/ Out-of-range numeric string falls back to hashCode.
        assert_eq!(
            parse_seed("9223372036854775808"),
            Some(java_string_hash_code("9223372036854775808"))
        );
    }

    #[test]
    fn java_hash_code_matches_known_values() {
        assert_eq!(java_string_hash_code(""), 0);
        assert_eq!(java_string_hash_code("a"), 97);
        assert_eq!(java_string_hash_code("hello"), 99_162_322);
        // 经典样例：hashCode 恰好是 Integer.MIN_VALUE，符号扩展成负 long。
        // Classic sample: hashCode is exactly Integer.MIN_VALUE, sign-extended.
        assert_eq!(
            java_string_hash_code("polygenelubricants"),
            -2_147_483_648i64
        );
    }

    #[test]
    fn level_type_names_and_settings() {
        assert_eq!(
            LevelType::from_level_id("minecraft:normal"),
            Some(LevelType::Normal)
        );
        assert_eq!(LevelType::from_level_id("NORMAL"), Some(LevelType::Normal));
        assert_eq!(LevelType::from_level_id("default"), Some(LevelType::Normal));
        assert_eq!(
            LevelType::from_level_id("minecraft:flat"),
            Some(LevelType::Flat)
        );
        assert_eq!(
            LevelType::from_level_id("largebiomes"),
            Some(LevelType::LargeBiomes)
        );
        assert_eq!(
            LevelType::from_level_id("minecraft:amplified"),
            Some(LevelType::Amplified)
        );
        assert_eq!(LevelType::from_level_id("nonsense"), None);
        assert_eq!(LevelType::Normal.settings_id(), Some("minecraft:overworld"));
        assert_eq!(
            LevelType::LargeBiomes.settings_id(),
            Some("minecraft:large_biomes")
        );
        assert_eq!(
            LevelType::Amplified.settings_id(),
            Some("minecraft:amplified")
        );
        assert_eq!(LevelType::Flat.settings_id(), None);
    }

    #[test]
    fn flat_default_matches_the_official_preset() {
        let settings = FlatSettings::default();
        assert_eq!(settings.layers.len(), 3);
        assert_eq!(settings.layers[0], (blocks::BEDROCK, 1));
        assert_eq!(settings.layers[1], (blocks::DIRT, 2));
        assert_eq!(settings.layers[2], (blocks::GRASS_BLOCK, 1));
        assert_eq!(settings.biome, registries::biome::PLAINS as u32);
    }

    #[test]
    fn flat_parse_empty_and_custom() {
        assert_eq!(FlatSettings::parse("{}").unwrap(), FlatSettings::default());
        assert_eq!(FlatSettings::parse("").unwrap(), FlatSettings::default());
        let custom =
            r#"{"layers":[{"block":"minecraft:stone","height":3}],"biome":"minecraft:desert"}"#;
        let parsed = FlatSettings::parse(custom).unwrap();
        assert_eq!(parsed.layers, vec![(blocks::STONE, 3)]);
        assert_eq!(parsed.biome, registries::biome::DESERT as u32);
        assert!(FlatSettings::parse("[]").is_err());
        assert!(FlatSettings::parse(r#"{"layers":"x"}"#).is_err());
    }

    #[test]
    fn from_properties_normal_by_default_and_random_seed() {
        let (config, warnings) =
            WorldGenConfig::from_properties("", "minecraft:normal", "{}", true);
        assert_eq!(config.level_type, LevelType::Normal);
        assert!(config.generate_structures);
        assert!(warnings.is_empty());
        // 空种子由 from_properties 解析成某个 long；这里只验证 random_seed 稳定且非 0。
        // An empty seed resolves to some long; here we only check random_seed is stable and nonzero.
        assert_eq!(random_seed(), random_seed());
        assert_ne!(random_seed(), 0);
    }

    #[test]
    fn from_properties_reports_bad_values() {
        let (config, warnings) =
            WorldGenConfig::from_properties("seed", "minecraft:nope", "[", false);
        assert_eq!(config.level_type, LevelType::Normal);
        assert_eq!(config.seed, java_string_hash_code("seed"));
        assert!(!config.generate_structures);
        assert_eq!(warnings.len(), 2);
    }
}
