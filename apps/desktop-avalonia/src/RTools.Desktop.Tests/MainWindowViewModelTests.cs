using RTools.Desktop.Models;
using RTools.Desktop.Services;
using RTools.Desktop.ViewModels;

namespace RTools.Desktop.Tests;

public sealed class MainWindowViewModelTests
{
    [Fact]
    public async Task CreateTask_AddsTaskAndPersistsThroughEngine()
    {
        var engine = new InMemoryEngineClient();
        await engine.StartAsync();
        await using var vm = new MainWindowViewModel(engine);
        await vm.InitializeAsync();
        vm.NewTaskTitle = "测试任务";
        vm.SetHighPriorityCommand.Execute(null);

        await vm.AddTaskCommand.ExecuteAsync(null);

        Assert.Single(vm.ActiveTasks);
        Assert.Equal("测试任务", vm.ActiveTasks[0].Title);
        Assert.Equal(TaskPriority.High, vm.ActiveTasks[0].Priority);
        Assert.Equal("rtools (1/1)", vm.Title);
    }

    [Fact]
    public async Task CompleteTask_MovesTaskToCompletedList()
    {
        var engine = new InMemoryEngineClient();
        await engine.StartAsync();
        await using var vm = new MainWindowViewModel(engine);
        await vm.InitializeAsync();
        vm.NewTaskTitle = "完成我";
        await vm.AddTaskCommand.ExecuteAsync(null);
        var task = vm.ActiveTasks.Single();

        await vm.ToggleCompleteCommand.ExecuteAsync(task);

        Assert.Empty(vm.ActiveTasks);
        Assert.Single(vm.CompletedTasks);
        Assert.Equal("完成我", vm.CompletedTasks[0].Title);
    }

    [Fact]
    public async Task DeleteTask_RemovesTask()
    {
        var engine = new InMemoryEngineClient();
        await engine.StartAsync();
        await using var vm = new MainWindowViewModel(engine);
        await vm.InitializeAsync();
        vm.NewTaskTitle = "删除我";
        await vm.AddTaskCommand.ExecuteAsync(null);

        await vm.DeleteTaskCommand.ExecuteAsync(vm.ActiveTasks.Single());

        Assert.Empty(vm.ActiveTasks);
        Assert.Empty(vm.CompletedTasks);
    }

    [Fact]
    public async Task EmptyTitle_DoesNotCreateTask()
    {
        var engine = new InMemoryEngineClient();
        await engine.StartAsync();
        await using var vm = new MainWindowViewModel(engine);
        await vm.InitializeAsync();
        vm.NewTaskTitle = "   ";

        await vm.AddTaskCommand.ExecuteAsync(null);

        Assert.Empty(vm.ActiveTasks);
        Assert.Equal("请输入任务标题", vm.ErrorMessage);
    }

    [Fact]
    public async Task Navigation_ShowsOnlyOneWorkspacePage()
    {
        var engine = new InMemoryEngineClient();
        await engine.StartAsync();
        await using var vm = new MainWindowViewModel(engine);
        await vm.InitializeAsync();

        await vm.OpenMonthCommand.ExecuteAsync(null);
        Assert.True(vm.IsMonthOpen);
        Assert.False(vm.IsSettingsOpen);
        Assert.False(vm.IsTransferOpen);

        await vm.OpenSettingsCommand.ExecuteAsync(null);
        Assert.False(vm.IsMonthOpen);
        Assert.True(vm.IsSettingsOpen);
        Assert.False(vm.IsTransferOpen);

        await vm.OpenTransferCommand.ExecuteAsync(null);
        Assert.False(vm.IsMonthOpen);
        Assert.False(vm.IsSettingsOpen);
        Assert.True(vm.IsTransferOpen);

        vm.ShowTasksCommand.Execute(null);
        Assert.True(vm.IsTasksPage);
    }

    [Fact]
    public async Task ImportJson_PreservesCompletionAndSortOrder()
    {
        var path = Path.GetTempFileName();
        try
        {
            await File.WriteAllTextAsync(
                path,
                """
                {
                  "format": "r-todo/todo",
                  "version": 1,
                  "todos": [
                    {
                      "id": "legacy-1",
                      "title": "旧任务",
                      "priority": "high",
                      "remind_at": null,
                      "remind_offset_min": 15,
                      "repeat_rule": null,
                      "sort_order": 100,
                      "deleted": false,
                      "completed_at": "{{TODAY}}",
                      "created_at": "{{YESTERDAY}}",
                      "updated_at": "{{TODAY}}"
                    }
                  ]
                }
                """.Replace(
                    "{{TODAY}}",
                    DateTimeOffset.Now.ToString("O"),
                    StringComparison.Ordinal)
                    .Replace(
                        "{{YESTERDAY}}",
                        DateTimeOffset.Now.AddDays(-1).ToString("O"),
                        StringComparison.Ordinal));
            var engine = new InMemoryEngineClient();
            await engine.StartAsync();
            await using var vm = new MainWindowViewModel(engine);
            await vm.InitializeAsync();

            await vm.ImportJsonAsync(path);

            Assert.Single(vm.CompletedTasks);
            Assert.Equal("旧任务", vm.CompletedTasks[0].Title);
            Assert.Equal(TaskPriority.High, vm.CompletedTasks[0].Priority);
        }
        finally
        {
            File.Delete(path);
        }
    }
}

