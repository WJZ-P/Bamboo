//! 村民的静态数据：交易表、职业、品种、八卦和日程。
//!
//! 表本体由生成器写在 `generated/villager.rs`，这里只提供按名字 / ID 的查询。
//! 交易求值（按等级抽条目、价格浮动）留到期 5，这里只做名字到数据的映射。
//!
//! Villager static data: trade tables, professions, types, gossip and schedules. The
//! generated module holds the tables; this module only maps names and ids to them.

pub use crate::generated::villager::*;

/// 没有职业时的名字，官方 `VillagerProfession.NONE`。
pub const NONE_PROFESSION: &str = "minecraft:none";

/// 默认品种，官方 `VillagerData.DEFAULT_TYPE`。
pub const DEFAULT_TYPE: &str = "minecraft:plains";

/// 按注册表名字找品种 ID。
pub fn type_id(name: &str) -> Option<i32> {
    VILLAGER_TYPES
        .iter()
        .position(|entry| *entry == name)
        .map(|index| index as i32)
}

/// 按注册表 ID 取品种名字。
pub fn type_name(id: i32) -> Option<&'static str> {
    usize::try_from(id)
        .ok()
        .and_then(|index| VILLAGER_TYPES.get(index))
        .copied()
}

/// 按注册表名字找职业 ID。
pub fn profession_id(name: &str) -> Option<i32> {
    PROFESSIONS
        .iter()
        .position(|entry| entry.name == name)
        .map(|index| index as i32)
}

/// 按注册表 ID 取职业。
pub fn profession(id: i32) -> Option<&'static Profession> {
    usize::try_from(id)
        .ok()
        .and_then(|index| PROFESSIONS.get(index))
}

/// `VillagerType.byBiome`：群系注册表名字 → 品种名字，不在表里是平原。
pub fn type_name_for_biome(biome: &str) -> &'static str {
    TYPE_BY_BIOME
        .iter()
        .find(|(name, _)| *name == biome)
        .map_or(DEFAULT_TYPE, |(_, kind)| *kind)
}

/// `VillagerType.byBiome`：按群系注册表 ID 选品种 ID。
pub fn type_for_biome(biome: i32) -> i32 {
    let name = crate::registries::biome::name_of(biome).unwrap_or("");
    type_id(type_name_for_biome(name)).unwrap_or(0)
}

/// 一个 `trade_set` 展开后的条目名。
pub fn trade_set_entries(name: &str) -> Option<&'static [&'static str]> {
    TRADE_SET_ENTRIES
        .iter()
        .find(|(id, _)| *id == name)
        .map(|(_, entries)| *entries)
}

/// 一个 `trade_set` 文件的原文。
pub fn trade_set(name: &str) -> Option<&'static str> {
    TRADE_SETS
        .iter()
        .find(|(id, _)| *id == name)
        .map(|(_, raw)| *raw)
}

/// 一个 `villager_trade` 文件的原文。
pub fn trade(name: &str) -> Option<&'static str> {
    VILLAGER_TRADES
        .iter()
        .find(|(id, _)| *id == name)
        .map(|(_, raw)| *raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registries::biome;

    /// 官方 26.3 数据包的数量：68 个 `trade_set`、391 个 `villager_trade`、73 个标签。
    #[test]
    fn table_sizes_match_the_official_pack() {
        assert_eq!(TRADE_SETS.len(), 68);
        assert_eq!(VILLAGER_TRADES.len(), 391);
        assert_eq!(VILLAGER_TRADE_TAGS.len(), 73);
        assert_eq!(TRADE_SET_ENTRIES.len(), 68);
    }

    /// 每个 `trade_set` 的标签都展开成非空条目表，引用到的交易都存在；标签原文也都
    /// 是官方文件。
    #[test]
    fn every_trade_set_entry_resolves_to_a_trade() {
        for &(id, entries) in TRADE_SET_ENTRIES {
            assert!(!entries.is_empty(), "{id} 展开成空表");
            assert!(trade_set(id).is_some(), "{id} 没有原文");
            for entry in entries {
                assert!(trade(entry).is_some(), "{id} 引用了不存在的交易 {entry}");
            }
            assert_eq!(
                entries.len(),
                entries
                    .iter()
                    .collect::<std::collections::BTreeSet<_>>()
                    .len(),
                "{id} 有重复条目"
            );
        }
    }

    #[test]
    fn professions_match_the_registry() {
        assert_eq!(PROFESSIONS.len(), 15);
        assert_eq!(PROFESSIONS[0].name, NONE_PROFESSION);
        assert_eq!(profession_id(NONE_PROFESSION), Some(0));
        let armorer = profession(profession_id("minecraft:armorer").unwrap()).unwrap();
        assert_eq!(armorer.held_job_site, ["minecraft:armorer"]);
        assert_eq!(armorer.acquirable_job_site, ["minecraft:armorer"]);
        assert_eq!(
            armorer.trade_sets_by_level,
            [
                "minecraft:armorer/level_1",
                "minecraft:armorer/level_2",
                "minecraft:armorer/level_3",
                "minecraft:armorer/level_4",
                "minecraft:armorer/level_5",
            ]
        );
        // `none` 没有任何工作站，也没有交易表。
        assert!(PROFESSIONS[0].held_job_site.is_empty());
        assert!(PROFESSIONS[0].acquirable_job_site.len() == 13);
        assert!(
            PROFESSIONS[0]
                .trade_sets_by_level
                .iter()
                .all(|id| id.is_empty())
        );
        for profession in PROFESSIONS {
            for id in profession.trade_sets_by_level {
                if !id.is_empty() {
                    assert!(
                        trade_set(id).is_some(),
                        "{} 的交易表 {id} 不存在",
                        profession.name
                    );
                }
            }
        }
    }

    #[test]
    fn villager_types_and_biomes_match_the_official_numbers() {
        assert_eq!(VILLAGER_TYPES.len(), 7);
        assert_eq!(TYPE_BY_BIOME.len(), 29);
        assert_eq!(type_id(DEFAULT_TYPE), Some(2));
        for &(biome_name, kind) in TYPE_BY_BIOME {
            assert_eq!(type_name_for_biome(biome_name), kind);
        }
        assert_eq!(type_name_for_biome("minecraft:not_a_biome"), DEFAULT_TYPE);
        assert_eq!(
            type_name_for_biome("minecraft:badlands"),
            "minecraft:desert"
        );
        assert_eq!(type_name_for_biome("minecraft:jungle"), "minecraft:jungle");
        let id = biome::by_name("minecraft:swamp").unwrap();
        assert_eq!(type_for_biome(id), type_id("minecraft:swamp").unwrap());
    }

    #[test]
    fn gossip_tables_match_the_official_constants() {
        assert_eq!(
            GOSSIP_TYPES.map(|gossip| (
                gossip.name,
                gossip.weight,
                gossip.max,
                gossip.decay_per_day,
                gossip.decay_per_transfer
            )),
            [
                ("major_negative", -5, 100, 10, 10),
                ("minor_negative", -1, 200, 20, 20),
                ("minor_positive", 1, 25, 1, 5),
                ("major_positive", 5, 20, 0, 20),
                ("trading", 1, 25, 2, 20),
            ]
        );
        assert_eq!(REPUTATION_PER_EVENT, 25);
        assert_eq!(REPUTATION_PER_EVERLASTING, 20);
        assert_eq!(REPUTATION_PER_TRADE, 2);
    }

    /// 三个元数据 index、序列化器 ID、尺寸和等级阈值都来自官方字节码。
    #[test]
    fn villager_metadata_and_dimensions_match_the_official_values() {
        assert_eq!(UNHAPPY_COUNTER_INDEX, 18);
        assert_eq!(VILLAGER_DATA_INDEX, 19);
        assert_eq!(VILLAGER_DATA_FINALIZED_INDEX, 20);
        assert_eq!(UNHAPPY_COUNTER_SERIALIZER, 1);
        assert_eq!(VILLAGER_DATA_SERIALIZER, 18);
        assert_eq!(VILLAGER_DATA_FINALIZED_SERIALIZER, 8);
        assert_eq!(VILLAGER_MOVEMENT_SPEED, 0.5);
        assert_eq!(
            (
                VILLAGER_WIDTH,
                VILLAGER_HEIGHT,
                VILLAGER_EYE_HEIGHT,
                BABY_VILLAGER_WIDTH,
                BABY_VILLAGER_HEIGHT,
                BABY_VILLAGER_EYE_HEIGHT
            ),
            (0.6, 1.95, 1.62, 0.49, 0.98, 0.63)
        );
        assert_eq!(MIN_VILLAGER_LEVEL, 1);
        assert_eq!(MAX_VILLAGER_LEVEL, 5);
        assert_eq!(NEXT_LEVEL_XP_THRESHOLDS, [0, 10, 70, 150, 250]);
    }

    #[test]
    fn schedule_keyframes_match_the_official_timeline() {
        assert_eq!(VILLAGER_SCHEDULE_PERIOD_TICKS, 24000);
        assert_eq!(
            VILLAGER_ACTIVITY_KEYFRAMES,
            [
                (10, "minecraft:idle"),
                (2000, "minecraft:work"),
                (9000, "minecraft:meet"),
                (11000, "minecraft:idle"),
                (12000, "minecraft:rest"),
            ]
        );
        assert_eq!(
            BABY_VILLAGER_ACTIVITY_KEYFRAMES,
            [
                (10, "minecraft:idle"),
                (3000, "minecraft:play"),
                (6000, "minecraft:idle"),
                (10000, "minecraft:play"),
                (12000, "minecraft:rest"),
            ]
        );
    }
}
