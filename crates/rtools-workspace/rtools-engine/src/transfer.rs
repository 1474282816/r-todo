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
    ) -> Result<TransferStatus> {
        let config = TransferConfig {
            alias: "rtools".into(),
            receive_dir,
            identity_dir,
            ..TransferConfig::default()
        };
        let (tx, mut rx) = mpsc::channel(128);
        let events = self.events.clone();
        match TransferService::start(config, tx).await {
            Ok(service) => {
                self.status.running = true;
                self.status.port = service.port();
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
        Ok(self.status.clone())
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
