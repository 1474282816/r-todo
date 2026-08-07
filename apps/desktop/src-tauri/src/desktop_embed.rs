//! Windows 桌面嵌入：挂载到 WorkerW（桌面图标层），Win+D 显示桌面时不收起。
//!
//! 原理：Win+D 只会最小化"顶层窗口"。把窗口 `SetParent` 到 explorer 的
//! WorkerW 之后，它成为 WorkerW 的子窗口、不再是顶层窗口，Win+D 碰不到它；
//! 同时天然置底（应用窗口之下、桌面图标之上），不进任务栏 / Alt+Tab。
//!
//! 算法（滴答清单 / Fences 同款，见 backlog OPT-002）：
//! 1. 向 "Progman" 发 0x052C，让 explorer 创建并显示 WorkerW；
//! 2. 枚举顶层窗口，找到"含 SHELLDLL_DefView 子窗口的 WorkerW"（桌面图标层），
//!    `SetParent` 挂到它上面 —— 窗口显示在图标之上、应用之下，鼠标可正常命中；
//!    ⚠️ 不要挂"空 WorkerW（壁纸层）"：它在图标层之下，全屏图标层会挡住
//!    所有鼠标消息，窗口看得见但点不了（electron-as-wallpaper 用低层钩子
//!    转发输入就是因为这个原因）；
//! 3. `SetParent` 保持 WS_POPUP、绝不设 WS_CHILD
//!    —— MSDN 明确 SetParent 不改 WS_CHILD/WS_POPUP 样式；
//!    WS_CHILD + Progman 会让窗口完全不可见（OPT-002 实测过的失败原因）；
//! 4. `SetWindowPos(SWP_FRAMECHANGED)` 刷新，触发 WebView2 重排重绘。
//!
//! 已知限制（后续项）：
//! - explorer 崩溃/重启、壁纸切换时 WorkerW 会被重建，挂载关系丢失，
//!   需要低频 GetParent 检查 + 自动重挂。
//! - 跨进程 SetParent 会把本进程 DPI awareness forced reset，
//!   这里 SetParent 后立即尽力恢复 per-monitor v2。
//! - WebView2 输入焦点在个别机器上可能异常，需实机验证
//!   （tauri#10345 用户实测 SetParent 挂桌面层可用）。

#![cfg(target_os = "windows")]

use std::sync::atomic::{AtomicIsize, Ordering};

use tauri::{Manager, WebviewWindow};
use windows::core::{w, PCWSTR};
use windows::Win32::{
    Foundation::{BOOL, HWND, LPARAM, LRESULT, TRUE, WPARAM},
    Graphics::Dwm::DwmExtendFrameIntoClientArea,
    UI::{
        Controls::MARGINS,
        HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2},
        WindowsAndMessaging::{
            CallWindowProcW, DefWindowProcW, EnumWindows, FindWindowExW, FindWindowW,
            GetClassNameW, GetWindowLongPtrW, SendMessageTimeoutW, SetParent, SetWindowLongPtrW,
            SetWindowPos, GWLP_WNDPROC, GWL_EXSTYLE, HWND_NOTOPMOST, SMTO_NORMAL,
            SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
            WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
        },
    },
};

/// 让 Progman 创建/显示 WorkerW 的 explorer 内部消息
const WM_SPAWN_WORKERW: u32 = 0x052C;

/// 非客户区消息：失活重绘 / 绘制边框（经典 3D 边框就是从这两条消息画出来的）
const WM_NCACTIVATE: u32 = 0x0086;
const WM_NCPAINT: u32 = 0x0085;

/// 嵌入期间挂的 WndProc 原链（0 = 未安装）。
///
/// ⚠️ 用无锁原子而非 Mutex：窗口过程在 UI 线程每收到一条消息都会读它，
/// 若在此加锁，install 线程（tokio）持锁调用 SetWindowLongPtrW 等待 UI 线程时
/// 会与 UI 线程在窗口过程里的等锁形成死锁（实测：启动后窗口卡死）。
/// isize 读写本身原子，AtomicIsize 足够。
static ORIGINAL_WNDPROC: AtomicIsize = AtomicIsize::new(0);

unsafe extern "system" fn embedded_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        // 点击桌面空白处 → 窗口失活 → 系统重绘非客户区，把经典白色边框画出来。
        // 直接吞掉，不画（无边框窗口标准做法）。
        WM_NCACTIVATE => return LRESULT(1),
        WM_NCPAINT => return LRESULT(0),
        _ => {}
    }
    let orig = ORIGINAL_WNDPROC.load(Ordering::Relaxed);
    if orig == 0 {
        // 兜底：install 尚未写完（理论不会发生，防御时序竞争）
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    // Option<fn> 带 niche（None=0），直接 transmute isize → WNDPROC 严格语义下是 UB；
    // 先转裸函数指针再包 Some
    let f: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT =
        std::mem::transmute(orig);
    CallWindowProcW(Some(f), hwnd, msg, wparam, lparam)
}

/// 安装 subclass：阻止系统绘制非客户区（经典边框）
fn install_borderless_wndproc(hwnd: HWND) {
    unsafe {
        // 防御窗口销毁/重建后原子残留旧指针：先清理上次挂载（同进程内 tao
        // 所有窗口共用同一 WndProc 函数，恢复无害；下面会重新 Get 覆盖）
        let stale = ORIGINAL_WNDPROC.swap(0, Ordering::Relaxed);
        if stale != 0 {
            SetWindowLongPtrW(hwnd, GWLP_WNDPROC, stale);
        }
        let orig = GetWindowLongPtrW(hwnd, GWLP_WNDPROC);
        // 先写原链，再替换窗口过程（避免替换瞬间回调读到 0）
        ORIGINAL_WNDPROC.store(orig, Ordering::Relaxed);
        SetWindowLongPtrW(hwnd, GWLP_WNDPROC, embedded_wndproc as *const () as isize);
    }
}

/// 卸载 subclass，恢复 tao 原 WndProc
fn restore_wndproc(hwnd: HWND) {
    unsafe {
        let orig = ORIGINAL_WNDPROC.swap(0, Ordering::Relaxed);
        if orig != 0 {
            SetWindowLongPtrW(hwnd, GWLP_WNDPROC, orig);
        }
    }
}

fn get_hwnd(window: &WebviewWindow) -> Result<HWND, String> {
    let raw = window.hwnd().map_err(|e| format!("cannot get HWND: {e}"))?;
    Ok(HWND(raw.0 as *mut _))
}

// ── WorkerW 查找 ──────────────────────────────────────────────

/// "WorkerW" 的 UTF-16 编码（避免 per-window 堆分配字符串）
const WORKER_W_CLASS: &[u16] = &[0x57, 0x6F, 0x72, 0x6B, 0x65, 0x72, 0x57];

#[derive(Default)]
struct WorkerWSearch {
    /// 含 SHELLDLL_DefView 子窗口的 WorkerW（桌面图标层）——**首选**挂载目标。
    /// 挂上去后窗口显示在图标之上、应用之下，鼠标可正常命中（可交互）。
    interactive: Option<HWND>,
    /// 空 WorkerW（壁纸层）——fallback。挂在它下面会被图标层 WorkerW
    /// 挡住所有鼠标消息，窗口不可交互。
    wallpaper: Option<HWND>,
}

/// EnumWindows 回调里**不能**调用 FindWindow/FindWindowEx（它们内部会发消息，
/// 造成重入死锁，EnumWindows 报 0x80070102 超时）——所以这里只收集句柄，
/// 类名判断和子窗口检查全部放到回调外的循环里。
unsafe extern "system" fn collect_top_level(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let windows = &mut *(lparam.0 as *mut Vec<HWND>);
    windows.push(hwnd);
    TRUE
}

/// 找到可挂载的 WorkerW 桌面层
fn find_worker_w() -> Result<HWND, String> {
    unsafe {
        // 1. Progman 触发 explorer 创建/显示 WorkerW
        let progman = FindWindowW(w!("Progman"), PCWSTR::null())
            .map_err(|_| "找不到 Progman 窗口".to_string())?;
        let mut result = 0usize;
        let sent = SendMessageTimeoutW(
            progman,
            WM_SPAWN_WORKERW,
            WPARAM(0),
            LPARAM(0),
            SMTO_NORMAL,
            1000,
            Some(&mut result),
        );
        if sent.0 == 0 {
            // 不致命：WorkerW 可能已存在，枚举仍能命中
            log::warn!("0x052C 发送失败，继续尝试枚举已有 WorkerW");
        }

        // 2. 枚举所有顶层窗口。只收集句柄（回调内不能调 FindWindowExW），
        //    类名判断 + 子窗口检查放到回调外的循环里
        let mut all: Vec<HWND> = Vec::new();
        EnumWindows(
            Some(collect_top_level),
            LPARAM(&mut all as *mut Vec<HWND> as isize),
        )
        .map_err(|e| format!("EnumWindows 失败: {e}"))?;

        // 3. 挑出挂载目标（EnumWindows 按 z-order 从上到下枚举）
        let mut search = WorkerWSearch::default();
        for hwnd in all {
            let mut class_buf = [0u16; 256];
            let len = GetClassNameW(hwnd, &mut class_buf);
            if len <= 0 {
                continue;
            }
            if &class_buf[..len as usize] != WORKER_W_CLASS {
                continue;
            }

            let has_defview = match FindWindowExW(
                hwnd,
                HWND::default(),
                w!("SHELLDLL_DefView"),
                PCWSTR::null(),
            ) {
                Ok(dv) => !dv.0.is_null(),
                Err(_) => false,
            };
            if has_defview {
                // 桌面图标层：首选，窗口在图标之上可交互
                if search.interactive.is_none() {
                    search.interactive = Some(hwnd);
                }
            } else if search.wallpaper.is_none() {
                // 壁纸层：fallback（会被图标层挡住鼠标，不可交互）
                search.wallpaper = Some(hwnd);
            }
        }

        search
            .interactive
            .or(search.wallpaper)
            .ok_or_else(|| "找不到 WorkerW 桌面层（0x052C 未生效？）".to_string())
    }
}

// ── 嵌入 / 卸载 ───────────────────────────────────────────────

/// 卸载后恢复普通窗口：回到任务栏 / Alt+Tab
/// （嵌入时不需要对称操作：SetParent 成子窗口后本就不进任务栏 / Alt+Tab）
fn apply_floating_style(hwnd: HWND) {
    unsafe {
        let cur = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let mut new = cur;
        new &= !(WS_EX_TOOLWINDOW.0 as isize);
        new |= WS_EX_APPWINDOW.0 as isize;
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new);
    }
}

fn embed_impl(window: &WebviewWindow) -> Result<(), String> {
    let hwnd = get_hwnd(window)?;
    let workerw = find_worker_w()?;

    unsafe {
        // 关键：SetParent 不修改 WS_CHILD/WS_POPUP（MSDN 兼容性保证），
        // 窗口保持 WS_POPUP 挂到壁纸层，从此不再是顶层窗口。
        SetParent(hwnd, workerw).map_err(|e| format!("SetParent(WorkerW) 失败: {e}"))?;

        // 跨进程 SetParent 会让 DWM 的"无边框"状态失效：tao 的无边框窗口
        // 靠 WS_THICKFRAME + 把整个客户区扩展为 DWM 标题栏区域来隐藏边框，
        // SetParent 后该状态丢失，WS_THICKFRAME 退化为经典 3D 白色边框
        // （用户实测：嵌入后窗口外圈多了一圈浅色边框）。重新扩展客户区即可
        // 隐藏（tao/winit 创建无边框窗口时同款做法）。
        let margins = MARGINS {
            cxLeftWidth: -1,
            cxRightWidth: -1,
            cyTopHeight: -1,
            cyBottomHeight: -1,
        };
        let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);

        // 刷新非客户区 + 确保显示，触发 WebView2 重排重绘
        let _ = SetWindowPos(
            hwnd,
            HWND::default(),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_FRAMECHANGED | SWP_SHOWWINDOW | SWP_NOACTIVATE,
        );

        // 跨进程 SetParent 会 forced reset 本进程 DPI awareness，尽力恢复
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        // 拦截 WM_NCACTIVATE/WM_NCPAINT：点击桌面（窗口失活）时系统会重绘
        // 非客户区、把经典 3D 白色边框画出来（resize 时因重合成暂时掩盖）。
        install_borderless_wndproc(hwnd);
    }

    log::info!("embedded to desktop WorkerW: {workerw:?}");
    Ok(())
}

fn unembed_impl(window: &WebviewWindow) -> Result<(), String> {
    let hwnd = get_hwnd(window)?;

    unsafe {
        // 先恢复 tao 原 WndProc
        restore_wndproc(hwnd);

        // 恢复为顶层窗口
        SetParent(hwnd, HWND::default()).map_err(|e| format!("SetParent(NULL) 失败: {e}"))?;
        let _ = SetWindowPos(
            hwnd,
            HWND_NOTOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_FRAMECHANGED | SWP_SHOWWINDOW | SWP_NOACTIVATE,
        );
    }

    log::info!("unembedded from desktop");
    Ok(())
}

#[tauri::command]
pub async fn embed_to_desktop(window: tauri::Window) -> Result<(), String> {
    let win = window
        .get_webview_window(window.label())
        .ok_or_else(|| "webview window missing".to_string())?;
    embed_impl(&win)
}

#[tauri::command]
pub async fn unembed_from_desktop(window: tauri::Window) -> Result<(), String> {
    let win = window
        .get_webview_window(window.label())
        .ok_or_else(|| "webview window missing".to_string())?;
    unembed_impl(&win)?;
    apply_floating_style(get_hwnd(&win)?);
    Ok(())
}
