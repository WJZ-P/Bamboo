//! 一次存档会话：启动加载、按官方顺序保存、关服释放锁。
//!
//! `SaveSession` 由世界线程在启动时创建，启动阶段同步读 `level.dat` 和 `data/`
//! （官方 `MinecraftServer` 构造时也是同步读），之后一直在世界线程手里。
//! 区块和玩家的读写由别的模块经 `dir()` 和 `level` 完成，本结构只负责生命周期顺序。
//!
//! 关服顺序照官方 `stopServer` / `saveAllChunks`：调用方先存所有在线玩家、再存所有
//! 区块，然后 [`SaveSession::save_all`] 写 `level.dat` 和 `data/`，等 IO 落盘、
//! `sync_all`，最后 [`SaveSession::close`] 释放 `session.lock`。
//!
//! A save session: startup load, saving in the official order, and releasing the
//! lock at shutdown. `SaveSession` is created by the world thread at startup. It
//! reads `level.dat` and `data/` synchronously during startup (official
//! `MinecraftServer` does the same), then stays owned by the world thread. Chunk
//! and player IO is done by other modules via `dir()` and `level`; this struct only
//! owns the lifecycle order.

use std::io;

use mc_bamboo_data::version;
use mc_bamboo_nbt::{Compression, Tag, write_compressed};
use mc_bamboo_storage::{
    level_dat::LevelData,
    world_dir::{
        LevelSource, SessionLock, WorldDir, load_level_data, read_saved_data, read_saved_data_at,
        safe_replace, save_level_data, write_default_saved_data, write_saved_data,
        write_saved_data_at,
    },
};
use tracing::{info, warn};

use crate::{global::Global, inbound::PlayerId, save_data::StartupData};

/// 命令发给世界线程的存档请求。命令动作不能直接碰 IO，压进 `Global`，由 `World` 消费。
/// `SaveAll.source` 记下发起命令的玩家：成功消息要等保存完成（flush 还要等 IO 回执）
/// 才发，所以世界线程得知道发给谁。
/// A save request from a command. Command actions cannot touch IO, so they push
/// one of these into `Global` for the world thread to consume. `SaveAll.source`
/// is the requesting player: the success message is sent only after the save
/// completes (and after the IO ack for `flush`), so the world thread needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveCommand {
    SaveAll { flush: bool, source: PlayerId },
    SaveOn,
    SaveOff,
}

/// 一次运行期间的存档状态。`None` 的 `world_dir` 表示不存档（测试、oracle）。
#[derive(Debug)]
pub struct SaveSession {
    dir: WorldDir,
    lock: Option<SessionLock>,
    level: LevelData,
    auto_save: bool,
    /// 启动时从 `data/` 读到的游戏规则 / 世界边界，供 `World::build` 采用。
    /// Game rules / world border loaded from `data/` at startup for `World::build`.
    startup: StartupData,
    /// `level.dat` 是不是真的从盘上读到（全新世界没有，运行时默认值才是权威）。
    /// Whether `level.dat` was actually read from disk (a fresh world has none,
    /// so the runtime defaults are authoritative).
    loaded_level: bool,
}

impl SaveSession {
    /// 建目录、拿独占锁、读关卡数据、写出默认的 `data/` 文件。
    /// 拿不到锁时返回错误，调用方应拒绝启动。
    pub fn open(root: &std::path::Path) -> io::Result<(Self, LevelSource)> {
        let dir = WorldDir::new(root);
        dir.create_layout()?;
        let lock = SessionLock::acquire(&dir)?;
        let loaded = load_level_data(&dir);
        match loaded.source {
            LevelSource::Legacy => warn!(
                path = %dir.root().display(),
                "legacy world layout found; treating it as an empty 26.3 world"
            ),
            LevelSource::Fallback => warn!(
                path = %dir.level_dat().display(),
                "level.dat failed to load; restored from level.dat_old"
            ),
            LevelSource::Fresh => info!(
                path = %dir.root().display(),
                "creating a new world"
            ),
            LevelSource::Primary => {}
        }
        write_default_saved_data(&dir)?;
        // 启动时读回游戏规则（全局 data/）和世界边界（维度 data/）；文件不存在时用默认。
        // Read back game rules (global data/) and world border (dimension data/) at
        // startup; a missing file means defaults.
        let startup = StartupData {
            game_rules: read_saved_data(&dir, "minecraft:game_rules"),
            world_border: read_saved_data_at(&dir.world_border_dat()),
        };
        let loaded_level = matches!(loaded.source, LevelSource::Primary | LevelSource::Fallback);
        Ok((
            Self {
                dir,
                lock: Some(lock),
                level: loaded.data,
                auto_save: true,
                startup,
                loaded_level,
            },
            loaded.source,
        ))
    }

    pub fn dir(&self) -> &WorldDir {
        &self.dir
    }

    pub fn level(&self) -> &LevelData {
        &self.level
    }

    pub fn level_mut(&mut self) -> &mut LevelData {
        &mut self.level
    }

    /// 启动时读到的游戏规则 / 世界边界。
    /// Game rules / world border loaded at startup.
    pub(crate) fn startup(&self) -> &StartupData {
        &self.startup
    }

    /// 盘上是不是真的有 `level.dat`（全新世界为假，运行时默认值才是权威）。
    /// Whether a `level.dat` was read from disk (false for a fresh world, where
    /// the runtime defaults are authoritative).
    pub(crate) fn has_loaded_level(&self) -> bool {
        self.loaded_level
    }

    pub fn auto_save(&self) -> bool {
        self.auto_save
    }

    pub fn set_auto_save(&mut self, enabled: bool) {
        self.auto_save = enabled;
    }

    /// 按官方 `saveAllChunks` 顺序写 `level.dat` → `data/` → `chunk_tickets.dat`。
    /// 先把运行时的世界设置（出生点、默认游戏模式、难度）同步进 `level.dat`，
    /// 再把游戏规则、世界边界写成官方 codec 的独立文件；Bamboo 没建模的 saved data
    /// 写空默认。玩家和区块由调用方在此之前存好；本方法不等待 IO 线程。
    /// Write `level.dat` → `data/` → `chunk_tickets.dat` in the official order,
    /// syncing runtime world settings into `level.dat` and writing game rules /
    /// world border as their official saved-data files. Unmodelled saved data gets
    /// empty defaults. Players and chunks are saved by the caller, not here.
    pub(crate) fn save_all(&mut self, global: &Global, last_played: i64) -> io::Result<()> {
        crate::save_data::sync_level_data(
            &mut self.level,
            global.state(),
            global.time().0,
            last_played,
        );
        save_level_data(&self.dir, &self.level)?;
        write_saved_data(
            &self.dir,
            "minecraft:game_rules",
            &crate::save_data::game_rules_tag(global.rules()),
        )?;
        write_saved_data_at(
            &self.dir.world_border_dat(),
            &crate::save_data::world_border_tag(&global.world_border().export_settings()),
        )?;
        write_default_saved_data(&self.dir)?;
        self.write_chunk_tickets()
    }

    /// 官方 `TicketStorage.TYPE` 的空内容：目前没有带存盘标志的票，
    /// 写一个合法的空列表，让官方服务端能直接读。
    fn write_chunk_tickets(&self) -> io::Result<()> {
        let root = Tag::Compound(vec![
            (
                "data".to_owned(),
                Tag::Compound(vec![("tickets".to_owned(), Tag::List(Vec::new()))]),
            ),
            ("DataVersion".to_owned(), Tag::Int(version::DATA_VERSION)),
        ]);
        let bytes = write_compressed("", &root, Compression::Gzip)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        safe_replace(&self.dir.chunk_tickets(), &bytes, None)
    }

    /// 释放 `session.lock`。关服路径最后一步调用；出错也只记日志，不让进程挂死。
    pub fn close(&mut self) {
        self.lock.take();
    }
}

impl Drop for SaveSession {
    fn drop(&mut self) {
        self.close();
    }
}

/// 连接层进服时调用：读玩家存档的原始 gzip 字节（连接任务负责做这一次 IO）。
/// 没有存档 / 没启用存档时返回 `None`，按新玩家进服。
///
/// Called by the connection layer on join: read the raw player-save bytes (the
/// connection task does this IO). `None` means a new player.
pub fn load_player_bytes(root: &std::path::Path, uuid_bytes: &[u8; 16]) -> Option<Vec<u8>> {
    let uuid = mc_bamboo_storage::world_dir::player_uuid_string(uuid_bytes);
    mc_bamboo_storage::world_dir::read_player_bytes(&WorldDir::new(root), &uuid)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("bamboo-save-session-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn open_creates_layout_and_level_and_data() {
        let tmp = TempDir::new("open");
        let (mut session, source) = SaveSession::open(&tmp.0).unwrap();
        assert_eq!(source, LevelSource::Fresh);
        assert!(!session.dir().level_dat().exists());
        assert!(session.dir().region_dir().is_dir());
        assert!(session.dir().saved_data("minecraft:game_rules").exists());
        session.save_all(&Global::default(), 456).unwrap();
        assert!(session.dir().level_dat().exists());
        assert!(session.dir().chunk_tickets().exists());
        // 游戏规则和世界边界都写成官方 codec 的独立文件。
        assert!(session.dir().saved_data("minecraft:game_rules").exists());
        assert!(session.dir().world_border_dat().exists());
        session.close();
        // 释放锁之后可以再打开。
        assert!(SaveSession::open(&tmp.0).is_ok());
    }

    #[test]
    fn second_open_is_refused_while_locked() {
        let tmp = TempDir::new("lock");
        let _session = SaveSession::open(&tmp.0).unwrap();
        assert!(SaveSession::open(&tmp.0).is_err());
    }

    #[test]
    fn auto_save_toggles() {
        let tmp = TempDir::new("autosave");
        let (mut session, _) = SaveSession::open(&tmp.0).unwrap();
        assert!(session.auto_save());
        session.set_auto_save(false);
        assert!(!session.auto_save());
    }

    /// 运行时的游戏规则、世界设置、世界边界写盘后再开一局，startup 路径要还原回来。
    /// Runtime game rules, world settings and world border survive a save/reopen.
    #[test]
    fn runtime_settings_round_trip_through_disk() {
        use mc_bamboo_core::BlockPos;
        use mc_bamboo_data::gamerules;
        use mc_bamboo_protocol::packet::play::GameMode;

        let tmp = TempDir::new("roundtrip");
        let mut global = Global::default();
        let index = gamerules::find("minecraft:random_tick_speed").unwrap();
        assert!(matches!(
            global.rules_mut().assign(index, "11"),
            crate::gamerules::Assignment::Updated(_)
        ));
        global.state_mut().default_game_mode = GameMode::Survival;
        global.state_mut().difficulty = crate::world_state::Difficulty::Hard;
        global.state_mut().spawn =
            crate::world_state::RespawnData::of(BlockPos::new(100, 70, -200), 45.0, 20.0);
        let border = global.world_border_mut();
        let _ = border.set_center(12.0, -8.0);
        let _ = border.set_size(321.0);
        let _ = border.set_warning_time(42);

        {
            let (mut session, _) = SaveSession::open(&tmp.0).unwrap();
            session.save_all(&global, 1234).unwrap();
            session.close();
        }

        let (session, _) = SaveSession::open(&tmp.0).unwrap();
        let mut restored = Global::default();
        let mut region = crate::region::Region::new(2, 20, false);
        crate::save_data::apply_level_data(restored.state_mut(), session.level());
        session
            .startup()
            .apply(&mut restored, &mut region, session.level().time);

        assert_eq!(restored.rules().int(index), 11);
        assert_eq!(restored.state().default_game_mode, GameMode::Survival);
        assert_eq!(
            restored.state().difficulty,
            crate::world_state::Difficulty::Hard
        );
        assert_eq!(restored.state().spawn.pos, BlockPos::new(100, 70, -200));
        let settings = restored.world_border().export_settings();
        assert_eq!(settings.center_x, 12.0);
        assert_eq!(settings.center_z, -8.0);
        assert_eq!(settings.size, 321.0);
        assert_eq!(settings.warning_time, 42);
        assert_eq!(session.level().last_played, 1234);
    }
}
