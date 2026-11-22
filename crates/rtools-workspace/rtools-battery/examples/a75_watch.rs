//! 临时排查工具：同时盯住 A75 的 Feature 0x06 和 0xFF02 输入报文，
//! 用来抓"插线 / 拔线"这一瞬间设备到底发了什么。
//!
//! 全程只读，不发任何命令。

use std::time::{Duration, Instant};

use rtools_battery::hid::{self, HidDevice};

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn stamp(start: Instant) -> String {
    format!("{:>7.2}s", start.elapsed().as_secs_f64())
}

fn main() {
    let seconds: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(300);

    let interfaces = hid::enumerate();
    let feat_iface = interfaces
        .iter()
        .find(|i| i.vendor_id == 0x3554 && i.usage_page == 0xFF04 && i.feature_len > 0);
    let in_iface = interfaces.iter().find(|i| {
        i.vendor_id == 0x3554 && i.usage_page == 0xFF02 && i.input_len > 0
    });

    let (Some(fi), Some(ii)) = (feat_iface, in_iface) else {
        println!("没找到 A75（3554:FA09）的接口，退出了");
        return;
    };

    let feat = match HidDevice::open(fi, false) {
        Ok(d) => d,
        Err(e) => {
            println!("打开 Feature 接口失败：{e}");
            return;
        }
    };
    let input = match HidDevice::open(ii, false) {
        Ok(d) => d,
        Err(e) => {
            println!("打开 0xFF02 读接口失败：{e}");
            return;
        }
    };

    let start = Instant::now();
    let deadline = start + Duration::from_secs(seconds);
    let mut last_feature = String::new();
    let mut last_feature_at = Instant::now() - Duration::from_secs(10);

    println!(
        "{} 开始观察 {} 秒：每秒读 Feature 0x06，同时监听 0xFF02 报文",
        stamp(start),
        seconds
    );

    while Instant::now() < deadline {
        // 输入报文：200ms 超时，一有就打印
        if let Some(frame) = input.read_input(200) {
            println!("{} [in] {}", stamp(start), hex(&frame));
        }

        // Feature：每秒对比一次，只在变化或每 15 秒时打印
        if last_feature_at.elapsed() >= Duration::from_secs(1) {
            if let Some(raw) = feat.get_feature(0x06) {
                let line = hex(&raw[..raw.len().min(12)]);
                let changed = line != last_feature;
                if changed || last_feature_at.elapsed() >= Duration::from_secs(15) {
                    println!(
                        "{} [feat]{} {}",
                        stamp(start),
                        if changed { " *" } else { "" },
                        line
                    );
                    last_feature = line;
                    last_feature_at = Instant::now();
                }
            }
        }
    }
    println!("{} 观察结束", stamp(start));
}
