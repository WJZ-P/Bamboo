//! Bamboo 桌面控制台。
//!
//! 服务端直接跑在这个进程里。这里只负责把 `mc-bamboo-server` 的能力接到界面上，
//! 不写任何游戏逻辑。

// 发布版不弹控制台窗口。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod controller;
mod log_bridge;
mod metrics;
mod settings;

use std::sync::Arc;

use mc_bamboo_server::{config_files::read, logging::LogSetup};
use tauri::{AppHandle, Manager};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use crate::{
    controller::ServerController,
    log_bridge::{LogHub, LogLayer},
    metrics::MetricsHub,
};

fn main() {
    let logs = Arc::new(LogHub::default());
    tauri::Builder::default()
        .manage(ServerController::default())
        .manage(Arc::clone(&logs))
        .manage(Arc::new(MetricsHub::default()))
        .setup(|app| {
            // 服务器目录要等 Tauri 起好才能从 AppHandle 拿到，所以订阅者装在 setup 里。
            // The server directory needs a live AppHandle, so the subscriber is
            // installed here in setup rather than at the top of main.
            let handle = app.handle().clone();
            let hub = Arc::clone(app.state::<Arc<LogHub>>().inner());
            setup_logging(&handle, &hub)?;
            log_bridge::spawn_forwarder(app.handle().clone(), app.state::<Arc<LogHub>>().inner());
            metrics::spawn_sampler(app.handle().clone(), app.state::<Arc<MetricsHub>>().inner());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_snapshot,
            commands::start_server,
            commands::stop_server,
            commands::load_settings,
            commands::save_settings,
            commands::default_settings,
            commands::performance_presets,
            commands::merge_performance_preset,
            commands::recent_logs,
            commands::recent_metrics,
        ])
        .run(tauri::generate_context!())
        .expect("Tauri 应用启动失败");
}

/// 装好三层订阅者：控制台、logs/latest.log、送到界面的 LogLayer，三层同一份级别。
/// 和命令行版共用 `mc-bamboo-server::logging` 的文件写入器和过滤指令。
///
/// Install the three layers: console, logs/latest.log, and the LogLayer feeding
/// the UI, all with the same level. The file writer and filter directive come
/// from `mc-bamboo-server::logging`, shared with the CLI.
fn setup_logging(handle: &AppHandle, hub: &Arc<LogHub>) -> Result<(), Box<dyn std::error::Error>> {
    let directory = settings::server_directory(handle).map_err(std::io::Error::other)?;
    let files = read(&directory).unwrap_or_default();
    let LogSetup {
        file_writer,
        directive,
    } = mc_bamboo_server::logging::setup(
        &directory,
        &files.bamboo.log_level,
        &files.bamboo.log_filter,
    );
    tracing_subscriber::registry()
        .with(fmt::layer().with_filter(EnvFilter::new(&directive)))
        .with(
            fmt::layer()
                .with_writer(file_writer)
                .with_ansi(false)
                .with_filter(EnvFilter::new(&directive)),
        )
        .with(LogLayer::new(Arc::clone(hub)).with_filter(EnvFilter::new(&directive)))
        .init();
    Ok(())
}
