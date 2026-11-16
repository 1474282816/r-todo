using Avalonia;
using Avalonia.Threading;
using Avalonia.Controls;
using Avalonia.Controls.ApplicationLifetimes;
using Avalonia.Markup.Xaml;
using Microsoft.Extensions.DependencyInjection;
using RTools.Desktop.Services;
using RTools.Desktop.ViewModels;
using System.Threading;

namespace RTools.Desktop;

public partial class App : Application
{
    private Mutex? _singleInstance;
    private EventWaitHandle? _activateSignal;
    private RegisteredWaitHandle? _activateWait;

    public override void Initialize()
    {
        AvaloniaXamlLoader.Load(this);
    }

    public override void OnFrameworkInitializationCompleted()
    {
        if (ApplicationLifetime is IClassicDesktopStyleApplicationLifetime desktop)
        {
            var suffix = Environment.UserName.Replace('\\', '_').Replace('/', '_');
            _singleInstance = new Mutex(
                initiallyOwned: true,
                $@"Local\rtools-desktop-single-instance-{suffix}",
                out var createdNew);
            _activateSignal = new EventWaitHandle(
                false,
                EventResetMode.AutoReset,
                $@"Local\rtools-desktop-activate-{suffix}");
            if (!createdNew)
            {
                _activateSignal.Set();
                desktop.Shutdown();
                Environment.Exit(0);
                return;
            }
            var services = new ServiceCollection();
            services.AddSingleton<IEngineClient, EngineProcessClient>();
            services.AddSingleton<MainWindowViewModel>();
            var provider = services.BuildServiceProvider();
            var viewModel = provider.GetRequiredService<MainWindowViewModel>();
            var window = new MainWindow { DataContext = viewModel };
            desktop.MainWindow = window;
            _activateWait = ThreadPool.RegisterWaitForSingleObject(
                _activateSignal,
                (_, _) => Dispatcher.UIThread.Post(() =>
                {
                    window.Show();
                    window.WindowState = WindowState.Normal;
                    window.Activate();
                }),
                null,
                Timeout.Infinite,
                executeOnlyOnce: false);
            TrayIconService.Install(window, viewModel);
            Dispatcher.UIThread.Post(
                async () => await viewModel.InitializeAsync(),
                DispatcherPriority.Background);
            desktop.ShutdownRequested += async (_, _) => await viewModel.DisposeAsync();
        }

        base.OnFrameworkInitializationCompleted();
    }
}
