use chrono::{DateTime, Utc};
use rtools_domain::{Task, TaskPatch};
use serde::{Deserialize, Serialize};

pub const API_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairRequest {
    pub code: String,
    pub device_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairResponse {
    pub token: String,
    pub device_id: String,
    pub server_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingCodeResponse {
    pub code: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Operation {
    pub operation_id: String,
    pub task_id: String,
    #[serde(default)]
    pub base_revision: i64,
    pub client_updated_at: DateTime<Utc>,
    #[serde(default)]
    pub create: Option<Task>,
    #[serde(default)]
    pub patch: Option<TaskPatch>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationBatch {
    pub device_id: String,
    pub operations: Vec<Operation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationResult {
    pub operation_id: String,
    pub task_id: String,
    pub revision: i64,
    pub accepted: bool,
    #[serde(default)]
    pub task: Option<Task>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationResponse {
    pub cursor: i64,
    pub results: Vec<OperationResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangePage {
    pub cursor: i64,
    pub tasks: Vec<Task>,
    #[serde(default)]
    pub deleted_ids: Vec<String>,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub cursor: i64,
    pub tasks: Vec<Task>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeNotice {
    #[serde(rename = "type")]
    pub kind: String,
    pub cursor: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiError {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub retryable: bool,
}

impl ApiError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            retryable: false,
        }
    }
}
