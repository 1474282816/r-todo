using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using RTools.Desktop.Services;
using System.Collections.ObjectModel;
using System.Text.Json;
using Avalonia.Controls;
using Avalonia.Platform.Storage;

namespace RTools.Desktop.ViewModels;

public sealed partial class TransferViewModel : ObservableObject
{
    private readonly IEngineClient _engine;
    private readonly Dictionary<string, TransferSessionItem> _sessions = [];

    public TransferViewModel(IEngineClient engine)
    {
        _engine = engine;
        _engine.TransferEventReceived += OnTransferEvent;
        _engine.ChatChanged += OnChatChanged;
    }

    public ObservableCollection<TransferDevice> Devices { get; } = [];
    public ObservableCollection<TransferSessionItem> Sessions { get; } = [];
    public ObservableCollection<string> SelectedFiles { get; } = [];
    public ObservableCollection<ChatConversation> Conversations { get; } = [];
    public ObservableCollection<ChatMessageItem> Messages { get; } = [];

    [ObservableProperty]
    private bool _isRunning;

    [ObservableProperty]
    private string _manualIp = "";

    [ObservableProperty]
    private string _chatText = "";

    [ObservableProperty]
    private string _alias = "rtools";

    [ObservableProperty]
    private int _port = 53317;

    [ObservableProperty]
    private string _receiveDir = "";

    [ObservableProperty]
    private string _pin = "";

    [ObservableProperty]
    private bool _autoAcceptFavorites;

    [ObservableProperty]
    private string _fingerprint = "";

    [ObservableProperty]
    private bool _isDiscovering;

    [ObservableProperty]
    private bool _isBusy;

    [ObservableProperty]
    private PendingIncoming? _pendingIncoming;

    public bool HasPendingIncoming => PendingIncoming is not null;

    partial void OnPendingIncomingChanged(PendingIncoming? value)
        => OnPropertyChanged(nameof(HasPendingIncoming));

    [ObservableProperty]
    private ChatConversation? _selectedConversation;

    [ObservableProperty]
    private string _statusMessage = "";

    [ObservableProperty]
    private string? _errorMessage;

    public async Task LoadAsync()
    {
        try
        {
            var status = await _engine.GetTransferStatusAsync();
            ApplyStatus(status);
            await RefreshDevicesAsync();
            await LoadTrustedPeersAsync();
            await RefreshConversationsAsync();
            var pending = await _engine.GetPendingIncomingAsync();
            PendingIncoming = pending.FirstOrDefault();
        }
        catch (Exception ex)
        {
            ErrorMessage = ex.Message;
        }
    }

    [RelayCommand]
    private async Task ToggleAsync()
    {
        try
        {
            var status = IsRunning
                ? await _engine.StopTransferAsync()
                : await _engine.StartTransferAsync();
            ApplyStatus(status);
            if (status.Running)
            {
                await RefreshDevicesAsync();
                await LoadTrustedPeersAsync();
            }
        }
        catch (Exception ex)
        {
            ErrorMessage = ex.Message;
        }
    }

    [RelayCommand]
    private async Task RefreshAsync()
    {
        IsDiscovering = true;
        try
        {
            await RefreshDevicesAsync();
        }
        finally
        {
            IsDiscovering = false;
        }
    }

    [RelayCommand]
    private async Task AddManualIpAsync()
    {
        var text = ManualIp.Trim();
        if (text.Length == 0)
        {
            return;
        }
        var split = text.Split(':', 2);
        var port = split.Length > 1 && int.TryParse(split[1], out var parsed)
            ? parsed
            : Port;
        IsBusy = true;
        try
        {
            var device = await _engine.AddManualTransferIpAsync(split[0], port);
            if (device is null)
            {
                ErrorMessage = "该地址没有响应";
            }
            else
            {
                MergeDevice(device);
                ManualIp = "";
            }
        }
        catch (Exception ex)
        {
            ErrorMessage = ex.Message;
        }
        finally
        {
            IsBusy = false;
        }
    }

    [RelayCommand]
    private async Task PickFilesAsync()
    {
        var files = await TopLevel.GetTopLevel(
            Avalonia.Application.Current!.ApplicationLifetime
                is Avalonia.Controls.ApplicationLifetimes.IClassicDesktopStyleApplicationLifetime desktop
                ? desktop.MainWindow!
                : throw new InvalidOperationException("窗口尚未就绪"))!.StorageProvider.OpenFilePickerAsync(
            new FilePickerOpenOptions
            {
                AllowMultiple = true,
                Title = "选择要发送的文件"
            });
        foreach (var file in files)
        {
            var path = file.TryGetLocalPath();
            if (path is not null && !SelectedFiles.Contains(path))
            {
                SelectedFiles.Add(path);
            }
        }
    }

    [RelayCommand]
    private void RemoveFile(string? path)
    {
        if (path is not null)
        {
            SelectedFiles.Remove(path);
        }
    }

    [RelayCommand]
    private async Task SendToAsync(TransferDevice? device)
    {
        if (device is null || SelectedFiles.Count == 0)
        {
            return;
        }
        IsBusy = true;
        try
        {
            await _engine.SendTransferFilesAsync(
                SelectedFiles.ToList(),
                device.Fingerprint,
                string.IsNullOrWhiteSpace(Pin) ? null : Pin);
            AddSession(new TransferSessionItem
            {
                Id = Guid.NewGuid().ToString("N"),
                Direction = "send",
                Peer = device.Alias,
                Status = "传输中"
            });
            SelectedFiles.Clear();
            StatusMessage = $"正在发送给 {device.Alias}";
        }
        catch (Exception ex)
        {
            ErrorMessage = ex.Message;
        }
        finally
        {
            IsBusy = false;
        }
    }

    [RelayCommand]
    private async Task AcceptIncomingAsync()
    {
        if (PendingIncoming is null)
        {
            return;
        }
        IsBusy = true;
        try
        {
            await _engine.RespondIncomingAsync(
                PendingIncoming.SessionId,
                PendingIncoming.Files.Select(file => file.Id).ToList());
            await _engine.TrustChatPeerAsync(PendingIncoming.Device.Fingerprint);
            _trustedFingerprints.Add(PendingIncoming.Device.Fingerprint);
            await _engine.SendTrustMessageAsync(
                PendingIncoming.Device.Fingerprint,
                Alias);
            StatusMessage = $"正在接收 {PendingIncoming.Files.Count} 个文件";
            PendingIncoming = null;
        }
        catch (Exception ex)
        {
            ErrorMessage = ex.Message;
        }
        finally
        {
            IsBusy = false;
        }
    }

    [RelayCommand]
    private async Task RejectIncomingAsync()
    {
        if (PendingIncoming is null)
        {
            return;
        }
        try
        {
            await _engine.RejectIncomingAsync(PendingIncoming.SessionId);
            PendingIncoming = null;
        }
        catch (Exception ex)
        {
            ErrorMessage = ex.Message;
        }
    }

    [RelayCommand]
    private async Task CancelSessionAsync(TransferSessionItem? session)
    {
        if (session is null)
        {
            return;
        }
        try
        {
            await _engine.CancelTransferSessionAsync(session.Id);
            session.Status = "已取消";
        }
        catch (Exception ex)
        {
            ErrorMessage = ex.Message;
        }
    }

    [RelayCommand]
    private async Task SaveSettingsAsync()
    {
        try
        {
            await _engine.UpdateTransferSettingsAsync(
                Alias,
                Port,
                ReceiveDir,
                Pin,
                AutoAcceptFavorites);
            StatusMessage = "互传设置已保存，重启服务后生效";
        }
        catch (Exception ex)
        {
            ErrorMessage = ex.Message;
        }
    }

    [RelayCommand]
    private async Task OpenConversationAsync(ChatConversation? conversation)
    {
        if (conversation is null)
        {
            return;
        }
        SelectedConversation = conversation;
        await _engine.MarkConversationReadAsync(conversation.Id);
        await RefreshMessagesAsync();
        await RefreshConversationsAsync();
    }

    [RelayCommand]
    private async Task SendChatAsync()
    {
        if (string.IsNullOrWhiteSpace(ChatText))
        {
            return;
        }
        var conversation = SelectedConversation;
        if (conversation is null)
        {
            ErrorMessage = "请先选择会话";
            return;
        }
        var self = Fingerprint;
        var message = new ChatMessageItem
        {
            Id = Guid.NewGuid().ToString("N"),
            ConversationId = conversation.Id,
            Sender = self,
            SenderAlias = Alias,
            Kind = "text",
            Body = ChatText.Trim(),
            CreatedAt = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds(),
            Status = "sent",
            Direction = "send"
        };
        try
        {
            await _engine.SendChatTextAsync(conversation.Id, message);
            ChatText = "";
            await RefreshMessagesAsync();
        }
        catch (Exception ex)
        {
            ErrorMessage = ex.Message;
        }
    }

    [RelayCommand]
    private async Task ClearConversationAsync()
    {
        if (SelectedConversation is null)
        {
            return;
        }
        await _engine.ClearConversationAsync(SelectedConversation.Id);
        SelectedConversation = null;
        Messages.Clear();
        await RefreshConversationsAsync();
    }

    [RelayCommand]
    private async Task RevokeTrustAsync()
    {
        if (SelectedConversation is null)
        {
            return;
        }
        try
        {
            await _engine.RevokeChatTrustAsync(SelectedConversation.Id);
            StatusMessage = $"已撤销 {SelectedConversation.Alias} 的免确认信任";
        }
        catch (Exception ex)
        {
            ErrorMessage = ex.Message;
        }
    }

    [RelayCommand]
    private async Task SendConversationFilesAsync()
    {
        if (SelectedConversation is null)
        {
            ErrorMessage = "请先选择会话";
            return;
        }
        var files = await Avalonia.Controls.TopLevel.GetTopLevel(
            Avalonia.Application.Current!.ApplicationLifetime
                is Avalonia.Controls.ApplicationLifetimes.IClassicDesktopStyleApplicationLifetime desktop
                ? desktop.MainWindow!
                : throw new InvalidOperationException("窗口尚未就绪"))!.StorageProvider.OpenFilePickerAsync(
            new FilePickerOpenOptions
            {
                AllowMultiple = true,
                Title = "选择要发送到会话的文件"
            });
        var paths = files
            .Select(file => file.TryGetLocalPath())
            .Where(path => path is not null)
            .Cast<string>()
            .ToList();
        if (paths.Count == 0)
        {
            return;
        }
        try
        {
            await _engine.SendChatFilesAsync(SelectedConversation.Id, paths);
            StatusMessage = $"已向 {SelectedConversation.Alias} 发送 {paths.Count} 个文件";
        }
        catch (Exception ex)
        {
            ErrorMessage = ex.Message;
        }
    }

    [RelayCommand]
    private async Task DeleteMessageFileAsync(ChatMessageItem? message)
    {
        if (message?.FilePath is null)
        {
            return;
        }
        try
        {
            await _engine.DeleteChatFileAsync(message.FilePath);
            StatusMessage = "已删除接收文件";
            await RefreshMessagesAsync();
        }
        catch (Exception ex)
        {
            ErrorMessage = ex.Message;
        }
    }

    public async Task RefreshDevicesAsync()
    {
        var devices = await _engine.DiscoverTransferDevicesAsync();
        Devices.Clear();
        foreach (var device in devices)
        {
            Devices.Add(device);
        }
    }

    public async Task RefreshConversationsAsync()
    {
        var conversations = await _engine.ListConversationsAsync();
        Conversations.Clear();
        foreach (var conversation in conversations)
        {
            Conversations.Add(conversation);
        }
    }

    public async Task RefreshMessagesAsync()
    {
        if (SelectedConversation is null)
        {
            Messages.Clear();
            return;
        }
        var messages = await _engine.ListMessagesAsync(SelectedConversation.Id);
        Messages.Clear();
        foreach (var message in messages)
        {
            Messages.Add(message);
        }
    }

    private void ApplyStatus(TransferStatus status)
    {
        IsRunning = status.Running;
        Alias = status.Alias;
        Port = status.Port;
        ReceiveDir = status.ReceiveDir;
        Fingerprint = status.Fingerprint;
        AutoAcceptFavorites = status.AutoAcceptFavorites;
        ErrorMessage = status.Error;
    }

    private void MergeDevice(TransferDevice device)
    {
        var existing = Devices.FirstOrDefault(item =>
            item.Fingerprint.Equals(device.Fingerprint, StringComparison.OrdinalIgnoreCase));
        if (existing is not null)
        {
            Devices.Remove(existing);
        }
        Devices.Add(device);
    }

    private void AddSession(TransferSessionItem session)
    {
        _sessions[session.Id] = session;
        Sessions.Insert(0, session);
        while (Sessions.Count > 6)
        {
            Sessions.RemoveAt(Sessions.Count - 1);
        }
    }

    private void OnTransferEvent(object? sender, TransferEventEventArgs e)
    {
        var payload = e.Payload;
        var type = payload.TryGetProperty("type", out var typeValue)
            ? typeValue.GetString()
            : null;
        if (type == "devices" &&
            payload.TryGetProperty("devices", out var devices))
        {
            Devices.Clear();
            foreach (var item in devices.EnumerateArray())
            {
                var device = item.Deserialize<TransferDevice>(
                    new JsonSerializerOptions
                    {
                        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower
                    });
                if (device is not null)
                {
                    Devices.Add(device);
                }
            }
        }
        else if (type == "incoming")
        {
            var incoming = payload.Deserialize<PendingIncoming>(
                new JsonSerializerOptions
                {
                    PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower
                });
            if (incoming is not null && AutoAcceptFavorites)
            {
                _ = HandleIncomingWithTrustAsync(incoming);
            }
            else
            {
                PendingIncoming = incoming;
            }
        }
        else if (type == "progress")
        {
            var id = payload.GetProperty("session_id").GetString() ?? "";
            var session = Sessions.FirstOrDefault(item => item.Id == id);
            if (session is null)
            {
                session = new TransferSessionItem
                {
                    Id = id,
                    Direction = payload.GetProperty("direction").GetString() ?? "send",
                    Peer = "传输"
                };
                AddSession(session);
            }
            session.Progress = payload.GetProperty("percent").GetDouble();
            session.Status = $"{session.Progress:0}%";
        }
        else if (type == "session_ended")
        {
            var id = payload.GetProperty("session_id").GetString() ?? "";
            var session = Sessions.FirstOrDefault(item => item.Id == id);
            if (session is not null)
            {
                session.Status = payload.GetProperty("reason").GetString() switch
                {
                    "completed" => "已完成",
                    "cancelled" => "已取消",
                    "rejected" => "已拒绝",
                    _ => "失败"
                };
            }
        }
        else if (type == "error")
        {
            ErrorMessage = payload.GetProperty("message").GetString();
        }
    }

    private readonly HashSet<string> _trustedFingerprints =
        new(StringComparer.OrdinalIgnoreCase);

    private async Task HandleIncomingWithTrustAsync(PendingIncoming incoming)
    {
        try
        {
            if (await _engine.IsChatTrustedAsync(incoming.Device.Fingerprint))
            {
                _trustedFingerprints.Add(incoming.Device.Fingerprint);
                await AcceptTrustedIncomingAsync(incoming);
            }
            else
            {
                PendingIncoming = incoming;
            }
        }
        catch (Exception ex)
        {
            PendingIncoming = incoming;
            ErrorMessage = ex.Message;
        }
    }

    private async Task AcceptTrustedIncomingAsync(PendingIncoming incoming)
    {
        try
        {
            await _engine.RespondIncomingAsync(
                incoming.SessionId,
                incoming.Files.Select(file => file.Id).ToList());
            StatusMessage = $"已自动接收 {incoming.Device.Alias} 的文件";
        }
        catch (Exception ex)
        {
            PendingIncoming = incoming;
            ErrorMessage = ex.Message;
        }
    }

    public async Task LoadTrustedPeersAsync()
    {
        _trustedFingerprints.Clear();
        foreach (var device in Devices)
        {
            if (await _engine.IsChatTrustedAsync(device.Fingerprint))
            {
                _trustedFingerprints.Add(device.Fingerprint);
            }
        }
    }

    private void OnChatChanged(object? sender, EventArgs e)
    {
        _ = RefreshConversationsAsync();
        if (SelectedConversation is not null)
        {
            _ = RefreshMessagesAsync();
        }
    }
}

public sealed partial class TransferSessionItem : ObservableObject
{
    public required string Id { get; init; }
    public required string Direction { get; init; }
    public required string Peer { get; init; }

    [ObservableProperty]
    private double _progress;

    [ObservableProperty]
    private string _status = "等待中";
}

