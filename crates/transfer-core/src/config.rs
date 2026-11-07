use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// LocalSend 默认端口（TCP + UDP 共用）
pub const DEFAULT_PORT: u16 = 53317;

/// 互传服务配置。
///
/// `receive_dir` 由各端壳填充：
/// - Windows：默认 `Downloads\r-todo`
/// - Android：仅作为 UI 展示，实际落盘走 MediaStore
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferConfig {
    pub alias: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub receive_dir: PathBuf,
    #[serde(default)]
    pub pin: Option<String>,
    #[serde(default)]
    pub auto_accept_favorites: bool,
}

impl Default for TransferConfig {
    fn default() -> Self {
        Self {
            alias: "r-todo".to_string(),
            port: DEFAULT_PORT,
            receive_dir: PathBuf::new(),
            pin: None,
            auto_accept_favorites: false,
        }
    }
}

fn default_port() -> u16 {
    DEFAULT_PORT
}
