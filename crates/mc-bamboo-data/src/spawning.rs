//! 自然刷怪用的数据表：`MobCategory` 的上限和每个群系的刷怪设置。
//!
//! 表由 `mc-bamboo-vanilla` 从官方 `MobCategory` 和 `Biome.getMobSettings()` 导出，
//! 这个模块只给出按类别、按群系查表的入口。
//!
//! Data tables for natural spawning: the `MobCategory` caps and each biome's spawn
//! settings, exported from the official `MobCategory` and `Biome.getMobSettings()`.
//! This module only provides the lookups.

use crate::generated::mob_spawns::{BiomeMobSpawns, MobCategoryDef, MobCategorySpawns};
use crate::generated::registries;

pub use crate::generated::mob_spawns::{
    BIOME_MOB_SPAWNS, MOB_CATEGORIES, MobSpawnCostDef, MobSpawnerDef,
};

/// 官方 `MobCategory`，判别值就是 `MobCategory.values()` 的下标。
///
/// The official `MobCategory`; the discriminant is the `values()` index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MobCategory {
    Monster = 0,
    Creature = 1,
    Ambient = 2,
    Axolotls = 3,
    UndergroundWaterCreature = 4,
    WaterCreature = 5,
    WaterAmbient = 6,
    Misc = 7,
}

impl MobCategory {
    /// `MobCategory.values()`。
    pub const ALL: [Self; 8] = [
        Self::Monster,
        Self::Creature,
        Self::Ambient,
        Self::Axolotls,
        Self::UndergroundWaterCreature,
        Self::WaterCreature,
        Self::WaterAmbient,
        Self::Misc,
    ];

    /// `NaturalSpawner.SPAWNING_CATEGORIES`：除 `MISC` 之外的七类。
    pub const SPAWNING: [Self; 7] = [
        Self::Monster,
        Self::Creature,
        Self::Ambient,
        Self::Axolotls,
        Self::UndergroundWaterCreature,
        Self::WaterCreature,
        Self::WaterAmbient,
    ];

    pub const fn index(self) -> usize {
        self as usize
    }

    /// 这一类在官方表里的常量。
    pub fn facts(self) -> &'static MobCategoryDef {
        &MOB_CATEGORIES[self.index()]
    }
}

/// 一个群系的刷怪设置；群系没有条目时返回空表。
///
/// A biome's spawn settings; an unknown biome gets the empty table.
#[derive(Debug, Clone, Copy, Default)]
pub struct MobSpawnSettings {
    pub name: &'static str,
    pub categories: &'static [MobCategorySpawns],
    pub costs: &'static [MobSpawnCostDef],
    /// 环境属性 `CREATURE_WORLD_GEN_SPAWN_PROBABILITY`：生成期刷动物的概率。
    ///
    /// The `CREATURE_WORLD_GEN_SPAWN_PROBABILITY` attribute: the world-generation
    /// creature odds.
    pub creature_spawn_probability: f32,
}

/// `CREATURE_WORLD_GEN_SPAWN_PROBABILITY` 的缺省值 `0.1F`。
///
/// The `0.1F` default of `CREATURE_WORLD_GEN_SPAWN_PROBABILITY`.
pub const DEFAULT_CREATURE_SPAWN_PROBABILITY: f32 = 0.1;

/// 空表：`MobSpawnSettings.EMPTY`。
pub const EMPTY: MobSpawnSettings = MobSpawnSettings {
    name: "",
    categories: &[],
    costs: &[],
    creature_spawn_probability: DEFAULT_CREATURE_SPAWN_PROBABILITY,
};

impl MobSpawnSettings {
    /// `getMobsToSpawn(category)`：这一类没有条目时返回空表。
    pub fn mobs_to_spawn(&self, category: MobCategory) -> &'static [MobSpawnerDef] {
        self.categories
            .iter()
            .find(|row| row.category == category.index() as u8)
            .map_or(&[], |row| row.entries)
    }

    /// `getMobSpawnCost(type)`。
    pub fn spawn_cost(&self, entity_type: i32) -> Option<MobSpawnCostDef> {
        self.costs
            .iter()
            .find(|cost| cost.entity_type == entity_type)
            .copied()
    }
}

/// 按群系注册名查刷怪设置。
pub fn by_biome_name(name: &str) -> Option<&'static BiomeMobSpawns> {
    BIOME_MOB_SPAWNS.iter().find(|biome| biome.name == name)
}

/// 按群系注册表 ID 查刷怪设置，超过已知范围或没有条目时给空表。
pub fn for_biome_id(id: i32) -> MobSpawnSettings {
    let Some(name) = registries::biome::name_of(id) else {
        return EMPTY;
    };
    by_biome_name(name).map_or(EMPTY, |biome| MobSpawnSettings {
        name: biome.name,
        categories: biome.categories,
        costs: biome.costs,
        creature_spawn_probability: biome.creature_spawn_probability,
    })
}

/// `StructureSpawnOverride.BoundingBoxType`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnOverrideBox {
    /// `STRUCTURE`：整个 `StructureStart` 的包围盒。
    ///
    /// `STRUCTURE`: the whole `StructureStart` bounding box.
    Structure,
    /// `PIECE`：位置要落在某个结构件的包围盒里。
    ///
    /// `PIECE`: the position must fall inside a structure piece's box.
    Piece,
}

/// 官方 `Structure.spawnOverrides()` 的一项：某结构在某一类里替换群系表的加权列表。
/// `Structures.bootstrap` 里前哨站、女巫小屋、海底神殿、下界要塞等都注册了覆盖；
/// 这里先收会刷已建模生物的两种，其余结构以后按同一张表追加即可。
///
/// One entry of vanilla `Structure.spawnOverrides()`: a structure replaces the biome
/// table for one category. `Structures.bootstrap` registers overrides for outposts,
/// swamp huts, ocean monuments, fortresses and more; the two that spawn modelled
/// mobs are here, and future structures append to the same table.
#[derive(Debug, Clone, Copy)]
pub struct StructureSpawnOverride {
    /// 结构的注册名，例如 `minecraft:pillager_outpost`。
    ///
    /// The structure's registry name, e.g. `minecraft:pillager_outpost`.
    pub structure: &'static str,
    pub bounding: SpawnOverrideBox,
    pub category: MobCategory,
    /// `StructureSpawnOverride.spawns()` 的加权表。
    ///
    /// The weighted `StructureSpawnOverride.spawns()` list.
    pub spawns: &'static [MobSpawnerDef],
}

/// `WeightedList.of(SpawnerData(ENTITY, ConstantInt(1)))`：默认权重 1、数量恒 1。
const fn one(entity_type: i32) -> MobSpawnerDef {
    MobSpawnerDef {
        entity_type,
        weight: 1,
        min: 1,
        max: 1,
    }
}

/// 前哨站 `MONSTER` 的覆盖表（`STRUCTURE` 盒，只有掠夺者）。
const OUTPOST_MONSTERS: &[MobSpawnerDef] = &[one(registries::entity_type::PILLAGER)];
/// 女巫小屋 `MONSTER` / `CREATURE` 的覆盖表（`PIECE` 盒，女巫 / 猫）。
const HUT_MONSTERS: &[MobSpawnerDef] = &[one(registries::entity_type::WITCH)];
const HUT_CREATURES: &[MobSpawnerDef] = &[one(registries::entity_type::CAT)];

/// 所有结构刷怪覆盖，按 `structure` 和 `category` 查。[`structure_spawn_override`]
/// 是唯一的入口，新增结构只要往这张表追加一行。
///
/// Every structure spawn override, looked up by `structure` and `category`.
/// [`structure_spawn_override`] is the single entry point; adding a structure is
/// one more row here.
pub const STRUCTURE_SPAWN_OVERRIDES: &[StructureSpawnOverride] = &[
    StructureSpawnOverride {
        structure: "minecraft:pillager_outpost",
        bounding: SpawnOverrideBox::Structure,
        category: MobCategory::Monster,
        spawns: OUTPOST_MONSTERS,
    },
    StructureSpawnOverride {
        structure: "minecraft:swamp_hut",
        bounding: SpawnOverrideBox::Piece,
        category: MobCategory::Monster,
        spawns: HUT_MONSTERS,
    },
    StructureSpawnOverride {
        structure: "minecraft:swamp_hut",
        bounding: SpawnOverrideBox::Piece,
        category: MobCategory::Creature,
        spawns: HUT_CREATURES,
    },
];

/// 按结构名和类别取覆盖表；没有覆盖的结构返回 `None`（调用方回退到群系表）。
///
/// The override list for a structure and category, or `None` when the structure has
/// no override (the caller then falls back to the biome table).
pub fn structure_spawn_override(
    structure: &str,
    category: MobCategory,
) -> Option<&'static [MobSpawnerDef]> {
    STRUCTURE_SPAWN_OVERRIDES
        .iter()
        .find(|row| row.structure == structure && row.category == category)
        .map(|row| row.spawns)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_match_the_official_order() {
        assert_eq!(MOB_CATEGORIES.len(), MobCategory::ALL.len());
        for (index, category) in MobCategory::ALL.into_iter().enumerate() {
            assert_eq!(category.index(), index);
            assert_eq!(category.facts().name, MOB_CATEGORIES[index].name);
        }
        assert_eq!(MobCategory::Monster.facts().max_instances, 70);
        assert_eq!(MobCategory::Creature.facts().max_instances, 10);
        assert!(MobCategory::Creature.facts().friendly);
        assert!(!MobCategory::Monster.facts().friendly);
        assert!(!MobCategory::Monster.facts().persistent);
        assert_eq!(MobCategory::Monster.facts().despawn_distance, 128);
        assert_eq!(MobCategory::Monster.facts().no_despawn_distance, 32);
        assert_eq!(MobCategory::WaterAmbient.facts().despawn_distance, 64);
        assert!(!MobCategory::SPAWNING.contains(&MobCategory::Misc));
    }

    #[test]
    fn plains_and_desert_tables_are_exported() {
        let plains = for_biome_id(registries::biome::PLAINS);
        assert_eq!(plains.name, "minecraft:plains");
        let creatures = plains.mobs_to_spawn(MobCategory::Creature);
        assert!(
            creatures
                .iter()
                .any(|entry| entry.entity_type == registries::entity_type::COW)
        );
        assert_eq!(plains.creature_spawn_probability, 0.1);
        assert!(
            BIOME_MOB_SPAWNS
                .iter()
                .any(|biome| biome.creature_spawn_probability < 0.1),
            "少数群系把生成期刷怪概率覆盖得更低"
        );
        let monsters = plains.mobs_to_spawn(MobCategory::Monster);
        assert!(
            monsters
                .iter()
                .any(|entry| entry.entity_type == registries::entity_type::ZOMBIE)
        );
        // 平原没有刷怪代价。
        assert!(plains.spawn_cost(registries::entity_type::ZOMBIE).is_none());

        // 灵魂沙峡谷给恶魂、末影人、骷髅和炽足兽记了代价。
        let nether = by_biome_name("minecraft:soul_sand_valley").expect("soul sand valley table");
        assert!(!nether.costs.is_empty());
        let cost = nether
            .costs
            .iter()
            .find(|cost| cost.entity_type == registries::entity_type::GHAST)
            .expect("ghast cost");
        assert!(cost.charge > 0.0);
        assert!(cost.energy_budget > 0.0);
    }

    #[test]
    fn unknown_biomes_get_the_empty_table() {
        assert_eq!(for_biome_id(-1).name, "");
        assert!(for_biome_id(-1).categories.is_empty());
        assert!(by_biome_name("minecraft:not_a_biome").is_none());
    }
}
