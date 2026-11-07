use serde::{Deserialize, Serialize};
use thiserror::Error;

/// 对外错误码。桌面/手机必须使用同一套字符串。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    PortInUse,
    Firewall,
    MulticastUnavailable,
    Tls,
    PinRequired,
    PinInvalid,
    Rejected,
    ChecksumMismatch,
    Cancelled,
    Io,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ErrorScope {
    Send,
    Receive,
    Discovery,
    Server,
}

#[derive(Debug, Error)]
pub enum TransferError {
    #[error("not implemented yet")]
    NotImplemented,
    #[error("invalid config: {0}")]
    InvalidConfig(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("{scope:?}/{code:?}: {message}")]
    Protocol {
        scope: ErrorScope,
        code: ErrorCode,
        message: String,
    },
}

impl TransferError {
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::NotImplemented | Self::InvalidConfig(_) => ErrorCode::Unknown,
            Self::Io(_) => ErrorCode::Io,
            Self::Protocol { code, .. } => *code,
        }
    }

    pub fn scope(&self) -> ErrorScope {
        match self {
            Self::NotImplemented | Self::InvalidConfig(_) | Self::Io(_) => ErrorScope::Server,
            Self::Protocol { scope, .. } => *scope,
        }
    }
}
