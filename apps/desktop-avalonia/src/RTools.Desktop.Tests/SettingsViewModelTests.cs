using RTools.Desktop.Services;
using RTools.Desktop.ViewModels;

namespace RTools.Desktop.Tests;

public sealed class SettingsViewModelTests
{
    [Fact]
    public async Task Save_PersistsSettings()
    {
        var engine = new InMemoryEngineClient();
        await engine.StartAsync();
        var vm = new SettingsViewModel(engine)
        {
            IsDesktopWidget = true,
            SelectedMaterial = "Mica",
            Opacity = 0.72,
            CollapseOnSnap = false,
            DefaultReminderOffset = 30
        };

        await vm.SaveCommand.ExecuteAsync(null);
        var loaded = new SettingsViewModel(engine);
        await loaded.LoadAsync();

        Assert.True(loaded.IsDesktopWidget);
        Assert.Equal("Mica", loaded.SelectedMaterial);
        Assert.Equal(0.72, loaded.Opacity, 2);
        Assert.False(loaded.CollapseOnSnap);
        Assert.Equal(30, loaded.DefaultReminderOffset);
    }

    [Fact]
    public async Task McpToggle_GeneratesToken()
    {
        var engine = new InMemoryEngineClient();
        await engine.StartAsync();
        var vm = new SettingsViewModel(engine);
        await vm.ToggleMcpCommand.ExecuteAsync(null);

        Assert.True(vm.McpEnabled);
        Assert.True(vm.McpRunning);
        Assert.False(string.IsNullOrWhiteSpace(vm.McpToken));
    }

    [Fact]
    public async Task TransferToggle_StartsWhenInitiallyStopped()
    {
        var engine = new InMemoryEngineClient();
        await engine.StartAsync();
        var vm = new SettingsViewModel(engine);

        await vm.ToggleTransferCommand.ExecuteAsync(null);

        Assert.True(vm.TransferRunning);
        Assert.True((await engine.GetTransferStatusAsync()).Running);
    }
}

