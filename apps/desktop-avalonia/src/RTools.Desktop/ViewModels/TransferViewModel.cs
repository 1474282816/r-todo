using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using RTools.Desktop.Services;
using System.Collections.ObjectModel;

namespace RTools.Desktop.ViewModels;

public sealed partial class TransferViewModel : ObservableObject
{
    private readonly IEngineClient _engine;

    public TransferViewModel(IEngineClient engine)
    {
        _engine = engine;
    }

    public ObservableCollection<string> Devices { get; } = [];

    [ObservableProperty]
    private bool _isRunning;

    [ObservableProperty]
    private string _manualIp = "";

    [ObservableProperty]
    private string _chatText = "";

    [ObservableProperty]
    private string? _errorMessage;

    public async Task LoadAsync()
    {
        try
        {
            var status = await _engine.GetTransferStatusAsync();
            IsRunning = status.Running;
            ErrorMessage = status.Error;
        }
        catch (Exception ex)
        {
            ErrorMessage = ex.Message;
        }
    }

    [RelayCommand]
    private async Task ToggleAsync()
    {
        try
        {
            var status = IsRunning
                ? await _engine.StopTransferAsync()
                : await _engine.StartTransferAsync();
            IsRunning = status.Running;
            ErrorMessage = status.Error;
        }
        catch (Exception ex)
        {
            ErrorMessage = ex.Message;
        }
    }

    [RelayCommand]
    private void SendChat()
    {
        if (string.IsNullOrWhiteSpace(ChatText))
        {
            return;
        }
        ChatText = "";
    }
}

