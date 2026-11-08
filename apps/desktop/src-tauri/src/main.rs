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

use tauri::Manager;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

use crate::{
    controller::ServerController,
    log_bridge::{LogHub, LogLayer},
    metrics::MetricsHub,
};

fn main() {
    let logs = Arc::new(LogHub::default());
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_subscriber::fmt::layer())
        .with(LogLayer::new(Arc::clone(&logs)))
        .init();

    tauri::Builder::default()
        .manage(ServerController::default())
        .manage(Arc::clone(&logs))
        .manage(Arc::new(MetricsHub::default()))
        .setup(|app| {
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
            commands::recent_logs,
            commands::recent_metrics,
        ])
        .run(tauri::generate_context!())
        .expect("Tauri 应用启动失败");
}
