//! r-todo 局域网互传核心（LocalSend 兼容）
//!
//! 这个 crate 是桌面端（Tauri）与手机端（Flutter/FRB）共用的协议与状态层。
//!
//! 当前状态：**skeleton**。类型/事件/错误码/API 已冻结，协议实现尚未接入。
//! Desktop agent 负责把 LocalSend `packages/core` 接进来；Mobile agent 只消费这里的 API。
//!
//! 契约变更规则：
//! - `SCHEMA_VERSION` 变更必须同时更新桌面端、手机端和 `tests/contract.rs`
//! - 事件名/字段/错误码不允许单方面改动

mod config;
mod device;
mod error;
mod event;
mod service;

pub use config::{TransferConfig, DEFAULT_PORT};
pub use device::{DeviceChannel, DeviceInfo, DeviceSummary, Protocol};
pub use error::{ErrorCode, ErrorScope, TransferError};
pub use event::{
    Direction, EventEnvelope, FileInfo, SessionEndReason, TransferEvent, SCHEMA_VERSION,
};
pub use service::{EventSender, TransferService};
