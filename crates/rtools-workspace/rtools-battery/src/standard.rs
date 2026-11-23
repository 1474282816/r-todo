//! 标准 HID 电量字段：按报告描述符精确匹配，不再按字节猜。
//!
//! **旧实现是错的**：拿 Feature 报文的前 8 个字节，只要"恰好有一个 ≤100 的数"
//! 就当电量报出去。任何设备的灯效/配置报文都可能命中，于是列表里会冒出
//! 不存在的"设备"，显示的还是编造出来的百分比。
//!
//! 现在的流程全部交给 Windows 的 HID 解析器，猜的部分一步都没有：
//! 1. `HidD_GetPreparsedData` + `HidP_GetCaps` 拿这份描述符的能力；
//! 2. `HidP_GetValueCaps` 找出**真正声明过**电量 usage 的 Feature 值；
//! 3. `HidP_GetUsageValue` 让解析器按描述符把值取出来。
//! 三步任何一步失败就返回 `None` —— 宁可显示"读不到"，也不编数字。
//!
//! 认的 usage：
//! | 0x0006 / 0x0020 | Battery Strength |
//! | 0x0085 / 0x0066 | RelativeStateOfCharge |
//! | 0x0085 / 0x0065 | AbsoluteStateOfCharge |

use windows::Win32::Devices::HumanInterfaceDevice::{
    HidD_FreePreparsedData, HidD_GetPreparsedData, HidP_GetCaps, HidP_GetUsageValue,
    HidP_GetValueCaps, HidP_Feature, HIDP_CAPS, HIDP_STATUS_SUCCESS, HIDP_VALUE_CAPS,
    PHIDP_PREPARSED_DATA,
};

use super::hid::{HidDevice, HidInterface};

/// 声明过的电量 usage（page, usage）。
const BATTERY_USAGES: &[(u16, u16)] = &[(0x0006, 0x0020), (0x0085, 0x0066), (0x0085, 0x0065)];

/// 解析器句柄的释放器。
struct Preparsed(PHIDP_PREPARSED_DATA);

impl Drop for Preparsed {
    fn drop(&mut self) {
        unsafe {
            let _ = HidD_FreePreparsedData(self.0);
        }
    }
}

/// 读一次标准电量字段。读不到返回 `None`（不是错误 —— 多数设备压根没声明）。
pub fn read(iface: &HidInterface) -> Option<u8> {
    if iface.feature_len == 0 {
        return None;
    }

    let device = HidDevice::open(iface, false).ok()?;

    let mut preparsed = PHIDP_PREPARSED_DATA(0);
    if !unsafe { HidD_GetPreparsedData(device.raw_handle(), &mut preparsed) } {
        return None;
    }
    let _preparsed = Preparsed(preparsed);

    let mut caps = HIDP_CAPS::default();
    if unsafe { HidP_GetCaps(preparsed, &mut caps) }.0 != HIDP_STATUS_SUCCESS.0 {
        return None;
    }
    let count = caps.NumberFeatureValueCaps as usize;
    if count == 0 {
        return None;
    }

    let mut vcaps = vec![HIDP_VALUE_CAPS::default(); count];
    let mut len = count as u16;
    if unsafe { HidP_GetValueCaps(HidP_Feature, vcaps.as_mut_ptr(), &mut len, preparsed) }.0
        != HIDP_STATUS_SUCCESS.0
    {
        return None;
    }

    for cap in vcaps.iter().take(len as usize) {
        let Some(usage) = declared_battery_usage(cap) else {
            continue;
        };
        let Some(raw) = device.get_feature(cap.ReportID) else {
            continue;
        };

        // 两种 buffer 约定都试一次（含 / 不含 Report ID）。文档对这一点有歧义，
        // 但只有 HID 解析器认可的布局才会返回 HIDP_STATUS_SUCCESS ——
        // 让解析器自己判断，比我们数偏移可靠。
        let mut buffers: Vec<&[u8]> = Vec::new();
        if cap.ReportID == 0 {
            buffers.push(&raw[..]);
        } else {
            buffers.push(&raw[..]);
            if raw.len() > 1 {
                buffers.push(&raw[1..]);
            }
        }

        for buf in buffers {
            let mut value = 0u32;
            let status = unsafe {
                HidP_GetUsageValue(
                    HidP_Feature,
                    cap.UsagePage,
                    Some(cap.LinkCollection),
                    usage,
                    &mut value,
                    preparsed,
                    buf,
                )
            };
            if status.0 != HIDP_STATUS_SUCCESS.0 {
                continue;
            }
            if let Some(percent) = normalize(value, cap.LogicalMin, cap.LogicalMax) {
                return Some(percent);
            }
        }
    }

    None
}

/// 这个值能力声明的是不是电量 usage。
fn declared_battery_usage(cap: &HIDP_VALUE_CAPS) -> Option<u16> {
    let wanted = BATTERY_USAGES
        .iter()
        .filter(|(page, _)| *page == cap.UsagePage)
        .map(|(_, usage)| *usage)
        .collect::<Vec<_>>();
    if wanted.is_empty() {
        return None;
    }

    if cap.IsRange {
        let range = unsafe { cap.Anonymous.Range };
        return wanted
            .into_iter()
            .find(|&u| u >= range.UsageMin && u <= range.UsageMax);
    }

    let usage = unsafe { cap.Anonymous.NotRange.Usage };
    wanted.contains(&usage).then_some(usage)
}

/// 原始值按逻辑范围归一化到 0–100。
fn normalize(value: u32, logical_min: i32, logical_max: i32) -> Option<u8> {
    if (value as i64) < logical_min as i64 {
        return None;
    }
    let max = if logical_max <= 0 { 100 } else { logical_max };
    let percent = if max <= 100 {
        value as i64
    } else {
        ((value as f64) * 100.0 / (max as f64)).round() as i64
    };
    (0..=100).contains(&percent).then_some(percent as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_handles_100_scale() {
        assert_eq!(normalize(95, 0, 100), Some(95));
        assert_eq!(normalize(101, 0, 100), None);
    }

    #[test]
    fn normalize_scales_255() {
        assert_eq!(normalize(255, 0, 255), Some(100));
        assert_eq!(normalize(128, 0, 255), Some(50));
    }

    #[test]
    fn normalize_ignores_below_logical_min() {
        assert_eq!(normalize(0, 1, 100), None);
    }
}
