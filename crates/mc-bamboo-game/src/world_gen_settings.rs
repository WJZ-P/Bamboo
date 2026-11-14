//! `data/minecraft/world_gen_settings.dat` 的 `data` 复合标签读写。
//!
//! 形状照官方 `WorldGenSettings.CODEC`：`seed` / `generate_structures` /
//! `bonus_chest` 三个 `WorldOptions` 字段，加 `dimensions` 里主世界的 `LevelStem`
//! （`type` + `generator`）。`generator` 要么是 `minecraft:flat`（`FlatLevelGeneratorSettings`
//! 的 `settings`），要么是 `minecraft:noise`（`biome_source` + `settings` 注册名）。
//! 新世界把它写进 `data/`，读档时从存档里取种子和世界类型而不是重新用
//! `server.properties`（官方也是这样固定随机种子的）。
//!
//! Read/write the `data` compound of `data/minecraft/world_gen_settings.dat`.
//! Shape follows vanilla `WorldGenSettings.CODEC`: the three `WorldOptions`
//! fields (`seed` / `generate_structures` / `bonus_chest`) plus `dimensions`'s
//! overworld `LevelStem` (`type` + `generator`). The generator is either
//! `minecraft:flat` (a `FlatLevelGeneratorSettings` `settings`) or
//! `minecraft:noise` (`biome_source` + a `settings` id). A new world writes it
//! to `data/`; on reload the seed and level type come from it rather than from
//! `server.properties` (this is how vanilla fixes a random seed too).

use mc_bamboo_data::{block_by_name, block_of_state, registries};
use mc_bamboo_nbt::Tag;

use crate::world_options::{FlatSettings, LevelType, WorldGenConfig};

/// 从复合标签里按名字取一个字段。
/// Get a field by name from a compound tag.
fn field<'a>(tag: &'a Tag, key: &str) -> Option<&'a Tag> {
    match tag {
        Tag::Compound(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

fn string<'a>(tag: &'a Tag, key: &str) -> Option<&'a str> {
    match field(tag, key)? {
        Tag::String(value) => Some(value),
        _ => None,
    }
}

fn long(tag: &Tag, key: &str) -> Option<i64> {
    match field(tag, key)? {
        Tag::Long(value) => Some(*value),
        _ => None,
    }
}

fn boolean(tag: &Tag, key: &str, default: bool) -> bool {
    match field(tag, key) {
        Some(Tag::Byte(value)) => *value != 0,
        _ => default,
    }
}

/// 把一局的世界生成参数编成官方 `WorldGenSettings` 的 `data` 复合标签。
///
/// Encode a session's world-gen config into the official `WorldGenSettings` `data` compound.
pub fn encode(config: &WorldGenConfig) -> Tag {
    let generator = if config.level_type.is_flat() {
        flat_generator(&config.flat)
    } else {
        noise_generator(
            config
                .level_type
                .settings_id()
                .unwrap_or("minecraft:overworld"),
        )
    };
    let overworld = Tag::Compound(vec![
        ("type".to_owned(), string_tag("minecraft:overworld")),
        ("generator".to_owned(), generator),
    ]);
    Tag::Compound(vec![
        ("bonus_chest".to_owned(), Tag::Byte(0)),
        ("seed".to_owned(), Tag::Long(config.seed)),
        (
            "generate_structures".to_owned(),
            Tag::Byte(i8::from(config.generate_structures)),
        ),
        (
            "dimensions".to_owned(),
            Tag::Compound(vec![("minecraft:overworld".to_owned(), overworld)]),
        ),
    ])
}

fn string_tag(value: &str) -> Tag {
    Tag::String(value.to_owned())
}

/// `NoiseBasedChunkGenerator.CODEC`：`biome_source` 用 `minecraft:overworld` 预设，
/// `settings` 是 `NoiseGeneratorSettings` 的注册名。
/// `NoiseBasedChunkGenerator.CODEC`: `biome_source` uses the `minecraft:overworld`
/// preset, `settings` is a `NoiseGeneratorSettings` id.
fn noise_generator(settings_id: &str) -> Tag {
    Tag::Compound(vec![
        ("type".to_owned(), string_tag("minecraft:noise")),
        (
            "biome_source".to_owned(),
            Tag::Compound(vec![
                ("type".to_owned(), string_tag("minecraft:multi_noise")),
                ("preset".to_owned(), string_tag("minecraft:overworld")),
            ]),
        ),
        ("settings".to_owned(), string_tag(settings_id)),
    ])
}

/// `FlatLevelSource.CODEC` 的 `settings`：默认关掉 features/lakes，只写地层和群系。
/// `settings` for `FlatLevelSource.CODEC`: features/lakes off, only layers and biome.
fn flat_generator(flat: &FlatSettings) -> Tag {
    let layers = flat
        .layers
        .iter()
        .map(|&(state, height)| {
            let name = block_of_state(state)
                .map(|b| b.name)
                .unwrap_or("minecraft:air");
            Tag::Compound(vec![
                ("block".to_owned(), string_tag(name)),
                ("height".to_owned(), Tag::Int(height)),
            ])
        })
        .collect();
    let biome = registries::biome::NAMES
        .get(flat.biome as usize)
        .copied()
        .unwrap_or("minecraft:plains");
    Tag::Compound(vec![
        ("type".to_owned(), string_tag("minecraft:flat")),
        (
            "settings".to_owned(),
            Tag::Compound(vec![
                ("features".to_owned(), Tag::Byte(0)),
                ("lakes".to_owned(), Tag::Byte(0)),
                ("biome".to_owned(), string_tag(biome)),
                ("layers".to_owned(), Tag::List(layers)),
            ]),
        ),
    ])
}

/// 从存档里的 `data` 复合标签恢复世界生成参数。文件缺失或坏字段时返回 `None`，
/// 调用方回退到 `server.properties` 的配置。
///
/// Restore world-gen config from a saved `data` compound. `None` on a missing or
/// malformed file; the caller falls back to `server.properties`.
pub fn decode(data: &Tag) -> Option<WorldGenConfig> {
    let seed = long(data, "seed")?;
    let generate_structures = boolean(data, "generate_structures", true);
    let dimensions = field(data, "dimensions")?;
    let overworld = field(dimensions, "minecraft:overworld")?;
    let generator = field(overworld, "generator")?;
    let kind = string(generator, "type")?;
    let (level_type, flat) = match kind {
        "minecraft:flat" => (LevelType::Flat, decode_flat(generator)),
        "minecraft:noise" => {
            let settings = string(generator, "settings").unwrap_or("minecraft:overworld");
            let level = match settings {
                "minecraft:large_biomes" => LevelType::LargeBiomes,
                "minecraft:amplified" => LevelType::Amplified,
                _ => LevelType::Normal,
            };
            (level, FlatSettings::default())
        }
        _ => return None,
    };
    Some(WorldGenConfig {
        seed,
        level_type,
        generate_structures,
        flat,
    })
}

/// 解析 flat 的 `settings.layers` / `settings.biome`；缺字段或坏方块名回退默认预设。
/// Parse flat `settings.layers` / `settings.biome`; fall back to the default preset.
fn decode_flat(generator: &Tag) -> FlatSettings {
    let Some(settings) = field(generator, "settings") else {
        return FlatSettings::default();
    };
    let mut flat = FlatSettings::default();
    if let Some(biome) = string(settings, "biome")
        && let Some(id) = registries::biome::NAMES
            .iter()
            .position(|&name| name == biome)
    {
        flat.biome = id as u32;
    }
    if let Some(Tag::List(entries)) = field(settings, "layers") {
        let mut layers = Vec::with_capacity(entries.len());
        for entry in entries {
            let (Some(name), Some(height)) = (
                string(entry, "block"),
                match field(entry, "height") {
                    Some(Tag::Int(value)) if *value > 0 => Some(*value),
                    _ => None,
                },
            ) else {
                return FlatSettings::default();
            };
            let Some(block) = block_by_name(name) else {
                return FlatSettings::default();
            };
            layers.push((block.default_state, height));
        }
        if !layers.is_empty() {
            flat.layers = layers;
        }
    }
    flat
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_round_trips_seed_structures_and_level_type() {
        for level in [
            LevelType::Normal,
            LevelType::LargeBiomes,
            LevelType::Amplified,
        ] {
            let config = WorldGenConfig {
                seed: -9_223_372_036_854_775_808,
                level_type: level,
                generate_structures: false,
                ..WorldGenConfig::default()
            };
            let decoded = decode(&encode(&config)).expect("decodes");
            assert_eq!(decoded.seed, config.seed);
            assert_eq!(decoded.level_type, level);
            assert!(!decoded.generate_structures);
        }
    }

    #[test]
    fn flat_round_trips_layers_and_biome() {
        let config = WorldGenConfig {
            seed: 42,
            level_type: LevelType::Flat,
            generate_structures: true,
            flat: FlatSettings {
                layers: vec![(mc_bamboo_data::blocks::STONE, 3)],
                biome: registries::biome::DESERT as u32,
            },
        };
        let decoded = decode(&encode(&config)).expect("decodes");
        assert_eq!(decoded.seed, 42);
        assert_eq!(decoded.level_type, LevelType::Flat);
        assert_eq!(
            decoded.flat.layers,
            vec![(mc_bamboo_data::blocks::STONE, 3)]
        );
        assert_eq!(decoded.flat.biome, registries::biome::DESERT as u32);
    }

    #[test]
    fn default_flat_settings_match_the_exported_snbt() {
        // 编出来的 data 和 mc-bamboo-data 里导出的超平坦常量在 seed/type/layers 上一致。
        // The encoded data agrees with the exported flat SNBT on seed/type/layers.
        let config = WorldGenConfig::default();
        let tag = encode(&config);
        assert_eq!(long(&tag, "seed"), Some(0));
        let generator = field(
            field(field(&tag, "dimensions").unwrap(), "minecraft:overworld").unwrap(),
            "generator",
        )
        .unwrap();
        assert_eq!(string(generator, "type"), Some("minecraft:flat"));
        assert_eq!(decode_flat(generator), FlatSettings::default());
    }
}
