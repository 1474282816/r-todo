using RTools.Desktop.Models;
using System.Collections.Concurrent;
using System.Diagnostics;
using System.IO.Pipes;
using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;

namespace RTools.Desktop.Services;

public sealed class EngineProcessClient : IEngineClient
{
    private const int ProtocolVersion = 1;
    private const int MaxFrameSize = 16 * 1024 * 1024;
    private readonly SemaphoreSlim _connectLock = new(1, 1);
    private readonly ConcurrentDictionary<string, TaskCompletionSource<JsonElement>> _pending = new();
    private readonly CancellationTokenSource _shutdown = new();
    private Process? _process;
    private NamedPipeClientStream? _pipe;
    private Task? _readerTask;
    private bool _disposed;

    public event EventHandler<TaskChangedEventArgs>? TasksChanged;
    public event EventHandler<ReminderDueEventArgs>? ReminderDue;

    public bool IsConnected => _pipe?.IsConnected == true;

    public string PipeName { get; } =
        $@"\\.\pipe\rtools-engine-{Environment.UserName}-{Environment.ProcessId}";

    public async Task StartAsync(CancellationToken cancellationToken = default)
    {
        if (IsConnected)
        {
            return;
        }
        await _connectLock.WaitAsync(cancellationToken);
        try
        {
            if (IsConnected)
            {
                return;
            }
            _process = StartEngineProcess();
            _pipe = new NamedPipeClientStream(
                ".",
                PipeName.StartsWith(@"\\.\pipe\", StringComparison.OrdinalIgnoreCase)
                    ? PipeName[@"\\.\pipe\".Length..]
                    : PipeName,
                PipeDirection.InOut,
                PipeOptions.Asynchronous | PipeOptions.CurrentUserOnly);
            await _pipe.ConnectAsync(5000, cancellationToken);
            _readerTask = ReadLoopAsync(_shutdown.Token);
            await CallAsync("system.health", new { }, cancellationToken);
        }
        finally
        {
            _connectLock.Release();
        }
    }

    public Task<IReadOnlyList<TaskItem>> ListTasksAsync(
        CancellationToken cancellationToken = default)
        => SendAsync<IReadOnlyList<TaskItem>>("tasks.list", new { }, cancellationToken);

    public Task<TaskItem> CreateTaskAsync(
        string title,
        TaskPriority priority,
        DateTimeOffset? remindAt,
        int remindOffsetMinutes,
        string? repeatRule,
        CancellationToken cancellationToken = default)
        => SendAsync<TaskItem>(
            "tasks.create",
            new
            {
                title,
                priority = priority.ToString().ToLowerInvariant(),
                remind_at = remindAt,
                remind_offset_min = remindOffsetMinutes,
                repeat_rule = repeatRule
            },
            cancellationToken);

    public Task<TaskItem> UpdateTaskAsync(
        TaskItem task,
        CancellationToken cancellationToken = default)
        => SendAsync<TaskItem>("tasks.update", ToWire(task), cancellationToken);

    public Task<TaskItem> CompleteTaskAsync(
        string id,
        bool completed,
        CancellationToken cancellationToken = default)
        => SendAsync<TaskItem>(
            "tasks.complete",
            new { id, completed },
            cancellationToken);

    public Task DeleteTaskAsync(string id, CancellationToken cancellationToken = default)
        => SendVoidAsync("tasks.delete", new { id }, cancellationToken);

    public Task ReorderTasksAsync(
        IReadOnlyList<string> orderedIds,
        CancellationToken cancellationToken = default)
        => SendVoidAsync("tasks.reorder", new { ids = orderedIds }, cancellationToken);

    public async Task<int> ImportJsonAsync(
        string path,
        CancellationToken cancellationToken = default)
    {
        var result = await CallAsync("tasks.import_json", new { path }, cancellationToken);
        return result.GetProperty("imported").GetInt32();
    }

    public Task ExportJsonAsync(string path, CancellationToken cancellationToken = default)
        => SendVoidAsync("tasks.export_json", new { path }, cancellationToken);

    public async Task ExportIcalAsync(
        string path,
        CancellationToken cancellationToken = default)
    {
        var tasks = await ListTasksAsync(cancellationToken);
        var lines = new List<string>
        {
            "BEGIN:VCALENDAR",
            "VERSION:2.0",
            "PRODID:-//rtools//todo//CN"
        };
        foreach (var task in tasks)
        {
            lines.Add("BEGIN:VTODO");
            lines.Add($"UID:{task.Id}");
            lines.Add($"SUMMARY:{task.Title.Replace("\n", "\\n", StringComparison.Ordinal)}");
            lines.Add(task.CompletedAt is null ? "STATUS:NEEDS-ACTION" : "STATUS:COMPLETED");
            lines.Add("END:VTODO");
        }
        lines.Add("END:VCALENDAR");
        await File.WriteAllLinesAsync(path, lines, cancellationToken);
    }

    public async Task<SyncStatus> GetSyncStatusAsync(
        CancellationToken cancellationToken = default)
    {
        var value = await CallAsync("sync.status", new { }, cancellationToken);
        return new SyncStatus
        {
            Paired = value.GetProperty("paired").GetBoolean(),
            Connected = value.GetProperty("paired").GetBoolean(),
            Syncing = false,
            PendingOperations = value.GetProperty("pending").GetInt32(),
            ServerUrl = NullableString(value, "server_url"),
            DeviceName = NullableString(value, "device_name"),
            LastSyncAt = NullableDate(value, "last_sync_at"),
            Error = NullableString(value, "last_error")
        };
    }

    public async Task<SyncStatus> PairAsync(
        string serverUrl,
        string pairingCode,
        string deviceName,
        CancellationToken cancellationToken = default)
    {
        await CallAsync(
            "sync.pair",
            new { server_url = serverUrl, pairing_code = pairingCode, device_name = deviceName },
            cancellationToken);
        return await GetSyncStatusAsync(cancellationToken);
    }

    public async Task<SyncStatus> SyncNowAsync(CancellationToken cancellationToken = default)
    {
        await CallAsync("sync.sync_now", new { }, cancellationToken);
        return await GetSyncStatusAsync(cancellationToken);
    }

    public async Task LogoutSyncAsync(CancellationToken cancellationToken = default)
    {
        await CallAsync("sync.logout", new { }, cancellationToken);
    }

    public async Task<IReadOnlyList<DeviceItem>> ListDevicesAsync(
        CancellationToken cancellationToken = default)
    {
        var value = await CallAsync("devices.list", new { }, cancellationToken);
        return value.Deserialize<IReadOnlyList<DeviceItem>>(JsonOptions) ?? [];
    }

    public Task RevokeDeviceAsync(
        string deviceId,
        CancellationToken cancellationToken = default)
        => SendVoidAsync("devices.revoke", new { device_id = deviceId }, cancellationToken);

    public Task<PairingCode> CreatePairingCodeAsync(
        CancellationToken cancellationToken = default)
        => SendAsync<PairingCode>("devices.pairing_code", new { }, cancellationToken);

    public async Task<bool> GetAutostartAsync(
        CancellationToken cancellationToken = default)
    {
        var value = await CallAsync("autostart.get", new { }, cancellationToken);
        return value.GetProperty("enabled").GetBoolean();
    }

    public Task SetAutostartAsync(bool enabled, CancellationToken cancellationToken = default)
        => SendVoidAsync("autostart.set", new { enabled }, cancellationToken);

    public async Task<string?> GetSettingAsync(
        string key,
        CancellationToken cancellationToken = default)
    {
        var value = await CallAsync("settings.get", new { key }, cancellationToken);
        return NullableString(value, "value");
    }

    public Task SetSettingAsync(
        string key,
        string value,
        CancellationToken cancellationToken = default)
        => SendVoidAsync("settings.set", new { key, value }, cancellationToken);

    public async Task<McpStatus> GetMcpStatusAsync(
        CancellationToken cancellationToken = default)
        => ParseMcp(await CallAsync("mcp.status", new { }, cancellationToken));

    public async Task<McpStatus> SetMcpEnabledAsync(
        bool enabled,
        CancellationToken cancellationToken = default)
        => ParseMcp(
            await CallAsync("mcp.set_enabled", new { enabled }, cancellationToken));

    public async Task<McpStatus> RotateMcpTokenAsync(
        CancellationToken cancellationToken = default)
        => ParseMcp(await CallAsync("mcp.rotate_token", new { }, cancellationToken));

    public async Task<TransferStatus> GetTransferStatusAsync(
        CancellationToken cancellationToken = default)
        => ParseTransfer(await CallAsync("transfer.status", new { }, cancellationToken));

    public async Task<TransferStatus> StartTransferAsync(
        CancellationToken cancellationToken = default)
        => ParseTransfer(await CallAsync("transfer.start", new { }, cancellationToken));

    public async Task<TransferStatus> StopTransferAsync(
        CancellationToken cancellationToken = default)
        => ParseTransfer(await CallAsync("transfer.stop", new { }, cancellationToken));

    private Process StartEngineProcess()
    {
        var exeName = OperatingSystem.IsWindows() ? "rtools-engine.exe" : "rtools-engine";
        var appDir = AppContext.BaseDirectory;
        var path = Path.Combine(appDir, exeName);
        if (!File.Exists(path))
        {
            throw new FileNotFoundException("rtools-engine 未安装", path);
        }
        var dataDir = Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
            "rtools",
            "engine");
        Directory.CreateDirectory(dataDir);
        var info = new ProcessStartInfo(path)
        {
            UseShellExecute = false,
            CreateNoWindow = true,
            WorkingDirectory = appDir,
            RedirectStandardOutput = true,
            RedirectStandardError = true
        };
        info.ArgumentList.Add("serve");
        info.ArgumentList.Add("--data-dir");
        info.ArgumentList.Add(dataDir);
        info.ArgumentList.Add("--pipe");
        info.ArgumentList.Add(PipeName);
        var logDir = Path.Combine(dataDir, "logs");
        Directory.CreateDirectory(logDir);
        var stdoutPath = Path.Combine(logDir, "engine.stdout.log");
        var stderrPath = Path.Combine(logDir, "engine.stderr.log");
        var process = Process.Start(info)
            ?? throw new InvalidOperationException("无法启动 rtools-engine");
        _ = PumpOutputAsync(process.StandardOutput, stdoutPath);
        _ = PumpOutputAsync(process.StandardError, stderrPath);
        return process;
    }

    private static async Task PumpOutputAsync(StreamReader reader, string path)
    {
        await using var writer = new StreamWriter(path, append: true, Encoding.UTF8)
        {
            AutoFlush = true
        };
        while (true)
        {
            var line = await reader.ReadLineAsync();
            if (line is null)
            {
                break;
            }
            await writer.WriteLineAsync(line);
        }
    }

    private async Task<T> SendAsync<T>(
        string method,
        object payload,
        CancellationToken cancellationToken)
    {
        var value = await CallAsync(method, payload, cancellationToken);
        return value.Deserialize<T>(JsonOptions)
            ?? throw new InvalidOperationException($"无法解析 {method} 响应");
    }

    private async Task SendVoidAsync(
        string method,
        object payload,
        CancellationToken cancellationToken)
    {
        await CallAsync(method, payload, cancellationToken);
    }

    private async Task<JsonElement> CallAsync(
        string method,
        object payload,
        CancellationToken cancellationToken)
    {
        ObjectDisposedException.ThrowIf(_disposed, this);
        if (!IsConnected)
        {
            await StartAsync(cancellationToken);
        }
        var requestId = Guid.NewGuid().ToString("N");
        var completion = new TaskCompletionSource<JsonElement>(
            TaskCreationOptions.RunContinuationsAsynchronously);
        if (!_pending.TryAdd(requestId, completion))
        {
            throw new InvalidOperationException("重复请求 ID");
        }
        try
        {
            var request = new
            {
                protocol_version = ProtocolVersion,
                message_type = "request",
                request_id = requestId,
                method,
                payload
            };
            await WriteFrameAsync(
                JsonSerializer.SerializeToUtf8Bytes(request, JsonOptions),
                cancellationToken);
            return await completion.Task.WaitAsync(
                TimeSpan.FromSeconds(30),
                cancellationToken);
        }
        finally
        {
            _pending.TryRemove(requestId, out _);
        }
    }

    private async Task WriteFrameAsync(byte[] data, CancellationToken cancellationToken)
    {
        if (_pipe is null || !_pipe.IsConnected)
        {
            throw new IOException("Engine 命名管道未连接");
        }
        if (data.Length is 0 or > MaxFrameSize)
        {
            throw new InvalidDataException("无效 RPC 帧大小");
        }
        var header = new byte[4];
        System.Buffers.Binary.BinaryPrimitives.WriteUInt32BigEndian(
            header,
            (uint)data.Length);
        await _pipe.WriteAsync(header, cancellationToken);
        await _pipe.WriteAsync(data, cancellationToken);
        await _pipe.FlushAsync(cancellationToken);
    }

    private async Task ReadLoopAsync(CancellationToken cancellationToken)
    {
        try
        {
            var header = new byte[4];
            while (!cancellationToken.IsCancellationRequested && _pipe is not null)
            {
                await _pipe.ReadExactlyAsync(header, cancellationToken);
                var size = System.Buffers.Binary.BinaryPrimitives.ReadUInt32BigEndian(header);
                if (size is 0 or > MaxFrameSize)
                {
                    throw new InvalidDataException("Engine 返回无效 RPC 帧");
                }
                var frame = new byte[size];
                await _pipe.ReadExactlyAsync(frame, cancellationToken);
                using var document = JsonDocument.Parse(frame);
                var root = document.RootElement;
                if (root.GetProperty("message_type").GetString() == "event")
                {
                    HandleEvent(root);
                    continue;
                }
                var requestId = root.GetProperty("request_id").GetString()!;
                if (!_pending.TryRemove(requestId, out var completion))
                {
                    continue;
                }
                if (root.TryGetProperty("error", out var error) &&
                    error.ValueKind != JsonValueKind.Null)
                {
                    completion.TrySetException(
                        new EngineRpcException(
                            error.GetProperty("code").GetString() ?? "UNKNOWN",
                            error.GetProperty("message").GetString() ?? "Engine 调用失败"));
                }
                else
                {
                    completion.TrySetResult(root.GetProperty("result").Clone());
                }
            }
        }
        catch (OperationCanceledException)
        {
        }
        catch (Exception ex)
        {
            foreach (var item in _pending.Values)
            {
                item.TrySetException(ex);
            }
        }
    }

    private void HandleEvent(JsonElement root)
    {
        var name = root.GetProperty("event").GetString();
        if (name == "tasks.changed")
        {
            TasksChanged?.Invoke(this, new TaskChangedEventArgs());
        }
        else if (name == "reminder.due")
        {
            var payload = root.GetProperty("payload");
            ReminderDue?.Invoke(
                this,
                new ReminderDueEventArgs
                {
                    TaskId = payload.GetProperty("task_id").GetString() ?? "",
                    Title = payload.GetProperty("title").GetString() ?? ""
                });
        }
    }

    private static object ToWire(TaskItem task) => new
    {
        id = task.Id,
        title = task.Title,
        priority = task.Priority.ToString().ToLowerInvariant(),
        remind_at = task.RemindAt,
        remind_offset_min = task.RemindOffsetMinutes,
        repeat_rule = task.RepeatRule,
        sort_order = task.SortOrder,
        completed_at = task.CompletedAt,
        deleted_at = task.DeletedAt,
        created_at = task.CreatedAt,
        updated_at = task.UpdatedAt,
        revision = task.Revision
    };

    private static McpStatus ParseMcp(JsonElement value) => new()
    {
        Enabled = value.GetProperty("enabled").GetBoolean(),
        Running = value.GetProperty("running").GetBoolean(),
        Token = NullableString(value, "token"),
        Error = NullableString(value, "error"),
        Port = value.TryGetProperty("port", out var port) ? port.GetInt32() : 18765
    };

    private static TransferStatus ParseTransfer(JsonElement value) => new()
    {
        Running = value.GetProperty("running").GetBoolean(),
        Port = value.TryGetProperty("port", out var port) ? port.GetInt32() : 53317,
        Error = NullableString(value, "error")
    };

    private static string? NullableString(JsonElement value, string property)
    {
        if (!value.TryGetProperty(property, out var item) ||
            item.ValueKind is JsonValueKind.Null or JsonValueKind.Undefined)
        {
            return null;
        }
        var text = item.GetString();
        return string.IsNullOrEmpty(text) ? null : text;
    }

    private static DateTimeOffset? NullableDate(JsonElement value, string property)
    {
        if (!value.TryGetProperty(property, out var item) ||
            item.ValueKind is JsonValueKind.Null or JsonValueKind.Undefined)
        {
            return null;
        }
        return item.GetDateTimeOffset();
    }

    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
        PropertyNameCaseInsensitive = true,
        DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull
    };

    public async ValueTask DisposeAsync()
    {
        if (_disposed)
        {
            return;
        }
        _disposed = true;
        _shutdown.Cancel();
        if (_pipe is not null)
        {
            await _pipe.DisposeAsync();
        }
        if (_process is { HasExited: false })
        {
            try
            {
                _process.Kill(entireProcessTree: true);
                await _process.WaitForExitAsync();
            }
            catch
            {
            }
        }
        _process?.Dispose();
        _shutdown.Dispose();
        _connectLock.Dispose();
    }
}

public sealed class EngineRpcException(string code, string message) : Exception(message)
{
    public string Code { get; } = code;
}

