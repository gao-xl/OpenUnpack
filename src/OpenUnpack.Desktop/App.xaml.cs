using System.Security.Cryptography;
using System.Text;
using System.Windows;

namespace OpenUnpack.Desktop;

public partial class App : Application
{
    private Mutex? _instance;
    private bool _ownsMutex;
    protected override void OnStartup(StartupEventArgs e)
    {
        base.OnStartup(e);
        string? previewPath = null;
        for (var i = 0; i < e.Args.Length - 1; i++)
            if (e.Args[i] == "--render-preview") previewPath = Path.GetFullPath(e.Args[i + 1]);
        var dataDirectory = previewPath is not null
            ? Path.Combine(Path.GetTempPath(), "OpenUnpack-preview-" + Guid.NewGuid().ToString("N"))
            : Environment.GetEnvironmentVariable("OPENUNPACK_DATA_DIR")
                ?? Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "OpenUnpack");
        dataDirectory = Path.GetFullPath(dataDirectory);
        var suffix = Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(dataDirectory.ToUpperInvariant())))[..24];
        _instance = new Mutex(true, @"Local\OpenUnpack-" + suffix, out _ownsMutex);
        if (!_ownsMutex)
        {
            MessageBox.Show("OpenUnpack 已经在使用这个本地数据目录。请回到已打开的窗口。", "OpenUnpack", MessageBoxButton.OK, MessageBoxImage.Information);
            Shutdown();
            return;
        }
        var window = new MainWindow(dataDirectory, previewPath);
        MainWindow = window;
        window.Show();
    }
    protected override void OnExit(ExitEventArgs e)
    {
        if (_ownsMutex) _instance?.ReleaseMutex();
        _instance?.Dispose();
        base.OnExit(e);
    }
}
