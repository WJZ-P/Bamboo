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

use crate::{level_dat::LevelData, player_data::PlayerData};

/// `DirectoryLock` 写进 `session.lock` 的内容，官方是 UTF-8 的 "☃"。
const SNOWMAN: &[u8] = &[0xe2, 0x98, 0x83];

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// 官方 `Player.getStringUUID()`（`UUID.toString()`）：带连字符的小写 UUID
/// `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`。进服读档和离服写档都用它做文件名，
/// 免得两处格式不一致。
///
/// Official `Player.getStringUUID()` (`UUID.toString()`): the hyphenated lowercase
/// UUID. Both the join read and the leave write use this as the file stem so the
/// two paths cannot disagree.
pub fn player_uuid_string(bytes: &[u8; 16]) -> String {
    let h = |b: u8| format!("{b:02x}");
    format!(
        "{}{}{}{}-{}{}-{}{}-{}{}-{}{}{}{}{}{}",
        h(bytes[0]),
        h(bytes[1]),
        h(bytes[2]),
        h(bytes[3]),
        h(bytes[4]),
        h(bytes[5]),
        h(bytes[6]),
        h(bytes[7]),
        h(bytes[8]),
        h(bytes[9]),
        h(bytes[10]),
        h(bytes[11]),
        h(bytes[12]),
        h(bytes[13]),
        h(bytes[14]),
        h(bytes[15]),
    )
}

/// 官方 `FileNameDateFormatter`：`yyyy-MM-dd_HH-mm-ss`，给坏掉的玩家文件做备份名。
/// 官方用 `ZonedDateTime.now()`（本地时区）；这里按 UTC 的 epoch 秒算，只有备份文件名
/// 里的时刻会因时区不同，格式和归档行为一致（见 README 已知限制）。
///
/// Official `FileNameDateFormatter`: `yyyy-MM-dd_HH-mm-ss`, used for the corrupted
/// player-file backup name. Vanilla uses local time (`ZonedDateTime.now()`); here we
/// derive it from UTC epoch seconds, so only the clock in the backup name differs by
/// timezone. The format and archival behaviour match (see README known limitations).
fn file_name_stamp(secs_since_epoch: i64) -> String {
    let days = secs_since_epoch.div_euclid(86_400);
    let rem = secs_since_epoch.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}_{:02}-{:02}-{:02}",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60,
    )
}

/// 天数（相对 1970-01-01）→ 公历年月日，Howard Hinnant 的 civil_from_days。
/// Days since 1970-01-01 -> civil (year, month, day), Howard Hinnant's algorithm.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    if month <= 2 {
        year += 1;
    }
    (year, month, day)
}

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

    /// 维度自己的 `data/minecraft/`，`chunk_tickets.dat` 和 `world_border.dat` 放这里。
    /// 官方 `ServerChunkCache` 的 `dataFolder` 是维度目录下的 `data/`，
    /// `WorldBorder.TYPE` 的 id 是 `minecraft:world_border`。
    pub fn dimension_data_dir(&self) -> PathBuf {
        self.dimension_dir().join("data/minecraft")
    }

    /// 主世界的 `world_border.dat`，和官方 `forDataFileInDimension("overworld", ...)`
    /// 一致；全局 `data/` 下的游戏规则也在同一个 `data/` 树，但边界是按维度存的。
    /// The overworld `world_border.dat`, matching official
    /// `forDataFileInDimension("overworld", "minecraft/world_border.dat")`.
    pub fn world_border_dat(&self) -> PathBuf {
        self.dimension_data_dir().join("world_border.dat")
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

/// 官方 `Util.runWithRetries(10, ...)`：一步最多重试 10 次。
/// 闭包内部按官方顺序执行若干操作 + 最终检查，全成功才算这一步成功。
/// Official `Util.runWithRetries(10, ...)`: retry a step at most 10 times.
/// The closure runs the operations in sequence plus the final check.
const SAFE_REPLACE_RETRIES: usize = 10;

fn run_with_retries(mut step: impl FnMut() -> bool) -> bool {
    for _ in 0..SAFE_REPLACE_RETRIES {
        if step() {
            return true;
        }
    }
    false
}

/// 原子替换 `target`：写临时文件并刷盘 → 旧文件挪到 `backup`（若有）→
/// 删旧 `target` → 临时文件改名成 `target`。每一步都照官方
/// `Util.safeReplaceOrMoveFile` 最多重试 10 次；`replace` 失败时总是把备份
/// 挪回 `target` 再返回错误（官方 `safeReplaceFile` 传的 `noRollback=false`）。
/// `backup` 为 `None` 时也用一份临时备份回滚，绝不留下没有主文件的世界。
///
/// Atomic replace following official `Util.safeReplaceOrMoveFile`: each step
/// is retried up to 10 times; on a failed replace the backup is restored
/// (official `safeReplaceFile` passes `noRollback=false`; a `None` backup uses
/// a temporary one so the world is never left without its main file).
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

    // 1. create backup：官方只在 target 已经存在时做这一步。data/ 文件不写
    // 长期备份，但失败时要能回滚，所以这里用一份临时备份。
    let rollback_only = backup.is_none();
    let temporary_backup = dir.join(format!(
        "{}.rollback-{}",
        tmp.file_name().unwrap_or_default().to_string_lossy(),
        std::process::id()
    ));
    let effective_backup: Option<&Path> = match backup {
        Some(bk) => Some(bk),
        None => target.exists().then_some(temporary_backup.as_path()),
    };
    if target.exists()
        && let Some(bk) = effective_backup
    {
        let ok = run_with_retries(|| {
            let _ = fs::remove_file(bk); // createDeleter(backup)
            let _ = fs::rename(target, bk); // createRenamer(target, backup)
            bk.is_file() // createFileCreatedCheck(backup)
        });
        if !ok {
            let _ = fs::remove_file(&tmp);
            return Err(io::Error::other(format!(
                "could not back up {} to {}",
                target.display(),
                bk.display()
            )));
        }
    }

    // 2. remove old target：官方无条件执行（上一步已把 target 挪走时是空操作）。
    let removed = run_with_retries(|| {
        let _ = fs::remove_file(target); // createDeleter(target)
        !target.exists() // createFileDeletedCheck(target)
    });
    if !removed {
        if let Some(bk) = effective_backup {
            let _ = restore_backup(bk, target);
        }
        let _ = remove_rollback(&temporary_backup, rollback_only);
        let _ = fs::remove_file(&tmp);
        return Err(io::Error::other(format!(
            "could not remove old {}",
            target.display()
        )));
    }

    // 3. replace：临时文件改名成 target。
    let replaced = run_with_retries(|| {
        let _ = fs::rename(&tmp, target); // createRenamer(newPath, target)
        target.is_file() // createFileCreatedCheck(target)
    });
    if !replaced {
        // 回滚照官方 `safeReplaceFile`（noRollback=false）：从备份恢复，
        // 再报错。data/ 的临时备份恢复完就删掉。
        // Official rollback on a failed replace: restore the backup, then
        // report the failure (noRollback is always false here).
        if let Some(bk) = effective_backup {
            let _ = restore_backup(bk, target);
        }
        let _ = remove_rollback(&temporary_backup, rollback_only);
        let _ = fs::remove_file(&tmp);
        return Err(io::Error::other(format!(
            "could not replace {}",
            target.display()
        )));
    }
    let _ = remove_rollback(&temporary_backup, rollback_only);
    Ok(())
}

fn remove_rollback(path: &Path, temporary: bool) -> io::Result<()> {
    if temporary && path.exists() {
        fs::remove_file(path)
    } else {
        Ok(())
    }
}

/// 官方 rollback 那一步：`rename(backup → target)` 也最多重试 10 次。
fn restore_backup(bk: &Path, target: &Path) -> bool {
    run_with_retries(|| {
        let _ = fs::rename(bk, target);
        target.is_file()
    })
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

/// 编出 `level.dat` 的 gzip 字节。世界线程只编码，写盘交给 IO 线程。
/// Encode `level.dat` to gzip bytes; the world thread only encodes.
pub fn encode_level_data(data: &LevelData) -> io::Result<Vec<u8>> {
    data.to_gzip()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
}

/// 写 `level.dat`，照官方 `saveDataTag`：写临时文件 → 轮换 `level.dat_old`。
/// 测试用；运行中的保存走 [`encode_level_data`] + IO 线程。
/// Write `level.dat` (startup/tests); run-time saves use `encode_level_data` + IO.
pub fn save_level_data(dir: &WorldDir, data: &LevelData) -> io::Result<()> {
    safe_replace(
        &dir.level_dat(),
        &encode_level_data(data)?,
        Some(&dir.level_dat_old()),
    )
}

/// 编出官方 `players/data/<uuid>.dat` 的 gzip 字节。写盘交给 IO 线程。
pub fn encode_player_data(data: &PlayerData) -> io::Result<Vec<u8>> {
    data.to_gzip()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
}

/// 玩家存档的主文件和备份文件路径，交给 IO 线程做 `_old` 轮换。
pub fn player_paths(dir: &WorldDir, uuid: &str) -> (PathBuf, PathBuf) {
    (dir.player_dat(uuid), dir.player_dat_old(uuid))
}

/// 读玩家存档的原始 gzip 字节（连接层做 IO，世界线程再解析）。
/// 主文件坏了就另存 `<uuid>_corrupted_<毫秒>.dat` 并退回 `<uuid>.dat_old`（官方 §8）。
/// 文件不存在或都坏了返回 `None`，按新玩家进服。
pub fn read_player_bytes(dir: &WorldDir, uuid: &str) -> Option<Vec<u8>> {
    let primary = dir.player_dat(uuid);
    match fs::read(&primary) {
        Ok(bytes) => {
            if PlayerData::from_gzip(&bytes).is_ok() {
                return Some(bytes);
            }
            backup_corrupted_player(dir, uuid, &primary);
            read_valid_player_old(dir, uuid)
        }
        Err(_) => read_valid_player_old(dir, uuid),
    }
}

fn read_valid_player_old(dir: &WorldDir, uuid: &str) -> Option<Vec<u8>> {
    let bytes = fs::read(dir.player_dat_old(uuid)).ok()?;
    if PlayerData::from_gzip(&bytes).is_ok() {
        Some(bytes)
    } else {
        None
    }
}

fn backup_corrupted_player(dir: &WorldDir, uuid: &str, primary: &Path) {
    // 官方 `PlayerDataStorage.backup`：`<uuid>_corrupted_<FileNameDateFormatter 日期>.dat`。
    // Official `PlayerDataStorage.backup`: `<uuid>_corrupted_<date>.dat`.
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0);
    let backup = dir
        .players_data_dir()
        .join(format!("{uuid}_corrupted_{}.dat", file_name_stamp(secs)));
    let _ = fs::copy(primary, backup);
}

/// 写一份 `data/` saved data。官方 `SavedDataStorage` 的根是
/// `{ data: <codec 值>, DataVersion: <int> }`，gzip 压缩。
pub fn write_saved_data(dir: &WorldDir, id: &str, data: &Tag) -> io::Result<()> {
    write_saved_data_at(&dir.saved_data(id), data)
}

/// 编出官方 `SavedDataStorage.encodeUnchecked` 的 gzip 字节：根是
/// `{ data: <codec 值>, DataVersion: <int> }`。世界线程只编码，写盘交给 IO 线程。
/// Encode the official `SavedDataStorage.encodeUnchecked` envelope to gzip bytes.
/// The world thread only encodes; the IO thread writes.
pub fn encode_saved_data(data: &Tag) -> io::Result<Vec<u8>> {
    let root = Tag::Compound(vec![
        ("data".to_owned(), data.clone()),
        ("DataVersion".to_owned(), Tag::Int(version::DATA_VERSION)),
    ]);
    write_compressed("", &root, Compression::Gzip)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
}

/// 按绝对路径写一份 saved data。边界（维度目录）和全局 `data/` 共用这一层。
/// 测试/启动用；运行中的保存走 [`encode_saved_data`] + IO 线程。
/// Write a saved-data file at an absolute path. Used at startup and in tests;
/// run-time saves go through `encode_saved_data` + the IO thread.
pub fn write_saved_data_at(path: &Path, data: &Tag) -> io::Result<()> {
    safe_replace(path, &encode_saved_data(data)?, None)
}

/// 读一份 `data/` saved data 的 `data` 内容；不存在或坏了返回 `None`。
pub fn read_saved_data(dir: &WorldDir, id: &str) -> Option<Tag> {
    read_saved_data_at(&dir.saved_data(id))
}

/// 按绝对路径读一份 saved data 的 `data` 内容；不存在或坏了返回 `None`。
/// Read a saved-data `data` compound from an absolute path; `None` on missing/bad.
pub fn read_saved_data_at(path: &Path) -> Option<Tag> {
    let bytes = fs::read(path).ok()?;
    let (_name, entries) =
        read_compressed(&bytes, Compression::Gzip, Accounter::uncompressed_quota()).ok()?;
    field(&Tag::Compound(entries), "data").cloned()
}

/// 补 10：Bamboo 没建模的 saved data（`weather`、`world_gen_settings` 等）**不写空
/// `{}` 文件。官方 `SavedDataStorage.computeIfAbsent` 找不到文件就用默认构造，
/// 「没有就不写」是安全的；写空 `{}` 反而可能被官方 codec 当成坏档。已经存在的
/// 这类文件（旧世界或官方服务端写的）原样保留，读盘时照常处理。
/// Item 10: unmodelled saved data is NOT written as empty `{}` files. Official
/// `SavedDataStorage.computeIfAbsent` constructs defaults when a file is
/// missing, so not writing is safe; an empty `{}` could be read as corrupt by
/// an official codec. Existing such files are left untouched.
///
/// Only `level.dat`, `game_rules.dat`, `world_border.dat` and `chunk_tickets.dat`
/// are written, all via `SaveSession::encode_saves`.
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

    /// F8：官方 `Util.runWithRetries(10, ...)` 的步数：最多 10 次，
    /// 第 10 次成功就算成功，一直失败返回 false 且正好试 10 次。
    /// F8: official `Util.runWithRetries(10, ...)` — at most 10 attempts;
    /// succeeding on the 10th counts, persistent failure tries exactly 10.
    #[test]
    fn run_with_retries_tries_at_most_ten_times() {
        let mut attempts = 0;
        assert!(run_with_retries(|| {
            attempts += 1;
            attempts >= 10
        }));
        assert_eq!(attempts, 10);

        let mut attempts = 0;
        assert!(!run_with_retries(|| {
            attempts += 1;
            false
        }));
        assert_eq!(attempts, 10);
    }

    /// F8：备份这一步重试 10 次仍失败时，`safe_replace` 报错，
    /// 原主文件原封不动，临时文件也清掉（绝不留下半写的世界）。
    /// F8: when the backup step fails through all 10 retries, `safe_replace`
    /// returns Err, the original file is untouched and the temp file is gone.
    #[test]
    fn a_failed_backup_leaves_the_original_intact() {
        let tmp = TempDir::new("retry-backup");
        fs::create_dir_all(&tmp.0).unwrap();
        let target = tmp.0.join("level.dat");
        fs::write(&target, b"original").unwrap();
        // 备份路径的父目录是个普通文件：rename 必然失败，10 次都不行。
        let blocker = tmp.0.join("blocker");
        fs::write(&blocker, b"not a directory").unwrap();
        let backup = blocker.join("level.dat_old");

        assert!(safe_replace(&target, b"new", Some(&backup)).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"original");
        assert!(
            fs::read_dir(&tmp.0)
                .unwrap()
                .flatten()
                .all(|entry| !entry.file_name().to_string_lossy().contains(".tmp")),
            "the temporary file must be cleaned up"
        );
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
        // 补 10：建目录不会凭空写出未建模的 saved data，官方 computeIfAbsent 会自己造默认值。
        assert!(
            read_saved_data(&dir, "minecraft:weather").is_none(),
            "unmodelled saved data must not be written as empty files"
        );
    }

    /// 官方 `Player.getStringUUID()` 带连字符，玩家文件名必须是这个形式。
    /// Official `Player.getStringUUID()` is hyphenated; the file stem must be too.
    #[test]
    fn player_file_uses_the_hyphenated_uuid() {
        let bytes = [
            0x06, 0x9a, 0x79, 0xf4, 0x44, 0xe9, 0x47, 0x26, 0xa5, 0xbe, 0xfc, 0xa9, 0x0e, 0x38,
            0xaa, 0xf5,
        ];
        let uuid = player_uuid_string(&bytes);
        assert_eq!(uuid, "069a79f4-44e9-4726-a5be-fca90e38aaf5");
        let dir = WorldDir::new("world");
        assert_eq!(
            dir.player_dat(&uuid),
            PathBuf::from("world/players/data/069a79f4-44e9-4726-a5be-fca90e38aaf5.dat")
        );
        assert_eq!(
            dir.player_dat_old(&uuid),
            PathBuf::from("world/players/data/069a79f4-44e9-4726-a5be-fca90e38aaf5.dat_old")
        );
    }

    /// 坏文件的备份名照 `FileNameDateFormatter`：`yyyy-MM-dd_HH-mm-ss`，不是毫秒。
    /// The corrupted-backup name follows `FileNameDateFormatter` (a date, not millis).
    #[test]
    fn corrupted_backup_name_uses_the_official_date_format() {
        // 1970-01-01 00:00:00 UTC 和 2026-11-12 各一个时刻，钉住格式。
        // Two instants pin the format: epoch and a known date.
        assert_eq!(file_name_stamp(0), "1970-01-01_00-00-00");
        let secs = 1_763_000_000;
        let stamp = file_name_stamp(secs);
        assert_eq!(stamp.len(), 19, "yyyy-MM-dd_HH-mm-ss");
        assert_eq!(&stamp[4..5], "-");
        assert_eq!(&stamp[7..8], "-");
        assert_eq!(&stamp[10..11], "_");
        assert_eq!(&stamp[13..14], "-");
        assert_eq!(&stamp[16..17], "-");

        let tmp = TempDir::new("corrupt-name");
        let dir = WorldDir::new(&tmp.0);
        dir.create_layout().unwrap();
        let uuid = player_uuid_string(&[0xab; 16]);
        let (main, old) = player_paths(&dir, &uuid);
        let good = PlayerData::default();
        let good_bytes = encode_player_data(&good).unwrap();
        safe_replace(&main, &good_bytes, Some(&old)).unwrap();
        fs::write(&main, b"broken").unwrap();

        assert_eq!(read_player_bytes(&dir, &uuid), None);
        let backups: Vec<String> = fs::read_dir(dir.players_data_dir())
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains("_corrupted_"))
            .collect();
        assert_eq!(backups.len(), 1, "坏文件要另存一份");
        assert!(
            backups[0].starts_with(&format!("{uuid}_corrupted_")),
            "备份名以带连字符的 uuid 开头：{}",
            backups[0]
        );
        assert!(backups[0].ends_with(".dat"), "备份名以 .dat 结尾");
        // 中间是 yyyy-MM-dd_HH-mm-ss，不含毫秒那种纯数字串。
        let middle = &backups[0][format!("{uuid}_corrupted_").len()..backups[0].len() - 4];
        assert_eq!(middle.len(), 19, "日期段是 19 个字符：{middle}");
        assert_eq!(&middle[10..11], "_", "日期和时间之间是下划线");
    }

    #[test]
    fn player_data_round_trips_with_old_rotation() {
        let tmp = TempDir::new("player");
        let dir = WorldDir::new(&tmp.0);
        dir.create_layout().unwrap();
        let uuid = "01234567-89ab-cdef-0123-456789abcdef";
        let (main, old) = player_paths(&dir, uuid);

        let first = PlayerData {
            position: [1.0, 2.0, 3.0],
            ..PlayerData::default()
        };
        let first_bytes = encode_player_data(&first).unwrap();
        safe_replace(&main, &first_bytes, Some(&old)).unwrap();
        assert_eq!(read_player_bytes(&dir, uuid), Some(first_bytes));

        let second = PlayerData {
            position: [9.0, 9.0, 9.0],
            ..PlayerData::default()
        };
        let second_bytes = encode_player_data(&second).unwrap();
        safe_replace(&main, &second_bytes, Some(&old)).unwrap();
        assert_eq!(read_player_bytes(&dir, uuid), Some(second_bytes));
        assert_eq!(
            PlayerData::from_gzip(&fs::read(&old).unwrap())
                .unwrap()
                .position,
            [1.0, 2.0, 3.0]
        );
    }

    #[test]
    fn a_corrupt_player_file_is_backed_up_and_falls_back_to_old() {
        let tmp = TempDir::new("player-corrupt");
        let dir = WorldDir::new(&tmp.0);
        dir.create_layout().unwrap();
        let uuid = "aaaa";
        let (main, old) = player_paths(&dir, uuid);
        let good = PlayerData {
            position: [7.0, 7.0, 7.0],
            ..PlayerData::default()
        };
        let good_bytes = encode_player_data(&good).unwrap();
        safe_replace(&main, &good_bytes, Some(&old)).unwrap();
        // 再写一次把 good 挪到 _old，然后打坏主文件。
        safe_replace(&main, &good_bytes, Some(&old)).unwrap();
        fs::write(&main, b"broken").unwrap();

        assert_eq!(read_player_bytes(&dir, uuid), Some(good_bytes));
        let backups: Vec<_> = fs::read_dir(dir.players_data_dir())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().contains("corrupted"))
            .collect();
        assert_eq!(backups.len(), 1, "坏文件要另存一份");
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
