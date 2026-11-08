//! `TransferError` → 统一的 JSON 错误串（Dart 侧解析出 code/scope/message）。

use rtodo_transfer_core::{ErrorCode, ErrorScope, TransferError};

pub fn to_json(err: &TransferError) -> String {
    // Protocol 变体的 Display 会带 "Scope/Code: " 前缀，这里只要人话部分。
    let message = match err {
        TransferError::Protocol { message, .. } => message.clone(),
        other => other.to_string(),
    };

    serde_json::json!({
        "code": err.code(),
        "scope": err.scope(),
        "message": message,
    })
    .to_string()
}

/// 服务未启动/已停止时给 Dart 的固定错误。
pub fn not_running() -> String {
    serde_json::json!({
        "code": ErrorCode::Unknown,
        "scope": ErrorScope::Server,
        "message": "互传服务未启动",
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(json: &str) -> serde_json::Value {
        serde_json::from_str(json).expect("valid json")
    }

    #[test]
    fn protocol_error_keeps_code_and_scope() {
        let err = TransferError::Protocol {
            scope: ErrorScope::Receive,
            code: ErrorCode::PinInvalid,
            message: "PIN 错误".into(),
        };

        let json = value(&to_json(&err));
        assert_eq!(json["code"], "PIN_INVALID");
        assert_eq!(json["scope"], "receive");
        assert_eq!(json["message"], "PIN 错误");
    }

    #[test]
    fn skeleton_not_implemented_maps_to_unknown_server() {
        let json = value(&to_json(&TransferError::NotImplemented));
        assert_eq!(json["code"], "UNKNOWN");
        assert_eq!(json["scope"], "server");
    }

    #[test]
    fn not_running_is_unknown() {
        let json = value(&not_running());
        assert_eq!(json["code"], "UNKNOWN");
        assert_eq!(json["scope"], "server");
        assert!(!json["message"].as_str().unwrap().is_empty());
    }
}
