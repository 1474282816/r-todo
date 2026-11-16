using Avalonia;
using Avalonia.Threading;
using Avalonia.Controls.ApplicationLifetimes;
using Avalonia.Markup.Xaml;
using Microsoft.Extensions.DependencyInjection;
using RTools.Desktop.Services;
using RTools.Desktop.ViewModels;

namespace RTools.Desktop;

public partial class App : Application
{
    public override void Initialize()
    {
        AvaloniaXamlLoader.Load(this);
    }

    public override void OnFrameworkInitializationCompleted()
    {
        if (ApplicationLifetime is IClassicDesktopStyleApplicationLifetime desktop)
        {
            var services = new ServiceCollection();
            services.AddSingleton<IEngineClient, EngineProcessClient>();
            services.AddSingleton<MainWindowViewModel>();
            var provider = services.BuildServiceProvider();
            var viewModel = provider.GetRequiredService<MainWindowViewModel>();
            var window = new MainWindow { DataContext = viewModel };
            desktop.MainWindow = window;
            TrayIconService.Install(window, viewModel);
            Dispatcher.UIThread.Post(
                async () => await viewModel.InitializeAsync(),
                DispatcherPriority.Background);
            desktop.ShutdownRequested += async (_, _) => await viewModel.DisposeAsync();
        }

        base.OnFrameworkInitializationCompleted();
    }
}
