use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// RTools 默认端口（TCP + UDP 共用）。
///
/// 官方 LocalSend 使用 53317，但 Windows Hyper-V/WSL 经常把该端口纳入保留段，
/// 导致绑定直接失败。使用独立高位端口可以避免和官方 LocalSend 争用；
/// 发现协议会携带真实端口，已配对设备无需硬编码。
pub const DEFAULT_PORT: u16 = 54417;

/// 互传服务配置。
///
/// `receive_dir` / `identity_dir` 由各端壳填充：
/// - Windows：`Downloads\r-todo` + `app_data_dir\transfer`
/// - Android：接收目录仅作 UI 展示（实际落盘走 MediaStore），身份目录用 App 私有目录
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferConfig {
    pub alias: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub receive_dir: PathBuf,
    /// 证书/私钥持久化目录。留空时退化为临时目录（身份随进程重建）。
    #[serde(default)]
    pub identity_dir: PathBuf,
    #[serde(default)]
    pub pin: Option<String>,
    #[serde(default)]
    pub auto_accept_favorites: bool,
    /// 接收时是否校验发送方给出的 SHA-256（默认校验）。
    #[serde(default = "default_true")]
    pub verify_checksums: bool,
}

impl Default for TransferConfig {
    fn default() -> Self {
        Self {
            alias: "r-todo".to_string(),
            port: DEFAULT_PORT,
            receive_dir: PathBuf::new(),
            identity_dir: PathBuf::new(),
            pin: None,
            auto_accept_favorites: false,
            verify_checksums: true,
        }
    }
}

impl TransferConfig {
    /// 实际使用的接收目录：未配置时退回系统临时目录下的 `r-todo`。
    pub fn receive_dir_or_default(&self) -> PathBuf {
        if self.receive_dir.as_os_str().is_empty() {
            std::env::temp_dir().join("r-todo")
        } else {
            self.receive_dir.clone()
        }
    }

    /// 实际使用的身份目录：未配置时退回系统临时目录下的 `r-todo-transfer`。
    pub fn identity_dir_or_default(&self) -> PathBuf {
        if self.identity_dir.as_os_str().is_empty() {
            std::env::temp_dir().join("r-todo-transfer")
        } else {
            self.identity_dir.clone()
        }
    }
}

fn default_port() -> u16 {
    DEFAULT_PORT
}

fn default_true() -> bool {
    true
}
