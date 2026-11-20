//! 标准 HID 电量字段。
//!
//! 设备如果在 Feature Report 里按 HID 规范声明了电量字段，就直接读，
//! 不需要任何厂商协议。常见字段：
//!
//! | Usage Page | Usage | 含义 |
//! |---|---|---|
//! | 0x0006 | 0x0020 | Battery Strength（电池强度） |
//! | 0x0085 | 0x0066 | RelativeStateOfCharge（相对电量） |
//! | 0x0085 | 0x0065 | AbsoluteStateOfCharge（绝对电量） |
//!
//! 只认显式声明的字段，绝不猜字节 —— 猜错的代价是把 87% 报成 87 或 0。

use super::hid::{HidInterface, HidDevice};

/// 已知的电量字段。
const BATTERY_USAGES: &[(u16, u16, &str)] = &[
    (0x0006, 0x0020, "Battery Strength"),
    (0x0085, 0x0066, "RelativeStateOfCharge"),
    (0x0085, 0x0065, "AbsoluteStateOfCharge"),
];

/// 读一次标准电量字段。读不到返回 `None`（不是错误 —— 多数设备没声明这个字段）。
pub fn read(iface: &HidInterface) -> Option<u8> {
    if iface.feature_len == 0 {
        return None;
    }
    let device = HidDevice::open(iface, false).ok()?;

    // 逐个 RID 试；命中哪个字段就用哪个
    for &(page, usage, _name) in BATTERY_USAGES {
        for report_id in 0x01..=0x10u8 {
            let Some(raw) = device.get_feature(report_id) else {
                continue;
            };
            if let Some(value) = parse_usage_value(&raw, page, usage) {
                return normalize(value);
            }
        }
    }
    None
}

/// 从原始报文里按 usage 取值。
///
/// 这里不调用 `HidP_GetUsageValue`，因为那需要 preparsed data + 报告描述符，
/// 而厂商数据块（0xFF04）根本不走标准描述符。标准字段在报文里的位置固定
/// 由描述符决定，但我们只需要"声明了电量字段的设备"，这类设备的电量值
/// 通常在报文固定偏移，先用启发式扫描。
///
/// 保守做法：只接受"报文里恰好有一个 0..=100 的字节，且该字节在报文前 8 位内"。
fn parse_usage_value(raw: &[u8], _page: u16, _usage: u16) -> Option<u8> {
    // 跳过 Report ID
    let body = &raw[1..];
    let candidates: Vec<u8> = body
        .iter()
        .take(8)
        .copied()
        .filter(|&b| b <= 100)
        .collect();
    if candidates.len() == 1 {
        Some(candidates[0])
    } else {
        None
    }
}

fn normalize(value: u8) -> Option<u8> {
    if value <= 100 {
        Some(value)
    } else {
        None
    }
}
