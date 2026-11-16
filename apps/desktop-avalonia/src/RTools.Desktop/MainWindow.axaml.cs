using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.Interactivity;
using Avalonia.Platform.Storage;
using RTools.Desktop.Interop;
using RTools.Desktop.ViewModels;

namespace RTools.Desktop;

public partial class MainWindow : Window
{
    private DesktopWidgetController? _widget;
    private bool _closing;

    public MainWindow()
    {
        InitializeComponent();
        Closing += OnClosing;
    }

    protected override async void OnOpened(EventArgs e)
    {
        base.OnOpened(e);
        if (DataContext is not MainWindowViewModel viewModel)
        {
            return;
        }
        var engine = viewModel.Engine;
        _widget = new DesktopWidgetController(this, engine);
        try
        {
            var enabled = await engine.GetSettingAsync("desktop_widget") == "true";
            if (enabled)
            {
                await _widget.SetEnabledAsync(true);
            }
        }
        catch (Exception ex)
        {
            viewModel.ErrorMessage = ex.Message;
        }
    }

    private void OnClosing(object? sender, WindowClosingEventArgs e)
    {
        if (_closing)
        {
            return;
        }
        e.Cancel = true;
        Hide();
    }

    public void ExitApplication()
    {
        _closing = true;
        _widget?.Dispose();
        Close();
    }

    private void OnTitleBarPointerPressed(object? sender, PointerPressedEventArgs e)
    {
        if (e.GetCurrentPoint(this).Properties.IsLeftButtonPressed)
        {
            BeginMoveDrag(e);
        }
    }

    private void OnQuickAddKeyDown(object? sender, KeyEventArgs e)
    {
        if (e.Key == Key.Enter && DataContext is MainWindowViewModel viewModel)
        {
            viewModel.AddTaskCommand.Execute(null);
            e.Handled = true;
        }
    }

    private async void OnImportJson(object? sender, RoutedEventArgs e)
    {
        if (DataContext is not MainWindowViewModel viewModel)
        {
            return;
        }
        var files = await StorageProvider.OpenFilePickerAsync(
            new FilePickerOpenOptions
            {
                Title = "导入待办 JSON",
                AllowMultiple = false,
                FileTypeFilter =
                [
                    new FilePickerFileType("r-todo JSON")
                    {
                        Patterns = ["*.json"]
                    }
                ]
            });
        var file = files.FirstOrDefault();
        if (file is null)
        {
            return;
        }
        var path = file.TryGetLocalPath();
        if (path is null)
        {
            viewModel.ErrorMessage = "无法读取所选文件路径";
            return;
        }
        await viewModel.ImportJsonAsync(path);
    }

    private async void OnDesktopWidgetChanged(object? sender, RoutedEventArgs e)
    {
        if (_widget is null ||
            sender is not CheckBox checkBox ||
            DataContext is not MainWindowViewModel viewModel)
        {
            return;
        }
        try
        {
            await _widget.SetEnabledAsync(checkBox.IsChecked == true);
            await viewModel.Settings.SetDesktopWidgetAsync(checkBox.IsChecked == true);
        }
        catch (Exception ex)
        {
            checkBox.IsChecked = _widget.IsEnabled;
            viewModel.ErrorMessage = ex.Message;
        }
    }

    private async void OnExportJson(object? sender, RoutedEventArgs e)
    {
        if (DataContext is not MainWindowViewModel viewModel)
        {
            return;
        }
        var file = await StorageProvider.SaveFilePickerAsync(
            new FilePickerSaveOptions
            {
                Title = "导出待办 JSON",
                SuggestedFileName = $"rtools-{DateTime.Now:yyyy-MM-dd}.json",
                FileTypeChoices =
                [
                    new FilePickerFileType("rtools JSON")
                    {
                        Patterns = ["*.json"]
                    }
                ]
            });
        var path = file?.TryGetLocalPath();
        if (path is not null)
        {
            await viewModel.ExportJsonAsync(path);
        }
    }
}
