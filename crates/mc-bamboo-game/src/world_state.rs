//! 不属于任何区块、但全世界共用的状态。权威副本挂在 [`Global`](crate::global::Global) 上。
//!
//! 难度、默认游戏模式、世界出生点、种子、天气和随机序列都由命令改，进服序列从这里读。
//! 难度、默认游戏模式、出生点写进 `level.dat` 并在启动时读回；天气、种子、随机序列不存档；
//! 玩家自己的重生点存在 `Player`，不在这里。
//!
//! World-wide state that belongs to no chunk. Commands change it and the join sequence reads it.
//! Difficulty, default game mode and spawn are saved to `level.dat` and read back at startup;
//! weather, seed and random sequences are not saved. Per-player respawn points live on `Player`.

use mc_bamboo_core::BlockPos;
use mc_bamboo_loot::RandomSequences;
use mc_bamboo_protocol::packet::play::GameMode;

/// 没有存档也没有 `level-seed` 配置项，世界种子固定为 0。
///
/// There is no save file and no `level-seed` option, so the world seed is fixed at 0.
pub(crate) const WORLD_SEED: i64 = 0;

/// 官方专用服务器 `server.properties` 的 `difficulty` 默认 EASY。
const DEFAULT_DIFFICULTY: Difficulty = Difficulty::Easy;

/// 官方 `force-gamemode` 默认关闭，Bamboo 也没有这个配置项。
///
/// Official `force-gamemode` defaults to off; Bamboo has no option for it either.
pub(crate) const FORCE_GAME_MODE: bool = false;

/// 默认出生点：超平坦世界 (0, 0) 区块的中心，y 和高度无关，进服时另取地面。
const DEFAULT_SPAWN_POS: BlockPos = BlockPos::new(8, 0, 8);

/// 官方 `Difficulty`，枚举值就是 `getId()` 和网络包里的字节。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Difficulty {
    Peaceful = 0,
    Easy = 1,
    Normal = 2,
    Hard = 3,
}

impl Difficulty {
    /// 命令树里的名字，官方 `getSerializedName()`。
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Peaceful => "peaceful",
            Self::Easy => "easy",
            Self::Normal => "normal",
            Self::Hard => "hard",
        }
    }

    pub(crate) const ALL: [Self; 4] = [Self::Peaceful, Self::Easy, Self::Normal, Self::Hard];

    /// 官方 `getDisplayName()`：`options.difficulty.<name>`。
    pub(crate) fn display_name(self) -> String {
        format!("options.difficulty.{}", self.name())
    }
}

/// 官方 `LevelData.RespawnData`：世界出生点、朝向，以及所属维度。
///
/// 只有一个世界，维度固定是 `minecraft:overworld`。`of` 对应官方的
/// `RespawnData.of`：水平角 wrap 到 (-180, 180]，俯仰角限制在 [-90, 90]。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RespawnData {
    pub(crate) pos: BlockPos,
    pub(crate) yaw: f32,
    pub(crate) pitch: f32,
}

impl RespawnData {
    pub(crate) fn of(pos: BlockPos, mut yaw: f32, mut pitch: f32) -> Self {
        // `Mth.wrapDegrees` 的结果落在 [-180, 180)。官方 `clamp` 只约束俯仰角。
        yaw = mc_bamboo_core::mth::wrap_degrees(yaw);
        pitch = pitch.clamp(-90.0, 90.0);
        Self { pos, yaw, pitch }
    }
}

impl Default for RespawnData {
    fn default() -> Self {
        Self {
            pos: DEFAULT_SPAWN_POS,
            yaw: 0.0,
            pitch: 0.0,
        }
    }
}

/// 官方 `WeatherData` 加 `ServerLevel` 的四个渐变强度。时间都是 tick 数，强度是 0 到 1。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct Weather {
    pub(crate) raining: bool,
    pub(crate) rain_time: i32,
    pub(crate) thundering: bool,
    pub(crate) thunder_time: i32,
    pub(crate) clear_time: i32,
    pub(crate) rain_level: f32,
    pub(crate) o_rain_level: f32,
    pub(crate) thunder_level: f32,
    pub(crate) o_thunder_level: f32,
}

impl Weather {
    /// `Level.isRaining()`：主世界能降雨，雨强大于 0.2 才算下雨。
    pub(crate) fn is_raining(self) -> bool {
        f64::from(self.rain_level) > 0.2
    }

    /// 方块逻辑读的天气快照。
    pub(crate) fn snapshot(self) -> mc_bamboo_blocks::BlockWeather {
        mc_bamboo_blocks::BlockWeather {
            rain_level: self.rain_level,
            thunder_level: self.thunder_level,
        }
    }
}

/// 一局游戏的世界设置。不存档，重启全部回到默认值。
pub(crate) struct WorldState {
    pub(crate) difficulty: Difficulty,
    /// 官方 `isDifficultyLocked()`。没有命令能改它，命令发来的锁定状态照官方处理。
    pub(crate) difficulty_locked: bool,
    /// 新玩家进服时的游戏模式。Bamboo 现有行为是创造模式，不是官方专用的生存模式。
    pub(crate) default_game_mode: GameMode,
    pub(crate) seed: i64,
    pub(crate) spawn: RespawnData,
    pub(crate) weather: Weather,
    /// `/random` 和带 `random_sequence` 的战利品表共用。
    pub(crate) sequences: RandomSequences,
}

impl Default for WorldState {
    fn default() -> Self {
        Self {
            difficulty: DEFAULT_DIFFICULTY,
            difficulty_locked: false,
            default_game_mode: GameMode::Creative,
            seed: WORLD_SEED,
            spawn: RespawnData::default(),
            weather: Weather::default(),
            sequences: RandomSequences::new(WORLD_SEED),
        }
    }
}

impl WorldState {
    /// 官方 `BiomeManager.obfuscateSeed`：Guava 的 sha256(long)，第一个 long 按小端读。
    ///
    /// `HashCode.asLong()` 取摘要的前 8 字节，Guava 的 `hashLong` 用小端写输入。
    pub(crate) fn hashed_seed(&self) -> i64 {
        use sha2::{Digest, Sha256};
        let digest = Sha256::digest(self.seed.to_le_bytes());
        let mut bytes = [0_u8; 8];
        bytes.copy_from_slice(&digest[..8]);
        i64::from_le_bytes(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn obfuscated_seed_matches_guava() {
        // Guava `Hashing.sha256().hashLong(0).asLong()`，官方 `BiomeManager.obfuscateSeed(0)`。
        assert_eq!(
            WorldState::default().hashed_seed(),
            8_794_265_229_978_523_055
        );
    }

    #[test]
    fn respawn_data_wraps_yaw_and_clamps_pitch() {
        let data = RespawnData::of(BlockPos::new(1, 2, 3), 190.0, 120.0);
        assert_eq!(data.yaw, -170.0);
        assert_eq!(data.pitch, 90.0);
    }

    #[test]
    fn defaults_match_the_dedicated_server() {
        let state = WorldState::default();
        assert_eq!(state.difficulty, Difficulty::Easy);
        assert!(!state.difficulty_locked);
        assert_eq!(state.seed, 0);
        assert_eq!(state.spawn.pos, BlockPos::new(8, 0, 8));
    }
}
