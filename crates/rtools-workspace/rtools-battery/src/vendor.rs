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
use super::profile::{ChargeQuery, ChargeState, VendorSpec};

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
    /// 读到了报文、但这份报文里没有可用电量时的说明（给界面做提示）。
    pub note: Option<String>,
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

    let (percent, note) = extract_percent(&raw, spec, iface.feature_len as usize);

    // 充电状态只从这份 Feature 报文里取（迈从这类设备就写在这里）。
    // A75 的充电位不在这里 —— 由上层用被动监听到的状态覆盖，见 `lib.rs`。
    let state = spec
        .state_index
        .and_then(|idx| raw.get(idx))
        .and_then(|b| spec.state_map.get(&b.to_string()).copied())
        .unwrap_or(ChargeState::Unknown);

    Ok(VendorReading {
        percent,
        state,
        raw,
        note,
    })
}

/// 按档案参数从报文里取电量，并**拒绝声明长度之外的字节**。
///
/// 为什么要有这道校验：设备用 64 字节缓冲读时会在声明长度之后多吐数据，
/// 那些字节含义未知。A75 的 `[8]` 就是这样 —— 固定 0x5F（95），插拔电、
/// 用一整天都不变，根本不是百分比。当电量显示只会误导人。
fn extract_percent(
    raw: &[u8],
    spec: &VendorSpec,
    declared_len: usize,
) -> (Option<u8>, Option<String>) {
    if spec.percent_index >= declared_len {
        return (
            None,
            Some(format!(
                "设备不上报电量：报文只声明 {declared_len} 字节，电量位 [{}] 在声明之外且恒定不变",
                spec.percent_index
            )),
        );
    }

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
    (percent, None)
}

/// 解析设备主动推送的充电状态帧。**只读，不发任何命令。**
///
/// 实测帧（A75，Compx 方案）：
/// ```text
/// 插线  13 0A 01 00 04 02 01 …                 ← 0x02 组，[6]=1 插着线
///       13 0A 01 00 04 05 64 11 …               ← 0x05 组，[7] 带 0x10 充电位
/// 拔线  13 0A 01 00 04 02 00 …                 ← 0x02 组，[6]=0
///       13 0A 01 00 04 05 64 01 …               ← 0x05 组，[7] 没有充电位
/// ```
///
/// 两组帧说的是同一件事，谁先到用谁；两组都会到，重复覆盖没有副作用。
pub fn parse_charge_event(frame: &[u8], query: &ChargeQuery) -> Option<ChargeState> {
    // 校验和先过一遍：最后一字节 = 前 19 字节之和。
    // 不验校验和会把残帧、错位帧当成状态 —— MouseDock 的 framed() 就卡这一层，
    // 我们上一版改写时把它丢了，属于回归。
    if frame.len() != 20 || !checksum_ok(frame) || frame[1] != query.response_id {
        return None;
    }
    // 下标注意：`13 0A 01 00 04 05 64 11`
    //                  ↑[4]  ↑[5] 类型  ↑[6]  ↑[7] 状态
    match frame[5] {
        // 0x02 组：第 6 字节是插线标志（0/1）
        0x02 => Some(if frame[6] & 0x01 != 0 {
            ChargeState::Charging
        } else {
            ChargeState::Discharging
        }),
        // 0x05 组：电量包，[7] 是状态位
        0x05 => {
            let byte = frame[query.state_index];
            if let Some(bit) = query.charging_bit {
                Some(if byte & bit != 0 {
                    ChargeState::Charging
                } else {
                    ChargeState::Discharging
                })
            } else {
                query.state_map.get(&byte.to_string()).copied()
            }
        }
        _ => None,
    }
}

/// 20 字节状态帧的校验和（含 Report ID 在内的前 19 字节之和）。
fn checksum_ok(frame: &[u8]) -> bool {
    if frame.len() != 20 {
        return false;
    }
    let sum = frame[..19]
        .iter()
        .fold(0u8, |a, b| a.wrapping_add(*b));
    sum == frame[19]
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

#[cfg(test)]
mod tests;

