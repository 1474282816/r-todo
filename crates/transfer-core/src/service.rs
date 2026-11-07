use std::path::{Path, PathBuf};

use tokio::sync::mpsc;

use crate::{
    DeviceInfo, EventEnvelope, TransferConfig, TransferError,
};

pub type EventSender = mpsc::Sender<EventEnvelope>;

/// 互传服务骨架。
///
/// 目前只保存配置和事件通道，协议实现（LocalSend core 接入）由 Desktop agent 完成。
/// Mobile agent 可以先用这个类型打通 FRB/Dart/Kotlin 链路。
#[derive(Debug)]
pub struct TransferService {
    config: TransferConfig,
    event_tx: EventSender,
}

impl TransferService {
    pub async fn start(
        config: TransferConfig,
        event_tx: EventSender,
    ) -> Result<Self, TransferError> {
        Ok(Self { config, event_tx })
    }

    pub fn config(&self) -> &TransferConfig {
        &self.config
    }

    /// 壳层可以 clone 这个 sender，把协议事件桥接给 Tauri / Dart。
    pub fn event_sender(&self) -> EventSender {
        self.event_tx.clone()
    }

    pub fn devices(&self) -> Vec<DeviceInfo> {
        Vec::new()
    }

    pub async fn refresh(&self) -> Result<(), TransferError> {
        Ok(())
    }

    pub async fn add_manual_ip(
        &self,
        _host: &str,
        _port: u16,
    ) -> Result<Option<DeviceInfo>, TransferError> {
        Err(TransferError::NotImplemented)
    }

    pub async fn send(
        &self,
        _files: Vec<PathBuf>,
        _fingerprint: &str,
        _pin: Option<String>,
    ) -> Result<String, TransferError> {
        Err(TransferError::NotImplemented)
    }

    pub async fn respond_prepare(
        &self,
        _session_id: &str,
        _accepted_file_ids: Vec<String>,
    ) -> Result<(), TransferError> {
        Err(TransferError::NotImplemented)
    }

    pub async fn respond_file(
        &self,
        _session_id: &str,
        _file_id: &str,
        _save_path: &Path,
    ) -> Result<(), TransferError> {
        Err(TransferError::NotImplemented)
    }

    pub async fn reject(&self, _session_id: &str) -> Result<(), TransferError> {
        Err(TransferError::NotImplemented)
    }

    pub async fn cancel(&self, _session_id: &str) -> Result<(), TransferError> {
        Err(TransferError::NotImplemented)
    }

    pub async fn stop(self) -> Result<(), TransferError> {
        Ok(())
    }
}
