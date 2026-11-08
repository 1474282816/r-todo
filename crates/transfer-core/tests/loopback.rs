//! 回环集成测试：两个服务实例通过 HTTPS v2 协议互传一个文件。
//!
//! 不依赖真实网卡组播：用 `add_manual_ip` 手动注册 127.0.0.1。

use std::sync::Arc;
use std::time::Duration;

use rtodo_transfer_core::{
    Direction, EventEnvelope, SessionEndReason, TransferConfig, TransferEvent, TransferService,
};
use tokio::sync::mpsc;

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rtodo_loopback_{tag}_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn config(dir: &std::path::Path) -> TransferConfig {
    TransferConfig {
        alias: "test".to_string(),
        port: 0,
        receive_dir: dir.join("inbox"),
        identity_dir: dir.join("identity"),
        ..TransferConfig::default()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn loopback_send_and_receive() {
    let root = temp_dir("pair");

    let (a_tx, mut a_rx) = mpsc::channel::<EventEnvelope>(128);
    let (b_tx, mut b_rx) = mpsc::channel::<EventEnvelope>(128);

    let receiver = Arc::new(
        TransferService::start(config(&root.join("b")), b_tx)
            .await
            .expect("start receiver"),
    );
    let sender = Arc::new(
        TransferService::start(config(&root.join("a")), a_tx)
            .await
            .expect("start sender"),
    );

    let receiver_fingerprint = receiver.fingerprint().to_string();
    let receiver_port = receiver.port();

    let found = sender
        .add_manual_ip("127.0.0.1", receiver_port)
        .await
        .expect("manual ip");
    assert!(found.is_some(), "手动 IP 应能发现接收端");

    let src = root.join("hello.txt");
    std::fs::write(&src, b"hello r-todo transfer").expect("write source");

    let send_task = tokio::spawn({
        let sender = sender.clone();
        let src = src.clone();
        async move { sender.send(vec![src], &receiver_fingerprint, None).await }
    });

    let mut received_path: Option<String> = None;
    let mut receiver_done = false;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while tokio::time::Instant::now() < deadline && !receiver_done {
        match tokio::time::timeout(Duration::from_millis(500), b_rx.recv()).await {
            Ok(Some(envelope)) => match envelope.event {
                TransferEvent::Incoming {
                    session_id, files, ..
                } => {
                    let ids: Vec<String> = files.iter().map(|f| f.id.clone()).collect();
                    receiver
                        .respond_prepare(&session_id, ids)
                        .await
                        .expect("accept prepare");
                }
                TransferEvent::Completed {
                    direction: Direction::Receive,
                    path,
                    ..
                } => {
                    received_path = path;
                }
                TransferEvent::Error { message, .. } => {
                    panic!("receiver error: {message}");
                }
                TransferEvent::SessionEnded { reason, .. } => {
                    assert_eq!(reason, SessionEndReason::Completed);
                    receiver_done = true;
                }
                _ => {}
            },
            Ok(None) => break,
            Err(_) => {}
        }
    }

    assert!(receiver_done, "接收方没有在超时内结束会话");

    let path = received_path.expect("应记录落盘路径");
    let content = std::fs::read(&path).expect("read received file");
    assert_eq!(content, b"hello r-todo transfer");
    assert!(path.ends_with("hello.txt"));

    let session_id = send_task.await.expect("send task").expect("send started");

    // 发送侧应收到 Completed + SessionEnded(Completed)
    let mut sender_done = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while tokio::time::Instant::now() < deadline && !sender_done {
        match tokio::time::timeout(Duration::from_millis(500), a_rx.recv()).await {
            Ok(Some(envelope)) => {
                if let TransferEvent::SessionEnded {
                    session_id: ended,
                    reason,
                } = envelope.event
                {
                    if ended == session_id {
                        assert_eq!(reason, SessionEndReason::Completed);
                        sender_done = true;
                    }
                }
            }
            Ok(None) => break,
            Err(_) => {}
        }
    }
    assert!(sender_done, "发送方没有在超时内结束会话");

    let _ = std::fs::remove_dir_all(&root);
}

/// `respond_file` 指定落盘路径：接收端应落在用户选的路径（而不是接收目录）。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn loopback_custom_save_path() {
    let root = temp_dir("custom");

    let (a_tx, _a_rx) = mpsc::channel::<EventEnvelope>(128);
    let (b_tx, mut b_rx) = mpsc::channel::<EventEnvelope>(128);

    let receiver = Arc::new(
        TransferService::start(config(&root.join("b")), b_tx)
            .await
            .expect("start receiver"),
    );
    let sender = Arc::new(
        TransferService::start(config(&root.join("a")), a_tx)
            .await
            .expect("start sender"),
    );

    let fingerprint = receiver.fingerprint().to_string();
    let port = receiver.port();
    assert!(sender.add_manual_ip("127.0.0.1", port).await.expect("manual").is_some());

    let src = root.join("src.txt");
    std::fs::write(&src, b"custom path payload").expect("write");

    tokio::spawn({
        let sender = sender.clone();
        let src = src.clone();
        async move {
            let _ = sender.send(vec![src], &fingerprint, None).await;
        }
    });

    let custom = root.join("chosen").join("renamed.txt");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    let mut ended = false;

    while tokio::time::Instant::now() < deadline && !ended {
        let Ok(Some(envelope)) = tokio::time::timeout(Duration::from_millis(500), b_rx.recv()).await
        else {
            continue;
        };

        match envelope.event {
            TransferEvent::Incoming {
                session_id, files, ..
            } => {
                let ids: Vec<String> = files.iter().map(|f| f.id.clone()).collect();
                for file in &files {
                    receiver
                        .respond_file(&session_id, &file.id, &custom)
                        .await
                        .expect("respond file");
                }
                receiver
                    .respond_prepare(&session_id, ids)
                    .await
                    .expect("accept");
            }
            TransferEvent::SessionEnded { reason, .. } => {
                assert_eq!(reason, SessionEndReason::Completed);
                ended = true;
            }
            TransferEvent::Error { message, .. } => panic!("receiver error: {message}"),
            _ => {}
        }
    }

    assert!(ended, "会话没有结束");
    let content = std::fs::read(&custom).expect("自定义路径应存在文件");
    assert_eq!(content, b"custom path payload");

    let _ = std::fs::remove_dir_all(&root);
}

/// PIN：无 PIN 被拒（PIN_REQUIRED），PIN 正确则正常传输。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn loopback_pin_required_then_accepted() {
    let root = temp_dir("pin");

    let (a_tx, mut a_rx) = mpsc::channel::<EventEnvelope>(256);
    let (b_tx, mut b_rx) = mpsc::channel::<EventEnvelope>(256);

    let mut receiver_config = config(&root.join("b"));
    receiver_config.pin = Some("1234".to_string());
    let receiver = Arc::new(
        TransferService::start(receiver_config, b_tx)
            .await
            .expect("start receiver"),
    );
    let sender = Arc::new(
        TransferService::start(config(&root.join("a")), a_tx)
            .await
            .expect("start sender"),
    );

    let fingerprint = receiver.fingerprint().to_string();
    let port = receiver.port();
    assert!(sender.add_manual_ip("127.0.0.1", port).await.expect("manual").is_some());

    let src = root.join("pin.txt");
    std::fs::write(&src, b"pin payload").expect("write");

    // 1) 不带 PIN
    let session = sender
        .send(vec![src.clone()], &fingerprint, None)
        .await
        .expect("send without pin");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    let mut got_pin_required = false;
    let mut ended = false;
    while tokio::time::Instant::now() < deadline && !ended {
        let Ok(Some(envelope)) = tokio::time::timeout(Duration::from_millis(500), a_rx.recv()).await
        else {
            continue;
        };
        match envelope.event {
            TransferEvent::Error { code, .. } => {
                if code == rtodo_transfer_core::ErrorCode::PinRequired {
                    got_pin_required = true;
                }
            }
            TransferEvent::SessionEnded {
                session_id,
                reason,
            } if session_id == session => {
                assert_eq!(reason, SessionEndReason::Error);
                ended = true;
            }
            _ => {}
        }
    }
    assert!(got_pin_required, "无 PIN 时应报 PIN_REQUIRED");
    assert!(ended, "无 PIN 会话应结束");

    // 2) 带正确 PIN
    tokio::spawn({
        let sender = sender.clone();
        let src = src.clone();
        let fingerprint = fingerprint.clone();
        async move {
            let _ = sender
                .send(vec![src], &fingerprint, Some("1234".to_string()))
                .await;
        }
    });

    let mut received = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while tokio::time::Instant::now() < deadline && !received {
        let Ok(Some(envelope)) = tokio::time::timeout(Duration::from_millis(500), b_rx.recv()).await
        else {
            continue;
        };
        match envelope.event {
            TransferEvent::Incoming {
                session_id, files, ..
            } => {
                let ids: Vec<String> = files.iter().map(|f| f.id.clone()).collect();
                receiver
                    .respond_prepare(&session_id, ids)
                    .await
                    .expect("accept with pin");
            }
            TransferEvent::SessionEnded { reason, .. } => {
                assert_eq!(reason, SessionEndReason::Completed);
                received = true;
            }
            TransferEvent::Error { message, .. } => panic!("receiver error: {message}"),
            _ => {}
        }
    }
    assert!(received, "带正确 PIN 应传输完成");

    let _ = std::fs::remove_dir_all(&root);
}

/// 接收方中途取消：不能再落一个完整文件，也不留 `.part`。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn loopback_receiver_cancel_discards_file() {
    let root = temp_dir("cancel");

    let (a_tx, mut a_rx) = mpsc::channel::<EventEnvelope>(512);
    let (b_tx, mut b_rx) = mpsc::channel::<EventEnvelope>(512);

    let receiver = Arc::new(
        TransferService::start(config(&root.join("b")), b_tx)
            .await
            .expect("start receiver"),
    );
    let sender = Arc::new(
        TransferService::start(config(&root.join("a")), a_tx)
            .await
            .expect("start sender"),
    );

    let fingerprint = receiver.fingerprint().to_string();
    let port = receiver.port();
    assert!(sender.add_manual_ip("127.0.0.1", port).await.expect("manual").is_some());

    let src = root.join("big.bin");
    std::fs::write(&src, vec![7u8; 64 * 1024 * 1024]).expect("write big file");

    tokio::spawn({
        let sender = sender.clone();
        let src = src.clone();
        let fingerprint = fingerprint.clone();
        async move {
            let _ = sender.send(vec![src], &fingerprint, None).await;
        }
    });

    let inbox = root.join("b").join("inbox");
    let mut cancelled = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);

    while tokio::time::Instant::now() < deadline {
        let Ok(Some(envelope)) = tokio::time::timeout(Duration::from_millis(300), b_rx.recv()).await
        else {
            if cancelled {
                break;
            }
            continue;
        };

        match envelope.event {
            TransferEvent::Incoming { session_id, files, .. } => {
                let ids: Vec<String> = files.iter().map(|f| f.id.clone()).collect();
                receiver.respond_prepare(&session_id, ids).await.expect("accept");
            }
            TransferEvent::Progress { session_id, .. } => {
                // 传了一点之后取消
                receiver.cancel(&session_id).await.expect("cancel");
                cancelled = true;
            }
            TransferEvent::SessionEnded { reason, .. } => {
                assert_eq!(reason, SessionEndReason::Cancelled, "用户取消后会话应为 Cancelled");
                break;
            }
            TransferEvent::Error { .. } => {}
            _ => {}
        }
    }
    assert!(cancelled, "应该在传输中触发过取消");

    // 等落盘任务收尾，然后断言没有残留（最终文件或 .part）
    let deadline = tokio::time::Instant::now() + Duration::from_secs(25);
    loop {
        let leftovers: Vec<String> = std::fs::read_dir(&inbox)
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .collect()
            })
            .unwrap_or_default();

        if leftovers.is_empty() {
            break;
        }
        if tokio::time::Instant::now() > deadline {
            panic!("取消后仍有残留文件: {leftovers:?}");
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    // 发送方也要有明确结局（取消或报错），不能一直挂着
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    let mut sender_done = false;
    while tokio::time::Instant::now() < deadline && !sender_done {
        match tokio::time::timeout(Duration::from_millis(300), a_rx.recv()).await {
            Ok(Some(envelope)) => {
                if matches!(envelope.event, TransferEvent::SessionEnded { .. }) {
                    sender_done = true;
                }
            }
            Ok(None) => break,
            Err(_) => {}
        }
    }
    assert!(sender_done, "发送方没有在超时内结束会话");

    let _ = std::fs::remove_dir_all(&root);
}
