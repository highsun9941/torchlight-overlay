using Tartarus.Vision;

namespace Tartarus.Rules.Tests;

public sealed class RerolledCardsTests
{
    private static readonly DateTime Click = new DateTime(2026, 11, 5, 12, 0, 0, DateTimeKind.Utc).ToLocalTime();
    private static readonly PickRead Original = Pick("chains", "axe", "whip");

    [Fact]
    public void Identical_cards_can_be_hinted_again_after_the_limit()
    {
        var guard = new RerolledCards();
        var log = new GameLog();
        Assert.True(log.Feed("[2026.11.05-11.59.00:000] normal-click Btn_S16GamePlay_Start"));
        var run = Assert.Single(log.Runs);
        Assert.False(guard.StillGoing(run, Original, Click.AddSeconds(-1)));
        Assert.True(log.Feed("[2026.11.05-12.00.00:000] normal-click Btn_S16Board_RelicPop_Reroll"));
        Assert.True(guard.StillGoing(run, Pick("chains", null, "whip"), Click));
        Assert.True(guard.StillGoing(run, Original, Click.AddMilliseconds(2999)));
        Assert.False(guard.StillGoing(run, Original, Click.AddSeconds(3)));
    }

    [Fact]
    public void The_same_cards_in_a_different_order_are_ready_immediately()
    {
        var guard = new RerolledCards();
        var run = new LoggedRun();
        guard.StillGoing(run, Original, Click.AddSeconds(-1));
        (run.LastRerollAt, run.RerollsUsed) = (Click, 1);
        Assert.False(guard.StillGoing(run, Pick("whip", "chains", "axe"), Click));
        Assert.False(guard.StillGoing(run, Original, Click.AddMilliseconds(500)));
    }

    [Fact]
    public void The_previous_pick_cannot_hold_up_another_offer()
    {
        var guard = new RerolledCards();
        var run = new LoggedRun();
        guard.StillGoing(run, Original, Click.AddSeconds(-1));
        (run.LastRerollAt, run.RerollsUsed) = (Click, 1);
        Assert.True(guard.StillGoing(run, Original, Click));
        run.InstrumentPicks++;
        Assert.False(guard.StillGoing(run, Original, Click.AddMilliseconds(500)));
        Assert.False(guard.StillGoing(new LoggedRun(), Original, Click.AddMilliseconds(500)));
        Assert.False(guard.StillGoing(null, Original, Click.AddMilliseconds(500)));
    }

    [Fact]
    public void A_scan_after_the_limit_can_hint_an_identical_offer_immediately()
    {
        var guard = new RerolledCards();
        var run = new LoggedRun();
        guard.StillGoing(run, Original, Click.AddSeconds(-1));
        (run.LastRerollAt, run.RerollsUsed) = (Click, 1);
        Assert.False(guard.StillGoing(run, Original, Click.AddSeconds(4)));
    }

    private static PickRead Pick(params string?[] ids) => new()
    {
        Cards = ids.Select((id, index) => new PickCardRead { Id = id, Index = index }).ToList(),
    };
}
