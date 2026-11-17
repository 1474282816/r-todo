using Avalonia;
using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.Interactivity;
using Avalonia.VisualTree;
using RTools.Desktop.Interop;
using RTools.Desktop.Models;
using RTools.Desktop.Services;
using RTools.Desktop.ViewModels;

namespace RTools.Desktop;

public sealed partial class DesktopWidgetWindow : Window
{
    private IEngineClient? _engine;
    private bool _allowClose;
    private TaskItem? _dragTask;
    private Point _dragStartPoint;
    private bool _draggingTask;
    private int _dragSourceIndex;

    public DesktopWidgetWindow()
    {
        InitializeComponent();
        Closing += OnClosing;
    }

    public DesktopWidgetWindow(IEngineClient engine) : this()
    {
        _engine = engine;
        WidgetDragRegion.PointerPressed += OnTitleBarPointerPressed;
    }

    public event EventHandler? OpenMonthRequested;
    public event EventHandler? OpenSettingsRequested;

    public async Task ShowDesktopWidgetAsync()
    {
        Show();
        await RestoreGeometryAsync();
        TransparencyLevelHint =
            [WindowTransparencyLevel.AcrylicBlur, WindowTransparencyLevel.Blur];
    }

    public void CloseForModeChange()
    {
        _allowClose = true;
        Close();
    }

    public void AllowClose()
    {
        _allowClose = true;
    }

    private void OnTitleBarPointerPressed(object? sender, PointerPressedEventArgs e)
    {
        if (e.GetCurrentPoint(this).Properties.IsLeftButtonPressed)
        {
            BeginMoveDrag(e);
            _ = PersistAfterDragAsync();
            e.Handled = true;
        }
    }

    private async Task RestoreGeometryAsync()
    {
        var engine = _engine
            ?? throw new InvalidOperationException("Engine 尚未初始化");
        var x = await engine.GetSettingAsync("widget_x");
        var y = await engine.GetSettingAsync("widget_y");
        var width = await engine.GetSettingAsync("widget_width");
        var height = await engine.GetSettingAsync("widget_height");
        if (int.TryParse(x, out var px) &&
            int.TryParse(y, out var py) &&
            int.TryParse(width, out var pw) &&
            int.TryParse(height, out var ph))
        {
            Position = new PixelPoint(px, py);
            Width = Math.Max(MinWidth, pw / RenderScaling);
            Height = Math.Max(MinHeight, ph / RenderScaling);
            return;
        }

        // 首次启用时放在主屏右侧中部，避免覆盖桌面左上角图标。
        var screen = Screens.Primary;
        if (screen is null)
        {
            return;
        }
        var area = screen.WorkingArea;
        var initialWidth = Math.Max((int)MinWidth, (int)Width);
        var initialHeight = Math.Max((int)MinHeight, (int)Height);
        Position = new PixelPoint(
            Math.Max(area.X + 24, area.Right - initialWidth - 24),
            Math.Max(area.Y + 24, area.Y + (int)((area.Height - initialHeight) / 2.0)));
    }

    private void OnClosing(object? sender, WindowClosingEventArgs e)
    {
        if (_allowClose)
        {
            return;
        }
        e.Cancel = true;
        Hide();
    }

    protected override async void OnClosed(EventArgs e)
    {
        if (_engine is not null)
        {
            await SaveGeometryAsync();
        }
        base.OnClosed(e);
    }

    private async Task SaveGeometryAsync()
    {
        if (_engine is null)
        {
            return;
        }
        var hwnd = TryGetPlatformHandle()?.Handle ?? IntPtr.Zero;
        if (hwnd != IntPtr.Zero && Win32Native.GetWindowRect(hwnd, out var rect))
        {
            await _engine.SetSettingAsync("widget_x", rect.Left.ToString());
            await _engine.SetSettingAsync("widget_y", rect.Top.ToString());
            await _engine.SetSettingAsync("widget_width", (rect.Right - rect.Left).ToString());
            await _engine.SetSettingAsync("widget_height", (rect.Bottom - rect.Top).ToString());
            return;
        }
        await _engine.SetSettingAsync("widget_x", Position.X.ToString());
        await _engine.SetSettingAsync("widget_y", Position.Y.ToString());
        await _engine.SetSettingAsync("widget_width", ((int)Bounds.Width).ToString());
        await _engine.SetSettingAsync("widget_height", ((int)Bounds.Height).ToString());
    }

    private async Task PersistAfterDragAsync()
    {
        // 等待系统拖拽完全结束，再读取物理窗口矩形并保存。
        while (OperatingSystem.IsWindows() &&
               (Win32Native.GetKeyState(0x01) & 0x8000) != 0)
        {
            await Task.Delay(40);
        }
        await Task.Delay(250);
        await SaveGeometryAsync();
    }


    private void OnQuickAddKeyDown(object? sender, KeyEventArgs e)
    {
        if (e.Key == Key.Enter && DataContext is MainWindowViewModel viewModel)
        {
            viewModel.AddTaskCommand.Execute(null);
            e.Handled = true;
        }
    }

    private void OnOpenMonthClicked(object? sender, RoutedEventArgs e)
        => OpenMonthRequested?.Invoke(this, EventArgs.Empty);

    private void OnOpenSettingsClicked(object? sender, RoutedEventArgs e)
        => OpenSettingsRequested?.Invoke(this, EventArgs.Empty);

    private async void OnSaveEditedTask(object? sender, RoutedEventArgs e)
    {
        if (DataContext is MainWindowViewModel viewModel)
        {
            await viewModel.SaveEditedTaskAsync();
        }
    }

    private void OnTaskRowPointerPressed(object? sender, PointerPressedEventArgs e)
    {
        if (DataContext is not MainWindowViewModel viewModel ||
            sender is not Border border ||
            border.DataContext is not TaskItem task ||
            e.Source is Control source &&
            source.FindAncestorOfType<Button>() is not null)
        {
            return;
        }
        _dragTask = task;
        _dragSourceIndex = viewModel.ActiveTasks.IndexOf(task);
        _dragStartPoint = e.GetPosition(WidgetTaskList);
        _draggingTask = false;
        e.Pointer.Capture(border);
    }

    private void OnTaskRowPointerMoved(object? sender, PointerEventArgs e)
    {
        if (_dragTask is null)
        {
            return;
        }
        var current = e.GetPosition(WidgetTaskList);
        if (!_draggingTask &&
            Math.Abs(current.Y - _dragStartPoint.Y) < 6)
        {
            return;
        }
        _draggingTask = true;
        _dragTask.IsDragging = true;
        e.Pointer.Capture(sender as IInputElement);
    }

    private async void OnTaskRowPointerReleased(object? sender, PointerReleasedEventArgs e)
    {
        var task = _dragTask;
        var moved = _draggingTask;
        _dragTask = null;
        _draggingTask = false;
        e.Pointer.Capture(null);
        if (task is null)
        {
            return;
        }
        task.IsDragging = false;
        if (!moved || DataContext is not MainWindowViewModel viewModel)
        {
            return;
        }
        var y = e.GetPosition(WidgetTaskList).Y;
        var rowHeight = 36.0;
        var targetIndex = Math.Clamp(
            (int)Math.Round(y / rowHeight),
            0,
            viewModel.ActiveTasks.Count - 1);
        if (targetIndex == _dragSourceIndex)
        {
            return;
        }
        // 目标索引是插入位置；向下拖动时移除源行后索引要回退一位。
        var finalIndex = targetIndex > _dragSourceIndex
            ? targetIndex - 1
            : targetIndex;
        await viewModel.ReorderTaskToIndexAsync(task, finalIndex);
    }

    private void OnTaskRowPointerCaptureLost(object? sender, PointerCaptureLostEventArgs e)
    {
        if (_dragTask is not null)
        {
            _dragTask.IsDragging = false;
        }
        _dragTask = null;
        _draggingTask = false;
    }

    private void OnTaskTitlePointerPressed(object? sender, PointerPressedEventArgs e)
    {
        if (sender is not Border border ||
            border.DataContext is not TaskItem task ||
            DataContext is not MainWindowViewModel viewModel ||
            _draggingTask)
        {
            return;
        }
        viewModel.EditTaskCommand.Execute(task);
        e.Handled = true;
    }
}
