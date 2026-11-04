using Tartarus.Vision;
using Tartarus.Vision.Ocr;

namespace Tartarus.Rules.Tests;

/// <summary>Reading the screen in another language than English: OCR's lines matched to the game's own texts.</summary>
public class GameTextTests
{
    [Fact]
    public void TextsAreThere() => Assert.True(GameText.Loaded, "models/text/tartarus_text.json");

    [Theory]
    // As the East Slavic recognizer read them off Russian frames: spaces dropped, a letter off.
    [InlineData("Крюкотбора", "hook")]
    [InlineData("Шипастая железная плеть", "whip")]
    [InlineData("Палаческая секира надзирателя", "axe")]
    [InlineData("Столб для сожжения", "stake")]
    [InlineData("Жeтон каторжника", "dogtag")]
    [InlineData("Sorting Hook", "hook")]
    public void InstrumentNamesInAnyLanguage(string read, string id)
    {
        var (instrument, score) = TortureInstruments.Match(read);
        Assert.Equal(id, instrument.Id);
        Assert.True(score >= LayoutScanner.PickNameScore, $"{read}: {score:0.00}");
    }

    [Theory]
    [InlineData("Злоупотребление властью", "tyranny")]
    [InlineData("Железный закон", "ironlaw")]
    public void ThemeNames(string read, string id) => Assert.Equal(id, RunThemes.Match(read).Theme.Id);

    [Theory]
    [InlineData("Пожирание", "consume")]
    [InlineData("Заменить", "replace")]
    [InlineData("Завершить", "end")]
    [InlineData("Consume", "consume")]
    [InlineData("Пойти в №3.", null)]
    public void DecisionWords(string read, string? kind) => Assert.Equal(kind, ScreenText.WordKind(read));

    [Fact]
    public void RussianScreen()
    {
        var text = Parse(("title", "Выберите цель"), ("cards", "Пойти в №3."), ("cards", "Пойти вдопросную"),
            ("objective", "Цель: Закон джунглей"), ("objective", "Пожирание2/4"), ("days", "До бунта осталось 16 дн."));
        Assert.True(text.RatwayPick);
        Assert.Equal(16, text.Days);
        Assert.Equal((2, 4), text.Progress);
        Assert.Equal(Rules.ObjectiveKind.SurvivalOfTheFittest, GameObservation.Objective(text));
        Assert.Equal([3, 1], text.Destinations.Select(d => d ?? 0));
    }

    [Fact]
    public void ConsumeBanner() => Assert.True(Parse(("banner", "Выберите узника для пожирания")).ConsumePick);

    [Fact]
    public void A_pick_is_not_the_Sewer_Ratway_the_last_Sandbox_text_told_of()
    {
        // 2026-10-28: the Hunt's "Select an Objective" was the last Sandbox text before a Warden's Treasure, whose hint
        // became a Sewer Ratway's cell (no card to frame).
        var text = Parse(("title", "Select an Objective"));
        Assert.True(text.RatwayPick);
        var under = EditableSandbox.Empty();
        under.Warden = 7;
        var pick = new PickRead { Kind = PickRead.Treasure, Cards = [new PickCardRead { Index = 1, Id = "vintage" }] };
        var o = GameObservation.FromPick(pick, under, text);
        Assert.False(o.Ratway);
        Assert.Single(o.Treasures);
    }

    [Theory]
    [InlineData(ObjectiveKind.Undercurrents, false, 12, true)]
    [InlineData(ObjectiveKind.PrisonWalk, true, 12, true)]
    [InlineData(ObjectiveKind.PrisonWalk, true, 11, false)]
    [InlineData(ObjectiveKind.PrisonWalk, false, 12, false)]
    public void Objective_reward_observes_a_deferred_incident(ObjectiveKind kind, bool marked, int day, bool deferred)
    {
        var run = new RunMemory();
        var under = EditableSandbox.Empty();
        under.Phase = Phases.Destination;
        under.Days = 12;
        under.Destinations.Add(7);
        under[7].Incident = marked;
        run.RememberDestinations(under);
        run.RememberObjective(new Observation { Objective = kind, ObjectiveProgress = 2 });

        under.Warden = 7;
        under.Days = day;
        under[7].Incident = false; // The reward popup covers the objective and the marker has disappeared.
        under.Instruments.Add("axe");
        var pick = new PickRead { Kind = PickRead.Instrument, Cards = [new PickCardRead { Index = 1, Id = "hook" }] };
        var observation = GameObservation.FromPick(pick, under);
        run.PreparePick(observation);

        Assert.True(observation.OfferForObjective);
        Assert.Equal(kind, observation.Objective);
        Assert.Equal(deferred, observation.IncidentAfterPick);

        run.NewRun();
        var opening = GameObservation.FromPick(pick, EditableSandbox.Empty());
        run.PreparePick(opening);
        Assert.Null(opening.Objective);
        Assert.False(opening.IncidentAfterPick);
    }

    /// <summary>Lines as OCR gives them, by region, laid out on a 1920x1200 screen (cards left to right).</summary>
    private static ScreenText Parse(params (string Role, string Text)[] lines)
    {
        var layout = new ScreenLayout(1920, 1200);
        var byRole = ScreenLayout.TextRegions.ToDictionary(r => r.Role, _ => new List<TextLine>());
        int card = 0;
        foreach (var (role, text) in lines)
        {
            int x = role == "cards" ? 960 + (card++ - 1) * 345 - 70 : 100;
            var box = new PixelBox(x, 100 + 30 * byRole[role].Count, x + 140, 124 + 30 * byRole[role].Count);
            byRole[role].Add(new TextLine { Text = text, Box = box, Ink = box, Conf = 1, DetScore = 1, Chars = [] });
        }
        return ScreenText.Parse(byRole, layout);
    }
}
