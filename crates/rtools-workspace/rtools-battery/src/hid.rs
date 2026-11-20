//! HID 底层：SetupAPI 枚举 + 重叠 I/O 读写。
//!
//! 三条硬约束（踩过的坑，别再踩）：
//! 1. 绝不在同一句柄上"阻塞 ReadFile + 并发 WriteFile" —— 会直接杀掉进程，
//!    连异常栈都没有。读必须走独立的 `FILE_FLAG_OVERLAPPED` 句柄 + 事件超时 + `CancelIoEx`。
//! 2. 鼠标/键盘/笔是系统独占顶层集合，只能用 `dwDesiredAccess = 0` 打开：
//!    能读属性，不能收发指令。厂商通道（0xFF00/0xFF02/0xFF04）不受此限。
//! 3. `HidD_GetFeature` 的 buffer 不能只按接口声明的 `FeatureReportByteLength` 给 ——
//!    厂商会把额外字节写在声明长度之外，必须给足余量才读得到。

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

use anyhow::{anyhow, Result};
use windows::core::PCWSTR;
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW,
    SetupDiGetDeviceInterfaceDetailW, DIGCF_DEVICEINTERFACE, DIGCF_PRESENT,
    SP_DEVICE_INTERFACE_DATA, SP_DEVICE_INTERFACE_DETAIL_DATA_W,
};
use windows::Win32::Devices::HumanInterfaceDevice::{
    HidD_FreePreparsedData, HidD_GetAttributes, HidD_GetFeature, HidD_GetManufacturerString,
    HidD_GetPreparsedData, HidD_GetProductString, HidD_GetSerialNumberString,
    HidD_SetNumInputBuffers, HidP_GetCaps, GUID_DEVINTERFACE_HID, HIDD_ATTRIBUTES, HIDP_CAPS,
    HIDP_STATUS_SUCCESS, PHIDP_PREPARSED_DATA,
};
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_IO_PENDING, GENERIC_READ, GENERIC_WRITE, HANDLE,
    INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, WriteFile, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE,
    OPEN_EXISTING,
};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

/// 读 Feature Report 时额外申请的字节数。
///
/// 厂商常把数据写在声明长度之外（A75 声明 8 字节，电量在第 9 字节），
/// 这里统一多要一段，读到后由协议层决定取哪几个字节。
const FEATURE_SLACK: usize = 56;

/// 一条 HID 接口（一个 top-level collection）的静态信息。
#[derive(Debug, Clone)]
pub struct HidInterface {
    pub path: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub version: u16,
    pub usage_page: u16,
    pub usage: u16,
    pub input_len: u16,
    pub output_len: u16,
    pub feature_len: u16,
    pub product: String,
    pub manufacturer: String,
    pub serial: String,
}

impl HidInterface {
    pub fn vid_pid(&self) -> String {
        format!("{:04X}:{:04X}", self.vendor_id, self.product_id)
    }
}

fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(Some(0)).collect()
}

/// 枚举系统里所有 HID 接口。
///
/// 单个接口读取失败不影响整体 —— 蓝牙设备离线、权限不足都很常见。
pub fn enumerate() -> Vec<HidInterface> {
    let mut out = Vec::new();
    let guid = GUID_DEVINTERFACE_HID;

    let Ok(dev_info) = (unsafe {
        SetupDiGetClassDevsW(
            Some(&guid),
            PCWSTR::null(),
            None,
            DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
        )
    }) else {
        return out;
    };

    let mut index = 0u32;
    loop {
        let mut iface_data = SP_DEVICE_INTERFACE_DATA {
            cbSize: std::mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
            ..Default::default()
        };
        if unsafe { SetupDiEnumDeviceInterfaces(dev_info, None, &guid, index, &mut iface_data) }
            .is_err()
        {
            break;
        }
        index += 1;

        let Some(path) = device_path(dev_info, &iface_data) else {
            continue;
        };
        if let Some(iface) = read_interface(&path) {
            out.push(iface);
        }
    }

    unsafe {
        let _ = SetupDiDestroyDeviceInfoList(dev_info);
    }
    out
}

fn device_path(
    dev_info: windows::Win32::Devices::DeviceAndDriverInstallation::HDEVINFO,
    iface_data: &SP_DEVICE_INTERFACE_DATA,
) -> Option<String> {
    // 先问需要多大缓冲
    let mut needed = 0u32;
    unsafe {
        let _ = SetupDiGetDeviceInterfaceDetailW(
            dev_info,
            iface_data,
            None,
            0,
            Some(&mut needed),
            None,
        );
    }
    if needed == 0 {
        return None;
    }

    let mut buf = vec![0u8; needed as usize];
    let detail = buf.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
    unsafe {
        // cbSize：x64 为 8，x86 为 6
        (*detail).cbSize = if cfg!(target_pointer_width = "64") { 8 } else { 6 };
        if SetupDiGetDeviceInterfaceDetailW(
            dev_info,
            iface_data,
            Some(detail),
            needed,
            None,
            None,
        )
        .is_err()
        {
            return None;
        }
        let ptr = (*detail).DevicePath.as_ptr();
        let mut len = 0usize;
        while *ptr.add(len) != 0 {
            len += 1;
        }
        Some(String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len)))
    }
}

fn read_interface(path: &str) -> Option<HidInterface> {
    // 读属性只需要属性访问权限，用 0 打开最稳（独占设备也能拿到信息）
    let handle = open_handle(path, false, false)?;
    let _guard = HandleGuard(handle);

    let mut attrs = HIDD_ATTRIBUTES {
        Size: std::mem::size_of::<HIDD_ATTRIBUTES>() as u32,
        ..Default::default()
    };
    if !unsafe { HidD_GetAttributes(handle, &mut attrs) } {
        return None;
    }

    let mut preparsed = PHIDP_PREPARSED_DATA::default();
    if !unsafe { HidD_GetPreparsedData(handle, &mut preparsed) } {
        return None;
    }
    let mut caps = HIDP_CAPS::default();
    let status = unsafe { HidP_GetCaps(preparsed, &mut caps) };
    unsafe {
        let _ = HidD_FreePreparsedData(preparsed);
    }
    if status != HIDP_STATUS_SUCCESS {
        return None;
    }

    Some(HidInterface {
        path: path.to_string(),
        vendor_id: attrs.VendorID,
        product_id: attrs.ProductID,
        version: attrs.VersionNumber,
        usage_page: caps.UsagePage,
        usage: caps.Usage,
        input_len: caps.InputReportByteLength,
        output_len: caps.OutputReportByteLength,
        feature_len: caps.FeatureReportByteLength,
        product: read_string(handle, HidD_GetProductString),
        manufacturer: read_string(handle, HidD_GetManufacturerString),
        serial: read_string(handle, HidD_GetSerialNumberString),
    })
}

fn read_string(handle: HANDLE, getter: unsafe fn(HANDLE, *mut core::ffi::c_void, u32) -> bool) -> String {
    let mut buf = [0u16; 256];
    let ok = unsafe {
        getter(
            handle,
            buf.as_mut_ptr() as *mut core::ffi::c_void,
            (buf.len() * 2) as u32,
        )
    };
    if !ok {
        return String::new();
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

/// 打开 HID 句柄。`read`/`write` 控制访问位；两者都为 false 时只拿属性。
pub fn open_handle(path: &str, read: bool, write: bool) -> Option<HANDLE> {
    let mut access = 0u32;
    if read {
        access |= GENERIC_READ.0;
    }
    if write {
        access |= GENERIC_WRITE.0;
    }

    let wide_path = wide(path);
    match unsafe {
        CreateFileW(
            PCWSTR(wide_path.as_ptr()),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED,
            None,
        )
    } {
        Ok(h) if h != INVALID_HANDLE_VALUE => Some(h),
        _ => None,
    }
}

struct HandleGuard(HANDLE);

impl Drop for HandleGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// 一条可读写的 HID 连接。
///
/// 所有 I/O 都是重叠 I/O + 事件超时，超时后 `CancelIoEx`，
/// 保证上层调用不会被设备挂死。
pub struct HidDevice {
    /// 主句柄：写 Output Report、读 Feature Report 走它。
    handle: HANDLE,
    /// 独立的读句柄。
    ///
    /// **读写必须分开**。厂商命令通道（0xFF02）上，用同一个读写句柄
    /// 写完立刻读，设备回复会被吞掉 —— 实测 A75 的 `13 05 00` 在
    /// 同句柄下 6/6 无回复，换成独立读句柄后立刻能收到。
    ///
    /// 打开失败时为 `INVALID_HANDLE_VALUE`，此时退回用主句柄读
    /// （Feature-only 的设备不需要独立读句柄）。
    read_handle: HANDLE,
    pub iface: HidInterface,
}

// HANDLE 在 Windows 上是内核对象句柄，跨线程使用是安全的；
// 内部没有共享可变状态，读写的并发安全由调用方保证（本模块一律串行调用）。
unsafe impl Send for HidDevice {}

impl HidDevice {
    pub fn open(iface: &HidInterface, write: bool) -> Result<Self> {
        let handle = open_handle(&iface.path, true, write)
            .ok_or_else(|| anyhow!("打开 HID 设备失败：{}", iface.path))?;
        unsafe {
            // 输入缓冲开大一点，避免高频报文（鼠标）把队列打满后丢回复
            let _ = HidD_SetNumInputBuffers(handle, 64);
        }

        // 只有需要收 Output Report 回复的设备（有 input_len）才另开读句柄。
        //
        // **必须带 GENERIC_READ**（`read: true`）。用 `access=0` 打开只能读属性，
        // `ReadFile` 读不到 Input Report —— 实测就是"写成功、永远无回复"，
        // 而 Python 侧用 GENERIC_READ 开句柄能正常收到，差异全在这里。
        let read_handle = if iface.input_len > 0 {
            match open_handle(&iface.path, true, false) {
                Some(h) => h,
                None => INVALID_HANDLE_VALUE,
            }
        } else {
            INVALID_HANDLE_VALUE
        };

        Ok(Self {
            handle,
            read_handle,
            iface: iface.clone(),
        })
    }

    /// 写一条 Output Report。长度不足时补 0 到接口声明的输出长度。
    pub fn write_output(&self, report: &[u8], timeout_ms: u32) -> Result<()> {
        let len = (self.iface.output_len as usize).max(report.len());
        let mut buf = vec![0u8; len];
        buf[..report.len()].copy_from_slice(report);
        write_overlapped(self.handle, &buf, timeout_ms)
    }

    /// 读一条 Input Report。超时返回 `None`（不是错误 —— 设备休眠是常态）。
    pub fn read_input(&self, timeout_ms: u32) -> Option<Vec<u8>> {
        let len = self.iface.input_len as usize;
        if len == 0 {
            return None;
        }
        // 优先用独立读句柄；没有则退回主句柄
        let h = if self.read_handle != INVALID_HANDLE_VALUE {
            self.read_handle
        } else {
            self.handle
        };
        read_overlapped(h, len, timeout_ms)
    }

    /// 读 Feature Report。
    ///
    /// buffer 比接口声明长度多给 [`FEATURE_SLACK`] 字节 —— 厂商常把电量等
    /// 关键字段写在声明长度之外，按声明长度读会永远读不到。
    pub fn get_feature(&self, report_id: u8) -> Option<Vec<u8>> {
        let len = (self.iface.feature_len as usize).max(1) + FEATURE_SLACK;
        let mut buf = vec![0u8; len];
        buf[0] = report_id;
        let ok = unsafe { HidD_GetFeature(self.handle, buf.as_mut_ptr() as *mut _, len as u32) };
        if !ok {
            return None;
        }
        Some(buf)
    }

    /// 写 Feature Report。
    ///
    /// **本模块不提供这个能力，故意留空。**
    ///
    /// 厂商的 Feature 通道（0xFF04 这类）既能读也能写，写进去的是配置命令：
    /// 灯效、回报率、休眠、按键映射，以及各家私有的"锁定/复位"类指令。
    /// 一旦按"扫一遍看哪个 RID 有反应"的思路盲写，就可能把设备写进
    /// 固件不处理任何输入的异常态 —— 实测在 Leobog A75 上踩过：
    /// 接收器仍然在线、Windows 也报告设备正常，但键盘 MCU 不再产生按键事件，
    /// 只能拔插接收器 / 断电复位才能恢复。
    ///
    /// 读电量只需要 `get_feature`。真要支持"改配置"，必须先把命令语义
    /// 从官方驱动里逆向清楚、逐条验证，而不是靠枚举试出来。
    pub fn set_feature_disabled(&self, _report: &[u8]) -> bool {
        false
    }

    /// 把输入队列里积压的报文读干净，避免上一条命令的迟到回复被当成本次结果。
    pub fn drain(&self, max: usize) {
        for _ in 0..max {
            if self.read_input(1).is_none() {
                break;
            }
        }
    }
}

impl Drop for HidDevice {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.handle);
            if self.read_handle != INVALID_HANDLE_VALUE && self.read_handle != self.handle {
                let _ = CloseHandle(self.read_handle);
            }
        }
    }
}

fn write_overlapped(handle: HANDLE, data: &[u8], timeout_ms: u32) -> Result<()> {
    let event = unsafe { CreateEventW(None, true, false, PCWSTR::null()) }
        .map_err(|e| anyhow!("CreateEventW 失败：{e}"))?;
    let _event_guard = HandleGuard(event);

    let mut overlapped = OVERLAPPED {
        hEvent: event,
        ..Default::default()
    };
    let mut written = 0u32;

    let res = unsafe {
        WriteFile(
            handle,
            Some(data),
            Some(&mut written),
            Some(&mut overlapped),
        )
    };

    if let Err(_) = res {
        let err = unsafe { GetLastError() };
        if err != ERROR_IO_PENDING {
            return Err(anyhow!("WriteFile 失败：Win32 {err:?}"));
        }
        let wait = unsafe { WaitForSingleObject(event, timeout_ms) };
        if wait != WAIT_OBJECT_0 {
            unsafe {
                let _ = CancelIoEx(handle, Some(&overlapped));
            }
            return Err(anyhow!("写 Output Report 超时"));
        }
        let mut got = 0u32;
        if unsafe { GetOverlappedResult(handle, &overlapped, &mut got, false) }.is_err() {
            let err = unsafe { GetLastError() };
            return Err(anyhow!("写 Output Report 失败：Win32 {err:?}"));
        }
    }
    Ok(())
}

fn read_overlapped(handle: HANDLE, len: usize, timeout_ms: u32) -> Option<Vec<u8>> {
    let event = unsafe { CreateEventW(None, true, false, PCWSTR::null()) }.ok()?;
    let _event_guard = HandleGuard(event);

    let mut buf = vec![0u8; len];
    let mut overlapped = OVERLAPPED {
        hEvent: event,
        ..Default::default()
    };
    let mut read = 0u32;

    let res = unsafe {
        ReadFile(
            handle,
            Some(&mut buf),
            Some(&mut read),
            Some(&mut overlapped),
        )
    };

    if let Err(_) = res {
        let err = unsafe { GetLastError() };
        if err != ERROR_IO_PENDING {
            return None;
        }
        let wait = unsafe { WaitForSingleObject(event, timeout_ms) };
        if wait != WAIT_OBJECT_0 {
            unsafe {
                let _ = CancelIoEx(handle, Some(&overlapped));
            }
            // 取消后要等事件收尾，否则句柄关闭时内核对象仍被占用
            unsafe {
                let _ = WaitForSingleObject(event, 100);
            }
            return None;
        }
        let mut got = 0u32;
        if unsafe { GetOverlappedResult(handle, &overlapped, &mut got, false) }.is_err() {
            return None;
        }
        read = got;
    }

    buf.truncate(read as usize);
    Some(buf)
}
