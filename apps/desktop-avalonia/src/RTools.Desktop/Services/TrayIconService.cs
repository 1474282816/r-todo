using Avalonia.Controls;
using RTools.Desktop.ViewModels;

namespace RTools.Desktop.Services;

public static class TrayIconService
{
    private static TrayIcon? _tray;

    public static void Install(Window window, MainWindowViewModel viewModel)
    {
        if (_tray is not null)
        {
            return;
        }
        var show = new NativeMenuItem("显示主窗口");
        show.Click += (_, _) => ShowMainWindow(window);
        var hide = new NativeMenuItem("隐藏窗口");
        hide.Click += (_, _) => window.Hide();
        var transfer = new NativeMenuItem("打开互传");
        transfer.Click += async (_, _) => await viewModel.OpenTransferCommand.ExecuteAsync(null);
        var quit = new NativeMenuItem("退出");
        quit.Click += (_, _) =>
        {
            if (window is MainWindow mainWindow)
            {
                mainWindow.ExitApplication();
            }
            else
            {
                Environment.Exit(0);
            }
        };
        var menu = new NativeMenu
        {
            Items =
            {
                show,
                hide,
                new NativeMenuItemSeparator(),
                transfer,
                new NativeMenuItemSeparator(),
                quit
            }
        };
        _tray = new TrayIcon
        {
            ToolTipText = "rtools",
            Menu = menu,
            Icon = LoadIcon(),
            IsVisible = true
        };
        _tray.Clicked += (_, _) => ShowMainWindow(window);
    }

    private static void ShowMainWindow(Window window)
    {
        if (window is MainWindow mainWindow)
        {
            mainWindow.ShowFromTray();
        }
        else
        {
            window.Show();
            window.WindowState = WindowState.Normal;
            window.Activate();
        }
    }

    private static WindowIcon LoadIcon()
    {
        var candidates = new[]
        {
            Path.Combine(AppContext.BaseDirectory, "Assets", "rtools.ico"),
            Path.Combine(AppContext.BaseDirectory, "rtools.ico")
        };
        var path = candidates.FirstOrDefault(File.Exists);
        try
        {
            if (path is not null)
            {
                using var stream = File.OpenRead(path);
                return new WindowIcon(stream);
            }
        }
        catch
        {
        }
        return CreateFallbackIcon();
    }

    private static WindowIcon CreateFallbackIcon()
    {
        try
        {
            using var stream = new MemoryStream(
                Convert.FromBase64String(
                    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9Y9ZQmcAAAAASUVORK5CYII="));
            return new WindowIcon(stream);
        }
        catch
        {
        using var icon = Avalonia.Platform.AssetLoader.Open(
                new Uri("avares://RTools.Desktop/Assets/rtools.ico"));
        return new WindowIcon(icon);
        }
    }
}

