using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using RTools.Desktop.Models;
using RTools.Desktop.Services;
using System.Collections.ObjectModel;

namespace RTools.Desktop.ViewModels;

public sealed partial class MainWindowViewModel : ObservableObject, IAsyncDisposable
{
    private readonly IEngineClient _engine;
    private bool _loading;

    public MainWindowViewModel(IEngineClient engine)
    {
        _engine = engine;
        Settings = new SettingsViewModel(engine);
        Settings.SyncCompleted += async (_, _) => await ReloadSafeAsync();
        Transfer = new TransferViewModel(engine);
        _engine.TasksChanged += OnTasksChanged;
        _engine.ReminderDue += OnReminderDue;
    }

    public ObservableCollection<TaskItem> ActiveTasks { get; } = [];
    public ObservableCollection<TaskItem> CompletedTasks { get; } = [];
    public SettingsViewModel Settings { get; }
    public TransferViewModel Transfer { get; }
    public IEngineClient Engine => _engine;
    public event EventHandler<ReminderDueEventArgs>? ReminderRaised;

    [ObservableProperty]
    private string _newTaskTitle = "";

    [ObservableProperty]
    private TaskPriority _newTaskPriority = TaskPriority.Medium;

    [ObservableProperty]
    private TaskItem? _selectedTask;

    [ObservableProperty]
    private int _selectedTaskPriorityIndex;

    [ObservableProperty]
    private bool _isSettingsOpen;

    [ObservableProperty]
    private bool _isMonthOpen;

    [ObservableProperty]
    private bool _isTransferOpen;

    [ObservableProperty]
    private string? _errorMessage;

    [ObservableProperty]
    private string _engineStatus = "正在连接 Engine...";

    [ObservableProperty]
    private string _syncSummary = "未连接同步";

    public int ActiveCount => ActiveTasks.Count;
    public int VisibleCount => ActiveTasks.Count + CompletedTasks.Count;
    public bool IsEmpty => !_loading && VisibleCount == 0;

    public string Title => $"rtools ({ActiveCount}/{VisibleCount})";

    public async Task InitializeAsync(CancellationToken cancellationToken = default)
    {
        _loading = true;
        try
        {
            await _engine.StartAsync(cancellationToken);
            EngineStatus = "Engine 已连接";
            await ReloadAsync(cancellationToken);
            var sync = await _engine.GetSyncStatusAsync(cancellationToken);
            SyncSummary = sync.Paired
                ? sync.LastSyncAt is { } last
                    ? $"已同步 {last.LocalDateTime:HH:mm:ss}"
                    : "已配对"
                : "未连接同步";
            _ = SyncInBackgroundAsync();
        }
        catch (Exception ex)
        {
            ErrorMessage = ex.Message;
            EngineStatus = "Engine 连接失败";
        }
        finally
        {
            _loading = false;
            RaiseCounts();
        }
    }

    private async Task SyncInBackgroundAsync()
    {
        try
        {
            var sync = await _engine.SyncNowAsync();
            SyncSummary = sync.Paired
                ? sync.LastSyncAt is { } last
                    ? $"已同步 {last.LocalDateTime:HH:mm:ss}"
                    : "已配对"
                : "未连接同步";
            await ReloadSafeAsync();
        }
        catch (Exception ex)
        {
            ErrorMessage = ex.Message;
            SyncSummary = "同步暂不可用";
        }
    }

    [RelayCommand]
    private async Task AddTaskAsync()
    {
        var title = NewTaskTitle.Trim();
        if (title.Length == 0)
        {
            ErrorMessage = "请输入任务标题";
            return;
        }
        try
        {
            ErrorMessage = null;
            var task = await _engine.CreateTaskAsync(
                title,
                NewTaskPriority,
                remindAt: null,
                remindOffsetMinutes: 15,
                repeatRule: null);
            await ReloadAsync();
            NewTaskTitle = "";
            RaiseCounts();
        }
        catch (Exception ex)
        {
            ErrorMessage = ex.Message;
        }
    }

    [RelayCommand]
    private async Task ToggleCompleteAsync(TaskItem? task)
    {
        if (task is null)
        {
            return;
        }
        try
        {
            ErrorMessage = null;
            var completed = await _engine.CompleteTaskAsync(task.Id, !task.IsCompleted);
            await ReloadAsync();
        }
        catch (Exception ex)
        {
            ErrorMessage = ex.Message;
        }
    }

    [RelayCommand]
    private async Task DeleteTaskAsync(TaskItem? task)
    {
        if (task is null)
        {
            return;
        }
        try
        {
            ErrorMessage = null;
            await _engine.DeleteTaskAsync(task.Id);
            await ReloadAsync();
            RaiseCounts();
        }
        catch (Exception ex)
        {
            ErrorMessage = ex.Message;
        }
    }

    [RelayCommand]
    private async Task SaveTaskAsync(TaskItem? task)
    {
        if (task is null)
        {
            return;
        }
        try
        {
            ErrorMessage = null;
            await _engine.UpdateTaskAsync(task);
            await ReloadAsync();
            SelectedTask = null;
        }
        catch (Exception ex)
        {
            ErrorMessage = ex.Message;
        }
    }

    [RelayCommand]
    private async Task OpenSettings()
    {
        await Settings.LoadAsync();
        IsSettingsOpen = true;
    }

    [RelayCommand]
    private void CloseSettings() => IsSettingsOpen = false;

    [RelayCommand]
    private async Task OpenMonth()
    {
        await LoadMonthAsync();
        IsMonthOpen = true;
    }

    [RelayCommand]
    private void CloseMonth() => IsMonthOpen = false;

    [RelayCommand]
    private async Task OpenTransfer()
    {
        await Transfer.LoadAsync();
        IsTransferOpen = true;
    }

    [RelayCommand]
    private void CloseTransfer() => IsTransferOpen = false;

    [RelayCommand]
    private void EditTask(TaskItem? task)
    {
        SelectedTask = task?.Clone();
        SelectedTaskPriorityIndex = SelectedTask?.Priority switch
        {
            TaskPriority.High => 0,
            TaskPriority.Low => 2,
            _ => 1
        };
    }

    partial void OnSelectedTaskChanged(TaskItem? value)
    {
        if (value is not null)
        {
            value.PropertyChanged += (_, args) =>
            {
                if (args.PropertyName == nameof(TaskItem.RemindAt))
                {
                    value.RemindAtText = value.RemindAt?.LocalDateTime
                        .ToString("yyyy-MM-dd HH:mm") ?? "";
                }
            };
        }
    }

    partial void OnSelectedTaskPriorityIndexChanged(int value)
    {
        if (SelectedTask is null)
        {
            return;
        }
        SelectedTask.Priority = value switch
        {
            0 => TaskPriority.High,
            2 => TaskPriority.Low,
            _ => TaskPriority.Medium
        };
    }

    [RelayCommand]
    private void CloseEditor() => SelectedTask = null;

    [RelayCommand]
    private void SetHighPriority() => NewTaskPriority = TaskPriority.High;

    [RelayCommand]
    private void SetMediumPriority() => NewTaskPriority = TaskPriority.Medium;

    [RelayCommand]
    private void SetLowPriority() => NewTaskPriority = TaskPriority.Low;

    [ObservableProperty]
    private DateTimeOffset? _selectedMonth = DateTimeOffset.Now;

    [ObservableProperty]
    private ObservableCollection<MonthDayItem> _monthDays = [];

    [ObservableProperty]
    private ObservableCollection<TaskItem> _monthTasks = [];

    [ObservableProperty]
    private string _monthTitle = DateTime.Now.ToString("yyyy 年 M 月");

    [RelayCommand]
    private async Task PreviousMonth()
    {
        SelectedMonth = (SelectedMonth ?? DateTimeOffset.Now).AddMonths(-1);
        await LoadMonthAsync();
    }

    [RelayCommand]
    private async Task NextMonth()
    {
        SelectedMonth = (SelectedMonth ?? DateTimeOffset.Now).AddMonths(1);
        await LoadMonthAsync();
    }

    public async Task LoadMonthAsync()
    {
        var month = SelectedMonth ?? DateTimeOffset.Now;
        var start = new DateTimeOffset(month.Year, month.Month, 1, 0, 0, 0, month.Offset);
        var end = start.AddMonths(1);
        var tasks = await _engine.ListTasksAsync();
        var selected = tasks
            .Where(task => task.DeletedAt is null)
            .Where(task =>
                task.CompletedAt is { } completed &&
                completed >= start &&
                completed < end)
            .OrderByDescending(task => task.CompletedAt)
            .ToList();
        MonthTasks = new ObservableCollection<TaskItem>(selected);
        MonthDays = new ObservableCollection<MonthDayItem>(
            BuildMonthDays(start, selected));
        MonthTitle = $"{start.Year} 年 {start.Month} 月";
    }

    [RelayCommand]
    private async Task ReorderTask(TaskItem? task)
    {
        if (task is null)
        {
            return;
        }
        var index = ActiveTasks.IndexOf(task);
        if (index <= 0)
        {
            return;
        }
        var ids = ActiveTasks.Select(item => item.Id).ToList();
        ids.RemoveAt(index);
        ids.Insert(index - 1, task.Id);
        try
        {
            await _engine.ReorderTasksAsync(ids);
            await ReloadAsync();
        }
        catch (Exception ex)
        {
            ErrorMessage = ex.Message;
        }
    }

    public async Task ReloadAsync(CancellationToken cancellationToken = default)
    {
        var tasks = await _engine.ListTasksAsync(cancellationToken);
        ActiveTasks.Clear();
        CompletedTasks.Clear();
        foreach (var task in tasks
                     .Where(x => x.DeletedAt is null)
                     .Where(x => !IsCompletedBeforeToday(x))
                     .OrderBy(x => x.IsCompleted)
                     .ThenBy(x => x.IsCompleted ? 0 : x.SortOrder)
                     .ThenByDescending(x => x.CompletedAt))
        {
            if (task.IsCompleted)
            {
                CompletedTasks.Add(task);
            }
            else
            {
                ActiveTasks.Add(task);
            }
        }
        RaiseCounts();
    }

    public async Task ImportJsonAsync(string path)
    {
        try
        {
            ErrorMessage = null;
            var count = await _engine.ImportJsonAsync(path);
            await ReloadAsync();
            SyncSummary = $"已导入 {count} 条";
        }
        catch (Exception ex)
        {
            ErrorMessage = ex.Message;
        }
    }

    public async Task ExportJsonAsync(string path)
    {
        try
        {
            ErrorMessage = null;
            await _engine.ExportJsonAsync(path);
            SyncSummary = "已导出 JSON";
        }
        catch (Exception ex)
        {
            ErrorMessage = ex.Message;
        }
    }

    public async Task ExportIcalAsync(string path)
    {
        try
        {
            await _engine.ExportIcalAsync(path);
            SyncSummary = "已导出 iCalendar";
        }
        catch (Exception ex)
        {
            ErrorMessage = ex.Message;
        }
    }

    public async Task ExportCsvAsync(string path)
    {
        try
        {
            await _engine.ExportTickTickCsvAsync(path);
            SyncSummary = "已导出 TickTick CSV";
        }
        catch (Exception ex)
        {
            ErrorMessage = ex.Message;
        }
    }

    public async Task ImportCsvAsync(string path)
    {
        try
        {
            var count = await _engine.ImportTickTickCsvAsync(path);
            await ReloadAsync();
            SyncSummary = $"已从 CSV 导入 {count} 条";
        }
        catch (Exception ex)
        {
            ErrorMessage = ex.Message;
        }
    }

    public async Task SaveEditedTaskAsync()
    {
        if (SelectedTask is null)
        {
            return;
        }
        await SaveTaskAsync(SelectedTask);
    }

    private void OnTasksChanged(object? sender, TaskChangedEventArgs e)
    {
        _ = ReloadSafeAsync();
    }

    private void OnReminderDue(object? sender, ReminderDueEventArgs e)
    {
        ReminderRaised?.Invoke(this, e);
    }

    private async Task ReloadSafeAsync()
    {
        try
        {
            await ReloadAsync();
        }
        catch (Exception ex)
        {
            ErrorMessage = ex.Message;
        }
    }

    private void RaiseCounts()
    {
        OnPropertyChanged(nameof(ActiveCount));
        OnPropertyChanged(nameof(VisibleCount));
        OnPropertyChanged(nameof(IsEmpty));
        OnPropertyChanged(nameof(Title));
    }

    private static bool IsCompletedBeforeToday(TaskItem task)
    {
        if (task.CompletedAt is null)
        {
            return false;
        }
        return task.CompletedAt.Value.LocalDateTime.Date < DateTime.Today;
    }

    private static IReadOnlyList<MonthDayItem> BuildMonthDays(
        DateTimeOffset monthStart,
        IReadOnlyList<TaskItem> completed)
    {
        var firstGrid = monthStart.AddDays(-(int)monthStart.DayOfWeek);
        var result = new List<MonthDayItem>(42);
        for (var offset = 0; offset < 42; offset++)
        {
            var date = firstGrid.AddDays(offset);
            var tasks = completed
                .Where(task => task.CompletedAt is { } completedAt &&
                    completedAt.LocalDateTime.Date == date.LocalDateTime.Date)
                .Select(task => task.Title)
                .ToList();
            result.Add(new MonthDayItem
            {
                Date = date,
                Day = date.Day,
                IsCurrentMonth = date.Month == monthStart.Month,
                IsToday = date.LocalDateTime.Date == DateTime.Today,
                CompletedCount = tasks.Count,
                Summary = string.Join(" · ", tasks.Take(2))
            });
        }
        return result;
    }

    public async ValueTask DisposeAsync()
    {
        _engine.TasksChanged -= OnTasksChanged;
        _engine.ReminderDue -= OnReminderDue;
        await _engine.DisposeAsync();
    }
}

public sealed class MonthDayItem
{
    public required DateTimeOffset Date { get; init; }
    public int Day { get; init; }
    public bool IsCurrentMonth { get; init; }
    public bool IsToday { get; init; }
    public int CompletedCount { get; init; }
    public string Summary { get; init; } = "";
}
