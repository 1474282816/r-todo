//! 厂商 Feature Report 数据块解析（Compx 方案）。
//!
//! 这条通道的报文是 8 字节声明长度 + 尾部追加数据：
//! ```text
//! 迈从 G87:  06 00 02 64 64 64 65 5A        [3] = 电量
//! Leobog A75: 06 00 02 00 DD 00 D1 19 5F    [8] = 电量（声明长度之外）
//! ```
//!
//! **必须给足 buffer 才能读到 `[8]`** —— 按接口声明的 `FeatureReportByteLength`（8）
//! 读，`[8]` 永远是 0。这是 A75 卡了很久的根因。

use anyhow::{anyhow, Result};

use super::hid::HidDevice;
use super::profile::{ChargeState, VendorSpec};

/// 读 Feature Report 时用的 buffer 大小。
///
/// 厂商会把电量写在声明长度之外（A75 声明 8 字节，电量在第 9 字节），
/// 给到 64 字节足够覆盖所有已知型号。
const REPORT_BUFFER: usize = 64;

/// 一次厂商读取的结果。
#[derive(Debug, Clone)]
pub struct VendorReading {
    pub percent: Option<u8>,
    pub state: ChargeState,
    pub raw: Vec<u8>,
}

/// 按档案参数读一次电量。
pub fn read(interfaces: &[super::hid::HidInterface], spec: &VendorSpec) -> Result<VendorReading> {
    let iface = interfaces
        .iter()
        .find(|i| i.usage_page == spec.usage_page && i.feature_len > 0)
        .ok_or_else(|| anyhow!("找不到 usage_page=0x{:04X} 的 Feature 接口", spec.usage_page))?;

    let device = HidDevice::open(iface, false)?;
    let raw = device
        .get_feature(spec.report_id)
        .ok_or_else(|| anyhow!("读 Feature Report 0x{:02X} 失败", spec.report_id))?;

    // 接口声明长度可能小于实际数据，取实际读到的长度
    let raw = raw[..raw.len().min(REPORT_BUFFER)].to_vec();

    let percent = raw.get(spec.percent_index).copied().and_then(|b| {
        if b == 0 && spec.zero_means_unknown {
            // 0 多半表示"还没读出来"，不是真的没电 —— 报 0% 会吓人
            None
        } else {
            let scaled = (b as f32 * spec.scale).round();
            if scaled > 100.0 {
                None
            } else {
                Some(scaled as u8)
            }
        }
    });

    let state = spec
        .state_index
        .and_then(|idx| raw.get(idx))
        .and_then(|b| spec.state_map.get(&b.to_string()).copied())
        .unwrap_or(ChargeState::Unknown);

    Ok(VendorReading {
        percent,
        state,
        raw,
    })
}

/// 扫描所有 Feature 接口，把每个 RID 的数据块导出来。
///
/// 给 `battery.probe` 用：用户换了新设备时，先看原始数据长什么样，
/// 再决定电量字节在哪一位。
pub fn probe(interfaces: &[super::hid::HidInterface]) -> Vec<ProbeEntry> {
    let mut out = Vec::new();
    for iface in interfaces.iter().filter(|i| i.feature_len > 0) {
        let Ok(device) = HidDevice::open(iface, false) else {
            continue;
        };
        for report_id in 0x01..=0x10u8 {
            if let Some(raw) = device.get_feature(report_id) {
                // 全 0 的报文说明这个 RID 不存在，跳过
                if raw.iter().skip(1).all(|&b| b == 0) {
                    continue;
                }
                out.push(ProbeEntry {
                    usage_page: iface.usage_page,
                    usage: iface.usage,
                    report_id,
                    data: raw[..raw.len().min(REPORT_BUFFER)].to_vec(),
                });
            }
        }
    }
    out
}

/// `battery.probe` 的单条结果。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeEntry {
    pub usage_page: u16,
    pub usage: u16,
    pub report_id: u8,
    pub data: Vec<u8>,
}
