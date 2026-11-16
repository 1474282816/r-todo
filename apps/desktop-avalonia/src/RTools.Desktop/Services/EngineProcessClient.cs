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
    private readonly SemaphoreSlim _writeLock = new(1, 1);
    private readonly ConcurrentDictionary<string, TaskCompletionSource<JsonElement>> _pending = new();
    private readonly CancellationTokenSource _shutdown = new();
    private Process? _process;
    private NamedPipeClientStream? _pipe;
    private Task? _readerTask;
    private bool _disposed;

    public event EventHandler<TaskChangedEventArgs>? TasksChanged;
    public event EventHandler<ReminderDueEventArgs>? ReminderDue;
    public event EventHandler<TransferEventEventArgs>? TransferEventReceived;
    public event EventHandler? ChatChanged;

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
            var token = WindowsCredentialStore.Read("sync-token");
            var server = WindowsCredentialStore.Read("sync-server-url");
            var device = WindowsCredentialStore.Read("sync-device-id");
            var name = WindowsCredentialStore.Read("sync-device-name");
            if (token is not null && server is not null && device is not null && name is not null)
            {
                await CallAsync(
                    "sync.restore_credentials",
                    new
                    {
                        server_url = server,
                        device_id = device,
                        device_name = name,
                        token
                    },
                    cancellationToken);
            }
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

    public async Task ExportTickTickCsvAsync(
        string path,
        CancellationToken cancellationToken = default)
    {
        var tasks = await ListTasksAsync(cancellationToken);
        var lines = new List<string> { "Title,Content,Status,Completed Time,Start Date,Due Date,Priority" };
        foreach (var task in tasks)
        {
            var status = task.IsCompleted ? "Completed" : "0";
            lines.Add(string.Join(
                ',',
                Csv(task.Title),
                Csv(""),
                Csv(status),
                Csv(task.CompletedAt?.ToString("O") ?? ""),
                Csv(""),
                Csv(task.RemindAt?.ToString("O") ?? ""),
                Csv(task.Priority.ToString())));
        }
        await File.WriteAllLinesAsync(path, lines, cancellationToken);
    }

    public async Task<int> ImportTickTickCsvAsync(
        string path,
        CancellationToken cancellationToken = default)
    {
        var lines = await File.ReadAllLinesAsync(path, cancellationToken);
        if (lines.Length < 2)
        {
            return 0;
        }
        var headers = ParseCsvLine(lines[0]);
        int Column(params string[] names)
        {
            for (var index = 0; index < headers.Count; index++)
            {
                if (names.Any(name =>
                        headers[index].Equals(name, StringComparison.OrdinalIgnoreCase)))
                {
                    return index;
                }
            }
            return -1;
        }
        var titleIndex = Column("Title", "标题");
        var dueIndex = Column("Due Date", "到期时间");
        var priorityIndex = Column("Priority", "优先级");
        if (titleIndex < 0)
        {
            throw new InvalidDataException("CSV 缺少 Title 列");
        }
        var count = 0;
        foreach (var line in lines.Skip(1))
        {
            var columns = ParseCsvLine(line);
            if (titleIndex >= columns.Count ||
                string.IsNullOrWhiteSpace(columns[titleIndex]))
            {
                continue;
            }
            var priority = priorityIndex >= 0 && priorityIndex < columns.Count
                ? columns[priorityIndex].ToLowerInvariant() switch
                {
                    "high" or "高" or "3" => TaskPriority.High,
                    "low" or "低" or "1" => TaskPriority.Low,
                    _ => TaskPriority.Medium
                }
                : TaskPriority.Medium;
            DateTimeOffset? due = null;
            if (dueIndex >= 0 && dueIndex < columns.Count)
            {
                due = DateTimeOffset.TryParse(columns[dueIndex], out var parsed)
                    ? parsed
                    : null;
            }
            await CreateTaskAsync(
                columns[titleIndex],
                priority,
                due,
                15,
                null,
                cancellationToken);
            count++;
        }
        return count;
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
        var value = await CallAsync(
            "sync.pair",
            new { server_url = serverUrl, pairing_code = pairingCode, device_name = deviceName },
            cancellationToken);
        var deviceId = NullableString(value, "device_id");
        var token = NullableString(value, "token");
        if (deviceId is not null && token is not null)
        {
            WindowsCredentialStore.Save("sync-server-url", serverUrl.TrimEnd('/'));
            WindowsCredentialStore.Save("sync-device-id", deviceId);
            WindowsCredentialStore.Save("sync-device-name", deviceName);
            WindowsCredentialStore.Save("sync-token", token);
        }
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
        WindowsCredentialStore.Delete("sync-token");
        WindowsCredentialStore.Delete("sync-device-id");
        WindowsCredentialStore.Delete("sync-device-name");
        WindowsCredentialStore.Delete("sync-server-url");
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

    public Task<bool> GetAutostartAsync(CancellationToken cancellationToken = default)
        => Task.FromResult(OperatingSystem.IsWindows() && AutostartService.IsEnabled());

    public async Task SetAutostartAsync(
        bool enabled,
        CancellationToken cancellationToken = default)
    {
        await SendVoidAsync("autostart.set", new { enabled }, cancellationToken);
        if (OperatingSystem.IsWindows())
        {
            AutostartService.Set(enabled);
        }
    }

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

    public async Task<McpTemplates> GetMcpTemplatesAsync(
        CancellationToken cancellationToken = default)
    {
        var status = await GetMcpStatusAsync(cancellationToken);
        var token = status.Token ?? "";
        const string url = "http://127.0.0.1:18765/mcp";
        var bearer = $"Bearer {token}";
        return new McpTemplates
        {
            ClaudeCode =
                $"claude mcp add rtools --transport http {url} --header \"Authorization: {bearer}\"",
            ClaudeDesktop =
                $"{{\"mcpServers\":{{\"rtools\":{{\"url\":\"{url}\",\"headers\":{{\"Authorization\":\"{bearer}\"}}}}}}}}",
            Codex =
                $"[mcp_servers.rtools]\\ntype = \"http\"\\nurl = \"{url}\"\\nheaders = {{ Authorization = \"{bearer}\" }}",
            OpenCode =
                $"{{\"mcp\":{{\"rtools\":{{\"type\":\"http\",\"url\":\"{url}\",\"headers\":{{\"Authorization\":\"{bearer}\"}}}}}}}}"
        };
    }

    public async Task<UpdateCheckResult> CheckForUpdatesAsync(
        string currentVersion,
        CancellationToken cancellationToken = default)
    {
        var client = new HttpClient();
        client.DefaultRequestHeaders.UserAgent.ParseAdd("rtools-updater");
        client.DefaultRequestHeaders.Accept.ParseAdd("application/vnd.github+json");
        string json;
        try
        {
            json = await client.GetStringAsync(
                "https://api.github.com/repos/1474282816/r-todo/releases/latest",
                cancellationToken);
        }
        catch (HttpRequestException)
        {
            // 私有仓库的匿名 API 不返回 release；改用公开静态清单。
            json = await client.GetStringAsync(
                "https://raw.githubusercontent.com/1474282816/r-todo/master/latest.json",
                cancellationToken);
        }
        using var document = JsonDocument.Parse(json);
        var root = document.RootElement;
        var latest = root.TryGetProperty("tag_name", out var tag)
            ? tag.GetString()
            : root.GetProperty("version").GetString();
        var notes = root.TryGetProperty("body", out var body) ? body.GetString() : null;
        var download = root.TryGetProperty("download_url", out var direct)
            ? direct.GetString()
            : root.TryGetProperty("assets", out var assets)
            ? assets.EnumerateArray()
                .Select(asset => asset.TryGetProperty("browser_download_url", out var url)
                    ? url.GetString()
                    : null)
                .FirstOrDefault(url =>
                    url?.EndsWith("-setup.exe", StringComparison.OrdinalIgnoreCase) == true)
            : null;
        return new UpdateCheckResult
        {
            CurrentVersion = currentVersion,
            LatestVersion = latest,
            DownloadUrl = download,
            Notes = notes
        };
    }

    public async Task<string> DownloadUpdateAsync(
        string url,
        IProgress<double>? progress = null,
        CancellationToken cancellationToken = default)
    {
        var directory = Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
            "rtools",
            "updates");
        Directory.CreateDirectory(directory);
        var path = Path.Combine(directory, "rtools-update-setup.exe");
        using var client = new HttpClient();
        using var response = await client.GetAsync(
            url,
            HttpCompletionOption.ResponseHeadersRead,
            cancellationToken);
        response.EnsureSuccessStatusCode();
        var total = response.Content.Headers.ContentLength ?? 0;
        await using var source = await response.Content.ReadAsStreamAsync(cancellationToken);
        await using var target = File.Create(path);
        var buffer = new byte[81920];
        long read = 0;
        while (true)
        {
            var count = await source.ReadAsync(buffer, cancellationToken);
            if (count == 0)
            {
                break;
            }
            await target.WriteAsync(buffer.AsMemory(0, count), cancellationToken);
            read += count;
            if (total > 0)
            {
                progress?.Report((double)read / total);
            }
        }
        return path;
    }

    public async Task<TransferStatus> GetTransferStatusAsync(
        CancellationToken cancellationToken = default)
        => ParseTransfer(await CallAsync("transfer.status", new { }, cancellationToken));

    public async Task<TransferStatus> StartTransferAsync(
        CancellationToken cancellationToken = default)
        => ParseTransfer(await CallAsync("transfer.start", new { }, cancellationToken));

    public async Task<TransferStatus> StopTransferAsync(
        CancellationToken cancellationToken = default)
        => ParseTransfer(await CallAsync("transfer.stop", new { }, cancellationToken));

    public async Task<IReadOnlyList<TransferDevice>> DiscoverTransferDevicesAsync(
        CancellationToken cancellationToken = default)
    {
        await CallAsync("transfer.refresh", new { }, cancellationToken);
        return await SendAsync<IReadOnlyList<TransferDevice>>(
            "transfer.devices",
            new { },
            cancellationToken);
    }

    public async Task<TransferDevice?> AddManualTransferIpAsync(
        string host,
        int port,
        CancellationToken cancellationToken = default)
    {
        var value = await CallAsync(
            "transfer.add_manual_ip",
            new { host, port },
            cancellationToken);
        return value.ValueKind == JsonValueKind.Null
            ? null
            : value.Deserialize<TransferDevice>(JsonOptions);
    }

    public Task UpdateTransferSettingsAsync(
        string alias,
        int port,
        string receiveDir,
        string pin,
        bool autoAcceptFavorites,
        CancellationToken cancellationToken = default)
        => SendVoidAsync(
            "transfer.settings",
            new { alias, port, receive_dir = receiveDir, pin, auto_accept_favorites = autoAcceptFavorites },
            cancellationToken);

    public async Task<string> SendTransferFilesAsync(
        IReadOnlyList<string> paths,
        string fingerprint,
        string? pin,
        CancellationToken cancellationToken = default)
    {
        var value = await CallAsync(
            "transfer.send",
            new { paths, fingerprint, pin },
            cancellationToken);
        return value.GetProperty("session_id").GetString()
            ?? throw new InvalidOperationException("Engine 未返回传输会话 ID");
    }

    public Task<IReadOnlyList<PendingIncoming>> GetPendingIncomingAsync(
        CancellationToken cancellationToken = default)
        => SendAsync<IReadOnlyList<PendingIncoming>>(
            "transfer.pending",
            new { },
            cancellationToken);

    public Task RespondIncomingAsync(
        string sessionId,
        IReadOnlyList<string> fileIds,
        CancellationToken cancellationToken = default)
        => SendVoidAsync(
            "transfer.respond",
            new { session_id = sessionId, file_ids = fileIds },
            cancellationToken);

    public Task RejectIncomingAsync(
        string sessionId,
        CancellationToken cancellationToken = default)
        => SendVoidAsync(
            "transfer.reject",
            new { session_id = sessionId },
            cancellationToken);

    public Task CancelTransferSessionAsync(
        string sessionId,
        CancellationToken cancellationToken = default)
        => SendVoidAsync(
            "transfer.cancel",
            new { session_id = sessionId },
            cancellationToken);

    public Task<IReadOnlyList<ChatConversation>> ListConversationsAsync(
        CancellationToken cancellationToken = default)
        => SendAsync<IReadOnlyList<ChatConversation>>(
            "chat.list_conversations",
            new { },
            cancellationToken);

    public Task<IReadOnlyList<ChatMessageItem>> ListMessagesAsync(
        string conversationId,
        CancellationToken cancellationToken = default)
        => SendAsync<IReadOnlyList<ChatMessageItem>>(
            "chat.list_messages",
            new { conversation_id = conversationId },
            cancellationToken);

    public Task MarkConversationReadAsync(
        string conversationId,
        CancellationToken cancellationToken = default)
        => SendVoidAsync(
            "chat.mark_read",
            new { conversation_id = conversationId },
            cancellationToken);

    public Task ClearConversationAsync(
        string conversationId,
        CancellationToken cancellationToken = default)
        => SendVoidAsync(
            "chat.clear",
            new { conversation_id = conversationId },
            cancellationToken);

    public Task SendChatTextAsync(
        string fingerprint,
        ChatMessageItem message,
        CancellationToken cancellationToken = default)
        => SendVoidAsync(
            "chat.send_text",
            new { fingerprint, message },
            cancellationToken);

    public async Task<bool> IsChatTrustedAsync(
        string fingerprint,
        CancellationToken cancellationToken = default)
    {
        var value = await CallAsync(
            "chat.trusted",
            new { fingerprint },
            cancellationToken);
        return value.GetProperty("trusted").GetBoolean();
    }

    public Task TrustChatPeerAsync(
        string fingerprint,
        CancellationToken cancellationToken = default)
        => SendVoidAsync("chat.trust", new { fingerprint }, cancellationToken);

    public Task RevokeChatTrustAsync(
        string fingerprint,
        CancellationToken cancellationToken = default)
        => SendVoidAsync("chat.revoke_trust", new { fingerprint }, cancellationToken);

    public Task SendChatFilesAsync(
        string fingerprint,
        IReadOnlyList<string> paths,
        CancellationToken cancellationToken = default)
        => SendVoidAsync(
            "chat.send_files",
            new { fingerprint, paths },
            cancellationToken);

    public Task DeleteChatFileAsync(
        string path,
        CancellationToken cancellationToken = default)
        => SendVoidAsync("chat.delete_file", new { path }, cancellationToken);

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
        await _writeLock.WaitAsync(cancellationToken);
        try
        {
            await _pipe.WriteAsync(header, cancellationToken);
            await _pipe.WriteAsync(data, cancellationToken);
            await _pipe.FlushAsync(cancellationToken);
        }
        finally
        {
            _writeLock.Release();
        }
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
        else if (name is "transfer.event" or "transfer.incoming")
        {
            TransferEventReceived?.Invoke(
                this,
                new TransferEventEventArgs
                {
                    Payload = root.GetProperty("payload").Clone()
                });
        }
        else if (name == "chat.changed")
        {
            ChatChanged?.Invoke(this, EventArgs.Empty);
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
        Alias = NullableString(value, "alias") ?? "rtools",
        Fingerprint = NullableString(value, "fingerprint") ?? "",
        ReceiveDir = NullableString(value, "receive_dir") ?? "",
        PinConfigured = value.TryGetProperty("pin_configured", out var pin) && pin.GetBoolean(),
        AutoAcceptFavorites = value.TryGetProperty("auto_accept_favorites", out var auto) &&
            auto.GetBoolean(),
        Error = NullableString(value, "error")
    };

    private static List<string> ParseCsvLine(string line)
    {
        var values = new List<string>();
        var current = new StringBuilder();
        var quoted = false;
        for (var index = 0; index < line.Length; index++)
        {
            var ch = line[index];
            if (ch == '"')
            {
                if (quoted && index + 1 < line.Length && line[index + 1] == '"')
                {
                    current.Append('"');
                    index++;
                }
                else
                {
                    quoted = !quoted;
                }
            }
            else if (ch == ',' && !quoted)
            {
                values.Add(current.ToString());
                current.Clear();
            }
            else
            {
                current.Append(ch);
            }
        }
        values.Add(current.ToString());
        return values;
    }

    private static string Csv(string value)
        => $"\"{value.Replace("\"", "\"\"", StringComparison.Ordinal)}\"";

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
        _writeLock.Dispose();
    }
}

public sealed class EngineRpcException(string code, string message) : Exception(message)
{
    public string Code { get; } = code;
}

