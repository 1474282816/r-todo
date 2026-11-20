//! 无线设备电量：设备发现、读取、缓存、事件。
//!
//! 分三层：
//! - [`hid`]：SetupAPI 枚举 + 重叠 I/O，只负责"把字节收发下来"
//! - [`hidpp`] / [`vendor`] / [`standard`]：三条协议，各自把字节解成百分比
//! - 本模块：发现设备、挑协议、串行轮询、缓存
//!
//! **串行是刻意的**：HID 设备是独占资源，并发读会让请求和回复错配。

pub mod hid;
pub mod hidpp;
pub mod profile;
pub mod standard;
pub mod vendor;

pub use profile::{BatterySource, ChargeState, DeviceKind, DeviceProfile, VendorSpec};

use std::collections::HashMap;
use std::time::{Duration, Instant};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use profile::DeviceProfile as Profile;

/// 轮询间隔的可配范围。
pub const MIN_INTERVAL_SECS: u64 = 30;
pub const MAX_INTERVAL_SECS: u64 = 1800;
pub const DEFAULT_INTERVAL_SECS: u64 = 60;

/// 一台设备当前的读数。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceReading {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub transport: String,
    /// 百分比。`None` = 读到了设备但没读出电量（休眠/未就绪）。
    pub percent: Option<u8>,
    pub state: ChargeState,
    /// 是否在线（能打开通道并收到回复）。
    pub online: bool,
    /// 电压（毫伏），只有罗技这类报电压的才有。
    pub voltage_mv: Option<u16>,
    /// 读数来源，排查用。
    pub source: String,
    /// 原始字节，排查用。
    pub raw: String,
    /// 最后一次成功读到电量的时间（Unix 秒）。
    pub updated_at: Option<i64>,
    /// 读失败的原因。`None` = 正常。
    pub error: Option<String>,
}

impl DeviceReading {
    /// 生成占位读数。
    ///
    /// 读失败时必须用真实档案生成，否则界面会冒出"未知设备"这种没有意义的一行 ——
    /// 用户看到的应该是"G502 LIGHTSPEED 休眠中"。
    fn placeholder(profile: &Profile, error: Option<String>) -> Self {
        Self {
            id: profile.id.clone(),
            name: profile.name.clone(),
            kind: profile.kind.as_str().to_string(),
            transport: profile.transport.clone(),
            percent: None,
            state: ChargeState::Unknown,
            online: false,
            voltage_mv: None,
            source: source_label(profile.source).to_string(),
            raw: String::new(),
            updated_at: None,
            error,
        }
    }
}

fn source_label(source: BatterySource) -> &'static str {
    match source {
        BatterySource::LogitechHidpp => "HID++",
        BatterySource::VendorFeature => "厂商 Feature",
        BatterySource::StandardHid => "标准 HID",
    }
}

/// 电量服务。持有缓存和扫描状态，跨 RPC 调用共享。
pub struct BatteryService {
    cache: Mutex<HashMap<String, DeviceReading>>,
    /// 用户标定的档案（`battery.calibrate` 写入），优先级高于内置表。
    custom: Mutex<HashMap<String, Profile>>,
    /// 上次扫描时间，避免每次 RPC 都枚举一遍系统 HID。
    last_scan: Mutex<Option<(Instant, Vec<Profile>)>>,
}

impl Default for BatteryService {
    fn default() -> Self {
        Self::new()
    }
}

impl BatteryService {
    pub fn new() -> Self {
        Self {
            cache: Mutex::new(HashMap::new()),
            custom: Mutex::new(HashMap::new()),
            last_scan: Mutex::new(None),
        }
    }

    /// 从持久化的 JSON 恢复用户标定档案。
    pub async fn load_custom(&self, json: Option<String>) {
        let Some(json) = json else { return };
        let Ok(map) = serde_json::from_str::<HashMap<String, Profile>>(&json) else {
            return;
        };
        *self.custom.lock().await = map;
    }

    /// 导出用户档案，供写回持久化。
    pub async fn dump_custom(&self) -> String {
        let map = self.custom.lock().await;
        serde_json::to_string(&*map).unwrap_or_else(|_| "{}".into())
    }

    /// 列出设备（用缓存，不读硬件）。
    ///
    /// 扫描结果缓存 5 秒 —— 前端切页面时可能连发几次调用，
    /// 每次重扫会让 HID 通道繁忙，反而读不出电量。
    pub async fn list(&self, force_rescan: bool) -> Vec<DeviceReading> {
        let profiles = self.scan(force_rescan).await;
        let cache = self.cache.lock().await;
        profiles
            .iter()
            .map(|p| {
                cache
                    .get(&p.id)
                    .cloned()
                    .unwrap_or_else(|| DeviceReading::placeholder(p, None))
            })
            .collect()
    }

    async fn scan(&self, force: bool) -> Vec<Profile> {
        if !force {
            let guard = self.last_scan.lock().await;
            if let Some((at, list)) = guard.as_ref() {
                if at.elapsed() < Duration::from_secs(5) {
                    return list.clone();
                }
            }
        }

        let interfaces = hid::enumerate();
        let custom = self.custom.lock().await.clone();
        let mut found: Vec<Profile> = Vec::new();

        // 按 VID:PID 去重，同一台设备会枚举出好几个 collection
        let mut seen = std::collections::HashSet::new();
        for iface in &interfaces {
            let key = (iface.vendor_id, iface.product_id);
            if !seen.insert(key) {
                continue;
            }

            let id_vendor = format!("vendor:{:04X}:{:04X}", key.0, key.1);
            let id_logi = format!("logitech:{:04X}:{:04X}", key.0, key.1);

            // 1. 用户标定过的优先
            if let Some(p) = custom.get(&id_vendor).or_else(|| custom.get(&id_logi)) {
                found.push(p.clone());
                continue;
            }

            // 2. 内置表
            if let Some(p) = profile::find_builtin(key.0, key.1) {
                found.push(p);
                continue;
            }

            // 3. 罗技设备（接收器或鼠标本体）
            //
            // 两个坑都要避开：
            // a) 虚拟 HID 会借用罗技 VID（046D:407F "HID VHF Driver"），
            //    它也有 0xFF00 接口，只按 VID 判断会多出一台不存在的设备。
            // b) 鼠标插着 USB 线时，本体（如 046D:C08D）和接收器（046D:C539）
            //    会同时枚举出来 —— 那是同一只鼠标的两条通道，不是两台设备。
            //    这里只认接收器，本体跳过；纯有线连接（无接收器）时再回退到本体。
            if key.0 == 0x046D
                && is_real_peripheral(iface)
                && is_logitech_primary(iface, &interfaces)
                && interfaces
                    .iter()
                    .any(|i| i.vendor_id == 0x046D && i.usage_page == 0xFF00)
            {
                found.push(Profile {
                    id: id_logi,
                    vendor_id: key.0,
                    product_id: key.1,
                    name: if iface.product.is_empty() {
                        "罗技无线设备".into()
                    } else {
                        iface.product.clone()
                    },
                    kind: DeviceKind::Unknown,
                    source: BatterySource::LogitechHidpp,
                    transport: "2.4G".into(),
                    vendor: None,
                    calibrated: false,
                });
                continue;
            }

            // 4. 有 Feature 接口的，试试标准 HID 电量字段
            if is_real_peripheral(iface) && iface.feature_len > 0 && standard::read(iface).is_some() {
                found.push(Profile {
                    id: format!("hid:{:04X}:{:04X}", key.0, key.1),
                    vendor_id: key.0,
                    product_id: key.1,
                    name: if iface.product.is_empty() {
                        format!("HID 设备 {:04X}:{:04X}", key.0, key.1)
                    } else {
                        iface.product.clone()
                    },
                    kind: guess_kind(iface),
                    source: BatterySource::StandardHid,
                    transport: "HID".into(),
                    vendor: None,
                    calibrated: false,
                });
            }
        }

        *self.last_scan.lock().await = Some((Instant::now(), found.clone()));
        found
    }

    /// 刷新所有设备读数，返回最新结果。
    ///
    /// HID 操作是阻塞的（同步 Win32 调用），放进 `spawn_blocking` 避免卡住 tokio 运行时。
    pub async fn refresh(&self) -> Vec<DeviceReading> {
        let profiles = self.scan(true).await;
        let interfaces = hid::enumerate();

        let mut readings = Vec::with_capacity(profiles.len());
        for profile in profiles {
            let ifaces = interfaces.clone();
            let fallback = profile.clone();
            let reading = tokio::task::spawn_blocking(move || {
                read_blocking(&profile, &ifaces)
                    .unwrap_or_else(|err| DeviceReading::placeholder(&fallback, Some(err.to_string())))
            })
            .await
            .unwrap_or_else(|e| DeviceReading::placeholder(&fallback_for(&interfaces), Some(e.to_string())));
            readings.push(reading);
        }

        // 写缓存：读失败时保留上次成功的百分比，避免界面闪成"未知"
        let mut cache = self.cache.lock().await;
        let mut merged_list = Vec::with_capacity(readings.len());
        for reading in readings {
            let merged = match cache.get(&reading.id) {
                Some(prev) if reading.percent.is_none() && prev.percent.is_some() => {
                    let mut m = reading.clone();
                    m.percent = prev.percent;
                    m.state = prev.state;
                    m.voltage_mv = prev.voltage_mv;
                    m.updated_at = prev.updated_at;
                    m
                }
                _ => reading.clone(),
            };
            if !merged.id.is_empty() {
                cache.insert(merged.id.clone(), merged.clone());
            }
            merged_list.push(merged);
        }
        drop(cache);

        merged_list
    }

    /// 写入一条用户标定档案。
    pub async fn calibrate(&self, profile: Profile) -> Result<()> {
        self.custom.lock().await.insert(profile.id.clone(), profile);
        *self.last_scan.lock().await = None;
        Ok(())
    }

    /// 忘掉某台设备（缓存和标定里都移除）。
    pub async fn forget(&self, id: &str) {
        self.cache.lock().await.remove(id);
        self.custom.lock().await.remove(id);
        *self.last_scan.lock().await = None;
    }

    /// 导出原始 Feature 数据，供用户标定新设备时参考。
    pub async fn probe(&self) -> Vec<vendor::ProbeEntry> {
        let interfaces = hid::enumerate();
        tokio::task::spawn_blocking(move || vendor::probe(&interfaces))
            .await
            .unwrap_or_default()
    }
}

fn fallback_for(_interfaces: &[hid::HidInterface]) -> Profile {
    // spawn_blocking 崩了（线程池被销毁等极端情况）时用的兜底档案。
    // 正常路径走不到这里。
    Profile {
        id: String::new(),
        vendor_id: 0,
        product_id: 0,
        name: "读取线程异常".into(),
        kind: DeviceKind::Unknown,
        source: BatterySource::StandardHid,
        transport: String::new(),
        vendor: None,
        calibrated: false,
    }
}

fn read_blocking(profile: &Profile, interfaces: &[hid::HidInterface]) -> Result<DeviceReading> {
    match profile.source {
        BatterySource::LogitechHidpp => read_logitech(profile, interfaces),
        BatterySource::VendorFeature => read_vendor(profile, interfaces),
        BatterySource::StandardHid => read_standard(profile, interfaces),
    }
}

fn read_vendor(profile: &Profile, interfaces: &[hid::HidInterface]) -> Result<DeviceReading> {
    let spec = profile
        .vendor
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("档案缺少厂商参数"))?;
    let reading = vendor::read(interfaces, spec)?;

    Ok(DeviceReading {
        id: profile.id.clone(),
        name: profile.name.clone(),
        kind: profile.kind.as_str().to_string(),
        transport: profile.transport.clone(),
        percent: reading.percent,
        state: reading.state,
        online: true,
        voltage_mv: None,
        source: format!("Feature 0x{:02X}", spec.report_id),
        raw: hex(&reading.raw),
        updated_at: reading.percent.map(|_| now_unix()),
        error: None,
    })
}

fn read_standard(profile: &Profile, interfaces: &[hid::HidInterface]) -> Result<DeviceReading> {
    let iface = interfaces
        .iter()
        .find(|i| i.vendor_id == profile.vendor_id && i.product_id == profile.product_id)
        .ok_or_else(|| anyhow::anyhow!("设备已断开"))?;

    let percent = standard::read(iface);
    Ok(DeviceReading {
        id: profile.id.clone(),
        name: profile.name.clone(),
        kind: profile.kind.as_str().to_string(),
        transport: profile.transport.clone(),
        percent,
        state: ChargeState::Unknown,
        online: percent.is_some(),
        voltage_mv: None,
        source: "标准 HID".into(),
        raw: String::new(),
        updated_at: percent.map(|_| now_unix()),
        error: if percent.is_none() {
            Some("设备未声明标准电量字段".into())
        } else {
            None
        },
    })
}

fn read_logitech(profile: &Profile, interfaces: &[hid::HidInterface]) -> Result<DeviceReading> {
    let transport = hidpp::HidppTransport::open(interfaces)?;

    // 接收器上可能有多个索引，逐个探，取第一个能读到的。
    //
    // 整个流程有**时间预算**：鼠标深度休眠时，6 个索引全部"沉默"，
    // 每个索引做深度唤醒要 1.9 秒，6 个就是 11 秒 —— 实测能把一次刷新
    // 拖到 40 秒以上。那种情况下无论怎么试都读不到，早点放弃更合理。
    const BUDGET: std::time::Duration = std::time::Duration::from_secs(3);
    let started = std::time::Instant::now();

    let mut last_error = None;
    for index in 1..=6u8 {
        if started.elapsed() > BUDGET {
            last_error.get_or_insert_with(|| "读取超时（设备可能深度休眠）".into());
            break;
        }

        let probe = transport.probe_index(index);
        if probe == hidpp::IndexProbe::Absent {
            // 该索引没配对设备，跳过
            continue;
        }

        // 醒着就直接读；沉默的先试着叫醒
        let version = if probe == hidpp::IndexProbe::Alive {
            transport.ping(index)
        } else if started.elapsed() < BUDGET && transport.wake(index) {
            transport.ping(index)
        } else {
            None
        };

        let Some(version) = version else {
            continue;
        };

        if version < 2.0 {
            // HID++ 1.0：寄存器 0x0D 直接给百分比
            if let Some(pct) = read_hidpp10(&transport, index) {
                return Ok(build_logitech(
                    profile,
                    Some(pct),
                    None,
                    ChargeState::Unknown,
                    format!("HID++ 1.0 索引 {index}"),
                ));
            }
            continue;
        }

        match read_hidpp20(&transport, index) {
            Ok((percent, voltage, state, source)) => {
                return Ok(build_logitech(profile, percent, voltage, state, source));
            }
            Err(err) => last_error = Some(err.to_string()),
        }
    }

    Err(anyhow::anyhow!(
        last_error.unwrap_or_else(|| "接收器上没有可响应的设备（休眠中）".into())
    ))
}

fn build_logitech(
    profile: &Profile,
    percent: Option<u8>,
    voltage_mv: Option<u16>,
    state: ChargeState,
    source: String,
) -> DeviceReading {
    DeviceReading {
        id: profile.id.clone(),
        name: profile.name.clone(),
        kind: profile.kind.as_str().to_string(),
        transport: profile.transport.clone(),
        percent,
        state,
        online: true,
        voltage_mv,
        source,
        raw: String::new(),
        updated_at: percent.map(|_| now_unix()),
        error: None,
    }
}

fn read_hidpp10(transport: &hidpp::HidppTransport, index: u8) -> Option<u8> {
    // 0x0D：直接给百分比；0x07：只给 5 档
    let body = [0x81, 0x0D];
    if let Some(hidpp::HidppReply::Ok(data)) = transport.exchange(index, &body, 800) {
        if let Some(&pct) = data.first() {
            if pct <= 100 {
                return Some(pct);
            }
        }
    }
    None
}

fn read_hidpp20(
    transport: &hidpp::HidppTransport,
    index: u8,
) -> Result<(Option<u8>, Option<u16>, ChargeState, String)> {
    // 0x1004 UNIFIED_BATTERY：fn1 才是真实百分比，fn0 首字节常是别的
    if let Some(fi) = transport.feature_index(index, 0x1004) {
        let status = transport.call(index, fi, 0x00, &[], 1100);
        let level = transport.call(index, fi, 0x01, &[], 1100);

        let mut percent = level
            .as_ref()
            .and_then(|r| r.data())
            .and_then(|d| d.first().copied())
            .filter(|&p| p <= 100);

        let state = status
            .as_ref()
            .and_then(|r| r.data())
            .and_then(|d| d.get(2).copied())
            .map(map_charge_state)
            .unwrap_or(ChargeState::Unknown);

        if percent.is_none() {
            percent = status
                .as_ref()
                .and_then(|r| r.data())
                .and_then(|d| d.first().copied())
                .filter(|&p| p <= 100);
        }

        if percent.is_some() {
            return Ok((percent, None, state, "HID++ 0x1004".into()));
        }
    }

    // 0x1001 BATTERY_VOLTAGE：只有电压，按放电曲线换算
    if let Some(fi) = transport.feature_index(index, 0x1001) {
        if let Some(data) = transport
            .call(index, fi, 0x00, &[], 1100)
            .as_ref()
            .and_then(|r| r.data())
        {
            if data.len() >= 2 {
                // 大端：0E 66 = 3686mV
                let mv = u16::from_be_bytes([data[0], data[1]]);
                if (3000..=4500).contains(&mv) {
                    // 电压换算出的百分比在充电时是虚高的：
                    // 锂电池充电电压被顶到 4.2V 附近，按放电曲线算就是 100%，
                    // 但实际可能只有一半电量。所以充电态不报百分比，只报"充电中"。
                    // 判据：>= 4100mV 认为正在充电。
                    //
                    // 实测这只 G502 插线时读数在 4120~4145mV 之间浮动，
                    // 拔线后掉到 3686mV 附近。4100 这条线能分开两种状态，
                    // 又不会把刚充满静置的电池误判成充电。
                    let charging = mv >= 4100;
                    let percent = if charging {
                        None
                    } else {
                        hidpp::voltage_to_percent(mv)
                    };
                    return Ok((
                        percent,
                        Some(mv),
                        if charging {
                            ChargeState::Charging
                        } else {
                            ChargeState::Unknown
                        },
                        format!("HID++ 0x1001 电压 {mv}mV"),
                    ));
                }
            }
        }
    }

    // 0x1000 BATTERY_STATUS：老设备
    if let Some(fi) = transport.feature_index(index, 0x1000) {
        if let Some(data) = transport
            .call(index, fi, 0x00, &[], 1100)
            .as_ref()
            .and_then(|r| r.data())
        {
            if data.len() >= 3 {
                let percent = (data[0] <= 100).then_some(data[0]);
                let voltage = if data.len() >= 5 {
                    let v = u16::from_le_bytes([data[3], data[4]]);
                    (2000..=5000).contains(&v).then_some(v)
                } else {
                    None
                };
                return Ok((
                    percent,
                    voltage,
                    map_charge_state(data[2]),
                    "HID++ 0x1000".into(),
                ));
            }
        }
    }

    Err(anyhow::anyhow!("该设备没有可读的电量特性"))
}

fn map_charge_state(code: u8) -> ChargeState {
    match code {
        0 => ChargeState::Discharging,
        1 | 2 | 4 => ChargeState::Charging,
        3 => ChargeState::Full,
        _ => ChargeState::Unknown,
    }
}

/// 是不是真实外设（而不是系统虚拟 HID）。
///
/// Microsoft 的 "HID VHF Driver" 是给虚拟设备用的转发层，
/// 它也会枚举出 Feature 接口并返回可读数据，但没有电池概念。
fn is_real_peripheral(iface: &hid::HidInterface) -> bool {
    let product = iface.product.as_str();
    let manufacturer = iface.manufacturer.as_str();

    if product.contains("VHF Driver") {
        return false;
    }
    // 主板 RGB / 传感器这类板载设备：没有电池，Feature 数据是灯效配置
    if product.contains("MBoard") {
        return false;
    }
    if product.trim().is_empty() {
        return false;
    }
    if manufacturer.eq_ignore_ascii_case("Microsoft") {
        return false;
    }
    true
}

/// 罗技设备去重：同一只鼠标会同时以"接收器"和"本体"两个 PID 出现。
///
/// 判据是产品名 —— 接收器叫 "USB Receiver"，本体叫具体型号
/// （如 "G502 LIGHTSPEED Wireless Gaming Mouse"）。
/// 有接收器在时只保留接收器：走接收器通道才能同时看到其它配对的设备，
/// 而且接收器不会因为鼠标关机而消失。
fn is_logitech_primary(iface: &hid::HidInterface, all: &[hid::HidInterface]) -> bool {
    let is_receiver = iface.product.contains("Receiver");
    if is_receiver {
        return true;
    }
    // 本体：只有在没有任何接收器时才作为主设备
    !all.iter()
        .any(|i| i.vendor_id == 0x046D && i.product.contains("Receiver"))
}

fn guess_kind(iface: &hid::HidInterface) -> DeviceKind {
    match (iface.usage_page, iface.usage) {
        (0x0001, 0x0006) => DeviceKind::Keyboard,
        (0x0001, 0x0002) => DeviceKind::Mouse,
        (0x000D, _) => DeviceKind::Touchpad,
        _ => DeviceKind::Unknown,
    }
}

fn hex(data: &[u8]) -> String {
    data.iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn now_unix() -> i64 {
    chrono::Utc::now().timestamp()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interval_bounds_are_sane() {
        assert!(MIN_INTERVAL_SECS < DEFAULT_INTERVAL_SECS);
        assert!(DEFAULT_INTERVAL_SECS < MAX_INTERVAL_SECS);
    }

    #[test]
    fn hex_formats_uppercase_pairs() {
        assert_eq!(hex(&[0x06, 0xAB, 0x00]), "06 AB 00");
    }

    #[test]
    fn virtual_hid_is_rejected() {
        let mut iface = hid::HidInterface {
            path: String::new(),
            vendor_id: 0x046D,
            product_id: 0x407F,
            version: 0,
            usage_page: 0x0059,
            usage: 0x0001,
            input_len: 2,
            output_len: 0,
            feature_len: 51,
            product: "HID VHF Driver".into(),
            manufacturer: "Microsoft".into(),
            serial: String::new(),
        };
        assert!(!is_real_peripheral(&iface));

        iface.product = "G502 LIGHTSPEED".into();
        iface.manufacturer = "Logitech".into();
        assert!(is_real_peripheral(&iface));
    }
}
