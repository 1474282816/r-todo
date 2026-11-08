//! 自动接收对端：`cargo run --example serve -- <接收目录> [端口]`
//!
//! 起一个互传服务并**自动接受所有接收请求**，把文件落到指定目录。
//! 用于自动化验收「真实 app 发送 → 本工具接收」或反向调试。
//!
//! 输出（每行一条，便于脚本解析）：
//! ```text
//! LISTEN_PORT=53319
//! FINGERPRINT=....
//! INCOMING=2 files from 192.168.1.9
//! SAVED=E:\tmp\inbox\a.pdf
//! ENDED=Completed
//! ```

use std::time::Duration;

use rtodo_transfer_core::{
    Direction, SessionEndReason, TransferConfig, TransferService, TransferEvent,
};
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let receive_dir = args
        .next()
        .unwrap_or_else(|| std::env::temp_dir().join("rtodo-serve").to_string_lossy().to_string());
    let port: u16 = args.next().and_then(|p| p.parse().ok()).unwrap_or(0);

    let (tx, mut rx) = mpsc::channel(256);
    let service = TransferService::start(
        TransferConfig {
            alias: "r-todo peer".to_string(),
            port,
            receive_dir: std::path::PathBuf::from(&receive_dir),
            identity_dir: std::env::temp_dir().join("rtodo-serve-identity"),
            ..TransferConfig::default()
        },
        tx,
    )
    .await?;

    println!("LISTEN_PORT={}", service.port());
    println!("FINGERPRINT={}", service.fingerprint());
    println!("RECEIVE_DIR={receive_dir}");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(300);

    loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        if left.is_zero() {
            println!("TIMEOUT");
            break;
        }

        let Ok(Some(envelope)) = tokio::time::timeout(left, rx.recv()).await else {
            continue;
        };

        match envelope.event {
            TransferEvent::Incoming {
                session_id, files, ..
            } => {
                println!("INCOMING={} files", files.len());
                let ids: Vec<String> = files.iter().map(|f| f.id.clone()).collect();
                if let Err(err) = service.respond_prepare(&session_id, ids).await {
                    eprintln!("ACCEPT_FAILED={err}");
                }
            }
            TransferEvent::Completed {
                direction: Direction::Receive,
                path,
                ..
            } => println!("SAVED={}", path.unwrap_or_default()),
            TransferEvent::Error { code, message, .. } => {
                eprintln!("ERR={code:?}: {message}");
            }
            TransferEvent::SessionEnded { reason, .. } => {
                println!(
                    "ENDED={}",
                    match reason {
                        SessionEndReason::Completed => "Completed",
                        SessionEndReason::Cancelled => "Cancelled",
                        SessionEndReason::Rejected => "Rejected",
                        SessionEndReason::Error => "Error",
                    }
                );
                break;
            }
            _ => {}
        }
    }

    service.stop().await?;
    Ok(())
}
