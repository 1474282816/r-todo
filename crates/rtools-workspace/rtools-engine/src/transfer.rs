use anyhow::Result;
use rtodo_transfer_core::{TransferConfig, TransferService};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::sync::{mpsc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferStatus {
    pub running: bool,
    pub port: u16,
    pub alias: String,
    pub fingerprint: String,
    pub receive_dir: String,
    pub pin_configured: bool,
    pub auto_accept_favorites: bool,
    pub error: Option<String>,
}

pub struct TransferRuntime {
    service: Arc<Mutex<Option<TransferService>>>,
    status: TransferStatus,
    events: broadcast::Sender<rtodo_transfer_core::EventEnvelope>,
}

impl TransferRuntime {
    pub fn new() -> Self {
        Self {
            service: Arc::new(Mutex::new(None)),
            status: TransferStatus {
                running: false,
                port: rtodo_transfer_core::DEFAULT_PORT,
                alias: "rtools".into(),
                fingerprint: String::new(),
                receive_dir: String::new(),
                pin_configured: false,
                auto_accept_favorites: false,
                error: None,
            },
            events: broadcast::channel(256).0,
        }
    }

    pub fn status(&self) -> TransferStatus {
        self.status.clone()
    }

    pub async fn start(
        &mut self,
        receive_dir: std::path::PathBuf,
        identity_dir: std::path::PathBuf,
        config: TransferConfig,
    ) -> Result<TransferStatus> {
        let config = TransferConfig {
            receive_dir: receive_dir.clone(),
            identity_dir,
            ..config
        };
        self.status.alias = config.alias.clone();
        self.status.receive_dir = receive_dir.to_string_lossy().into_owned();
        self.status.pin_configured = config.pin.as_ref().is_some_and(|value| !value.is_empty());
        self.status.auto_accept_favorites = config.auto_accept_favorites;
        let (tx, mut rx) = mpsc::channel(128);
        let events = self.events.clone();
        match TransferService::start(config, tx).await {
            Ok(service) => {
                self.status.running = true;
                self.status.port = service.port();
                self.status.fingerprint = service.fingerprint().to_string();
                self.status.error = None;
                *self.service.lock().await = Some(service);
                tokio::spawn(async move {
                    while let Some(event) = rx.recv().await {
                        let _ = events.send(event.clone());
                        tracing::info!(?event, "transfer event");
                    }
                });
            }
            Err(err) => {
                self.status.running = false;
                self.status.error = Some(err.to_string());
            }
        }
        Ok(self.status.clone())
    }

    pub async fn stop(&mut self) -> Result<TransferStatus> {
        if let Some(service) = self.service.lock().await.take() {
            service.stop().await?;
        }
        self.status.running = false;
        self.status.fingerprint.clear();
        Ok(self.status.clone())
    }

    pub fn set_settings(
        &mut self,
        alias: String,
        port: u16,
        receive_dir: String,
        pin_configured: bool,
        auto_accept_favorites: bool,
    ) {
        self.status.alias = alias;
        self.status.port = port;
        self.status.receive_dir = receive_dir;
        self.status.pin_configured = pin_configured;
        self.status.auto_accept_favorites = auto_accept_favorites;
    }

    pub fn subscribe(&self) -> broadcast::Receiver<rtodo_transfer_core::EventEnvelope> {
        self.events.subscribe()
    }

    pub async fn devices(&self) -> Vec<rtodo_transfer_core::DeviceInfo> {
        match self.service.lock().await.as_ref() {
            Some(service) => service.devices().await,
            None => Vec::new(),
        }
    }

    pub async fn refresh(&self) -> Result<(), String> {
        let guard = self.service.lock().await;
        let service = guard.as_ref().ok_or("互传服务未启动")?;
        service.refresh().await.map_err(|e| e.to_string())
    }

    pub async fn add_manual_ip(
        &self,
        host: &str,
        port: u16,
    ) -> Result<Option<rtodo_transfer_core::DeviceInfo>, String> {
        let guard = self.service.lock().await;
        let service = guard.as_ref().ok_or("互传服务未启动")?;
        service
            .add_manual_ip(host, port)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn send(
        &self,
        paths: Vec<std::path::PathBuf>,
        fingerprint: &str,
        pin: Option<String>,
    ) -> Result<String, String> {
        let guard = self.service.lock().await;
        let service = guard.as_ref().ok_or("互传服务未启动")?;
        service
            .send(paths, fingerprint, pin)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn pending_incoming(&self) -> Vec<rtodo_transfer_core::PendingIncoming> {
        match self.service.lock().await.as_ref() {
            Some(service) => service.pending_incoming().await,
            None => Vec::new(),
        }
    }

    pub async fn respond_prepare(
        &self,
        session_id: &str,
        file_ids: Vec<String>,
    ) -> Result<(), String> {
        let guard = self.service.lock().await;
        let service = guard.as_ref().ok_or("互传服务未启动")?;
        service
            .respond_prepare(session_id, file_ids)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn respond_file(
        &self,
        session_id: &str,
        file_id: &str,
        path: &std::path::Path,
    ) -> Result<(), String> {
        let guard = self.service.lock().await;
        let service = guard.as_ref().ok_or("互传服务未启动")?;
        service
            .respond_file(session_id, file_id, path)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn reject(&self, session_id: &str) -> Result<(), String> {
        let guard = self.service.lock().await;
        let service = guard.as_ref().ok_or("互传服务未启动")?;
        service.reject(session_id).await.map_err(|e| e.to_string())
    }

    pub async fn cancel(&self, session_id: &str) -> Result<(), String> {
        let guard = self.service.lock().await;
        let service = guard.as_ref().ok_or("互传服务未启动")?;
        service.cancel(session_id).await.map_err(|e| e.to_string())
    }

    pub async fn send_message(
        &self,
        fingerprint: &str,
        message: &rtodo_transfer_core::ChatMessage,
    ) -> Result<(), String> {
        let guard = self.service.lock().await;
        let service = guard.as_ref().ok_or("互传服务未启动")?;
        service
            .send_message(fingerprint, message)
            .await
            .map_err(|e| e.to_string())
    }
}

pub fn default_transfer_dirs(
    data_dir: &std::path::Path,
) -> (std::path::PathBuf, std::path::PathBuf) {
    let receive = dirs_download().join("rtools");
    let identity = data_dir.join("transfer");
    (receive, identity)
}

fn dirs_download() -> std::path::PathBuf {
    std::env::var_os("USERPROFILE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("Downloads")
}
