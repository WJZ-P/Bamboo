//! 世界目录的文件系统层：26.3 布局、`session.lock`、`level.dat`、`data/`。
//!
//! 这一层只碰文件和目录，不接游戏、不起线程。编解码已经在
//! [`crate::level_dat`] / [`crate::player_data`] / [`crate::chunk_nbt`] 完成，这里把它们
//! 写进官方 26.3 的目录，并照 `Util.safeReplaceFile` 做备份轮换。
//!
//! 关服的保存顺序（照官方 `MinecraftServer.stopServer` / `saveAllChunks`）由调用方编排：
//! 先存所有在线玩家，再存所有区块，最后写 `level.dat` 与 `data/`，等 IO 落盘、
//! `sync_all`，最后释放 `session.lock`。本模块提供每一步的原语；负责编排的是
//! `mc-bamboo-game` 的存档会话。
//!
//! The filesystem layer for a world directory: the 26.3 layout, `session.lock`,
//! `level.dat`, and `data/`. This layer only touches files and directories: no game
//! state and no threads. Codecs live in the sibling modules; this one writes them into
//! the official 26.3 layout and rotates backups like `Util.safeReplaceFile`.
//!
//! The shutdown save order (official `MinecraftServer.stopServer` / `saveAllChunks`) is
//! orchestrated by the caller: save all online players, then all chunks, then write
//! `level.dat` and `data/`, wait for IO to land and `sync_all`, and finally release
//! `session.lock`. This module provides the primitives for each step.

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use mc_bamboo_data::version;
use mc_bamboo_nbt::{Accounter, Compression, Tag, read_compressed, write_compressed};

use crate::level_dat::LevelData;

/// `DirectoryLock` 写进 `session.lock` 的内容，官方是 UTF-8 的 "☃"。
const SNOWMAN: &[u8] = &[0xe2, 0x98, 0x83];

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// 一个世界目录。所有路径都从根派生，不在这里创建任何文件。
#[derive(Debug, Clone)]
pub struct WorldDir {
    root: PathBuf,
}

/// 主世界的维度目录名，官方 `LevelStorageSource.getDimensionPath`。
const OVERWORLD: &str = "dimensions/minecraft/overworld";

impl WorldDir {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn level_dat(&self) -> PathBuf {
        self.root.join("level.dat")
    }

    pub fn level_dat_old(&self) -> PathBuf {
        self.root.join("level.dat_old")
    }

    pub fn session_lock(&self) -> PathBuf {
        self.root.join("session.lock")
    }

    pub fn players_data_dir(&self) -> PathBuf {
        self.root.join("players/data")
    }

    pub fn player_dat(&self, uuid: &str) -> PathBuf {
        self.players_data_dir().join(format!("{uuid}.dat"))
    }

    pub fn player_dat_old(&self, uuid: &str) -> PathBuf {
        self.players_data_dir().join(format!("{uuid}.dat_old"))
    }

    /// `id` 形如 `minecraft:game_rules` → `data/minecraft/game_rules.dat`。
    pub fn saved_data(&self, id: &str) -> PathBuf {
        let (ns, name) = id.split_once(':').unwrap_or(("minecraft", id));
        self.root.join("data").join(ns).join(format!("{name}.dat"))
    }

    pub fn dimension_dir(&self) -> PathBuf {
        self.root.join(OVERWORLD)
    }

    pub fn region_dir(&self) -> PathBuf {
        self.dimension_dir().join("region")
    }

    pub fn entities_dir(&self) -> PathBuf {
        self.dimension_dir().join("entities")
    }

    pub fn poi_dir(&self) -> PathBuf {
        self.dimension_dir().join("poi")
    }

    /// 维度自己的 `data/minecraft/`，`chunk_tickets.dat` 放这里。
    pub fn dimension_data_dir(&self) -> PathBuf {
        self.dimension_dir().join("data/minecraft")
    }

    pub fn chunk_tickets(&self) -> PathBuf {
        self.dimension_data_dir().join("chunk_tickets.dat")
    }

    /// 按 26.3 布局建好所有目录。可重复调用。
    pub fn create_layout(&self) -> io::Result<()> {
        for dir in [
            self.root.clone(),
            self.root.join("data"),
            self.root.join("data/minecraft"),
            self.players_data_dir(),
            self.dimension_data_dir(),
            self.region_dir(),
            self.entities_dir(),
            self.poi_dir(),
        ] {
            fs::create_dir_all(dir)?;
        }
        Ok(())
    }

    /// 老布局（1.x 把 `region/` / `DIM-1` 放在世界根）当成空世界（Q3）。
    /// 判据只看根目录：26.3 的 `create_layout` 从不在世界根建这些目录。
    /// Legacy layout (1.x kept `region/` / `DIM-1` at the world root) is treated
    /// as empty (Q3). The 26.3 layout never creates these under the root.
    pub fn has_legacy_layout(&self) -> bool {
        self.root.join("region").is_dir()
            || self.root.join("DIM-1").is_dir()
            || self.root.join("DIM1").is_dir()
    }
}

/// 独占锁，照官方 `DirectoryLock`：创建目录、写 "☃"、刷盘、`tryLock()`。
/// 锁随 `File` 的存活而持有，drop 时释放；拿不到就返回错误，让调用方拒绝启动。
#[derive(Debug)]
pub struct SessionLock {
    _file: File,
}

impl SessionLock {
    pub fn acquire(dir: &WorldDir) -> io::Result<Self> {
        fs::create_dir_all(dir.root())?;
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(dir.session_lock())?;
        // 先拿锁再写内容：被别的进程占着时，try_lock 直接失败，不会去写被锁的文件。
        // Take the lock before writing: when another process holds it, try_lock fails
        // without touching the locked file.
        match file.try_lock() {
            Ok(()) => {}
            Err(error) => {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    format!("world directory is already locked: {error}"),
                ));
            }
        }
        file.write_all(SNOWMAN)?;
        file.sync_all()?;
        Ok(Self { _file: file })
    }
}

/// 在 `dir` 里建一个唯一的临时文件，返回路径和句柄。
fn temp_file(dir: &Path, tag: &str) -> io::Result<(PathBuf, File)> {
    for _ in 0..128 {
        let seq = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = dir.join(format!(".{tag}.{}.{seq}.tmp", std::process::id()));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a temporary save file",
    ))
}

/// 原子替换 `target`：先写临时文件并刷盘，再把旧文件挪到 `backup`（若有），
/// 最后把临时文件改名成 `target`。和官方 `Util.safeReplaceFile` 的落盘结果一致。
/// `backup` 为 `None` 时（官方 `data/` 文件）直接覆盖，不留备份。
pub fn safe_replace(target: &Path, bytes: &[u8], backup: Option<&Path>) -> io::Result<()> {
    let dir = target.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(dir)?;
    let (tmp, mut file) = temp_file(dir, "save")?;
    if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(&tmp);
        return Err(error);
    }
    drop(file);

    if target.exists() {
        if let Some(bk) = backup {
            let _ = fs::remove_file(bk);
            if let Err(error) = fs::rename(target, bk) {
                let _ = fs::remove_file(&tmp);
                return Err(error);
            }
        } else if let Err(error) = fs::remove_file(target) {
            let _ = fs::remove_file(&tmp);
            return Err(error);
        }
    }

    if let Err(error) = fs::rename(&tmp, target) {
        // 尽量把备份挪回来，不留下一个没有主文件的世界。
        if let Some(bk) = backup
            && bk.exists()
            && !target.exists()
        {
            let _ = fs::rename(bk, target);
        }
        let _ = fs::remove_file(&tmp);
        return Err(error);
    }
    Ok(())
}

/// 从复合标签里取字段（`CompoundTag` 键序无关，重名按最后一次）。
fn field<'a>(tag: &'a Tag, key: &str) -> Option<&'a Tag> {
    match tag {
        Tag::Compound(entries) => entries
            .iter()
            .rev()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value),
        _ => None,
    }
}

/// `level.dat` 是从哪里读出来的，交给调用方记日志。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LevelSource {
    /// 从 `level.dat` 正常读到。
    Primary,
    /// `level.dat` 坏了或读不到，退回 `level.dat_old`。
    Fallback,
    /// 读到了老布局，按官方只支持 26.3 新布局的决定当空世界。
    Legacy,
    /// 两个文件都没有，是全新世界。
    Fresh,
}

/// 启动读到的关卡数据。
#[derive(Debug, Clone)]
pub struct LoadedLevel {
    pub data: LevelData,
    pub source: LevelSource,
}

fn read_level_file(path: &Path) -> Result<LevelData, String> {
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    LevelData::from_gzip(&bytes).map_err(|e| e.to_string())
}

/// 启动读 `level.dat`：正常文件优先，坏文件退回 `level.dat_old`（官方
/// `LevelStorageAccess.getUnfixedDataTagWithFallback`），老布局或全缺时按空世界。
pub fn load_level_data(dir: &WorldDir) -> LoadedLevel {
    if dir.has_legacy_layout() {
        return LoadedLevel {
            data: LevelData::default(),
            source: LevelSource::Legacy,
        };
    }
    if let Ok(data) = read_level_file(&dir.level_dat()) {
        return LoadedLevel {
            data,
            source: LevelSource::Primary,
        };
    }
    if let Ok(data) = read_level_file(&dir.level_dat_old()) {
        return LoadedLevel {
            data,
            source: LevelSource::Fallback,
        };
    }
    LoadedLevel {
        data: LevelData::default(),
        source: LevelSource::Fresh,
    }
}

/// 写 `level.dat`，照官方 `saveDataTag`：写临时文件 → 轮换 `level.dat_old`。
pub fn save_level_data(dir: &WorldDir, data: &LevelData) -> io::Result<()> {
    let bytes = data
        .to_gzip()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    safe_replace(&dir.level_dat(), &bytes, Some(&dir.level_dat_old()))
}

/// 写一份 `data/` saved data。官方 `SavedDataStorage` 的根是
/// `{ data: <codec 值>, DataVersion: <int> }`，gzip 压缩。
pub fn write_saved_data(dir: &WorldDir, id: &str, data: &Tag) -> io::Result<()> {
    let root = Tag::Compound(vec![
        ("data".to_owned(), data.clone()),
        ("DataVersion".to_owned(), Tag::Int(version::DATA_VERSION)),
    ]);
    let bytes = write_compressed("", &root, Compression::Gzip)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    safe_replace(&dir.saved_data(id), &bytes, None)
}

/// 读一份 `data/` saved data 的 `data` 内容；不存在或坏了返回 `None`。
pub fn read_saved_data(dir: &WorldDir, id: &str) -> Option<Tag> {
    let bytes = fs::read(dir.saved_data(id)).ok()?;
    let (_name, entries) =
        read_compressed(&bytes, Compression::Gzip, Accounter::uncompressed_quota()).ok()?;
    field(&Tag::Compound(entries), "data").cloned()
}

/// 全局 `data/` 下要写的独立文件（`docs/m3-storage.md` 9.3、Q9）。
/// `chunk_tickets.dat` 在维度的 `data/minecraft/` 下，不在这里。
pub const SAVED_DATA_IDS: &[&str] = &[
    "minecraft:game_rules",
    "minecraft:world_gen_settings",
    "minecraft:world_clocks",
    "minecraft:weather",
    "minecraft:scheduled_events",
    "minecraft:wandering_trader",
    "minecraft:custom_boss_events",
    "minecraft:ender_dragon_fight",
    "minecraft:world_border",
];

/// 给一份新世界写出全部 `data/` 独立文件，内容为空复合（各 codec 用默认值）。
/// 已经存在的文件不覆盖，保留上一次保存的值。
pub fn write_default_saved_data(dir: &WorldDir) -> io::Result<()> {
    for &id in SAVED_DATA_IDS {
        if !dir.saved_data(id).exists() {
            write_saved_data(dir, id, &Tag::Compound(Vec::new()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "bamboo-world-dir-{tag}-{}-{}",
                std::process::id(),
                TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&path);
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn layout_uses_the_26_3_paths() {
        let tmp = TempDir::new("layout");
        let dir = WorldDir::new(&tmp.0);
        dir.create_layout().unwrap();
        assert!(dir.root().is_dir());
        assert!(dir.region_dir().is_dir());
        assert!(dir.entities_dir().is_dir());
        assert!(dir.poi_dir().is_dir());
        assert!(dir.dimension_data_dir().is_dir());
        assert!(dir.players_data_dir().is_dir());
        assert_eq!(
            dir.saved_data("minecraft:game_rules"),
            tmp.0.join("data/minecraft/game_rules.dat")
        );
        assert_eq!(
            dir.chunk_tickets(),
            tmp.0
                .join("dimensions/minecraft/overworld/data/minecraft/chunk_tickets.dat")
        );
    }

    #[test]
    fn level_dat_round_trips_and_rotates_old() {
        let tmp = TempDir::new("level");
        let dir = WorldDir::new(&tmp.0);
        dir.create_layout().unwrap();

        assert_eq!(load_level_data(&dir).source, LevelSource::Fresh);

        let first = LevelData {
            time: 100,
            ..LevelData::default()
        };
        save_level_data(&dir, &first).unwrap();
        assert!(dir.level_dat().exists());
        assert_eq!(load_level_data(&dir).data.time, 100);

        let second = LevelData {
            time: 200,
            ..LevelData::default()
        };
        save_level_data(&dir, &second).unwrap();
        assert_eq!(load_level_data(&dir).data.time, 200);
        // 旧的 level.dat 应该轮换到 level.dat_old。
        assert!(dir.level_dat_old().exists());
        assert_eq!(read_level_file(&dir.level_dat_old()).unwrap().time, 100);
    }

    #[test]
    fn a_corrupt_primary_falls_back_to_old() {
        let tmp = TempDir::new("fallback");
        let dir = WorldDir::new(&tmp.0);
        dir.create_layout().unwrap();
        let data = LevelData {
            time: 777,
            ..LevelData::default()
        };
        save_level_data(&dir, &data).unwrap();
        // 造一份备份：再存一次把 777 挪到 _old。
        let newer = LevelData {
            time: 888,
            ..LevelData::default()
        };
        save_level_data(&dir, &newer).unwrap();
        // 打坏主文件，读取应退回 _old 的 777。
        fs::write(dir.level_dat(), b"not gzip nbt").unwrap();
        let loaded = load_level_data(&dir);
        assert_eq!(loaded.source, LevelSource::Fallback);
        assert_eq!(loaded.data.time, 777);
    }

    #[test]
    fn legacy_layout_is_reported_as_empty() {
        let tmp = TempDir::new("legacy");
        let dir = WorldDir::new(&tmp.0);
        fs::create_dir_all(tmp.0.join("region")).unwrap();
        let loaded = load_level_data(&dir);
        assert_eq!(loaded.source, LevelSource::Legacy);
        assert_eq!(loaded.data.time, 0);
    }

    #[test]
    fn saved_data_writes_the_official_envelope() {
        let tmp = TempDir::new("saved");
        let dir = WorldDir::new(&tmp.0);
        dir.create_layout().unwrap();
        let value = Tag::Compound(vec![("keepTerrain".to_owned(), Tag::Byte(0))]);
        write_saved_data(&dir, "minecraft:game_rules", &value).unwrap();
        assert_eq!(read_saved_data(&dir, "minecraft:game_rules"), Some(value));

        write_default_saved_data(&dir).unwrap();
        // 已经写过的 game_rules 不被默认值覆盖。
        assert_eq!(
            read_saved_data(&dir, "minecraft:game_rules"),
            Some(Tag::Compound(vec![(
                "keepTerrain".to_owned(),
                Tag::Byte(0)
            )]))
        );
        assert!(read_saved_data(&dir, "minecraft:weather").is_some());
    }

    #[test]
    fn session_lock_is_exclusive_and_writes_the_snowman() {
        let tmp = TempDir::new("lock");
        let dir = WorldDir::new(&tmp.0);
        let first = SessionLock::acquire(&dir).unwrap();
        // 第二个进程（这里是同一进程里的另一把锁）拿不到。
        assert!(SessionLock::acquire(&dir).is_err());
        drop(first);
        // 释放之后能读到内容，也能再拿锁。
        assert_eq!(fs::read(dir.session_lock()).unwrap(), SNOWMAN);
        assert!(SessionLock::acquire(&dir).is_ok());
    }
}
