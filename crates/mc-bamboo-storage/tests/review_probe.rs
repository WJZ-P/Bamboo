//! 复查报告的回归测试：验证 IO 回执的跨线程记账。
use mc_bamboo_core::ChunkPos;
use mc_bamboo_storage::{
    chunk_io::{ChunkIo, Reply},
    world_dir::WorldDir,
};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

struct Tmp(PathBuf);
impl Tmp {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!(
            "bamboo-review-{tag}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&p);
        Self(p)
    }
}
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn drain(io: &ChunkIo) -> Vec<Reply> {
    let _ = io.flush();
    let mut out = Vec::new();
    while let Some(r) = io.try_recv() {
        out.push(r);
    }
    out
}

/// 全新区块：第一次写失败、第二次写还在途时，`WriteFailed` 的 `drop_inflight`
/// 会把「新写在途」这个事实一起抹掉，于是 `has_chunk` 变假，世界线程会走模板。
#[test]
fn an_older_failure_clears_the_inflight_flag_of_a_newer_write() {
    let tmp = Tmp::new("inflight");
    let dir = WorldDir::new(&tmp.0);
    dir.create_layout().unwrap();
    let mut io = ChunkIo::open(&dir).unwrap();
    let pos = ChunkPos::new(3, 4);
    assert!(!io.has_chunk(pos), "全新区块，盘上没数据");

    // 让第一次写必然失败：region/ 和 entities/ 都变成普通文件，open_region 开不了。
    for directory in [dir.region_dir(), dir.entities_dir()] {
        fs::remove_dir_all(&directory).unwrap();
        fs::write(&directory, b"not a directory").unwrap();
    }
    io.submit_write(pos, 2, Some(b"v2".to_vec()), None);
    let r2 = drain(&io);
    assert!(
        r2.iter()
            .any(|r| matches!(r, Reply::WriteFailed { seq: Some(2), .. })),
        "第一次写必须失败：{r2:?}"
    );
    assert!(
        !r2.iter().any(|r| matches!(r, Reply::Written { .. })),
        "失败绝不回 Written：{r2:?}"
    );

    // 目录恢复，排第二次写（最新内容），先不 flush：它还排在 IO 信箱里。
    for directory in [dir.region_dir(), dir.entities_dir()] {
        fs::remove_file(&directory).unwrap();
        fs::create_dir_all(&directory).unwrap();
    }
    io.submit_write(pos, 3, Some(b"v3-newest".to_vec()), None);
    assert!(io.has_chunk(pos), "最新写在途，has_chunk 必须为‘盘上会有’");

    // 世界线程在 tick 开头收回上一刻的失败回执：poll_io 对 pos 调 drop_inflight。
    // 旧失败回执只清自己的 seq：较新的在途写仍然让 has_chunk 为真（致命 1）。
    io.drop_inflight(pos, Some(2));
    println!(
        "失败回执到达、最新写仍在途时 has_chunk = {}",
        io.has_chunk(pos)
    );
    assert!(
        io.has_chunk(pos),
        "旧失败回执不能抹掉较新写在途的事实，读绝不能走模板"
    );

    let r3 = drain(&io);
    assert!(
        r3.iter()
            .any(|r| matches!(r, Reply::Written { seq: 3, .. })),
        "新写最终成功：{r3:?}"
    );
    assert!(io.set_known(pos, true, 3));
    assert!(io.has_chunk(pos));
    io.close();
}

/// 同一批里两条同位置写：worker 的 `latest` 只写最新 seq，旧 seq 既不回 Written
/// 也不回 WriteFailed。DirectorHost 的 `pending_writes` 只记最新 seq、parking 也只留一份，
/// 所以这里是设计内的；单独列出以便确认没有额外丢数据的路径。
#[test]
fn two_writes_in_one_batch_only_the_newest_is_acked() {
    let tmp = Tmp::new("dedup");
    let dir = WorldDir::new(&tmp.0);
    dir.create_layout().unwrap();
    let mut io = ChunkIo::open(&dir).unwrap();
    let fresh = ChunkPos::new(100, 100);
    io.submit_write(fresh, 10, Some(b"new-a".to_vec()), None);
    io.submit_write(fresh, 11, Some(b"new-b".to_vec()), None);
    let r = drain(&io);
    println!("同一批两条同位置写的回执：{r:?}");
    assert!(
        r.iter()
            .any(|x| matches!(x, Reply::Written { seq: 11, .. }))
    );
    assert!(
        !r.iter()
            .any(|x| matches!(x, Reply::Written { seq: 10, .. }))
    );
    assert!(io.set_known(fresh, true, 11));
    assert!(io.has_chunk(fresh));
    io.close();
}

/// F6：worker 一次把整批请求从通道里取走、并在**开始处理前**就对每条
/// `fetch_sub`，所以 `in_flight_requests()` 只量「还排在通道里」的条数，
/// 不量「本线程手里的 batch」。Crash 夹在同一批中间时，后面的请求既没处理、
/// 计数也永远留在正数（没人再减）。
#[test]
fn in_flight_counter_leaks_when_the_worker_leaves_a_batch_early() {
    let tmp = Tmp::new("counter-leak");
    let dir = WorldDir::new(&tmp.0);
    dir.create_layout().unwrap();
    let mut io = ChunkIo::open(&dir).unwrap();
    // 不 flush：让这些请求有机会落在同一批里。
    io.submit_write(ChunkPos::new(0, 0), 1, Some(vec![1]), None);
    io.crash_for_test();
    io.submit_write(ChunkPos::new(1, 0), 2, Some(vec![2]), None);
    for _ in 0..2000 {
        if io.worker_stopped() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(io.worker_stopped(), "worker 确实退出了");
    println!(
        "IN_FLIGHT_AFTER_EARLY_EXIT={} (0 = 这次三条恰好分成多批，非确定性)",
        io.in_flight_requests()
    );
    // 静态结论：计数在 worker 取走 batch 时就归零，背压门限拦不住大批写入。
    io.close();
}
