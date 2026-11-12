//! 区块区域文件的 IO 线程：读写 `region/` 和 `entities/`，压缩/解压都在这条线程。
//!
//! 世界线程绝不等待 IO：读请求只是塞进 `requests` 通道，结果在下一刻开头由
//! `try_recv` 从 `results` 信箱收回。写请求是即发即忘；`flush` 的**阻塞**栅栏
//! 只在关服使用，`save-all` 用不阻塞的 `Barrier` 回执确认写都落盘（F3）。
//!
//! 磁盘上是官方格式：`region/r.x.z.mca` 存区块 NBT，`entities/r.x.z.mca` 存实体 NBT，
//! 都是 `RegionFile`（zlib 压缩）。世界线程交给这里的是**未压缩的 NBT 字节**，
//! 解析和序列化在世界线程上做（见 `mc-bamboo-game` 的接入层）。
//!
//! The IO thread for chunk region files: reads and writes `region/` and `entities/`,
//! with compression/decompression on this thread. The world thread never waits for
//! IO: a read request is only pushed into `requests`, and the result is collected
//! from the `results` mailbox at the start of the next tick. Writes are fire-and-
//! forget; the blocking `flush` barrier is shutdown-only, while `save-all` uses
//! the non-blocking `Barrier` ack to confirm its writes landed (F3).

use std::{
    collections::{HashMap, HashSet},
    fs, io,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread::{self, JoinHandle},
    time::{SystemTime, UNIX_EPOCH},
};

use mc_bamboo_core::ChunkPos;

use crate::{
    anvil::{RegionCompression, RegionFile, RegionPos, open_region},
    world_dir::{WorldDir, player_paths, safe_replace},
};

/// 一次区域文件读的配额上限，和 NBT 解压上限一致。
const READ_LIMIT: usize = 1 << 24;
/// 打开的区域文件缓存上限，超过就整体关掉重开（设计稿 LRU 256 的简化版）。
const CACHE_LIMIT: usize = 256;

/// 世界线程发给 IO 线程的请求。
enum Request {
    Read {
        pos: ChunkPos,
        task_id: u64,
        generation: u64,
    },
    Write {
        pos: ChunkPos,
        seq: u64,
        chunk: Option<Vec<u8>>,
        entities: Option<Vec<u8>>,
        timestamp: i32,
    },
    /// 某个区块有了更新的快照：丢掉还没落盘的旧 seq。
    DropSnapshot {
        pos: ChunkPos,
        seq: u64,
    },
    /// 写一个独立文件（玩家数据等），`backup` 非空时照官方做 `_old` 轮换。
    /// `player` 非空时这是一份玩家存档（带连字符 uuid 和一次写的序号）：写完后回一条
    /// `PlayerWritten`，世界线程据此丢掉内存里"已交给 IO 但还没落盘"的副本。
    /// When `player` is set this is a player save (hyphenated uuid + write seq):
    /// after the write the worker replies `PlayerWritten`, so the world thread
    /// can drop its in-memory "handed to IO but not yet on disk" copy.
    WriteFile {
        path: PathBuf,
        backup: Option<PathBuf>,
        bytes: Vec<u8>,
        player: Option<(String, u64)>,
    },
    /// 栅栏：确认此前所有请求都处理完，并对所有打开的区域文件 `sync_all`。
    /// 回执带上刷盘结果，写失败不当成功。
    /// Barrier: wait for all earlier requests and `sync_all` open region files;
    /// the reply carries the sync result so a failure is not treated as success.
    Flush {
        done: Sender<io::Result<()>>,
    },
    /// 异步栅栏：处理到它时把回执送进 `results` 信箱，不阻塞世界线程。
    /// `save-all flush` 用它等 IO 线程「此前所有写都落盘」再发成功消息。
    /// An async barrier: when reached, reply through the results mailbox instead
    /// of blocking the world thread. `save-all flush` waits for this ack.
    Barrier {
        seq: u64,
    },
    /// 测试接缝：让 IO 线程 panic，供世界线程验证「发现 → 关服」路径。
    /// 正常运行时没有任何调用方。
    /// Test seam: panic the IO thread so the world thread's detect-and-shutdown
    /// path can be tested. Nothing sends this during normal operation.
    Crash,
    Shutdown,
}

/// IO 线程送回世界线程信箱的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    /// 区块读完成。`chunk` 为 `None` 表示盘上没有这个区块；`error` 表示读坏了。
    Read {
        pos: ChunkPos,
        task_id: u64,
        generation: u64,
        chunk: Option<Vec<u8>>,
        entities: Option<Vec<u8>>,
        error: Option<String>,
    },
    /// 一次写已经落盘，世界线程可以丢掉对应的写盘途中缓冲，并按 `present`
    /// 把 `known` 置成盘上真实状态（写成功之前不许当成盘上有）。
    /// A write landed: the world thread drops the in-progress buffer and updates
    /// `known` to the true on-disk state. `known` must not be set before success.
    Written {
        pos: ChunkPos,
        seq: u64,
        present: bool,
    },
    /// 一份玩家存档已经落盘（成功或失败），世界线程据此结束这次"待写"。
    /// A player save finished (ok or not); the world thread ends that pending write.
    PlayerWritten {
        uuid: String,
        seq: u64,
        ok: bool,
        error: Option<String>,
    },
    /// 一次写盘或刷盘失败。带路径和原因，世界线程据此 `error!` 记录、计数、
    /// 保留 `parking` 盒子，连续失败就停止接收新保存请求。区块写带上 `pos/seq`，
    /// 独立文件写为 `None`。`path` 是失败的文件路径文本。
    /// A write or sync failed: the world thread logs it, keeps the parked box and
    /// stops accepting new saves after consecutive failures. Chunk writes carry
    /// `pos/seq`; standalone file writes use `None`.
    WriteFailed {
        pos: Option<ChunkPos>,
        seq: Option<u64>,
        path: String,
        error: String,
    },
    /// 一道异步栅栏之前的所有写都已处理完并 `sync_all`。`seq` 对应提交时的序号；
    /// `ok` 是这段窗口里有没有写/刷盘失败（`save-all` 据此报成功或失败）。
    /// All writes before an async barrier have been processed and synced; `seq`
    /// pairs with the submitted barrier and `ok` says whether any write in that
    /// window failed (`save-all` reports success or failure from it).
    Flushed { seq: u64, ok: bool },
}

/// 一条 IO 线程的句柄。世界线程持有，非阻塞提交、tick 开头收信箱。
#[derive(Debug)]
pub struct ChunkIo {
    dir: WorldDir,
    /// 盘上已经有区块数据的坐标。启动扫一次区域文件头，写盘时更新。
    /// 世界线程只读这个内存集合来判断「盘上有没有」，不做同步磁盘 IO。
    /// Chunk positions that already have data on disk. Scanned from region
    /// headers at startup and updated on writes, so the world thread can ask
    /// "is this on disk?" without doing synchronous IO.
    known: Arc<Mutex<HashSet<ChunkPos>>>,
    /// 已提交、还没收到写完回执的区块。读请求要把它当成「盘上会有」，
    /// 因为写请求和之后的读请求在同一 FIFO 信箱里、写先处理；但 `known`
    /// 里绝不提前登记，写失败时这条也清掉，避免 `has_chunk` 撒谎。
    /// Chunks submitted but not yet acked. A read queued behind a write in the
    /// same FIFO mailbox must see them, but `known` (on-disk truth) is not set
    /// until the write succeeds.
    inflight: Mutex<HashSet<ChunkPos>>,
    requests: Sender<Request>,
    results: Receiver<Reply>,
    worker: Option<JoinHandle<()>>,
    /// 玩家存档写的序号，交给 IO 线程回执时原样带回，用来只丢弃「最新一次」的待写副本。
    /// Sequence number for player saves; echoed by the IO thread so the world
    /// thread only drops the newest in-flight copy.
    next_player_seq: AtomicU64,
    /// 异步栅栏的序号，和玩家写分开计数；回执原样带回用来配对等待的消息。
    /// Sequence number for async barriers; echoed to pair with waiting messages.
    next_barrier_seq: AtomicU64,
}

impl ChunkIo {
    /// 起一条 IO 线程。`dir` 的布局由调用方（`SaveSession`）预先建好。
    pub fn open(dir: &WorldDir) -> io::Result<Self> {
        fs::create_dir_all(dir.region_dir())?;
        fs::create_dir_all(dir.entities_dir())?;
        let layout = dir.clone();
        let known = Arc::new(Mutex::new(scan_existing_chunks(&dir.region_dir())));
        let (requests, rx) = mpsc::channel::<Request>();
        let (tx, results) = mpsc::channel::<Reply>();
        let worker = thread::Builder::new()
            .name("bamboo-chunk-io".to_owned())
            .spawn(move || worker_loop(layout, rx, tx))?;
        Ok(Self {
            dir: dir.clone(),
            known,
            inflight: Mutex::new(HashSet::new()),
            requests,
            results,
            worker: Some(worker),
            next_player_seq: AtomicU64::new(1),
            next_barrier_seq: AtomicU64::new(1),
        })
    }

    /// 盘上是否已经有这个区块的数据（或有一份已经入队、还没回执的写）。
    /// 只查内存集合，不碰磁盘。`known` 只在写成功后更新。
    pub fn has_chunk(&self, pos: ChunkPos) -> bool {
        self.known
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(&pos)
            || self
                .inflight
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .contains(&pos)
    }

    /// 写回执到达：更新盘上真实状态，清掉在途标记。世界线程调用。
    /// A write ack arrived: update the true on-disk state and clear inflight.
    /// Called by the world thread.
    pub fn set_known(&self, pos: ChunkPos, present: bool) {
        {
            let mut known = self.known.lock().unwrap_or_else(|e| e.into_inner());
            if present {
                known.insert(pos);
            } else {
                known.remove(&pos);
            }
        }
        self.inflight
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&pos);
    }

    /// 写失败：清掉在途标记，盘上状态不变。世界线程调用。
    /// A write failed: clear inflight but leave the on-disk truth unchanged.
    pub fn drop_inflight(&self, pos: ChunkPos) {
        self.inflight
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&pos);
    }

    /// 提交一个异步读请求，世界线程不等待。
    pub fn submit_read(&self, pos: ChunkPos, task_id: u64, generation: u64) {
        let _ = self.requests.send(Request::Read {
            pos,
            task_id,
            generation,
        });
    }

    /// 提交一个写请求。`chunk` / `entities` 为 `None` 表示删掉对应文件里的这一格。
    /// 这里**不**更新 `known`（盘上真实状态），只登记在途；世界线程收到
    /// `Written` 回执才 `set_known`。写失败时 `has_chunk` 不会撒谎。
    /// This does NOT update `known` (on-disk truth); only registers the in-flight
    /// write. The world thread calls `set_known` on the `Written` ack, so a failed
    /// write never makes `has_chunk` lie.
    pub fn submit_write(
        &self,
        pos: ChunkPos,
        seq: u64,
        chunk: Option<Vec<u8>>,
        entities: Option<Vec<u8>>,
    ) {
        self.inflight
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(pos);
        let _ = self.requests.send(Request::Write {
            pos,
            seq,
            chunk,
            entities,
            timestamp: unix_seconds(),
        });
    }

    /// 有了更新的快照，通知 IO 线程可以跳过旧 seq 的写。
    pub fn drop_snapshot(&self, pos: ChunkPos, seq: u64) {
        let _ = self.requests.send(Request::DropSnapshot { pos, seq });
    }

    /// 提交一个独立文件写（玩家数据等）。`backup` 非空时轮换旧文件到它。
    /// 和区域文件共用一条 IO 线程、共用 `flush` 栅栏。
    pub fn submit_file_write(&self, path: PathBuf, backup: Option<PathBuf>, bytes: Vec<u8>) {
        let _ = self.requests.send(Request::WriteFile {
            path,
            backup,
            bytes,
            player: None,
        });
    }

    /// 写 `players/data/<uuid>.dat`，照官方 `PlayerDataStorage.save` 轮换 `.dat_old`。
    /// 返回这次写的序号；IO 线程写完会回一条 `PlayerWritten`，世界线程据此丢掉
    /// 内存里"已交给 IO、还没落盘"的副本，避免离开后立刻重进读到旧档。
    /// Returns this write's seq. The IO thread replies `PlayerWritten` on
    /// completion; the world thread uses it to drop the in-flight in-memory
    /// copy so an immediate rejoin never reads stale disk bytes.
    pub fn submit_player_write(&self, uuid: &str, bytes: Vec<u8>) -> u64 {
        let seq = self.next_player_seq.fetch_add(1, Ordering::Relaxed);
        let (path, backup) = player_paths(&self.dir, uuid);
        let _ = self.requests.send(Request::WriteFile {
            path,
            backup: Some(backup),
            bytes,
            player: Some((uuid.to_owned(), seq)),
        });
        seq
    }

    /// 非阻塞地收一条结果；世界线程在 tick 开头循环调用到 `None`。
    pub fn try_recv(&self) -> Option<Reply> {
        self.results.try_recv().ok()
    }

    /// IO 线程是不是已经退出（panic 或通道断开）。世界线程据此当成致命错误走关服。
    /// Whether the IO thread has quit (panic or a disconnected channel); the
    /// world thread treats this as a fatal error and shuts down.
    pub fn worker_stopped(&self) -> bool {
        self.worker
            .as_ref()
            .is_some_and(|worker| worker.is_finished())
    }

    /// 阻塞栅栏：等此前所有请求处理完并 `sync_all`。**只允许关服使用**（设计稿
    /// 明确允许世界线程等待 IO 的唯一一处）；`save-all` 一律走不阻塞的 `Barrier`
    /// 回执。刷盘或写盘失败会作为 `Err` 返回，不当成功。
    /// Blocking barrier: wait for all earlier requests and `sync_all`.
    /// **Shutdown only** (the single design-approved wait point); `save-all`
    /// always uses the non-blocking `Barrier` ack. A sync or write failure is
    /// returned as `Err`, never silently treated as success.
    pub fn flush(&self) -> io::Result<()> {
        let (done, wait) = mpsc::channel();
        self.requests
            .send(Request::Flush { done })
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "chunk IO thread stopped"))?;
        wait.recv().map_err(|_| {
            io::Error::new(
                io::ErrorKind::BrokenPipe,
                "chunk IO thread stopped during flush",
            )
        })?
    }

    /// 测试接缝：让 IO 线程 panic。正常代码不要调用。
    /// Test seam: panic the IO thread. Do not call from production code.
    #[doc(hidden)]
    pub fn crash_for_test(&self) {
        let _ = self.requests.send(Request::Crash);
    }

    /// 排一道不阻塞的栅栏并返回它的序号。IO 线程按 FIFO 处理到它时，此前提交的所有
    /// 写都已落盘且区域文件已 `sync_all`，随后往 `results` 信箱回一条 `Flushed { seq }`。
    /// 世界线程不等待，在 tick 开头收信箱时据此发 `save-all flush` 的成功消息。
    ///
    /// Enqueue a non-blocking barrier and return its seq. When the IO thread
    /// reaches it (FIFO), every earlier write has landed and been synced; it then
    /// replies `Flushed { seq }` to the results mailbox. The world thread never
    /// waits: it collects the ack at the start of a tick.
    pub fn submit_barrier(&self) -> u64 {
        let seq = self.next_barrier_seq.fetch_add(1, Ordering::Relaxed);
        let _ = self.requests.send(Request::Barrier { seq });
        seq
    }

    /// 关掉 IO 线程。先排一道栅栏，保证写都落盘。
    pub fn close(&mut self) {
        let _ = self.flush();
        let _ = self.requests.send(Request::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for ChunkIo {
    fn drop(&mut self) {
        self.close();
    }
}

fn unix_seconds() -> i32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i32)
        .unwrap_or(0)
}

/// 启动扫一遍 `region/` 里已有 `.mca` 的头，得到盘上已经有数据的区块。
/// 只读 offset 表，不解压任何区块；这一步在启动时做一次。
/// Scan existing `.mca` headers at startup to learn which chunks already have
/// data on disk; reads only offset tables, no decompression.
fn scan_existing_chunks(region_dir: &Path) -> HashSet<ChunkPos> {
    let mut out = HashSet::new();
    let Ok(entries) = fs::read_dir(region_dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(inner) = name.strip_prefix("r.").and_then(|s| s.strip_suffix(".mca")) else {
            continue;
        };
        let mut parts = inner.split('.');
        let (Some(x), Some(z)) = (parts.next(), parts.next()) else {
            continue;
        };
        let (Ok(rx), Ok(rz)) = (x.parse::<i32>(), z.parse::<i32>()) else {
            continue;
        };
        let Ok(region) = RegionFile::open(entry.path(), region_dir, RegionCompression::Zlib) else {
            continue;
        };
        for index in 0..1024_i32 {
            let pos = ChunkPos::new(rx * 32 + index % 32, rz * 32 + index / 32);
            if region.has_chunk(pos) {
                out.insert(pos);
            }
        }
    }
    out
}

/// 区域文件缓存。`kind` 0 = `region/`，1 = `entities/`。
#[derive(Default)]
struct Cache {
    files: HashMap<(u8, RegionPos), RegionFile>,
}

impl Cache {
    fn file(&mut self, dir: &WorldDir, kind: u8, pos: ChunkPos) -> io::Result<&mut RegionFile> {
        let key = (kind, RegionPos::of(pos));
        if !self.files.contains_key(&key) {
            if self.files.len() >= CACHE_LIMIT {
                // 简化版 LRU：满了就整体关掉，下次按需重开。写已经各自落盘。
                let mut old = std::mem::take(&mut self.files);
                for region in old.values_mut() {
                    let _ = region.sync();
                }
            }
            let directory = if kind == 0 {
                dir.region_dir()
            } else {
                dir.entities_dir()
            };
            let region = open_region(&directory, key.1, RegionCompression::Zlib)
                .map_err(|e| io::Error::other(e.to_string()))?;
            self.files.insert(key, region);
        }
        Ok(self.files.get_mut(&key).expect("刚插入"))
    }

    /// 刷盘并返回失败的 `(路径, 原因)`；由 worker 送回世界线程记 `error!`。
    /// Sync every open region file; return `(path, reason)` for each failure so
    /// the worker can report it to the world thread.
    fn sync_all(&mut self, dir: &WorldDir) -> Vec<(String, String)> {
        let mut failures = Vec::new();
        for ((kind, region_pos), region) in &mut self.files {
            if let Err(error) = region.sync() {
                let directory = if *kind == 0 {
                    dir.region_dir()
                } else {
                    dir.entities_dir()
                };
                failures.push((
                    directory.join(region_pos.file_name()).display().to_string(),
                    error.to_string(),
                ));
            }
        }
        failures
    }
}

/// 把一批写/刷盘失败送回世界线程信箱，`pos/seq` 只对区块写非空。
/// Send write/sync failures back to the world thread; `pos/seq` are set for chunks.
fn report_failures(
    tx: &Sender<Reply>,
    failures: Vec<(String, String)>,
    pos: Option<ChunkPos>,
    seq: Option<u64>,
) -> bool {
    let mut any = false;
    for (path, error) in failures {
        any = true;
        let _ = tx.send(Reply::WriteFailed {
            pos,
            seq,
            path,
            error,
        });
    }
    any
}

fn worker_loop(dir: WorldDir, rx: Receiver<Request>, tx: Sender<Reply>) {
    let mut cache = Cache::default();
    // 每个区块最新快照的 seq，用于丢掉还没落盘的旧快照。
    let mut latest: HashMap<ChunkPos, u64> = HashMap::new();
    // 上一道栅栏之后有没有写/刷盘失败：栅栏回执据此带 ok=false（F9）。
    // Whether any write/sync failed since the last barrier; the ack carries it.
    let mut failed_since_barrier = false;
    while let Ok(first) = rx.recv() {
        // 一次收齐当前排队的所有请求：先登记本批里每个区块最新的 seq，
        // 再逐个处理，这样同一批里同区块的旧快照会被跳过。
        // Drain all queued requests first, register the newest seq per chunk for
        // this batch, then process: stale snapshots in the same batch are skipped.
        let mut batch = vec![first];
        batch.extend(rx.try_iter());
        for request in &batch {
            match request {
                Request::Write { pos, seq, .. } | Request::DropSnapshot { pos, seq } => {
                    let current = latest.entry(*pos).or_insert(*seq);
                    if *seq > *current {
                        *current = *seq;
                    }
                }
                _ => {}
            }
        }
        for request in batch {
            match request {
                Request::Read {
                    pos,
                    task_id,
                    generation,
                } => {
                    let (chunk, entities, error) = read_one(&mut cache, &dir, pos);
                    let _ = tx.send(Reply::Read {
                        pos,
                        task_id,
                        generation,
                        chunk,
                        entities,
                        error,
                    });
                }
                Request::Write {
                    pos,
                    seq,
                    chunk,
                    entities,
                    timestamp,
                } => {
                    if latest.get(&pos) == Some(&seq) {
                        let present = chunk.is_some();
                        let failures = write_one(&mut cache, &dir, pos, chunk, entities, timestamp);
                        // 只有两份都写成功才回 Written；失败绝不谎报成功，
                        // 回带 pos/seq 的 WriteFailed，世界线程据此留着 parking 盒子。
                        // Only ack on total success; never report a failed write
                        // as Written. The failure carries pos/seq so the world
                        // thread keeps the parked box and stops accepting saves.
                        if failures.is_empty() {
                            let _ = tx.send(Reply::Written { pos, seq, present });
                            latest.remove(&pos);
                        } else if report_failures(&tx, failures, Some(pos), Some(seq)) {
                            failed_since_barrier = true;
                        }
                    }
                }
                Request::DropSnapshot { .. } => {}
                Request::WriteFile {
                    path,
                    backup,
                    bytes,
                    player,
                } => {
                    let result = safe_replace(&path, &bytes, backup.as_deref());
                    if let Err(error) = result.as_ref()
                        && report_failures(
                            &tx,
                            vec![(path.display().to_string(), error.to_string())],
                            None,
                            None,
                        )
                    {
                        failed_since_barrier = true;
                    }
                    if let Some((uuid, seq)) = player {
                        let error = result.as_ref().err().map(|e| e.to_string());
                        let ok = result.is_ok();
                        let _ = tx.send(Reply::PlayerWritten {
                            uuid,
                            seq,
                            ok,
                            error,
                        });
                    }
                }
                Request::Flush { done } => {
                    let failures = cache.sync_all(&dir);
                    let sync_failed = report_failures(&tx, failures.clone(), None, None);
                    let failed = failed_since_barrier || sync_failed || !failures.is_empty();
                    let result = if failed {
                        Err(io::Error::other(format!(
                            "{} region file(s) failed to sync / earlier writes failed",
                            failures.len()
                        )))
                    } else {
                        Ok(())
                    };
                    let _ = done.send(result);
                    failed_since_barrier = false;
                }
                Request::Barrier { seq } => {
                    // FIFO 保证：排在这道栅栏之前的写都在本批里先处理过了。
                    // FIFO guarantees every write queued before this barrier has
                    // already been processed in this batch.
                    let sync_failed = report_failures(&tx, cache.sync_all(&dir), None, None);
                    let ok = !failed_since_barrier && !sync_failed;
                    let _ = tx.send(Reply::Flushed { seq, ok });
                    failed_since_barrier = false;
                }
                Request::Crash => panic!("injected chunk IO panic (test seam)"),
                Request::Shutdown => {
                    let _ = report_failures(&tx, cache.sync_all(&dir), None, None);
                    return;
                }
            }
        }
    }
    let _ = report_failures(&tx, cache.sync_all(&dir), None, None);
}

fn read_one(
    cache: &mut Cache,
    dir: &WorldDir,
    pos: ChunkPos,
) -> (Option<Vec<u8>>, Option<Vec<u8>>, Option<String>) {
    let mut error = None;
    let name = RegionPos::of(pos).file_name();
    // 只读已经存在的区域文件：官方读盘不会凭空创建空的 .mca。
    // Only read files that exist: official reads never create empty .mca files.
    let chunk = if dir.region_dir().join(&name).exists() {
        match cache.file(dir, 0, pos) {
            Ok(region) => match region.read(pos, READ_LIMIT) {
                Ok(bytes) => bytes,
                Err(e) => {
                    error = Some(format!("region chunk {pos:?}: {e}"));
                    None
                }
            },
            Err(e) => {
                error = Some(format!("open region {pos:?}: {e}"));
                None
            }
        }
    } else {
        None
    };
    let entities = if dir.entities_dir().join(&name).exists() {
        match cache.file(dir, 1, pos) {
            Ok(region) => match region.read(pos, READ_LIMIT) {
                Ok(bytes) => bytes,
                Err(e) => {
                    if error.is_none() {
                        error = Some(format!("entities {pos:?}: {e}"));
                    }
                    None
                }
            },
            Err(e) => {
                if error.is_none() {
                    error = Some(format!("open entities {pos:?}: {e}"));
                }
                None
            }
        }
    } else {
        None
    };
    (chunk, entities, error)
}

/// 写 `region/` 和 `entities/` 两份，返回失败的 `(路径, 原因)`。
/// Write the `region/` and `entities/` files; return `(path, reason)` failures.
fn write_one(
    cache: &mut Cache,
    dir: &WorldDir,
    pos: ChunkPos,
    chunk: Option<Vec<u8>>,
    entities: Option<Vec<u8>>,
    timestamp: i32,
) -> Vec<(String, String)> {
    let mut failures = Vec::new();
    for (kind, data) in [(0u8, chunk.as_deref()), (1u8, entities.as_deref())] {
        let directory = if kind == 0 {
            dir.region_dir()
        } else {
            dir.entities_dir()
        };
        let path = directory.join(RegionPos::of(pos).file_name());
        match cache.file(dir, kind, pos) {
            Ok(region) => {
                if let Err(error) = region.write(pos, data, timestamp) {
                    failures.push((path.display().to_string(), error.to_string()));
                }
            }
            Err(error) => {
                failures.push((path.display().to_string(), error.to_string()));
            }
        }
    }
    failures
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("bamboo-chunk-io-{tag}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn collect(io: &ChunkIo) -> Vec<Reply> {
        let mut out = Vec::new();
        while let Some(reply) = io.try_recv() {
            out.push(reply);
        }
        out
    }

    #[test]
    fn a_write_can_be_read_back_and_missing_chunks_are_none() {
        let tmp = TempDir::new("roundtrip");
        let dir = WorldDir::new(&tmp.0);
        dir.create_layout().unwrap();
        let mut io = ChunkIo::open(&dir).unwrap();
        let pos = ChunkPos::new(3, -2);

        io.submit_write(
            pos,
            1,
            Some(b"chunk-nbt".to_vec()),
            Some(b"entity-nbt".to_vec()),
        );
        io.flush().unwrap();
        collect(&io); // 丢掉 Written 回执

        io.submit_read(pos, 42, 7);
        io.flush().unwrap();
        let replies = collect(&io);
        assert_eq!(
            replies,
            vec![Reply::Read {
                pos,
                task_id: 42,
                generation: 7,
                chunk: Some(b"chunk-nbt".to_vec()),
                entities: Some(b"entity-nbt".to_vec()),
                error: None,
            }]
        );

        io.submit_read(ChunkPos::new(20, 20), 43, 1);
        io.flush().unwrap();
        let replies = collect(&io);
        assert!(matches!(
            replies.as_slice(),
            [Reply::Read { chunk: None, .. }]
        ));
        io.close();
    }

    #[test]
    fn chunks_survive_reopening_the_io_thread() {
        let tmp = TempDir::new("reopen");
        let dir = WorldDir::new(&tmp.0);
        dir.create_layout().unwrap();
        let pos = ChunkPos::new(1, 1);
        {
            let mut io = ChunkIo::open(&dir).unwrap();
            io.submit_write(pos, 1, Some(b"persisted".to_vec()), None);
            io.flush().unwrap();
            io.close();
        }
        let mut reopened = ChunkIo::open(&dir).unwrap();
        reopened.submit_read(pos, 9, 2);
        reopened.flush().unwrap();
        let replies = collect(&reopened);
        assert!(matches!(
            replies.as_slice(),
            [Reply::Read { chunk: Some(bytes), .. }] if bytes == b"persisted"
        ));
        // 读取一个从没写过的区域不该凭空创建空的 .mca 文件。
        let untouched = ChunkPos::new(40, 40);
        reopened.submit_read(untouched, 10, 3);
        reopened.flush().unwrap();
        let _ = collect(&reopened);
        assert!(
            !dir.region_dir()
                .join(RegionPos::of(untouched).file_name())
                .exists()
        );
        reopened.close();
    }

    #[test]
    fn a_standalone_file_write_goes_through_the_io_thread() {
        let tmp = TempDir::new("filewrite");
        let dir = WorldDir::new(&tmp.0);
        dir.create_layout().unwrap();
        let mut io = ChunkIo::open(&dir).unwrap();
        let path = dir.players_data_dir().join("some-player.dat");
        io.submit_file_write(path.clone(), None, b"player bytes".to_vec());
        io.flush().unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"player bytes");
        io.close();
    }

    /// 玩家写会回一条带 uuid/seq 的 `PlayerWritten`，世界线程据此结束待写。
    /// A player write replies `PlayerWritten` with its uuid/seq so the world
    /// thread can end the pending write.
    #[test]
    fn player_write_emits_a_completion_receipt() {
        let tmp = TempDir::new("player-receipt");
        let dir = WorldDir::new(&tmp.0);
        dir.create_layout().unwrap();
        let mut io = ChunkIo::open(&dir).unwrap();

        let first = io.submit_player_write("aaaa-bbbb", b"save one".to_vec());
        let second = io.submit_player_write("cccc-dddd", b"save two".to_vec());
        io.flush().unwrap();

        let mut receipts = Vec::new();
        while let Some(reply) = io.try_recv() {
            if let Reply::PlayerWritten {
                uuid,
                seq,
                ok,
                error,
            } = reply
            {
                receipts.push((uuid, seq, ok, error));
            }
        }
        receipts.sort_by_key(|(_, seq, _, _)| *seq);
        assert_eq!(first, 1);
        assert_eq!(second, 2);
        assert_eq!(
            receipts,
            vec![
                ("aaaa-bbbb".to_owned(), first, true, None),
                ("cccc-dddd".to_owned(), second, true, None),
            ]
        );
        // 两份存档都真的落盘了。
        assert_eq!(fs::read(dir.player_dat("aaaa-bbbb")).unwrap(), b"save one");
        assert_eq!(fs::read(dir.player_dat("cccc-dddd")).unwrap(), b"save two");
        io.close();
    }

    /// 异步栅栏的回执排在它之前的写全部处理完之后才到，且带提交时的序号。
    /// The async barrier ack arrives after all earlier writes and carries its seq.
    #[test]
    fn an_async_barrier_replies_after_prior_writes() {
        let tmp = TempDir::new("barrier");
        let dir = WorldDir::new(&tmp.0);
        dir.create_layout().unwrap();
        let mut io = ChunkIo::open(&dir).unwrap();
        let pos = ChunkPos::new(2, -3);
        io.submit_write(pos, 1, Some(b"chunk".to_vec()), None);
        let path = dir.players_data_dir().join("p.dat");
        io.submit_file_write(path.clone(), None, b"player".to_vec());
        let seq = io.submit_barrier();
        // 用阻塞 flush 作测试的同步点：它排在栅栏之后，返回时栅栏回执已经进信箱。
        // Use a blocking flush as the test's sync point: queued after the barrier,
        // so when it returns the barrier ack is already in the mailbox.
        io.flush().unwrap();
        let replies = collect(&io);
        assert!(
            replies
                .iter()
                .any(|r| matches!(r, Reply::Flushed { seq: got, ok: true } if *got == seq)),
            "the barrier should reply through the results mailbox with its seq"
        );
        // 排在栅栏之前的独立文件写确实落了盘。
        assert_eq!(fs::read(&path).unwrap(), b"player");
        io.close();
    }

    #[test]
    fn a_newer_snapshot_drops_the_older_write() {
        let tmp = TempDir::new("drop");
        let dir = WorldDir::new(&tmp.0);
        dir.create_layout().unwrap();
        let mut io = ChunkIo::open(&dir).unwrap();
        let pos = ChunkPos::new(0, 0);
        // 同一批里两个写：只有最新的 seq 该落盘并回执。
        io.submit_write(pos, 5, Some(b"old".to_vec()), None);
        io.submit_write(pos, 6, Some(b"new".to_vec()), None);
        io.flush().unwrap();
        let written: Vec<u64> = collect(&io)
            .into_iter()
            .filter_map(|r| match r {
                Reply::Written { seq, .. } => Some(seq),
                _ => None,
            })
            .collect();
        assert_eq!(written, vec![6]);
        io.close();
    }

    /// 注入一个必然失败的独立文件写：世界线程必须收到带路径和原因的 `WriteFailed`。
    /// Inject a doomed standalone write: the world thread must get a
    /// `WriteFailed` reply carrying the path and the reason.
    #[test]
    fn a_failing_write_is_reported_with_path_and_reason() {
        let tmp = TempDir::new("writefail");
        let dir = WorldDir::new(&tmp.0);
        dir.create_layout().unwrap();
        let mut io = ChunkIo::open(&dir).unwrap();
        // 把父目录变成一个文件，safe_replace 的 create_dir_all 必然失败。
        // Make the parent a file so safe_replace's create_dir_all must fail.
        let blocker = dir.players_data_dir().join("blocker");
        fs::write(&blocker, b"not a directory").unwrap();
        let doomed = blocker.join("player.dat");
        io.submit_file_write(doomed, None, b"data".to_vec());
        // 这里故意写失败，flush 会如实报 Err，只用它当同步点。
        let _ = io.flush();
        let failures: Vec<(String, String)> = collect(&io)
            .into_iter()
            .filter_map(|r| match r {
                Reply::WriteFailed { path, error, .. } => Some((path, error)),
                _ => None,
            })
            .collect();
        assert_eq!(failures.len(), 1, "exactly one failure should be reported");
        assert!(failures[0].0.contains("blocker"), "path should be reported");
        assert!(!failures[0].1.is_empty(), "reason should be reported");
        io.close();
    }

    /// 复查 F1 的用例反过来断言：区块写失败时**不**发 `Written`，改发带
    /// `pos/seq` 的 `WriteFailed`；世界线程处理失败后 `has_chunk` 为假，
    /// 之后读盘得到 `chunk: None`（照官方走模板，不会假装盘上有旧数据）。
    /// The inverse of review finding F1: a failed chunk write sends NO `Written`,
    /// but a `WriteFailed` carrying pos/seq. After the world thread handles it,
    /// `has_chunk` is false and a read honestly reports no chunk on disk.
    #[test]
    fn a_failed_chunk_write_sends_no_written_ack_and_never_marks_known() {
        let tmp = TempDir::new("chunk-write-fail");
        let dir = WorldDir::new(&tmp.0);
        dir.create_layout().unwrap();
        let mut io = ChunkIo::open(&dir).unwrap();
        let pos = ChunkPos::new(5, -3);
        // 把 region/ 和 entities/ 换成普通文件，open_region 必然失败。
        // Replace the region directories with files so open_region must fail.
        for directory in [dir.region_dir(), dir.entities_dir()] {
            fs::remove_dir(&directory).unwrap();
            fs::write(&directory, b"not a directory").unwrap();
        }

        io.submit_write(pos, 7, Some(b"chunk-nbt".to_vec()), None);
        // 写必然失败，flush 会如实返回 Err，这里只拿它当同步点。
        let _ = io.flush();
        let replies = collect(&io);

        // ① 没有假的成功回执。
        assert!(
            !replies.iter().any(|r| matches!(r, Reply::Written { .. })),
            "a failed write must never report Written"
        );
        // ② 失败回执带 pos/seq 和原因，世界线程据此保留 parking 盒子。
        let failures: Vec<_> = replies
            .iter()
            .filter_map(|r| match r {
                Reply::WriteFailed {
                    pos: Some(p),
                    seq: Some(s),
                    path,
                    error,
                } => Some((*p, *s, path.as_str(), error.as_str())),
                _ => None,
            })
            .collect();
        assert!(!failures.is_empty(), "the failure must be reported");
        assert_eq!(failures[0].0, pos);
        assert_eq!(failures[0].1, 7);
        assert!(!failures[0].3.is_empty(), "the reason must be included");

        // 世界线程收到失败回执后清掉在途标记：盘上状态绝不能被提前登记。
        io.drop_inflight(pos);
        assert!(
            !io.has_chunk(pos),
            "a failed write must not make has_chunk lie"
        );

        io.submit_read(pos, 1, 0);
        let _ = io.flush();
        let reads = collect(&io);
        assert!(
            reads
                .iter()
                .any(|r| matches!(r, Reply::Read { chunk: None, .. })),
            "the read honestly reports no chunk on disk"
        );
        io.close();
    }

    /// IO 线程 panic 后 `worker_stopped` 为真，`flush` 立刻返回 Err 而不是挂死。
    /// After an IO-thread panic, `worker_stopped` is true and `flush` returns
    /// Err immediately instead of hanging.
    #[test]
    fn a_dead_io_thread_is_detected_and_flush_reports_failure() {
        let tmp = TempDir::new("panic");
        let dir = WorldDir::new(&tmp.0);
        dir.create_layout().unwrap();
        let mut io = ChunkIo::open(&dir).unwrap();
        assert!(!io.worker_stopped());
        io.crash_for_test();
        for _ in 0..2000 {
            if io.worker_stopped() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(io.worker_stopped(), "worker death must be visible");
        assert!(
            io.flush().is_err(),
            "flush on a dead worker must fail, not hang"
        );
        io.close();
    }
}
