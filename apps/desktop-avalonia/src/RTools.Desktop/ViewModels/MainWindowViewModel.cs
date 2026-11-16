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
        Transfer = new TransferViewModel(engine);
        _engine.TasksChanged += OnTasksChanged;
    }

    public ObservableCollection<TaskItem> ActiveTasks { get; } = [];
    public ObservableCollection<TaskItem> CompletedTasks { get; } = [];
    public SettingsViewModel Settings { get; }
    public TransferViewModel Transfer { get; }
    public IEngineClient Engine => _engine;

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
            var saved = await _engine.UpdateTaskAsync(task);
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
    private void OpenMonth() => IsMonthOpen = true;

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

    public async ValueTask DisposeAsync()
    {
        _engine.TasksChanged -= OnTasksChanged;
        await _engine.DisposeAsync();
    }
}

