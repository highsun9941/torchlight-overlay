using R = Tartarus.Rules;

namespace Tartarus.Vision;

/// <summary>
/// What a run has gathered, kept from scan to scan, for the reads a tooltip, a fade or an animation can spoil: the
/// Torture Instruments held and the Chromatic count. Both only ever grow within a run. A new run (Remaining Days
/// going up) starts over. Also the run's theme, which shows once, in the lobby before the run.
/// </summary>
public sealed class RunMemory
{
    private readonly List<string> _held = [];
    private readonly HashSet<int> _kept = [];
    private int? _lastDays;
    private int _chromatic;
    private int? _chromaticSeen;
    private DateTimeOffset _chromaticSince;
    private (R.ObjectiveKind Kind, int Progress, bool Done)? _objective;
    private (R.ObjectiveKind Kind, int Reads)? _otherObjective;
    private (int Days, HashSet<int> Cells)? _markedDestinations;
    private string? _theme;
    /// <summary>The theme was seen since the last Sandbox: it is the coming run's.</summary>
    private bool _themeComing;

    /// <summary>How long a higher Chromatic read must hold before it counts: a portrait flies into the panel in about a second.</summary>
    public static readonly TimeSpan ChromaticHold = TimeSpan.FromSeconds(1.2);

    public IReadOnlyList<string> Held => _held;
    /// <summary>Instrument slots shown from memory in the last update (their read wasn't an instrument), by index.</summary>
    public IReadOnlySet<int> Kept => _kept;
    public int Chromatic => _chromatic;
    /// <summary>The Remaining Days last read (or told by the log) in this run; null before any.</summary>
    public int? LastDays => _lastDays;
    /// <summary>The run's theme by id (<see cref="RunThemes"/>), or null.</summary>
    public string? Theme => _theme;

    /// <summary>
    /// Whether more Remaining Days than before means a new run. Off while the game's log tells runs apart: a misread
    /// day count would otherwise wipe the run's memory.
    /// </summary>
    public bool NewRunByDays { get; set; } = true;

    /// <summary>Notes a scan's Remaining Days: more days than before is a new run (keeping a theme seen just before it).</summary>
    public void Days(int? days)
    {
        if (days == null) return;
        if (NewRunByDays && _lastDays is int last && days > last) NewRun();
        _lastDays = days;
        _themeComing = false;
    }

    /// <summary>A new run begins: everything goes but a theme seen just before it (the lobby shows it first).</summary>
    public void NewRun()
    {
        string? theme = _themeComing ? _theme : null;
        bool coming = _themeComing;
        Clear();
        (_theme, _themeComing) = (theme, coming);
    }

    /// <summary>Notes the theme screen (in the lobby, before a run): the coming run's theme.</summary>
    public void SawTheme(string id)
    {
        _theme = id;
        _themeComing = true;
    }

    /// <summary>
    /// The instruments to show for a scan's slot reading (ids in slot order). A slot read as an instrument shows it
    /// (and is remembered); one that can't be read now (a tooltip over it: <see cref="SlotReader.Hidden"/>, the
    /// network unsure: <see cref="SlotReader.Unknown"/>, or read empty) shows what was read there before.
    /// </summary>
    public IReadOnlyList<string> Instruments(IReadOnlyList<string> read)
    {
        _kept.Clear();
        var shown = new List<string>();
        for (int i = 0; i < Math.Max(read.Count, _held.Count); i++)
        {
            string? r = i < read.Count ? read[i] : null;
            if (r != null && SlotReader.IsInstrument(r))
            {
                if (i < _held.Count) _held[i] = r;
                else if (i == _held.Count) _held.Add(r);
                shown.Add(r);
            }
            else if (i < _held.Count)
            {
                shown.Add(_held[i]);
                _kept.Add(i);
            }
            else if (r != null) shown.Add(SlotReader.Unknown);
        }
        return shown;
    }

    /// <summary>
    /// The Chromatic count to show for a scan's read of a frame captured <paramref name="at"/>: never fewer than
    /// before (a fade or a day's banner reads low), and more once the higher read has held for
    /// <see cref="ChromaticHold"/> (a portrait flying into the panel reads high for a moment). A read with slots a
    /// tooltip covers (<paramref name="covered"/>) changes nothing.
    /// </summary>
    public int ChromaticCount(int read, bool covered, DateTimeOffset at)
    {
        if (read <= _chromatic || covered)
        {
            _chromaticSeen = null;
            return _chromatic;
        }
        if (read != _chromaticSeen) (_chromaticSeen, _chromaticSince) = (read, at);
        else if (at - _chromaticSince >= ChromaticHold) _chromatic = read;
        return _chromatic;
    }

    /// <summary>Keep the last marked destinations for the reward pick that may cover the board after moving.</summary>
    public void RememberDestinations(EditableSandbox board)
    {
        if (board.Days is int days && board.Phase == Phases.Destination && board.Destinations.Count > 0)
            _markedDestinations = (days, board.Destinations.Where(d => d > 1 && board[d].Incident).ToHashSet());
    }

    /// <summary>Keep the objective through covered panels and short misreads during a transition.</summary>
    public void RememberObjective(R.Observation observation)
    {
        if (observation.Objective is { } kind && _objective is { } known && kind != known.Kind)
        {
            int reads = _otherObjective is { } other && other.Kind == kind ? other.Reads + 1 : 1;
            _otherObjective = (kind, reads);
            if (reads >= 3) _objective = null;
        }
        else _otherObjective = null;

        RestoreObjective(observation);
        if (observation.Objective is { } current)
            _objective = (current, observation.ObjectiveProgress, observation.ObjectiveDone);
    }

    /// <summary>A move onto a marked cell can display the objective reward before its incident plays out.</summary>
    public void PreparePick(R.Observation observation)
    {
        if (!observation.StartOfRun) RestoreObjective(observation);
        observation.IncidentAfterPick = observation.OfferForObjective &&
            (observation.Objective == R.ObjectiveKind.Undercurrents ||
                _markedDestinations is { } marked && marked.Days == observation.RemainingDays &&
                marked.Cells.Contains(observation.Warden + 1));
    }

    private void RestoreObjective(R.Observation observation)
    {
        if (_objective is not { } known) return;
        if (observation.Objective != known.Kind)
            (observation.Objective, observation.ObjectiveProgress) = (known.Kind, known.Progress);
        observation.ObjectiveDone |= known.Done;
    }

    public void Clear()
    {
        _held.Clear();
        _kept.Clear();
        _lastDays = null;
        _chromatic = 0;
        _chromaticSeen = null;
        _objective = null;
        _otherObjective = null;
        _markedDestinations = null;
        _theme = null;
        _themeComing = false;
    }
}
