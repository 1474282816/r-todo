use serde::{Deserialize, Serialize};

/// 通道协议。默认 HTTPS；HTTP 仅用于调试/兜底。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Http,
    Https,
}

impl Default for Protocol {
    fn default() -> Self {
        Self::Https
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceChannel {
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub protocol: Protocol,
}

/// 已发现设备的完整信息。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub alias: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_model: Option<String>,
    /// 保持字符串，避免与上游枚举值强耦合（mobile/desktop/web/headless/server）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_type: Option<String>,
    pub fingerprint: String,
    #[serde(default)]
    pub download: bool,
    pub channels: Vec<DeviceChannel>,
}

/// 事件里的设备摘要（不保证有 channels）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceSummary {
    pub alias: String,
    pub fingerprint: String,
}

/// 等待用户确认的接收请求（用于窗口重开/前端重载后恢复弹窗）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingIncoming {
    pub session_id: String,
    pub ip: String,
    pub device: DeviceSummary,
    pub files: Vec<crate::FileInfo>,
}
