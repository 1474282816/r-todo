//! 局域网互传（LocalSend 兼容）：Tauri 状态、命令、事件桥、独立窗口。

mod events;

use std::path::PathBuf;
use std::sync::Arc;

use rtodo_transfer_core::{
    DeviceInfo, EventEnvelope, PendingIncoming, TransferConfig, TransferError, TransferService,
    DEFAULT_PORT,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tokio::sync::{mpsc, Mutex};

pub const WINDOW_LABEL: &str = "transfer";

// ───────────────────────── 设置 ─────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferSettings {
    /// 是否随应用启动互传服务（设置面板里的开关）。
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_alias")]
    pub alias: String,
    #[serde(default = "default_port")]
    pub port: u16,
    /// 空字符串表示用系统下载目录下的 `r-todo`。
    #[serde(default)]
    pub receive_dir: String,
    #[serde(default)]
    pub pin: Option<String>,
    #[serde(default)]
    pub auto_accept_favorites: bool,
}

impl Default for TransferSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            alias: default_alias(),
            port: DEFAULT_PORT,
            receive_dir: String::new(),
            pin: None,
            auto_accept_favorites: false,
        }
    }
}

fn default_alias() -> String {
    std::env::var("COMPUTERNAME")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "r-todo".to_string())
}

fn default_port() -> u16 {
    DEFAULT_PORT
}

pub fn settings_path(app: &AppHandle) -> PathBuf {
    app.path()
        .app_config_dir()
        .map(|d| d.join("transfer-config.json"))
        .unwrap_or_else(|_| std::env::temp_dir().join("transfer-config.json"))
}

pub fn load_settings(path: &std::path::Path) -> TransferSettings {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_settings(path: &std::path::Path, settings: &TransferSettings) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(
        path,
        serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

/// 解析接收目录：空 → 系统下载目录 / r-todo。
fn resolve_receive_dir(app: &AppHandle, settings: &TransferSettings) -> PathBuf {
    if !settings.receive_dir.trim().is_empty() {
        return PathBuf::from(settings.receive_dir.trim());
    }

    app.path()
        .download_dir()
        .map(|d| d.join("r-todo"))
        .unwrap_or_else(|_| std::env::temp_dir().join("r-todo"))
}

fn identity_dir(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .map(|d| d.join("transfer"))
        .unwrap_or_else(|_| std::env::temp_dir().join("r-todo-transfer"))
}

// ───────────────────────── 状态 ─────────────────────────

#[derive(Default)]
pub struct TransferState {
    service: Mutex<Option<TransferService>>,
    bridge: Mutex<Option<tokio::task::JoinHandle<()>>>,
    last_error: Mutex<Option<String>>,
}

pub type SharedTransferState = Arc<TransferState>;

impl TransferState {
    pub fn new() -> SharedTransferState {
        Arc::new(Self::default())
    }

    async fn running(&self) -> bool {
        self.service.lock().await.is_some()
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferStateView {
    pub running: bool,
    pub port: u16,
    pub alias: String,
    pub fingerprint: String,
    pub settings: TransferSettings,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartInfo {
    pub running: bool,
    pub port: u16,
    pub alias: String,
    pub fingerprint: String,
}

fn error_text(err: TransferError) -> String {
    match err {
        TransferError::Protocol { message, .. } => message,
        other => other.to_string(),
    }
}

async fn shutdown_service(state: &TransferState) {
    if let Some(service) = state.service.lock().await.take() {
        let _ = service.stop().await;
    }
    if let Some(task) = state.bridge.lock().await.take() {
        task.abort();
    }
}

async fn spawn_service(
    app: &AppHandle,
    state: &TransferState,
    settings: &TransferSettings,
) -> Result<StartInfo, String> {
    shutdown_service(state).await;

    let config = TransferConfig {
        alias: settings.alias.clone(),
        port: settings.port,
        receive_dir: resolve_receive_dir(app, settings),
        identity_dir: identity_dir(app),
        pin: settings.pin.clone().filter(|p| !p.is_empty()),
        auto_accept_favorites: settings.auto_accept_favorites,
        verify_checksums: true,
    };

    let (tx, mut rx) = mpsc::channel::<EventEnvelope>(128);

    let service = match TransferService::start(config, tx).await {
        Ok(service) => service,
        Err(err) => {
            let message = error_text(err);
            *state.last_error.lock().await = Some(message.clone());
            return Err(message);
        }
    };

    let info = StartInfo {
        running: true,
        port: service.port(),
        alias: service.config().alias.clone(),
        fingerprint: service.fingerprint().to_string(),
    };

    // 事件桥：core EventEnvelope → Tauri event
    //
    // 收到文件请求时，若互传窗口没开就自动打开（否则事件没人接，发送方只能超时）；
    // 新窗口的 JS 还没挂上监听，所以延迟再补发一次。
    let bridge = tokio::spawn({
        let app = app.clone();
        async move {
            while let Some(envelope) = rx.recv().await {
                let (name, payload) = events::map(&envelope);

                let need_open = name == events::INCOMING
                    && app.get_webview_window(WINDOW_LABEL).is_none();

                if need_open {
                    if let Err(e) = open_transfer_window(app.clone()).await {
                        eprintln!("[transfer] auto open window failed: {e}");
                        continue;
                    }
                }

                let _ = app.emit(name, payload.clone());

                if need_open {
                    let app = app.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_millis(900)).await;
                        let _ = app.emit(name, payload);
                    });
                }
            }
        }
    });

    *state.last_error.lock().await = None;
    *state.service.lock().await = Some(service);
    *state.bridge.lock().await = Some(bridge);

    Ok(info)
}

/// 应用退出时调用：停服务并等端口释放。
pub fn shutdown_blocking(app: &AppHandle) {
    let Some(state) = app.try_state::<SharedTransferState>() else {
        return;
    };
    let state = state.inner().clone();
    let _ = tauri::async_runtime::block_on(async move {
        let _ = tokio::time::timeout(std::time::Duration::from_secs(3), async {
            shutdown_service(&state).await;
        })
        .await;
    });
}

// ───────────────────────── 命令 ─────────────────────────

#[tauri::command]
pub async fn transfer_get_state(app: AppHandle) -> Result<TransferStateView, String> {
    let state = app.state::<SharedTransferState>().inner().clone();
    let settings = load_settings(&settings_path(&app));

    let (running, port, alias, fingerprint, error): (
        bool,
        u16,
        String,
        String,
        Option<String>,
    ) = {
        let guard = state.service.lock().await;
        match guard.as_ref() {
            Some(service) => (
                true,
                service.port(),
                service.config().alias.clone(),
                service.fingerprint().to_string(),
                service.error().await,
            ),
            None => (
                false,
                settings.port,
                settings.alias.clone(),
                String::new(),
                state.last_error.lock().await.clone(),
            ),
        }
    };

    Ok(TransferStateView {
        running,
        port,
        alias,
        fingerprint,
        settings,
        error,
    })
}

#[tauri::command]
pub async fn transfer_start(app: AppHandle) -> Result<StartInfo, String> {
    let state = app.state::<SharedTransferState>().inner().clone();

    // 已经在跑就直接返回当前状态：重复「启动」不应重启服务。
    // 重启会作废服务端待确认的接收会话、清空设备表，也让对端认知里的本机闪断。
    {
        let guard = state.service.lock().await;
        if let Some(service) = guard.as_ref() {
            return Ok(StartInfo {
                running: true,
                port: service.port(),
                alias: service.config().alias.clone(),
                fingerprint: service.fingerprint().to_string(),
            });
        }
    }

    let mut settings = load_settings(&settings_path(&app));
    settings.enabled = true;
    save_settings(&settings_path(&app), &settings)?;

    spawn_service(&app, &state, &settings).await
}

#[tauri::command]
pub async fn transfer_stop(app: AppHandle) -> Result<(), String> {
    let state = app.state::<SharedTransferState>().inner().clone();
    let mut settings = load_settings(&settings_path(&app));
    settings.enabled = false;
    save_settings(&settings_path(&app), &settings)?;

    shutdown_service(&state).await;
    Ok(())
}

#[tauri::command]
pub async fn transfer_set_settings(
    app: AppHandle,
    alias: Option<String>,
    port: Option<u16>,
    receive_dir: Option<String>,
    pin: Option<String>,
    auto_accept_favorites: Option<bool>,
) -> Result<TransferSettings, String> {
    let state = app.state::<SharedTransferState>().inner().clone();

    let mut settings = load_settings(&settings_path(&app));
    let previous = settings.clone();
    if let Some(alias) = alias {
        let alias = alias.trim().to_string();
        if !alias.is_empty() {
            settings.alias = alias;
        }
    }
    if let Some(port) = port {
        if port > 0 {
            settings.port = port;
        }
    }
    if let Some(dir) = receive_dir {
        settings.receive_dir = dir.trim().to_string();
    }
    if let Some(pin) = pin {
        let pin = pin.trim().to_string();
        settings.pin = if pin.is_empty() { None } else { Some(pin) };
    }
    if let Some(auto) = auto_accept_favorites {
        settings.auto_accept_favorites = auto;
    }

    save_settings(&settings_path(&app), &settings)?;

    // 只有监听相关字段（端口 / 别名 / PIN / 接收目录）变了才需要重启服务；
    // 改「已确认设备自动接收」这类开关不该打断进行中的传输与待确认会话。
    let needs_restart = previous.port != settings.port
        || previous.alias != settings.alias
        || previous.pin != settings.pin
        || previous.receive_dir != settings.receive_dir;

    if needs_restart && settings.enabled && state.running().await {
        spawn_service(&app, &state, &settings).await?;
    }

    Ok(settings)
}

#[tauri::command]
pub async fn transfer_discover(
    app: AppHandle,
    timeout_ms: Option<u64>,
) -> Result<(), String> {
    let state = app.state::<SharedTransferState>().inner().clone();
    let guard = state.service.lock().await;
    let Some(service) = guard.as_ref() else {
        return Err("互传服务未启动".into());
    };
    service
        .refresh_with_timeout(timeout_ms.unwrap_or(1200))
        .await
        .map_err(error_text)
}

#[tauri::command]
pub async fn transfer_add_manual_ip(
    app: AppHandle,
    host: String,
    port: Option<u16>,
    protocol: Option<String>,
) -> Result<Option<DeviceInfo>, String> {
    // 目前只支持 HTTPS（LocalSend 默认），保留参数只为前后端契约稳定
    let _ = protocol;
    let state = app.state::<SharedTransferState>().inner().clone();
    let guard = state.service.lock().await;
    let Some(service) = guard.as_ref() else {
        return Err("互传服务未启动".into());
    };
    service
        .add_manual_ip(host.trim(), port.unwrap_or(DEFAULT_PORT))
        .await
        .map_err(error_text)
}

#[tauri::command]
pub async fn transfer_send(
    app: AppHandle,
    files: Vec<String>,
    fingerprint: String,
    pin: Option<String>,
) -> Result<String, String> {
    let state = app.state::<SharedTransferState>().inner().clone();
    let guard = state.service.lock().await;
    let Some(service) = guard.as_ref() else {
        return Err("互传服务未启动".into());
    };

    let paths: Vec<PathBuf> = files.iter().map(PathBuf::from).collect();
    service
        .send(paths, &fingerprint, pin.filter(|p| !p.is_empty()))
        .await
        .map_err(error_text)
}

#[tauri::command]
pub async fn transfer_respond_prepare(
    app: AppHandle,
    session_id: String,
    accepted_file_ids: Vec<String>,
) -> Result<(), String> {
    let state = app.state::<SharedTransferState>().inner().clone();
    let guard = state.service.lock().await;
    let Some(service) = guard.as_ref() else {
        return Err("互传服务未启动".into());
    };
    service
        .respond_prepare(&session_id, accepted_file_ids)
        .await
        .map_err(error_text)
}

#[tauri::command]
pub async fn transfer_respond_file(
    app: AppHandle,
    session_id: String,
    file_id: String,
    save_path: Option<String>,
) -> Result<(), String> {
    let Some(save_path) = save_path.filter(|p| !p.trim().is_empty()) else {
        // 不传就走默认接收目录，由 core 生成安全文件名
        return Ok(());
    };

    let state = app.state::<SharedTransferState>().inner().clone();
    let guard = state.service.lock().await;
    let Some(service) = guard.as_ref() else {
        return Err("互传服务未启动".into());
    };
    service
        .respond_file(&session_id, &file_id, std::path::Path::new(&save_path))
        .await
        .map_err(error_text)
}

/// 仍在等待用户确认的接收请求（窗口重开后恢复弹窗）
#[tauri::command]
pub async fn transfer_pending(app: AppHandle) -> Result<Vec<PendingIncoming>, String> {
    let state = app.state::<SharedTransferState>().inner().clone();
    let guard = state.service.lock().await;
    let Some(service) = guard.as_ref() else {
        return Ok(Vec::new());
    };
    Ok(service.pending_incoming().await)
}

#[tauri::command]
pub async fn transfer_reject(app: AppHandle, session_id: String) -> Result<(), String> {
    let state = app.state::<SharedTransferState>().inner().clone();
    let guard = state.service.lock().await;
    let Some(service) = guard.as_ref() else {
        return Err("互传服务未启动".into());
    };
    service.reject(&session_id).await.map_err(error_text)
}

#[tauri::command]
pub async fn transfer_cancel(app: AppHandle, session_id: String) -> Result<(), String> {
    let state = app.state::<SharedTransferState>().inner().clone();
    let guard = state.service.lock().await;
    let Some(service) = guard.as_ref() else {
        return Err("互传服务未启动".into());
    };
    service.cancel(&session_id).await.map_err(error_text)
}

/// 打开（或聚焦）互传窗口（与 `open_month_window` 同样的单例模式）。
#[tauri::command]
pub async fn open_transfer_window(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
        return Ok(());
    }

    WebviewWindowBuilder::new(
        &app,
        WINDOW_LABEL,
        WebviewUrl::App("index.html?view=transfer".into()),
    )
    .title("互传 · r-todo")
    .inner_size(960.0, 600.0)
    .min_inner_size(720.0, 480.0)
    .center()
    .resizable(true)
    .decorations(true)
    .transparent(false)
    .visible(true)
    .build()
    .map_err(|e| format!("打开互传窗口失败: {e}"))?;

    Ok(())
}

// ───────────────────────── 会话式互传命令 ─────────────────────────

/// 删除白名单的纯函数部分（便于单测）：路径必须落在接收目录内。
fn path_within(path: &std::path::Path, dir: &std::path::Path) -> bool {
    path.starts_with(dir)
}

fn within_receive_dir(app: &AppHandle, path: &std::path::Path) -> bool {
    let settings = load_settings(&settings_path(app));
    path_within(path, &resolve_receive_dir(app, &settings))
}

#[tauri::command]
pub async fn chat_send_text(
    app: AppHandle,
    fingerprint: String,
    message: serde_json::Value,
) -> Result<(), String> {
    let state = app.state::<SharedTransferState>().inner().clone();
    let guard = state.service.lock().await;
    let Some(service) = guard.as_ref() else {
        return Err("互传服务未启动".into());
    };
    let msg: rtodo_transfer_core::ChatMessage =
        serde_json::from_value(message).map_err(|e| format!("消息格式错误: {e}"))?;
    service
        .send_message(&fingerprint, &msg)
        .await
        .map_err(error_text)
}

#[tauri::command]
pub async fn chat_send_files(
    app: AppHandle,
    files: Vec<String>,
    fingerprint: String,
    previews: std::collections::HashMap<String, String>,
    pin: Option<String>,
) -> Result<String, String> {
    let state = app.state::<SharedTransferState>().inner().clone();
    let guard = state.service.lock().await;
    let Some(service) = guard.as_ref() else {
        return Err("互传服务未启动".into());
    };
    let paths: Vec<PathBuf> = files.iter().map(PathBuf::from).collect();
    let preview_map = previews
        .into_iter()
        .map(|(path, base64)| (PathBuf::from(path), base64))
        .collect();
    service
        .send_with_previews(paths, &fingerprint, pin.filter(|p| !p.is_empty()), preview_map)
        .await
        .map_err(error_text)
}

/// 删除消息的本地文件：**只删接收副本**，绝不删用户发送时的原始源文件（Q32）。
#[tauri::command]
pub async fn chat_delete_file(app: AppHandle, path: String) -> Result<(), String> {
    let target = PathBuf::from(path.trim());
    if !target.is_file() {
        return Ok(());
    }
    if !within_receive_dir(&app, &target) {
        return Err("只能删除接收目录中的文件，发送方原始文件不会被删".into());
    }
    std::fs::remove_file(&target).map_err(|e| e.to_string())
}

/// 前端生成图片缩略图 / 复制到剪贴板时读原图字节（上限 20MB）。
#[tauri::command]
pub async fn chat_read_file_bytes(path: String) -> Result<Vec<u8>, String> {
    const MAX: u64 = 20 * 1024 * 1024;
    let meta = std::fs::metadata(path.trim()).map_err(|e| e.to_string())?;
    if meta.len() > MAX {
        return Err("文件过大，无法预览".into());
    }
    std::fs::read(path.trim()).map_err(|e| e.to_string())
}

/// 右键「另存为」：复制一份到用户选择的位置。
#[tauri::command]
pub async fn chat_copy_file(src: String, dest: String) -> Result<(), String> {
    std::fs::copy(src.trim(), dest.trim()).map_err(|e| e.to_string())?;
    Ok(())
}

/// 转发前确认原文件还在（Q31：被删则转发置灰）。
#[tauri::command]
pub async fn chat_file_exists(path: String) -> bool {
    std::path::Path::new(path.trim()).is_file()
}

/// 窗口未聚焦时让任务栏图标闪（Q37/Q46）。
#[tauri::command]
pub async fn chat_flash_window(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
        let _ = window.request_user_attention(Some(tauri::UserAttentionType::Critical));
    }
    Ok(())
}

/// 启动时按设置决定是否自动开启服务。
pub fn auto_start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let settings = load_settings(&settings_path(&app));
        if !settings.enabled {
            return;
        }
        let state = app.state::<SharedTransferState>().inner().clone();
        if let Err(err) = spawn_service(&app, &state, &settings).await {
            eprintln!("[transfer] 自动启动失败: {err}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_roundtrip_and_defaults() {
        let path = std::env::temp_dir().join(format!(
            "rtodo_transfer_settings_{}.json",
            std::process::id()
        ));

        let mut settings = TransferSettings::default();
        assert_eq!(settings.port, 53317);
        assert!(!settings.enabled);

        settings.enabled = true;
        settings.pin = Some("1234".into());
        save_settings(&path, &settings).expect("save");

        let loaded = load_settings(&path);
        assert!(loaded.enabled);
        assert_eq!(loaded.pin.as_deref(), Some("1234"));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn delete_guard_only_allows_receive_dir() {
        let dir = PathBuf::from("C:/Users/x/Downloads/r-todo");
        assert!(path_within(&dir.join("a.png"), &dir));
        assert!(!path_within(
            &PathBuf::from("C:/Users/x/Pictures/original.png"),
            &dir,
        ));
        assert!(!path_within(&PathBuf::from("C:/elsewhere/a.png"), &dir));
    }

    #[test]
    fn corrupted_settings_fall_back_to_default() {
        let path = std::env::temp_dir().join(format!(
            "rtodo_transfer_settings_corrupt_{}.json",
            std::process::id()
        ));
        std::fs::write(&path, "{oops").expect("write");
        let loaded = load_settings(&path);
        assert_eq!(loaded.port, 53317);
        let _ = std::fs::remove_file(&path);
    }
}
