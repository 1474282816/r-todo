//! 手动互传调试工具：向指定 `host:port` 发送若干文件。
//!
//! ```powershell
//! cargo run --example send -- 127.0.0.1 53317 .\报告.pdf
//! cargo run --example send -- 192.168.1.9 53317 .\a.zip .\b.png --alias "另一台"
//! ```
//!
//! 用途：
//! - 验收桌面端/手机端接收 UI（会触发对方的接收确认弹窗）
//! - 排查组播发现不到时的网络问题（手动 IP 直连）
//!
//! 本工具自己也会起一个临时服务（端口随机、临时身份），只作发送端。

use std::time::Duration;

use rtodo_transfer_core::{Direction, SessionEndReason, TransferConfig, TransferService};
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        eprintln!("用法: send <host> <port> <file>... [--alias 设备名] [--pin 1234]");
        std::process::exit(2);
    }

    // 解析可选参数
    let mut alias_filter: Option<String> = None;
    let mut pin: Option<String> = None;
    let mut positional: Vec<String> = Vec::new();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--alias" if index + 1 < args.len() => {
                alias_filter = Some(args[index + 1].clone());
                index += 2;
            }
            "--pin" if index + 1 < args.len() => {
                pin = Some(args[index + 1].clone());
                index += 2;
            }
            other => {
                positional.push(other.to_string());
                index += 1;
            }
        }
    }
    args.clear();

    if positional.len() < 3 {
        eprintln!("缺少参数：host port file...");
        std::process::exit(2);
    }

    let host = positional[0].clone();
    let port: u16 = positional[1].parse()?;
    let files: Vec<std::path::PathBuf> = positional[2..].iter().map(std::path::PathBuf::from).collect();

    let (tx, mut rx) = mpsc::channel(256);
    let service = TransferService::start(
        TransferConfig {
            alias: "r-todo send tool".to_string(),
            port: 0,
            receive_dir: std::env::temp_dir().join("rtodo-send-tool"),
            identity_dir: std::env::temp_dir().join("rtodo-send-tool-identity"),
            ..TransferConfig::default()
        },
        tx,
    )
    .await?;

    println!("本机 fingerprint: {}", service.fingerprint());
    println!("正在连接 {host}:{port} …");

    let device = service.add_manual_ip(&host, port).await?;
    let Some(device) = device else {
        eprintln!("连接失败：对方没有响应 register（端口/防火墙/协议不匹配）");
        service.stop().await?;
        std::process::exit(1);
    };

    println!(
        "发现设备: {} ({}) fingerprint={}",
        device.alias,
        device
            .device_model
            .clone()
            .unwrap_or_else(|| "未知型号".to_string()),
        device.fingerprint
    );

    let target = match &alias_filter {
        Some(alias) => service
            .devices()
            .await
            .into_iter()
            .find(|d| d.alias == *alias)
            .ok_or_else(|| format!("设备列表里没有别名为 {alias} 的设备"))?,
        None => device,
    };

    let session_id = service
        .send(files, &target.fingerprint, pin)
        .await?;

    println!("已发起会话 {session_id}，等待对端确认…（对方需要在弹窗里点「接收」）");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(600);
    let mut last_percent = -1i32;

    while tokio::time::Instant::now() < deadline {
        let Ok(Some(envelope)) = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await
        else {
            continue;
        };

        match envelope.event {
            rtodo_transfer_core::TransferEvent::Progress {
                direction: Direction::Send,
                percent,
                transferred,
                total,
                ..
            } => {
                let rounded = percent as i32;
                if rounded != last_percent {
                    last_percent = rounded;
                    println!("发送中 {rounded}% ({transferred}/{total} 字节)");
                }
            }
            rtodo_transfer_core::TransferEvent::Completed {
                direction: Direction::Send,
                path,
                ..
            } => {
                println!("已发送 {}", path.unwrap_or_default());
            }
            rtodo_transfer_core::TransferEvent::SessionEnded {
                session_id: ended,
                reason,
            } if ended == session_id => {
                println!(
                    "会话结束: {}",
                    match reason {
                        SessionEndReason::Completed => "完成",
                        SessionEndReason::Cancelled => "已取消",
                        SessionEndReason::Rejected => "对方拒绝",
                        SessionEndReason::Error => "出错",
                    }
                );
                service.stop().await?;
                std::process::exit(if reason == SessionEndReason::Completed { 0 } else { 1 });
            }
            rtodo_transfer_core::TransferEvent::Error { code, message, .. } => {
                eprintln!("错误 {code:?}: {message}");
            }
            _ => {}
        }
    }

    eprintln!("超时退出");
    service.stop().await?;
    std::process::exit(1);
}
