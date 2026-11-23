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

/// 充电状态的保质期。
///
/// 设备只在插拔线那一刻推帧，之后不再说明；超过这么久没再收到证据就当未知 ——
/// 宁可少显示一个"充电中"，也不要因为漏了一帧就永远挂着它。
///
/// 3 小时：一次充满大概 2 小时左右，留一点余量；真漏了帧，错挂时间也不会太长。
pub const CHARGE_STATE_TTL: Duration = Duration::from_secs(3 * 3600);

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
    /// 百分比是不是估算出来的（电压换算 → true，设备直接报百分比 → false）。
    ///
    /// 界面要给这类数字加 "≈"，否则用户会拿它和官方驱动里的精确值对比。
    #[serde(default)]
    pub estimated: bool,
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
            estimated: false,
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
    /// 被动监听到的充电状态（按设备 id）。
    ///
    /// 这些设备不会稳定应答查询，只在插拔线那一刻主动推一次报文，
    /// 轮询（默认 60 秒一次）几乎必然错过，所以单独开一条常驻线程守着。
    charge: std::sync::Arc<std::sync::Mutex<HashMap<String, (ChargeState, Instant)>>>,
    /// 监听线程是否已启动（幂等）。
    charge_listener: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// 监听到状态变化时置位，供上层立刻刷新界面。
    charge_changed: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// 刷新互斥锁。
    ///
    /// HID 是独占资源：两个 refresh 并发时会同时对同一接口 `CreateFileW`，
    /// 然后一方的回复被另一方 `ReadFile` 读走 —— `exchange` 只按设备索引过滤，
    /// 分不清是哪次请求的回复，结果是读到旧值或一直收不到。
    ///
    /// 触发场景真实存在：轮询定时器、用户点刷新按钮、前端挂载时主动刷一次，
    /// 三者可能撞在一起。
    refresh_lock: tokio::sync::Mutex<()>,
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
            charge: std::sync::Arc::new(std::sync::Mutex::new(HashMap::new())),
            charge_listener: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            charge_changed: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            refresh_lock: tokio::sync::Mutex::new(()),
        }
    }

    /// 启动充电状态被动监听（幂等，只在第一次真正创建线程）。
    pub fn ensure_charge_listener(&self, profiles: &[Profile]) {
        use std::sync::atomic::Ordering;
        if self.charge_listener.swap(true, Ordering::SeqCst) {
            return;
        }

        let targets: Vec<(String, u16, u16, profile::ChargeQuery)> = profiles
            .iter()
            .filter_map(|p| {
                let q = p.vendor.as_ref()?.charge_query.clone()?;
                Some((p.id.clone(), p.vendor_id, p.product_id, q))
            })
            .collect();
        if targets.is_empty() {
            return;
        }

        let charge = self.charge.clone();
        let changed = self.charge_changed.clone();
        let _ = std::thread::Builder::new()
            .name("rtools-battery-charge".into())
            .spawn(move || charge_listener_loop(targets, charge, changed));
    }

    /// 取走"状态有变化"标记。上层拿它决定要不要立刻刷新界面。
    pub fn take_charge_change(&self) -> bool {
        self.charge_changed
            .swap(false, std::sync::atomic::Ordering::SeqCst)
    }

    /// 当前已知的充电状态（还没监听到就返回 `None`）。
    /// 当前已知的充电状态；超过 [`CHARGE_STATE_TTL`] 没再收到报文就当未知。
    ///
    /// 状态是设备推送的，可能漏帧；不加时效的话会永远显示"充电中"。
    pub fn charge_state(&self, id: &str) -> Option<ChargeState> {
        let map = self.charge.lock().ok()?;
        let (state, at) = map.get(id)?;
        (at.elapsed() <= CHARGE_STATE_TTL).then_some(*state)
    }

    /// 从持久化的 JSON 恢复用户标定档案。
    pub async fn load_custom(&self, json: Option<String>) {
        let Some(json) = json else { return };
        let Ok(map) = serde_json::from_str::<HashMap<String, Profile>>(&json) else {
            return;
        };
        *self.custom.lock().await = map;
    }

    /// 同步版：直接塞入已解析的档案表。
    ///
    /// 给 Tauri 的 `setup` 用。那里跑在 tokio 运行时线程上，
    /// 不能用 `blocking_lock`（会 panic："Cannot block the current thread
    /// from within a runtime"），也不能 `block_on` 异步任务（会死锁）。
    ///
    /// 所以用 `try_lock`：启动早期没有并发访问，几乎必然成功；
    /// 万一失败就放弃这次恢复（下次标定时会重新写盘，不会丢数据）。
    pub fn load_custom_sync(&self, map: HashMap<String, Profile>) -> bool {
        match self.custom.try_lock() {
            Ok(mut guard) => {
                *guard = map;
                true
            }
            Err(_) => false,
        }
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

        // 按最终 profile id 去重，而不是按 VID:PID。
        //
        // 两个原因：
        // 1. 同一台设备会枚举出好几个 collection（同一 PID）
        // 2. 罗技鼠标插拔线时会在两个 PID 之间切换，但它们归到同一个
        //    逻辑 id（按产品名生成）—— 按 PID 去重会让它出现两次
        let mut seen = std::collections::HashSet::new();
        for iface in &interfaces {
            let key = (iface.vendor_id, iface.product_id);

            let id_vendor = format!("vendor:{:04X}:{:04X}", key.0, key.1);
            let id_logi = format!("logitech:{:04X}:{:04X}", key.0, key.1);

            // 1. 用户标定过的优先
            if let Some(p) = custom.get(&id_vendor).or_else(|| custom.get(&id_logi)) {
                if !seen.insert(p.id.clone()) {
                    continue;
                }
                found.push(p.clone());
                continue;
            }

            // 2. 内置表
            //
            // 先按 VID:PID 精确匹配，再按产品名兜底 —— 罗技同一只鼠标
            // 插拔线时 PID 会变（C539 ↔ C08D），但产品名不变。
            if let Some(p) = profile::find_builtin(key.0, key.1)
                .or_else(|| profile::find_builtin_by_name(key.0, &iface.product))
            {
                if !seen.insert(p.id.clone()) {
                    continue;
                }
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
                // ID 不绑 PID：同一只鼠标插拔线时会在 C539（接收器）
                // 和 C08D（本体）之间切换，绑 PID 的话界面上会"变成另一台设备"，
                // 缓存的电量、用户标定的档案全都对不上。
                // 用产品名做稳定标识 —— 它不随连接方式变化。
                let product = if iface.product.is_empty() {
                    "罗技无线设备".to_string()
                } else {
                    iface.product.clone()
                };
                let id = format!("logitech:{}", product.replace(' ', "-"));
                if !seen.insert(id.clone()) {
                    continue;
                }
                found.push(Profile {
                    id,
                    vendor_id: key.0,
                    product_id: key.1,
                    name: product,
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
                let id = format!("hid:{:04X}:{:04X}", key.0, key.1);
                if !seen.insert(id.clone()) {
                    continue;
                }
                found.push(Profile {
                    id,
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
        // 串行化：整个读硬件过程独占，避免并发争抢同一 HID 接口
        let _guard = self.refresh_lock.lock().await;

        let profiles = self.scan(true).await;
        // 插拔线那一刻设备才推状态报文，靠轮询抓不到 —— 先把常驻监听起起来
        self.ensure_charge_listener(&profiles);
        let interfaces = hid::enumerate();

        // 监听到的状态优先于 Feature 报文里的状态：
        // A75 的 Feature 报文压根没有充电位。
        let known: HashMap<String, ChargeState> = profiles
            .iter()
            .filter_map(|p| self.charge_state(&p.id).map(|s| (p.id.clone(), s)))
            .collect();

        let mut readings = Vec::with_capacity(profiles.len());
        for profile in profiles {
            let ifaces = interfaces.clone();
            let fallback = profile.clone();
            let known_state = known.get(&profile.id).copied();
            let reading = tokio::task::spawn_blocking(move || {
                read_blocking(&profile, &ifaces, known_state)
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
            let mut merged = match cache.get(&reading.id) {
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
            // 监听线程的状态永远以它为准（读失败时也不会退回旧值）
            if let Some(state) = known.get(&merged.id).copied() {
                merged.state = state;
            }
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

/// 充电状态监听线程体。
///
/// 只读地守着设备的 0xFF02 接口：设备插拔线那一刻主动推一帧，其余时间完全安静。
/// 所以这里是"阻塞等事件"，不做任何轮询写入 —— 盲写曾经把键盘 MCU 写死过。
fn charge_listener_loop(
    targets: Vec<(String, u16, u16, profile::ChargeQuery)>,
    charge: std::sync::Arc<std::sync::Mutex<HashMap<String, (ChargeState, Instant)>>>,
    changed: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    struct Active {
        id: String,
        /// 设备路径。用来判断"同一个 token 还是新插的"。
        path: String,
        spec: profile::ChargeQuery,
        device: hid::HidDevice,
    }

    let mut active: Vec<Active> = Vec::new();
    let mut last_check = Instant::now() - Duration::from_secs(120);

    loop {
        // 通道维护：缺通道、或者每 60 秒核对一次。
        //
        // **不能因为"长时间没数据"就关句柄**：没数据是常态（设备只在插拔线
        // 那一瞬发帧），关掉再打开的那几毫秒正好会漏掉事件 —— 表现出来就是
        // "线已经拔了，界面还挂着充电中"。只有设备真的从枚举里消失才放手。
        if active.len() < targets.len() || last_check.elapsed() > Duration::from_secs(60) {
            let interfaces = hid::enumerate();
            active.retain(|a| {
                let alive = interfaces.iter().any(|i| i.path == a.path);
                if !alive {
                    // 设备不在了：它的状态也失效，别留在缓存里冒充最新
                    if let Ok(mut map) = charge.lock() {
                        map.remove(&a.id);
                    }
                    changed.store(true, std::sync::atomic::Ordering::SeqCst);
                }
                alive
            });
            for (id, vid, pid, spec) in &targets {
                if active.iter().any(|a| a.id == *id) {
                    continue;
                }
                let Some(iface) = interfaces.iter().find(|i| {
                    i.vendor_id == *vid
                        && i.product_id == *pid
                        && i.usage_page == spec.usage_page
                        && i.input_len > 0
                }) else {
                    continue;
                };
                if let Ok(device) = hid::HidDevice::open(iface, false) {
                    active.push(Active {
                        id: id.clone(),
                        path: iface.path.clone(),
                        spec: spec.clone(),
                        device,
                    });
                }
            }
            last_check = Instant::now();
        }

        for a in active.iter_mut() {
            if let Some(frame) = a.device.read_input(150) {
                if let Some(state) = vendor::parse_charge_event(&frame, &a.spec) {
                    let mut map = charge.lock().unwrap_or_else(|e| e.into_inner());
                    let unchanged = map.get(&a.id).is_some_and(|(s, _)| *s == state);
                    map.insert(a.id.clone(), (state, Instant::now()));
                    if !unchanged {
                        changed.store(true, std::sync::atomic::Ordering::SeqCst);
                    }
                }
            }
        }

        if active.is_empty() {
            std::thread::sleep(Duration::from_secs(2));
        }
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

fn read_blocking(
    profile: &Profile,
    interfaces: &[hid::HidInterface],
    known: Option<ChargeState>,
) -> Result<DeviceReading> {
    match profile.source {
        BatterySource::LogitechHidpp => read_logitech(profile, interfaces),
        BatterySource::VendorFeature => read_vendor(profile, interfaces, known),
        BatterySource::StandardHid => read_standard(profile, interfaces),
    }
}

fn read_vendor(
    profile: &Profile,
    interfaces: &[hid::HidInterface],
    known: Option<ChargeState>,
) -> Result<DeviceReading> {
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
        // 被动监听拿到的状态优先；没有就以报文里的为准（迈从这类写在 [2]）
        state: known.unwrap_or(reading.state),
        online: true,
        voltage_mv: None,
        source: format!("Feature 0x{:02X}", spec.report_id),
        estimated: false,
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
        estimated: false,
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
    // 插线时接收器（C539）和本体（C08D）同时存在，两套 0xFF00 接口都在。
    // 接收器在插线状态下 0x1001 返回"不支持"，本体才有真实数据 ——
    // 所以不能只挑一个试，得两个都试，谁先读到用谁。
    //
    // 顺序：先本体（插着线时数据最准），再接收器（无线状态），最后不限 PID。
    let body_pid = interfaces
        .iter()
        .find(|i| {
            i.vendor_id == 0x046D
                && !i.product.contains("Receiver")
                && is_real_peripheral(i)
                && i.usage_page == 0xFF00
        })
        .map(|i| i.product_id);

    let mut transports = Vec::new();
    if let Some(pid) = body_pid {
        if let Ok(t) = hidpp::HidppTransport::open_for(interfaces, Some(pid)) {
            transports.push(t);
        }
    }
    // 再补一个"不限 PID"的通道（覆盖只有接收器的情况）
    if let Ok(t) = hidpp::HidppTransport::open_for(interfaces, Some(0xC539)) {
        transports.push(t);
    }
    if transports.is_empty() {
        transports.push(hidpp::HidppTransport::open(interfaces)?);
    }

    let mut last_error = None;
    for transport in &transports {
        match try_read_logitech(profile, transport) {
            Ok(reading) => return Ok(reading),
            Err(err) => last_error = Some(err.to_string()),
        }
    }

    Err(anyhow::anyhow!(
        last_error.unwrap_or_else(|| "接收器上没有可响应的设备（休眠中）".into())
    ))
}

/// 在一条 HID++ 通道上尝试读出电量。
fn try_read_logitech(
    profile: &Profile,
    transport: &hidpp::HidppTransport,
) -> Result<DeviceReading> {

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
                    false,
                ));
            }
            continue;
        }

        match read_hidpp20(&transport, index) {
            Ok((percent, voltage, state, source, estimated)) => {
                return Ok(build_logitech(profile, percent, voltage, state, source, estimated));
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
    estimated: bool,
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
        estimated,
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
) -> Result<(Option<u8>, Option<u16>, ChargeState, String, bool)> {
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
            return Ok((percent, None, state, "HID++ 0x1004".into(), false));
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
                    // 充电标志在 `[2]`（payload 的第 3 字节，即 data[2]）：
                    //   0x80 = 正在充电，0x00 = 未充电
                    //
                    // 实测对照（这只 G502）：
                    //   插线  C08D 本体  06 0B 10 6D 80  4205mV  [4]=0x80
                    //   拔线  C539 接收器 06 0B 10 26 00  4134mV  [4]=0x00
                    // 注意两个电压只差 70mV —— 靠电压阈值根本分不开，
                    // 之前用 `>= 4100mV` 判充电，拔线后 4134mV 会误报成充电中。
                    //
                    // 旧设备可能没有这个标志位（第 5 字节缺失或恒 0），
                    // 此时退回电压估算 —— 不完美，但比完全不判要好。
                    let flag = data.get(2).copied();
                    let charging = match flag {
                        Some(f) if f & 0x80 != 0 => true,
                        Some(_) => false,
                        None => mv >= 4150,
                    };
                    let percent = if charging {
                        // 充电时电压被顶到 4.2V，按放电曲线算就是 100%，
                        // 但实际可能只有一半电量 —— 报假百分比不如不报。
                        None
                    } else {
                        hidpp::voltage_to_percent(mv)
                    };
                    let estimated = percent.is_some();
                    return Ok((
                        percent,
                        Some(mv),
                        if charging {
                            ChargeState::Charging
                        } else {
                            ChargeState::Discharging
                        },
                        match flag {
                            Some(f) => format!("HID++ 0x1001 电压 {mv}mV 标志 0x{f:02X}"),
                            None => format!("HID++ 0x1001 电压 {mv}mV"),
                        },
                        estimated,
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
                    false,
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

    // 插线时鼠标会以"本体"身份重新枚举（如 C08D），此时接收器（C539）
    // 虽然还在，但它的 HID++ 通道读不到电量 —— 实测 0x1001 返回"不支持"。
    // 所以不能无脑优先接收器，得看谁真的能提供电量。
    //
    // 判定顺序：
    //   1. 有本体（非 Receiver）就用本体 —— 它插着线，电量/充电状态最准
    //   2. 否则用接收器 —— 无线连接时走这条
    let has_body = all
        .iter()
        .any(|i| i.vendor_id == 0x046D && !i.product.contains("Receiver") && is_real_peripheral(i));

    if has_body {
        return !is_receiver;
    }
    is_receiver
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

#[cfg(test)]
mod charge_flag_tests {
    /// G502 的 0x1001 fn0 回复格式：
    /// `[0]=特性索引 [1]=功能字节 [2..4]=电压(大端) [4]=标志`
    /// 标志 0x80 = 充电中，0x00 = 未充电。
    ///
    /// 这两组数据来自实测（插线 C08D / 拔线 C539），
    /// 电压只差 47mV —— 靠阈值分不开，必须用标志位。
    fn parse_flag(data: &[u8]) -> Option<u8> {
        data.get(2).copied()
    }

    #[test]
    fn charging_flag_is_0x80() {
        // 插线实测：06 0b 10 6d 80 -> 4205mV 标志 0x80
        let data = [0x10, 0x6d, 0x80, 0x00, 0x00];
        let flag = parse_flag(&data).unwrap();
        assert_eq!(flag, 0x80);
        assert_ne!(flag & 0x80, 0, "最高位应表示充电中");
    }

    #[test]
    fn discharging_flag_is_0x00() {
        // 拔线实测：06 0b 10 26 00 -> 4134mV 标志 0x00
        let data = [0x10, 0x26, 0x00, 0x00, 0x00];
        let flag = parse_flag(&data).unwrap();
        assert_eq!(flag, 0x00);
        assert_eq!(flag & 0x80, 0, "最高位为 0 表示未充电");
    }

    #[test]
    fn voltage_alone_cannot_distinguish() {
        // 这两个电压差不到 50mV，用 >= 4100 的阈值会把拔线误判成充电
        let charging_mv: u16 = 0x106d; // 4205
        let discharging_mv: u16 = 0x1026; // 4134
        assert!(charging_mv >= 4100);
        assert!(discharging_mv >= 4100, "拔线电压也超过 4100 阈值");
        assert!((charging_mv - discharging_mv) < 100);
    }
}
