//! Failure locations shared by the companion status and the read-only on-game overlay.
use crate::draw::{Pill,Plan};
use vorax_scan::layout::layout_at;
use vorax_scan::pixels::Rect;
use vorax_scan::reading::Reading;

pub struct Feedback { pub title:String,pub detail:String,pub summary:String,pub plan:Plan }

pub fn field_label(field:&str)->String {
    match field {
        "round"=>"라운드 숫자".into(),
        "refreshes"=>"새로고침 남은 횟수".into(),
        "hand"=>"카드 선택지".into(),
        f if f.starts_with("hand")=>f[4..].parse::<usize>().map_or("카드 이름".into(),|i|format!("{}번 카드 이름",i+1)),
        f if f.starts_with("tank")=>{
            let (index,part)=f[4..].split_once('.').unwrap_or(("",""));
            let part=match part {"activity"=>"활성","quantity"=>"수량","archetype"=>"몬스터 종류","rarity"=>"등급",_=>"내용"};
            index.parse::<usize>().map_or("배양 탱크".into(),|i|format!("{}번 탱크 {part}",i+1))
        },
        _=>"화면 정보".into(),
    }
}

pub fn for_reading(r:&Reading,requested:&[usize])->Feedback {
    let sure=|field:&str|r.fields.get(field).is_some_and(|f|f.sure);
    let size=r.hand_ranks.as_ref().map_or(r.hand_size.unwrap_or(3),|h|h.len().max(3));
    let tanks=(0..6).filter(|i|sure(&format!("tank{i}.empty")) || ["rarity","archetype","activity","quantity"].iter().all(|p|sure(&format!("tank{i}.{p}")))).count();
    let cards=(0..size).filter(|i|sure(&format!("hand{i}"))).count();
    let round=if sure("round") {r.state.round.map_or("미확인".into(),|n|n.to_string())} else {"미확인".into()};
    let summary=format!("라운드 {round} · 탱크 {tanks}/6 · 카드 {cards}/{size}");
    let mut fields=r.blocking_fields();
    fields.sort_by_key(|f|if f=="round" {0} else if f.starts_with("tank") {1} else if f=="refreshes" {2} else {3});
    let failed=fields.iter().map(|f|field_label(f)).collect::<Vec<_>>().join(", ");
    let title=if fields.is_empty() {"입력 확인 완료".into()} else {format!("확인 필요: {failed}")};
    let mut lines=vec![summary.clone()];
    for i in 0..size {
        let field=format!("hand{i}");
        let name=r.fields.get(&field).and_then(|f|f.guess.as_deref()).map(vorax_scan::korean::name).unwrap_or("이름 미확인");
        lines.push(format!("{}번 카드: {name} ({})",i+1,if sure(&field) {"확인"} else {"미확인"}));
    }
    let refresh=if sure("refreshes") {
        let n=if r.state.phase==vorax::advisor::ScreenPhase::Potion {r.state.potion_refreshes} else {r.state.instrument_refreshes};
        format!("{}회 확인",n.unwrap_or(0))
    } else {"미확인 — 확인 버튼 오른쪽 주사위 옆 숫자".into()};
    if r.state.phase!=vorax::advisor::ScreenPhase::Protocol {lines.push(format!("새로고침 남은 횟수: {refresh}"));}
    if !fields.is_empty() {lines.push(format!("실패 항목: {failed}"));}
    if let Some(i)=requested.first() {lines.push(format!("{}번 카드에 마우스를 올려 확대 내용을 보여 주세요.",i+1));}
    let tank=fields.iter().find_map(|f|f.strip_prefix("tank").and_then(|s|s.split_once('.')).filter(|(_,p)|matches!(*p,"activity"|"quantity")).and_then(|(i,_)|i.parse::<usize>().ok()));
    let tank_guide=tank.map(|i|format!("{}번 탱크 아래 글자에 마우스를 올려 왼쪽 확대 정보를 보여 주세요.",i+1));
    if let Some(guide)=&tank_guide {lines.push(guide.clone());}
    let mut plan=Plan{pill:Some(Pill{main:title.clone(),sub:summary.clone(),dim:false}),..Plan::default()};
    if let (Some(pill),Some(guide))=(&mut plan.pill,tank_guide) {pill.sub=guide;}
    if let Some(frame)=&r.frame {
        let l=layout_at(frame.width,frame.height,r.ui()).shifted_hand(r.hand_dy.unwrap_or(0.0));
        for i in requested {if let Some(c)=l.hand_at(size).get(*i) {plan.inspect.push((*c,230.0*l.k,340.0*l.k));}}
        let mut marked=std::collections::BTreeSet::new();
        for field in &fields {
            if field=="refreshes" {plan.attention.push(Rect::new(l.refresh.x-40.0*l.k,l.refresh.y-15.0*l.k,l.refresh.w+80.0*l.k,l.refresh.h+30.0*l.k));}
            else if field=="round" {plan.attention.push(Rect::new(l.round.x,l.round.y,l.round.w*1.8,l.round.h*1.15));}
            else if let Some(index)=field.strip_prefix("tank").and_then(|s|s.split('.').next()).and_then(|s|s.parse::<usize>().ok()).filter(|i|*i<6) {
                if marked.insert(index) {let band=l.name_band[index];plan.attention.push(Rect::new(band.x,band.y-8.0*l.k,band.w,(l.aq_band[index].y+l.aq_band[index].h-band.y)+16.0*l.k));}
            }
        }
    }
    Feedback{title,detail:lines.join("\n"),summary,plan}
}

#[cfg(test)]
mod tests {
    use super::*;
    fn recorded()->Reading {
        let d:serde_json::Value=serde_json::from_str(include_str!("../../../fixtures/round10/reading-0.json")).unwrap();
        serde_json::from_value(d["reading"].clone()).unwrap()
    }
    #[test]
    fn reported_counter_failure_is_visible_with_no_actionable_advice() {
        let mut r=recorded();
        let frame=crate::pipeline::decode_png(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/round10/rest.png")).unwrap();
        r.frame=Some(vorax_scan::reading::Frame{width:frame.width,height:frame.height});
        let f=for_reading(&r,&[]);
        assert_eq!(f.title,"확인 필요: 새로고침 남은 횟수");
        assert_eq!(f.summary,"라운드 10 · 탱크 6/6 · 카드 3/3");
        for name in ["혼합 거머리 용액","생골 가루약","강력 이종 제거 가루약","확인 버튼 오른쪽 주사위 옆 숫자"] {assert!(f.detail.contains(name),"{}",f.detail);}
        assert_eq!(f.plan.attention.len(),1);assert!(f.plan.inspect.is_empty());
        assert!(f.plan.card.is_none() && f.plan.button.is_none() && f.plan.tanks.is_empty());
        assert!(f.plan.pill.is_some());
        #[cfg(windows)]
        if std::env::var_os("VORAX_QA_CROPS").is_some() {
            let fonts=crate::draw::Fonts::system().unwrap();
            let pm=crate::draw::render(&f.plan,frame.width as u32,frame.height as u32,&fonts,crate::draw::ACCENTS[0].1);
            let mut composite=frame.clone();
            for (dst,src) in composite.data.chunks_exact_mut(4).zip(pm.data().chunks_exact(4)) {
                let alpha=src[3] as f64/255.0;
                for c in 0..3 {dst[c]=(src[c] as f64+dst[c] as f64*(1.0-alpha)).round().min(255.0) as u8;} dst[3]=255;
            }
            let counter=f.plan.attention[0];
            for (name,area) in [("banner",Rect::new(frame.width as f64/2.0-500.0,0.0,1000.0,65.0)),("counter",Rect::new(counter.x-15.0,counter.y-15.0,counter.w+30.0,counter.h+30.0))] {
                let image=vorax_scan::pixels::crop(&composite,area);let path=std::env::temp_dir().join(format!("feedback-{name}.png"));
                crate::pipeline::encode_png(&path,image.width as u32,image.height as u32,&image.data).unwrap();
                let hex=std::fs::read(path).unwrap().iter().map(|b|format!("{b:02x}")).collect::<String>();println!("QA_FAILURE {name} {hex}");
            }
        }
    }
    #[test]
    fn tank_and_card_failures_have_distinct_numbers_and_locations() {
        let mut r=recorded();r.fields.get_mut("tank2.quantity").unwrap().sure=false;r.fields.get_mut("hand1").unwrap().sure=false;
        r.problems.push(vorax_scan::reading::Problem::Unreadable{field:"tank2.quantity".into()});
        let f=for_reading(&r,&[1]);assert!(f.title.contains("3번 탱크 수량"));assert!(f.title.contains("2번 카드 이름"));
        assert!(f.summary.contains("탱크 5/6 · 카드 2/3"));assert_eq!(f.plan.attention.len(),2);assert_eq!(f.plan.inspect.len(),1);
        assert!(f.plan.card.is_none() && f.plan.button.is_none() && f.plan.tanks.is_empty());
    }
}
