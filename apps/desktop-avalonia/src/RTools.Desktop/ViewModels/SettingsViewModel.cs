using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using RTools.Desktop.Models;
using RTools.Desktop.Services;
using System.Collections.ObjectModel;

namespace RTools.Desktop.ViewModels;

public sealed partial class SettingsViewModel : ObservableObject
{
    private readonly IEngineClient _engine;

    public event EventHandler? SyncCompleted;

    public SettingsViewModel(IEngineClient engine)
    {
        _engine = engine;
    }

    [ObservableProperty]
    private bool _isDesktopWidget;

    [ObservableProperty]
    private string _material = "acrylic";

    [ObservableProperty]
    private double _opacity = 0.82;

    [ObservableProperty]
    private bool _autostart;

    [ObservableProperty]
    private bool _collapseOnSnap = true;

    [ObservableProperty]
    private int _defaultReminderOffset = 15;

    [ObservableProperty]
    private string _syncServerUrl = "https://todo.renrunqi.com";

    [ObservableProperty]
    private string _pairingCode = "";

    [ObservableProperty]
    private string _deviceName = Environment.MachineName;

    [ObservableProperty]
    private string _syncState = "未配对";

    [ObservableProperty]
    private string _mcpToken = "";

    [ObservableProperty]
    private bool _mcpEnabled;

    [ObservableProperty]
    private bool _mcpRunning;

    [ObservableProperty]
    private string _mcpTemplates = "";

    [ObservableProperty]
    private bool _transferRunning;

    [ObservableProperty]
    private string _version = "0.2.0";

    [ObservableProperty]
    private string? _statusMessage;

    [ObservableProperty]
    private string? _errorMessage;

    [ObservableProperty]
    private ObservableCollection<DeviceItem> _devices = [];

    public IReadOnlyList<string> Materials { get; } =
        ["半透明", "亚克力", "Mica", "Mica Alt", "无"];

    [ObservableProperty]
    private string? _selectedMaterial;

    public IReadOnlyList<int> ReminderOffsets { get; } = [0, 5, 10, 15, 30, 60, 120];

    public async Task LoadAsync(CancellationToken cancellationToken = default)
    {
        try
        {
            IsDesktopWidget =
                await _engine.GetSettingAsync("desktop_widget", cancellationToken) == "true";
            Material =
                await _engine.GetSettingAsync("material", cancellationToken) ?? "acrylic";
            SelectedMaterial = Material switch
            {
                "translucent" => "半透明",
                "mica" => "Mica",
                "mica_alt" => "Mica Alt",
                "none" => "无",
                _ => "亚克力"
            };
            if (double.TryParse(
                    await _engine.GetSettingAsync("opacity", cancellationToken),
                    out var opacity))
            {
                Opacity = Math.Clamp(opacity, 0.2, 1.0);
            }
            if (int.TryParse(
                    await _engine.GetSettingAsync("default_reminder_offset", cancellationToken),
                    out var offset))
            {
                DefaultReminderOffset = offset;
            }
            CollapseOnSnap =
                await _engine.GetSettingAsync("collapse_on_snap", cancellationToken) != "false";
            Autostart = await _engine.GetAutostartAsync(cancellationToken);
            var sync = await _engine.GetSyncStatusAsync(cancellationToken);
            ApplySync(sync);
            var mcp = await _engine.GetMcpStatusAsync(cancellationToken);
            ApplyMcp(mcp);
            var templates = await _engine.GetMcpTemplatesAsync(cancellationToken);
            McpTemplates = string.Join(
                Environment.NewLine,
                $"Claude Code: {templates.ClaudeCode}",
                $"Claude Desktop: {templates.ClaudeDesktop}",
                $"Codex: {templates.Codex}",
                $"OpenCode: {templates.OpenCode}");
            var transfer = await _engine.GetTransferStatusAsync(cancellationToken);
            TransferRunning = transfer.Running;
            Devices = new ObservableCollection<DeviceItem>(
                await _engine.ListDevicesAsync(cancellationToken));
        }
        catch (Exception ex)
        {
            ErrorMessage = ex.Message;
        }
    }

    [RelayCommand]
    private async Task SaveAsync()
    {
        try
        {
            ErrorMessage = null;
            await _engine.SetSettingAsync("desktop_widget", IsDesktopWidget ? "true" : "false");
            await _engine.SetSettingAsync(
                "material",
                SelectedMaterial switch
                {
                    "半透明" => "translucent",
                    "Mica" => "mica",
                    "Mica Alt" => "mica_alt",
                    "无" => "none",
                    _ => "acrylic"
                });
            await _engine.SetSettingAsync(
                "opacity",
                Opacity.ToString(System.Globalization.CultureInfo.InvariantCulture));
            await _engine.SetSettingAsync(
                "collapse_on_snap",
                CollapseOnSnap ? "true" : "false");
            await _engine.SetSettingAsync(
                "default_reminder_offset",
                DefaultReminderOffset.ToString(System.Globalization.CultureInfo.InvariantCulture));
            await _engine.SetAutostartAsync(Autostart);
            StatusMessage = "设置已保存";
        }
        catch (Exception ex)
        {
            ErrorMessage = ex.Message;
        }
    }

    public async Task SetDesktopWidgetAsync(bool enabled)
    {
        IsDesktopWidget = enabled;
        await _engine.SetSettingAsync("desktop_widget", enabled ? "true" : "false");
    }

    [RelayCommand]
    private async Task PairAsync()
    {
        try
        {
            ErrorMessage = null;
            var status = await _engine.PairAsync(SyncServerUrl, PairingCode, DeviceName);
            ApplySync(status);
            Devices = new ObservableCollection<DeviceItem>(
                await _engine.ListDevicesAsync());
            StatusMessage = "配对成功";
            PairingCode = "";
            SyncCompleted?.Invoke(this, EventArgs.Empty);
        }
        catch (Exception ex)
        {
            ErrorMessage = ex.Message;
        }
    }

    [RelayCommand]
    private async Task CreatePairingCodeAsync()
    {
        try
        {
            var code = await _engine.CreatePairingCodeAsync();
            PairingCode = code.Code;
            StatusMessage = $"新设备接入码：{code.Code}（10 分钟内有效）";
        }
        catch (Exception ex)
        {
            ErrorMessage = ex.Message;
        }
    }

    [RelayCommand]
    private async Task RevokeDeviceAsync(DeviceItem? device)
    {
        if (device is null)
        {
            return;
        }
        try
        {
            await _engine.RevokeDeviceAsync(device.Id);
            Devices = new ObservableCollection<DeviceItem>(
                await _engine.ListDevicesAsync());
            StatusMessage = $"已撤销 {device.Name}";
        }
        catch (Exception ex)
        {
            ErrorMessage = ex.Message;
        }
    }

    [RelayCommand]
    private async Task SyncNowAsync()
    {
        try
        {
            ErrorMessage = null;
            ApplySync(await _engine.SyncNowAsync());
            StatusMessage = "同步完成";
            SyncCompleted?.Invoke(this, EventArgs.Empty);
        }
        catch (Exception ex)
        {
            ErrorMessage = ex.Message;
        }
    }

    [RelayCommand]
    private async Task LogoutSyncAsync()
    {
        try
        {
            ErrorMessage = null;
            await _engine.LogoutSyncAsync();
            SyncState = "未配对";
            StatusMessage = "已退出同步";
        }
        catch (Exception ex)
        {
            ErrorMessage = ex.Message;
        }
    }

    [RelayCommand]
    private async Task ToggleMcpAsync()
    {
        try
        {
            ErrorMessage = null;
            ApplyMcp(await _engine.SetMcpEnabledAsync(!McpEnabled));
        }
        catch (Exception ex)
        {
            ErrorMessage = ex.Message;
        }
    }

    [RelayCommand]
    private async Task RotateMcpTokenAsync()
    {
        try
        {
            ErrorMessage = null;
            ApplyMcp(await _engine.RotateMcpTokenAsync());
            var templates = await _engine.GetMcpTemplatesAsync();
            McpTemplates = string.Join(
                Environment.NewLine,
                $"Claude Code: {templates.ClaudeCode}",
                $"Claude Desktop: {templates.ClaudeDesktop}",
                $"Codex: {templates.Codex}",
                $"OpenCode: {templates.OpenCode}");
            StatusMessage = "MCP token 已重置";
        }
        catch (Exception ex)
        {
            ErrorMessage = ex.Message;
        }
    }

    [RelayCommand]
    private async Task ToggleTransferAsync()
    {
        try
        {
            ErrorMessage = null;
            var status = TransferRunning
                ? await _engine.StopTransferAsync()
                : await _engine.StartTransferAsync();
            TransferRunning = status.Running;
        }
        catch (Exception ex)
        {
            ErrorMessage = ex.Message;
        }
    }

    private void ApplySync(SyncStatus sync)
    {
        SyncState = sync.Paired
            ? sync.Connected
                ? "已连接"
                : "已配对，当前离线"
            : "未配对";
        SyncServerUrl = sync.ServerUrl ?? SyncServerUrl;
        DeviceName = sync.DeviceName ?? DeviceName;
    }

    private void ApplyMcp(McpStatus mcp)
    {
        McpEnabled = mcp.Enabled;
        McpRunning = mcp.Running;
        McpToken = mcp.Token ?? "";
        if (mcp.Error is not null)
        {
            ErrorMessage = mcp.Error;
        }
    }
}

