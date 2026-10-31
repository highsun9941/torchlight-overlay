using System.Text.RegularExpressions;

namespace Tartarus.Vision;

public enum GameContent { None, Tartarus, Vorax }

/// <summary>Tracks actual page lifecycle events, not audio, preloads or page-stack dumps.</summary>
public sealed partial class ContentDetector
{
    public GameContent Current { get; private set; }

    public void Reset() => Current = GameContent.None;

    public void Feed(string line)
    {
        if (!line.Contains("[Game]", StringComparison.Ordinal) ||
            (!line.Contains("S13", StringComparison.Ordinal) && !line.Contains("S16", StringComparison.Ordinal))) return;
        var match = PageEvent().Match(line);
        if (!match.Success) match = PageRun().Match(line);
        if (!match.Success) match = DirectRun().Match(line);
        if (!match.Success) return;
        string page = match.Groups["page"].Value;
        if (page.EndsWith("Ctrl", StringComparison.Ordinal)) page = page[..^4];
        string state = match.Groups["state"].Value;
        bool open = state is "OpenView" or "Run";
        if (open)
        {
            if (page == "S13GamePlayMain") Current = GameContent.Vorax;
            else if (page is "S16BoardMain" or "S16BoardThemePop" or "S16BoardTaskPop" or
                     "S16BoardRelicPop" or "S16BoardNewDayPop" or "S16BoardEventPop" or "S16BoardColoredPop")
                Current = GameContent.Tartarus;
            else if ((Current == GameContent.Tartarus && page is "S16GamePlayEntry" or "S16GamePlayCheckOut") ||
                     (Current == GameContent.Vorax && page is "S13GamePlay" or "S13GamePlayReward")) Reset();
        }
        else if ((Current == GameContent.Tartarus && page == "S16BoardMain") ||
                 (Current == GameContent.Vorax && page == "S13GamePlayMain")) Reset();
    }

    [GeneratedRegex(@"Page(?:Base)?@[^\r\n]*?\b(?<state>OpenView|CloseView)\s+(?:\d+_)?(?<page>S(?:13|16)\w+)\b")]
    private static partial Regex PageEvent();
    [GeneratedRegex(@"DispatchPageRunChange PageName\s*=\s*(?<page>S(?:13|16)\w+)\s*,\s*PageRunState\s*=\s*(?<state>Run|Hide|Destory|Destroy)\b")]
    private static partial Regex PageRun();
    [GeneratedRegex(@"\[Game\]\s+(?<page>S(?:13|16)\w+)\s+(?<state>Run|Hide)\b")]
    private static partial Regex DirectRun();
}
