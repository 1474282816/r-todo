namespace RTools.Desktop.Models;

public sealed class SyncStatus
{
    public bool Paired { get; init; }
    public bool Connected { get; init; }
    public bool Syncing { get; init; }
    public int PendingOperations { get; init; }
    public string? ServerUrl { get; init; }
    public string? DeviceName { get; init; }
    public DateTimeOffset? LastSyncAt { get; init; }
    public string? Error { get; init; }
}

public sealed class DeviceItem
{
    public required string Id { get; init; }
    public required string Name { get; init; }
    public DateTimeOffset CreatedAt { get; init; }
    public DateTimeOffset? LastSeenAt { get; init; }
    public DateTimeOffset? RevokedAt { get; init; }
    public bool Revoked => RevokedAt is not null;
}

public sealed class PairingCode
{
    public required string Code { get; init; }
    public DateTimeOffset ExpiresAt { get; init; }
}

