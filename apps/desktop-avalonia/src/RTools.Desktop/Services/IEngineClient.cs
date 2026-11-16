using RTools.Desktop.Models;

namespace RTools.Desktop.Services;

public interface IEngineClient : IAsyncDisposable
{
    event EventHandler<TaskChangedEventArgs>? TasksChanged;
    event EventHandler<ReminderDueEventArgs>? ReminderDue;

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

    Task<TransferStatus> GetTransferStatusAsync(CancellationToken cancellationToken = default);
    Task<TransferStatus> StartTransferAsync(CancellationToken cancellationToken = default);
    Task<TransferStatus> StopTransferAsync(CancellationToken cancellationToken = default);
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

public sealed class McpStatus
{
    public bool Enabled { get; init; }
    public bool Running { get; init; }
    public string? Token { get; init; }
    public string? Error { get; init; }
    public int Port { get; init; } = 18765;
}

public sealed class TransferStatus
{
    public bool Running { get; init; }
    public int Port { get; init; } = 53317;
    public string? Error { get; init; }
}

