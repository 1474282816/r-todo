//! 事件 → JSON 字符串（Dart 侧 `parseTransferEvent` 的唯一输入）。

use rtodo_transfer_core::EventEnvelope;

pub fn to_json(envelope: &EventEnvelope) -> String {
    envelope.to_json_string().unwrap_or_else(|_| {
        // 序列化失败本身也要让 Dart 收到一条可解析的错误，而不是静默丢事件。
        r#"{"schema_version":1,"type":"error","scope":"server","code":"UNKNOWN","message":"事件序列化失败"}"#
            .to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rtodo_transfer_core::{
        DeviceChannel, DeviceInfo, DeviceSummary, Direction, ErrorCode, ErrorScope, FileInfo,
        Protocol, SessionEndReason, TransferEvent, SCHEMA_VERSION,
    };

    fn value(json: &str) -> serde_json::Value {
        serde_json::from_str(json).expect("valid json")
    }

    #[test]
    fn progress_keeps_schema_version_and_fields() {
        let envelope = EventEnvelope::new(TransferEvent::Progress {
            session_id: "s1".into(),
            file_id: "f1".into(),
            direction: Direction::Send,
            transferred: 1024,
            total: 4096,
            percent: 25.0,
        });

        let json = value(&to_json(&envelope));
        assert_eq!(json["schema_version"], SCHEMA_VERSION);
        assert_eq!(json["type"], "progress");
        assert_eq!(json["direction"], "send");
        assert_eq!(json["transferred"], 1024);
    }

    #[test]
    fn incoming_keeps_chinese_file_name() {
        let envelope = EventEnvelope::new(TransferEvent::Incoming {
            session_id: "s2".into(),
            ip: "192.168.1.9".into(),
            device: DeviceSummary {
                alias: "PC".into(),
                fingerprint: "FP".into(),
            },
            files: vec![FileInfo {
                id: "f1".into(),
                file_name: "报告.pdf".into(),
                size: 1,
                file_type: Some("application/pdf".into()),
                sha256: None,
            }],
        });

        let json = value(&to_json(&envelope));
        assert_eq!(json["type"], "incoming");
        assert_eq!(json["files"][0]["file_name"], "报告.pdf");
    }

    #[test]
    fn devices_use_lowercase_protocol() {
        let envelope = EventEnvelope::new(TransferEvent::Devices {
            devices: vec![DeviceInfo {
                alias: "PC".into(),
                version: "2.2".into(),
                device_model: None,
                device_type: Some("desktop".into()),
                fingerprint: "ABC".into(),
                download: false,
                channels: vec![DeviceChannel {
                    host: "192.168.1.5".into(),
                    port: 53317,
                    protocol: Protocol::Https,
                }],
            }],
        });

        let json = value(&to_json(&envelope));
        assert_eq!(json["type"], "devices");
        assert_eq!(json["devices"][0]["channels"][0]["protocol"], "https");
    }

    #[test]
    fn error_and_session_ended_shapes() {
        let error = value(&to_json(&EventEnvelope::new(TransferEvent::Error {
            scope: ErrorScope::Receive,
            code: ErrorCode::PinInvalid,
            message: "PIN 错误".into(),
        })));
        assert_eq!(error["type"], "error");
        assert_eq!(error["code"], "PIN_INVALID");
        assert_eq!(error["scope"], "receive");

        let ended = value(&to_json(&EventEnvelope::new(
            TransferEvent::SessionEnded {
                session_id: "s3".into(),
                reason: SessionEndReason::Completed,
            },
        )));
        assert_eq!(ended["type"], "session_ended");
        assert_eq!(ended["reason"], "completed");
    }
}
