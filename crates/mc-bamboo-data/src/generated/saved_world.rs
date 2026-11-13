// 这个文件由 mc-bamboo-vanilla 生成，不要手动修改。

//! 官方 `WorldGenSettings.CODEC` 编出的超平坦默认值。主世界生成器是
//! `minecraft:flat`，地层与 Bamboo 的超平坦模板一致，官方服务端打开
//! 这个世界时新生成的区块和 Bamboo 一致。

/// `world_gen_settings.dat` 的 `data` 复合标签（SNBT）。
pub const WORLD_GEN_SETTINGS: &str = r#"{bonus_chest: 0b, seed: 0L, generate_structures: 1b, dimensions: {"minecraft:overworld": {generator: {settings: {features: 0b, biome: "minecraft:plains", layers: [{block: "minecraft:bedrock", height: 1}, {block: "minecraft:dirt", height: 2}, {block: "minecraft:grass_block", height: 1}], lakes: 0b}, type: "minecraft:flat"}, type: "minecraft:overworld"}}}"#;
