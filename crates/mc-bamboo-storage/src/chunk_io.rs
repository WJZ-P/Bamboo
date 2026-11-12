//! 区块区域文件的 IO 线程：读写 `region/` 和 `entities/`，压缩/解压都在这条线程。
//!
//! 世界线程绝不等待 IO：读请求只是塞进 `requests` 通道，结果在下一刻开头由
//! `try_recv` 从 `results` 信箱收回。写请求是即发即忘，关服或 `save-all flush`
//! 用 `flush` 排一道栅栏，确保所有写落盘并 `sync_all`。
//!
//! 磁盘上是官方格式：`region/r.x.z.mca` 存区块 NBT，`entities/r.x.z.mca` 存实体 NBT，
//! 都是 `RegionFile`（zlib 压缩）。世界线程交给这里的是**未压缩的 NBT 字节**，
//! 解析和序列化在世界线程上做（见 `mc-bamboo-game` 的接入层）。
//!
//! The IO thread for chunk region files: reads and writes `region/` and `entities/`,
//! with compression/decompression on this thread. The world thread never waits for
//! IO: a read request is only pushed into `requests`, and the result is collected
//! from the `results` mailbox at the start of the next tick. Writes are fire-and-
//! forget; `flush` is a barrier used at shutdown and by `save-all flush`.

use std::{
    collections::HashMap,
    fs, io,
    sync::mpsc::{self, Receiver, Sender},
    thread::{self, JoinHandle},
    time::{SystemTime, UNIX_EPOCH},
};

use mc_bamboo_core::ChunkPos;

use crate::{
    anvil::{RegionCompression, RegionFile, RegionPos, open_region},
    world_dir::WorldDir,
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
    /// 栅栏：确认此前所有请求都处理完，并对所有打开的区域文件 `sync_all`。
    Flush {
        done: Sender<()>,
    },
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
    /// 一次写已经落盘，世界线程可以丢掉对应的写盘途中缓冲。
    Written { pos: ChunkPos, seq: u64 },
}

/// 一条 IO 线程的句柄。世界线程持有，非阻塞提交、tick 开头收信箱。
#[derive(Debug)]
pub struct ChunkIo {
    requests: Sender<Request>,
    results: Receiver<Reply>,
    worker: Option<JoinHandle<()>>,
}

impl ChunkIo {
    /// 起一条 IO 线程。`dir` 的布局由调用方（`SaveSession`）预先建好。
    pub fn open(dir: &WorldDir) -> io::Result<Self> {
        fs::create_dir_all(dir.region_dir())?;
        fs::create_dir_all(dir.entities_dir())?;
        let layout = dir.clone();
        let (requests, rx) = mpsc::channel::<Request>();
        let (tx, results) = mpsc::channel::<Reply>();
        let worker = thread::Builder::new()
            .name("bamboo-chunk-io".to_owned())
            .spawn(move || worker_loop(layout, rx, tx))?;
        Ok(Self {
            requests,
            results,
            worker: Some(worker),
        })
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
    pub fn submit_write(
        &self,
        pos: ChunkPos,
        seq: u64,
        chunk: Option<Vec<u8>>,
        entities: Option<Vec<u8>>,
    ) {
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

    /// 非阻塞地收一条结果；世界线程在 tick 开头循环调用到 `None`。
    pub fn try_recv(&self) -> Option<Reply> {
        self.results.try_recv().ok()
    }

    /// 栅栏：等此前所有请求处理完并 `sync_all`。只在关服 / `save-all flush` 用，
    /// 这是设计稿明确允许世界线程等待 IO 的两处。
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
        })
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

    fn sync_all(&mut self) {
        for region in self.files.values_mut() {
            let _ = region.sync();
        }
    }
}

fn worker_loop(dir: WorldDir, rx: Receiver<Request>, tx: Sender<Reply>) {
    let mut cache = Cache::default();
    // 每个区块最新快照的 seq，用于丢掉还没落盘的旧快照。
    let mut latest: HashMap<ChunkPos, u64> = HashMap::new();
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
                        write_one(&mut cache, &dir, pos, chunk, entities, timestamp);
                        let _ = tx.send(Reply::Written { pos, seq });
                    }
                }
                Request::DropSnapshot { .. } => {}
                Request::Flush { done } => {
                    cache.sync_all();
                    let _ = done.send(());
                }
                Request::Shutdown => {
                    cache.sync_all();
                    return;
                }
            }
        }
    }
    cache.sync_all();
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

fn write_one(
    cache: &mut Cache,
    dir: &WorldDir,
    pos: ChunkPos,
    chunk: Option<Vec<u8>>,
    entities: Option<Vec<u8>>,
    timestamp: i32,
) {
    if let Ok(region) = cache.file(dir, 0, pos) {
        let _ = region.write(pos, chunk.as_deref(), timestamp);
    }
    if let Ok(region) = cache.file(dir, 1, pos) {
        let _ = region.write(pos, entities.as_deref(), timestamp);
    }
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
}
