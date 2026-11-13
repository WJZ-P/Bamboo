//! 假玩家压测工具和基线脚本用的内置服务端。
//!
//! ```text
//! bamboo-loadtest --addr 127.0.0.1:25565 --bots 50 --pattern walk --layout spread \
//!                 --duration 60 --ramp 5 [--speed 4.3] [--json <文件>]
//!     在 5 秒里陆续连上 50 名假玩家，跑 60 秒，结束时打印客户端一侧的统计
//! bamboo-loadtest serve --addr 127.0.0.1:25565 [--max-players 500] [--view-distance 8]
//!                       [--world-dir <目录>] [--chunk-worker-threads <N>]
//!     用放开人数上限的 Bamboo 服务端起服，供基线脚本使用
//! ```
//!
//! `serve` 和假玩家是脚本里两个独立的进程，不共用一个 tokio 运行时。

mod bot;
mod motion;
mod stats;
mod wire;

use std::{
    error::Error,
    future,
    path::PathBuf,
    time::{Duration, Instant},
};

use motion::{Layout, Pattern};
use stats::Stats;
use tokio::task::JoinSet;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

const DEFAULT_ADDR: &str = "127.0.0.1:25565";
const DEFAULT_MAX_PLAYERS: u32 = 500;
/// 玩家名不超过 16 个可见 ASCII 字符，前缀留 4 位给编号。
const DEFAULT_NAME_PREFIX: &str = "Bot";

fn main() -> Result<(), Box<dyn Error>> {
    init_tracing();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|first| first == "serve") {
        return serve(&args[1..]);
    }
    loadtest(&args)
}

/// 和命令行版一样的日志初始化：剖析报告是 `tracing::info!` 打出来的。
fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
}

/// 起一个放开人数上限的 Bamboo 服务端。
///
/// 除了 `max_players` 和 `view_distance` 之外，配置和命令行版 `bamboo`
/// 的 `ServerConfig::default()` 完全相同；没有改 `crates/` 下任何代码。
#[tokio::main]
async fn serve(args: &[String]) -> Result<(), Box<dyn Error>> {
    let addr = option(args, "--addr")?.unwrap_or_else(|| DEFAULT_ADDR.to_owned());
    let max_players = number_option(args, "--max-players")?
        .unwrap_or(DEFAULT_MAX_PLAYERS)
        .max(1);
    let view_distance = number_option::<u8>(args, "--view-distance")?
        .unwrap_or_else(|| mc_bamboo_server::ServerConfig::default().view_distance);
    // 带世界目录跑压测时存档路径全部打开；不给就保持纯内存（默认）。
    let world_dir = option(args, "--world-dir")?.map(PathBuf::from);
    // 区块计算池的线程数，缺省 0（内联，和命令行版默认一致）；世界线程自己再夹上限。
    let chunk_worker_threads = number_option::<usize>(args, "--chunk-worker-threads")?.unwrap_or(0);
    let config = mc_bamboo_server::ServerConfig {
        max_players,
        view_distance,
        world_dir,
        chunk_worker_threads,
        ..mc_bamboo_server::ServerConfig::default()
    };
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!(
        "loadtest server is listening on {} for Minecraft {} (max_players={max_players}, view_distance={view_distance}, chunk_worker_threads={chunk_worker_threads})",
        listener.local_addr()?,
        mc_bamboo_server::GAME_VERSION
    );
    mc_bamboo_server::Server::new(listener, config)?
        .run(shutdown_signal())
        .await;
    info!("loadtest server stopped");
    Ok(())
}

async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        warn!(%error, "cannot listen for Ctrl+C");
        future::pending::<()>().await;
    }
}

#[tokio::main]
async fn loadtest(args: &[String]) -> Result<(), Box<dyn Error>> {
    let addr = option(args, "--addr")?.unwrap_or_else(|| DEFAULT_ADDR.to_owned());
    let bots = number_option::<usize>(args, "--bots")?.unwrap_or(1).max(1);
    let pattern = Pattern::parse(
        option(args, "--pattern")?
            .unwrap_or_else(|| "stand".to_owned())
            .as_str(),
    )?;
    let layout = Layout::parse(
        option(args, "--layout")?
            .unwrap_or_else(|| "cluster".to_owned())
            .as_str(),
    )?;
    let duration = number_option::<f64>(args, "--duration")?.unwrap_or(60.0);
    let ramp = number_option::<f64>(args, "--ramp")?.unwrap_or(0.0);
    let speed = number_option::<f64>(args, "--speed")?.unwrap_or(pattern.default_speed());
    let json_path = option(args, "--json")?.map(PathBuf::from);
    let name_prefix =
        option(args, "--name-prefix")?.unwrap_or_else(|| DEFAULT_NAME_PREFIX.to_owned());
    if !duration.is_finite() || duration <= 0.0 {
        return Err("--duration must be a positive number of seconds".into());
    }
    if !ramp.is_finite() || ramp < 0.0 {
        return Err("--ramp must be a non-negative number of seconds".into());
    }
    if !speed.is_finite() || speed < 0.0 || speed * (1.0 / 20.0) > 10.0 {
        return Err("--speed must be finite, non-negative, and under 200 blocks/s".into());
    }
    if name_prefix.len() + 4 > 16 {
        return Err("--name-prefix plus a 4-digit index must fit in 16 characters".into());
    }

    let duration = Duration::from_secs_f64(duration);
    let ramp = Duration::from_secs_f64(ramp);
    let stats = Stats::shared();
    let start = Instant::now();
    let stop = start + duration;
    let name_prefix: std::sync::Arc<str> = name_prefix.into();
    info!(
        "starting {bots} bots against {addr}: pattern={} layout={} speed={speed:.2} duration={:.1}s ramp={:.1}s",
        pattern.name(),
        layout.name(),
        duration.as_secs_f64(),
        ramp.as_secs_f64()
    );

    let mut tasks = JoinSet::new();
    for index in 0..bots {
        // ramp 内均匀放人，既不一起冲击登录，也不会在最后一秒才连上。
        let delay = if bots <= 1 || ramp.is_zero() {
            Duration::ZERO
        } else {
            ramp.mul_f64(index as f64 / (bots - 1) as f64)
        };
        let config = bot::BotConfig {
            addr: addr.clone(),
            index,
            bots,
            pattern,
            layout,
            speed,
            start,
            stop,
            stats: stats.clone(),
            name_prefix: name_prefix.clone(),
        };
        tasks.spawn(async move {
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }
            let outcome = bot::run(config).await;
            (index, outcome)
        });
    }

    while let Some(result) = tasks.join_next().await {
        match result {
            Ok((index, outcome)) => match outcome {
                bot::Outcome::Completed => {}
                bot::Outcome::JoinFailed(reason) => {
                    stats.login_failure(reason.clone());
                    warn!(bot = index, reason = %reason, "bot could not join");
                }
                bot::Outcome::Dropped(reason) => {
                    stats.dropped(index, start.elapsed(), reason.clone());
                    warn!(bot = index, reason = %reason, "bot ended early");
                }
            },
            Err(error) => warn!(%error, "a bot task panicked"),
        }
    }

    let report = stats.report(bots, duration);
    print!("{}", report.text());
    if let Some(path) = json_path {
        std::fs::write(&path, serde_json::to_string_pretty(&report.json())?)?;
        println!("json report: {}", path.display());
    }
    Ok(())
}

/// 取一个 `--name value` 形式的参数。
fn option(args: &[String], name: &str) -> Result<Option<String>, String> {
    let mut found = None;
    for pair in args.windows(2) {
        if pair[0] == name {
            if found.is_some() {
                return Err(format!("{name} was given more than once"));
            }
            found = Some(pair[1].clone());
        }
    }
    if args.last().is_some_and(|last| last == name) {
        return Err(format!("{name} needs a value"));
    }
    Ok(found)
}

fn number_option<T: std::str::FromStr>(args: &[String], name: &str) -> Result<Option<T>, String> {
    option(args, name)?
        .map(|value| {
            value
                .parse::<T>()
                .map_err(|_| format!("{name} needs a number, got '{value}'"))
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn options_are_parsed_and_validated() {
        let args = args(&[
            "--addr",
            "127.0.0.1:26000",
            "--bots",
            "50",
            "--duration",
            "60.5",
            "--ramp",
            "5",
        ]);
        assert_eq!(
            option(&args, "--addr").unwrap().as_deref(),
            Some("127.0.0.1:26000")
        );
        assert_eq!(number_option::<usize>(&args, "--bots").unwrap(), Some(50));
        assert_eq!(
            number_option::<f64>(&args, "--duration").unwrap(),
            Some(60.5)
        );
        assert_eq!(option(&args, "--json").unwrap(), None);
    }

    #[test]
    fn bad_options_are_reported_without_panicking() {
        let missing = args(&["--addr"]);
        assert!(option(&missing, "--addr").is_err());
        let duplicate = args(&["--bots", "1", "--bots", "2"]);
        assert!(option(&duplicate, "--bots").is_err());
        let not_a_number = args(&["--bots", "many"]);
        assert!(number_option::<usize>(&not_a_number, "--bots").is_err());
    }

    #[test]
    fn patterns_and_layouts_have_the_expected_names() {
        assert_eq!(Pattern::parse("fly").unwrap(), Pattern::Fly);
        assert_eq!(Layout::parse("spread").unwrap(), Layout::Spread);
        assert!(Pattern::parse("swim").is_err());
        assert!(Layout::parse("grid").is_err());
        assert_eq!(Pattern::Fly.default_speed(), 33.0);
        assert_eq!(Pattern::Walk.default_speed(), 4.3);
    }
}
