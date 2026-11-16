using Avalonia.Controls;
using Avalonia.Threading;
using RTools.Desktop.Services;
using System.Runtime.InteropServices;

namespace RTools.Desktop.Interop;

public sealed class DesktopWidgetController : IDisposable
{
    private readonly Window _window;
    private readonly IEngineClient _engine;
    private Win32Properties.CustomWndProcHookCallback? _hook;
    private DispatcherTimer? _keeper;
    private bool _enabled;
    private bool _disposed;

    public DesktopWidgetController(Window window, IEngineClient engine)
    {
        _window = window;
        _engine = engine;
    }

    public bool IsEnabled => _enabled;

    public async Task SetEnabledAsync(bool enabled)
    {
        if (_enabled == enabled)
        {
            return;
        }
        if (enabled)
        {
            Enable();
        }
        else
        {
            Disable();
        }
        await _engine.SetSettingAsync(
            "desktop_widget",
            enabled ? "true" : "false");
    }

    private void Enable()
    {
        if (!OperatingSystem.IsWindows())
        {
            throw new PlatformNotSupportedException("桌面挂件仅支持 Windows");
        }
        var hwnd = _window.TryGetPlatformHandle()?.Handle
            ?? throw new InvalidOperationException("无法获取窗口句柄");
        var progman = Win32Native.FindWindow("Progman", null);
        if (progman == IntPtr.Zero)
        {
            throw new InvalidOperationException("找不到 Progman 窗口");
        }
        Win32Native.SetWindowLongPtr(hwnd, -8, progman);
        var exStyle = Win32Native.GetWindowLongPtr(hwnd, Win32Native.GwlExStyle).ToInt64();
        exStyle &= ~Win32Native.WsExAppWindow;
        exStyle |= Win32Native.WsExToolWindow;
        Win32Native.SetWindowLongPtr(
            hwnd,
            Win32Native.GwlExStyle,
            new IntPtr(exStyle));
        Win32Native.SetWindowPos(
            hwnd,
            Win32Native.HwndBottom,
            0,
            0,
            0,
            0,
            Win32Native.SwpNoMove |
            Win32Native.SwpNoSize |
            Win32Native.SwpNoActivate |
            Win32Native.SwpFrameChanged);
        _hook = (IntPtr h, uint msg, IntPtr wParam, IntPtr lParam, ref bool handled) =>
        {
            const uint WmSysCommand = 0x0112;
            const uint ScMinimize = 0xF020;
            const uint WmNcActivate = 0x0086;
            const uint WmNcPaint = 0x0085;
            const uint WmSettingChange = 0x001A;
            if (msg == Win32Native.TaskbarCreatedMessage)
            {
                var currentProgman = Win32Native.FindWindow("Progman", null);
                if (currentProgman != IntPtr.Zero)
                {
                    Win32Native.SetWindowLongPtr(h, -8, currentProgman);
                    Win32Native.SetWindowPos(
                        h,
                        Win32Native.HwndBottom,
                        0,
                        0,
                        0,
                        0,
                        Win32Native.SwpNoMove |
                        Win32Native.SwpNoSize |
                        Win32Native.SwpNoActivate |
                        Win32Native.SwpFrameChanged);
                }
            }
            if (msg == WmSettingChange)
            {
                // 壁纸或系统主题变化后，保持挂件在桌面层。
                Win32Native.SetWindowPos(
                    h,
                    Win32Native.HwndBottom,
                    0,
                    0,
                    0,
                    0,
                    Win32Native.SwpNoMove |
                    Win32Native.SwpNoSize |
                    Win32Native.SwpNoActivate);
            }
            if (msg == WmSysCommand &&
                ((uint)wParam.ToInt64() & 0xFFF0) == ScMinimize)
            {
                handled = true;
                return IntPtr.Zero;
            }
            if (msg == WmNcActivate)
            {
                handled = true;
                return new IntPtr(1);
            }
            if (msg == WmNcPaint)
            {
                handled = true;
                return IntPtr.Zero;
            }
            return IntPtr.Zero;
        };
        Win32Properties.AddWndProcHookCallback(_window, _hook);
        _keeper = new DispatcherTimer(TimeSpan.FromSeconds(1), DispatcherPriority.Background, (_, _) =>
        {
            if (!_enabled || _window.IsActive)
            {
                return;
            }
            var current = _window.TryGetPlatformHandle()?.Handle ?? IntPtr.Zero;
            if (current == IntPtr.Zero)
            {
                return;
            }
            if (!Win32Native.IsWindowVisible(current))
            {
                // 挂件被外部窗口管理器隐藏时自动回滚，避免程序在跑但桌面无内容。
                _keeper?.Stop();
                if (_hook is not null)
                {
                    Win32Properties.RemoveWndProcHookCallback(_window, _hook);
                    _hook = null;
                }
                var hwnd = _window.TryGetPlatformHandle()?.Handle ?? IntPtr.Zero;
                if (hwnd != IntPtr.Zero)
                {
                    Win32Native.SetWindowLongPtr(hwnd, -8, IntPtr.Zero);
                    var exStyle = Win32Native.GetWindowLongPtr(
                        hwnd,
                        Win32Native.GwlExStyle).ToInt64();
                    exStyle &= ~Win32Native.WsExToolWindow;
                    exStyle |= Win32Native.WsExAppWindow;
                    Win32Native.SetWindowLongPtr(
                        hwnd,
                        Win32Native.GwlExStyle,
                        new IntPtr(exStyle));
                }
                _enabled = false;
                _window.Show();
                return;
            }
            if (Win32Native.IsWindowVisible(current))
            {
                Win32Native.SetWindowPos(
                    current,
                    Win32Native.HwndBottom,
                    0,
                    0,
                    0,
                    0,
                    Win32Native.SwpNoMove |
                    Win32Native.SwpNoSize |
                    Win32Native.SwpNoActivate);
            }
        });
        _keeper.Start();
        _enabled = true;
    }

    private void Disable()
    {
        _keeper?.Stop();
        _keeper = null;
        if (_hook is not null)
        {
            Win32Properties.RemoveWndProcHookCallback(_window, _hook);
            _hook = null;
        }
        if (OperatingSystem.IsWindows())
        {
            var hwnd = _window.TryGetPlatformHandle()?.Handle ?? IntPtr.Zero;
            if (hwnd != IntPtr.Zero)
            {
                Win32Native.SetWindowLongPtr(hwnd, -8, IntPtr.Zero);
                var exStyle = Win32Native.GetWindowLongPtr(hwnd, Win32Native.GwlExStyle).ToInt64();
                exStyle &= ~Win32Native.WsExToolWindow;
                exStyle |= Win32Native.WsExAppWindow;
                Win32Native.SetWindowLongPtr(
                    hwnd,
                    Win32Native.GwlExStyle,
                    new IntPtr(exStyle));
            }
        }
        _enabled = false;
    }

    public void Dispose()
    {
        if (_disposed)
        {
            return;
        }
        _disposed = true;
        if (_enabled)
        {
            Disable();
        }
    }
}

