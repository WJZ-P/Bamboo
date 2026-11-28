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

use mc_bamboo_server::{
    config_files::read,
    logging::{self, LogGuard, LogSetup},
};
use tauri::{AppHandle, Manager, RunEvent};
use tracing_subscriber::{EnvFilter, prelude::*};

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
        .build(tauri::generate_context!())
        .expect("Tauri 应用启动失败")
        .run(flush_logs_on_exit);
}

/// 装好三层订阅者：控制台、logs/latest.log、送到界面的 LogLayer；界面那一层和控制台
/// 同一份级别，文件可以单独设。和命令行版共用 `mc-bamboo-server::logging`。
///
/// Install the three layers: console, logs/latest.log, and the LogLayer feeding
/// the UI. The UI layer shares the console level; the file may have its own.
/// Everything comes from `mc-bamboo-server::logging`, shared with the CLI.
fn setup_logging(handle: &AppHandle, hub: &Arc<LogHub>) -> Result<(), Box<dyn std::error::Error>> {
    let directory = settings::server_directory(handle).map_err(std::io::Error::other)?;
    let files = read(&directory).unwrap_or_default();
    let LogSetup {
        console_writer,
        file_writer,
        console_directive,
        file_directive,
        guard,
    } = logging::setup(&directory, &files.bamboo);
    tracing_subscriber::registry()
        .with(logging::console_layer(console_writer, &console_directive))
        .with(logging::file_layer(file_writer, &file_directive))
        .with(LogLayer::new(Arc::clone(hub)).with_filter(EnvFilter::new(&console_directive)))
        .init();
    // 守卫交给 Tauri 管着，退出事件里用它把排队的日志行刷到磁盘。
    // Tauri keeps the guard; the exit event flushes queued log lines through it.
    handle.manage(guard);
    Ok(())
}

/// 应用退出前把还排着队的日志行刷到磁盘：Tauri 退出时走 `std::process::exit`，不跑析构。
///
/// Flush queued log lines before the app exits: Tauri exits through
/// `std::process::exit`, which runs no destructors.
fn flush_logs_on_exit(handle: &AppHandle, event: RunEvent) {
    if matches!(event, RunEvent::Exit)
        && let Some(guard) = handle.try_state::<LogGuard>()
    {
        guard.flush();
    }
}
