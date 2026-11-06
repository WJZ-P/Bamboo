//! 目标游戏版本的全部数据：协议号、数据包 ID、方块状态、注册表和标签。
//!
//! `generated` 目录由 `mc-bamboo-vanilla` 从官方服务端生成。
//! 版本相关的数字都集中在这里，其他 crate 只引用名字，升级版本时不用改逻辑代码。

#[rustfmt::skip]
mod generated;

pub use generated::{blocks, packets, registries, tags, version};

/// 客户端和服务端都认识的内容包。双方都有时，注册表内容就不需要通过网络传输。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnownPack {
    pub namespace: &'static str,
    pub id: &'static str,
    pub version: &'static str,
}

/// 需要同步给客户端的注册表。条目的位置就是它的数字 ID。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncedRegistry {
    pub name: &'static str,
    pub entries: &'static [&'static str],
}

/// 一个注册表下的所有标签。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TagGroup {
    pub registry: &'static str,
    pub tags: &'static [Tag],
}

/// 一个标签和它包含的条目 ID。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tag {
    pub name: &'static str,
    pub ids: &'static [i32],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_the_target_version() {
        assert_eq!(version::GAME_VERSION, "26.3");
        assert_eq!(version::PROTOCOL_VERSION, 777);
        assert_eq!(version::DATA_VERSION, 5023);
    }

    #[test]
    fn packet_ids_match_the_official_report() {
        assert_eq!(packets::login::clientbound::LOGIN_FINISHED, 2);
        assert_eq!(packets::configuration::clientbound::REGISTRY_DATA, 7);
        assert_eq!(packets::play::clientbound::LEVEL_CHUNK_WITH_LIGHT, 46);
    }

    #[test]
    fn air_is_block_state_zero() {
        assert_eq!(blocks::AIR, 0);
        const { assert!(blocks::GRASS_BLOCK < blocks::TOTAL_STATES) };
    }

    #[test]
    fn every_entry_list_is_filled() {
        assert!(!registries::SYNCED_REGISTRIES.is_empty());
        assert!(
            registries::SYNCED_REGISTRIES
                .iter()
                .all(|registry| !registry.name.is_empty())
        );
        const { assert!(registries::biome::TOTAL > 0) };
        assert_eq!(
            registries::KNOWN_PACKS,
            [KnownPack {
                namespace: "minecraft",
                id: "core",
                version: "26.3",
            }]
        );
    }

    #[test]
    fn static_and_dynamic_registries_have_tags() {
        let has_group = |name| tags::TAG_GROUPS.iter().any(|group| group.registry == name);
        assert!(has_group("minecraft:block"));
        assert!(has_group("minecraft:item"));
        assert!(has_group("minecraft:worldgen/biome"));
    }
}
