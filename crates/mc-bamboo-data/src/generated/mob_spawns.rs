// 这个文件由 mc-bamboo-vanilla 生成，不要手动修改。

/// 一个 `MobCategory` 的常量，下标和 `MobCategory` 的判别值一致。
#[derive(Debug, Clone, Copy)]
pub struct MobCategoryDef {
    pub name: &'static str,
    /// `getMaxInstancesPerChunk()`。
    pub max_instances: i32,
    /// `isFriendly()`。
    pub friendly: bool,
    /// `isPersistent()`。
    pub persistent: bool,
    /// `getDespawnDistance()`。
    pub despawn_distance: i32,
    /// `getNoDespawnDistance()`。
    pub no_despawn_distance: i32,
}

/// `MobCategory.values()`，顺序就是官方的声明顺序。
#[rustfmt::skip]
pub static MOB_CATEGORIES: &[MobCategoryDef] = &[
    MobCategoryDef { name: "monster", max_instances: 70, friendly: false, persistent: false, despawn_distance: 128, no_despawn_distance: 32 },
    MobCategoryDef { name: "creature", max_instances: 10, friendly: true, persistent: true, despawn_distance: 128, no_despawn_distance: 32 },
    MobCategoryDef { name: "ambient", max_instances: 15, friendly: true, persistent: false, despawn_distance: 128, no_despawn_distance: 32 },
    MobCategoryDef { name: "axolotls", max_instances: 5, friendly: true, persistent: false, despawn_distance: 128, no_despawn_distance: 32 },
    MobCategoryDef { name: "underground_water_creature", max_instances: 5, friendly: true, persistent: false, despawn_distance: 128, no_despawn_distance: 32 },
    MobCategoryDef { name: "water_creature", max_instances: 5, friendly: true, persistent: false, despawn_distance: 128, no_despawn_distance: 32 },
    MobCategoryDef { name: "water_ambient", max_instances: 20, friendly: true, persistent: false, despawn_distance: 64, no_despawn_distance: 32 },
    MobCategoryDef { name: "misc", max_instances: -1, friendly: true, persistent: true, despawn_distance: 128, no_despawn_distance: 32 },
];

/// 加权刷怪表里的一项（`MobSpawnSettings.SpawnerData` 加权重）。
#[derive(Debug, Clone, Copy)]
pub struct MobSpawnerDef {
    /// `minecraft:entity_type` 的注册表 ID。
    pub entity_type: i32,
    pub weight: i32,
    /// `SpawnerData.count()` 的下界。
    pub min: i32,
    /// `SpawnerData.count()` 的上界。
    pub max: i32,
}

/// 一个类别下的加权刷怪表。
#[derive(Debug, Clone, Copy)]
pub struct MobCategorySpawns {
    /// `MobCategory` 的下标。
    pub category: u8,
    pub entries: &'static [MobSpawnerDef],
}

/// 一个实体的刷怪代价（`MobSpawnSettings.MobSpawnCost`）。
#[derive(Debug, Clone, Copy)]
pub struct MobSpawnCostDef {
    pub entity_type: i32,
    pub charge: f64,
    pub energy_budget: f64,
}

/// 一个群系的刷怪设置，按群系注册名排序。
#[derive(Debug, Clone, Copy)]
pub struct BiomeMobSpawns {
    pub name: &'static str,
    pub categories: &'static [MobCategorySpawns],
    pub costs: &'static [MobSpawnCostDef],
    /// `CREATURE_WORLD_GEN_SPAWN_PROBABILITY`（主世界维度层叠群系层）。
    pub creature_spawn_probability: f32,
    /// `SNOW_GOLEM_MELTS` 在这个群系的最终值。
    pub snow_golem_melts: bool,
}

/// 每个群系的刷怪设置。没有条目的群系不在这里，查不到就是空表。
#[rustfmt::skip]
pub static BIOME_MOB_SPAWNS: &[BiomeMobSpawns] = &[
    BiomeMobSpawns {
        name: "minecraft:badlands",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 95, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 1,
                entries: &[
                    MobSpawnerDef { entity_type: 114, weight: 12, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 101, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 26, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 30, weight: 8, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 4, weight: 6, min: 1, max: 2 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3cf5c28f),
        snow_golem_melts: true,
    },
    BiomeMobSpawns {
        name: "minecraft:bamboo_jungle",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 95, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 92, weight: 2, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 1,
                entries: &[
                    MobSpawnerDef { entity_type: 114, weight: 12, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 101, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 26, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 30, weight: 8, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 26, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 99, weight: 40, min: 1, max: 2 },
                    MobSpawnerDef { entity_type: 97, weight: 80, min: 1, max: 2 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:basalt_deltas",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 58, weight: 40, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 81, weight: 100, min: 2, max: 5 },
                ],
            },
            MobCategorySpawns {
                category: 1,
                entries: &[
                    MobSpawnerDef { entity_type: 132, weight: 60, min: 1, max: 2 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:beach",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 95, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 1,
                entries: &[
                    MobSpawnerDef { entity_type: 141, weight: 5, min: 2, max: 5 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:birch_forest",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 95, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 1,
                entries: &[
                    MobSpawnerDef { entity_type: 114, weight: 12, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 101, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 26, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 30, weight: 8, min: 4, max: 4 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:cherry_grove",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 95, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 1,
                entries: &[
                    MobSpawnerDef { entity_type: 101, weight: 1, min: 1, max: 2 },
                    MobSpawnerDef { entity_type: 111, weight: 2, min: 2, max: 6 },
                    MobSpawnerDef { entity_type: 114, weight: 2, min: 2, max: 4 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:cold_ocean",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 95, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 39, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
            MobCategorySpawns {
                category: 5,
                entries: &[
                    MobSpawnerDef { entity_type: 130, weight: 3, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 89, weight: 2, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 6,
                entries: &[
                    MobSpawnerDef { entity_type: 27, weight: 15, min: 3, max: 6 },
                    MobSpawnerDef { entity_type: 113, weight: 15, min: 1, max: 5 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:crimson_forest",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 158, weight: 1, min: 2, max: 4 },
                    MobSpawnerDef { entity_type: 65, weight: 9, min: 3, max: 4 },
                    MobSpawnerDef { entity_type: 102, weight: 5, min: 3, max: 4 },
                ],
            },
            MobCategorySpawns {
                category: 1,
                entries: &[
                    MobSpawnerDef { entity_type: 132, weight: 60, min: 1, max: 2 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:dappled_forest",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 95, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 1,
                entries: &[
                    MobSpawnerDef { entity_type: 114, weight: 12, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 101, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 26, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 30, weight: 8, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 111, weight: 4, min: 2, max: 4 },
                    MobSpawnerDef { entity_type: 55, weight: 4, min: 2, max: 4 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:dark_forest",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 95, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 1,
                entries: &[
                    MobSpawnerDef { entity_type: 114, weight: 12, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 101, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 26, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 30, weight: 8, min: 4, max: 4 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:deep_cold_ocean",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 95, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 39, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
            MobCategorySpawns {
                category: 5,
                entries: &[
                    MobSpawnerDef { entity_type: 130, weight: 3, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 89, weight: 2, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 6,
                entries: &[
                    MobSpawnerDef { entity_type: 27, weight: 15, min: 3, max: 6 },
                    MobSpawnerDef { entity_type: 113, weight: 15, min: 1, max: 5 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:deep_dark",
        categories: &[
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:deep_frozen_ocean",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 95, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 39, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 1,
                entries: &[
                    MobSpawnerDef { entity_type: 105, weight: 1, min: 1, max: 2 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
            MobCategorySpawns {
                category: 5,
                entries: &[
                    MobSpawnerDef { entity_type: 130, weight: 1, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 89, weight: 2, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 6,
                entries: &[
                    MobSpawnerDef { entity_type: 113, weight: 15, min: 1, max: 5 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:deep_lukewarm_ocean",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 95, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 39, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
            MobCategorySpawns {
                category: 5,
                entries: &[
                    MobSpawnerDef { entity_type: 130, weight: 8, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 36, weight: 2, min: 1, max: 2 },
                    MobSpawnerDef { entity_type: 89, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 6,
                entries: &[
                    MobSpawnerDef { entity_type: 27, weight: 8, min: 3, max: 6 },
                    MobSpawnerDef { entity_type: 110, weight: 5, min: 1, max: 3 },
                    MobSpawnerDef { entity_type: 140, weight: 25, min: 8, max: 8 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:deep_ocean",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 95, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 39, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
            MobCategorySpawns {
                category: 5,
                entries: &[
                    MobSpawnerDef { entity_type: 130, weight: 1, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 36, weight: 1, min: 1, max: 2 },
                    MobSpawnerDef { entity_type: 89, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 6,
                entries: &[
                    MobSpawnerDef { entity_type: 27, weight: 10, min: 3, max: 6 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:desert",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 19, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 1, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 50, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 68, weight: 80, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 98, weight: 50, min: 4, max: 4 },
                ],
            },
            MobCategorySpawns {
                category: 1,
                entries: &[
                    MobSpawnerDef { entity_type: 111, weight: 12, min: 2, max: 3 },
                    MobSpawnerDef { entity_type: 19, weight: 1, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: true,
    },
    BiomeMobSpawns {
        name: "minecraft:dripstone_caves",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 95, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 39, weight: 95, min: 4, max: 4 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:end_barrens",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 4, max: 4 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:end_highlands",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 4, max: 4 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:end_midlands",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 4, max: 4 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:eroded_badlands",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 95, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 1,
                entries: &[
                    MobSpawnerDef { entity_type: 114, weight: 12, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 101, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 26, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 30, weight: 8, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 4, weight: 6, min: 1, max: 2 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3cf5c28f),
        snow_golem_melts: true,
    },
    BiomeMobSpawns {
        name: "minecraft:flower_forest",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 95, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 1,
                entries: &[
                    MobSpawnerDef { entity_type: 114, weight: 12, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 101, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 26, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 30, weight: 8, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 111, weight: 4, min: 2, max: 3 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:forest",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 95, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 1,
                entries: &[
                    MobSpawnerDef { entity_type: 114, weight: 12, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 101, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 26, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 30, weight: 8, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 152, weight: 5, min: 4, max: 4 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:frozen_ocean",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 95, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 39, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 1,
                entries: &[
                    MobSpawnerDef { entity_type: 105, weight: 1, min: 1, max: 2 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
            MobCategorySpawns {
                category: 5,
                entries: &[
                    MobSpawnerDef { entity_type: 130, weight: 1, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 89, weight: 2, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 6,
                entries: &[
                    MobSpawnerDef { entity_type: 113, weight: 15, min: 1, max: 5 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:frozen_peaks",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 95, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 1,
                entries: &[
                    MobSpawnerDef { entity_type: 63, weight: 5, min: 1, max: 3 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:frozen_river",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 95, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 39, weight: 1, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
            MobCategorySpawns {
                category: 5,
                entries: &[
                    MobSpawnerDef { entity_type: 130, weight: 2, min: 1, max: 4 },
                ],
            },
            MobCategorySpawns {
                category: 6,
                entries: &[
                    MobSpawnerDef { entity_type: 113, weight: 5, min: 1, max: 5 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:grove",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 95, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 1,
                entries: &[
                    MobSpawnerDef { entity_type: 152, weight: 1, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 111, weight: 8, min: 2, max: 3 },
                    MobSpawnerDef { entity_type: 55, weight: 4, min: 2, max: 4 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:ice_spikes",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 95, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 20, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 131, weight: 80, min: 4, max: 4 },
                ],
            },
            MobCategorySpawns {
                category: 1,
                entries: &[
                    MobSpawnerDef { entity_type: 111, weight: 10, min: 2, max: 3 },
                    MobSpawnerDef { entity_type: 105, weight: 1, min: 1, max: 2 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3d8f5c29),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:jagged_peaks",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 95, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 1,
                entries: &[
                    MobSpawnerDef { entity_type: 63, weight: 5, min: 1, max: 3 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:jungle",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 95, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 92, weight: 2, min: 1, max: 3 },
                ],
            },
            MobCategorySpawns {
                category: 1,
                entries: &[
                    MobSpawnerDef { entity_type: 114, weight: 12, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 101, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 26, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 30, weight: 8, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 26, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 99, weight: 40, min: 1, max: 2 },
                    MobSpawnerDef { entity_type: 97, weight: 1, min: 1, max: 2 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:lukewarm_ocean",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 95, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 39, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
            MobCategorySpawns {
                category: 5,
                entries: &[
                    MobSpawnerDef { entity_type: 130, weight: 10, min: 1, max: 2 },
                    MobSpawnerDef { entity_type: 36, weight: 2, min: 1, max: 2 },
                    MobSpawnerDef { entity_type: 89, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 6,
                entries: &[
                    MobSpawnerDef { entity_type: 27, weight: 15, min: 3, max: 6 },
                    MobSpawnerDef { entity_type: 110, weight: 5, min: 1, max: 3 },
                    MobSpawnerDef { entity_type: 140, weight: 25, min: 8, max: 8 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:lush_caves",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 95, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 3,
                entries: &[
                    MobSpawnerDef { entity_type: 7, weight: 10, min: 4, max: 6 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
            MobCategorySpawns {
                category: 6,
                entries: &[
                    MobSpawnerDef { entity_type: 140, weight: 25, min: 8, max: 8 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:mangrove_swamp",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 95, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 70, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 120, weight: 1, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 16, weight: 30, min: 4, max: 4 },
                ],
            },
            MobCategorySpawns {
                category: 1,
                entries: &[
                    MobSpawnerDef { entity_type: 56, weight: 10, min: 2, max: 5 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
            MobCategorySpawns {
                category: 6,
                entries: &[
                    MobSpawnerDef { entity_type: 140, weight: 25, min: 8, max: 8 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:meadow",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 95, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 1,
                entries: &[
                    MobSpawnerDef { entity_type: 37, weight: 1, min: 1, max: 2 },
                    MobSpawnerDef { entity_type: 111, weight: 2, min: 2, max: 6 },
                    MobSpawnerDef { entity_type: 114, weight: 2, min: 2, max: 4 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:mushroom_fields",
        categories: &[
            MobCategorySpawns {
                category: 1,
                entries: &[
                    MobSpawnerDef { entity_type: 87, weight: 8, min: 4, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:nether_wastes",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 58, weight: 50, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 158, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 81, weight: 2, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 1, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 102, weight: 15, min: 4, max: 4 },
                ],
            },
            MobCategorySpawns {
                category: 1,
                entries: &[
                    MobSpawnerDef { entity_type: 132, weight: 60, min: 1, max: 2 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:ocean",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 95, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 39, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
            MobCategorySpawns {
                category: 5,
                entries: &[
                    MobSpawnerDef { entity_type: 130, weight: 1, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 36, weight: 1, min: 1, max: 2 },
                    MobSpawnerDef { entity_type: 89, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 6,
                entries: &[
                    MobSpawnerDef { entity_type: 27, weight: 10, min: 3, max: 6 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:old_growth_birch_forest",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 95, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 1,
                entries: &[
                    MobSpawnerDef { entity_type: 114, weight: 12, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 101, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 26, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 30, weight: 8, min: 4, max: 4 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:old_growth_pine_taiga",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 25, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 1,
                entries: &[
                    MobSpawnerDef { entity_type: 114, weight: 12, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 101, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 26, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 30, weight: 8, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 152, weight: 8, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 111, weight: 4, min: 2, max: 3 },
                    MobSpawnerDef { entity_type: 55, weight: 8, min: 2, max: 4 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:old_growth_spruce_taiga",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 95, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 1,
                entries: &[
                    MobSpawnerDef { entity_type: 114, weight: 12, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 101, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 26, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 30, weight: 8, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 152, weight: 8, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 111, weight: 4, min: 2, max: 3 },
                    MobSpawnerDef { entity_type: 55, weight: 8, min: 2, max: 4 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:pale_garden",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 95, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:plains",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 90, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 155, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 1,
                entries: &[
                    MobSpawnerDef { entity_type: 114, weight: 12, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 101, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 26, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 30, weight: 8, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 67, weight: 5, min: 2, max: 6 },
                    MobSpawnerDef { entity_type: 37, weight: 1, min: 1, max: 3 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:river",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 95, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 39, weight: 100, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
            MobCategorySpawns {
                category: 5,
                entries: &[
                    MobSpawnerDef { entity_type: 130, weight: 2, min: 1, max: 4 },
                ],
            },
            MobCategorySpawns {
                category: 6,
                entries: &[
                    MobSpawnerDef { entity_type: 113, weight: 5, min: 1, max: 5 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:savanna",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 90, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 155, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 1,
                entries: &[
                    MobSpawnerDef { entity_type: 114, weight: 12, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 101, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 26, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 30, weight: 8, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 67, weight: 1, min: 2, max: 6 },
                    MobSpawnerDef { entity_type: 37, weight: 1, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 4, weight: 10, min: 2, max: 3 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: true,
    },
    BiomeMobSpawns {
        name: "minecraft:savanna_plateau",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 90, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 155, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 1,
                entries: &[
                    MobSpawnerDef { entity_type: 114, weight: 12, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 101, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 26, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 30, weight: 8, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 67, weight: 1, min: 2, max: 6 },
                    MobSpawnerDef { entity_type: 37, weight: 1, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 4, weight: 10, min: 2, max: 3 },
                    MobSpawnerDef { entity_type: 79, weight: 8, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 152, weight: 8, min: 4, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: true,
    },
    BiomeMobSpawns {
        name: "minecraft:small_end_islands",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 4, max: 4 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:snowy_beach",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 95, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:snowy_plains",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 90, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 155, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 20, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 131, weight: 80, min: 4, max: 4 },
                ],
            },
            MobCategorySpawns {
                category: 1,
                entries: &[
                    MobSpawnerDef { entity_type: 111, weight: 10, min: 2, max: 3 },
                    MobSpawnerDef { entity_type: 105, weight: 1, min: 1, max: 2 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3d8f5c29),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:snowy_slopes",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 95, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 1,
                entries: &[
                    MobSpawnerDef { entity_type: 111, weight: 4, min: 2, max: 3 },
                    MobSpawnerDef { entity_type: 63, weight: 5, min: 1, max: 3 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:snowy_taiga",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 95, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 1,
                entries: &[
                    MobSpawnerDef { entity_type: 114, weight: 12, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 101, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 26, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 30, weight: 8, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 152, weight: 8, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 111, weight: 4, min: 2, max: 3 },
                    MobSpawnerDef { entity_type: 55, weight: 8, min: 2, max: 4 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:soul_sand_valley",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 118, weight: 20, min: 5, max: 5 },
                    MobSpawnerDef { entity_type: 58, weight: 50, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 1, min: 4, max: 4 },
                ],
            },
            MobCategorySpawns {
                category: 1,
                entries: &[
                    MobSpawnerDef { entity_type: 132, weight: 60, min: 1, max: 2 },
                ],
            },
        ],
        costs: &[
            MobSpawnCostDef { entity_type: 42, charge: 0.7, energy_budget: 0.15 },
            MobSpawnCostDef { entity_type: 58, charge: 0.7, energy_budget: 0.15 },
            MobSpawnCostDef { entity_type: 118, charge: 0.7, energy_budget: 0.15 },
            MobSpawnCostDef { entity_type: 132, charge: 0.7, energy_budget: 0.15 },
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:sparse_jungle",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 95, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 1,
                entries: &[
                    MobSpawnerDef { entity_type: 114, weight: 12, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 101, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 26, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 30, weight: 8, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 26, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 152, weight: 8, min: 2, max: 4 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:stony_peaks",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 95, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:stony_shore",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 95, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:sulfur_caves",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 133, weight: 100, min: 2, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 50, min: 2, max: 2 },
                    MobSpawnerDef { entity_type: 118, weight: 50, min: 2, max: 2 },
                    MobSpawnerDef { entity_type: 120, weight: 25, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 22, weight: 20, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 154, weight: 50, min: 2, max: 2 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 148, weight: 1, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:sunflower_plains",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 90, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 155, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 1,
                entries: &[
                    MobSpawnerDef { entity_type: 114, weight: 12, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 101, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 26, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 30, weight: 8, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 67, weight: 5, min: 2, max: 6 },
                    MobSpawnerDef { entity_type: 37, weight: 1, min: 1, max: 3 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:swamp",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 95, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 70, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 120, weight: 1, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 16, weight: 30, min: 4, max: 4 },
                ],
            },
            MobCategorySpawns {
                category: 1,
                entries: &[
                    MobSpawnerDef { entity_type: 114, weight: 12, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 101, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 26, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 30, weight: 8, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 56, weight: 10, min: 2, max: 5 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:taiga",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 95, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 1,
                entries: &[
                    MobSpawnerDef { entity_type: 114, weight: 12, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 101, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 26, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 30, weight: 8, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 152, weight: 8, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 111, weight: 4, min: 2, max: 3 },
                    MobSpawnerDef { entity_type: 55, weight: 8, min: 2, max: 4 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:the_end",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 4, max: 4 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:the_void",
        categories: &[
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:warm_ocean",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 39, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 95, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
            MobCategorySpawns {
                category: 5,
                entries: &[
                    MobSpawnerDef { entity_type: 89, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 130, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 36, weight: 2, min: 1, max: 2 },
                ],
            },
            MobCategorySpawns {
                category: 6,
                entries: &[
                    MobSpawnerDef { entity_type: 110, weight: 15, min: 1, max: 3 },
                    MobSpawnerDef { entity_type: 140, weight: 25, min: 8, max: 8 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:warped_forest",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 42, weight: 1, min: 4, max: 4 },
                ],
            },
            MobCategorySpawns {
                category: 1,
                entries: &[
                    MobSpawnerDef { entity_type: 132, weight: 60, min: 1, max: 2 },
                ],
            },
        ],
        costs: &[
            MobSpawnCostDef { entity_type: 42, charge: 1.0, energy_budget: 0.12 },
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:windswept_forest",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 95, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 1,
                entries: &[
                    MobSpawnerDef { entity_type: 114, weight: 12, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 101, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 26, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 30, weight: 8, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 79, weight: 5, min: 4, max: 6 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:windswept_gravelly_hills",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 95, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 1,
                entries: &[
                    MobSpawnerDef { entity_type: 114, weight: 12, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 101, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 26, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 30, weight: 8, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 79, weight: 5, min: 4, max: 6 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:windswept_hills",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 95, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 1,
                entries: &[
                    MobSpawnerDef { entity_type: 114, weight: 12, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 101, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 26, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 30, weight: 8, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 79, weight: 5, min: 4, max: 6 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: false,
    },
    BiomeMobSpawns {
        name: "minecraft:windswept_savanna",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 90, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 155, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 1,
                entries: &[
                    MobSpawnerDef { entity_type: 114, weight: 12, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 101, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 26, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 30, weight: 8, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 67, weight: 1, min: 2, max: 6 },
                    MobSpawnerDef { entity_type: 37, weight: 1, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 4, weight: 10, min: 2, max: 3 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3dcccccd),
        snow_golem_melts: true,
    },
    BiomeMobSpawns {
        name: "minecraft:wooded_badlands",
        categories: &[
            MobCategorySpawns {
                category: 0,
                entries: &[
                    MobSpawnerDef { entity_type: 127, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 154, weight: 95, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 157, weight: 5, min: 1, max: 1 },
                    MobSpawnerDef { entity_type: 118, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 32, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 120, weight: 100, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 42, weight: 10, min: 1, max: 4 },
                    MobSpawnerDef { entity_type: 148, weight: 5, min: 1, max: 1 },
                ],
            },
            MobCategorySpawns {
                category: 1,
                entries: &[
                    MobSpawnerDef { entity_type: 114, weight: 12, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 101, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 26, weight: 10, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 30, weight: 8, min: 4, max: 4 },
                    MobSpawnerDef { entity_type: 4, weight: 6, min: 1, max: 2 },
                    MobSpawnerDef { entity_type: 152, weight: 2, min: 4, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 2,
                entries: &[
                    MobSpawnerDef { entity_type: 10, weight: 10, min: 8, max: 8 },
                ],
            },
            MobCategorySpawns {
                category: 4,
                entries: &[
                    MobSpawnerDef { entity_type: 62, weight: 10, min: 4, max: 6 },
                ],
            },
        ],
        costs: &[
        ],
        creature_spawn_probability: f32::from_bits(0x3d23d70a),
        snow_golem_melts: true,
    },
];

/// `SNOW_GOLEM_MELTS`，下标是 `minecraft:worldgen/biome` 的注册表 ID。
#[rustfmt::skip]
pub static SNOW_GOLEM_MELTS: &[bool] = &[
    true, // minecraft:badlands
    false, // minecraft:bamboo_jungle
    false, // minecraft:basalt_deltas
    false, // minecraft:beach
    false, // minecraft:birch_forest
    false, // minecraft:cherry_grove
    false, // minecraft:cold_ocean
    false, // minecraft:crimson_forest
    false, // minecraft:dappled_forest
    false, // minecraft:dark_forest
    false, // minecraft:deep_cold_ocean
    false, // minecraft:deep_dark
    false, // minecraft:deep_frozen_ocean
    false, // minecraft:deep_lukewarm_ocean
    false, // minecraft:deep_ocean
    true, // minecraft:desert
    false, // minecraft:dripstone_caves
    false, // minecraft:end_barrens
    false, // minecraft:end_highlands
    false, // minecraft:end_midlands
    true, // minecraft:eroded_badlands
    false, // minecraft:flower_forest
    false, // minecraft:forest
    false, // minecraft:frozen_ocean
    false, // minecraft:frozen_peaks
    false, // minecraft:frozen_river
    false, // minecraft:grove
    false, // minecraft:ice_spikes
    false, // minecraft:jagged_peaks
    false, // minecraft:jungle
    false, // minecraft:lukewarm_ocean
    false, // minecraft:lush_caves
    false, // minecraft:mangrove_swamp
    false, // minecraft:meadow
    false, // minecraft:mushroom_fields
    false, // minecraft:nether_wastes
    false, // minecraft:ocean
    false, // minecraft:old_growth_birch_forest
    false, // minecraft:old_growth_pine_taiga
    false, // minecraft:old_growth_spruce_taiga
    false, // minecraft:pale_garden
    false, // minecraft:plains
    false, // minecraft:river
    true, // minecraft:savanna
    true, // minecraft:savanna_plateau
    false, // minecraft:small_end_islands
    false, // minecraft:snowy_beach
    false, // minecraft:snowy_plains
    false, // minecraft:snowy_slopes
    false, // minecraft:snowy_taiga
    false, // minecraft:soul_sand_valley
    false, // minecraft:sparse_jungle
    false, // minecraft:stony_peaks
    false, // minecraft:stony_shore
    false, // minecraft:sulfur_caves
    false, // minecraft:sunflower_plains
    false, // minecraft:swamp
    false, // minecraft:taiga
    false, // minecraft:the_end
    false, // minecraft:the_void
    false, // minecraft:warm_ocean
    false, // minecraft:warped_forest
    false, // minecraft:windswept_forest
    false, // minecraft:windswept_gravelly_hills
    false, // minecraft:windswept_hills
    true, // minecraft:windswept_savanna
    true, // minecraft:wooded_badlands
];

/// 官方 `StructureSpawnOverride.BoundingBoxType`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructureSpawnOverrideBox {
    /// `STRUCTURE`：整个起点的包围盒。
    Structure,
    /// `PIECE`：某个结构件的包围盒。
    Piece,
}

/// 一项结构刷怪覆盖。
#[derive(Debug, Clone, Copy)]
pub struct StructureSpawnOverride {
    pub structure: &'static str,
    pub bounding: StructureSpawnOverrideBox,
    /// `MobCategory` 的判别值。
    pub category: u8,
    pub spawns: &'static [MobSpawnerDef],
}

/// `data/minecraft/worldgen/structure/*.json` 的 `spawn_overrides`，按结构名和类别排序。
#[rustfmt::skip]
pub static STRUCTURE_SPAWN_OVERRIDES: &[StructureSpawnOverride] = &[
    // minecraft:ancient_city / monster
    StructureSpawnOverride { structure: "minecraft:ancient_city", bounding: StructureSpawnOverrideBox::Structure, category: 0, spawns: SPAWN_OVERRIDE_LIST_0 },
    // minecraft:ancient_city / creature
    StructureSpawnOverride { structure: "minecraft:ancient_city", bounding: StructureSpawnOverrideBox::Structure, category: 1, spawns: SPAWN_OVERRIDE_LIST_1 },
    // minecraft:ancient_city / ambient
    StructureSpawnOverride { structure: "minecraft:ancient_city", bounding: StructureSpawnOverrideBox::Structure, category: 2, spawns: SPAWN_OVERRIDE_LIST_2 },
    // minecraft:ancient_city / axolotls
    StructureSpawnOverride { structure: "minecraft:ancient_city", bounding: StructureSpawnOverrideBox::Structure, category: 3, spawns: SPAWN_OVERRIDE_LIST_3 },
    // minecraft:ancient_city / underground_water_creature
    StructureSpawnOverride { structure: "minecraft:ancient_city", bounding: StructureSpawnOverrideBox::Structure, category: 4, spawns: SPAWN_OVERRIDE_LIST_4 },
    // minecraft:ancient_city / water_creature
    StructureSpawnOverride { structure: "minecraft:ancient_city", bounding: StructureSpawnOverrideBox::Structure, category: 5, spawns: SPAWN_OVERRIDE_LIST_5 },
    // minecraft:ancient_city / water_ambient
    StructureSpawnOverride { structure: "minecraft:ancient_city", bounding: StructureSpawnOverrideBox::Structure, category: 6, spawns: SPAWN_OVERRIDE_LIST_6 },
    // minecraft:ancient_city / misc
    StructureSpawnOverride { structure: "minecraft:ancient_city", bounding: StructureSpawnOverrideBox::Structure, category: 7, spawns: SPAWN_OVERRIDE_LIST_7 },
    // minecraft:fortress / monster
    StructureSpawnOverride { structure: "minecraft:fortress", bounding: StructureSpawnOverrideBox::Piece, category: 0, spawns: SPAWN_OVERRIDE_LIST_8 },
    // minecraft:monument / monster
    StructureSpawnOverride { structure: "minecraft:monument", bounding: StructureSpawnOverrideBox::Structure, category: 0, spawns: SPAWN_OVERRIDE_LIST_9 },
    // minecraft:monument / axolotls
    StructureSpawnOverride { structure: "minecraft:monument", bounding: StructureSpawnOverrideBox::Structure, category: 3, spawns: SPAWN_OVERRIDE_LIST_10 },
    // minecraft:monument / underground_water_creature
    StructureSpawnOverride { structure: "minecraft:monument", bounding: StructureSpawnOverrideBox::Structure, category: 4, spawns: SPAWN_OVERRIDE_LIST_11 },
    // minecraft:pillager_outpost / monster
    StructureSpawnOverride { structure: "minecraft:pillager_outpost", bounding: StructureSpawnOverrideBox::Structure, category: 0, spawns: SPAWN_OVERRIDE_LIST_12 },
    // minecraft:swamp_hut / monster
    StructureSpawnOverride { structure: "minecraft:swamp_hut", bounding: StructureSpawnOverrideBox::Piece, category: 0, spawns: SPAWN_OVERRIDE_LIST_13 },
    // minecraft:swamp_hut / creature
    StructureSpawnOverride { structure: "minecraft:swamp_hut", bounding: StructureSpawnOverrideBox::Piece, category: 1, spawns: SPAWN_OVERRIDE_LIST_14 },
    // minecraft:trial_chambers / monster
    StructureSpawnOverride { structure: "minecraft:trial_chambers", bounding: StructureSpawnOverrideBox::Piece, category: 0, spawns: SPAWN_OVERRIDE_LIST_15 },
    // minecraft:trial_chambers / creature
    StructureSpawnOverride { structure: "minecraft:trial_chambers", bounding: StructureSpawnOverrideBox::Piece, category: 1, spawns: SPAWN_OVERRIDE_LIST_16 },
    // minecraft:trial_chambers / ambient
    StructureSpawnOverride { structure: "minecraft:trial_chambers", bounding: StructureSpawnOverrideBox::Piece, category: 2, spawns: SPAWN_OVERRIDE_LIST_17 },
    // minecraft:trial_chambers / axolotls
    StructureSpawnOverride { structure: "minecraft:trial_chambers", bounding: StructureSpawnOverrideBox::Piece, category: 3, spawns: SPAWN_OVERRIDE_LIST_18 },
    // minecraft:trial_chambers / underground_water_creature
    StructureSpawnOverride { structure: "minecraft:trial_chambers", bounding: StructureSpawnOverrideBox::Piece, category: 4, spawns: SPAWN_OVERRIDE_LIST_19 },
    // minecraft:trial_chambers / water_creature
    StructureSpawnOverride { structure: "minecraft:trial_chambers", bounding: StructureSpawnOverrideBox::Piece, category: 5, spawns: SPAWN_OVERRIDE_LIST_20 },
    // minecraft:trial_chambers / water_ambient
    StructureSpawnOverride { structure: "minecraft:trial_chambers", bounding: StructureSpawnOverrideBox::Piece, category: 6, spawns: SPAWN_OVERRIDE_LIST_21 },
    // minecraft:trial_chambers / misc
    StructureSpawnOverride { structure: "minecraft:trial_chambers", bounding: StructureSpawnOverrideBox::Piece, category: 7, spawns: SPAWN_OVERRIDE_LIST_22 },
];

#[rustfmt::skip]
const SPAWN_OVERRIDE_LIST_0: &[MobSpawnerDef] = &[
];

#[rustfmt::skip]
const SPAWN_OVERRIDE_LIST_1: &[MobSpawnerDef] = &[
];

#[rustfmt::skip]
const SPAWN_OVERRIDE_LIST_2: &[MobSpawnerDef] = &[
];

#[rustfmt::skip]
const SPAWN_OVERRIDE_LIST_3: &[MobSpawnerDef] = &[
];

#[rustfmt::skip]
const SPAWN_OVERRIDE_LIST_4: &[MobSpawnerDef] = &[
];

#[rustfmt::skip]
const SPAWN_OVERRIDE_LIST_5: &[MobSpawnerDef] = &[
];

#[rustfmt::skip]
const SPAWN_OVERRIDE_LIST_6: &[MobSpawnerDef] = &[
];

#[rustfmt::skip]
const SPAWN_OVERRIDE_LIST_7: &[MobSpawnerDef] = &[
];

#[rustfmt::skip]
const SPAWN_OVERRIDE_LIST_8: &[MobSpawnerDef] = &[
    MobSpawnerDef { entity_type: 14, weight: 10, min: 2, max: 3 },
    MobSpawnerDef { entity_type: 158, weight: 5, min: 4, max: 4 },
    MobSpawnerDef { entity_type: 150, weight: 8, min: 5, max: 5 },
    MobSpawnerDef { entity_type: 118, weight: 2, min: 5, max: 5 },
    MobSpawnerDef { entity_type: 81, weight: 3, min: 4, max: 4 },
];

#[rustfmt::skip]
const SPAWN_OVERRIDE_LIST_9: &[MobSpawnerDef] = &[
    MobSpawnerDef { entity_type: 64, weight: 1, min: 2, max: 4 },
];

#[rustfmt::skip]
const SPAWN_OVERRIDE_LIST_10: &[MobSpawnerDef] = &[
];

#[rustfmt::skip]
const SPAWN_OVERRIDE_LIST_11: &[MobSpawnerDef] = &[
];

#[rustfmt::skip]
const SPAWN_OVERRIDE_LIST_12: &[MobSpawnerDef] = &[
    MobSpawnerDef { entity_type: 104, weight: 1, min: 1, max: 1 },
];

#[rustfmt::skip]
const SPAWN_OVERRIDE_LIST_13: &[MobSpawnerDef] = &[
    MobSpawnerDef { entity_type: 148, weight: 1, min: 1, max: 1 },
];

#[rustfmt::skip]
const SPAWN_OVERRIDE_LIST_14: &[MobSpawnerDef] = &[
    MobSpawnerDef { entity_type: 21, weight: 1, min: 1, max: 1 },
];

#[rustfmt::skip]
const SPAWN_OVERRIDE_LIST_15: &[MobSpawnerDef] = &[
];

#[rustfmt::skip]
const SPAWN_OVERRIDE_LIST_16: &[MobSpawnerDef] = &[
];

#[rustfmt::skip]
const SPAWN_OVERRIDE_LIST_17: &[MobSpawnerDef] = &[
];

#[rustfmt::skip]
const SPAWN_OVERRIDE_LIST_18: &[MobSpawnerDef] = &[
];

#[rustfmt::skip]
const SPAWN_OVERRIDE_LIST_19: &[MobSpawnerDef] = &[
];

#[rustfmt::skip]
const SPAWN_OVERRIDE_LIST_20: &[MobSpawnerDef] = &[
];

#[rustfmt::skip]
const SPAWN_OVERRIDE_LIST_21: &[MobSpawnerDef] = &[
];

#[rustfmt::skip]
const SPAWN_OVERRIDE_LIST_22: &[MobSpawnerDef] = &[
];
