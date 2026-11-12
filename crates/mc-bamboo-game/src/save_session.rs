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
        LevelSource, SessionLock, WorldDir, load_level_data, safe_replace, save_level_data,
        write_default_saved_data,
    },
};
use tracing::{info, warn};

/// 一次运行期间的存档状态。`None` 的 `world_dir` 表示不存档（测试、oracle）。
#[derive(Debug)]
pub struct SaveSession {
    dir: WorldDir,
    lock: Option<SessionLock>,
    level: LevelData,
    auto_save: bool,
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
        Ok((
            Self {
                dir,
                lock: Some(lock),
                level: loaded.data,
                auto_save: true,
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

    pub fn auto_save(&self) -> bool {
        self.auto_save
    }

    pub fn set_auto_save(&mut self, enabled: bool) {
        self.auto_save = enabled;
    }

    /// 按官方 `saveAllChunks` 顺序写 `level.dat` → `data/` → `chunk_tickets.dat`。
    /// 玩家和区块由调用方在此之前存好；本方法不涉及它们，也不等待 IO 线程（由调用方排空）。
    pub fn save_all(&self) -> io::Result<()> {
        save_level_data(&self.dir, &self.level)?;
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
        session.level_mut().time = 123;
        session.save_all().unwrap();
        assert!(session.dir().level_dat().exists());
        assert!(session.dir().chunk_tickets().exists());
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
}
