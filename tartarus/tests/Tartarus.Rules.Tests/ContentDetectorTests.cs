using Tartarus.Vision;

namespace Tartarus.Rules.Tests;

public class ContentDetectorTests
{
    private static string Page(string action) => "[2026.10.31-01.00.00:000][120]TLShipping: Display: [Game] Page@ " + action;

    [Theory]
    [InlineData("PageBase@ OpenFlow5! OpenView S13GamePlayMain", GameContent.Vorax)]
    [InlineData("PageBase@ OpenView 26798_S13GamePlayMainCtrl nil 876246", GameContent.Vorax)]
    [InlineData("TipMsgShowMgr@DispatchPageRunChange PageName = S13GamePlayMain , PageRunState = Run", GameContent.Vorax)]
    [InlineData("S13GamePlayMain Run", GameContent.Vorax)]
    [InlineData("Page@ OpenView S16BoardMain", GameContent.Tartarus)]
    [InlineData("Page@ OpenView S16BoardRelicPop", GameContent.Tartarus)]
    [InlineData("S16BoardNewDayPop Run", GameContent.Tartarus)]
    public void RecognizesObservedPageFormats(string message, GameContent expected)
    {
        var detector = new ContentDetector();
        detector.Feed("[Game] " + message);
        Assert.Equal(expected, detector.Current);
    }

    [Fact]
    public void SwitchesBothWaysAndIgnoresOldContentClosing()
    {
        var detector = new ContentDetector();
        detector.Feed(Page("OpenView S16BoardMain"));
        detector.Feed(Page("OpenView S13GamePlayMain"));
        detector.Feed(Page("CloseView 3490_S16BoardMainCtrl"));
        Assert.Equal(GameContent.Vorax, detector.Current);
        detector.Feed(Page("OpenView S16BoardMain"));
        detector.Feed("[Game] S13GamePlayMain Hide");
        Assert.Equal(GameContent.Tartarus, detector.Current);
        detector.Feed(Page("CloseView 3490_S16BoardRelicPopCtrl"));
        Assert.Equal(GameContent.Tartarus, detector.Current);
        detector.Feed(Page("OpenView S16GamePlayCheckOut"));
        Assert.Equal(GameContent.None, detector.Current);
    }

    [Theory]
    [InlineData("PageStack@ PageName = S13GamePlayMain, PageAction = HideLastFuncPage, LifeState = Run")]
    [InlineData("UILevel@ CheckLoadSubLevel S13GamePlayMain S13GamePlay")]
    [InlineData("AudioMgr@PlayAudio UI_S16_Premium_Quality")]
    [InlineData("Page@ OpenView S13GamePlayMainOther")]
    [InlineData("Page@ OpenView S13GamePlay")]
    public void DoesNotActivateFromPreloadsAudioOrEntryScreens(string message)
    {
        var detector = new ContentDetector();
        detector.Feed("[Game] " + message);
        Assert.Equal(GameContent.None, detector.Current);
    }

    [Theory]
    [InlineData("Page@ CloseView 26798_S13GamePlayMainCtrl")]
    [InlineData("TipMsgShowMgr@DispatchPageRunChange PageName = S13GamePlayMain , PageRunState = Destory")]
    [InlineData("S13GamePlayMain Hide")]
    [InlineData("Page@ OpenView S13GamePlayReward")]
    public void ReturnsToIdleOnVoraxExit(string message)
    {
        var detector = new ContentDetector();
        detector.Feed(Page("OpenView S13GamePlayMain"));
        detector.Feed("[Game] " + message);
        Assert.Equal(GameContent.None, detector.Current);
    }

    [Fact]
    public async Task WatcherRestoresExistingContentThenHandlesAppendPartialAndTruncation()
    {
        string dir = Path.Combine(AppContext.BaseDirectory, "torchlight-content-" + Guid.NewGuid());
        Directory.CreateDirectory(dir);
        string path = Path.Combine(dir, "UE_game.log");
        try
        {
            await File.WriteAllTextAsync(path, Page("OpenView S16BoardMain") + "\n" + Page("OpenView S13GamePlayMain") + "\n");
            using var watcher = new GameLogWatcher(path);
            await WaitFor(watcher, GameContent.Vorax);
            await File.AppendAllTextAsync(path, Page("OpenView S16BoardMain"));
            await Task.Delay(650);
            Assert.Equal(GameContent.Vorax, watcher.Content); // no partial-line decisions
            await File.AppendAllTextAsync(path, "\n");
            await WaitFor(watcher, GameContent.Tartarus);
            await File.WriteAllTextAsync(path, "new session\n");
            await WaitFor(watcher, GameContent.None);
            await File.AppendAllTextAsync(path, Page("OpenView S13GamePlayMain") + "\n");
            await WaitFor(watcher, GameContent.Vorax);
            File.Delete(path);
            await WaitFor(watcher, GameContent.None);
        }
        finally
        {
            Assert.StartsWith(Path.GetFullPath(AppContext.BaseDirectory), Path.GetFullPath(dir), StringComparison.OrdinalIgnoreCase);
            Directory.Delete(dir, recursive: true);
        }
    }

    private static async Task WaitFor(GameLogWatcher watcher, GameContent expected)
    {
        for (int i = 0; i < 60 && watcher.Content != expected; i++) await Task.Delay(50);
        Assert.Equal(expected, watcher.Content);
    }
}
