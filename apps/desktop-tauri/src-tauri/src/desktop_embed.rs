//! Windows 桌面挂件：shell ownership + 始终置底 + Win+D 豁免 + 真 DWM 材质。
//!
//! 为什么不用 SetParent(Progman/WorkerW)（旧方案，已废弃）：
//! SetParent 会把窗口变成 explorer 的 **child window**，而 Win11 的全部 DWM
//! 材质 API 都只对 top-level 窗口生效 —— 实测三条路全被系统拒绝：
//!   - DwmSetWindowAttribute(DWMWA_SYSTEMBACKDROP_TYPE)：调用成功但无视觉效果
//!   - SetWindowCompositionAttribute(ACCENT_ENABLE_ACRYLICBLURBEHIND)：INVALID_HANDLE
//!   - DwmEnableBlurBehindWindow：E_INVALIDARG
//! child 化之后永远拿不到系统毛玻璃。
//!
//! 新方案（WitchDrawer / Rainlendar "On Desktop" 同款）：
//! 窗口**保持 top-level**，只把 **owner**（GWLP_HWNDPARENT）设为 explorer 的
//! Progman。owner 关系带来：
//!   1. Win+D 不收起（owner 是 shell 桌面，Show Desktop 不会最小化 owned 窗口）
//!   2. top-level 身份保留 → DWM SystemBackdrop（Mica / Acrylic / Mica Alt）
//!      正常生效 → **实时毛玻璃**，换壁纸/移动窗口 DWM 自动跟随，零 CPU
//!   3. 不与 explorer 合并输入队列（跨进程 SetParent 的已知恶果，Raymond Chen
//!      在 The Old New Thing 里警告过）
//! 配合三件套：
//!   - WS_EX_TOOLWINDOW：不进任务栏 / Alt+Tab
//!   - WndProc 拦 WM_SYSCOMMAND/SC_MINIMIZE：防系统手势把挂件最小化
//!   - SetWinEventHook(EVENT_SYSTEM_FOREGROUND)：前台切到别的应用时把自己
//!     重新压回 HWND_BOTTOM（挂件自己被激活时不压，保证可交互）
//! 另外监听 TaskbarCreated：explorer 崩溃重启后 owner 链丢失，自动重挂。

#![cfg(target_os = "windows")]

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::OnceLock;

use tauri::{Emitter, Manager, WebviewWindow};
use windows::core::w;
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED},
    UI::{
        Accessibility::{SetWinEventHook, UnhookWinEvent},
        Shell::{ITaskbarList, TaskbarList},
        WindowsAndMessaging::{
            CallWindowProcW, DefWindowProcW, DispatchMessageW, GetAncestor, GetForegroundWindow,
            GetWindow, GetWindowLongPtrW, IsWindow, IsWindowVisible, MsgWaitForMultipleObjectsEx,
            PeekMessageW, RegisterWindowMessageW, SetWindowLongPtrW, SetWindowPos, TranslateMessage,
            EVENT_SYSTEM_FOREGROUND, GA_ROOT, GWLP_HWNDPARENT, GWLP_WNDPROC, GW_HWNDNEXT, GWL_EXSTYLE,
            HWND_BOTTOM, HWND_TOP, MSG, MWMO_INPUTAVAILABLE, PM_REMOVE, QS_ALLINPUT, SC_MINIMIZE,
            SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
            WINEVENT_OUTOFCONTEXT, WM_NCACTIVATE, WM_NCPAINT, WM_SYSCOMMAND, WS_EX_APPWINDOW,
            WS_EX_TOOLWINDOW,
        },
    },
};

/// 挂件模式的 HWND（0 = 未启用）。前台监听线程读它来重新置底。
static WIDGET_HWND: AtomicIsize = AtomicIsize::new(0);
/// 前台监听线程只启动一次
static KEEPER_STARTED: AtomicBool = AtomicBool::new(false);

/// AppHandle（wndproc 静态上下文里向前端 emit 用），embed 时写入
static APP_HANDLE: OnceLock<tauri::AppHandle> = OnceLock::new();

fn try_app_handle() -> Option<&'static tauri::AppHandle> {
    APP_HANDLE.get()
}

/// 挂件期间的原 WndProc 链（0 = 未安装）。
///
/// ⚠️ 无锁原子而非 Mutex：窗口过程在 UI 线程每条消息都会读它，
/// install 线程（tokio）持锁调 SetWindowLongPtrW 等 UI 线程时，会与 UI 线程
/// 在窗口过程里等锁形成死锁（实测踩过：启动后窗口卡死）。
static ORIGINAL_WNDPROC: AtomicIsize = AtomicIsize::new(0);

/// explorer 重启广播（RegisterWindowMessageW("TaskbarCreated") 的值运行时取得）
static mut MSG_TASKBAR_CREATED: u32 = 0;

/// 系统设置变更广播（壁纸变化会带 lParam="ImageColor"）
const WM_SETTINGCHANGE: u32 = 0x001A;

unsafe extern "system" fn widget_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // explorer 重启广播：重挂 owner + 置底（MSG_TASKBAR_CREATED 为 0 时跳过）
    if msg != 0 && msg == MSG_TASKBAR_CREATED {
        reattach_after_explorer_restart(hwnd);
    }
    match msg {
        // 壁纸等系统设置变化 → 通知前端重新生成模糊背景（实时跟随换壁纸）
        WM_SETTINGCHANGE => {
            if let Some(app) = try_app_handle() {
                let _ = app.emit("wallpaper-changed", ());
            }
        }
        // 点击桌面空白处 → 窗口失活 → 系统重绘非客户区，画出经典白色边框。
        // 吞掉不画（无边框窗口标准做法）。
        WM_NCACTIVATE => return LRESULT(1),
        WM_NCPAINT => return LRESULT(0),
        // Win+D / 系统手势会对每个顶层窗口发 SC_MINIMIZE，直接拒绝，
        // 挂件保持可见（WitchDrawer 同款做法）。
        WM_SYSCOMMAND if (wparam.0 & 0xFFF0) == SC_MINIMIZE as usize => {
            return LRESULT(0);
        }
        _ => {}
    }
    let orig = ORIGINAL_WNDPROC.load(Ordering::Relaxed);
    if orig == 0 {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let f: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT =
        std::mem::transmute(orig);
    CallWindowProcW(Some(f), hwnd, msg, wparam, lparam)
}

/// explorer 崩溃/重启后 owner 链丢失：重新设置 owner = 新的 Progman 并置底
unsafe fn reattach_after_explorer_restart(hwnd: HWND) {
    let Ok(progman) = windows::Win32::UI::WindowsAndMessaging::FindWindowW(w!("Progman"), None)
    else {
        return;
    };
    SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, progman.0 as isize);
    let _ = SetWindowPos(
        hwnd,
        HWND_BOTTOM,
        0,
        0,
        0,
        0,
        SWP_NOMOVE | SWP_NOSIZE | SWP_FRAMECHANGED | SWP_SHOWWINDOW | SWP_NOACTIVATE,
    );
    log::info!("explorer restarted: reattached widget to new Progman");
}

/// 安装 subclass（防边框 + 防 SC_MINIMIZE）
fn install_widget_wndproc(hwnd: HWND) {
    unsafe {
        let stale = ORIGINAL_WNDPROC.swap(0, Ordering::Relaxed);
        if stale != 0 {
            SetWindowLongPtrW(hwnd, GWLP_WNDPROC, stale);
        }
        let orig = GetWindowLongPtrW(hwnd, GWLP_WNDPROC);
        ORIGINAL_WNDPROC.store(orig, Ordering::Relaxed);
        SetWindowLongPtrW(hwnd, GWLP_WNDPROC, widget_wndproc as *const () as isize);
    }
}

/// 卸载 subclass
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

// ── 前台监听（保持置底） ─────────────────────────────────────

/// WinEventHook 回调：任何前台变化（不含挂件自己）→ 挂件重新压底。
/// 需要线程有消息循环（WINEVENT_OUTOFCONTEXT 通过消息队列投递）。
unsafe extern "system" fn on_foreground_changed(
    _hook: windows::Win32::UI::Accessibility::HWINEVENTHOOK,
    _event: u32,
    _hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _thread: u32,
    _time: u32,
) {
    send_widget_to_bottom();
}

fn send_widget_to_bottom() {
    let h = WIDGET_HWND.load(Ordering::Relaxed);
    if h == 0 {
        return;
    }
    let hwnd = HWND(h as *mut _);
    unsafe {
        if !IsWindow(hwnd).as_bool() || !IsWindowVisible(hwnd).as_bool() {
            return;
        }
        // 已经在 Z 序最底（没有下一个窗口）：不用重复压
        if Ok(HWND::default()) == GetWindow(hwnd, GW_HWNDNEXT) {
            return;
        }
        // 只在挂件不是前台窗口时压底：挂件被用户点开交互时保持在前台
        let fg = GetForegroundWindow();
        if fg != hwnd && GetAncestor(fg, GA_ROOT) != hwnd {
            let _ = SetWindowPos(
                hwnd,
                HWND_BOTTOM,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }
}

/// 把窗口从任务栏摘掉。
///
/// 只改 EXSTYLE 不够：explorer 已经为它建了任务栏按钮，样式变更后不会自动移除。
/// 必须显式 ITaskbarList::DeleteTab（ attributed to Windows shell 行为）。
fn remove_taskbar_button(hwnd: HWND) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        if let Ok(list) = CoCreateInstance::<_, ITaskbarList>(&TaskbarList, None, CLSCTX_INPROC_SERVER)
        {
            let _ = list.DeleteTab(hwnd);
        }
    }
}

/// 启动前台监听 + 周期保底的线程（幂等）
///
/// 光靠 EVENT_SYSTEM_FOREGROUND 不保险：挂件化之后如果一直没人切换窗口，
/// WebView2 初始化 / Tauri 的 show() 等会把窗口又顶到 Z 序最上面，
/// 视觉上就退化成一个普通浮空窗口。所以再加 1s 轮询保底，把非前台的挂件压回底部。
fn start_bottom_keeper(hwnd: HWND) {
    WIDGET_HWND.store(hwnd.0 as isize, Ordering::Relaxed);
    if KEEPER_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(|| unsafe {
        use windows::Win32::Foundation::HMODULE;

        let hook = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            HMODULE::default(),
            Some(on_foreground_changed),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        );

        let mut msg = MSG::default();
        'outer: loop {
            // 等 1s 或等消息到达（WinEvent 靠消息循环投递）
            MsgWaitForMultipleObjectsEx(None, 1000, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
            while PeekMessageW(&mut msg, HWND::default(), 0, 0, PM_REMOVE).as_bool() {
                if msg.message == 0x0012 {
                    // WM_QUIT：进程退出
                    break 'outer;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            send_widget_to_bottom();
        }
        if !hook.is_invalid() {
            let _ = UnhookWinEvent(hook);
        }
    });
}

// ── 挂件 / 卸载 ──────────────────────────────────────────────

fn embed_impl(window: &WebviewWindow) -> Result<(), String> {
    let hwnd = get_hwnd(window)?;
    let _ = APP_HANDLE.set(window.app_handle().clone());

    unsafe {
        // 1. 确保 top-level（如果之前是 child，先脱离）
        use windows::Win32::UI::WindowsAndMessaging::SetParent;
        let _ = SetParent(hwnd, HWND::default());

        // 2. shell ownership：owner = Progman（桌面）。
        //    owned 窗口不被 Win+D 最小化；窗口仍是 top-level，DWM 材质可用。
        let progman = windows::Win32::UI::WindowsAndMessaging::FindWindowW(
            w!("Progman"),
            None,
        )
        .map_err(|_| "找不到 Progman 窗口".to_string())?;
        SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, progman.0 as isize);

        // 3. 挂件样式：不进任务栏 / Alt+Tab。
        //
        //    必须同时清掉 WS_EX_APPWINDOW：两个 flag 并存时 **APPWINDOW 优先**，
        //    任务栏照样显示按钮（Tauri 建窗时自己加了 APPWINDOW），
        //    这正是"看起来还是个普通窗口"的原因。
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let ex = (ex & !(WS_EX_APPWINDOW.0 as isize)) | WS_EX_TOOLWINDOW.0 as isize;
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex);
        let _ = SetWindowPos(
            hwnd,
            HWND_BOTTOM,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_FRAMECHANGED | SWP_SHOWWINDOW | SWP_NOACTIVATE,
        );

        // 4. subclass：拦边框重绘 + SC_MINIMIZE
        install_widget_wndproc(hwnd);

        // 5. explorer 重启恢复：TaskbarCreated 到来时重挂 owner + 置底
        MSG_TASKBAR_CREATED = RegisterWindowMessageW(w!("TaskbarCreated"));

        // 6. 显示 + 先压到底
        let _ = SetWindowPos(
            hwnd,
            HWND_BOTTOM,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_FRAMECHANGED | SWP_SHOWWINDOW | SWP_NOACTIVATE,
        );
    }

    // 7. 摘掉可能已存在的任务栏按钮（explorer 不会因为改样式自动移除）
    remove_taskbar_button(hwnd);

    // 后置校验：绝不把用户留在"程序在跑但桌面上什么都没有"的状态
    if !unsafe { IsWindowVisible(hwnd).as_bool() } {
        let _ = unembed_impl(window);
        let _ = window.show();
        return Err("挂件化后窗口不可见，已回滚为浮空窗口".to_string());
    }

    start_bottom_keeper(hwnd);
    // 挂件作为 shell owned top-level 窗口，DWM SystemBackdrop 全档可用，
    // 材质由前端 refreshWindowVisual() 调 set_window_backdrop 应用。
    log::info!("widget mode: owner=Progman, top-level, sent to bottom");
    Ok(())
}

fn unembed_impl(window: &WebviewWindow) -> Result<(), String> {
    let hwnd = get_hwnd(window)?;

    unsafe {
        restore_wndproc(hwnd);
        WIDGET_HWND.store(0, Ordering::Relaxed);

        // 摘掉 shell ownership，恢复普通窗口
        SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, 0);
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex & !(WS_EX_TOOLWINDOW.0 as isize) | WS_EX_APPWINDOW.0 as isize);

        let _ = SetWindowPos(
            hwnd,
            HWND_TOP,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_FRAMECHANGED | SWP_SHOWWINDOW | SWP_NOACTIVATE,
        );
    }

    log::info!("unembedded: back to floating window");
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
    Ok(())
}
