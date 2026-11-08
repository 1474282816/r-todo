//! core 事件 → Tauri 事件名与 payload 映射。
//!
//! 事件名与手机端保持一致（连字符，避免 `:` / `//` 兼容问题）。

use rtodo_transfer_core::{EventEnvelope, TransferEvent};

pub const DEVICES: &str = "transfer-devices";
pub const INCOMING: &str = "transfer-incoming";
pub const PROGRESS: &str = "transfer-progress";
pub const COMPLETED: &str = "transfer-completed";
pub const ERROR: &str = "transfer-error";
pub const SESSION_ENDED: &str = "transfer-session-ended";
pub const MESSAGE: &str = "transfer-message";

/// 返回 `(事件名, payload)`；payload 是带 `schema_version` 的完整信封。
pub fn map(envelope: &EventEnvelope) -> (&'static str, serde_json::Value) {
    let name = match &envelope.event {
        TransferEvent::Devices { .. } => DEVICES,
        TransferEvent::Incoming { .. } => INCOMING,
        TransferEvent::Progress { .. } => PROGRESS,
        TransferEvent::Completed { .. } => COMPLETED,
        TransferEvent::Error { .. } => ERROR,
        TransferEvent::SessionEnded { .. } => SESSION_ENDED,
        TransferEvent::Message { .. } => MESSAGE,
    };

    let payload = serde_json::to_value(envelope).unwrap_or_else(|_| serde_json::json!({}));
    (name, payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rtodo_transfer_core::{Direction, SessionEndReason, TransferEvent};

    #[test]
    fn maps_session_ended_to_hyphenated_name() {
        let envelope = EventEnvelope::new(TransferEvent::SessionEnded {
            session_id: "s1".into(),
            reason: SessionEndReason::Completed,
        });
        let (name, payload) = map(&envelope);
        assert_eq!(name, "transfer-session-ended");
        assert_eq!(payload["schema_version"], rtodo_transfer_core::SCHEMA_VERSION);
        assert_eq!(payload["session_id"], "s1");
        assert_eq!(payload["type"], "session_ended");
    }

    #[test]
    fn maps_progress_direction() {
        let envelope = EventEnvelope::new(TransferEvent::Progress {
            session_id: "s1".into(),
            file_id: "f1".into(),
            direction: Direction::Receive,
            transferred: 1,
            total: 2,
            percent: 50.0,
        });
        let (name, payload) = map(&envelope);
        assert_eq!(name, "transfer-progress");
        assert_eq!(payload["direction"], "receive");
    }

    #[test]
    fn maps_message_to_hyphenated_name() {
        let message = rtodo_transfer_core::ChatMessage::new(
            "m1",
            "PEER",
            "SELF",
            "PC",
            rtodo_transfer_core::MessageKind::Text,
            "你好",
        );
        let envelope = EventEnvelope::new(TransferEvent::Message { message });
        let (name, payload) = map(&envelope);
        assert_eq!(name, "transfer-message");
        assert_eq!(payload["message"]["body"], "你好");
        assert_eq!(payload["schema_version"], 2);
    }
}
