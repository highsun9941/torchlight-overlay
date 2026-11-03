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

fn rare_recorded()->Reading {serde_json::from_str(include_str!("../../../fixtures/rare-tank-tooltip/reading.json")).unwrap()}
fn rare_expected()->TankHoverRead {
    let mut read=expected();read.name="이단학자".into();read.rarity=Rarity::Rare;read.total=180;read.activity=15;read.quantity=12;read
}

#[test]
fn missing_compact_total_does_not_block_a_verified_compact_pair() {
    let mut r=rare_recorded();let mut read=rare_expected();
    assert!(r.debug.as_ref().unwrap().tanks[0].as_ref().unwrap().total.is_empty());
    assert!(!r.fields["tank0.activity"].sure);
    let association=tank_hover::inspect_target(&read,&r,Some(0));
    assert_eq!(association.slot,Some(0));assert_eq!(association.candidates[0].1,"matched_activity_quantity");
    assert_eq!(tank_hover::target(&read,&r,None),Some(0));
    assert_eq!(tank_hover::target(&read,&r,Some(1)),None);
    read.activity=12;read.quantity=15;assert_eq!(tank_hover::target(&read,&r,Some(0)),None);
    read.activity=15;read.quantity=12;
    r.fields.get_mut("tank0.quantity").unwrap().score=Some(0.74);
    assert_eq!(tank_hover::target(&read,&r,Some(0)),None,"weak compact ink is not identity evidence");
    r.fields.get_mut("tank0.quantity").unwrap().score=Some(0.94);
    r.debug.as_mut().unwrap().tanks[0].as_mut().unwrap().aq="15x12l".into();
    assert_eq!(tank_hover::target(&read,&r,Some(0)),None);
    r.debug.as_mut().unwrap().tanks[0].as_mut().unwrap().aq="15x12".into();
    r.debug.as_mut().unwrap().tanks[0].as_mut().unwrap().total="181".into();
    assert_eq!(tank_hover::target(&read,&r,Some(0)),None,"do not ignore contradictory source evidence");
    assert_eq!(tank_hover::positive_number("180 Y"),None);assert_eq!(tank_hover::positive_number("180K"),None);
}

#[test]
fn rare_tank_confirmation_advances_to_the_unresolved_card_without_guessing_it() {
    let mut pipe=Pipeline::new(0);let mut r=rare_recorded();pipe.apply_zoom(&mut r);let read=rare_expected();
    assert_eq!(pipe.requested_tanks(),[0]);assert!(pipe.requested_slots().is_empty());
    assert!(!pipe.confirm_tank_hover(0,&read,500.0));assert!(pipe.confirm_tank_hover(0,&read,1000.0));
    assert!(pipe.requested_tanks().is_empty());assert_eq!(pipe.requested_slots(),[0]);
    let pending=pipe.pending().unwrap();assert_eq!(pending.state.hand,r.state.hand);assert!(!trustworthy(pending));
    assert!(pending.fields["tank0.activity"].sure && pending.fields["tank0.quantity"].sure && !pending.fields["hand0"].sure);
    let feedback=crate::feedback::for_reading(pending,&pipe.requested_slots());
    assert!(!feedback.title.contains("탱크"));assert!(feedback.title.contains("1번 카드"));assert_eq!(feedback.plan.inspect.len(),1);
    let mut noisy=rare_recorded();pipe.apply_zoom(&mut noisy);assert_eq!(pipe.requested_slots(),[0]);assert!(pipe.requested_tanks().is_empty());
    assert!(noisy.fields["tank0.quantity"].sure);assert!(!noisy.fields["hand0"].sure);
}

#[test]
fn gold_ornaments_are_removed_without_changing_the_centred_number() {
    let input=crate::pipeline::decode_png(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/rare-tank-tooltip/total-ornaments.png")).unwrap();
    let output=tank_hover::numeric_ink(&input);
    assert_eq!((input.width,input.height),(492,124));
    assert!(input.data.chunks_exact(4).zip(output.data.chunks_exact(4)).any(|(a,b)|a!=b));
    for y in 0..input.height {for x in 0..input.width {
        let i=(y*input.width+x)*4;
        if (180..=328).contains(&x) {assert_eq!(&input.data[i..i+4],&output.data[i..i+4]);}
        if x<170 || x>338 {assert_eq!(&output.data[i..i+4],&[255;4]);}
    }}
    let blank=Img{width:300,height:100,data:vec![255;300*100*4]};assert_eq!(tank_hover::numeric_ink(&blank).data,blank.data);
}
