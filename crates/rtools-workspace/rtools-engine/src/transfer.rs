use anyhow::Result;
use rtodo_transfer_core::{TransferConfig, TransferService};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
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
        }
    }

    pub fn status(&self) -> TransferStatus {
        self.status.clone()
    }

    pub async fn start(&mut self, receive_dir: std::path::PathBuf, identity_dir: std::path::PathBuf) -> Result<TransferStatus> {
        let config = TransferConfig {
            alias: "rtools".into(),
            receive_dir,
            identity_dir,
            ..TransferConfig::default()
        };
        let (tx, mut rx) = mpsc::channel(128);
        match TransferService::start(config, tx).await {
            Ok(service) => {
                self.status.running = true;
                self.status.port = service.port();
                self.status.error = None;
                *self.service.lock().await = Some(service);
                tokio::spawn(async move {
                    while let Some(event) = rx.recv().await {
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
}

pub fn default_transfer_dirs(data_dir: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf) {
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

