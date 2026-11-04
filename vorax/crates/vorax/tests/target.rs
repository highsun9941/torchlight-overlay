use std::cmp::Ordering;
use std::time::Instant;
use vorax::advisor::{self, Best, Prepared, ScreenPhase, ScreenState};
use vorax::bot::{Goal, Tally};
use vorax::data::data;

#[test]
fn target_prioritizes_shortfall_then_success_then_combat_size() {
    let base = Tally { goal: 8_400_000.0, reached: 10, n: 10, monsters: 10_000.0, groups: 60.0 };
    let small = Tally { monsters: 100.0, groups: 10.0, ..base };
    assert!(small.compare(&base, Goal::Target).is_gt());
    assert_eq!(small.compare(&base, Goal::Mean), Ordering::Equal);
    let fewer_groups = Tally { groups: 20.0, ..base };
    assert!(fewer_groups.compare(&base, Goal::Target).is_gt());
    assert!(small.compare(&fewer_groups, Goal::Target).is_gt());
    // Saving combat time never compensates for losing expected score or success.
    assert!(Tally { goal: base.goal - 1.0, ..small }.compare(&base, Goal::Target).is_lt());
    assert!(Tally { reached: 9, ..small }.compare(&base, Goal::Target).is_lt());
    let twice = Tally { goal: base.goal * 2.0, reached: 20, n: 20, monsters: 20_000.0, groups: 120.0 };
    assert_eq!(twice.compare(&base, Goal::Target), Ordering::Equal);
}

#[test]
fn target_does_not_buy_one_jackpot_with_one_bad_run() {
    use vorax::bot::{potion_futures, LookAhead, Opt};
    use vorax::types::{Monster, Phase};
    let score = |goal, activity| {
        let mut s = vorax::game::create_game(data().default_settings, None, Some(1));
        s.phase = Phase::GameOver;
        s.tanks = [None; 6];
        s.tanks[0] = Some(Monster { activity, quantity: 1, ..Monster::default() });
        potion_futures(&s, 1, &LookAhead { goal, ..advisor::LOOK_AHEAD }, &[Opt::Refresh], 0, 1, false)[0].goal
    };
    let stable = (score(Goal::Target, 840_000) + score(Goal::Target, 840_000)) / 2.0;
    let lottery = (score(Goal::Target, 265_453) + score(Goal::Target, 4_000_000)) / 2.0;
    assert!(stable > lottery);
    assert_eq!(score(Goal::Target, 840_000), score(Goal::Target, 4_000_000));
    assert!(score(Goal::Mean, 4_000_000) > score(Goal::Mean, 840_000));
}

#[test]
fn live_advice_uses_combat_tie_break_for_cards_and_rerolls() {
    for phase in [ScreenPhase::Potion, ScreenPhase::Instrument] {
        let hand = if phase == ScreenPhase::Potion {
            data().potions.iter().take(2).map(|p| p.name.clone()).collect()
        } else {
            data().instruments.iter().take(2).map(|i| i.name.clone()).collect()
        };
        let x = ScreenState { tanks: vec![None; 6], hand, instruments: vec![], instrument_rounds: None,
            phase, round: Some(1), potion_refreshes: Some(1), instrument_refreshes: Some(1),
            shelley: Some(3), open_box: None };
        let Prepared::Work { work, .. } = advisor::prepare_for_goal(&x, Some(10), Goal::Target) else { panic!() };
        let mut ts = vec![Tally { goal: 8_400_000.0, reached: 10, n: 10, monsters: 10_000.0, groups: 60.0 }; 3];
        ts[1].monsters = 100.0;
        let a = advisor::conclude(&work, &x, vec![], Instant::now(), &ts);
        assert!(matches!(a.best, Best::Card(ref r) if r.index == 1));
        ts[2].monsters = 50.0;
        assert_eq!(advisor::conclude(&work, &x, vec![], Instant::now(), &ts).best, Best::Refresh);
        ts[2].goal -= 1.0;
        assert!(matches!(advisor::conclude(&work, &x, vec![], Instant::now(), &ts).best, Best::Card(ref r) if r.index == 1));
    }
}
