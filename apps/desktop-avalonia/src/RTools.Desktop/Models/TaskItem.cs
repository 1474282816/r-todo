using CommunityToolkit.Mvvm.ComponentModel;
using System.Text.Json.Serialization;

namespace RTools.Desktop.Models;

[JsonConverter(typeof(JsonStringEnumConverter<TaskPriority>))]
public enum TaskPriority
{
    [JsonStringEnumMemberName("high")]
    High,
    [JsonStringEnumMemberName("medium")]
    Medium,
    [JsonStringEnumMemberName("low")]
    Low
}

public sealed partial class TaskItem : ObservableObject
{
    public required string Id { get; init; }

    [ObservableProperty]
    private string _title = "";

    [ObservableProperty]
    private TaskPriority _priority = TaskPriority.Medium;

    [ObservableProperty]
    private DateTimeOffset? _remindAt;

    public string RemindAtText
    {
        get => RemindAt?.LocalDateTime.ToString("yyyy-MM-dd HH:mm") ?? "";
        set
        {
            if (string.IsNullOrWhiteSpace(value))
            {
                RemindAt = null;
            }
            else if (DateTimeOffset.TryParse(value, out var parsed))
            {
                RemindAt = parsed;
            }
            OnPropertyChanged();
        }
    }

    [ObservableProperty]
    private int _remindOffsetMinutes = 15;

    [ObservableProperty]
    private string? _repeatRule;

    [ObservableProperty]
    private double _sortOrder;

    [ObservableProperty]
    private DateTimeOffset? _completedAt;

    [ObservableProperty]
    private DateTimeOffset? _deletedAt;

    public required DateTimeOffset CreatedAt { get; init; }

    [ObservableProperty]
    private DateTimeOffset _updatedAt;

    [ObservableProperty]
    private long _revision;

    [ObservableProperty]
    private bool _isDragging;

    [ObservableProperty]
    private bool _isDeleteConfirming;

    public bool IsCompleted => CompletedAt is not null;

    public bool IsVisible => DeletedAt is null;

    public string PriorityLabel => Priority switch
    {
        TaskPriority.High => "高",
        TaskPriority.Medium => "中",
        _ => "低"
    };

    public bool IsHighPriority => Priority == TaskPriority.High;

    public bool IsMediumPriority => Priority == TaskPriority.Medium;

    public bool IsLowPriority => Priority == TaskPriority.Low;

    public string ReminderLabel => RemindAt is null
        ? ""
        : RemindAt.Value.LocalDateTime.ToString("MM-dd HH:mm");

    partial void OnPriorityChanged(TaskPriority value)
    {
        OnPropertyChanged(nameof(PriorityLabel));
        OnPropertyChanged(nameof(IsHighPriority));
        OnPropertyChanged(nameof(IsMediumPriority));
        OnPropertyChanged(nameof(IsLowPriority));
    }

    partial void OnRemindAtChanged(DateTimeOffset? value)
    {
        OnPropertyChanged(nameof(ReminderLabel));
    }

    public TaskItem Clone()
    {
        return new TaskItem
        {
            Id = Id,
            Title = Title,
            Priority = Priority,
            RemindAt = RemindAt,
            RemindOffsetMinutes = RemindOffsetMinutes,
            RepeatRule = RepeatRule,
            SortOrder = SortOrder,
            CompletedAt = CompletedAt,
            DeletedAt = DeletedAt,
            CreatedAt = CreatedAt,
            UpdatedAt = UpdatedAt,
            Revision = Revision
        };
    }
}

