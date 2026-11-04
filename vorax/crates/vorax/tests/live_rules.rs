use vorax::{advisor::{self, ScreenState}, bot::{self, Opt}, data::data, game, rng::Rng, types::*};

#[test]
fn observed_transitions_from_two_live_runs_are_possible() {
    // These seeds are witnesses for the observed outcomes, not the game's unknown RNG seed.
    // One contradictory race reading is documented separately, never silently corrected here.
    let rows:Vec<serde_json::Value>=serde_json::from_str(include_str!("../../../fixtures/live-transitions-20261101.json")).unwrap();
    assert_eq!(rows.len(),33);
    for r in rows {
        let before:ScreenState=serde_json::from_value(r["before"].clone()).unwrap();
        let after:ScreenState=serde_json::from_value(r["after"].clone()).unwrap();
        let mut s=advisor::to_state(&before,&mut vec![]);
        s.rng_state=r["seed"].as_u64().unwrap() as u32;
        let n=if before.phase==advisor::ScreenPhase::Protocol {
            s.phase=Phase::RoundEnd;
            while s.round<after.round.unwrap() && s.phase!=Phase::GameOver { s=game::dispatch(&s,Action::Confirm); }
            s
        } else if let Some(index)=r["index"].as_u64() {
            let targets:Vec<u8>=serde_json::from_value(r["targets"].clone()).unwrap();
            bot::play_opt(&s,&Opt::card(index as usize,&targets))
        } else {game::dispatch(&s,Action::Refresh)};
        assert_eq!(n.tanks,advisor::to_state(&after,&mut vec![]).tanks,"step {}",r["step"]);
    }
}

#[test]
fn reward_tool_starts_the_following_round_and_keeps_potion_rerolls() {
    let x: ScreenState=serde_json::from_value(serde_json::json!({
        "tanks":[null,null,null,null,null,null], "hand":["Rawhide Restraint"],
        "instruments":["Chrysalis Specimen"], "phase":"instrument", "round":2,
        "potionRefreshes":0, "instrumentRefreshes":1
    })).unwrap();
    let s=advisor::to_state(&x,&mut vec![]);
    assert_eq!(s.round,3);
    let n=bot::pick_and_confirm(&s,0);
    assert_eq!(n.round,3);
    assert_eq!(n.instruments[1].acquired_round,3);
    assert_eq!(n.potion_refreshes,0);
}

#[test]
fn owned_tools_can_reappear_but_one_hand_has_no_duplicates() {
    let mut owned=Equips::new(); owned.push(Equipped{id:13,acquired_round:3});
    let mut seen=false;
    for seed in 1..=100 {
        let h=game::deal_instruments(data(),&mut Rng::new(seed),false,&owned);
        seen |= h.iter().any(|c|c.id==13);
        for i in 0..h.len() { for j in i+1..h.len() {assert_ne!(h[i].id,h[j].id);} }
    }
    assert!(seen,"live game offered a second Rawhide Restraint");
}

#[test]
fn opened_kit_rerolls_use_the_box_pool_and_preserve_round() {
    let x: ScreenState=serde_json::from_value(serde_json::json!({
        "tanks":[null,null,null,null,null,null], "hand":["Vivigenic Powder"],
        "instruments":["Chrysalis Specimen"], "phase":"potion", "round":1,
        "potionRefreshes":2,"openBox":"Rare Potion Kit (Small)"
    })).unwrap();
    let s=advisor::to_state(&x,&mut vec![]);
    assert!(s.open_box.is_some());
    assert!(bot::shortlist(&s,1,&advisor::LOOK_AHEAD).iter().any(|o|o.0==Opt::Refresh));
    for seed in 1..=100 {
        let n=game::dispatch(&GameState{rng_state:seed,..s},Action::Refresh);
        assert_eq!((n.round,n.potion_refreshes,n.open_box),(1,1,s.open_box));
        assert!(n.hand.iter().all(|c| !matches!(data().potion(c.id).targeting,vorax::data::Targeting::Box(_))));
    }
}
