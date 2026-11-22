//! Bamboo 服务端的堆分析器：按「分配调用栈」统计还活着的内存，再按归属汇总。
//!
//! ```text
//! bamboo-memprof serve --server-dir <目录> --control <目录> [host:port]
//!     和命令行版 bamboo 一样读配置、起服，但全局分配器换成记账分配器。
//!     每 200 ms 看一眼 <control>/cmd 文件，内容是一行命令：
//!       snap <名字>   写 <名字>.os.txt（进程、堆、地址空间读数）和 <名字>.raw（各分配点）
//!       quit          把所有栈帧地址解析成函数名写进 symbols.txt，然后退出进程
//! bamboo-memprof report <快照.raw> <symbols.txt> [--top N] [--stacks N] [--owner 名字]
//!     按归属汇总一份快照；--owner 只看某一类里最大的分配点。
//! ```
//!
//! 符号解析要 PDB：用 `CARGO_PROFILE_RELEASE_DEBUG=1 cargo build --release -p mc-bamboo-memprof`。
//! 记账本身有开销（每块多 8 字节以上的头、一张分配点表），所以进程级读数会比真实
//! 服务端偏大；真实读数用不带分析器的 `bamboo` 另测，这里只用来看比例和归属。
//!
//! Heap profiler for the Bamboo server. `serve` runs the server exactly like
//! the CLI binary but with an accounting global allocator, and takes
//! snapshots on request through a control file; `report` groups a snapshot by
//! owner. Symbol lookup needs a PDB, so build with
//! `CARGO_PROFILE_RELEASE_DEBUG=1`. Accounting has its own cost (a header per
//! block and the site table), so process-level numbers read higher than the
//! real server; measure those with the plain `bamboo` binary.

mod os;
mod owners;
mod report;
mod tracker;

use std::{
    collections::BTreeSet,
    error::Error,
    fs::{self, File},
    future,
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

use mc_bamboo_server::{
    Server,
    config_files::{load_or_create, log_startup},
};
use tokio::net::TcpListener;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[global_allocator]
static ALLOCATOR: tracker::Tracker = tracker::Tracker;

const CONTROL_POLL: Duration = Duration::from_millis(200);

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("serve") => serve(&args[1..]),
        Some("report") => report::run(&args[1..]),
        Some("selftest") => {
            // 确认 PDB 找得到：应该打印出本函数的名字。
            // Checks that the PDB is found: this should print this function's name.
            println!("{:?}", os::symbolize(main as fn() -> _ as usize + 40));
            Ok(())
        }
        _ => Err("usage: bamboo-memprof serve|report ... (see the top of src/main.rs)".into()),
    }
}

fn option(args: &[String], name: &str) -> Option<String> {
    let index = args.iter().position(|arg| arg == name)?;
    args.get(index + 1).cloned()
}

#[tokio::main]
async fn serve(args: &[String]) -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
    let directory = PathBuf::from(option(args, "--server-dir").ok_or("--server-dir is required")?);
    let control = PathBuf::from(option(args, "--control").ok_or("--control is required")?);
    fs::create_dir_all(&control)?;
    let files = load_or_create(&directory, true)?;
    log_startup(&directory, &files);
    let address = args
        .last()
        .filter(|last| last.contains(':') && !last.contains('\\') && !last.contains('/'))
        .cloned()
        .unwrap_or_else(|| format!("127.0.0.1:{}", files.official.server_port));
    let listener = TcpListener::bind(&address).await?;
    info!("memprof server is listening on {}", listener.local_addr()?);
    thread::Builder::new()
        .name("memprof-control".to_owned())
        .spawn(move || control_loop(&control))?;
    Server::new(listener, files.to_server_config(&directory))?
        .run(future::pending())
        .await;
    Ok(())
}

/// 轮询控制文件，直到收到 `quit`。
///
/// Polls the control file until `quit` arrives.
fn control_loop(control: &Path) {
    let command_path = control.join("cmd");
    loop {
        thread::sleep(CONTROL_POLL);
        let Ok(text) = fs::read_to_string(&command_path) else {
            continue;
        };
        let _ = fs::remove_file(&command_path);
        let mut words = text.split_whitespace();
        let outcome = match (words.next(), words.next()) {
            (Some("snap"), Some(label)) => snapshot(control, label),
            (Some("quit"), _) => {
                let outcome = write_symbols(&control.join("symbols.txt"));
                info!(?outcome, "memprof wrote symbols and exits");
                std::process::exit(0);
            }
            _ => Ok(()),
        };
        if let Err(error) = outcome {
            tracing::error!(%error, "memprof command failed");
        }
    }
}

fn mb(bytes: usize) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

/// 先读操作系统的数，再写分配点：写文件本身的分配不该混进读数里。
///
/// Reads the OS counters first and dumps the sites afterwards, so the dump's
/// own allocations stay out of the readings.
fn snapshot(control: &Path, label: &str) -> io::Result<()> {
    let process = os::process_memory();
    let (live_bytes, live_count, peak_bytes) = tracker::totals();
    let heaps = os::walk_heaps();
    let space = os::address_space(tracker::table_range());
    let mut out = BufWriter::new(File::create(control.join(format!("{label}.os.txt")))?);
    writeln!(
        out,
        "process: working_set {:.1} MB, peak {:.1} MB, private_commit {:.1} MB",
        mb(process.working_set),
        mb(process.peak_working_set),
        mb(process.private_commit)
    )?;
    writeln!(
        out,
        "tracked: live {:.1} MB in {live_count} blocks, peak live {:.1} MB, {} sites",
        mb(live_bytes as usize),
        mb(peak_bytes as usize),
        tracker::sites_used()
    )?;
    writeln!(
        out,
        "heaps ({}): busy {:.1} MB in {} blocks, block overhead {:.1} MB, free {:.1} MB in {} blocks, uncommitted {:.1} MB",
        heaps.heaps,
        mb(heaps.busy_bytes),
        heaps.busy_blocks,
        mb(heaps.overhead_bytes),
        mb(heaps.free_bytes),
        heaps.free_blocks,
        mb(heaps.uncommitted_bytes)
    )?;
    writeln!(out, "address space, committed MB (resident MB):")?;
    for (name, total, resident) in [
        ("image", space.image, space.image_resident),
        ("mapped", space.mapped, space.mapped_resident),
        (
            "private (heaps and other)",
            space.private,
            space.private_resident,
        ),
        ("thread stacks", space.stacks, space.stacks_resident),
        (
            "profiler site table",
            space.profiler,
            space.profiler_resident,
        ),
    ] {
        writeln!(out, "  {name}: {:.1} ({:.1})", mb(total), mb(resident))?;
    }
    writeln!(out, "  thread stack count: {}", space.stack_count)?;
    out.flush()?;
    let mut raw = BufWriter::new(File::create(control.join(format!("{label}.raw")))?);
    tracker::dump(&mut raw)?;
    raw.flush()?;
    info!(label, "memprof snapshot written");
    Ok(())
}

/// 每行 `地址<TAB>函数名 | 函数名...`，内联进来的函数在前。
///
/// One line per address: `address<TAB>name | name...`, inlined callees first.
fn write_symbols(path: &Path) -> io::Result<()> {
    let mut addresses = BTreeSet::new();
    tracker::for_each_frame(|address| {
        addresses.insert(address);
    });
    let mut out = BufWriter::new(File::create(path)?);
    for address in addresses {
        writeln!(out, "{address:x}\t{}", os::symbolize(address).join(" | "))?;
    }
    out.flush()
}
