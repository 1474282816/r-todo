use std::{
    collections::HashMap,
    path::PathBuf,
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf},
    net::windows::named_pipe::{ClientOptions, NamedPipeClient},
    sync::{oneshot, Mutex},
};
use uuid::Uuid;

use super::credentials;

const PROTOCOL_VERSION: u32 = 1;
const MAX_FRAME_SIZE: usize = 16 * 1024 * 1024;

#[derive(Debug)]
pub struct EngineCallError {
    pub code: String,
    pub message: String,
}
impl std::fmt::Display for EngineCallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.message, self.code)
    }
}

impl std::error::Error for EngineCallError {}

impl Serialize for EngineCallError {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        #[derive(Serialize)]
        struct ErrorView<'a> {
            code: &'a str,
            message: &'a str,
        }
        ErrorView {
            code: &self.code,
            message: &self.message,
        }
        .serialize(serializer)
    }
}

type PendingMap = Arc<Mutex<HashMap<String, oneshot::Sender<std::result::Result<Value, EngineCallError>>>>>;

struct Session {
    writer: WriteHalf<NamedPipeClient>,
}

pub struct EngineBridge {
    app: AppHandle,
    pipe_name: String,
    child: Mutex<Option<std::process::Child>>,
    session: Mutex<Option<Session>>,
    start_lock: Mutex<()>,
    pending: PendingMap,
}

impl EngineBridge {
    pub fn new(app: AppHandle) -> Self {
        let suffix = Uuid::new_v4().simple().to_string();
        let user = std::env::var("USERNAME").unwrap_or_else(|_| "user".into());
        Self {
            app,
            pipe_name: format!(r"\\.\pipe\rtools-engine-{user}-{suffix}"),
            child: Mutex::new(None),
            session: Mutex::new(None),
            start_lock: Mutex::new(()),
            pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn ensure_started(&self) -> Result<()> {
        if self.session.lock().await.is_some() {
            return Ok(());
        }

        let _guard = self.start_lock.lock().await;
        if self.session.lock().await.is_some() {
            return Ok(());
        }

        let discovered_engine = find_engine_executable()?;
        let data_dir = engine_data_dir()?;
        let log_dir = data_dir.join("logs");
        std::fs::create_dir_all(&log_dir)?;
        let stdout_log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_dir.join("engine.stdout.log"))?;
        let stderr_log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_dir.join("engine.stderr.log"))?;

        let current = std::env::current_exe()?;
        if let Some(current_dir) = current.parent() {
            if current_dir
                != discovered_engine
                    .parent()
                    .unwrap_or_else(|| std::path::Path::new(""))
            {
                let target = current_dir.join("rtools-engine.exe");
                if target != discovered_engine {
                    std::fs::copy(&discovered_engine, &target).with_context(|| {
                        format!(
                            "复制 {} 到 {} 失败",
                            discovered_engine.display(),
                            target.display()
                        )
                    })?;
                }
            }
        }
        let engine = current
            .parent()
            .map(|dir| dir.join("rtools-engine.exe"))
            .filter(|path| path.is_file())
            .unwrap_or(discovered_engine);

        let mut command = Command::new(&engine);
        command
            .arg("serve")
            .arg("--data-dir")
            .arg(&data_dir)
            .arg("--pipe")
            .arg(&self.pipe_name)
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout_log))
            .stderr(Stdio::from(stderr_log));

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }

        let mut child = command
            .spawn()
            .with_context(|| format!("无法启动 {}", engine.display()))?;

        let deadline = Instant::now() + Duration::from_secs(12);
        let client = loop {
            match ClientOptions::new().open(&self.pipe_name) {
                Ok(client) => break client,
                Err(error) if Instant::now() < deadline => {
                    if let Ok(Some(_status)) = child.try_wait() {
                        return Err(anyhow!(
                            "rtools-engine 提前退出，请查看 {}",
                            log_dir.join("engine.stderr.log").display()
                        ));
                    }
                    tokio::time::sleep(Duration::from_millis(120)).await;
                    let _ = error;
                }
                Err(error) => {
                    return Err(anyhow!("连接 Engine 命名管道失败: {error}"));
                }
            }
        };

        let (reader, writer) = tokio::io::split(client);
        *self.session.lock().await = Some(Session { writer });
        *self.child.lock().await = Some(child);
        self.spawn_reader(reader);
        self.call_inner("system.health", json!({})).await?;
        self.restore_credentials().await;
        Ok(())
    }

    pub async fn shutdown(&self) {
        *self.session.lock().await = None;
        if let Some(mut child) = self.child.lock().await.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    async fn restore_credentials(&self) {
        let Some(values) = credentials::load_sync_credentials() else {
            return;
        };
        let payload = json!({
            "server_url": values.server_url,
            "device_id": values.device_id,
            "device_name": values.device_name,
            "token": values.token
        });
        if let Err(error) = self.call_inner("sync.restore_credentials", payload).await {
            log::warn!("restore sync credentials failed: {error}");
        }
    }

    fn spawn_reader(&self, mut reader: ReadHalf<NamedPipeClient>) {
        let app = self.app.clone();
        let pending = self.pending.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                let result = read_message(&mut reader).await;
                match result {
                    Ok(root) => {
                        let message_type = root
                            .get("message_type")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        if message_type == "event" {
                            let event = root
                                .get("event")
                                .and_then(Value::as_str)
                                .unwrap_or("engine.event");
                            let payload = root.get("payload").cloned().unwrap_or(Value::Null);
                            let _ = app.emit(
                                "engine-event",
                                json!({"event": event, "payload": payload}),
                            );
                            continue;
                        }
                        let Some(request_id) = root.get("request_id").and_then(Value::as_str) else {
                            continue;
                        };
                        let Some(sender) = pending.lock().await.remove(request_id) else {
                            continue;
                        };
                        let response = if let Some(error) = root.get("error").filter(|v| !v.is_null()) {
                            Err(EngineCallError {
                                code: error
                                    .get("code")
                                    .and_then(Value::as_str)
                                    .unwrap_or("ENGINE_ERROR")
                                    .into(),
                                message: error
                                    .get("message")
                                    .and_then(Value::as_str)
                                    .unwrap_or("Engine 调用失败")
                                    .into(),
                            })
                        } else {
                            Ok(root.get("result").cloned().unwrap_or(Value::Null))
                        };
                        let _ = sender.send(response);
                    }
                    Err(error) => {
                        let mut map = pending.lock().await;
                        let message = error.to_string();
                        for (_, sender) in map.drain() {
                            let _ = sender.send(Err(EngineCallError {
                                code: "ENGINE_DISCONNECTED".into(),
                                message: message.clone(),
                            }));
                        }
                        break;
                    }
                }
            }
        });
    }

    pub async fn call(&self, method: &str, payload: Value) -> std::result::Result<Value, EngineCallError> {
        if !allowed_method(method) {
            return Err(EngineCallError {
                code: "METHOD_NOT_ALLOWED".into(),
                message: format!("不允许调用 {method}"),
            });
        }

        self.ensure_started()
            .await
            .map_err(|error| EngineCallError {
                code: "ENGINE_START_FAILED".into(),
                message: error.to_string(),
            })?;

        self.call_inner(method, payload).await
    }

    async fn call_inner(&self, method: &str, payload: Value) -> std::result::Result<Value, EngineCallError> {
        let request_id = Uuid::new_v4().simple().to_string();
        let (sender, receiver) = oneshot::channel();
        self.pending.lock().await.insert(request_id.clone(), sender);
        let request = json!({
            "protocol_version": PROTOCOL_VERSION,
            "message_type": "request",
            "request_id": request_id,
            "method": method,
            "payload": payload
        });
        let bytes = serde_json::to_vec(&request).map_err(|error| EngineCallError {
            code: "SERIALIZE_ERROR".into(),
            message: error.to_string(),
        })?;
        if bytes.is_empty() || bytes.len() > MAX_FRAME_SIZE {
            return Err(EngineCallError {
                code: "INVALID_FRAME".into(),
                message: "消息大小超出 Engine 限制".into(),
            });
        }

        {
            let mut guard = self.session.lock().await;
            let Some(session) = guard.as_mut() else {
                return Err(EngineCallError {
                    code: "ENGINE_DISCONNECTED".into(),
                    message: "Engine 未连接".into(),
                });
            };
            session
                .writer
                .write_u32(bytes.len() as u32)
                .await
                .map_err(|error| EngineCallError {
                    code: "WRITE_ERROR".into(),
                    message: error.to_string(),
                })?;
            session
                .writer
                .write_all(&bytes)
                .await
                .map_err(|error| EngineCallError {
                    code: "WRITE_ERROR".into(),
                    message: error.to_string(),
                })?;
            session
                .writer
                .flush()
                .await
                .map_err(|error| EngineCallError {
                    code: "WRITE_ERROR".into(),
                    message: error.to_string(),
                })?;
        }

        tokio::time::timeout(Duration::from_secs(30), receiver)
            .await
            .map_err(|_| EngineCallError {
                code: "ENGINE_TIMEOUT".into(),
                message: format!("{method} 调用超时"),
            })?
            .map_err(|_| EngineCallError {
                code: "ENGINE_DISCONNECTED".into(),
                message: "Engine 连接已断开".into(),
            })?
    }
}

async fn read_message(reader: &mut ReadHalf<NamedPipeClient>) -> Result<Value> {
    let size = reader.read_u32().await? as usize;
    if size == 0 || size > MAX_FRAME_SIZE {
        return Err(anyhow!("Engine 返回非法帧大小: {size}"));
    }
    let mut frame = vec![0; size];
    reader.read_exact(&mut frame).await?;
    serde_json::from_slice(&frame).context("解析 Engine 响应失败")
}

fn allowed_method(method: &str) -> bool {
    matches!(
        method,
        "system.health"
            | "system.shutdown"
            | "tasks.list"
            | "tasks.get"
            | "tasks.create"
            | "tasks.update"
            | "tasks.complete"
            | "tasks.delete"
            | "tasks.reorder"
            | "tasks.import_json"
            | "tasks.export_json"
            | "sync.status"
            | "sync.pair"
            | "sync.sync_now"
            | "sync.logout"
            | "sync.restore_credentials"
            | "settings.get"
            | "settings.set"
            | "devices.list"
            | "devices.pairing_code"
            | "devices.revoke"
            | "autostart.get"
            | "autostart.set"
            | "mcp.status"
            | "mcp.set_enabled"
            | "mcp.rotate_token"
            | "transfer.status"
            | "transfer.start"
            | "transfer.stop"
            | "transfer.settings"
            | "transfer.refresh"
            | "transfer.devices"
            | "transfer.add_manual_ip"
            | "transfer.send"
            | "transfer.pending"
            | "transfer.respond"
            | "transfer.reject"
            | "transfer.cancel"
            | "chat.list_conversations"
            | "chat.list_messages"
            | "chat.mark_read"
            | "chat.clear"
            | "chat.trusted"
            | "chat.trust"
            | "chat.revoke_trust"
            | "chat.send_text"
            | "chat.send_files"
            | "chat.delete_file"
            | "chat.flush_outbox"
    )
}

fn find_engine_executable() -> Result<PathBuf> {
    let current = std::env::current_exe()?;
    let beside = current
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("rtools-engine.exe");
    if beside.is_file() {
        return Ok(beside);
    }

    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest.join("../../..");
    for profile in ["debug", "release"] {
        let candidate = workspace.join(format!(
            "crates/rtools-workspace/target/{profile}/rtools-engine.exe"
        ));
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(anyhow!("找不到 rtools-engine.exe"))
}

fn engine_data_dir() -> Result<PathBuf> {
    if let Ok(path) = std::env::var("RTOOLS_ENGINE_DATA_DIR") {
        return Ok(PathBuf::from(path));
    }
    let root = std::env::var_os("LOCALAPPDATA")
        .ok_or_else(|| anyhow!("LOCALAPPDATA 未设置"))?;
    Ok(PathBuf::from(root).join("rtools").join("engine"))
}
