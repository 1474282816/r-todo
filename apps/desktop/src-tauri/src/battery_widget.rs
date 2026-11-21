//! 无线设备电量挂件。
//!
//! 独立小窗口（不是塞进主挂件），可拖动、记忆位置、置底但不占任务列表空间。
//! 读取逻辑在 `rtools-battery` 共享 crate 里，桌面端和 engine 用同一份实现。

use std::sync::Arc;
use std::time::{Duration, Instant};

use rtools_battery::BatteryService;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tokio::sync::Mutex;

const WINDOW_LABEL: &str = "battery";
const SETTINGS_KEY: &str = "rtools/battery-widget";

/// 挂件设置。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetSettings {
    /// 是否显示挂件。
    #[serde(default = "yes")]
    pub visible: bool,
    /// 轮询间隔（秒）。
    #[serde(default = "default_interval")]
    pub interval_secs: u64,
    /// 隐藏的设备 id。
    #[serde(default)]
    pub hidden: Vec<String>,
}

fn yes() -> bool {
    true
}

fn default_interval() -> u64 {
    60
}

impl Default for WidgetSettings {
    fn default() -> Self {
        Self {
            visible: true,
            interval_secs: 60,
            hidden: Vec::new(),
        }
    }
}

/// 挂件运行时：服务 + 设置 + 轮询句柄。
pub struct BatteryWidget {
    service: Arc<BatteryService>,
    settings: Mutex<WidgetSettings>,
    /// 上次刷新时间，供前端判断数据新旧。
    last_refresh: Mutex<Option<Instant>>,
}

impl BatteryWidget {
    pub fn new() -> Self {
        Self {
            service: Arc::new(BatteryService::new()),
            settings: Mutex::new(WidgetSettings::default()),
            last_refresh: Mutex::new(None),
        }
    }
}

impl Default for BatteryWidget {
    fn default() -> Self {
        Self::new()
    }
}

/// 打开（或聚焦）电量挂件窗口。
#[tauri::command]
pub async fn open_battery_widget(app: AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window(WINDOW_LABEL) {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
        return Ok(());
    }

    WebviewWindowBuilder::new(
        &app,
        WINDOW_LABEL,
        WebviewUrl::App("index.html?view=battery".into()),
    )
    .title("设备电量")
    .inner_size(260.0, 118.0)
    // 最小高度要够装"标题栏 + 1 行"，否则 setSize 会被夹住，
    // 表现出来就是"设备少时窗口下方留一大片空白"。
    .min_inner_size(220.0, 80.0)
    .resizable(true)
    .decorations(false)
    .transparent(true)
    .shadow(false)
    // 挂件不该抢焦点、不该占任务栏 —— 用户要的是"瞥一眼"
    .always_on_top(false)
    .skip_taskbar(true)
    .visible(true)
    .build()
    .map_err(|e| format!("打开电量挂件失败: {e}"))?;

    // 窗口刚建出来时还没读设备，先按 1 行高度起，前端拿到数据后再调 fit
    Ok(())
}

/// 关闭挂件窗口。
#[tauri::command]
pub async fn close_battery_widget(app: AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window(WINDOW_LABEL) {
        w.close().map_err(|e| e.to_string())?;
    }
    // 窗口刚建出来时还没读设备，先按 1 行高度起，前端拿到数据后再调 fit
    Ok(())
}

/// 读一次设备列表（不触发刷新，用缓存）。
#[tauri::command]
pub async fn battery_list(
    widget: tauri::State<'_, Arc<BatteryWidget>>,
    rescan: Option<bool>,
) -> Result<serde_json::Value, String> {
    let devices = widget.service.list(rescan.unwrap_or(false)).await;
    let settings = widget.settings.lock().await.clone();
    Ok(serde_json::json!({
        "devices": filter_hidden(devices, &settings.hidden),
        "settings": settings,
    }))
}

/// 立刻刷新所有设备电量。
#[tauri::command]
pub async fn battery_refresh(
    app: AppHandle,
    widget: tauri::State<'_, Arc<BatteryWidget>>,
) -> Result<serde_json::Value, String> {
    let devices = widget.service.refresh().await;
    *widget.last_refresh.lock().await = Some(Instant::now());

    let settings = widget.settings.lock().await.clone();
    let visible = filter_hidden(devices, &settings.hidden);

    // 广播给所有窗口 —— 主挂件和电量挂件都能收到
    let _ = app.emit(
        "battery-changed",
        serde_json::json!({ "devices": visible }),
    );

    Ok(serde_json::json!({ "devices": visible }))
}

/// 导出原始 Feature 数据，用于给新设备做标定。
#[tauri::command]
pub async fn battery_probe(
    widget: tauri::State<'_, Arc<BatteryWidget>>,
) -> Result<serde_json::Value, String> {
    let entries = widget.service.probe().await;
    Ok(serde_json::json!({ "entries": entries }))
}

/// 写入一条用户标定档案。
#[tauri::command]
pub async fn battery_calibrate(
    app: AppHandle,
    widget: tauri::State<'_, Arc<BatteryWidget>>,
    profile: serde_json::Value,
) -> Result<(), String> {
    let profile: rtools_battery::profile::DeviceProfile =
        serde_json::from_value(profile).map_err(|e| format!("档案格式错误: {e}"))?;
    widget
        .service
        .calibrate(profile)
        .await
        .map_err(|e| e.to_string())?;
    persist(&app, &widget).await;
    persist_profiles(&app, &widget).await;
    Ok(())
}

/// 忘掉一台设备。
#[tauri::command]
pub async fn battery_forget(
    app: AppHandle,
    widget: tauri::State<'_, Arc<BatteryWidget>>,
    id: String,
) -> Result<(), String> {
    widget.service.forget(&id).await;
    persist(&app, &widget).await;
    persist_profiles(&app, &widget).await;
    Ok(())
}

/// 更新挂件设置（可见性 / 轮询间隔 / 隐藏列表）。
#[tauri::command]
pub async fn battery_set_settings(
    app: AppHandle,
    widget: tauri::State<'_, Arc<BatteryWidget>>,
    settings: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let mut current = widget.settings.lock().await;
    if let Some(v) = settings.get("intervalSecs").and_then(|v| v.as_u64()) {
        // 夹到合理区间，防止前端传 0 把 HID 通道打爆
        current.interval_secs = v.clamp(
            rtools_battery::MIN_INTERVAL_SECS,
            rtools_battery::MAX_INTERVAL_SECS,
        );
    }
    if let Some(v) = settings.get("hidden").and_then(|v| v.as_array()) {
        current.hidden = v
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect();
    }
    if let Some(v) = settings.get("visible").and_then(|v| v.as_bool()) {
        current.visible = v;
    }
    let snapshot = current.clone();
    drop(current);

    persist(&app, &widget).await;
    Ok(serde_json::json!({ "settings": snapshot }))
}


fn filter_hidden(
    devices: Vec<rtools_battery::DeviceReading>,
    hidden: &[String],
) -> Vec<rtools_battery::DeviceReading> {
    devices
        .into_iter()
        .filter(|d| !hidden.contains(&d.id))
        .collect()
}

/// 把设置写到 localStorage 之外的地方 —— 用 Tauri 的 app config 目录。
async fn persist(app: &AppHandle, widget: &BatteryWidget) {
    let Ok(dir) = app.path().app_config_dir() else {
        return;
    };
    let _ = std::fs::create_dir_all(&dir);
    let settings = widget.settings.lock().await.clone();
    if let Ok(json) = serde_json::to_string_pretty(&settings) {
        let path = dir.join("battery-widget.json");
        // 原子写：先写临时文件再替换，避免掉电留下半个 JSON
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

/// 把用户标定的设备档案写盘。
///
/// 和 `persist` 分开：档案是 `HashMap<id, DeviceProfile>`，结构和挂件设置不同，
/// 混在一个文件里以后加字段容易互相干扰。
///
/// 不写盘的话 `BatteryService::custom` 只是内存里的 HashMap，
/// 用户标定完新设备、重启就没了 —— 得重新标一次。
async fn persist_profiles(app: &AppHandle, widget: &BatteryWidget) {
    let Ok(dir) = app.path().app_config_dir() else {
        return;
    };
    let _ = std::fs::create_dir_all(&dir);
    let json = widget.service.dump_custom().await;
    let path = dir.join("battery-profiles.json");
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, json).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

/// 启动时同步恢复设置。
///
/// 用同步读文件而不是 `block_on` —— 在 `setup` 里 block_on 异步任务会死锁，
/// 因为 setup 本身跑在 tokio 运行时线程上。
pub fn restore_settings_blocking(app: &AppHandle, widget: &BatteryWidget) {
    let Ok(dir) = app.path().app_config_dir() else {
        return;
    };
    let path = dir.join("battery-widget.json");
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    if let Ok(settings) = serde_json::from_str::<WidgetSettings>(&text) {
        if let Ok(mut guard) = widget.settings.try_lock() {
            *guard = settings;
        }
    }
}

/// 启动时恢复用户标定的设备档案。
///
/// 读文件是同步的（和 `restore_settings_blocking` 同理，避免在 setup 里 block_on），
/// 解析出来的 JSON 直接喂给 `load_custom` —— 它内部只是塞进 Mutex，
/// 用 `blocking_lock` 不会和 tokio 运行时打架。
pub fn restore_profiles_blocking(app: &AppHandle, widget: &BatteryWidget) {
    let Ok(dir) = app.path().app_config_dir() else {
        return;
    };
    let path = dir.join("battery-profiles.json");
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    // 文件不存在或内容损坏都当作"没有档案"，不报错
    if text.trim().is_empty() || text.trim() == "{}" {
        return;
    }
    if let Ok(map) =
        serde_json::from_str::<std::collections::HashMap<String, rtools_battery::DeviceProfile>>(&text)
    {
        if !widget.service.load_custom_sync(map) {
            // 启动早期不该有竞争；真撞上了就跳过，下次标定时会重新写盘
            eprintln!("[battery] 恢复设备档案失败（锁被占用），跳过");
        }
    }
}

/// 启动轮询：按设置的间隔定期刷新并广播。
///
/// 只在挂件窗口存在时刷新 —— 窗口关了还轮询纯属浪费电。
pub fn spawn_poll_loop(app: AppHandle, widget: Arc<BatteryWidget>) {
    tauri::async_runtime::spawn(async move {
        // 启动后先等 3 秒：让 HID 栈稳定下来，避免和应用启动抢设备
        tokio::time::sleep(Duration::from_secs(3)).await;

        loop {
            let interval = {
                let s = widget.settings.lock().await;
                s.interval_secs
            };

            if app.get_webview_window(WINDOW_LABEL).is_some() {
                let devices = widget.service.refresh().await;
                let settings = widget.settings.lock().await.clone();
    let visible = filter_hidden(devices, &settings.hidden);
                let _ = app.emit(
                    "battery-changed",
                    serde_json::json!({ "devices": visible }),
                );
            }

            tokio::time::sleep(Duration::from_secs(interval)).await;
        }
    });
}
