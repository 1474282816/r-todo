using RTools.Desktop.Models;
using System.Text.Json;

namespace RTools.Desktop.Services;

public sealed class InMemoryEngineClient : IEngineClient
{
    private readonly List<TaskItem> _tasks = [];
    private readonly Dictionary<string, string> _settings = new(StringComparer.Ordinal);
    private bool _mcpEnabled;
    private string? _mcpToken;
    private bool _transferRunning;

    public event EventHandler<TaskChangedEventArgs>? TasksChanged;
    public event EventHandler<TransferEventEventArgs>? TransferEventReceived
    {
        add { }
        remove { }
    }
    public event EventHandler? ChatChanged
    {
        add { }
        remove { }
    }
    public event EventHandler<ReminderDueEventArgs>? ReminderDue
    {
        add { }
        remove { }
    }

    public bool IsConnected { get; private set; }

    public Task StartAsync(CancellationToken cancellationToken = default)
    {
        IsConnected = true;
        return Task.CompletedTask;
    }

    public Task<IReadOnlyList<TaskItem>> ListTasksAsync(
        CancellationToken cancellationToken = default)
    {
        IReadOnlyList<TaskItem> list = _tasks
            .Where(x => x.DeletedAt is null)
            .OrderBy(x => x.IsCompleted)
            .ThenBy(x => x.IsCompleted ? 0 : x.SortOrder)
            .ThenByDescending(x => x.CompletedAt)
            .Select(x => x.Clone())
            .ToArray();
        return Task.FromResult(list);
    }

    public Task<TaskItem> CreateTaskAsync(
        string title,
        TaskPriority priority,
        DateTimeOffset? remindAt,
        int remindOffsetMinutes,
        string? repeatRule,
        CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(title))
        {
            throw new ArgumentException("任务标题不能为空", nameof(title));
        }
        var now = DateTimeOffset.UtcNow;
        var task = new TaskItem
        {
            Id = Guid.NewGuid().ToString("N"),
            Title = title.Trim(),
            Priority = priority,
            RemindAt = remindAt,
            RemindOffsetMinutes = remindOffsetMinutes,
            RepeatRule = repeatRule,
            SortOrder = (_tasks.Where(x => !x.IsCompleted).MinBy(x => x.SortOrder)?.SortOrder ?? 0) - 1000,
            CreatedAt = now,
            UpdatedAt = now
        };
        _tasks.Add(task);
        RaiseChanged(task.Id);
        return Task.FromResult(task.Clone());
    }

    public Task<TaskItem> UpdateTaskAsync(
        TaskItem task,
        CancellationToken cancellationToken = default)
    {
        var index = _tasks.FindIndex(x => x.Id == task.Id);
        if (index < 0)
        {
            throw new InvalidOperationException("任务不存在");
        }
        var next = task.Clone();
        next.UpdatedAt = DateTimeOffset.UtcNow;
        next.Revision++;
        _tasks[index] = next;
        RaiseChanged(next.Id);
        return Task.FromResult(next.Clone());
    }

    public Task<TaskItem> CompleteTaskAsync(
        string id,
        bool completed,
        CancellationToken cancellationToken = default)
    {
        var task = _tasks.Single(x => x.Id == id);
        var next = task.Clone();
        next.CompletedAt = completed ? DateTimeOffset.UtcNow : null;
        next.UpdatedAt = DateTimeOffset.UtcNow;
        next.Revision++;
        _tasks[_tasks.FindIndex(x => x.Id == id)] = next;
        RaiseChanged(id);
        return Task.FromResult(next.Clone());
    }

    public Task DeleteTaskAsync(string id, CancellationToken cancellationToken = default)
    {
        var task = _tasks.Single(x => x.Id == id);
        var next = task.Clone();
        next.DeletedAt = DateTimeOffset.UtcNow;
        next.UpdatedAt = next.DeletedAt.Value;
        next.Revision++;
        _tasks[_tasks.FindIndex(x => x.Id == id)] = next;
        RaiseChanged(id);
        return Task.CompletedTask;
    }

    public Task ReorderTasksAsync(
        IReadOnlyList<string> orderedIds,
        CancellationToken cancellationToken = default)
    {
        for (var i = 0; i < orderedIds.Count; i++)
        {
            var index = _tasks.FindIndex(x => x.Id == orderedIds[i]);
            if (index < 0)
            {
                continue;
            }
            var next = _tasks[index].Clone();
            next.SortOrder = (i + 1) * 1000;
            next.UpdatedAt = DateTimeOffset.UtcNow;
            next.Revision++;
            _tasks[index] = next;
        }
        RaiseChanged(null);
        return Task.CompletedTask;
    }

    public async Task<int> ImportJsonAsync(
        string path,
        CancellationToken cancellationToken = default)
    {
        await using var stream = File.OpenRead(path);
        using var document = await JsonDocument.ParseAsync(
            stream,
            cancellationToken: cancellationToken);
        var root = document.RootElement;
        if (!root.TryGetProperty("format", out var format) ||
            format.GetString() != "r-todo/todo")
        {
            throw new InvalidDataException("不是有效的 r-todo JSON");
        }
        if (!root.TryGetProperty("version", out var version) || version.GetInt32() != 1)
        {
            throw new InvalidDataException("不支持的 JSON 版本");
        }
        var count = 0;
        foreach (var item in root.GetProperty("todos").EnumerateArray())
        {
            var id = item.GetProperty("id").GetString()!;
            if (_tasks.Any(x => x.Id == id))
            {
                continue;
            }
            var created = item.GetProperty("created_at").GetDateTimeOffset();
            var updated = item.GetProperty("updated_at").GetDateTimeOffset();
            var task = new TaskItem
            {
                Id = id,
                Title = item.GetProperty("title").GetString()!,
                Priority = ParsePriority(item.GetProperty("priority").GetString()),
                RemindAt = ParseNullableDateTime(item, "remind_at"),
                RemindOffsetMinutes = item.TryGetProperty("remind_offset_min", out var offset)
                    ? offset.GetInt32()
                    : 15,
                RepeatRule = ParseNullableString(item, "repeat_rule"),
                SortOrder = item.TryGetProperty("sort_order", out var sort) ? sort.GetDouble() : 0,
                CompletedAt = ParseNullableDateTime(item, "completed_at"),
                DeletedAt = item.TryGetProperty("deleted", out var deleted) && deleted.GetBoolean()
                    ? updated
                    : null,
                CreatedAt = created,
                UpdatedAt = updated
            };
            _tasks.Add(task);
            count++;
        }
        RaiseChanged(null);
        return count;
    }

    public async Task ExportJsonAsync(
        string path,
        CancellationToken cancellationToken = default)
    {
        var payload = new
        {
            format = "r-todo/todo",
            version = 1,
            exported_at = DateTimeOffset.UtcNow,
            todos = _tasks.Select(x => new
            {
                id = x.Id,
                title = x.Title,
                priority = x.Priority.ToString().ToLowerInvariant(),
                remind_at = x.RemindAt,
                remind_offset_min = x.RemindOffsetMinutes,
                repeat_rule = x.RepeatRule,
                sort_order = x.SortOrder,
                deleted = x.DeletedAt is not null,
                completed_at = x.CompletedAt,
                created_at = x.CreatedAt,
                updated_at = x.UpdatedAt
            })
        };
        await File.WriteAllTextAsync(
            path,
            JsonSerializer.Serialize(payload, new JsonSerializerOptions { WriteIndented = true }),
            cancellationToken);
    }

    public Task ExportIcalAsync(string path, CancellationToken cancellationToken = default)
    {
        var lines = new List<string>
        {
            "BEGIN:VCALENDAR",
            "VERSION:2.0",
            "PRODID:-//rtools//todo//CN"
        };
        foreach (var task in _tasks.Where(x => x.DeletedAt is null))
        {
            lines.Add("BEGIN:VTODO");
            lines.Add($"UID:{task.Id}");
            lines.Add($"SUMMARY:{task.Title.Replace("\n", "\\n", StringComparison.Ordinal)}");
            lines.Add(task.CompletedAt is null ? "STATUS:NEEDS-ACTION" : "STATUS:COMPLETED");
            lines.Add("END:VTODO");
        }
        lines.Add("END:VCALENDAR");
        return File.WriteAllLinesAsync(path, lines, cancellationToken);
    }

    public Task ExportTickTickCsvAsync(
        string path,
        CancellationToken cancellationToken = default)
        => File.WriteAllTextAsync(
            path,
            "Title,Content,Status,Completed Time,Start Date,Due Date,Priority\n",
            cancellationToken);

    public Task<int> ImportTickTickCsvAsync(
        string path,
        CancellationToken cancellationToken = default)
        => Task.FromResult(0);

    public Task<SyncStatus> GetSyncStatusAsync(CancellationToken cancellationToken = default)
        => Task.FromResult(new SyncStatus());

    public Task<SyncStatus> PairAsync(
        string serverUrl,
        string pairingCode,
        string deviceName,
        CancellationToken cancellationToken = default)
        => throw new NotSupportedException("请启动 rtools-engine 后配对");

    public Task<SyncStatus> SyncNowAsync(CancellationToken cancellationToken = default)
        => Task.FromResult(new SyncStatus());

    public Task LogoutSyncAsync(CancellationToken cancellationToken = default)
        => Task.CompletedTask;

    public Task<IReadOnlyList<DeviceItem>> ListDevicesAsync(
        CancellationToken cancellationToken = default)
        => Task.FromResult<IReadOnlyList<DeviceItem>>([]);

    public Task RevokeDeviceAsync(
        string deviceId,
        CancellationToken cancellationToken = default)
        => Task.CompletedTask;

    public Task<PairingCode> CreatePairingCodeAsync(
        CancellationToken cancellationToken = default)
        => throw new NotSupportedException("请启动 rtools-engine 后生成配对码");

    public Task<bool> GetAutostartAsync(CancellationToken cancellationToken = default)
        => Task.FromResult(
            _settings.TryGetValue("autostart", out var value) && value == "true");

    public Task SetAutostartAsync(
        bool enabled,
        CancellationToken cancellationToken = default)
    {
        _settings["autostart"] = enabled ? "true" : "false";
        return Task.CompletedTask;
    }

    public Task<string?> GetSettingAsync(
        string key,
        CancellationToken cancellationToken = default)
        => Task.FromResult(_settings.TryGetValue(key, out var value) ? value : null);

    public Task SetSettingAsync(
        string key,
        string value,
        CancellationToken cancellationToken = default)
    {
        _settings[key] = value;
        return Task.CompletedTask;
    }

    public Task<McpStatus> GetMcpStatusAsync(
        CancellationToken cancellationToken = default)
        => Task.FromResult(new McpStatus
        {
            Enabled = _mcpEnabled,
            Running = _mcpEnabled,
            Token = _mcpToken
        });

    public Task<McpStatus> SetMcpEnabledAsync(
        bool enabled,
        CancellationToken cancellationToken = default)
    {
        _mcpEnabled = enabled;
        if (enabled && _mcpToken is null)
        {
            _mcpToken = Guid.NewGuid().ToString("N");
        }
        return GetMcpStatusAsync(cancellationToken);
    }

    public Task<McpStatus> RotateMcpTokenAsync(
        CancellationToken cancellationToken = default)
    {
        _mcpToken = Guid.NewGuid().ToString("N");
        return GetMcpStatusAsync(cancellationToken);
    }

    public Task<McpTemplates> GetMcpTemplatesAsync(
        CancellationToken cancellationToken = default)
        => Task.FromResult(new McpTemplates
        {
            ClaudeCode = "claude mcp add rtools",
            ClaudeDesktop = "{}",
            Codex = "[mcp_servers.rtools]",
            OpenCode = "{}"
        });

    public Task<UpdateCheckResult> CheckForUpdatesAsync(
        string currentVersion,
        CancellationToken cancellationToken = default)
        => Task.FromResult(new UpdateCheckResult
        {
            CurrentVersion = currentVersion,
            LatestVersion = currentVersion
        });

    public Task<string> DownloadUpdateAsync(
        string url,
        IProgress<double>? progress = null,
        CancellationToken cancellationToken = default)
        => Task.FromResult(Path.GetTempFileName());

    public Task<TransferStatus> GetTransferStatusAsync(
        CancellationToken cancellationToken = default)
        => Task.FromResult(new TransferStatus
        {
            Running = _transferRunning,
            Alias = "rtools-test"
        });

    public Task<TransferStatus> StartTransferAsync(
        CancellationToken cancellationToken = default)
    {
        _transferRunning = true;
        return GetTransferStatusAsync(cancellationToken);
    }

    public Task<TransferStatus> StopTransferAsync(
        CancellationToken cancellationToken = default)
    {
        _transferRunning = false;
        return GetTransferStatusAsync(cancellationToken);
    }

    public Task<IReadOnlyList<TransferDevice>> DiscoverTransferDevicesAsync(
        CancellationToken cancellationToken = default)
        => Task.FromResult<IReadOnlyList<TransferDevice>>([]);

    public Task<TransferDevice?> AddManualTransferIpAsync(
        string host,
        int port,
        CancellationToken cancellationToken = default)
        => Task.FromResult<TransferDevice?>(null);

    public Task UpdateTransferSettingsAsync(
        string alias,
        int port,
        string receiveDir,
        string pin,
        bool autoAcceptFavorites,
        CancellationToken cancellationToken = default)
        => Task.CompletedTask;

    public Task<string> SendTransferFilesAsync(
        IReadOnlyList<string> paths,
        string fingerprint,
        string? pin,
        CancellationToken cancellationToken = default)
        => Task.FromResult("session-test");

    public Task<IReadOnlyList<PendingIncoming>> GetPendingIncomingAsync(
        CancellationToken cancellationToken = default)
        => Task.FromResult<IReadOnlyList<PendingIncoming>>([]);

    public Task RespondIncomingAsync(
        string sessionId,
        IReadOnlyList<string> fileIds,
        CancellationToken cancellationToken = default)
        => Task.CompletedTask;

    public Task RejectIncomingAsync(
        string sessionId,
        CancellationToken cancellationToken = default)
        => Task.CompletedTask;

    public Task CancelTransferSessionAsync(
        string sessionId,
        CancellationToken cancellationToken = default)
        => Task.CompletedTask;

    public Task<IReadOnlyList<ChatConversation>> ListConversationsAsync(
        CancellationToken cancellationToken = default)
        => Task.FromResult<IReadOnlyList<ChatConversation>>([]);

    public Task<IReadOnlyList<ChatMessageItem>> ListMessagesAsync(
        string conversationId,
        CancellationToken cancellationToken = default)
        => Task.FromResult<IReadOnlyList<ChatMessageItem>>([]);

    public Task MarkConversationReadAsync(
        string conversationId,
        CancellationToken cancellationToken = default)
        => Task.CompletedTask;

    public Task ClearConversationAsync(
        string conversationId,
        CancellationToken cancellationToken = default)
        => Task.CompletedTask;

    public Task SendChatTextAsync(
        string fingerprint,
        ChatMessageItem message,
        CancellationToken cancellationToken = default)
        => Task.CompletedTask;

    public Task<bool> IsChatTrustedAsync(
        string fingerprint,
        CancellationToken cancellationToken = default)
        => Task.FromResult(true);

    public Task TrustChatPeerAsync(
        string fingerprint,
        CancellationToken cancellationToken = default)
        => Task.CompletedTask;

    public Task RevokeChatTrustAsync(
        string fingerprint,
        CancellationToken cancellationToken = default)
        => Task.CompletedTask;

    public Task SendChatFilesAsync(
        string fingerprint,
        IReadOnlyList<string> paths,
        CancellationToken cancellationToken = default)
        => Task.CompletedTask;

    public Task SendTrustMessageAsync(
        string fingerprint,
        string senderAlias,
        CancellationToken cancellationToken = default)
        => Task.CompletedTask;

    public Task DeleteChatFileAsync(
        string path,
        CancellationToken cancellationToken = default)
        => Task.CompletedTask;

    public ValueTask DisposeAsync()
    {
        IsConnected = false;
        return ValueTask.CompletedTask;
    }

    private void RaiseChanged(string? id)
        => TasksChanged?.Invoke(this, new TaskChangedEventArgs
        {
            TaskIds = id is null ? null : [id]
        });

    private static TaskPriority ParsePriority(string? value) => value switch
    {
        "high" => TaskPriority.High,
        "low" => TaskPriority.Low,
        _ => TaskPriority.Medium
    };

    private static string? ParseNullableString(JsonElement item, string property)
    {
        if (!item.TryGetProperty(property, out var value) ||
            value.ValueKind == JsonValueKind.Null)
        {
            return null;
        }
        return value.GetString();
    }

    private static DateTimeOffset? ParseNullableDateTime(JsonElement item, string property)
    {
        if (!item.TryGetProperty(property, out var value) ||
            value.ValueKind == JsonValueKind.Null)
        {
            return null;
        }
        return value.GetDateTimeOffset();
    }
}

