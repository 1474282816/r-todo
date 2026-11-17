using Avalonia.Controls;
using Avalonia;
using Avalonia.Input;
using Avalonia.Interactivity;
using Avalonia.Platform.Storage;
using RTools.Desktop.Interop;
using RTools.Desktop.Services;
using RTools.Desktop.ViewModels;
using System.Diagnostics;

namespace RTools.Desktop;

public partial class MainWindow : Window
{
    private DesktopWidgetController? _widget;
    private bool _closing;
    private bool _snappedCollapsed;
    private PixelPoint? _expandedPosition;

    public MainWindow()
    {
        InitializeComponent();
        Closing += OnClosing;
    }

    protected override async void OnOpened(EventArgs e)
    {
        base.OnOpened(e);
        if (DataContext is not MainWindowViewModel viewModel)
        {
            return;
        }
        var engine = viewModel.Engine;
        viewModel.ReminderRaised += OnReminderRaised;
        _widget = new DesktopWidgetController(this, engine);
        try
        {
            var x = await engine.GetSettingAsync("window_x");
            var y = await engine.GetSettingAsync("window_y");
            var width = await engine.GetSettingAsync("window_width");
            var height = await engine.GetSettingAsync("window_height");
            if (int.TryParse(x, out var px) &&
                int.TryParse(y, out var py) &&
                int.TryParse(width, out var pw) &&
                int.TryParse(height, out var ph))
            {
                Position = new PixelPoint(px, py);
                Width = Math.Max(MinWidth, pw);
                Height = Math.Max(MinHeight, ph);
            }
            await ApplyAppearanceAsync(engine);
            var enabled = await engine.GetSettingAsync("desktop_widget") == "true";
            if (enabled)
            {
                await _widget.SetEnabledAsync(true);
            }
        }
        catch (Exception ex)
        {
            viewModel.ErrorMessage = ex.Message;
        }
    }

    public async Task ApplyAppearanceAsync(IEngineClient engine)
    {
        var material = await engine.GetSettingAsync("material") ?? "acrylic";
        TransparencyLevelHint = material switch
        {
            "none" => [WindowTransparencyLevel.None],
            "translucent" => [WindowTransparencyLevel.Transparent],
            "mica" => [WindowTransparencyLevel.Mica],
            "mica_alt" => [WindowTransparencyLevel.Mica, WindowTransparencyLevel.AcrylicBlur],
            _ => [WindowTransparencyLevel.AcrylicBlur, WindowTransparencyLevel.Blur]
        };
        if (material == "mica_alt" && OperatingSystem.IsWindows())
        {
            var hwnd = TryGetPlatformHandle()?.Handle ?? IntPtr.Zero;
            if (hwnd != IntPtr.Zero)
            {
                Win32Native.TryEnableMicaAlt(hwnd);
            }
        }
        // 材质透明由 DWM 和半透明表面共同完成，避免整窗降透明度导致文字发灰。
        Opacity = 1;
    }

    private void OnClosing(object? sender, WindowClosingEventArgs e)
    {
        if (_closing)
        {
            return;
        }
        e.Cancel = true;
        Hide();
    }

    private void OnReminderRaised(object? sender, ReminderDueEventArgs e)
    {
        try
        {
            using var process = Process.Start(new ProcessStartInfo
            {
                FileName = "powershell.exe",
                Arguments =
                    "-NoProfile -WindowStyle Hidden -Command " +
                    "\"Add-Type -AssemblyName System.Windows.Forms; " +
                    $"$n=New-Object System.Windows.Forms.NotifyIcon; " +
                    "$n.Icon=[System.Drawing.SystemIcons]::Information; " +
                    $"$n.Visible=$true; $n.ShowBalloonTip(6000,'rtools 提醒','{EscapePowerShell(e.Title)}',[System.Windows.Forms.ToolTipIcon]::Info); " +
                    "Start-Sleep -Seconds 7; $n.Dispose()\"",
                UseShellExecute = false,
                CreateNoWindow = true
            });
        }
        catch (Exception ex)
        {
            if (DataContext is MainWindowViewModel viewModel)
            {
                viewModel.ErrorMessage = $"提醒显示失败: {ex.Message}";
            }
        }
    }

    private static string EscapePowerShell(string value)
        => value.Replace("'", "''", StringComparison.Ordinal)
            .Replace("\r", " ", StringComparison.Ordinal)
            .Replace("\n", " ", StringComparison.Ordinal);

    public void ExitApplication()
    {
        _closing = true;
        _widget?.Dispose();
        Close();
    }

    protected override async void OnClosed(EventArgs e)
    {
        if (DataContext is MainWindowViewModel viewModel)
        {
            await viewModel.Engine.SetSettingAsync("window_x", Position.X.ToString());
            await viewModel.Engine.SetSettingAsync("window_y", Position.Y.ToString());
            await viewModel.Engine.SetSettingAsync(
                "window_width",
                ((int)Bounds.Width).ToString());
            await viewModel.Engine.SetSettingAsync(
                "window_height",
                ((int)Bounds.Height).ToString());
        }
        base.OnClosed(e);
    }

    private void OnTitleBarPointerPressed(object? sender, PointerPressedEventArgs e)
    {
        if (e.GetCurrentPoint(this).Properties.IsLeftButtonPressed)
        {
            BeginMoveDrag(e);
        }
    }

    private async void OnTitleBarPointerReleased(object? sender, PointerReleasedEventArgs e)
    {
        if (DataContext is not MainWindowViewModel viewModel || !OperatingSystem.IsWindows())
        {
            return;
        }
        var screen = Screens.ScreenFromWindow(this) ?? Screens.Primary;
        if (screen is null)
        {
            return;
        }
        var area = screen.WorkingArea;
        const int threshold = 14;
        var left = Position.X - area.X;
        var right = area.Right - (Position.X + (int)Bounds.Width);
        if (left > threshold && right > threshold)
        {
            return;
        }
        var snapLeft = left <= right;
        Position = new PixelPoint(
            snapLeft ? area.X : area.Right - (int)Bounds.Width,
            Position.Y);
        _expandedPosition = Position;
        if (await viewModel.Engine.GetSettingAsync("collapse_on_snap") != "false")
        {
            CollapseToEdge(snapLeft);
        }
    }

    protected override void OnPointerEntered(PointerEventArgs e)
    {
        base.OnPointerEntered(e);
        if (_snappedCollapsed && _expandedPosition is { } position)
        {
            Position = position;
            _snappedCollapsed = false;
        }
    }

    private void CollapseToEdge(bool left)
    {
        var screen = Screens.ScreenFromWindow(this) ?? Screens.Primary;
        if (screen is null)
        {
            return;
        }
        _expandedPosition = Position;
        var area = screen.WorkingArea;
        Position = new PixelPoint(
            left ? area.X - (int)Bounds.Width + 4 : area.Right - 4,
            Position.Y);
        _snappedCollapsed = true;
    }

    private void OnQuickAddKeyDown(object? sender, KeyEventArgs e)
    {
        if (e.Key == Key.Enter && DataContext is MainWindowViewModel viewModel)
        {
            viewModel.AddTaskCommand.Execute(null);
            e.Handled = true;
        }
    }

    private async void OnImportJson(object? sender, RoutedEventArgs e)
    {
        if (DataContext is not MainWindowViewModel viewModel)
        {
            return;
        }
        var files = await StorageProvider.OpenFilePickerAsync(
            new FilePickerOpenOptions
            {
                Title = "导入待办 JSON",
                AllowMultiple = false,
                FileTypeFilter =
                [
                    new FilePickerFileType("r-todo JSON")
                    {
                        Patterns = ["*.json"]
                    }
                ]
            });
        var file = files.FirstOrDefault();
        if (file is null)
        {
            return;
        }
        var path = file.TryGetLocalPath();
        if (path is null)
        {
            viewModel.ErrorMessage = "无法读取所选文件路径";
            return;
        }
        await viewModel.ImportJsonAsync(path);
    }

    private async void OnDesktopWidgetChanged(object? sender, RoutedEventArgs e)
    {
        if (_widget is null ||
            sender is not CheckBox checkBox ||
            DataContext is not MainWindowViewModel viewModel)
        {
            return;
        }
        try
        {
            await _widget.SetEnabledAsync(checkBox.IsChecked == true);
            await viewModel.Settings.SetDesktopWidgetAsync(checkBox.IsChecked == true);
        }
        catch (Exception ex)
        {
            checkBox.IsChecked = _widget.IsEnabled;
            viewModel.ErrorMessage = ex.Message;
        }
    }

    private async void OnExportJson(object? sender, RoutedEventArgs e)
    {
        if (DataContext is not MainWindowViewModel viewModel)
        {
            return;
        }
        var file = await StorageProvider.SaveFilePickerAsync(
            new FilePickerSaveOptions
            {
                Title = "导出待办 JSON",
                SuggestedFileName = $"rtools-{DateTime.Now:yyyy-MM-dd}.json",
                FileTypeChoices =
                [
                    new FilePickerFileType("rtools JSON")
                    {
                        Patterns = ["*.json"]
                    }
                ]
            });
        var path = file?.TryGetLocalPath();
        if (path is not null)
        {
            await viewModel.ExportJsonAsync(path);
        }
    }

    private async void OnExportIcal(object? sender, RoutedEventArgs e)
    {
        if (DataContext is not MainWindowViewModel viewModel)
        {
            return;
        }
        var file = await StorageProvider.SaveFilePickerAsync(
            new FilePickerSaveOptions
            {
                Title = "导出 iCalendar",
                SuggestedFileName = $"rtools-{DateTime.Now:yyyy-MM-dd}.ics",
                FileTypeChoices =
                [
                    new FilePickerFileType("iCalendar")
                    {
                        Patterns = ["*.ics"]
                    }
                ]
            });
        var path = file?.TryGetLocalPath();
        if (path is not null)
        {
            await viewModel.ExportIcalAsync(path);
        }
    }

    private async void OnExportCsv(object? sender, RoutedEventArgs e)
    {
        if (DataContext is not MainWindowViewModel viewModel)
        {
            return;
        }
        var file = await StorageProvider.SaveFilePickerAsync(
            new FilePickerSaveOptions
            {
                Title = "导出 TickTick CSV",
                SuggestedFileName = $"rtools-{DateTime.Now:yyyy-MM-dd}.csv",
                FileTypeChoices =
                [
                    new FilePickerFileType("CSV")
                    {
                        Patterns = ["*.csv"]
                    }
                ]
            });
        var path = file?.TryGetLocalPath();
        if (path is not null)
        {
            await viewModel.ExportCsvAsync(path);
        }
    }

    private async void OnImportCsv(object? sender, RoutedEventArgs e)
    {
        if (DataContext is not MainWindowViewModel viewModel)
        {
            return;
        }
        var files = await StorageProvider.OpenFilePickerAsync(
            new FilePickerOpenOptions
            {
                Title = "导入 TickTick CSV",
                AllowMultiple = false,
                FileTypeFilter =
                [
                    new FilePickerFileType("CSV")
                    {
                        Patterns = ["*.csv"]
                    }
                ]
            });
        var path = files.FirstOrDefault()?.TryGetLocalPath();
        if (path is not null)
        {
            await viewModel.ImportCsvAsync(path);
        }
    }

    private async void OnSaveEditedTask(object? sender, RoutedEventArgs e)
    {
        if (DataContext is MainWindowViewModel viewModel)
        {
            await viewModel.SaveEditedTaskAsync();
        }
    }

    private async void OnSaveSettings(object? sender, RoutedEventArgs e)
    {
        if (DataContext is not MainWindowViewModel viewModel)
        {
            return;
        }
        await viewModel.Settings.SaveCommand.ExecuteAsync(null);
        await ApplyAppearanceAsync(viewModel.Engine);
    }
}
