use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::{DeviceInfo, DeviceSummary, ErrorCode, ErrorScope};

/// 事件输出通道：壳层（Tauri / FRB）消费后映射成各自的事件系统。
pub type EventSender = mpsc::Sender<EventEnvelope>;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Send,
    Receive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionEndReason {
    Completed,
    Cancelled,
    Rejected,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileInfo {
    pub id: String,
    pub file_name: String,
    pub size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

/// 平台无关事件。桌面端映射为 Tauri event，手机端映射为 Dart Stream。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TransferEvent {
    Devices {
        devices: Vec<DeviceInfo>,
    },
    Incoming {
        session_id: String,
        ip: String,
        device: DeviceSummary,
        files: Vec<FileInfo>,
    },
    Progress {
        session_id: String,
        file_id: String,
        direction: Direction,
        transferred: u64,
        total: u64,
        percent: f32,
    },
    Completed {
        session_id: String,
        file_id: String,
        direction: Direction,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },
    Error {
        scope: ErrorScope,
        code: ErrorCode,
        message: String,
    },
    SessionEnded {
        session_id: String,
        reason: SessionEndReason,
    },
}

/// 事件信封：所有对外 JSON 都必须带 schema_version。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub schema_version: u32,
    #[serde(flatten)]
    pub event: TransferEvent,
}

impl EventEnvelope {
    pub fn new(event: TransferEvent) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            event,
        }
    }

    pub fn to_json_value(&self) -> Result<serde_json::Value, serde_json::Error> {
        serde_json::to_value(self)
    }

    pub fn to_json_string(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}
