//! 暴露给前端调用的命令。
//!
//! 这里不放业务逻辑，只把调用转给对应的模块。错误统一转成给用户看的字符串。

use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::{
    controller::{ServerController, ServerSnapshot},
    log_bridge::{LogHub, LogLine},
    metrics::{MetricsHub, MetricsSample},
    settings::{self, ServerSettings},
};

#[tauri::command]
pub async fn get_snapshot(
    controller: State<'_, ServerController>,
) -> Result<ServerSnapshot, String> {
    Ok(controller.snapshot().await)
}

#[tauri::command]
pub async fn start_server(
    app: AppHandle,
    controller: State<'_, ServerController>,
    settings: ServerSettings,
) -> Result<ServerSnapshot, String> {
    controller.start(&app, &settings).await
}

#[tauri::command]
pub async fn stop_server(
    app: AppHandle,
    controller: State<'_, ServerController>,
) -> Result<ServerSnapshot, String> {
    Ok(controller.stop(&app).await)
}

#[tauri::command]
pub fn load_settings(app: AppHandle) -> Result<ServerSettings, String> {
    settings::load(&app)
}

#[tauri::command]
pub fn save_settings(app: AppHandle, settings: ServerSettings) -> Result<(), String> {
    settings::save(&app, &settings)
}

/// 出厂设置。界面用它显示每一项的默认值，也用来恢复默认。
#[tauri::command]
pub fn default_settings(app: AppHandle) -> Result<ServerSettings, String> {
    settings::defaults(&app)
}

#[tauri::command]
pub fn recent_logs(logs: State<'_, Arc<LogHub>>) -> Vec<LogLine> {
    logs.recent()
}

/// 最近 60 秒的性能采样，打开界面时用它把曲线补齐。
#[tauri::command]
pub fn recent_metrics(metrics: State<'_, Arc<MetricsHub>>) -> Vec<MetricsSample> {
    metrics.recent()
}
