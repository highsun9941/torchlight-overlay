using System.Diagnostics;
using System.IO;
using System.Text;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Threading;
using Tartarus.Vision;

namespace Tartarus.App;

public partial class ControlWindow
{
    private Process? _vorax;
    private bool _companionReady;
    private GameContent _activeContent;
    private bool _gameAvailable;
    private readonly DispatcherTimer _companionTimer = new() { Interval = TimeSpan.FromMilliseconds(250) };
    private DateTime _voraxStatusTime;
    private static string VoraxData => Path.Combine(AppContext.BaseDirectory, "user-data", "vorax");
    private static bool PreviewMode => Environment.GetCommandLineArgs().Contains("--preview-ui");

    private void InitializeCompanion()
    {
        LanguageCombo.SelectedIndex = _settings.GameLanguage == "en" ? 1 : 0;
        ShelleyCombo.SelectedIndex = Math.Clamp(_settings.ShelleyLevel, 0, 6);
        VoraxQualityCombo.SelectedIndex = _settings.VoraxFutures <= 256 ? 0 : _settings.VoraxFutures <= 512 ? 1 : 2;
        VoraxCpuCombo.Items.Add($"게임 우선 / {CpuThreads(0)}개 스레드");
        VoraxCpuCombo.Items.Add($"균형 / {CpuThreads(1)}개 스레드");
        VoraxCpuCombo.Items.Add($"추천 속도 우선 / {CpuThreads(2)}개 스레드");
        VoraxCpuCombo.SelectedIndex = Math.Clamp(_settings.VoraxCpuMode, 0, 2);
        _settings.VoraxThreads = CpuThreads(VoraxCpuCombo.SelectedIndex);
        AutoContentCheck.IsChecked = _settings.AutoDetectContent;
        _companionTimer.Tick += (_, _) => { UpdateAutomaticContent(); ReadVoraxStatus(); };
        _companionTimer.Start();
        Loaded += (_, _) =>
        {
            _companionReady = true;
            if (PreviewMode) return;
            AutoScanCheck.IsChecked = false;
            AutoAttachCheck.IsChecked = false;
            if (_settings.AutoDetectContent) TryAutoAttach();
            else ActivateContent(ModeTabs.SelectedIndex == 1 ? GameContent.Vorax : GameContent.Tartarus);
        };
        if (PreviewMode) AutoAttachCheck.IsChecked = false;
    }

    internal async Task ExportUiPreview(string directory)
    {
        Directory.CreateDirectory(directory);
        for (int tab = 0; tab < 2; tab++)
        {
            ModeTabs.SelectedIndex = tab;
            await Dispatcher.InvokeAsync(() => UpdateLayout(), DispatcherPriority.ContextIdle);
            var content = (FrameworkElement)Content;
            var width = (int)content.ActualWidth + 44;
            var height = (int)content.ActualHeight + 44;
            var visual = new System.Windows.Media.DrawingVisual();
            using (var drawing = visual.RenderOpen())
            {
                drawing.DrawRectangle(Background, null, new Rect(0, 0, width, height));
                drawing.DrawRectangle(new System.Windows.Media.VisualBrush(content), null, new Rect(22, 22, content.ActualWidth, content.ActualHeight));
            }
            var bitmap = new System.Windows.Media.Imaging.RenderTargetBitmap(width, height, 96, 96, System.Windows.Media.PixelFormats.Pbgra32);
            bitmap.Render(visual);
            var encoder = new System.Windows.Media.Imaging.PngBitmapEncoder();
            encoder.Frames.Add(System.Windows.Media.Imaging.BitmapFrame.Create(bitmap));
            using var stream = File.Create(Path.Combine(directory, tab == 0 ? "tartarus-ui.png" : "vorax-ui.png"));
            encoder.Save(stream);
        }
    }

    private void ModeTabs_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        if (!_companionReady || e.Source != ModeTabs || PreviewMode) return;
        // In automatic mode tabs are settings pages; viewing one must not change the running engine.
        if (!_settings.AutoDetectContent)
            ActivateContent(ModeTabs.SelectedIndex == 1 ? GameContent.Vorax : GameContent.Tartarus);
    }

    private void AutoContent_Changed(object sender, RoutedEventArgs e)
    {
        if (!_companionReady || PreviewMode) return;
        _settings.AutoDetectContent = AutoContentCheck.IsChecked == true;
        _settings.Save();
        if (_settings.AutoDetectContent) TryAutoAttach();
        else ActivateContent(ModeTabs.SelectedIndex == 1 ? GameContent.Vorax : GameContent.Tartarus);
        ShowContentStatus();
    }

    private void UpdateAutomaticContent()
    {
        if (!_companionReady || PreviewMode || !_settings.AutoDetectContent) return;
        ActivateContent(_gameAvailable ? _gameLog?.Content ?? GameContent.None : GameContent.None);
        ShowContentStatus();
    }

    private void ShowContentStatus()
    {
        string name = _activeContent switch { GameContent.Tartarus => "타르타로스", GameContent.Vorax => "블러드 러스트", _ => "대기" };
        ContentStatus.Text = _settings.AutoDetectContent
            ? $"자동 감지: {name}" + (_activeContent == GameContent.None ? " / 게임 로그를 켜고 콘텐츠에 입장하세요." : " / 탭은 설정 보기용입니다.")
            : $"수동 선택: {name} / 탭을 바꾸면 전환됩니다.";
    }

    private void ActivateContent(GameContent content)
    {
        if (_activeContent == content) return;
        _activeContent = content;
        AutoScanCheck.IsChecked = false;
        AutoAttachCheck.IsChecked = false;
        _hints.Clear();
        ClearHint();
        StopCapture();
        StopVorax();
        _frame = null;
        _scan = null;
        _sandbox = _scannedSandbox = _lastRead = null;
        RefreshOverlay();
        if (_settings.AutoDetectContent && content != GameContent.None)
            ModeTabs.SelectedIndex = content == GameContent.Vorax ? 1 : 0;
        if (content == GameContent.Vorax)
        {
            StartVorax();
        }
        else if (content == GameContent.Tartarus)
        {
            AutoAttachCheck.IsChecked = true;
            AutoScanCheck.IsChecked = true;
            TryAutoAttach();
        }
        Log($"Content engine: {content} (automatic={_settings.AutoDetectContent})");
        ShowContentStatus();
    }

    private void CompanionSetting_Changed(object sender, SelectionChangedEventArgs e)
    {
        if (!_companionReady) return;
        _settings.GameLanguage = LanguageCombo.SelectedIndex == 1 ? "en" : "ko";
        _settings.ShelleyLevel = ShelleyCombo.SelectedIndex;
        _settings.VoraxFutures = new[] { 256, 512, 1024 }[Math.Clamp(VoraxQualityCombo.SelectedIndex, 0, 2)];
        _settings.VoraxCpuMode = Math.Clamp(VoraxCpuCombo.SelectedIndex, 0, 2);
        _settings.VoraxThreads = CpuThreads(_settings.VoraxCpuMode);
        _settings.Save();
        CheckGameLanguage();
        if (_activeContent == GameContent.Vorax && _vorax is { HasExited: false })
        {
            StopVorax();
            StartVorax();
        }
    }

    private static int CpuThreads(int mode) => mode switch
    {
        0 => Math.Clamp(Environment.ProcessorCount / 4, 1, 4),
        2 => Math.Clamp(Environment.ProcessorCount * 3 / 4, 1, 12),
        _ => Math.Clamp(Environment.ProcessorCount / 2, 1, 8)
    };

    private void StartVorax()
    {
        if (PreviewMode || _vorax is { HasExited: false }) return;
        var exe = Path.Combine(AppContext.BaseDirectory, "vorax-overlay.exe");
        if (!File.Exists(exe))
        {
            VoraxStatus.Text = "vorax-overlay.exe가 없습니다. 배포 폴더 전체를 사용하세요.";
            return;
        }
        try
        {
            Directory.CreateDirectory(VoraxData);
            // Discard our own previous IPC files, never the game's files.
            File.Delete(Path.Combine(VoraxData, "status.json"));
            File.Delete(Path.Combine(VoraxData, "command.txt"));
            var start = new ProcessStartInfo(exe) { UseShellExecute = false, CreateNoWindow = true, WorkingDirectory = AppContext.BaseDirectory };
            start.ArgumentList.Add("--shelley");
            start.ArgumentList.Add(_settings.ShelleyLevel.ToString());
            if (!string.IsNullOrEmpty(_gameLogPath))
            {
                start.ArgumentList.Add("--log");
                start.ArgumentList.Add(_gameLogPath);
            }
            start.Environment["VORAX_DATA_DIR"] = VoraxData;
            start.Environment["VORAX_LANGUAGE"] = _settings.GameLanguage ?? "ko";
            start.Environment["VORAX_FUTURES"] = _settings.VoraxFutures.ToString();
            start.Environment["RAYON_NUM_THREADS"] = Math.Clamp(_settings.VoraxThreads, 1, Environment.ProcessorCount).ToString();
            _vorax?.Dispose();
            _vorax = Process.Start(start);
            _voraxStatusTime = DateTime.MinValue;
            VoraxMove.Text = "수술 준비 화면 대기";
            VoraxDetail.Text = "카드가 멈춘 후 자동으로 인식합니다.";
            VoraxToggle.Content = "일시정지";
            VoraxStatus.Text = $"연결 중 / {_settings.VoraxFutures:N0}회 탐색 / {_settings.VoraxThreads}개 계산 스레드";
        }
        catch (Exception ex) { VoraxStatus.Text = "실행 실패: " + ex.Message; }
    }

    private void StopVorax()
    {
        if (_vorax == null) return;
        try { if (!_vorax.HasExited) _vorax.Kill(entireProcessTree: true); }
        catch (InvalidOperationException) { }
        finally { _vorax.Dispose(); _vorax = null; }
        VoraxToggle.Content = "다시 시작";
        VoraxMove.Text = "일시정지";
        VoraxDetail.Text = "다시 시작하면 현재 화면을 읽습니다.";
        VoraxStatus.Text = "블러드 러스트 인식 정지";
    }

    private void ReadVoraxStatus()
    {
        if (_vorax == null) return;
        if (_vorax.HasExited)
        {
            VoraxStatus.Text = $"엔진 종료 (코드 {_vorax.ExitCode}) / 다시 시작을 누르세요.";
            VoraxMove.Text = "추천 중지";
            VoraxToggle.Content = "다시 시작";
            _vorax.Dispose();
            _vorax = null;
            return;
        }
        var path = Path.Combine(VoraxData, "status.json");
        try
        {
            var stamp = File.GetLastWriteTimeUtc(path);
            if (stamp == _voraxStatusTime || !File.Exists(path)) return;
            using var data = JsonDocument.Parse(File.ReadAllText(path, Encoding.UTF8));
            var doc = data.RootElement;
            VoraxMove.Text = doc.GetProperty("title").GetString();
            VoraxDetail.Text = doc.GetProperty("detail").GetString();
            VoraxStatus.Text = doc.GetProperty("status").GetString();
            _voraxStatusTime = stamp;
        }
        catch (IOException) { } // Writer may be replacing the IPC snapshot.
        catch (JsonException) { } // A complete snapshot will be read on the next tick.
    }

    private void VoraxSnap_Click(object sender, RoutedEventArgs e)
    {
        if (_activeContent != GameContent.Vorax) return;
        StartVorax();
        if (_vorax is { HasExited: false })
            File.WriteAllText(Path.Combine(VoraxData, "command.txt"), "scan", new UTF8Encoding(false));
    }

    private void VoraxToggle_Click(object sender, RoutedEventArgs e)
    {
        if (_activeContent != GameContent.Vorax) return;
        if (_vorax is { HasExited: false }) StopVorax(); else StartVorax();
    }

    private void VoraxLog_Click(object sender, RoutedEventArgs e)
    {
        PickLogButton_Click(sender, e);
        if (_vorax is { HasExited: false }) { StopVorax(); StartVorax(); }
    }
}
