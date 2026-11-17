//! 提醒调度：接受前端下发的"任务 id + 触发时间 + 标题"，用 tokio 定时任务
//! 到点后调用系统通知。
//!
//! 关键点：
//! - 用一个全局 `Mutex<HashMap<TaskId, JoinHandle>>` 管理已排定的任务
//! - 任务可以取消（重排/删除时前端先调 cancel_reminder）
//! - 通知走 tauri-plugin-notification，Windows 弹 toast + 声音

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

use chrono::{DateTime, Utc};
use serde::Deserialize;
use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;
use tokio::task::JoinHandle;

type ScheduleMap = Mutex<HashMap<String, JoinHandle<()>>>;

fn map() -> &'static ScheduleMap {
    static M: OnceLock<ScheduleMap> = OnceLock::new();
    M.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Debug, Deserialize)]
pub struct ReminderRequest {
    pub task_id: String,
    pub fire_at: DateTime<Utc>,
    pub title: String,
    #[serde(default)]
    pub body: Option<String>,
}

#[tauri::command]
pub async fn schedule_reminder(
    app: AppHandle,
    req: ReminderRequest,
) -> Result<(), String> {
    // 若已存在则先取消
    cancel_reminder_inner(&req.task_id);

    let now = Utc::now();
    let delta = req.fire_at.signed_duration_since(now);
    if delta.num_seconds() < 0 {
        // 已过时，直接发一次（可选：也可以静默）
        send_now(&app, &req.title, req.body.as_deref());
        return Ok(());
    }
    let sleep_ms = delta.num_milliseconds().max(0) as u64;
    let task_id = req.task_id.clone();
    let app_clone = app.clone();
    let title = req.title;
    let body = req.body;

    let handle = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(sleep_ms)).await;
        send_now(&app_clone, &title, body.as_deref());
        // 触发后从表中移除
        cancel_reminder_inner(&task_id);
    });
    map()
        .lock()
        .map_err(|e| e.to_string())?
        .insert(req.task_id, handle);
    Ok(())
}

#[tauri::command]
pub fn cancel_reminder(task_id: String) -> Result<(), String> {
    cancel_reminder_inner(&task_id);
    Ok(())
}

fn cancel_reminder_inner(task_id: &str) {
    if let Ok(mut m) = map().lock() {
        if let Some(handle) = m.remove(task_id) {
            handle.abort();
        }
    }
}

fn send_now(app: &AppHandle, title: &str, body: Option<&str>) {
    let mut builder = app.notification().builder().title(title);
    if let Some(b) = body {
        builder = builder.body(b);
    }
    if let Err(e) = builder.show() {
        log::warn!("notification show failed: {e}");
    }
}
