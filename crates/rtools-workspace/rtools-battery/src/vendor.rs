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

use super::hid::{HidDevice, HidInterface};
use super::profile::{ChargeQuery, ChargeState, VendorSpec};

/// 读 Feature Report 时用的 buffer 大小。
///
/// 厂商会把电量写在声明长度之外（A75 声明 8 字节，电量在第 9 字节），
/// 给到 64 字节足够覆盖所有已知型号。
const REPORT_BUFFER: usize = 64;

/// 命令通道一帧的长度。
///
/// Compx 方案的 Output Report 固定 20 字节，最后一字节是校验和。
const COMMAND_FRAME_LEN: usize = 20;

/// 校验和字节的下标（= 帧长 - 1）。
const CHECKSUM_INDEX: usize = COMMAND_FRAME_LEN - 1;

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

    // 充电状态优先从命令通道查；查不到再退回报文里的状态字节。
    // 顺序不能反：A75 的报文里没有充电位，退回逻辑只会得到 Unknown。
    let state = spec
        .charge_query
        .as_ref()
        .and_then(|q| read_charge_state(interfaces, q))
        .or_else(|| {
            spec.state_index
                .and_then(|idx| raw.get(idx))
                .and_then(|b| spec.state_map.get(&b.to_string()).copied())
        })
        .unwrap_or(ChargeState::Unknown);

    Ok(VendorReading {
        percent,
        state,
        raw,
    })
}

/// 走命令通道查一次充电状态。
///
/// 返回 `None` 表示这条通道没应答（不代表"未充电"）—— 上层会退回报文里的状态字节。
///
/// # 这条通道不可靠，别指望它
///
/// 实测数据（Leobog A75，`13 05 00`）：
/// - 刚插拔线后的窗口期：6/12、10/12、4/5 能收到回复
/// - 静置一段时间后：**0/8、0/12 完全沉默**
/// - 同期对照 `13 07 01` 始终 8/8 —— 通道本身没坏，是这个命令不响应
///
/// 也就是说它吐出的状态位（`0x10` 充电 / `0x01` 放电）本身是对的，
/// 但只在特定时机才给。所以这里**只做一两次快速尝试**，拿不到就当没有，
/// 绝不为了它拖慢轮询。
pub fn read_charge_state(interfaces: &[HidInterface], query: &ChargeQuery) -> Option<ChargeState> {
    let iface = interfaces
        .iter()
        .find(|i| i.usage_page == query.usage_page && i.output_len > 0)?;

    let device = HidDevice::open(iface, true).ok()?;

    for _ in 0..query.retries {
        // 清掉上一条命令的迟到回复，否则可能读到旧状态
        device.drain(32);

        let frame = build_command_frame(query.command, query.argument);
        if device.write_output(&frame, 400).is_err() {
            continue;
        }

        // 回复要么很快到，要么根本不来 —— 等太久纯属浪费
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(400);
        while std::time::Instant::now() < deadline {
            let Some(reply) = device.read_input(60) else {
                continue;
            };
            if reply.len() <= query.state_index || reply[0] != query.response_id {
                continue;
            }
            // 校验和不对的回复直接丢（可能是残帧）
            if !checksum_ok(&reply) {
                continue;
            }
            let byte = reply[query.state_index];
            if let Some(state) = query.state_map.get(&byte.to_string()).copied() {
                return Some(state);
            }
        }
    }
    None
}

/// 拼一条命令帧：`[0]=0x13 [1]=cmd [2]=arg ... [19]=checksum`。
fn build_command_frame(command: u8, argument: u8) -> [u8; COMMAND_FRAME_LEN] {
    let mut frame = [0u8; COMMAND_FRAME_LEN];
    frame[0] = 0x13;
    frame[1] = command;
    frame[2] = argument;
    frame[CHECKSUM_INDEX] = frame[..CHECKSUM_INDEX]
        .iter()
        .fold(0u8, |a, b| a.wrapping_add(*b));
    frame
}

/// 校验回复帧的最后一字节是否等于前面所有字节之和。
fn checksum_ok(frame: &[u8]) -> bool {
    if frame.len() < COMMAND_FRAME_LEN {
        return false;
    }
    let sum = frame[..CHECKSUM_INDEX]
        .iter()
        .fold(0u8, |a, b| a.wrapping_add(*b));
    sum == frame[CHECKSUM_INDEX]
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
mod tests {
    use super::*;

    #[test]
    fn command_frame_has_correct_checksum() {
        // 实测帧：13 05 00 ... 校验和 0x18
        let frame = build_command_frame(0x05, 0x00);
        assert_eq!(frame[0], 0x13);
        assert_eq!(frame[1], 0x05);
        assert_eq!(frame[2], 0x00);
        assert_eq!(frame[CHECKSUM_INDEX], 0x13 + 0x05);
        assert!(checksum_ok(&frame));
    }

    #[test]
    fn checksum_rejects_bad_frame() {
        let mut frame = build_command_frame(0x05, 0x00);
        frame[CHECKSUM_INDEX] ^= 0xFF;
        assert!(!checksum_ok(&frame));
    }

    #[test]
    fn checksum_rejects_short_frame() {
        // 7 字节的短报文不是命令通道的格式
        assert!(!checksum_ok(&[0x13, 0x05, 0x00, 0, 0, 0, 0]));
    }

    #[test]
    fn real_charging_reply_is_recognized() {
        // 插线实测帧：[7]=0x10。
        // 校验和 = 0x13+0x0A+0x01+0x00+0x04+0x05+0x64+0x10 = 0x9B
        let reply = [
            0x13, 0x0A, 0x01, 0x00, 0x04, 0x05, 0x64, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x9B,
        ];
        assert!(checksum_ok(&reply));
        assert_eq!(reply[7], 0x10);
    }

    #[test]
    fn real_discharging_reply_is_recognized() {
        // 拔线实测帧：[7]=0x01。
        // 校验和 = 0x13+0x0A+0x01+0x00+0x04+0x05+0x64+0x01 = 0x8C
        let reply = [
            0x13, 0x0A, 0x01, 0x00, 0x04, 0x05, 0x64, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x8C,
        ];
        assert!(checksum_ok(&reply));
        assert_eq!(reply[7], 0x01);
    }

    #[test]
    fn real_known_good_frame_from_device() {
        // 从设备抓到的真实帧（13 07 01 的回复），校验和由设备自己算出来
        let reply = [
            0x13, 0x07, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x1C,
        ];
        assert!(checksum_ok(&reply));
    }
}
