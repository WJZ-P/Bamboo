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

use std::{io, path::PathBuf};

use mc_bamboo_nbt::Tag;
use mc_bamboo_storage::{
    level_dat::LevelData,
    world_dir::{
        LevelSource, SessionLock, WorldDir, encode_level_data, encode_saved_data, load_level_data,
        read_saved_data, read_saved_data_at,
    },
};
use tracing::{info, warn};

use crate::{global::Global, inbound::PlayerId, save_data::StartupData};

/// 一份交给 IO 线程写的独立文件：世界线程只编出字节，不碰磁盘（R5）。
/// A standalone file handed to the IO thread: the world thread only encodes
/// bytes and never touches the disk (R5).
#[derive(Debug, Clone)]
pub(crate) struct EncodedFile {
    pub path: PathBuf,
    pub backup: Option<PathBuf>,
    pub bytes: Vec<u8>,
}

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
    /// 启动时从 `data/` 读到的游戏规则 / 世界边界 / 世界时钟，供 `World::build` 采用。
    /// Game rules, world border and world clocks loaded from `data/` at startup.
    startup: StartupData,
    /// `level.dat` 是不是真的从盘上读到（全新世界没有，运行时默认值才是权威）。
    /// Whether `level.dat` was actually read from disk (a fresh world has none,
    /// so the runtime defaults are authoritative).
    loaded_level: bool,
    /// 启动时从 `world_gen_settings.dat` 读到的种子 / 世界类型（旧世界可能没有）。
    /// Seed / level type read from `world_gen_settings.dat` at startup (older
    /// worlds may not have the file).
    stored_world_gen: Option<crate::world_options::WorldGenConfig>,
    /// 这一局实际用的世界生成参数，写 `world_gen_settings.dat` 时用。
    /// The world-gen config actually used this session, written to
    /// `world_gen_settings.dat` on save.
    world_gen: crate::world_options::WorldGenConfig,
}

/// 开服时读回所有 `data/<命名空间>/command_storage.dat`。
/// 文件名由 `Identifier(ns, "command_storage").withSuffix(".dat").resolveAgainst(data)`
/// 得到；实测 26.3 写的是 `<ns>/command_storage.dat`，不是
/// `command_storage_<ns>.dat`。坏文件整份跳过，和官方 `readSavedData` 一致。
///
/// Read every `data/<namespace>/command_storage.dat` at startup.
fn read_command_storage(dir: &WorldDir) -> Vec<(String, Tag)> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir.root().join("data")) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(namespace) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let file = path.join("command_storage.dat");
        if let Some(tag) = read_saved_data_at(&file) {
            out.push((namespace, tag));
        }
    }
    out
}

/// 开服时读回 `data/minecraft/maps/last_id.dat` 与全部 `data/minecraft/maps/<id>.dat`。
/// 坏文件按官方 `SavedDataStorage.readSavedData` 跳过，不影响其余地图。
///
/// Read `data/minecraft/maps/last_id.dat` and all `data/minecraft/maps/<id>.dat` at startup.
fn read_maps(dir: &WorldDir) -> crate::map::MapStore {
    let mut store = crate::map::MapStore::new();
    if let Some(tag) = read_saved_data(dir, "minecraft:maps/last_id") {
        store.index = crate::map::MapIndex::from_tag(&tag);
    }
    let maps_dir = dir.root().join("data/minecraft/maps");
    let Ok(entries) = std::fs::read_dir(maps_dir) else {
        return store;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Some(stem) = name.strip_suffix(".dat") else {
            continue;
        };
        let Ok(id) = stem.parse::<i32>() else {
            continue;
        };
        if let Some(tag) = read_saved_data_at(&path)
            && let Some(data) = crate::map::MapItemSavedData::from_tag(&tag)
        {
            store.maps.insert(id, std::sync::Arc::new(data));
        }
    }
    store
}

impl SaveSession {
    /// 建目录、拿独占锁、读关卡数据、写出默认的 `data/` 文件。
    /// 拿不到锁时返回错误，调用方应拒绝启动。
    pub fn open(root: &std::path::Path) -> io::Result<(Self, LevelSource)> {
        let dir = WorldDir::new(root);
        dir.create_layout()?;
        let lock = SessionLock::acquire(&dir)?;
        let loaded = load_level_data(&dir);
        // 只支持一个版本：区块解码只认当前的 `DataVersion`。别的版本存的世界硬开，
        // 每个区块都会被当成读坏、从种子重新生成，下一次存盘就盖掉原来的内容。
        // 所以在这里拒绝启动（返回时锁随之释放），不碰世界目录里的任何文件。
        // One version only: the chunk decoder accepts the current `DataVersion`
        // alone. Opening a world from another version would treat every chunk as
        // unreadable, regenerate it from the seed and overwrite it on the next
        // save. Refuse to start instead (the lock is released on return) and
        // leave every file in the world directory alone.
        let expected = mc_bamboo_data::version::DATA_VERSION;
        if matches!(loaded.source, LevelSource::Primary | LevelSource::Fallback)
            && loaded.data.data_version != expected
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "the world was saved with DataVersion {}, but only {expected} is supported; \
                     opening it would regenerate and overwrite its chunks",
                    loaded.data.data_version
                ),
            ));
        }
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
        // 补 10：不写未建模的 saved data 空文件；文件不存在时官方 codec 会自己造默认值。
        // Item 10: no empty default files for unmodelled saved data; official
        // codecs construct defaults when a file is absent.
        // 启动时读回游戏规则 / 世界时钟（全局 data/）和世界边界（维度 data/）；
        // 文件不存在时用默认，和官方 `computeIfAbsent` 一样。
        // Read back game rules / world clocks (global data/) and world border
        // (dimension data/) at startup; a missing file means defaults, like
        // official `computeIfAbsent`.
        let startup = StartupData {
            game_rules: read_saved_data(&dir, "minecraft:game_rules"),
            world_border: read_saved_data_at(&dir.world_border_dat()),
            world_clocks: read_saved_data(&dir, "minecraft:world_clocks"),
            raids: read_saved_data(&dir, "minecraft:raids")
                .map(|tag| mc_bamboo_blocks::entity::raid::Raids::from_tag(&tag))
                .unwrap_or_default(),
            maps: read_maps(&dir),
            wandering_trader: read_saved_data(&dir, "minecraft:wandering_trader"),
            command_storage: read_command_storage(&dir),
        };
        let loaded_level = matches!(loaded.source, LevelSource::Primary | LevelSource::Fallback);
        // 世界生成设置（种子 + 维度生成器）存在 `data/` 里；有存档就以存档为准，
        // 这样空 `level-seed` 的随机种子在重启后仍指向同一个世界（官方行为）。
        // World-gen settings (seed + dimension generator) live in `data/`; a
        // saved world wins over config, so an empty `level-seed`'s random value
        // still addresses the same world after a restart (vanilla behaviour).
        let stored_world_gen = read_saved_data(&dir, "minecraft:world_gen_settings")
            .and_then(|data| crate::world_gen_settings::decode(&data));
        Ok((
            Self {
                dir,
                lock: Some(lock),
                level: loaded.data,
                auto_save: true,
                startup,
                loaded_level,
                stored_world_gen,
                world_gen: crate::world_options::WorldGenConfig::default(),
            },
            loaded.source,
        ))
    }

    /// 启动时从存档读到的世界生成参数；没有就是 `None`，调用方用配置。
    /// World-gen config read from the save at startup; `None` means use config.
    pub(crate) fn loaded_world_gen(&self) -> Option<&crate::world_options::WorldGenConfig> {
        self.stored_world_gen.as_ref()
    }

    /// 记下这一局实际用的参数，供 `encode_saves` 写回 `world_gen_settings.dat`。
    /// Record the config used this session for `encode_saves` to persist.
    pub(crate) fn set_world_gen(&mut self, world_gen: crate::world_options::WorldGenConfig) {
        self.world_gen = world_gen;
    }

    /// 官方 `SavedDataStorage.getDataFile`：`data/<命名空间>/command_storage.dat`。
    /// Official `SavedDataStorage.getDataFile`: `data/<namespace>/command_storage.dat`.
    pub(crate) fn command_storage_path(&self, namespace: &str) -> PathBuf {
        self.dir.saved_data(&format!("{namespace}:command_storage"))
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

    /// 启动时读到的游戏规则 / 世界边界 / 世界时钟。
    /// Game rules / world border / world clocks loaded at startup.
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

    /// 按官方 `saveAllChunks` 顺序**编出** `level.dat` → `data/` →
    /// `chunk_tickets.dat` 的字节，交给 IO 线程写。世界线程不碰磁盘（R5 / F3）。
    /// 先把运行时的世界设置（出生点、默认游戏模式、难度）同步进 `LevelData`。
    /// 玩家和区块由调用方在此之前存好。
    /// Encode, in the official `saveAllChunks` order, the bytes for `level.dat`,
    /// `data/` and `chunk_tickets.dat` for the IO thread to write. The world
    /// thread never touches the disk (R5 / F3). Runtime world settings are first
    /// synced into `LevelData`. Players and chunks are handled by the caller.
    pub(crate) fn encode_saves(
        &mut self,
        global: &Global,
        islands: &[(u32, mc_bamboo_blocks::entity::raid::Raids)],
        last_played: i64,
    ) -> io::Result<Vec<EncodedFile>> {
        crate::save_data::sync_level_data(
            &mut self.level,
            global.state(),
            self.dir.root(),
            global.time().0,
            last_played,
        );
        let mut files = vec![EncodedFile {
            path: self.dir.level_dat(),
            backup: Some(self.dir.level_dat_old()),
            bytes: encode_level_data(&self.level)?,
        }];
        files.push(EncodedFile {
            path: self.dir.saved_data("minecraft:game_rules"),
            backup: None,
            bytes: encode_saved_data(&crate::save_data::game_rules_tag(global.rules()))?,
        });
        files.push(EncodedFile {
            path: self.dir.world_border_dat(),
            backup: None,
            bytes: encode_saved_data(&crate::save_data::world_border_tag(
                &global.world_border().export_settings(),
            ))?,
        });
        // 两份世界时钟按 `PackedClockStates.CODEC` 写进全服的 `world_clocks.dat`；
        // 官方开服时由 `ServerClockManager.init` 读回，昼夜时间接着上次存盘的值走。
        // Both clocks go to the server-wide `world_clocks.dat` per
        // `PackedClockStates.CODEC`; `ServerClockManager.init` reads them back
        // so day time continues from the last save.
        files.push(EncodedFile {
            path: self.dir.saved_data("minecraft:world_clocks"),
            backup: None,
            bytes: encode_saved_data(&crate::save_data::world_clocks_tag(global))?,
        });
        // 按官方 `WorldGenSettings.CODEC` 写出这一局的种子和维度生成器：
        // flat 写 `minecraft:flat` + 地层，噪声世界写 `minecraft:noise` + settings 注册名。
        // 官方服务端再打开这个世界时按同样的生成器继续，种子也固定下来。
        // Write this session's seed and dimension generator per vanilla's
        // `WorldGenSettings.CODEC`: flat writes `minecraft:flat` + layers, noise
        // worlds write `minecraft:noise` + a settings id. An official server
        // reopening the world uses the same generator and the fixed seed.
        // `WanderingTraderData.TYPE`：全局一份 `data/minecraft/wandering_trader.dat`。
        // `WanderingTraderData.TYPE`: one global `data/minecraft/wandering_trader.dat`.
        files.push(EncodedFile {
            path: self.dir.saved_data("minecraft:wandering_trader"),
            backup: None,
            bytes: encode_saved_data(&crate::save_data::wandering_trader_tag(
                global.wandering_trader_data(),
            ))?,
        });
        files.push(EncodedFile {
            path: self.dir.saved_data("minecraft:world_gen_settings"),
            backup: None,
            bytes: encode_saved_data(&crate::world_gen_settings::encode(&self.world_gen))?,
        });
        // `Raids.TYPE` 的文件 id 是 `minecraft:raids`，每岛的袭击汇总成一份。
        // `Raids.TYPE` uses `minecraft:raids`; every island's raids are rolled up.
        files.push(EncodedFile {
            path: self.dir.saved_data("minecraft:raids"),
            backup: None,
            bytes: encode_saved_data(&crate::save_data::raids_tag(islands))?,
        });
        files.push(EncodedFile {
            path: self.dir.chunk_tickets(),
            backup: None,
            bytes: Self::chunk_tickets_bytes()?,
        });
        // 官方 `CommandStorage.Container.CODEC`：每个命名空间一份
        // `data/<ns>/command_storage.dat`，`set()` 标脏，官方只在
        // `SavedDataStorage.saveAndJoin` 写脏的命名空间。Bamboo 的 `save-all`
        // 是一条显式的「全量写」请求（不是每刻的增量调度），所以把全部已知
        // 命名空间（含空 contents）都写一遍，保证重启不丢数据。
        // Official `CommandStorage.Container.CODEC`: one file per namespace,
        // dirtied by `set()` and written by `SavedDataStorage.saveAndJoin`.
        // Bamboo's `save-all` is an explicit full save, so every known
        // namespace is written (empty `contents` included).
        for namespace in global.command_storage.namespaces() {
            files.push(EncodedFile {
                path: self.command_storage_path(&namespace),
                backup: None,
                bytes: encode_saved_data(&global.command_storage.namespace_tag(&namespace))?,
            });
        }
        // 官方 26.3：`MapIndex.TYPE` 写 `data/minecraft/maps/last_id.dat`，
        // 每张地图按 `MapItemSavedData.type(MapId)` 写 `data/minecraft/maps/<id>.dat`。
        // 没建过地图的新世界不写空的 `last_id.dat`（补 10）。
        // Official 26.3: `MapIndex.TYPE` writes `data/minecraft/maps/last_id.dat`
        // and each map writes `data/minecraft/maps/<id>.dat`. Fresh worlds with
        // no maps omit `last_id.dat` (Item 10).
        let maps = global.maps();
        if maps.index.last_map_id >= 0 || maps.index.dirty {
            files.push(EncodedFile {
                path: self.dir.saved_data("minecraft:maps/last_id"),
                backup: None,
                bytes: encode_saved_data(&maps.index.to_tag())?,
            });
        }
        for (&id, data) in &maps.maps {
            files.push(EncodedFile {
                path: self.dir.saved_data(&format!("minecraft:maps/{id}")),
                backup: None,
                bytes: encode_saved_data(&data.to_tag())?,
            });
        }
        // 补 10：只写建模过的 `data/` 文件；未建模的 saved data 保持「不存在」，
        // 官方 computeIfAbsent 会自己造默认值。已有的这类文件原样保留，不动。
        // Item 10: only the modelled `data/` files are written; unmodelled saved
        // data stays absent (official computeIfAbsent builds defaults). Existing
        // files of that kind are left untouched.
        Ok(files)
    }

    /// 测试专用：把 `encode_saves` 的字节同步落盘（模拟 IO 线程），便于断言文件内容。
    /// Test-only: write `encode_saves` bytes synchronously, standing in for the IO thread.
    #[cfg(test)]
    pub(crate) fn save_all(&mut self, global: &Global, last_played: i64) -> io::Result<()> {
        for file in self.encode_saves(global, &[], last_played)? {
            mc_bamboo_storage::world_dir::safe_replace(
                &file.path,
                &file.bytes,
                file.backup.as_deref(),
            )?;
        }
        Ok(())
    }

    /// 官方 `TicketStorage.TYPE` 的空内容：目前没有带存盘标志的票，
    /// 写一个合法的空列表，让官方服务端能直接读。
    fn chunk_tickets_bytes() -> io::Result<Vec<u8>> {
        encode_saved_data(&Tag::Compound(vec![(
            "tickets".to_owned(),
            Tag::List(Vec::new()),
        )]))
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

/// 连接层进服时读官方 `players/advancements/<uuid>.json` 的原文。
pub fn load_player_advancements(root: &std::path::Path, uuid_bytes: &[u8; 16]) -> Option<String> {
    let uuid = mc_bamboo_storage::world_dir::player_uuid_string(uuid_bytes);
    mc_bamboo_storage::world_dir::read_player_advancements(&WorldDir::new(root), &uuid)
}

/// 存盘 / 退出时写进度档案，路径照 26.3 `LevelResource.PLAYER_ADVANCEMENTS_DIR`。
pub fn save_player_advancements(
    root: &std::path::Path,
    uuid_bytes: &[u8; 16],
    text: &str,
) -> std::io::Result<()> {
    let uuid = mc_bamboo_storage::world_dir::player_uuid_string(uuid_bytes);
    mc_bamboo_storage::world_dir::write_player_advancements(&WorldDir::new(root), &uuid, text)
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
        // 补 10：全新世界不写任何 saved data 空文件。
        assert!(!session.dir().saved_data("minecraft:game_rules").exists());
        assert!(!session.dir().saved_data("minecraft:weather").exists());
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

    /// F3：世界线程只把 `level.dat` / `data/` 编成字节，绝不碰磁盘；
    /// 写盘是 IO 线程的事。
    /// F3: the world thread only encodes `level.dat`/`data/` bytes; the IO
    /// thread does the writing.
    #[test]
    fn encode_saves_only_encodes_bytes_and_never_writes() {
        let tmp = TempDir::new("encode-only");
        let (mut session, _) = SaveSession::open(&tmp.0).unwrap();
        assert!(!session.dir().level_dat().exists());
        let global = Global::default();
        let files = session.encode_saves(&global, &[], 777).unwrap();
        assert!(
            files
                .iter()
                .any(|f| f.path == session.dir().level_dat() && f.backup.is_some()),
            "level.dat bytes must be handed over with its _old backup"
        );
        assert!(
            files
                .iter()
                .any(|f| f.path == session.dir().saved_data("minecraft:game_rules")),
            "game rules bytes must be handed over"
        );
        assert!(
            files
                .iter()
                .any(|f| f.path == session.dir().world_border_dat()),
            "world border bytes must be handed over"
        );
        // 编码之后磁盘上仍然没有 level.dat：写是 IO 线程稍后做的。
        assert!(
            !session.dir().level_dat().exists(),
            "the world thread must not write the file"
        );
        // 模拟 IO 线程落盘，字节能被官方格式读回。
        for file in &files {
            mc_bamboo_storage::world_dir::safe_replace(
                &file.path,
                &file.bytes,
                file.backup.as_deref(),
            )
            .unwrap();
        }
        assert_eq!(
            mc_bamboo_storage::world_dir::load_level_data(session.dir())
                .data
                .last_played,
            777
        );
    }

    #[test]
    fn second_open_is_refused_while_locked() {
        let tmp = TempDir::new("lock");
        let _session = SaveSession::open(&tmp.0).unwrap();
        assert!(SaveSession::open(&tmp.0).is_err());
    }

    /// 别的版本存的世界（`level.dat` 的 `DataVersion` 不是 5023）必须拒绝打开。
    /// 区块解码只认 5023：硬开的话每个区块都被当成读坏、从种子重新生成，下一次
    /// 存盘就把玩家的建筑盖掉，`level.dat` 的版本号也被改写成 5023。官方对更新的
    /// 版本拒绝降级、对更旧的版本走升级，都不会这样毁档。
    ///
    /// A world saved by another version (`DataVersion` in `level.dat` is not
    /// 5023) must be refused. The chunk decoder accepts 5023 only: opening it
    /// anyway treats every chunk as unreadable, regenerates it from the seed, and
    /// the next save replaces the player's builds while `level.dat` is rewritten
    /// as 5023. Vanilla refuses a downgrade and upgrades older worlds; neither
    /// destroys the save.
    #[test]
    fn a_world_saved_by_another_version_is_refused() {
        use mc_bamboo_data::version::DATA_VERSION;
        use mc_bamboo_storage::world_dir::save_level_data;

        for other in [DATA_VERSION - 1, DATA_VERSION + 1] {
            let tmp = TempDir::new(&format!("other-version-{other}"));
            let dir = WorldDir::new(&tmp.0);
            dir.create_layout().unwrap();
            let level = LevelData {
                data_version: other,
                ..LevelData::default()
            };
            save_level_data(&dir, &level).unwrap();
            let before = std::fs::read(dir.level_dat()).unwrap();

            let error = SaveSession::open(&tmp.0)
                .expect_err("a world from another version must not be opened");
            assert!(error.to_string().contains(&other.to_string()), "{error}");
            assert_eq!(std::fs::read(dir.level_dat()).unwrap(), before);

            // 拒绝之后锁已经放开：同版本的世界照常能开。
            // The lock is released after the refusal: a matching world still opens.
            save_level_data(&dir, &LevelData::default()).unwrap();
            assert!(SaveSession::open(&tmp.0).is_ok());
        }
    }

    #[test]
    fn auto_save_toggles() {
        let tmp = TempDir::new("autosave");
        let (mut session, _) = SaveSession::open(&tmp.0).unwrap();
        assert!(session.auto_save());
        session.set_auto_save(false);
        assert!(!session.auto_save());
    }

    /// 运行时的游戏规则、世界设置、世界边界、世界时钟和游戏时间写盘后再开一局，
    /// startup 路径都要还原回来。
    /// Runtime game rules, world settings, border, clocks and game time all
    /// survive a save/reopen through the startup path.
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
        global.restore_game_time(4242);
        let clock = global.clock_named_mut("minecraft:overworld").unwrap();
        clock.set_total_ticks(18_000);
        clock.set_rate(3.0);
        clock.set_paused(false);
        let clocks = global.clocks();

        {
            let (mut session, _) = SaveSession::open(&tmp.0).unwrap();
            session.save_all(&global, 1234).unwrap();
            session.close();
        }

        let (session, _) = SaveSession::open(&tmp.0).unwrap();
        assert_eq!(session.level().time, 4242, "level.dat 的 Time 是游戏时间");
        let mut restored = Global::default();
        let mut region = crate::region::Region::new(2, 20, false);
        crate::save_data::apply_level_data(restored.state_mut(), session.level());
        restored.restore_game_time(session.level().time);
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
        assert_eq!(restored.time().0, 4242);
        assert_eq!(restored.clocks(), clocks, "两份世界时钟逐字段恢复");
        assert_eq!(session.level().last_played, 1234);
    }

    /// `/data modify storage` 写下的标签必须跨重启存活：文件名、NBT 信封和
    /// `contents` 结构都按官方 `CommandStorage` / `SavedDataStorage`。
    #[test]
    fn command_storage_round_trips_through_official_layout() {
        use mc_bamboo_nbt::Tag;
        use mc_bamboo_storage::world_dir::read_saved_data_at;

        let tmp = TempDir::new("command-storage");
        let mut global = Global::default();
        global.command_storage.set(
            "minecraft:test",
            Tag::Compound(vec![
                ("Value".to_owned(), Tag::Int(7)),
                ("Name".to_owned(), Tag::String("hi".to_owned())),
            ]),
        );
        global.command_storage.set(
            "bamboo:other",
            Tag::Compound(vec![("Other".to_owned(), Tag::Int(3))]),
        );
        {
            let (mut session, _) = SaveSession::open(&tmp.0).unwrap();
            session.save_all(&global, 1).unwrap();
            session.close();
        }
        let expected_minecraft = Tag::Compound(vec![(
            "contents".to_owned(),
            Tag::Compound(vec![(
                "test".to_owned(),
                Tag::Compound(vec![
                    ("Value".to_owned(), Tag::Int(7)),
                    ("Name".to_owned(), Tag::String("hi".to_owned())),
                ]),
            )]),
        )]);
        assert_eq!(
            read_saved_data_at(&tmp.0.join("data/minecraft/command_storage.dat")),
            Some(expected_minecraft)
        );
        assert!(
            tmp.0.join("data/bamboo/command_storage.dat").exists(),
            "非 minecraft 命名空间也单独一份"
        );
        let (session, _) = SaveSession::open(&tmp.0).unwrap();
        let mut restored = Global::default();
        let mut region = crate::region::Region::new(2, 20, false);
        session.startup().apply(&mut restored, &mut region, 0);
        assert_eq!(
            restored.command_storage.get("minecraft:test"),
            global.command_storage.get("minecraft:test")
        );
        assert_eq!(
            restored.command_storage.get("bamboo:other"),
            global.command_storage.get("bamboo:other")
        );
    }

    /// 官方服务器写出的 `command_storage.dat` 也要能读回来：额外直接写一份
    /// gzip 信封（等价于官方 `NbtIo.writeCompressed`），再开一局。
    #[test]
    fn command_storage_reads_official_written_files() {
        use mc_bamboo_nbt::Tag;
        use mc_bamboo_storage::world_dir::write_saved_data_at;

        let tmp = TempDir::new("official-command-storage");
        let (mut session, _) = SaveSession::open(&tmp.0).unwrap();
        let path = session.command_storage_path("minecraft");
        let data = Tag::Compound(vec![(
            "contents".to_owned(),
            Tag::Compound(vec![(
                "test".to_owned(),
                Tag::Compound(vec![("Value".to_owned(), Tag::Int(9))]),
            )]),
        )]);
        write_saved_data_at(&path, &data).unwrap();
        session.close();

        let (session, _) = SaveSession::open(&tmp.0).unwrap();
        let mut restored = Global::default();
        let mut region = crate::region::Region::new(2, 20, false);
        session.startup().apply(&mut restored, &mut region, 0);
        assert_eq!(
            restored.command_storage.get("minecraft:test"),
            Tag::Compound(vec![("Value".to_owned(), Tag::Int(9))])
        );
    }

    /// `world_clocks.dat` 和 `level.dat` 都是运行时的真值，运行时改了时钟再存，
    /// 重启后不能用第一次存盘的旧值。
    /// Re-saving after a clock change must overwrite the previous clocks file.
    #[test]
    fn world_clocks_and_game_time_are_resaved_from_runtime() {
        let tmp = TempDir::new("resave-time");
        let mut global = Global::default();
        global.restore_game_time(10);
        global.restore_clock(
            "minecraft:overworld",
            crate::global::Clock {
                total_ticks: 100,
                partial_tick: 0.0,
                rate: 1.0,
                paused: false,
            },
        );
        {
            let (mut session, _) = SaveSession::open(&tmp.0).unwrap();
            session.save_all(&global, 1).unwrap();
            session.close();
        }
        global.restore_game_time(90);
        global.restore_clock(
            "minecraft:overworld",
            crate::global::Clock {
                total_ticks: 900,
                partial_tick: 0.5,
                rate: 2.0,
                paused: true,
            },
        );
        {
            let (mut session, _) = SaveSession::open(&tmp.0).unwrap();
            session.save_all(&global, 2).unwrap();
            session.close();
        }

        let (session, _) = SaveSession::open(&tmp.0).unwrap();
        let mut restored = Global::default();
        restored.restore_game_time(session.level().time);
        let mut region = crate::region::Region::new(2, 20, false);
        session
            .startup()
            .apply(&mut restored, &mut region, session.level().time);
        assert_eq!(restored.time().0, 90);
        assert_eq!(restored.clocks(), global.clocks());
    }

    #[test]
    fn maps_and_last_id_round_trip_through_save_session() {
        let tmp = TempDir::new("maps-roundtrip");
        let mut global = Global::default();
        let mut store = crate::map::MapStore::new();
        let id0 = store.next_map_id();
        let mut data0 = crate::map::MapItemSavedData::create_fresh(
            10.0,
            -20.0,
            1,
            true,
            false,
            "minecraft:overworld",
        );
        data0.set_color(
            7,
            9,
            crate::map::packed_color(crate::map::map_color::WATER, crate::map::Brightness::High),
        );
        store.insert(id0, data0.clone());
        let id1 = store.next_map_id();
        let data1 = data0.locked();
        store.insert(id1, data1.clone());
        global.set_maps(store);

        {
            let (mut session, _) = SaveSession::open(&tmp.0).unwrap();
            let files = session.encode_saves(&global, &[], 123).unwrap();
            for file in files {
                mc_bamboo_storage::world_dir::safe_replace(
                    &file.path,
                    &file.bytes,
                    file.backup.as_deref(),
                )
                .unwrap();
            }
            assert!(session.dir().saved_data("minecraft:maps/last_id").exists());
            assert!(session.dir().saved_data("minecraft:maps/0").exists());
            assert!(session.dir().saved_data("minecraft:maps/1").exists());
            session.close();
        }

        let (session, _) = SaveSession::open(&tmp.0).unwrap();
        let mut restored_global = Global::default();
        let mut region = crate::region::Region::new(2, 20, false);
        session
            .startup()
            .apply(&mut restored_global, &mut region, session.level().time);
        assert_eq!(restored_global.maps().index.last_map_id, 1);
        assert_eq!(region.global.maps.index.last_map_id, 1);
        let r0 = restored_global.maps().get(0).expect("map 0 loaded");
        assert_eq!(r0.center_x, data0.center_x);
        assert_eq!(r0.center_z, data0.center_z);
        assert_eq!(r0.scale, 1);
        assert!(!r0.locked);
        assert_eq!(r0.colors, data0.colors);
        let r1 = restored_global.maps().get(1).expect("map 1 loaded");
        assert!(r1.locked);
        assert_eq!(r1.colors, data1.colors);
    }
}
