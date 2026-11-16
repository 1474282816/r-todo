using RTools.Desktop.Models;

namespace RTools.Desktop.Services;

public interface IEngineClient : IAsyncDisposable
{
    event EventHandler<TaskChangedEventArgs>? TasksChanged;
    event EventHandler<ReminderDueEventArgs>? ReminderDue;
    event EventHandler<TransferEventEventArgs>? TransferEventReceived;
    event EventHandler? ChatChanged;

    bool IsConnected { get; }

    Task StartAsync(CancellationToken cancellationToken = default);
    Task<IReadOnlyList<TaskItem>> ListTasksAsync(CancellationToken cancellationToken = default);
    Task<TaskItem> CreateTaskAsync(
        string title,
        TaskPriority priority,
        DateTimeOffset? remindAt,
        int remindOffsetMinutes,
        string? repeatRule,
        CancellationToken cancellationToken = default);
    Task<TaskItem> UpdateTaskAsync(TaskItem task, CancellationToken cancellationToken = default);
    Task<TaskItem> CompleteTaskAsync(
        string id,
        bool completed,
        CancellationToken cancellationToken = default);
    Task DeleteTaskAsync(string id, CancellationToken cancellationToken = default);
    Task ReorderTasksAsync(
        IReadOnlyList<string> orderedIds,
        CancellationToken cancellationToken = default);
    Task<int> ImportJsonAsync(string path, CancellationToken cancellationToken = default);
    Task ExportJsonAsync(string path, CancellationToken cancellationToken = default);
    Task ExportIcalAsync(string path, CancellationToken cancellationToken = default);
    Task ExportTickTickCsvAsync(string path, CancellationToken cancellationToken = default);
    Task<int> ImportTickTickCsvAsync(string path, CancellationToken cancellationToken = default);

    Task<SyncStatus> GetSyncStatusAsync(CancellationToken cancellationToken = default);
    Task<SyncStatus> PairAsync(
        string serverUrl,
        string pairingCode,
        string deviceName,
        CancellationToken cancellationToken = default);
    Task<SyncStatus> SyncNowAsync(CancellationToken cancellationToken = default);
    Task LogoutSyncAsync(CancellationToken cancellationToken = default);
    Task<IReadOnlyList<DeviceItem>> ListDevicesAsync(CancellationToken cancellationToken = default);
    Task RevokeDeviceAsync(string deviceId, CancellationToken cancellationToken = default);
    Task<PairingCode> CreatePairingCodeAsync(CancellationToken cancellationToken = default);

    Task<bool> GetAutostartAsync(CancellationToken cancellationToken = default);
    Task SetAutostartAsync(bool enabled, CancellationToken cancellationToken = default);
    Task<string?> GetSettingAsync(string key, CancellationToken cancellationToken = default);
    Task SetSettingAsync(
        string key,
        string value,
        CancellationToken cancellationToken = default);

    Task<McpStatus> GetMcpStatusAsync(CancellationToken cancellationToken = default);
    Task<McpStatus> SetMcpEnabledAsync(
        bool enabled,
        CancellationToken cancellationToken = default);
    Task<McpStatus> RotateMcpTokenAsync(CancellationToken cancellationToken = default);
    Task<McpTemplates> GetMcpTemplatesAsync(CancellationToken cancellationToken = default);

    Task<TransferStatus> GetTransferStatusAsync(CancellationToken cancellationToken = default);
    Task<TransferStatus> StartTransferAsync(CancellationToken cancellationToken = default);
    Task<TransferStatus> StopTransferAsync(CancellationToken cancellationToken = default);
    Task<IReadOnlyList<TransferDevice>> DiscoverTransferDevicesAsync(
        CancellationToken cancellationToken = default);
    Task<TransferDevice?> AddManualTransferIpAsync(
        string host,
        int port,
        CancellationToken cancellationToken = default);
    Task UpdateTransferSettingsAsync(
        string alias,
        int port,
        string receiveDir,
        string pin,
        bool autoAcceptFavorites,
        CancellationToken cancellationToken = default);
    Task<string> SendTransferFilesAsync(
        IReadOnlyList<string> paths,
        string fingerprint,
        string? pin,
        CancellationToken cancellationToken = default);
    Task<IReadOnlyList<PendingIncoming>> GetPendingIncomingAsync(
        CancellationToken cancellationToken = default);
    Task RespondIncomingAsync(
        string sessionId,
        IReadOnlyList<string> fileIds,
        CancellationToken cancellationToken = default);
    Task RejectIncomingAsync(string sessionId, CancellationToken cancellationToken = default);
    Task CancelTransferSessionAsync(string sessionId, CancellationToken cancellationToken = default);
    Task<IReadOnlyList<ChatConversation>> ListConversationsAsync(
        CancellationToken cancellationToken = default);
    Task<IReadOnlyList<ChatMessageItem>> ListMessagesAsync(
        string conversationId,
        CancellationToken cancellationToken = default);
    Task MarkConversationReadAsync(
        string conversationId,
        CancellationToken cancellationToken = default);
    Task ClearConversationAsync(
        string conversationId,
        CancellationToken cancellationToken = default);
    Task SendChatTextAsync(
        string fingerprint,
        ChatMessageItem message,
        CancellationToken cancellationToken = default);
    Task<bool> IsChatTrustedAsync(
        string fingerprint,
        CancellationToken cancellationToken = default);
    Task TrustChatPeerAsync(
        string fingerprint,
        CancellationToken cancellationToken = default);
    Task RevokeChatTrustAsync(
        string fingerprint,
        CancellationToken cancellationToken = default);
    Task SendChatFilesAsync(
        string fingerprint,
        IReadOnlyList<string> paths,
        CancellationToken cancellationToken = default);
    Task DeleteChatFileAsync(
        string path,
        CancellationToken cancellationToken = default);
}

public sealed class TaskChangedEventArgs : EventArgs
{
    public IReadOnlyList<string>? TaskIds { get; init; }
}

public sealed class ReminderDueEventArgs : EventArgs
{
    public required string TaskId { get; init; }
    public required string Title { get; init; }
}

public sealed class TransferEventEventArgs : EventArgs
{
    public required System.Text.Json.JsonElement Payload { get; init; }
}

public sealed class McpStatus
{
    public bool Enabled { get; init; }
    public bool Running { get; init; }
    public string? Token { get; init; }
    public string? Error { get; init; }
    public int Port { get; init; } = 18765;
}

public sealed class McpTemplates
{
    public required string ClaudeCode { get; init; }
    public required string ClaudeDesktop { get; init; }
    public required string Codex { get; init; }
    public required string OpenCode { get; init; }
}

public sealed class TransferStatus
{
    public bool Running { get; init; }
    public int Port { get; init; } = 53317;
    public string Alias { get; init; } = "rtools";
    public string Fingerprint { get; init; } = "";
    public string ReceiveDir { get; init; } = "";
    public bool PinConfigured { get; init; }
    public bool AutoAcceptFavorites { get; init; }
    public string? Error { get; init; }
}

public sealed class TransferDevice
{
    public required string Alias { get; init; }
    public required string Fingerprint { get; init; }
    public string? DeviceModel { get; init; }
    public string? DeviceType { get; init; }
    public bool Download { get; init; }
    public IReadOnlyList<TransferChannel> Channels { get; init; } = [];
    public string Address => Channels.FirstOrDefault() is { } channel
        ? $"{channel.Host}:{channel.Port}"
        : "";
}

public sealed class TransferChannel
{
    public required string Host { get; init; }
    public int Port { get; init; }
    public string Protocol { get; init; } = "https";
}

public sealed class TransferFileItem
{
    public required string Id { get; init; }
    public required string FileName { get; init; }
    public long Size { get; init; }
}

public sealed class PendingIncoming
{
    public required string SessionId { get; init; }
    public required string Ip { get; init; }
    public required TransferDevicePeer Device { get; init; }
    public IReadOnlyList<TransferFileItem> Files { get; init; } = [];
}

public sealed class TransferDevicePeer
{
    public required string Alias { get; init; }
    public required string Fingerprint { get; init; }
}

public sealed class ChatConversation
{
    public required string Id { get; init; }
    public required string Alias { get; init; }
    public long LastMessageAt { get; init; }
    public int Unread { get; init; }
    public bool Pinned { get; init; }
}

public sealed class ChatMessageItem
{
    public required string Id { get; init; }
    public required string ConversationId { get; init; }
    public required string Sender { get; init; }
    public required string SenderAlias { get; init; }
    public required string Kind { get; init; }
    public required string Body { get; init; }
    public string? FilePath { get; init; }
    public long? FileSize { get; init; }
    public string? Preview { get; init; }
    public string? ForwardedFrom { get; init; }
    public long CreatedAt { get; init; }
    public string Status { get; init; } = "sent";
    public string Direction { get; init; } = "receive";
    public double Progress { get; init; }
}

