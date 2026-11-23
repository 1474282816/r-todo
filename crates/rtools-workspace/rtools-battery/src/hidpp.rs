//! 罗技 HID++ 2.0 / 1.0。
//!
//! 报文格式（短报文 7 字节，走 `0xFF00/0x0001`）：
//! ```text
//! [0] = 0x10   Report ID
//! [1] = 设备索引（1..6，接收器上第几台配对设备）
//! [2] = 特性索引（0x00 表示 ROOT）
//! [3] = (功能号 << 4) | 软件 ID
//! [4..] = 参数
//! ```
//!
//! 回复可能是短报文（7 字节，`0xFF00/0x0001`）或长报文（20 字节，`0xFF00/0x0002`），
//! **两个接口都要监听**，只读短接口会漏掉大半回复。
//!
//! 错误回复以 `0x8F`（HID++ 2.0）或 `0xFF`（1.0）开头：
//! ```text
//! [0]=0x8F [1]=特性索引 [2]=功能字节 [3]=错误码
//! ```
//! 错误码 0x01 = 该通道按 1.0 解析（`INVALID_SUB_ID`），
//! 0x08 = 该索引没有设备（`UNKNOWN_DEVICE`）。

use anyhow::{anyhow, Result};

use super::hid::{HidDevice, HidInterface};

/// 软件 ID。避开 Logi Options+（0x0D）等已占用值。
const SW_ID: u8 = 0x0B;

const REPORT_SHORT: u8 = 0x10;

/// 一次 HID++ 调用的结果。
#[derive(Debug, Clone)]
pub enum HidppReply {
    Ok(Vec<u8>),
    Err { feature: u8, function: u8, code: u8 },
}

/// 单个接收器索引的快速探测结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexProbe {
    /// 有设备且当前醒着（能正常回复）。
    Alive,
    /// 该索引没有配对设备（设备明确回了 UNKNOWN_DEVICE），可以永久跳过。
    Absent,
    /// 有设备但没回应 —— 可能休眠，值得尝试唤醒。
    Silent,
}

impl HidppReply {
    pub fn data(&self) -> Option<&[u8]> {
        match self {
            Self::Ok(d) => Some(d),
            Self::Err { .. } => None,
        }
    }
}

pub struct HidppTransport {
    short: HidDevice,
    long: Option<HidDevice>,
}

impl HidppTransport {
    /// 从枚举结果里挑出 HID++ 接口。
    ///
    /// 短报文接口是必需的；长报文接口缺失时仍能工作（只是收不到长回复）。
    pub fn open(interfaces: &[HidInterface]) -> Result<Self> {
        // 必须限定罗技 VID：主板/其它厂商也用 0xFF00 usage page，
        // 只按 usage 找会抓到完全无关的设备（实测 Colorful MBoard 就是这个坑）
        Self::open_for(interfaces, None)
    }

    /// 按 PID 打开。
    ///
    /// 鼠标插着 USB 线时，接收器（如 C539）和本体（如 C08D）会**同时**存在，
    /// 两套都有 0xFF00 接口。不指定 PID 的话 `find` 会抓到先枚举出来的那个，
    /// 而接收器在插线状态下读不到电量（0x1001 返回"不支持"）。
    pub fn open_for(interfaces: &[HidInterface], product_id: Option<u16>) -> Result<Self> {
        let matches = |i: &&HidInterface| {
            i.vendor_id == 0x046D
                && i.usage_page == 0xFF00
                && product_id.is_none_or(|pid| i.product_id == pid)
        };

        let short = interfaces
            .iter()
            .find(|i| matches(i) && i.usage == 0x0001)
            .ok_or_else(|| anyhow!("找不到罗技 HID++ 短报文接口"))?;
        let long = interfaces
            .iter()
            .find(|i| matches(i) && i.usage == 0x0002);

        let short_dev = HidDevice::open(short, true)?;
        // 长接口打不开不影响主流程 —— 某些接收器没有这个 collection
        let long_dev = long.and_then(|i| HidDevice::open(i, true).ok());

        Ok(Self {
            short: short_dev,
            long: long_dev,
        })
    }

    /// 发一条请求并等回复。
    ///
    /// `body` 最多 5 字节：`[特性索引, (功能<<4)|SW, 参数...]`。
    pub fn exchange(&self, device_index: u8, body: &[u8], timeout_ms: u32) -> Option<HidppReply> {
        let mut msg = [0u8; 7];
        msg[0] = REPORT_SHORT;
        msg[1] = device_index;
        let n = body.len().min(5);
        msg[2..2 + n].copy_from_slice(&body[..n]);

        self.short.drain(24);
        if let Some(long) = &self.long {
            long.drain(24);
        }

        if self.short.write_output(&msg, 500).is_err() {
            return None;
        }

        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms as u64);
        while std::time::Instant::now() < deadline {
            let remaining = deadline
                .saturating_duration_since(std::time::Instant::now())
                .as_millis()
                .min(60) as u32;
            let remaining = remaining.max(1);

            // 先短后长：短回复更常见，长接口查询开销更大
            if let Some(r) = self.short.read_input(remaining) {
                if let Some(reply) = parse_reply(&r, device_index, body) {
                    return Some(reply);
                }
            }
            if let Some(long) = &self.long {
                if let Some(r) = long.read_input(1) {
                    if let Some(reply) = parse_reply(&r, device_index, body) {
                        return Some(reply);
                    }
                }
            }
        }
        None
    }

    /// 唤醒一台休眠设备。
    ///
    /// 罗技设备闲置几秒后进入休眠，此时所有请求都回 CONNECTION_REQUEST_FAILED
    /// （0x04）。官方软件的做法是连续发几轮空请求，第一轮把设备唤醒，
    /// 后续轮次才有真实回复 —— 单次 ping 失败不等于设备不存在。
    pub fn wake(&self, device_index: u8) -> bool {
        // 罗技设备休眠后第一次请求基本都会丢，要连发几轮：
        // 第一轮把射频从低功耗叫醒，后面几轮才拿得到回复。
        // 每轮之间留 120ms —— 太密会让接收器自己回 BUSY（0x07）。
        //
        // 超时用 250ms 而不是 900ms：设备醒着就会立刻回，
        // 不醒的话等再久也没用。3 轮 × 250ms 封顶约 1.1 秒。
        for round in 0..3 {
            if self
                .exchange(device_index, &[0x00, (0x01 << 4) | SW_ID, 0x00, 0x00], 250)
                .is_some()
            {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(80 + round * 40));
        }
        false
    }

    /// 快速探测接收器上有哪些索引配了设备。
    ///
    /// 只发一次、超时很短（120ms）。用途是先缩小范围，避免对 6 个索引
    /// 各做一轮深度唤醒 —— 那会拖到 30 秒以上。
    ///
    /// 未配对的索引会回 `0x08 UNKNOWN_DEVICE`（有回复但明确表示没设备），
    /// 休眠设备则完全不回。两者都判定为"当前不可用"，但前者可以永久跳过。
    pub fn probe_index(&self, device_index: u8) -> IndexProbe {
        // 发两次。
        //
        // 实测刚打开句柄后第一次请求经常丢（设备/接收器要"热身"），
        // 第二次才稳定回。只发一次会把醒着的设备误判成 Silent，
        // 进而触发不必要的深度唤醒 —— 那是十几秒的代价。
        //
        // 没设备的索引会立刻回 UNKNOWN_DEVICE（约 30ms），
        // 所以多试一次的开销可以忽略。
        let mut saw_absent = false;
        for _ in 0..2 {
            match self.exchange(device_index, &[0x00, (0x01 << 4) | SW_ID, 0x00, 0x00], 250) {
                Some(HidppReply::Err { code: 0x08, .. }) => {
                    saw_absent = true;
                    continue;
                }
                Some(_) => return IndexProbe::Alive,
                None => continue,
            }
        }
        if saw_absent {
            IndexProbe::Absent
        } else {
            IndexProbe::Silent
        }
    }

    /// 协议版本探测。返回 `Some(2.0)` / `Some(1.0)`；设备不可达返回 `None`。
    ///
    /// 接收器上未配对的索引会回 `0x08 UNKNOWN_DEVICE`，与"休眠"要区分开：
    /// 前者是稳定错误，后者重试会成功。
    pub fn ping(&self, device_index: u8) -> Option<f32> {
        for _ in 0..3 {
            // 不能写成 `exchange(...)?` —— 超时返回 None 时会直接把整个
            // 循环短路掉，后面两次重试根本没机会跑。
            let Some(reply) =
                self.exchange(device_index, &[0x00, (0x01 << 4) | SW_ID, 0x00, 0x00], 400)
            else {
                continue;
            };
            match reply {
                HidppReply::Err { code, .. } => {
                    // 0x01 = 该索引是 HID++ 1.0 设备
                    if code == 0x01 {
                        return Some(1.0);
                    }
                    // 0x08 = 没设备，不必重试
                    if code == 0x08 {
                        return None;
                    }
                    continue;
                }
                HidppReply::Ok(data) if data.len() >= 2 => {
                    return Some(data[0] as f32 + data[1] as f32 / 10.0);
                }
                _ => continue,
            }
        }
        None
    }

    /// 查特性索引。设备不支持该特性返回 `None`（索引 0 也表示不支持）。
    pub fn feature_index(&self, device_index: u8, feature_id: u16) -> Option<u8> {
        let body = [
            0x00,
            SW_ID,
            (feature_id >> 8) as u8,
            (feature_id & 0xFF) as u8,
        ];
        let reply = self.exchange(device_index, &body, 900)?;
        let data = reply.data()?;
        // 回复 data 已剥掉"特性索引 + 功能字节"的回显头，
        // 第一个字节就是特性索引本身（0 表示不支持该特性）。
        // 这里踩过坑：曾经取 data[2]，结果读到的是后续参数字节。
        data.first().copied().filter(|&idx| idx != 0)
    }

    /// 调用特性功能。
    pub fn call(
        &self,
        device_index: u8,
        feature_index: u8,
        function: u8,
        params: &[u8],
        timeout_ms: u32,
    ) -> Option<HidppReply> {
        let mut body = vec![feature_index, (function << 4) | SW_ID];
        body.extend_from_slice(&params[..params.len().min(3)]);
        self.exchange(device_index, &body, timeout_ms)
    }
}

/// 解析一条回复，只认属于本次请求的报文。
fn parse_reply(report: &[u8], device_index: u8, request: &[u8]) -> Option<HidppReply> {
    if report.len() < 4 {
        return None;
    }
    // 设备索引回显；部分固件把索引按位取反回（0xFF ^ idx）
    if report[1] != device_index && report[1] != (device_index ^ 0xFF) {
        return None;
    }
    let payload = &report[2..];

    // 错误回复：8F <feat> <func|sw> <code>
    if (payload[0] == 0x8F || payload[0] == 0xFF) && payload.len() >= 4 {
        return Some(HidppReply::Err {
            feature: payload[1],
            function: payload[2],
            code: payload[3],
        });
    }

    // 正常回复：前两字节回显 [特性索引, 功能字节]。
    //
    // **功能字节的低 4 位在回复里是 0，不是请求里带的 SW_ID** ——
    // 直接比 payload[1] == request[1] 会永远不匹配，表现为"设备无回复"。
    // 只比高 4 位（功能号）。
    if payload.len() >= 2
        && payload[0] == request[0]
        && (payload[1] & 0xF0) == (request[1] & 0xF0)
    {
        return Some(HidppReply::Ok(payload[2..].to_vec()));
    }

    None
}

/// 把电压换算成电量百分比。
///
/// 0x1001 只给毫伏，百分比是**估算**，所以节点表从哪来必须写清楚。
///
/// 这里的节点取自 Solaar（`logitech/hidpp20.py` 的放电节点表），是社区长期实测的
/// 结果。之前那条曲线是自己拍的（4100→100、4000→85、3900→66…），中低段明显偏乐观：
///
/// | 电压 | 旧曲线 | Solaar | 差 |
/// |---|---|---|---|
/// | 4060 mV | 94% | 89% | 5 |
/// | 4000 mV | 85% | 81% | 4 |
/// | 3700 mV | 28% | 17% | 11 |
/// | 3600 mV | 14% | 3%  | 11 |
///
/// 数字越往下差得越多，因为旧曲线把"还能用多久"想得太乐观。
pub fn voltage_to_percent(mv: u16) -> Option<u8> {
    // (毫伏, 百分比)，从空到满。
    const CURVE: [(u16, u8); 13] = [
        (3500, 0),
        (3579, 2),
        (3646, 5),
        (3671, 10),
        (3717, 20),
        (3751, 30),
        (3778, 40),
        (3811, 50),
        (3859, 60),
        (3922, 70),
        (3989, 80),
        (4067, 90),
        (4186, 100),
    ];

    if mv <= CURVE[0].0 {
        return Some(0);
    }
    if mv >= CURVE[CURVE.len() - 1].0 {
        return Some(100);
    }

    for w in CURVE.windows(2) {
        let (lo_mv, lo_pct) = w[0];
        let (hi_mv, hi_pct) = w[1];
        if mv >= lo_mv && mv <= hi_mv {
            let ratio = (mv - lo_mv) as f32 / (hi_mv - lo_mv) as f32;
            let span = hi_pct as f32 - lo_pct as f32;
            return Some((lo_pct as f32 + span * ratio).round() as u8);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voltage_curve_is_monotonic() {
        let mut last = 0u8;
        for mv in (3400..=4300).step_by(25) {
            let pct = voltage_to_percent(mv).unwrap();
            assert!(pct >= last, "{mv}mV 得到 {pct}% 比前一档 {last}% 还低");
            last = pct;
        }
    }

    #[test]
    fn voltage_endpoints() {
        assert_eq!(voltage_to_percent(4200), Some(100));
        assert_eq!(voltage_to_percent(4500), Some(100));
        assert_eq!(voltage_to_percent(3400), Some(0));
        assert_eq!(voltage_to_percent(3000), Some(0));
    }

    /// 曲线必须和 Solaar 的节点表一致。
    /// 旧曲线在 4060mV 报 94%，Solaar 是 89% —— 这正是和 MouseDock 对不上的原因。
    #[test]
    fn voltage_matches_solaar_knots() {
        assert_eq!(voltage_to_percent(4067), Some(90));
        assert_eq!(voltage_to_percent(4060), Some(89));
        assert_eq!(voltage_to_percent(3989), Some(80));
        assert_eq!(voltage_to_percent(3600), Some(3));
    }
}
