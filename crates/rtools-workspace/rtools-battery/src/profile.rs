//! 设备档案：谁、什么类型、走哪条协议、电量字节在哪。
//!
//! 内置表按 `VID:PID` 匹配；表里没有的设备走"自动探测"，
//! 探测结果可以固化成一条用户档案存进 settings 表。
//!
//! 支持以后新增设备的两条路：
//! 1. 往 [`BUILTIN`] 里加一行（新固件/同款换 PID）
//! 2. 运行时 `battery.calibrate` 写入用户档案（不重新编译）

use serde::{Deserialize, Serialize};

/// 设备类型，只影响图标和文案。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceKind {
    Keyboard,
    Mouse,
    Headset,
    Touchpad,
    Gamepad,
    Unknown,
}

impl DeviceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Keyboard => "keyboard",
            Self::Mouse => "mouse",
            Self::Headset => "headset",
            Self::Touchpad => "touchpad",
            Self::Gamepad => "gamepad",
            Self::Unknown => "unknown",
        }
    }
}

/// 电量读取方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatterySource {
    /// 罗技 HID++ 2.0：`BATTERY_VOLTAGE`(0x1001) / `UNIFIED_BATTERY`(0x1004) /
    /// `BATTERY_STATUS`(0x1000)，以及 HID++ 1.0 寄存器 0x07 / 0x0D。
    LogitechHidpp,
    /// 厂商 Feature Report 数据块（Compx 方案：迈从 G87、Leobog A75 等）。
    VendorFeature,
    /// 标准 HID 电量字段（Usage Page 0x0006/0x0020 等）。
    StandardHid,
}

/// 充电状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChargeState {
    Discharging,
    Charging,
    Full,
    Unknown,
}

/// 一条设备档案。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceProfile {
    /// 稳定 id，格式 `<协议>:<VID>:<PID>`，同一设备跨会话不变。
    pub id: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub name: String,
    pub kind: DeviceKind,
    pub source: BatterySource,
    /// 展示用的连接方式（2.4G / USB / 蓝牙）。
    pub transport: String,
    #[serde(default)]
    pub vendor: Option<VendorSpec>,
    /// 用户手动标定过（`battery.calibrate` 写入），内置表命中时为 false。
    #[serde(default)]
    pub calibrated: bool,
}

/// 厂商 Feature Report 数据块的解析参数。
///
/// 全部外置，改设备不用改代码 —— 这是"支持以后新增设备"的核心。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VendorSpec {
    /// 走哪个 usage page 的接口（如 0xFF04）。
    pub usage_page: u16,
    /// 读哪个 Feature Report ID（如 0x06）。
    pub report_id: u8,
    /// 电量字节在报文里的下标（A75 是 8，迈从 G87 是 3）。
    pub percent_index: usize,
    /// 充电状态字节下标；`None` 表示不解析。
    #[serde(default)]
    pub state_index: Option<usize>,
    /// 状态字节取值 → 充电状态的映射，形如 `{"0":"discharging","1":"charging","2":"full"}`。
    #[serde(default)]
    pub state_map: std::collections::BTreeMap<String, ChargeState>,
    /// 电量值的倍率（原始值 × scale = 百分比）。默认 1.0。
    #[serde(default = "one")]
    pub scale: f32,
    /// 读到 0 时是否视为无效（多数设备 0 表示"未就绪"而不是真的没电）。
    #[serde(default)]
    pub zero_means_unknown: bool,
    /// 充电状态报文解析参数（走 0xFF02 的输入报文，只读）。
    ///
    /// 有些设备的电量在 Feature Report 里，充电状态却只在 0xFF02 的报文里，
    /// 两条路得分开读。Leobog A75 就是这种：`0xFF04` 读到电量 95%，
    /// 但 `[4]` 字节插拔电都不变，真正的充电位在 `0xFF02` 的推送帧里。
    #[serde(default)]
    pub charge_query: Option<ChargeQuery>,
}

fn one() -> f32 {
    1.0
}

/// 充电状态报文的解析参数（Compx 方案）。
///
/// 这些设备**不会稳定应答查询** —— 插拔线那一刻自己往 0xFF02 推一帧就完事，
/// 静置后怎么问都沉默（实测 0/8、0/12）。所以这里只描述"怎么读懂它推的帧"，
/// 不含任何写命令参数：往厂商通道盲写曾经把键盘 MCU 写死过。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChargeQuery {
    /// 从哪个 usage page 的输入接口收报文（A75 是 0xFF02）。
    pub usage_page: u16,
    /// 状态帧的第 1 字节（A75 是 0x0A）。
    pub response_id: u8,
    /// 充电状态字节在帧里的下标（A75 是 7）。
    pub state_index: usize,
    /// 状态字节的**充电位**：与这个掩码相与非 0 就是充电中。
    ///
    /// A75 实测：`0x11` 充电、`0x01` 放电。两个值都带 `0x01` 位，
    /// 所以不能按"取值对表"匹配 —— 表里写 1 和 16，`0x11`(=17) 会漏成未知，
    /// 界面上就永远不显示"充电中"。必须按位判断。
    #[serde(default)]
    pub charging_bit: Option<u8>,
    /// 没有 `charging_bit` 的老式设备用取值表。
    ///
    /// A75 实测只认两个值：`0x01` 放电、`0x11` 充电；其它值（例如偶发抓到的
    /// `0x10`）一律当未知，绝不按位硬猜成"充电中"。
    #[serde(default)]
    pub state_map: std::collections::BTreeMap<String, ChargeState>,
    /// 可选：周期性发一条查询帧刷新状态。
    ///
    /// 设备平时不发帧，只在插拔线那一刻推一次；错过就再也问不到。
    /// A75 实测 `13 05 01 00 06`（MouseDock 当心跳用的身份帧）能稳定拿到回复，
    /// 回复里的 `13 0A … [7]` 就是状态位。靠它兜住"启动时已经插着"和"漏帧"两种情况。
    #[serde(default)]
    pub poll: Option<ChargePoll>,
}

/// 周期性状态查询的参数。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChargePoll {
    /// 帧内容（不含校验和），写成一串十六进制，例如 `"13 05 01 00 06"`。
    pub payload_hex: String,
    /// 查询周期（秒）。
    #[serde(default = "default_poll_interval")]
    pub interval_secs: u64,
}

fn default_poll_interval() -> u64 {
    60
}

/// 内置设备表。
///
/// 加新设备就往这里加一行 —— 参数全部来自实测，不要凭型号猜。
pub fn builtin() -> Vec<DeviceProfile> {
    vec![
        // ── 罗技 G502 LIGHTSPEED（USB 接收器 C539，设备索引 1）──
        // 实测：0x1001 BATTERY_VOLTAGE 返回 `0E 68`，按大端解析 3684mV（锂电池满电 4.2V）。
        // 该机型不支持 0x1000/0x1004，只能走电压 + 放电曲线换算。
        DeviceProfile {
            // ID 按产品名生成，不绑 PID —— 插拔线时会在 C539/C08D 之间切换，
            // 绑 PID 会导致同一只鼠标在界面上变成两台设备。
            id: "logitech:G502-LIGHTSPEED".into(),
            vendor_id: 0x046D,
            product_id: 0xC539,
            name: "G502 LIGHTSPEED".into(),
            kind: DeviceKind::Mouse,
            source: BatterySource::LogitechHidpp,
            transport: "2.4G".into(),
            vendor: None,
            calibrated: false,
        },
        // ── Leobog A75（厂商 "CX" = Compx 方案，与迈从同芯片）──
        //
        // **这台接收器不上报实时电量。**
        //
        // 描述符里 0xFF04/RID 0x06 只声明了 8 字节（一个 8 位 vendor 值，恒为 0）。
        // 用 64 字节缓冲读时设备会多吐一个字节 [8] = 0x5F(95)，但：
        //   - 插着线 95、拔了线 95、放一整天还是 95；
        //   - 同一份报文里 [4]/[6]/[7] 也全程不变。
        // 所以它不是电量，只是设备在声明长度之外多吐的固定数据。
        // 现在 `vendor::extract_percent` 会按声明长度拒绝它，界面显示"—"，
        // 而不是一直挂着 95%。
        //
        // 充电：靠被动监听，不写命令。
        //
        // Feature 报文里没有充电位（[4] 插拔电都是 DF，90 秒采样纹丝不动）。
        // 插拔线那一刻设备会主动往 0xFF02 推帧（三轮抓包实测）：
        //   插线  13 0A 01 00 04 02 01 …   ← 0x02 组，[6]=1 插着线
        //         13 0A 01 00 04 05 64 11 … ← 0x05 组，[7]=0x11（带 0x10 充电位）
        //   拔线  13 0A 01 00 04 02 00 …   ← [6]=0
        //         13 0A 01 00 04 05 64 01 … ← [7]=0x01（没有充电位）
        //
        // 之前记的"查询成功率不稳"是把设备主动推的帧当成了查询回复。
        // 现在改成常驻被动监听（`lib.rs` 的 charge_listener_loop），一个字节都不写。
        DeviceProfile {
            id: "vendor:3554:FA09".into(),
            vendor_id: 0x3554,
            product_id: 0xFA09,
            name: "Leobog A75".into(),
            kind: DeviceKind::Keyboard,
            source: BatterySource::VendorFeature,
            transport: "2.4G".into(),
            vendor: Some(VendorSpec {
                usage_page: 0xFF04,
                report_id: 0x06,
                percent_index: 8,
                // 这份报文里没有充电位，留空
                state_index: None,
                state_map: Default::default(),
                scale: 1.0,
                zero_means_unknown: true,
                charge_query: Some(ChargeQuery {
                    usage_page: 0xFF02,
                    response_id: 0x0A,
                    state_index: 7,
                    // 只认这两个实测值，其它一律未知（0x10 是偶发抖动，不能当充电）
                    charging_bit: None,
                    state_map: [
                        ("1".to_string(), ChargeState::Discharging),
                        ("17".to_string(), ChargeState::Charging),
                    ]
                    .into_iter()
                    .collect(),
                    poll: Some(ChargePoll {
                        payload_hex: "13 05 01 00 06".into(),
                        interval_secs: 60,
                    }),
                }),
            }),
            calibrated: false,
        },
        // ── 迈从 G87（参考项目已验证，同 Compx 方案但字节位置不同）──
        DeviceProfile {
            id: "vendor:41E4:2001".into(),
            vendor_id: 0x41E4,
            product_id: 0x2001,
            name: "迈从 G87".into(),
            kind: DeviceKind::Keyboard,
            source: BatterySource::VendorFeature,
            transport: "2.4G".into(),
            vendor: Some(VendorSpec {
                usage_page: 0xFF04,
                report_id: 0x06,
                percent_index: 3,
                state_index: Some(2),
                state_map: [
                    ("0".to_string(), ChargeState::Discharging),
                    ("1".to_string(), ChargeState::Charging),
                    ("2".to_string(), ChargeState::Full),
                ]
                .into_iter()
                .collect(),
                scale: 1.0,
                zero_means_unknown: true,
                charge_query: None,
            }),
            calibrated: false,
        },
    ]
}

/// 按 `VID:PID` 查内置档案。
pub fn find_builtin(vendor_id: u16, product_id: u16) -> Option<DeviceProfile> {
    builtin()
        .into_iter()
        .find(|p| p.vendor_id == vendor_id && p.product_id == product_id)
}

/// 按 VID + 产品名查内置档案。
///
/// 为什么需要这个：同一只罗技鼠标插拔 USB 线时会在两个 PID 之间切换
/// （G502：无线 C539 / 有线 C08D），但它们的产品名是一样的。
/// 只按 PID 查会让同一台设备在两种状态下落到不同档案，界面上变成两台。
pub fn find_builtin_by_name(vendor_id: u16, product: &str) -> Option<DeviceProfile> {
    if product.is_empty() {
        return None;
    }
    builtin().into_iter().find(|p| {
        p.vendor_id == vendor_id
            && !p.name.is_empty()
            && product.to_ascii_lowercase().contains(&p.name.to_ascii_lowercase())
    })
}
