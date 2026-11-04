use std::time::Instant;
use vorax::advisor::{self, Best, Prepared, ScreenPhase, ScreenState, Work};
use vorax::bot::*;
use vorax::data::{data, Targeting};
use vorax::game::{create_game, dispatch, needs_targets};
use vorax::types::*;

fn screen(phase: ScreenPhase, hand: Vec<String>) -> ScreenState {
    ScreenState { tanks: vec![None; 6], hand, instruments: vec![], instrument_rounds: None, phase,
        round: Some(1), potion_refreshes: Some(1), instrument_refreshes: Some(1), shelley: Some(3), open_box: None }
}

#[test]
fn mean_counts_scores_above_target_in_both_kinds_of_futures() {
    let mut s = create_game(data().default_settings, None, Some(1));
    s.phase = Phase::GameOver;
    s.tanks[0] = Some(Monster { activity: 2000, quantity: 1000, ..Monster::default() });
    let ua: i64 = s.tanks.iter().flatten().map(Monster::total).sum();
    for goal in [Goal::Target, Goal::Mean] {
        let expected = if goal == Goal::Target { 840_000.0 } else { ua as f64 };
        let la = LookAhead { goal, ..advisor::LOOK_AHEAD };
        for par in [false, true] {
            let p = potion_futures(&s, 7, &la, &[Opt::Refresh], 0, 2051, par);
            let i = instrument_futures(&s, 7, &[Pick::Refresh], &Chooser::Ev(Policy::new(2, 0)), goal, 0, 2051, par);
            assert_eq!(p, i);
            assert_eq!(p[0], Tally { goal: expected * 2051.0, reached: 2051, n: 2051,
                monsters: s.monsters().map(|m| m.quantity as f64).sum::<f64>() * 2051.0,
                groups: s.monsters().count() as f64 * 2051.0 });
        }
        let policy = Policy { value: Some(true), goal, ..Policy::new(2, 0) };
        assert_eq!(vorax::bot::expected(&s, &Opt::Refresh, &policy, 1, 0), expected);
    }
}

#[test]
fn simulated_reroll_is_not_overridden_by_a_potion_kit() {
    let kit = data().potions.iter().find(|p| matches!(p.targeting, Targeting::Box(_))).unwrap();
    let x = screen(ScreenPhase::Potion, vec![kit.name.clone()]);
    for goal in [Goal::Target, Goal::Mean] {
        let Prepared::Work { work, .. } = advisor::prepare_for_goal(&x, Some(10), goal) else { panic!() };
        let Work::Potion { opts, .. } = &work else { panic!() };
        let ts: Vec<_> = opts.iter().map(|(o, _)| Tally { goal: if *o == Opt::Refresh { 2000.0 } else { 1000.0 }, n: 10, ..Tally::default() }).collect();
        let a = advisor::conclude(&work, &x, vec![], Instant::now(), &ts);
        assert_eq!(a.best, Best::Refresh);
    }
}

#[test]
fn mean_orders_instruments_and_reroll_before_rounding() {
    let x = screen(ScreenPhase::Instrument, data().instruments.iter().take(2).map(|i| i.name.clone()).collect());
    let Prepared::Work { work, .. } = advisor::prepare_for_goal(&x, Some(100), Goal::Mean) else { panic!() };
    let ts = [10010.0, 10049.0, 10030.0].map(|goal| Tally { goal, n: 100, ..Tally::default() });
    let a = advisor::conclude(&work, &x, vec![], Instant::now(), &ts);
    let Best::Card(best) = a.best else { panic!("a rounded-down card must not lose to a worse reroll") };
    assert_eq!(best.index, 1);
    assert_eq!(a.ranking[0].index, 1);
}

#[test]
fn simulation_does_not_promote_a_lower_value_just_to_change_the_board() {
    let mut x = screen(ScreenPhase::Potion, vec!["Osteolytic Ointment".into(), "Green Bile Solution".into()]);
    x.potion_refreshes = Some(0);
    x.tanks = vec![Some(advisor::ScreenMonster { archetype: Archetype::Pest, rarity: Rarity::Normal, activity: 10, quantity: 10 }); 6];
    for goal in [Goal::Target, Goal::Mean] {
        let Prepared::Work { work, .. } = advisor::prepare_for_goal(&x, Some(10), goal) else { panic!() };
        let Work::Potion { opts, .. } = &work else { panic!() };
        let ts: Vec<_> = opts.iter().map(|(o, _)| Tally { goal: if o.index() == Some(0) { 1000.0 } else { 999.0 }, n: 10, ..Tally::default() }).collect();
        let a = advisor::conclude(&work, &x, vec![], Instant::now(), &ts);
        let Best::Card(best) = a.best else { panic!() };
        assert_eq!(best.index, 0);
        assert_eq!(best.no_effect, Some(true));
    }
}

fn reference_options(s: &GameState) -> Vec<Opt> {
    let mut out = vec![];
    for i in 0..s.hand.len() {
        let selected = dispatch(s, Action::SelectCard { index: i as i64 });
        match needs_targets(&selected) {
            None => out.push(Opt::card(i, &[])),
            Some(mode) => {
                let tanks = selected.non_empty();
                for &t in &tanks { out.push(Opt::card(i, &[t])); }
                if mode == Targeting::Select1To2 {
                    for &a in &tanks { for &b in &tanks { if a < b { out.push(Opt::card(i, &[a,b])); } } }
                }
            }
        }
    }
    out
}

#[test]
fn direct_option_enumeration_preserves_every_card_and_board_mask() {
    let mut s = create_game(data().default_settings, None, Some(1));
    for phase in [Phase::PotionSelect, Phase::InstrumentSelect, Phase::RoundEnd, Phase::GameOver] {
        s.phase = phase;
        for mask in 0..64 {
            for (i, tank) in s.tanks.iter_mut().enumerate() {
                *tank = (mask & (1 << i) != 0).then_some(Monster { activity: 100, quantity: 10, ..Monster::default() });
            }
            for id in 0..data().potions.len() {
                s.hand = Hand::from_slice(&[Card { kind: CardKind::Potion, id: id as u8 }]);
                let reference = reference_options(&s);
                assert_eq!(options(&s).as_slice(), reference, "phase {phase:?}, mask {mask}, potion {id}");
                for o in reference.into_iter().chain([Opt::card(0, &[0,0]), Opt::card(0, &[255]), Opt::card(0, &[5,0]), Opt::card(255, &[])]) {
                    assert_eq!(play_opt(&s, &o), vorax::game::play(&s, &o.actions()), "{phase:?}, mask {mask}, potion {id}, {o:?}");
                }
                assert_eq!(pick_and_confirm(&s, 0), vorax::game::play(&s, &[Action::SelectCard { index: 0 }, Action::Confirm]));
            }
        }
    }
}

#[test]
fn shortlist_keeps_the_old_order_and_tie_breaking() {
    for line in include_str!("../../../fixtures/advisor.jsonl").lines() {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        let x: ScreenState = serde_json::from_value(v["screen"].clone()).unwrap();
        let s = advisor::to_state(&x, &mut vec![]);
        let mut now: Vec<_> = options(&s).iter().map(|o| (*o, expected(&s, o, &Policy::new(8, 0), 19, 0))).collect();
        now.sort_by(|a,b| b.1.partial_cmp(&a.1).unwrap());
        for top in [0, 1, 4, 16, 128] {
            let mut list: Vec<usize> = now.iter().enumerate().filter(|(i,c)| now.iter().position(|d| d.0.index() == c.0.index()) == Some(*i)).map(|(i,_)| i).collect();
            for i in 0..now.len() { if list.len() < top && !list.contains(&i) { list.push(i); } }
            let mut reference: Vec<_> = list.iter().map(|&i| now[i]).collect();
            if s.open_box.is_none() && vorax::game::can_refresh(&s) { reference.push((Opt::Refresh, vorax::game::ultimate_activity(&s) as f64)); }
            assert_eq!(shortlist(&s, 19, &LookAhead { top, ..advisor::LOOK_AHEAD }), reference);
        }
    }
}
