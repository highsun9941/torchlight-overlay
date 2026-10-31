using System.Globalization;
using System.Windows;

namespace Tartarus.App;

/// <summary>
/// The player's runs counted from the game's log: how many and their Chromatic Inmates on show; their time is kept too
/// (statistic.json, the app's log), not shown yet.
/// </summary>
public partial class ControlWindow
{
    private readonly RunStatistics _stats = RunStatistics.Load();

    /// <summary>Counts the runs of the log that ended since: the current one, or ones played while the app was closed.</summary>
    private void CountRuns()
    {
        if (_gameLog is not { } watcher) return;
        var counted = new List<Tartarus.Vision.LoggedRun>();
        lock (watcher.Log)
            foreach (var run in watcher.Log.Runs)
                if (_stats.Add(run)) counted.Add(run);
        foreach (var run in counted)
            Log($"Statistics: a run counted — Chromatic {run.ChromaticAtLeast}, {Duration(run.End!.Value - run.Start)}");
        if (counted.Count > 0) ShowStats();
    }

    private void ShowStats()
    {
        var s = _stats;
        StatsText.Text = s.Runs == 0
            ? "완주 기록 대기 / 시작부터 폭동까지 로그에 기록된 판을 집계합니다."
            : $"완주 {s.Runs}회 / 찬란 {s.Chromatic}개 / 평균 {s.ChromaticPerRun:0.00}개";
        StatsText.ToolTip = $"{s.Since.ToLocalTime():yyyy-MM-dd HH:mm}부터 / 시작부터 폭동까지 로그에 기록된 판만 집계합니다.";
    }

    /// <summary>"1:02:03", or "12:34" under an hour.</summary>
    private static string Duration(TimeSpan t) =>
        t.TotalHours >= 1 ? $"{(int)t.TotalHours}:{t.Minutes:00}:{t.Seconds:00}" : $"{t.Minutes}:{t.Seconds:00}";

    private void ResetStatsButton_Click(object sender, RoutedEventArgs e)
    {
        if (MessageBox.Show(this, $"기록({_stats.Runs}회, 찬란 {_stats.Chromatic}개)을 초기화할까요?",
                "통계 초기화", MessageBoxButton.YesNo, MessageBoxImage.Question) != MessageBoxResult.Yes) return;
        _stats.Reset();
        Log("Statistics reset");
        ShowStats();
    }
}
