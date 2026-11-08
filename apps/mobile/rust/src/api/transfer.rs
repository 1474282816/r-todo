//! FRB 对外 API。
//!
//! Dart 侧看到：`MobileTransferConfig`、`start`、`MobileTransfer`（opaque）。
//! 事件以 JSON 字符串流的形式交给 Dart 解析（见 `transfer_event.dart`）。
//!
//! 为什么拆成 `start` + `subscribe`：FRB 2.13 对「同时接收 StreamSink 又返回 opaque」
//! 的函数只会把流返回给 Dart，opaque 句柄被丢弃（Rust 端对象随即析构）。

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
use flutter_rust_bridge::frb;
use rtodo_transfer_core::{EventEnvelope, TransferEvent, TransferService};
use tokio::sync::{mpsc, RwLock};

use crate::{config_check, error_json, event_json, frb_generated::StreamSink};

/// Dart 侧传入的互传配置。
pub struct MobileTransferConfig {
    pub alias: String,
    pub port: u16,
    pub receive_dir: String,
    /// App 私有目录，用于持久化自签证书/私钥（fingerprint 重启不变）。
    pub identity_dir: String,
    pub pin: Option<String>,
    pub auto_accept_favorites: bool,
}

/// 互传服务门面。同一时刻只持有一个 core `TransferService`。
#[frb(opaque)]
pub struct MobileTransfer {
    service: RwLock<Option<Arc<TransferService>>>,
    /// 最近一次 devices 事件（新页面首屏直接读，避免开流前丢设备）。
    devices_json: Arc<Mutex<String>>,
    /// core 事件的接收端；`subscribe` 时被取走并交给转发任务。
    events_rx: Arc<Mutex<Option<mpsc::Receiver<EventEnvelope>>>>,
}

impl MobileTransfer {
    /// 订阅事件流。必须在 `start` 之后调用且只能调用一次。
    pub async fn subscribe(&self, events: StreamSink<String>) -> Result<()> {
        let emit: EventEmit = Arc::new(move |json: String| events.add(json).is_ok());
        self.subscribe_inner(emit)
    }
    /// 当前设备快照（`devices` 事件信封，形状与事件流一致）。
    #[frb(sync)]
    pub fn devices_json(&self) -> String {
        self.devices_json
            .lock()
            .map(|json| json.clone())
            .unwrap_or_else(|_| empty_devices_json())
    }

    pub async fn refresh(&self) -> Result<()> {
        let service = self.require().await?;
        service.refresh().await.map_err(core_error)
    }

    /// 手动添加设备。成功时返回该设备的 JSON（`DeviceInfo`），未知/无响应返回 `None`。
    pub async fn add_manual_ip(&self, host: String, port: u16) -> Result<Option<String>> {
        let service = self.require().await?;
        let device = service.add_manual_ip(&host, port).await.map_err(core_error)?;
        Ok(device.and_then(|info| serde_json::to_string(&info).ok()))
    }

    /// 发送已暂存到 cache 的文件。返回 session id。
    pub async fn send(
        &self,
        staged_paths: Vec<String>,
        fingerprint: String,
        pin: Option<String>,
    ) -> Result<String> {
        let service = self.require().await?;
        let files: Vec<PathBuf> = staged_paths.into_iter().map(PathBuf::from).collect();
        service
            .send(files, &fingerprint, pin)
            .await
            .map_err(core_error)
    }

    pub async fn respond_prepare(
        &self,
        session_id: String,
        accepted_file_ids: Vec<String>,
    ) -> Result<()> {
        let service = self.require().await?;
        service
            .respond_prepare(&session_id, accepted_file_ids)
            .await
            .map_err(core_error)
    }

    pub async fn respond_file(
        &self,
        session_id: String,
        file_id: String,
        save_path: String,
    ) -> Result<()> {
        let service = self.require().await?;
        service
            .respond_file(&session_id, &file_id, &PathBuf::from(save_path))
            .await
            .map_err(core_error)
    }

    pub async fn reject(&self, session_id: String) -> Result<()> {
        let service = self.require().await?;
        service.reject(&session_id).await.map_err(core_error)
    }

    pub async fn cancel(&self, session_id: String) -> Result<()> {
        let service = self.require().await?;
        service.cancel(&session_id).await.map_err(core_error)
    }

    /// 停止服务。可重复调用；有在途调用（如 send 未返回）时返回错误。
    pub async fn stop(&self) -> Result<()> {
        let taken = self.service.write().await.take();
        let Some(service) = taken else {
            return Ok(());
        };

        match Arc::try_unwrap(service) {
            Ok(service) => service.stop().await.map_err(core_error),
            // 还有传输在跑（send 未返回）：让调用方先取消，而不是假装停掉了。
            Err(_) => Err(anyhow!("仍有传输在进行，请先取消再关闭互传")),
        }
    }

    async fn require(&self) -> Result<Arc<TransferService>> {
        self.service
            .read()
            .await
            .clone()
            .ok_or_else(|| anyhow!(error_json::not_running()))
    }

    pub(crate) fn subscribe_inner(&self, emit: EventEmit) -> Result<()> {
        let taken = self
            .events_rx
            .lock()
            .map_err(|_| anyhow!("事件流状态异常"))?
            .take();

        let Some(mut rx) = taken else {
            return Err(anyhow!("事件流已订阅"));
        };

        let cache = Arc::clone(&self.devices_json);
        tokio::spawn(async move {
            while let Some(envelope) = rx.recv().await {
                if let TransferEvent::Devices { devices } = &envelope.event {
                    if let Ok(mut slot) = cache.lock() {
                        *slot = event_json::to_json(&EventEnvelope::new(TransferEvent::Devices {
                            devices: devices.clone(),
                        }));
                    }
                }

                if !emit(event_json::to_json(&envelope)) {
                    break;
                }
            }
        });

        Ok(())
    }
}

/// 启动互传服务。事件需再调用 `subscribe` 才开始转发给 Dart。
pub async fn start(config: MobileTransferConfig) -> Result<MobileTransfer> {
    start_inner(config).await
}

/// 返回 true 表示继续接收，false 表示 Dart 侧流已关闭。
pub(crate) type EventEmit = Arc<dyn Fn(String) -> bool + Send + Sync>;

pub(crate) async fn start_inner(config: MobileTransferConfig) -> Result<MobileTransfer> {
    let core_config = config_check::to_core_config(&config).map_err(|message| anyhow!(message))?;

    let (tx, rx) = mpsc::channel::<EventEnvelope>(256);
    let service = TransferService::start(core_config, tx)
        .await
        .map_err(core_error)?;

    Ok(MobileTransfer {
        service: RwLock::new(Some(Arc::new(service))),
        devices_json: Arc::new(Mutex::new(empty_devices_json())),
        events_rx: Arc::new(Mutex::new(Some(rx))),
    })
}

fn core_error(err: rtodo_transfer_core::TransferError) -> anyhow::Error {
    anyhow!(error_json::to_json(&err))
}

pub(crate) fn empty_devices_json() -> String {
    event_json::to_json(&EventEnvelope::new(TransferEvent::Devices { devices: vec![] }))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU16, Ordering};
    use std::sync::Mutex as StdMutex;

    use super::*;

    /// 真实 core 会 bind 端口，单测并行跑不能都用 53317。
    static NEXT_PORT: AtomicU16 = AtomicU16::new(56000);

    fn config() -> MobileTransferConfig {
        MobileTransferConfig {
            alias: "手机".into(),
            port: NEXT_PORT.fetch_add(1, Ordering::Relaxed),
            receive_dir: "/data/cache/transfer".into(),
            identity_dir: "/data/files/transfer/identity".into(),
            pin: None,
            auto_accept_favorites: false,
        }
    }

    fn collector() -> EventEmit {
        let events: Arc<StdMutex<Vec<String>>> = Arc::new(StdMutex::new(Vec::new()));
        Arc::new(move |json: String| {
            if let Ok(mut events) = events.lock() {
                events.push(json);
            }
            true
        })
    }

    #[tokio::test]
    async fn start_exposes_empty_devices_envelope() {
        let transfer = start_inner(config()).await.expect("start");

        let value: serde_json::Value =
            serde_json::from_str(&transfer.devices_json()).expect("valid json");
        assert_eq!(value["type"], "devices");
        assert_eq!(value["devices"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn subscribe_twice_is_rejected() {
        let transfer = start_inner(config()).await.expect("start");

        transfer.subscribe_inner(collector()).expect("first subscribe");
        let err = transfer
            .subscribe_inner(collector())
            .expect_err("second subscribe must fail")
            .to_string();

        assert!(err.contains("已订阅"), "unexpected error: {err}");
    }

    #[tokio::test]
    async fn send_without_device_maps_to_unknown_error() {
        let transfer = start_inner(config()).await.expect("start");

        let err = transfer
            .send(vec!["/data/cache/outbox/a.pdf".into()], "FP".into(), None)
            .await
            .expect_err("skeleton send must fail")
            .to_string();

        assert!(err.contains("UNKNOWN"), "unexpected error: {err}");
    }

    #[tokio::test]
    async fn after_stop_service_is_unavailable() {
        let transfer = start_inner(config()).await.expect("start");

        transfer.stop().await.expect("stop");
        let err = transfer
            .refresh()
            .await
            .expect_err("service already stopped")
            .to_string();

        assert!(err.contains("未启动"), "unexpected error: {err}");
        // 重复 stop 必须幂等
        transfer.stop().await.expect("second stop");
    }

    #[tokio::test]
    async fn invalid_config_is_rejected() {
        let mut cfg = config();
        cfg.alias = "   ".into();

        let err = match start_inner(cfg).await {
            Ok(_) => panic!("invalid config must fail"),
            Err(err) => err.to_string(),
        };
        assert!(err.contains("设备名称不能为空"), "unexpected error: {err}");
    }
}
