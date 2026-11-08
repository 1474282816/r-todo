//! 发送侧：register → prepare-upload → 逐文件 upload。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use localsend::http::client::{ClientError, LsHttpClient, LsHttpClientVersion};
use localsend::http::dto::{PrepareUploadRequestDto, RegisterDto};
use localsend::model::discovery::{DeviceType, ProtocolType, PROTOCOL_VERSION_V2};
use localsend::model::transfer::{FileContent, FileDto, FileMetadata};
use tokio::sync::Mutex as AsyncMutex;
use tokio_util::sync::CancellationToken;

use crate::discovery::platform_name;
use crate::identity::Identity;
use crate::{
    Direction, ErrorCode, ErrorScope, EventEnvelope, EventSender, SessionEndReason, TransferError,
    TransferEvent,
};

/// 进度事件节流间隔。
const PROGRESS_THROTTLE: Duration = Duration::from_millis(100);

pub(crate) struct SendState {
    pub(crate) cancel: CancellationToken,
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) protocol: ProtocolType,
    pub(crate) identity: Identity,
    pub(crate) remote_session: AsyncMutex<Option<String>>,
    /// 是否遇到了“连不上/超时”这类网络错误（用于把离线设备从列表里摘掉）
    pub(crate) unreachable: std::sync::atomic::AtomicBool,
}

impl SendState {
    fn note_error(&self, err: &ClientError) {
        if is_network_error(err) {
            self.unreachable
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }
}

/// 连接层失败（对端离线、端口不对、超时）。
fn is_network_error(err: &ClientError) -> bool {
    matches!(err, ClientError::Reqwest(e) if e.is_connect() || e.is_timeout())
}

pub(crate) struct SendRequest {
    pub(crate) session_id: String,
    pub(crate) files: Vec<PathBuf>,
    pub(crate) pin: Option<String>,
    pub(crate) alias: String,
    pub(crate) local_port: u16,
    pub(crate) fingerprint: String,
    pub(crate) event_tx: EventSender,
}

/// 取消发送：本地取消 token + 通知对端。
pub(crate) async fn cancel_send(state: &SendState) {
    state.cancel.cancel();

    let remote_session = state.remote_session.lock().await.clone();
    let Some(session_id) = remote_session else {
        return;
    };

    if let Ok(client) = LsHttpClient::new(
        &state.identity.private_key_pem,
        &state.identity.cert_pem,
        LsHttpClientVersion::V2,
        None,
        Some(Duration::from_secs(3)),
    ) {
        let _ = client
            .cancel(
                state.protocol,
                &state.host,
                state.port,
                &session_id,
            )
            .await;
    }
}

pub(crate) async fn run_send(state: Arc<SendState>, request: SendRequest) {
    let session_id = request.session_id.clone();
    let event_tx = request.event_tx.clone();

    let outcome = send_inner(&state, &request).await;

    match outcome {
        Ok(()) => {
            send_event(
                &event_tx,
                TransferEvent::SessionEnded {
                    session_id,
                    reason: SessionEndReason::Completed,
                },
            )
            .await;
        }
        Err(err) => {
            let cancelled = err.code() == ErrorCode::Cancelled;
            if !cancelled {
                send_event(
                    &event_tx,
                    TransferEvent::Error {
                        scope: err.scope(),
                        code: err.code(),
                        message: err.to_string(),
                    },
                )
                .await;
            }
            send_event(
                &event_tx,
                TransferEvent::SessionEnded {
                    session_id,
                    reason: if cancelled {
                        SessionEndReason::Cancelled
                    } else {
                        SessionEndReason::Error
                    },
                },
            )
            .await;
        }
    }
}

async fn send_inner(state: &SendState, request: &SendRequest) -> Result<(), TransferError> {
    if request.files.is_empty() {
        return Err(protocol_error(
            ErrorScope::Send,
            ErrorCode::Io,
            "没有选择文件".to_string(),
        ));
    }

    // 发送前校验：读不到的文件不要开始会话
    let mut files: Vec<(String, PathBuf, u64)> = Vec::new();
    for path in &request.files {
        let meta = std::fs::metadata(path).map_err(|e| {
            protocol_error(
                ErrorScope::Send,
                ErrorCode::Io,
                format!("无法读取 {}: {e}", path.display()),
            )
        })?;
        if meta.is_dir() {
            return Err(protocol_error(
                ErrorScope::Send,
                ErrorCode::Io,
                format!("暂不支持发送文件夹: {}", path.display()),
            ));
        }
        let id = uuid::Uuid::new_v4().to_string();
        files.push((id, path.clone(), meta.len()));
    }

    let register_unpinned = LsHttpClient::new(
        &state.identity.private_key_pem,
        &state.identity.cert_pem,
        LsHttpClientVersion::V2,
        None,
        None,
    )
    .map_err(|e| protocol_error(ErrorScope::Send, ErrorCode::Tls, format!("初始化 TLS 失败: {e}")))?;

    let register = register_unpinned
        .register(
            state.protocol,
            &state.host,
            state.port,
            RegisterDto {
                alias: request.alias.clone(),
                version: PROTOCOL_VERSION_V2.to_string(),
                device_model: Some(platform_name()),
                device_type: Some(DeviceType::Desktop),
                token: request.fingerprint.clone(),
                port: request.local_port,
                protocol: ProtocolType::Https,
                has_web_interface: false,
            },
        )
        .await
        .map_err(|e| {
            state.note_error(&e);
            map_client_error(&e, request.pin.is_some())
        })?;

    // register 拿到的证书 fingerprint 用于钉住后续连接的 TLS
    let client = LsHttpClient::new(
        &state.identity.private_key_pem,
        &state.identity.cert_pem,
        LsHttpClientVersion::V2,
        register.cert_fingerprint.clone(),
        None,
    )
    .map_err(|e| protocol_error(ErrorScope::Send, ErrorCode::Tls, format!("初始化 TLS 失败: {e}")))?;

    let files_dto: HashMap<String, FileDto> = files
        .iter()
        .map(|(id, path, size)| (id.clone(), build_file_dto(id, path, *size)))
        .collect();

    let prepare = client
        .prepare_upload(
            state.protocol,
            &state.host,
            state.port,
            register.public_key.clone(),
            PrepareUploadRequestDto {
                info: RegisterDto {
                    alias: request.alias.clone(),
                    version: PROTOCOL_VERSION_V2.to_string(),
                    device_model: Some(platform_name()),
                    device_type: Some(DeviceType::Desktop),
                    token: request.fingerprint.clone(),
                    port: request.local_port,
                    protocol: ProtocolType::Https,
                    has_web_interface: false,
                },
                files: files_dto,
            },
            request.pin.as_deref(),
            state.cancel.clone(),
        )
        .await
        .map_err(|e| {
            state.note_error(&e);
            map_client_error(&e, request.pin.is_some())
        })?;

    if prepare.status_code == 204 {
        // 对端不需要任何文件（例如被全部过滤），视为完成
        return Ok(());
    }

    let Some(response) = prepare.response else {
        return Err(protocol_error(
            ErrorScope::Send,
            ErrorCode::Unknown,
            "对端未返回接收会话".to_string(),
        ));
    };

    *state.remote_session.lock().await = Some(response.session_id.clone());

    let by_id: HashMap<&str, &(String, PathBuf, u64)> =
        files.iter().map(|f| (f.0.as_str(), f)).collect();

    for (file_id, token) in &response.files {
        let Some((_, path, size)) = by_id.get(file_id.as_str()) else {
            continue;
        };
        upload_one(
            state,
            &client,
            register.public_key.clone(),
            &response.session_id,
            file_id,
            token,
            path,
            *size,
            &request.session_id,
            &request.event_tx,
        )
        .await?;
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn upload_one(
    state: &SendState,
    client: &LsHttpClient,
    public_key: Option<String>,
    remote_session_id: &str,
    file_id: &str,
    token: &str,
    path: &PathBuf,
    size: u64,
    session_id: &str,
    event_tx: &EventSender,
) -> Result<(), TransferError> {
    let last_emit = Arc::new(Mutex::new(None::<Instant>));
    let tx = event_tx.clone();
    let sid = session_id.to_string();
    let fid = file_id.to_string();

    let progress = move |sent: u64| {
        let now = Instant::now();
        {
            let mut guard = last_emit.lock().unwrap_or_else(|e| e.into_inner());
            if sent < size {
                if let Some(prev) = *guard {
                    if now.duration_since(prev) < PROGRESS_THROTTLE {
                        return;
                    }
                }
            }
            *guard = Some(now);
        }

        let percent = if size == 0 {
            100.0
        } else {
            (sent as f64 / size as f64 * 100.0).min(100.0) as f32
        };

        let _ = tx.try_send(EventEnvelope::new(TransferEvent::Progress {
            session_id: sid.clone(),
            file_id: fid.clone(),
            direction: Direction::Send,
            transferred: sent.min(size),
            total: size,
            percent,
        }));
    };

    client
        .upload(
            state.protocol,
            &state.host,
            state.port,
            public_key,
            remote_session_id,
            file_id,
            token,
            FileContent::Path(path.clone()),
            progress,
            state.cancel.clone(),
        )
        .await
        .map_err(|e| {
            state.note_error(&e);
            map_client_error(&e, false)
        })?;

    send_event(
        event_tx,
        TransferEvent::Completed {
            session_id: session_id.to_string(),
            file_id: file_id.to_string(),
            direction: Direction::Send,
            path: Some(path.to_string_lossy().to_string()),
        },
    )
    .await;

    Ok(())
}

fn build_file_dto(id: &str, path: &PathBuf, size: u64) -> FileDto {
    FileDto {
        id: id.to_string(),
        file_name: path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("file")
            .to_string(),
        size,
        file_type: guess_mime(path),
        sha256: None,
        preview: None,
        metadata: FileMetadata::from_path(path),
    }
}

fn guess_mime(path: &PathBuf) -> String {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "txt" | "md" | "log" => "text/plain",
        "json" => "application/json",
        "zip" => "application/zip",
        "7z" => "application/x-7z-compressed",
        "rar" => "application/vnd.rar",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "mp4" => "video/mp4",
        "mkv" => "video/x-matroska",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "exe" => "application/vnd.microsoft.portable-executable",
        "apk" => "application/vnd.android.package-archive",
        _ => "application/octet-stream",
    }
    .to_string()
}

pub(crate) fn map_client_error(err: &ClientError, pin_supplied: bool) -> TransferError {
    match err {
        ClientError::StatusCode(e) => {
            let (code, detail) = match e.status {
                401 if pin_supplied => (
                    ErrorCode::PinInvalid,
                    "PIN 不正确，请向对方确认".to_string(),
                ),
                401 => (
                    ErrorCode::PinRequired,
                    "对方要求输入 PIN".to_string(),
                ),
                403 => (
                    ErrorCode::Rejected,
                    "对方拒绝了本次传输".to_string(),
                ),
                409 => (
                    ErrorCode::Unknown,
                    "对方正忙（一次只能接收一个会话），请稍后再试".to_string(),
                ),
                422 => (
                    ErrorCode::ChecksumMismatch,
                    "文件校验失败，可能传输过程中被改动".to_string(),
                ),
                _ => (
                    ErrorCode::Unknown,
                    e.message
                        .clone()
                        .unwrap_or_else(|| format!("HTTP {}", e.status)),
                ),
            };
            protocol_error(ErrorScope::Send, code, detail)
        }
        ClientError::Cancelled => protocol_error(
            ErrorScope::Send,
            ErrorCode::Cancelled,
            "已取消".to_string(),
        ),
        ClientError::Reqwest(e) if e.is_connect() => protocol_error(
            ErrorScope::Send,
            ErrorCode::Io,
            "连不上该设备：可能已离线或端口不对。请点「刷新」重新发现，或用手动 IP 连接".to_string(),
        ),
        ClientError::Reqwest(e) if e.is_timeout() => protocol_error(
            ErrorScope::Send,
            ErrorCode::Io,
            "连接设备超时：请确认对方在线，且和本机在同一局域网".to_string(),
        ),
        ClientError::Reqwest(e) => protocol_error(
            ErrorScope::Send,
            ErrorCode::Io,
            format!("网络请求失败: {e}"),
        ),
        ClientError::Io(e) => protocol_error(ErrorScope::Send, ErrorCode::Io, e.to_string()),
        other => protocol_error(ErrorScope::Send, ErrorCode::Unknown, format!("{other:#}")),
    }
}

pub(crate) fn protocol_error(
    scope: ErrorScope,
    code: ErrorCode,
    message: String,
) -> TransferError {
    TransferError::Protocol {
        scope,
        code,
        message,
    }
}

pub(crate) async fn send_event(event_tx: &EventSender, event: TransferEvent) {
    let _ = event_tx
        .send(EventEnvelope::new(event))
        .await;
}
