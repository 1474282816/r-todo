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
    /// 充电状态查询（走 Output Report 命令通道）。
    ///
    /// 有些设备的电量在 Feature Report 里，充电状态却只在命令通道的回复中，
    /// 两条路得分开读。Leobog A75 就是这种：`0xFF04` 读到电量 95%，
    /// 但 `[4]` 字节插拔电都不变，真正的充电位在 `0xFF02` 的回复帧里。
    #[serde(default)]
    pub charge_query: Option<ChargeQuery>,
}

fn one() -> f32 {
    1.0
}

/// 通过 Output Report 命令查询充电状态的参数。
///
/// 命令帧格式（Compx 方案实测）：
/// ```text
/// [0]=0x13 Report ID  [1]=cmd  [2]=arg  ...  [19]=sum(byte[0..19]) & 0xFF
/// ```
/// 校验和不对的帧会被设备**静默丢弃**，看起来就像"设备不回复"。
///
/// 回复帧的 Report ID 与请求不同（A75 请求 0x13、回复 0x0A），要分开配。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChargeQuery {
    /// 走哪个 usage page 的 Output 接口（A75 是 0xFF02）。
    pub usage_page: u16,
    /// 命令字节（A75 是 0x05）。
    pub command: u8,
    /// 参数字节（A75 是 0x00）。
    #[serde(default)]
    pub argument: u8,
    /// 回复帧的 Report ID（A75 是 0x0A）。
    pub response_id: u8,
    /// 充电状态字节在回复帧里的下标（A75 是 7）。
    pub state_index: usize,
    /// 状态字节取值 → 充电状态。
    pub state_map: std::collections::BTreeMap<String, ChargeState>,
    /// 重试次数。
    ///
    /// 实测这条通道**不是稳定可用**的：它只在某些时段响应（刚插拔线、
    /// 刚上电后的窗口期），其余时间完全沉默。所以这里的目的不是"提高成功率"，
    /// 而是**快速放弃** —— 次数和超时都压低，保证查不到时不会拖慢整体刷新。
    ///
    /// 2 次 × 400ms ≈ 1 秒封顶，在 60 秒轮询周期里可以忽略。
    #[serde(default = "default_retries")]
    pub retries: u32,
}

fn default_retries() -> u32 {
    2
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
        // 电量：0xFF04 / RID 0x06，声明长度 8 但电量在第 9 字节 [8]，稳定读 95。
        //
        // 充电：**读不到，属于尽力而为**。
        //
        // 报文里没有充电位（[4] 插拔电都是 DF，90 秒连续采样纹丝不动）。
        // 状态位只出现在 0xFF02 命令通道：发 `13 05 00` → 回
        // `13 0A 01 00 04 05 64 XX`，XX = 0x10 充电 / 0x01 放电。
        //
        // 但这个命令**不是稳定可用**的：
        //   刚插拔线后的窗口期  6/12、10/12、4/5 有回复
        //   静置一段时间后      0/8、0/12 完全沉默
        //   同期 13 07 01       始终 8/8（通道没坏，是这个命令不响应）
        //
        // 所以留着 charge_query，能读到就显示"充电中"，读不到就显示未知。
        // 不要为了它加重试次数 —— 拖慢轮询不值当。
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
                    command: 0x05,
                    argument: 0x00,
                    response_id: 0x0A,
                    state_index: 7,
                    state_map: [
                        ("1".to_string(), ChargeState::Discharging),
                        ("16".to_string(), ChargeState::Charging),
                    ]
                    .into_iter()
                    .collect(),
                    retries: 2,
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
