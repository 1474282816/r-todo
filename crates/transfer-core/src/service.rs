//! 互传服务编排：发现 + 服务端 + 会话状态 + 事件输出。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use localsend::discovery::DiscoveryEvent;
use localsend::http::client::{LsHttpClient, LsHttpClientVersion};
use localsend::http::server::common::save::FileUploadTarget;
use localsend::http::server::v2::{PrepareUploadDecisionV2, ServerEventV2, SessionEndReasonV2};
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio_util::sync::CancellationToken;

use crate::client::{self, SendRequest, SendState};
use crate::discovery::{self, DiscoveryRuntime};
use crate::identity::Identity;
use crate::path;
use crate::server::{self, ServerRuntime};
use crate::{
    ChatMessage, DeviceInfo, DeviceSummary, Direction, ErrorCode, ErrorScope, EventEnvelope,
    EventSender, FileInfo, PendingIncoming, SessionEndReason, TransferConfig, TransferError,
    TransferEvent,
};

/// 接收进度节流间隔。
const RECEIVE_PROGRESS_THROTTLE: Duration = Duration::from_millis(100);

pub(crate) struct ReceiveSession {
    pub(crate) ip: String,
    /// 发送方的 HTTP 端口/协议：取消时要反向通知它停发
    pub(crate) peer_port: u16,
    pub(crate) peer_protocol: localsend::model::discovery::ProtocolType,
    pub(crate) device: DeviceSummary,
    pub(crate) files: HashMap<String, FileInfo>,
    /// 壳层通过 `respond_file` 指定的落盘路径（按 file id）。
    pub(crate) preferred: HashMap<String, PathBuf>,
    /// 是否已经回复过 prepare（用于 `pending_incoming` 恢复弹窗）。
    pub(crate) decided: bool,
    /// 每个文件正在写的 `.part` 路径（取消时用来清理/判断）
    pub(crate) parts: HashMap<String, PathBuf>,
}

struct Inner {
    config: TransferConfig,
    /// 实际绑定成功的端口（配置为 0 时由系统分配）。
    port: u16,
    identity: Identity,
    event_tx: EventSender,
    devices: Mutex<Vec<DeviceInfo>>,
    discovery: Mutex<Option<Arc<DiscoveryRuntime>>>,
    server: Mutex<Option<Arc<ServerRuntime>>>,
    pending_prepare: Mutex<HashMap<String, oneshot::Sender<PrepareUploadDecisionV2>>>,
    pending_uploads: Mutex<HashMap<(String, String), oneshot::Sender<FileUploadTarget>>>,
    receive: Mutex<HashMap<String, ReceiveSession>>,
    send: Mutex<HashMap<String, Arc<SendState>>>,
    cancel: CancellationToken,
    error: Mutex<Option<String>>,
    /// 仍在落盘的文件数：用于让 `SessionEnded` 排在 `Completed` 之后。
    writing: AtomicUsize,
    /// 用户取消的接收会话：写入任务看到后丢弃半成品、不再上报进度/完成。
    cancelled_sessions: Mutex<HashSet<String>>,
}

impl Inner {
    async fn snapshot_devices(&self, discovery: &DiscoveryRuntime) -> Vec<DeviceInfo> {
        let devices = discovery.devices();
        *self.devices.lock().await = devices.clone();
        devices
    }
}

/// 互传服务。
///
/// `start` 会真正绑定端口并启动发现；失败时返回带错误码的 [`TransferError`]。
pub struct TransferService {
    inner: Arc<Inner>,
    loop_task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl TransferService {
    pub async fn start(
        config: TransferConfig,
        event_tx: EventSender,
    ) -> Result<Self, TransferError> {
        let identity = match Identity::load_or_create(&config.identity_dir_or_default()) {
            Ok(identity) => identity,
            Err(err) => {
                // 身份目录不可写时退化为会话内临时身份，至少不阻断功能
                eprintln!("[transfer] 身份持久化失败，改用临时身份: {err}");
                Identity::ephemeral()?
            }
        };

        let server_rt = Arc::new(server::start(&config, &identity, event_tx.clone()).await?);

        let (disc_tx, disc_rx) = mpsc::channel(64);
        let discovery_rt = Arc::new(discovery::start(&config, &identity, disc_tx).await);

        let inner = Arc::new(Inner {
            config,
            port: server_rt.port,
            identity,
            event_tx,
            devices: Mutex::new(Vec::new()),
            discovery: Mutex::new(Some(discovery_rt.clone())),
            server: Mutex::new(Some(server_rt.clone())),
            pending_prepare: Mutex::new(HashMap::new()),
            pending_uploads: Mutex::new(HashMap::new()),
            receive: Mutex::new(HashMap::new()),
            send: Mutex::new(HashMap::new()),
            cancel: CancellationToken::new(),
            error: Mutex::new(None),
            writing: AtomicUsize::new(0),
            cancelled_sessions: Mutex::new(HashSet::new()),
        });

        // 组播不可用不应让功能整体失效：保留手动 IP，并上报一次错误
        if let Some(err) = discovery_rt.handle.multicast_error() {
            let detail = format!("{err:#}");
            // Windows 上常见原因：端口落在 Hyper-V/WSL 保留的排除区间，UDP 绑定会失败
            let message = if detail.contains("No network interface available for multicast") {
                format!(
                    "UDP 端口 {} 无法用于组播发现（可能被系统保留，如 Hyper-V/WSL 的端口排除段，或被其他程序占用）。\
                     可用「手动 IP」互传，或换一个端口后重启互传。",
                    inner.config.port
                )
            } else {
                format!("组播不可用（可手动输入 IP）: {detail}")
            };
            *inner.error.lock().await = Some(message.clone());
            client::send_event(
                &inner.event_tx,
                TransferEvent::Error {
                    scope: ErrorScope::Discovery,
                    code: ErrorCode::MulticastUnavailable,
                    message,
                },
            )
            .await;
        }

        let server_events = {
            // ServerRuntime 持有接收端，事件流取走一次
            let mut guard = server_rt.events.lock().await;
            guard.take()
        };

        // 启动后广播一次，让同一局域网内的设备能发现本机
        tokio::spawn({
            let discovery = discovery_rt.clone();
            async move { discovery.handle.announce().await }
        });

        let task = tokio::spawn(event_loop(
            inner.clone(),
            discovery_rt,
            server_events,
            disc_rx,
        ));

        Ok(Self {
            inner,
            loop_task: Mutex::new(Some(task)),
        })
    }

    pub fn config(&self) -> &TransferConfig {
        &self.inner.config
    }

    pub fn fingerprint(&self) -> &str {
        &self.inner.identity.fingerprint
    }

    pub fn port(&self) -> u16 {
        self.inner.port
    }

    pub fn event_sender(&self) -> EventSender {
        self.inner.event_tx.clone()
    }

    pub async fn devices(&self) -> Vec<DeviceInfo> {
        self.inner.devices.lock().await.clone()
    }

    pub async fn error(&self) -> Option<String> {
        self.inner.error.lock().await.clone()
    }

    /// 主动发现（广播 + 已知网卡 /24 兜底）。结果通过 `Devices` 事件推送。
    pub async fn refresh(&self) -> Result<(), TransferError> {
        self.refresh_with_timeout(1200).await
    }

    pub async fn refresh_with_timeout(&self, timeout_ms: u64) -> Result<(), TransferError> {
        let discovery = self.discovery().await?;
        discovery::discover_active(
            &discovery,
            self.inner.port,
            Duration::from_millis(timeout_ms.max(200)),
        )
        .await;
        Ok(())
    }

    /// 手动 IP 兜底。
    pub async fn add_manual_ip(
        &self,
        host: &str,
        port: u16,
    ) -> Result<Option<DeviceInfo>, TransferError> {
        let discovery = self.discovery().await?;
        let found = discovery::discover_manual(&discovery, host, port).await;
        if found.is_some() {
            let devices = self.inner.snapshot_devices(&discovery).await;
            client::send_event(&self.inner.event_tx, TransferEvent::Devices { devices }).await;
        }
        Ok(found.map(|d| discovery::device_info_from_stateful(&d)))
    }

    /// 发送文件，返回本地 session id（进度/完成走事件）。
    pub async fn send(
        &self,
        files: Vec<PathBuf>,
        fingerprint: &str,
        pin: Option<String>,
    ) -> Result<String, TransferError> {
        self.send_with_previews(files, fingerprint, pin, HashMap::new())
            .await
    }

    /// 会话式图片消息：缩略图塞进 `FileDto.preview`，接收方确认前即可预览。
    pub async fn send_with_previews(
        &self,
        files: Vec<PathBuf>,
        fingerprint: &str,
        pin: Option<String>,
        previews: HashMap<PathBuf, String>,
    ) -> Result<String, TransferError> {
        if files.is_empty() {
            return Err(client::protocol_error(
                ErrorScope::Send,
                ErrorCode::Io,
                "没有选择文件".to_string(),
            ));
        }

        let device = {
            let devices = self.inner.devices.lock().await;
            devices
                .iter()
                .find(|d| d.fingerprint.eq_ignore_ascii_case(fingerprint))
                .cloned()
        };

        let device = device.ok_or_else(|| {
            client::protocol_error(
                ErrorScope::Send,
                ErrorCode::Unknown,
                "目标设备不在线，请刷新设备列表或手动输入 IP".to_string(),
            )
        })?;

        let channel = device.channels.first().cloned().ok_or_else(|| {
            client::protocol_error(
                ErrorScope::Send,
                ErrorCode::Unknown,
                "目标设备没有可用地址".to_string(),
            )
        })?;

        let session_id = uuid::Uuid::new_v4().to_string();
        let state = Arc::new(SendState {
            cancel: CancellationToken::new(),
            host: channel.host.clone(),
            port: channel.port,
            protocol: discovery::protocol_type_of(channel.protocol),
            identity: self.inner.identity.clone(),
            remote_session: Mutex::new(None),
            unreachable: AtomicBool::new(false),
        });

        self.inner
            .send
            .lock()
            .await
            .insert(session_id.clone(), state.clone());

        let request = SendRequest {
            session_id: session_id.clone(),
            files,
            previews,
            pin,
            alias: self.inner.config.alias.clone(),
            local_port: self.inner.port,
            fingerprint: self.inner.identity.fingerprint.clone(),
            event_tx: self.inner.event_tx.clone(),
        };

        // 发送失败若是“连不上”，把该设备从列表里摘掉，避免用户反复点一个幽灵设备
        let fingerprint = device.fingerprint.clone();
        tokio::spawn({
            let inner = self.inner.clone();
            let state = state.clone();
            async move {
                client::run_send(state.clone(), request).await;
                if state.unreachable.load(Ordering::SeqCst) {
                    inner
                        .devices
                        .lock()
                        .await
                        .retain(|d| !d.fingerprint.eq_ignore_ascii_case(&fingerprint));
                    let devices = inner.devices.lock().await.clone();
                    client::send_event(&inner.event_tx, TransferEvent::Devices { devices }).await;
                }
            }
        });

        Ok(session_id)
    }

    /// 发送一条会话消息（文本 / 图片元数据 / 信任回执）。
    /// 对端离线或不支持会话时返回错误，壳层据此把消息留在 outbox 排队补投。
    pub async fn send_message(
        &self,
        fingerprint: &str,
        message: &ChatMessage,
    ) -> Result<(), TransferError> {
        let device = {
            let devices = self.inner.devices.lock().await;
            devices
                .iter()
                .find(|d| d.fingerprint.eq_ignore_ascii_case(fingerprint))
                .cloned()
        }
        .ok_or_else(|| {
            client::protocol_error(
                ErrorScope::Send,
                ErrorCode::Unknown,
                "目标设备不在线，消息已保留在待发送队列".to_string(),
            )
        })?;

        let channel = device.channels.first().cloned().ok_or_else(|| {
            client::protocol_error(
                ErrorScope::Send,
                ErrorCode::Unknown,
                "目标设备没有可用地址".to_string(),
            )
        })?;

        client::send_chat_message(
            &self.inner.identity,
            &channel.host,
            channel.port,
            discovery::protocol_type_of(channel.protocol),
            &device.fingerprint,
            &self.inner.identity.fingerprint,
            message,
        )
        .await
    }

    /// 接收确认：接受的文件 id 子集；传空表示拒绝。
    pub async fn respond_prepare(
        &self,
        session_id: &str,
        accepted_file_ids: Vec<String>,
    ) -> Result<(), TransferError> {
        let decision_tx = self
            .inner
            .pending_prepare
            .lock()
            .await
            .remove(session_id)
            .ok_or_else(|| {
                client::protocol_error(
                    ErrorScope::Receive,
                    ErrorCode::Unknown,
                    "该接收请求已结束或已被处理".to_string(),
                )
            })?;

        let decision = if accepted_file_ids.is_empty() {
            PrepareUploadDecisionV2::Decline
        } else {
            PrepareUploadDecisionV2::Accept(accepted_file_ids.into_iter().collect())
        };

        decision_tx.send(decision).map_err(|_| {
            client::protocol_error(
                ErrorScope::Receive,
                ErrorCode::Unknown,
                "发送方已断开".to_string(),
            )
        })?;

        // 标记已回复，避免 `pending_incoming` 再次弹窗
        if let Some(session) = self.inner.receive.lock().await.get_mut(session_id) {
            session.decided = true;
        }

        Ok(())
    }

    /// 仍在等待确认的接收请求。
    ///
    /// 窗口关闭/前端重载后 `Incoming` 事件已经错过，壳层可用它恢复弹窗。
    pub async fn pending_incoming(&self) -> Vec<PendingIncoming> {
        let sessions = self.inner.receive.lock().await;
        let mut list: Vec<PendingIncoming> = sessions
            .iter()
            .filter(|(_, session)| !session.decided)
            .map(|(session_id, session)| PendingIncoming {
                session_id: session_id.clone(),
                ip: session.ip.clone(),
                device: session.device.clone(),
                files: session.files.values().cloned().collect(),
            })
            .collect();
        list.sort_by(|a, b| a.device.alias.cmp(&b.device.alias));
        list
    }

    /// 指定某个文件（或整个会话的当前文件）的落盘路径。
    ///
    /// 若服务端还没开始该文件的上传，路径会被记录下来，等上传开始时使用。
    pub async fn respond_file(
        &self,
        session_id: &str,
        file_id: &str,
        save_path: &Path,
    ) -> Result<(), TransferError> {
        if let Some(parent) = save_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let size = {
            let mut sessions = self.inner.receive.lock().await;
            let Some(session) = sessions.get_mut(session_id) else {
                return Err(client::protocol_error(
                    ErrorScope::Receive,
                    ErrorCode::Unknown,
                    "接收会话不存在".to_string(),
                ));
            };
            session
                .preferred
                .insert(file_id.to_string(), save_path.to_path_buf());
            session.files.get(file_id).map(|f| f.size).unwrap_or(0)
        };

        let target_tx = self
            .inner
            .pending_uploads
            .lock()
            .await
            .remove(&(session_id.to_string(), file_id.to_string()));

        if let Some(target_tx) = target_tx {
            answer_upload(
                self.inner.clone(),
                session_id.to_string(),
                file_id.to_string(),
                size,
                save_path.to_path_buf(),
                target_tx,
            )?;
        }

        Ok(())
    }

    /// 拒绝整个接收会话。
    pub async fn reject(&self, session_id: &str) -> Result<(), TransferError> {
        if let Some(decision_tx) = self
            .inner
            .pending_prepare
            .lock()
            .await
            .remove(session_id)
        {
            let _ = decision_tx.send(PrepareUploadDecisionV2::Decline);
        }

        self.inner
            .receive
            .lock()
            .await
            .remove(session_id);

        client::send_event(
            &self.inner.event_tx,
            TransferEvent::SessionEnded {
                session_id: session_id.to_string(),
                reason: SessionEndReason::Rejected,
            },
        )
        .await;

        Ok(())
    }

    /// 取消发送或接收会话。
    pub async fn cancel(&self, session_id: &str) -> Result<(), TransferError> {
        // 先看接收侧
        let receive_session = self.inner.receive.lock().await.remove(session_id);

        if let Some(session) = receive_session {
            // 1) 标记取消：进行中的落盘任务看到后会丢弃半成品、停止上报
            self.inner
                .cancelled_sessions
                .lock()
                .await
                .insert(session_id.to_string());

            // 2) 清掉还没回复的上传请求 + 让服务端会话作废
            self.inner
                .pending_uploads
                .lock()
                .await
                .retain(|(sid, _), _| sid != session_id);
            if let Some(server) = self.inner.server.lock().await.as_ref() {
                server.handle.cancel_v2_session(session_id).await;
            }

            // 3) 反向通知发送方停发（对端是 r-todo/LocalSend 时会真的停下）
            {
                let identity = self.inner.identity.clone();
                let sid = session_id.to_string();
                let ip = session.ip.clone();
                let port = session.peer_port;
                let protocol = session.peer_protocol;
                tokio::spawn(async move {
                    if let Ok(http) = LsHttpClient::new(
                        &identity.private_key_pem,
                        &identity.cert_pem,
                        LsHttpClientVersion::V2,
                        None,
                        Some(Duration::from_secs(3)),
                    ) {
                        let _ = http.cancel(protocol, &ip, port, &sid).await;
                    }
                });
            }

            // 4) 尽力清掉 `.part`（正在写的会失败，交给落盘任务收尾）
            for part in session.parts.values() {
                let _ = std::fs::remove_file(part);
            }

            client::send_event(
                &self.inner.event_tx,
                TransferEvent::SessionEnded {
                    session_id: session_id.to_string(),
                    reason: SessionEndReason::Cancelled,
                },
            )
            .await;

            // 5) 兜底清理取消标记
            let inner = self.inner.clone();
            let sid = session_id.to_string();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(600)).await;
                inner.cancelled_sessions.lock().await.remove(&sid);
            });

            return Ok(());
        }

        // 再看发送侧
        let state = self.inner.send.lock().await.get(session_id).cloned();
        if let Some(state) = state {
            client::cancel_send(&state).await;
            self.inner.send.lock().await.remove(session_id);
            client::send_event(
                &self.inner.event_tx,
                TransferEvent::SessionEnded {
                    session_id: session_id.to_string(),
                    reason: SessionEndReason::Cancelled,
                },
            )
            .await;
            return Ok(());
        }

        Err(client::protocol_error(
            ErrorScope::Send,
            ErrorCode::Unknown,
            "会话不存在或已结束".to_string(),
        ))
    }

    pub async fn stop(self) -> Result<(), TransferError> {
        self.inner.cancel.cancel();

        for (_, state) in self.inner.send.lock().await.iter() {
            state.cancel.cancel();
        }
        self.inner.send.lock().await.clear();

        // 退出不能被单个任务卡死：每步都加超时，最坏情况由进程退出兜底
        if let Some(discovery) = self.inner.discovery.lock().await.take() {
            let _ = tokio::time::timeout(Duration::from_secs(2), discovery.stop()).await;
        }
        if let Some(server) = self.inner.server.lock().await.take() {
            let _ = tokio::time::timeout(Duration::from_secs(2), server.stop()).await;
        }

        if let Some(task) = self.loop_task.lock().await.take() {
            let _ = tokio::time::timeout(Duration::from_secs(3), task).await;
        }

        Ok(())
    }

    async fn discovery(&self) -> Result<Arc<DiscoveryRuntime>, TransferError> {
        self.inner
            .discovery
            .lock()
            .await
            .clone()
            .ok_or_else(|| {
                client::protocol_error(
                    ErrorScope::Discovery,
                    ErrorCode::Unknown,
                    "互传服务未启动".to_string(),
                )
            })
    }
}

async fn event_loop(
    inner: Arc<Inner>,
    discovery: Arc<DiscoveryRuntime>,
    mut server_events: Option<mpsc::Receiver<ServerEventV2>>,
    mut disc_events: mpsc::Receiver<DiscoveryEvent>,
) {
    let mut server_open = server_events.is_some();
    let mut disc_open = true;

    loop {
        tokio::select! {
            _ = inner.cancel.cancelled() => break,
            event = recv_opt(&mut server_events), if server_open => {
                match event {
                    Some(event) => handle_server_event(&inner, &discovery, event).await,
                    None => server_open = false,
                }
            }
            event = recv_opt_required(&mut disc_events), if disc_open => {
                match event {
                    Some(event) => handle_discovery_event(&inner, &discovery, event).await,
                    None => disc_open = false,
                }
            }
            else => break,
        }
    }
}

/// 可选通道：不存在时永久挂起（该分支实际被 `if` 条件禁用）。
async fn recv_opt<T>(rx: &mut Option<mpsc::Receiver<T>>) -> Option<T> {
    match rx {
        Some(rx) => rx.recv().await,
        None => std::future::pending().await,
    }
}

async fn recv_opt_required<T>(rx: &mut mpsc::Receiver<T>) -> Option<T> {
    rx.recv().await
}

async fn handle_discovery_event(
    inner: &Arc<Inner>,
    discovery: &Arc<DiscoveryRuntime>,
    event: DiscoveryEvent,
) {
    match event {
        DiscoveryEvent::Discovered { .. } | DiscoveryEvent::Updated { .. } => {
            let devices = inner.snapshot_devices(discovery).await;
            client::send_event(&inner.event_tx, TransferEvent::Devices { devices }).await;
        }
        DiscoveryEvent::MulticastFailed => {
            let message = "组播接收失败，可能无法自动发现设备；请使用手动 IP".to_string();
            *inner.error.lock().await = Some(message.clone());
            client::send_event(
                &inner.event_tx,
                TransferEvent::Error {
                    scope: ErrorScope::Discovery,
                    code: ErrorCode::MulticastUnavailable,
                    message,
                },
            )
            .await;
        }
    }
}

async fn handle_server_event(
    inner: &Arc<Inner>,
    discovery: &Arc<DiscoveryRuntime>,
    event: ServerEventV2,
) {
    match event {
        ServerEventV2::Register { ip, info } => {
            discovery::remember_registered(discovery, &ip.to_string(), &info, None).await;
            let devices = inner.snapshot_devices(discovery).await;
            client::send_event(&inner.event_tx, TransferEvent::Devices { devices }).await;
        }
        ServerEventV2::PrepareUpload {
            session_id,
            ip,
            info,
            cert_fingerprint,
            files,
            decision_tx,
        } => {
            let ip_text = ip.to_string();
            discovery::remember_registered(
                discovery,
                &ip_text,
                &info,
                cert_fingerprint.as_deref(),
            )
            .await;

            let fingerprint = cert_fingerprint
                .unwrap_or_else(|| info.fingerprint.clone())
                .to_ascii_uppercase();

            let mut file_infos: Vec<FileInfo> = files
                .values()
                .map(|f| FileInfo {
                    id: f.id.clone(),
                    file_name: f.file_name.clone(),
                    size: f.size,
                    file_type: Some(f.file_type.clone()),
                    sha256: f.sha256.clone(),
                    preview: f.preview.clone(),
                })
                .collect();
            file_infos.sort_by(|a, b| a.file_name.cmp(&b.file_name));

            let device_summary = DeviceSummary {
                alias: info.alias.clone(),
                fingerprint: fingerprint.clone(),
            };

            inner.pending_prepare.lock().await.insert(session_id.clone(), decision_tx);
            inner.receive.lock().await.insert(
                session_id.clone(),
                ReceiveSession {
                    ip: ip_text.clone(),
                    peer_port: info.port,
                    peer_protocol: info.protocol,
                    device: device_summary.clone(),
                    files: file_infos
                        .iter()
                        .map(|f| (f.id.clone(), f.clone()))
                        .collect(),
                    preferred: HashMap::new(),
                    decided: false,
                    parts: HashMap::new(),
                },
            );

            client::send_event(
                &inner.event_tx,
                TransferEvent::Incoming {
                    session_id,
                    ip: ip_text,
                    device: device_summary,
                    files: file_infos,
                },
            )
            .await;
        }
        ServerEventV2::FileUpload {
            session_id,
            file_id,
            file,
            target_tx,
        } => {
            let (preferred, size, receive_dir) = {
                let mut sessions = inner.receive.lock().await;
                match sessions.get_mut(&session_id) {
                    Some(session) => (
                        session.preferred.remove(&file_id),
                        session.files.get(&file_id).map(|f| f.size).unwrap_or(file.size),
                        inner.config.receive_dir_or_default(),
                    ),
                    None => (None, file.size, inner.config.receive_dir_or_default()),
                }
            };

            let final_path = match preferred {
                Some(path) => path,
                None => {
                    let name = path::sanitize_file_name(&file.file_name);
                    path::unique_path(&receive_dir, &name)
                }
            };

            if let Some(parent) = final_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }

            // 记录 `.part`，取消时用来清理
            if let Some(session) = inner.receive.lock().await.get_mut(&session_id) {
                session
                    .parts
                    .insert(file_id.clone(), path::part_path(&final_path));
            }

            if answer_upload(
                inner.clone(),
                session_id,
                file_id,
                size,
                final_path,
                target_tx,
            )
            .is_err()
            {
                client::send_event(
                    &inner.event_tx,
                    TransferEvent::Error {
                        scope: ErrorScope::Receive,
                        code: ErrorCode::Io,
                        message: "无法准备落盘路径".to_string(),
                    },
                )
                .await;
            }
        }
        ServerEventV2::SessionEnd { session_id, reason } => {
            inner.receive.lock().await.remove(&session_id);
            inner
                .pending_uploads
                .lock()
                .await
                .retain(|(sid, _), _| sid != &session_id);

            // 用户主动取消的会话，即使服务端报 Finished 也算取消
            let user_cancelled = inner
                .cancelled_sessions
                .lock()
                .await
                .remove(&session_id);

            let reason = if user_cancelled {
                SessionEndReason::Cancelled
            } else {
                match reason {
                    SessionEndReasonV2::Finished => SessionEndReason::Completed,
                    SessionEndReasonV2::Cancelled => SessionEndReason::Cancelled,
                }
            };

            // 服务端在所有文件写完后才发 SessionEnd，但我们的 rename/Completed 事件
            // 在独立任务里；等落盘任务收尾再发 SessionEnded，保证 UI 顺序正确。
            let inner = inner.clone();
            tokio::spawn(async move {
                let deadline = Instant::now() + Duration::from_secs(3);
                while inner.writing.load(Ordering::SeqCst) > 0 && Instant::now() < deadline {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                client::send_event(
                    &inner.event_tx,
                    TransferEvent::SessionEnded {
                        session_id,
                        reason,
                    },
                )
                .await;
            });
        }
        ServerEventV2::PrepareUploadAborted { session_id } => {
            inner.pending_prepare.lock().await.remove(&session_id);
            inner.receive.lock().await.remove(&session_id);
            inner.cancelled_sessions.lock().await.remove(&session_id);
            client::send_event(
                &inner.event_tx,
                TransferEvent::SessionEnded {
                    session_id,
                    reason: SessionEndReason::Cancelled,
                },
            )
            .await;
        }
        ServerEventV2::CancelReceived { ip, session_id } => {
            let ip_text = ip.to_string();
            let states: Vec<Arc<SendState>> = inner.send.lock().await.values().cloned().collect();
            for state in states {
                if state.host != ip_text {
                    continue;
                }
                let matches = state
                    .remote_session
                    .lock()
                    .await
                    .as_deref()
                    .map(|s| s == session_id)
                    .unwrap_or(true);
                if matches {
                    state.cancel.cancel();
                }
            }
        }
        ServerEventV2::ListenerFailed { error } => {
            let message = format!("监听端口失效: {error}");
            *inner.error.lock().await = Some(message.clone());
            client::send_event(
                &inner.event_tx,
                TransferEvent::Error {
                    scope: ErrorScope::Server,
                    code: ErrorCode::Firewall,
                    message,
                },
            )
            .await;
        }
    }
}

fn answer_upload(
    inner: Arc<Inner>,
    session_id: String,
    file_id: String,
    size: u64,
    final_path: PathBuf,
    target_tx: oneshot::Sender<FileUploadTarget>,
) -> Result<(), TransferError> {
    let part_path = path::part_path(&final_path);

    let (result_tx, result_rx) = oneshot::channel::<Result<(), String>>();
    let (progress_tx, mut progress_rx) = mpsc::channel::<u64>(16);

    target_tx
        .send(FileUploadTarget::Path {
            path: part_path.clone(),
            result_tx,
            progress_tx: Some(progress_tx),
        })
        .map_err(|_| {
            client::protocol_error(
                ErrorScope::Receive,
                ErrorCode::Unknown,
                "上传请求已结束".to_string(),
            )
        })?;

    // 进度
    {
        let event_tx = inner.event_tx.clone();
        let session_id = session_id.clone();
        let file_id = file_id.clone();
        let inner_progress = inner.clone();
        tokio::spawn(async move {
            let mut last = None::<Instant>;
            while let Some(written) = progress_rx.recv().await {
                if inner_progress
                    .cancelled_sessions
                    .lock()
                    .await
                    .contains(&session_id)
                {
                    break;
                }
                let now = Instant::now();
                let is_final = written >= size;
                if !is_final {
                    if let Some(prev) = last {
                        if now.duration_since(prev) < RECEIVE_PROGRESS_THROTTLE {
                            continue;
                        }
                    }
                }
                last = Some(now);

                let percent = if size == 0 {
                    100.0
                } else {
                    (written as f64 / size as f64 * 100.0).min(100.0) as f32
                };

                let _ = event_tx.try_send(EventEnvelope::new(TransferEvent::Progress {
                    session_id: session_id.clone(),
                    file_id: file_id.clone(),
                    direction: Direction::Receive,
                    transferred: written,
                    total: size,
                    percent,
                }));
            }
        });
    }

    // 结果：成功 rename，失败清理
    inner.writing.fetch_add(1, Ordering::SeqCst);
    tokio::spawn(async move {
        let _guard = WriteGuard(inner.clone());
        let result = match result_rx.await {
            Ok(result) => result,
            Err(_) => Err("上传中断".to_string()),
        };

        // 用户取消：丢掉半成品，不再上报完成/失败
        if inner
            .cancelled_sessions
            .lock()
            .await
            .contains(&session_id)
        {
            let _ = std::fs::remove_file(&part_path);
            return;
        }

        match result {
            Ok(()) => {
                if final_path.exists() {
                    let _ = std::fs::remove_file(&final_path);
                }
                if let Err(err) = std::fs::rename(&part_path, &final_path) {
                    let _ = std::fs::remove_file(&part_path);
                    let _ = inner.event_tx.send(EventEnvelope::new(TransferEvent::Error {
                        scope: ErrorScope::Receive,
                        code: ErrorCode::Io,
                        message: format!("保存到 {} 失败: {err}", final_path.display()),
                    })).await;
                    return;
                }

                let _ = inner
                    .event_tx
                    .send(EventEnvelope::new(TransferEvent::Completed {
                        session_id,
                        file_id,
                        direction: Direction::Receive,
                        path: Some(final_path.to_string_lossy().to_string()),
                    }))
                    .await;
            }
            Err(message) => {
                let _ = std::fs::remove_file(&part_path);
                let code = if message.to_ascii_lowercase().contains("checksum") {
                    ErrorCode::ChecksumMismatch
                } else {
                    ErrorCode::Io
                };
                let _ = inner
                    .event_tx
                    .send(EventEnvelope::new(TransferEvent::Error {
                        scope: ErrorScope::Receive,
                        code,
                        message: format!("接收失败: {message}"),
                    }))
                    .await;
            }
        }
    });

    Ok(())
}

/// 落盘任务计数守卫：任务结束（无论成功/失败/提前 return）时递减。
struct WriteGuard(Arc<Inner>);

impl Drop for WriteGuard {
    fn drop(&mut self) {
        self.0.writing.fetch_sub(1, Ordering::SeqCst);
    }
}
