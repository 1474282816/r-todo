//! r-todo 手机端互传桥（FRB）。
//!
//! 职责边界：
//! - 协议与状态：`rtodo-transfer-core`（与桌面端共用，本 crate 只消费）
//! - 本 crate：配置校验、事件 JSON 转发、错误码映射、Dart 可调用的门面
//! - 文件/平台能力：Kotlin（SAF、MediaStore、前台服务）

pub mod api;

mod config_check;
mod error_json;
mod event_json;

mod frb_generated;
