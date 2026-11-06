//! 暴露给前端调用的命令。
//!
//! 这里不放业务逻辑，只把调用转给对应的模块。错误统一转成给用户看的字符串。

use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::{
    controller::{ServerController, ServerSnapshot},
    log_bridge::{LogHub, LogLine},
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

#[tauri::command]
pub fn recent_logs(logs: State<'_, Arc<LogHub>>) -> Vec<LogLine> {
    logs.recent()
}
