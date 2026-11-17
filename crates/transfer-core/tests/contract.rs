use rtodo_transfer_core::{
    ChatMessage, DeviceChannel, DeviceInfo, DeviceSummary, Direction, ErrorCode, ErrorScope,
    EventEnvelope, FileInfo, MessageKind, Protocol, SessionEndReason, TransferConfig,
    TransferEvent, TransferService, DEFAULT_PORT, SCHEMA_VERSION,
};

#[test]
fn progress_event_has_frozen_shape() {
    let envelope = EventEnvelope::new(TransferEvent::Progress {
        session_id: "s1".into(),
        file_id: "f1".into(),
        direction: Direction::Send,
        transferred: 1024,
        total: 4096,
        percent: 25.0,
    });

    let value = envelope.to_json_value().expect("serialize");

    assert_eq!(value["schema_version"], SCHEMA_VERSION);
    assert_eq!(value["type"], "progress");
    assert_eq!(value["session_id"], "s1");
    assert_eq!(value["file_id"], "f1");
    assert_eq!(value["direction"], "send");
    assert_eq!(value["transferred"], 1024);
    assert_eq!(value["total"], 4096);
    assert_eq!(value["percent"], 25.0);
}

#[test]
fn incoming_event_keeps_file_fields() {
    let envelope = EventEnvelope::new(TransferEvent::Incoming {
        session_id: "s2".into(),
        ip: "192.168.1.9".into(),
        device: DeviceSummary {
            alias: "Phone".into(),
            fingerprint: "FP".into(),
        },
        files: vec![FileInfo {
            id: "f1".into(),
            file_name: "报告.pdf".into(),
            size: 12345,
            file_type: Some("application/pdf".into()),
            sha256: None,
            preview: None,
        }],
    });

    let value = envelope.to_json_value().expect("serialize");

    assert_eq!(value["type"], "incoming");
    assert_eq!(value["device"]["alias"], "Phone");
    assert_eq!(value["files"][0]["file_name"], "报告.pdf");
    assert_eq!(value["files"][0]["size"], 12345);
    assert!(value["files"][0].get("sha256").is_none());
}

#[test]
fn devices_event_uses_lowercase_protocol() {
    let envelope = EventEnvelope::new(TransferEvent::Devices {
        devices: vec![DeviceInfo {
            alias: "PC".into(),
            version: "2.2".into(),
            device_model: Some("Windows".into()),
            device_type: Some("desktop".into()),
            fingerprint: "ABC".into(),
            download: false,
            channels: vec![DeviceChannel {
                host: "192.168.1.5".into(),
                port: DEFAULT_PORT,
                protocol: Protocol::Https,
            }],
        }],
    });

    let value = envelope.to_json_value().expect("serialize");

    assert_eq!(value["type"], "devices");
    assert_eq!(value["devices"][0]["channels"][0]["protocol"], "https");
}

#[test]
fn error_code_is_screaming_snake_case() {
    assert_eq!(
        serde_json::to_string(&ErrorCode::PinInvalid).unwrap(),
        "\"PIN_INVALID\""
    );
    assert_eq!(
        serde_json::to_string(&ErrorScope::Receive).unwrap(),
        "\"receive\""
    );
}

#[test]
fn session_end_reason_is_lowercase() {
    let envelope = EventEnvelope::new(TransferEvent::SessionEnded {
        session_id: "s3".into(),
        reason: SessionEndReason::Completed,
    });

    let value = envelope.to_json_value().expect("serialize");
    assert_eq!(value["type"], "session_ended");
    assert_eq!(value["reason"], "completed");
}

#[test]
fn envelope_roundtrips() {
    let envelope = EventEnvelope::new(TransferEvent::Completed {
        session_id: "s4".into(),
        file_id: "f4".into(),
        direction: Direction::Receive,
        path: Some("C:\\Downloads\\r-todo\\a.pdf".into()),
    });

    let json = envelope.to_json_string().expect("serialize");
    let back: EventEnvelope = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(back, envelope);
}

#[test]
fn config_defaults_to_localsend_port() {
    let config = TransferConfig::default();
    assert_eq!(config.port, DEFAULT_PORT);
    assert_eq!(config.port, 54417);
    assert!(!config.auto_accept_favorites);
    assert!(config.pin.is_none());
}

#[tokio::test]
async fn service_starts_and_stops_on_free_port() {
    let (tx, _rx) = tokio::sync::mpsc::channel(64);

    let dir = std::env::temp_dir().join(format!("rtodo_transfer_contract_{}", std::process::id()));
    let config = TransferConfig {
        port: 0,
        identity_dir: dir.clone(),
        receive_dir: dir.join("inbox"),
        ..TransferConfig::default()
    };

    let service = TransferService::start(config, tx)
        .await
        .expect("service start");

    assert_eq!(service.fingerprint().len(), 64, "fingerprint 是大写 hex 的 SHA-256");
    assert!(service
        .fingerprint()
        .chars()
        .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_lowercase()));
    assert!(service.devices().await.is_empty());

    // 没有目标设备时发送应明确失败，而不是 panic
    let err = service
        .send(Vec::new(), "FP", None)
        .await
        .expect_err("empty send must fail");
    assert_eq!(err.code(), ErrorCode::Io);

    service.stop().await.expect("stop");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn schema_version_is_v2() {
    assert_eq!(SCHEMA_VERSION, 2);
}

#[test]
fn message_event_has_frozen_shape() {
    let message =
        ChatMessage::new("m1", "PEER", "SELF", "PC", MessageKind::Text, "你好");
    let envelope = EventEnvelope::new(TransferEvent::Message { message });
    let value = envelope.to_json_value().expect("serialize");

    assert_eq!(value["schema_version"], 2);
    assert_eq!(value["type"], "message");
    assert_eq!(value["message"]["kind"], "text");
    assert_eq!(value["message"]["body"], "你好");
    assert_eq!(value["message"]["conversation_id"], "PEER");

    let back: EventEnvelope = serde_json::from_value(value).expect("deserialize");
    assert_eq!(back, envelope);
}

#[test]
fn trust_message_roundtrips() {
    let message = ChatMessage::new("m2", "PEER", "SELF", "PC", MessageKind::Trust, "trust");
    let envelope = EventEnvelope::new(TransferEvent::Message { message });
    let json = envelope.to_json_string().expect("serialize");
    let back: EventEnvelope = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, envelope);
}

#[test]
fn incoming_preview_is_optional_but_preserved() {
    let mut file = FileInfo {
        id: "f1".into(),
        file_name: "a.png".into(),
        size: 10,
        file_type: Some("image/png".into()),
        sha256: None,
        preview: Some("BASE64".into()),
    };
    let envelope = EventEnvelope::new(TransferEvent::Incoming {
        session_id: "s".into(),
        ip: "192.168.1.9".into(),
        device: DeviceSummary {
            alias: "Phone".into(),
            fingerprint: "FP".into(),
        },
        files: vec![file.clone()],
    });
    let value = envelope.to_json_value().expect("serialize");
    assert_eq!(value["files"][0]["preview"], "BASE64");

    file.preview = None;
    let envelope = EventEnvelope::new(TransferEvent::Incoming {
        session_id: "s".into(),
        ip: "192.168.1.9".into(),
        device: DeviceSummary {
            alias: "Phone".into(),
            fingerprint: "FP".into(),
        },
        files: vec![file],
    });
    let value = envelope.to_json_value().expect("serialize");
    assert!(value["files"][0].get("preview").is_none());
}
