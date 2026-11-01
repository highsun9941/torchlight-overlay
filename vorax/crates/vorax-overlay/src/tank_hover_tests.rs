//! The recorded 3rd-tank failure, distinct from supplied parser-unit OCR strings.
use crate::pipeline::{Pipeline,trustworthy};
use vorax_scan::{Img,reading::Reading,tank_hover::{self,TankHoverRead}};
use vorax::types::Rarity;

fn recorded()->Reading {serde_json::from_str(include_str!("../../../fixtures/tank-tooltip/reading.json")).unwrap()}
fn popup()->Img {
    let f=crate::pipeline::decode_png(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/tank-tooltip/popup.png")).unwrap();
    vorax_scan::window::game_view(&f).map_or_else(||f.clone(),|r|vorax_scan::window::crop_to(&f,&r))
}
fn expected()->TankHoverRead {
    let search=vorax_scan::hover::inspect_monster_tooltip(&popup());
    assert!(search.tooltip.is_some(),"{search:?}");
    TankHoverRead{name:"순례자".into(),rarity:Rarity::Magic,total:120,activity:5,quantity:24,tooltip:search.tooltip.unwrap()}
}

#[test]
fn tank_popup_requires_monster_footer_and_three_independent_numbers() {
    // Parser guards only. Real model inference is separately mandatory in --ocr-validate.
    let frame=popup();let expected=expected();
    assert!(expected.tooltip.rect.w>150.0 && expected.tooltip.rect.h>250.0);
    for (words,stage) in [
        (vec!["매직포션"],"monster_footer"),
        (vec!["매직몬스터","순례자","120","5",""],"numbers"),
        (vec!["매직몬스터","순례자","121","5","24"],"product"),
        (vec!["매직몬스터","순례자","120","0","24"],"numbers"),
    ] {
        let mut words=words.into_iter();let mut read=|_:&Img|words.next().unwrap_or("").to_string();
        assert_eq!(tank_hover::recognize(&frame,&mut read).unwrap_err().stage,stage);
    }
    let mut words=["매직몬스터","순례자","120","5","24"].into_iter();
    let result=tank_hover::recognize(&frame,&mut |_|words.next().unwrap().to_string()).unwrap();
    assert_eq!((result.activity,result.quantity,result.total),(5,24,120));
    let mut words=["매직몬스터","","120","5","24"].into_iter();
    assert!(tank_hover::recognize(&frame,&mut |_|words.next().unwrap().to_string()).is_ok(),"numeric recovery does not require OCR of the portrait's name");
}

#[test]
fn tank_popup_association_checks_every_tank_and_the_players_pointer() {
    let mut r=recorded();let mut read=expected();
    assert_eq!(tank_hover::target(&read,&r,None),Some(2));
    assert_eq!(tank_hover::target(&read,&r,Some(0)),None);
    read.rarity=Rarity::Normal;assert_eq!(tank_hover::target(&read,&r,None),None);read.rarity=Rarity::Magic;
    read.total=121;assert_eq!(tank_hover::target(&read,&r,Some(2)),None);read.total=120;
    r.state.tanks[3]=r.state.tanks[2].clone();
    let duplicate=r.debug.as_ref().unwrap().tanks[2].clone();
    r.debug.as_mut().unwrap().tanks[3]=duplicate;
    for part in ["rarity","archetype"] {r.fields.insert(format!("tank3.{part}"),r.fields[&format!("tank2.{part}")].clone());}
    assert_eq!(tank_hover::target(&read,&r,None),None);
    assert_eq!(tank_hover::target(&read,&r,Some(2)),Some(2));
    assert_eq!(tank_hover::target(&read,&r,Some(3)),Some(3));
}

#[test]
fn tank_confirmation_is_sequential_expires_and_retains_confirmed_cards() {
    let mut pipe=Pipeline::new(0);let mut r=recorded();pipe.apply_zoom(&mut r);
    let read=expected();assert_eq!(pipe.requested_tanks(),[2]);assert!(pipe.requested_slots().is_empty());
    let cards=r.state.hand.clone();
    let feedback=crate::feedback::for_reading(&r,&[]);
    assert!(feedback.detail.contains("3번 탱크 아래 글자"));assert_eq!(feedback.plan.attention.len(),1);assert!(feedback.plan.card.is_none());
    assert!(!pipe.confirm_tank_hover(1,&read,0.0));
    assert!(!pipe.confirm_tank_hover(2,&read,500.0));
    assert!(!pipe.confirm_tank_hover(2,&read,3100.0),"old observations expire");
    let mut second=read.clone();second.name.clear();
    assert!(pipe.confirm_tank_hover(2,&second,3600.0),"stable numeric evidence survives a missing diagnostic name");
    assert!(pipe.requested_tanks().is_empty());assert!(pipe.hover_dirty());
    let after=pipe.pending().unwrap();assert!(trustworthy(after));assert_eq!(after.state.hand,cards);
    assert_eq!((after.state.tanks[2].as_ref().unwrap().activity,after.state.tanks[2].as_ref().unwrap().quantity),(5,24));
    let mut noisy=recorded();pipe.apply_zoom(&mut noisy);
    assert!(trustworthy(&noisy));assert_eq!(noisy.state.hand,cards);assert_eq!(noisy.state.tanks[2].as_ref().unwrap().quantity,24);
    let mut next=recorded();next.state.round=Some(2);next.fields.get_mut("round").unwrap().guess=Some("2".into());pipe.apply_zoom(&mut next);
    assert!(!trustworthy(&next));assert_eq!(pipe.requested_tanks(),[2]);
    assert!(!pipe.confirm_tank_hover(2,&read,4000.0),"new board needs two new observations");
    pipe.cancel_hover_candidate();assert!(!pipe.confirm_tank_hover(2,&read,4500.0));
}

#[test]
fn tank_label_pointer_hit_test_uses_capture_geometry() {
    for (w,h) in [(2048,877),(2172,931),(2560,1017)] {
        let l=vorax_scan::layout::layout_at(w,h,1.0);
        for i in 0..6 {let b=l.aq_band[i];assert_eq!(tank_hover::pointer_tank(w,h,1.0,b.x+b.w/2.0,b.y+b.h/2.0),Some(i));}
        assert_eq!(tank_hover::pointer_tank(w,h,1.0,5.0,h as f64-5.0),None);
    }
}
