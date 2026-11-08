//! r-todo 局域网互传核心（LocalSend 兼容）
//!
//! 这个 crate 是桌面端（Tauri）与手机端（Flutter/FRB）共用的协议与状态层。
//!
//! 协议实现基于 LocalSend 官方 Rust core（`crates/vendor/localsend-core`，v2 协议 +
//! mTLS + 组播发现）。本 crate 只负责：身份持久化、设备合并、会话状态、事件输出。
//!
//! 契约变更规则：
//! - `SCHEMA_VERSION` 变更必须同时更新桌面端、手机端和 `tests/contract.rs`
//! - 事件名/字段/错误码不允许单方面改动

pub mod chat;
mod client;
mod config;
mod device;
mod discovery;
mod error;
mod event;
mod identity;
mod path;
mod server;
mod service;

pub use chat::{ChatMessage, MessageKind, MessageStatus, MAX_PREVIEW_BYTES};
pub use config::{TransferConfig, DEFAULT_PORT};
pub use device::{DeviceChannel, DeviceInfo, DeviceSummary, PendingIncoming, Protocol};
pub use error::{ErrorCode, ErrorScope, TransferError};
pub use event::{
    Direction, EventEnvelope, EventSender, FileInfo, SessionEndReason, TransferEvent,
    SCHEMA_VERSION,
};
pub use identity::Identity;
pub use service::TransferService;
