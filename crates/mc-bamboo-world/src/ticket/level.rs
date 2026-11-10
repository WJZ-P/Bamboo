//! 区块等级和 `TicketType`，照 26.3 的 `ChunkLevel`、`FullChunkStatus` 和 `TicketType`。

/// 实体 tick 的最高等级（`ChunkLevel.isEntityTicking` 的界）。
pub const ENTITY_TICKING_LEVEL: i32 = 31;
/// 方块 tick 的最高等级（`ChunkLevel.isBlockTicking` 的界）。
pub const BLOCK_TICKING_LEVEL: i32 = 32;
/// 完工区块的最高等级（`ChunkLevel.fullStatus` 的 `FULL` 界）。
pub const FULL_CHUNK_LEVEL: i32 = 33;
/// `FULL` 阶段的累计依赖半径（`ChunkLevel.RADIUS_AROUND_FULL_CHUNK`）。
pub const RADIUS_AROUND_FULL_CHUNK: i32 = 11;
/// 还要加载的最高等级：`33 + RADIUS_AROUND_FULL_CHUNK`。
pub const MAX_LEVEL: i32 = FULL_CHUNK_LEVEL + RADIUS_AROUND_FULL_CHUNK;
/// 没有票时的等级：`MAX_LEVEL + 1`。加载平面用它，模拟平面用 [`SIMULATION_MAX_LEVEL`]。
pub const UNLOADED_LEVEL: i32 = MAX_LEVEL + 1;
/// 模拟平面内部的无票等级（`SimulationChunkTracker.MAX_LEVEL`）。
pub const SIMULATION_MAX_LEVEL: i32 = 33;
/// `DistanceManager.PLAYER_TICKET_LEVEL`，也是 `ChunkMap.FORCED_TICKET_LEVEL`。
pub const PLAYER_TICKET_LEVEL: i32 = ENTITY_TICKING_LEVEL;
/// `DistanceManager` 的默认模拟距离。
pub const DEFAULT_SIMULATION_DISTANCE: i32 = 10;
/// 官方视距的合法范围（`ChunkMap.MIN_VIEW_DISTANCE` / `MAX_VIEW_DISTANCE`）。
pub const MIN_VIEW_DISTANCE: i32 = 2;
/// 官方视距的上界。
pub const MAX_VIEW_DISTANCE: i32 = 32;

/// 区块在加载平面上的可用程度（`FullChunkStatus`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FullChunkStatus {
    /// 等级大于 33：只到某个生成阶段，或者完全没票。
    Inaccessible,
    /// 等级 33：完工、已加载、不 tick。
    Full,
    /// 等级 32：方块 tick。
    BlockTicking,
    /// 等级 <= 31：实体 tick。
    EntityTicking,
}

impl FullChunkStatus {
    /// `ChunkLevel.fullStatus`。
    pub const fn of_level(level: i32) -> Self {
        if level <= ENTITY_TICKING_LEVEL {
            Self::EntityTicking
        } else if level <= BLOCK_TICKING_LEVEL {
            Self::BlockTicking
        } else if level <= FULL_CHUNK_LEVEL {
            Self::Full
        } else {
            Self::Inaccessible
        }
    }

    /// `ChunkLevel.byStatus(FullChunkStatus)`。
    pub const fn level(self) -> i32 {
        match self {
            Self::Inaccessible => UNLOADED_LEVEL,
            Self::Full => FULL_CHUNK_LEVEL,
            Self::BlockTicking => BLOCK_TICKING_LEVEL,
            Self::EntityTicking => ENTITY_TICKING_LEVEL,
        }
    }

    /// `FullChunkStatus.isOrAfter`。
    pub const fn is_or_after(self, other: Self) -> bool {
        (self as i32) >= (other as i32)
    }
}

/// 26.3 的生成阶段（`ChunkStatus`），按 `ChunkStatus.getStatusList()` 的顺序编号。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ChunkStatus {
    Empty,
    StructureStarts,
    StructureReferences,
    Biomes,
    Terrain,
    Features,
    InitializeLight,
    Light,
    Spawn,
    Full,
}

impl ChunkStatus {
    /// 全部阶段，按官方注册顺序。
    pub const ALL: [Self; 10] = [
        Self::Empty,
        Self::StructureStarts,
        Self::StructureReferences,
        Self::Biomes,
        Self::Terrain,
        Self::Features,
        Self::InitializeLight,
        Self::Light,
        Self::Spawn,
        Self::Full,
    ];

    /// 官方名字，不带命名空间。
    pub const fn name(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::StructureStarts => "structure_starts",
            Self::StructureReferences => "structure_references",
            Self::Biomes => "biomes",
            Self::Terrain => "terrain",
            Self::Features => "features",
            Self::InitializeLight => "initialize_light",
            Self::Light => "light",
            Self::Spawn => "spawn",
            Self::Full => "full",
        }
    }

    /// `ChunkLevel.byStatus(ChunkStatus)`：`FULL` 阶段累计依赖图里，该阶段需要的最大半径加 33。
    pub const fn level(self) -> i32 {
        match self {
            Self::Empty | Self::StructureStarts => 44,
            Self::StructureReferences | Self::Biomes => 36,
            Self::Terrain => 35,
            Self::Features | Self::InitializeLight => 34,
            Self::Light | Self::Spawn | Self::Full => 33,
        }
    }
}

/// `ChunkLevel.generationStatus`：等级对应的目标生成阶段。
///
/// 等级大于 `MAX_LEVEL` 时官方返回 `null`（不加载），这里返回 `None`。
pub const fn generation_status(level: i32) -> Option<ChunkStatus> {
    if level < 0 {
        None
    } else if level <= FULL_CHUNK_LEVEL {
        Some(ChunkStatus::Full)
    } else if level > MAX_LEVEL {
        None
    } else {
        // accumulatedDependencies.get(level - 33)，下标 1..11。
        match level - FULL_CHUNK_LEVEL {
            1 => Some(ChunkStatus::InitializeLight),
            2 => Some(ChunkStatus::Terrain),
            3 => Some(ChunkStatus::Biomes),
            _ => Some(ChunkStatus::StructureStarts),
        }
    }
}

/// `ChunkLevel.fullStatus`。
pub const fn full_status(level: i32) -> FullChunkStatus {
    FullChunkStatus::of_level(level)
}

/// `ChunkLevel.isEntityTicking`。
pub const fn is_entity_ticking(level: i32) -> bool {
    level <= ENTITY_TICKING_LEVEL
}

/// `ChunkLevel.isBlockTicking`。
pub const fn is_block_ticking(level: i32) -> bool {
    level <= BLOCK_TICKING_LEVEL
}

/// `ChunkLevel.isLoaded`。
pub const fn is_loaded(level: i32) -> bool {
    level <= MAX_LEVEL
}

/// `TicketType` 的标志位（`TicketType.FLAG_*`）。
pub mod flag {
    /// 写进 `chunk_tickets`（`persist()`）。
    pub const PERSIST: u8 = 1;
    /// 参与加载平面（`doesLoad()`）。
    pub const LOADING: u8 = 2;
    /// 参与模拟平面（`doesSimulate()`）。
    pub const SIMULATION: u8 = 4;
    /// 让维度保持活跃（`shouldKeepDimensionActive()`）。
    pub const KEEP_DIMENSION_ACTIVE: u8 = 8;
    /// 区块未加载时也可以倒计时（`canExpireIfUnloaded()`）。
    pub const CAN_EXPIRE_IF_UNLOADED: u8 = 16;
}

/// 26.3 `BuiltInRegistries.TICKET_TYPE` 里的九种票。
///
/// 超时和标志逐项对着 `TicketType.<clinit>` 的字节码；0 表示没有超时。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TicketType {
    PlayerSpawn,
    SpawnSearch,
    Dragon,
    PlayerLoading,
    PlayerSimulation,
    Forced,
    Portal,
    EnderPearl,
    Unknown,
}

impl TicketType {
    /// 注册表里的全部类型，按注册顺序。
    pub const ALL: [Self; 9] = [
        Self::PlayerSpawn,
        Self::SpawnSearch,
        Self::Dragon,
        Self::PlayerLoading,
        Self::PlayerSimulation,
        Self::Forced,
        Self::Portal,
        Self::EnderPearl,
        Self::Unknown,
    ];

    /// 官方注册名（`BuiltInRegistries.TICKET_TYPE` 的键）。
    pub const fn name(self) -> &'static str {
        match self {
            Self::PlayerSpawn => "player_spawn",
            Self::SpawnSearch => "spawn_search",
            Self::Dragon => "dragon",
            Self::PlayerLoading => "player_loading",
            Self::PlayerSimulation => "player_simulation",
            Self::Forced => "forced",
            Self::Portal => "portal",
            Self::EnderPearl => "ender_pearl",
            Self::Unknown => "unknown",
        }
    }

    /// 按官方注册名解析，用于装回存盘的票。
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|ty| ty.name() == name)
    }

    /// `TicketType.timeout()`；0 是 `NO_TIMEOUT`。
    pub const fn timeout(self) -> u32 {
        match self {
            Self::PlayerSpawn => 20,
            Self::SpawnSearch | Self::Unknown => 1,
            Self::Portal => 300,
            Self::EnderPearl => 40,
            _ => 0,
        }
    }

    /// `TicketType.flags()`。
    pub const fn flags(self) -> u8 {
        match self {
            Self::PlayerSpawn | Self::SpawnSearch | Self::PlayerLoading => flag::LOADING,
            Self::Dragon => flag::LOADING | flag::SIMULATION,
            Self::PlayerSimulation => flag::SIMULATION | flag::KEEP_DIMENSION_ACTIVE,
            Self::Forced | Self::Portal => {
                flag::PERSIST | flag::LOADING | flag::SIMULATION | flag::KEEP_DIMENSION_ACTIVE
            }
            Self::EnderPearl => flag::LOADING | flag::SIMULATION | flag::KEEP_DIMENSION_ACTIVE,
            Self::Unknown => flag::LOADING | flag::CAN_EXPIRE_IF_UNLOADED,
        }
    }

    /// `hasTimeout()`。
    pub const fn has_timeout(self) -> bool {
        self.timeout() != 0
    }

    /// `persist()`：写进 saved data `chunk_tickets`。
    pub const fn persists(self) -> bool {
        self.flags() & flag::PERSIST != 0
    }

    /// `doesLoad()`：喂给加载平面。
    pub const fn loads(self) -> bool {
        self.flags() & flag::LOADING != 0
    }

    /// `doesSimulate()`：喂给模拟平面。
    pub const fn simulates(self) -> bool {
        self.flags() & flag::SIMULATION != 0
    }

    /// `shouldKeepDimensionActive()`。
    pub const fn keeps_dimension_active(self) -> bool {
        self.flags() & flag::KEEP_DIMENSION_ACTIVE != 0
    }

    /// `canExpireIfUnloaded()`：区块没有卸载完也能倒计时。
    pub const fn can_expire_if_unloaded(self) -> bool {
        self.flags() & flag::CAN_EXPIRE_IF_UNLOADED != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_matches_bytecode() {
        // TicketType.<clinit> 里 register(name, timeout, flags) 的九个调用。
        let expected = [
            (TicketType::PlayerSpawn, 20, 2),
            (TicketType::SpawnSearch, 1, 2),
            (TicketType::Dragon, 0, 6),
            (TicketType::PlayerLoading, 0, 2),
            (TicketType::PlayerSimulation, 0, 12),
            (TicketType::Forced, 0, 15),
            (TicketType::Portal, 300, 15),
            (TicketType::EnderPearl, 40, 14),
            (TicketType::Unknown, 1, 18),
        ];
        for (ty, timeout, flags) in expected {
            assert_eq!(ty.timeout(), timeout, "{} 的超时", ty.name());
            assert_eq!(ty.flags(), flags, "{} 的标志", ty.name());
            assert_eq!(ty.has_timeout(), timeout != 0);
            assert_eq!(ty.persists(), flags & 1 != 0);
            assert_eq!(ty.loads(), flags & 2 != 0);
            assert_eq!(ty.simulates(), flags & 4 != 0);
            assert_eq!(ty.keeps_dimension_active(), flags & 8 != 0);
            assert_eq!(ty.can_expire_if_unloaded(), flags & 16 != 0);
            assert_eq!(TicketType::from_name(ty.name()), Some(ty));
        }
        assert_eq!(TicketType::ALL.len(), 9);
        assert_eq!(TicketType::from_name("not_a_ticket"), None);
    }

    #[test]
    fn level_boundaries_match_bytecode() {
        assert_eq!(MAX_LEVEL, 44);
        assert_eq!(UNLOADED_LEVEL, 45);
        assert_eq!(generation_status(0), Some(ChunkStatus::Full));
        assert_eq!(generation_status(33), Some(ChunkStatus::Full));
        assert_eq!(generation_status(34), Some(ChunkStatus::InitializeLight));
        assert_eq!(generation_status(35), Some(ChunkStatus::Terrain));
        assert_eq!(generation_status(36), Some(ChunkStatus::Biomes));
        for level in 37..=44 {
            assert_eq!(generation_status(level), Some(ChunkStatus::StructureStarts));
        }
        assert_eq!(generation_status(45), None);
        assert_eq!(generation_status(-1), None);

        assert_eq!(full_status(31), FullChunkStatus::EntityTicking);
        assert_eq!(full_status(32), FullChunkStatus::BlockTicking);
        assert_eq!(full_status(33), FullChunkStatus::Full);
        assert_eq!(full_status(34), FullChunkStatus::Inaccessible);
        assert_eq!(FullChunkStatus::Inaccessible.level(), 45);
        assert!(FullChunkStatus::EntityTicking.is_or_after(FullChunkStatus::Full));
        assert!(!FullChunkStatus::Full.is_or_after(FullChunkStatus::BlockTicking));

        assert!(is_loaded(44));
        assert!(!is_loaded(45));
        assert!(is_entity_ticking(31) && !is_entity_ticking(32));
        assert!(is_block_ticking(32) && !is_block_ticking(33));
    }

    #[test]
    fn status_levels_match_bytecode() {
        // ChunkLevel.byStatus 在 JVM 里读出来的十个值。
        let expected = [44, 44, 36, 36, 35, 34, 34, 33, 33, 33];
        for (status, level) in ChunkStatus::ALL.into_iter().zip(expected) {
            assert_eq!(status.level(), level, "{}", status.name());
        }
    }
}
