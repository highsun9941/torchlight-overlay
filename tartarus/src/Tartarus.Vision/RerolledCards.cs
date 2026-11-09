namespace Tartarus.Vision;

/// <summary>Withhold instrument hints while the previous offer fades after a logged re-roll.</summary>
public sealed class RerolledCards
{
    private static readonly TimeSpan Limit = TimeSpan.FromSeconds(3);
    private (LoggedRun Run, int Picks, int Used)? _seen;
    private string[]? _lastWholeOffer;
    private DateTime? _clickedAt;

    public bool StillGoing(LoggedRun? run, PickRead pick, DateTime now)
    {
        if (run == null) return false;

        var current = (Run: run, Picks: run.InstrumentPicks, Used: run.RerollsUsed);
        if (_seen is not { } before || before.Run != current.Run || before.Picks != current.Picks)
        {
            _lastWholeOffer = null;
            _clickedAt = null;
        }
        else if (current.Used > before.Used)
        {
            _clickedAt = run.LastRerollAt;
        }
        _seen = current;

        var known = pick.Cards.Where(card => card.Id != null).Select(card => card.Id!).ToArray();
        bool whole = known.Length > 0 && known.Length == pick.Cards.Count;
        // A full offer in a new order is ready at once. Identical offers become eligible at the time limit.
        if (_lastWholeOffer is { } old && _clickedAt is { } click && now >= click && now - click < Limit && known.Length > 0
            && (whole ? known.SequenceEqual(old) : known.All(old.Contains)))
            return true;

        _clickedAt = null;
        if (whole) _lastWholeOffer = known;
        return false;
    }
}
