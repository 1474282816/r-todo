//! 接收侧 HTTP(S) 服务：LocalSend v2 协议 + 自签证书 mTLS。

use std::sync::Arc;

use localsend::http::server::web::{WebConfig, WebI18n, WebMode, WebPages};
use localsend::http::server::v2::ServerEventV2;
use localsend::http::server::{start_with_port, ServerConfigV2, ServerHandle, TlsConfig};
use localsend::http::state::ClientInfo;
use localsend::model::discovery::{DeviceType, PROTOCOL_VERSION_V2};
use tokio::sync::{mpsc, oneshot, Mutex};

use crate::discovery::platform_name;
use crate::identity::Identity;
use crate::{
    ChatMessage, ErrorCode, ErrorScope, EventEnvelope, EventSender, TransferConfig, TransferError,
    TransferEvent,
};

pub(crate) struct ServerRuntime {
    pub(crate) handle: Arc<ServerHandle>,
    /// 事件流只能取一次；由 `TransferService::start` 取走后交给事件循环。
    pub(crate) events: Mutex<Option<mpsc::Receiver<ServerEventV2>>>,
    pub(crate) port: u16,
    stop_tx: Mutex<Option<oneshot::Sender<()>>>,
    /// 自定义消息端点 → 事件流的转发任务。
    chat_task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl ServerRuntime {
    pub(crate) async fn stop(&self) {
        if let Some(tx) = self.stop_tx.lock().await.take() {
            let _ = tx.send(());
        }
        self.handle.wait_stopped().await;
        if let Some(task) = self.chat_task.lock().await.take() {
            task.abort();
        }
    }
}

pub(crate) async fn start(
    config: &TransferConfig,
    identity: &Identity,
    event_tx: EventSender,
) -> Result<ServerRuntime, TransferError> {
    let (server_event_tx, event_rx) = mpsc::channel::<ServerEventV2>(32);
    let (stop_tx, stop_rx) = oneshot::channel::<()>();

    // 自定义消息端点：vendor 只负责把 JSON 交给这里，语义解析留在壳层。
    let (chat_tx, mut chat_rx) = mpsc::channel::<serde_json::Value>(64);
    localsend::http::server::chat::register_chat_sink(chat_tx);
    let chat_task = tokio::spawn(async move {
        while let Some(value) = chat_rx.recv().await {
            match ChatMessage::from_json_value(value) {
                Ok(message) => {
                    let _ = event_tx
                        .send(EventEnvelope::new(TransferEvent::Message { message }))
                        .await;
                }
                Err(err) => eprintln!("[transfer] 忽略格式错误的会话消息: {err}"),
            }
        }
    });

    let handle = start_with_port(
        config.port,
        Some(TlsConfig {
            cert: identity.cert_pem.clone(),
            private_key: identity.private_key_pem.clone(),
        }),
        ClientInfo {
            alias: config.alias.clone(),
            version: PROTOCOL_VERSION_V2.to_string(),
            device_model: Some(platform_name()),
            device_type: Some(DeviceType::Desktop),
            token: identity.fingerprint.clone(),
        },
        None,
        Some(ServerConfigV2 {
            pin: config.pin.clone(),
            verify_checksums: config.verify_checksums,
            event_tx: server_event_tx,
        }),
        WebConfig {
            mode: WebMode::Disabled,
            i18n: WebI18n::default(),
            pages: WebPages::default(),
        },
        stop_rx,
    )
    .await
    .map_err(|err| map_start_error(config.port, err))?;

    let port = handle.port();

    Ok(ServerRuntime {
        handle: Arc::new(handle),
        events: Mutex::new(Some(event_rx)),
        port,
        stop_tx: Mutex::new(Some(stop_tx)),
        chat_task: Mutex::new(Some(chat_task)),
    })
}

fn map_start_error(port: u16, err: anyhow::Error) -> TransferError {
    let io_err = err
        .chain()
        .find_map(|e| e.downcast_ref::<std::io::Error>());

    let (code, message) = match io_err.map(|e| e.kind()) {
        Some(std::io::ErrorKind::AddrInUse) => (
            ErrorCode::PortInUse,
            format!(
                "端口 {port} 已被占用：可能是官方 LocalSend 正在运行，或该端口落在系统保留段\
                 （Hyper-V/WSL 的端口排除范围）。请关闭冲突程序，或在设置里换一个端口\
                 （换端口后对方可能发现不到本机，需要手动 IP）。"
            ),
        ),
        Some(std::io::ErrorKind::PermissionDenied) => (
            ErrorCode::Firewall,
            format!(
                "端口 {port} 被系统拒绝：常见原因是端口在 Hyper-V/WSL 的保留段，或防火墙未放行。\
                 请换个端口，或在防火墙里允许 r-todo（专用网络）。"
            ),
        ),
        _ => (ErrorCode::Unknown, format!("启动互传服务失败: {err:#}")),
    };

    TransferError::Protocol {
        scope: ErrorScope::Server,
        code,
        message,
    }
}
