//! Confirmed inputs for one offer, collected across frames instead of requiring one perfect frame.
use std::collections::BTreeMap;
use vorax_scan::reading::{Problem, Reading};

#[derive(Default)]
pub struct RoundMemory {
    saved: Option<Reading>,
    pub revision: u64,
    pub reused: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn partial() -> Reading {
        let value:serde_json::Value=serde_json::from_str(include_str!("../../../fixtures/round-memory/reading-0.json")).unwrap();
        serde_json::from_value(value["reading"].clone()).unwrap()
    }
    fn round_known() -> Reading {
        let mut r=partial();r.state.round=Some(8);r.fields.get_mut("round").unwrap().sure=true;
        r.problems.retain(|p|!matches!(p,Problem::Unreadable{field} if field=="round"));r
    }
    #[test]
    fn confirmed_round_tanks_and_each_card_accumulate_across_imperfect_frames() {
        let mut memory=RoundMemory::default();let first=round_known();memory.remember(&first);
        let mut next=partial();next.state.round=Some(1);next.state.tanks[0]=None;
        next.fields.get_mut("tank0.activity").unwrap().sure=false;
        next.problems.push(Problem::Unreadable{field:"tank0.activity".into()});
        memory.restore(&mut next);
        assert!(sure(&next,"round"));assert_eq!(next.state.round,Some(8));
        assert_eq!(next.state.tanks[0],first.state.tanks[0]);assert!(!sure(&next,"hand1"));
        assert_eq!(memory.names().keys().copied().collect::<Vec<_>>(),[0,2]);
        assert!(memory.reused.contains(&"round".to_string()));assert!(memory.reused.contains(&"tank0.activity".to_string()));
        next.fields.get_mut("hand1").unwrap().sure=true;next.problems.clear();memory.remember(&next);
        let mut later=partial();memory.restore(&mut later);
        assert!(sure(&later,"hand1"));assert_eq!(memory.names().len(),3);
        assert!(!later.problems.iter().any(|p|matches!(p,Problem::Unreadable{..}|Problem::UnknownArt{..})));
    }
    #[test]
    fn verified_round_refresh_tank_and_offer_changes_end_the_memory() {
        for change in 0..4 {
            let mut memory=RoundMemory::default();let before=round_known();memory.remember(&before);
            let mut after=before.clone();
            match change {
                0=>{after.state.round=Some(9);},
                1=>{after.state.potion_refreshes=Some(1);},
                2=>{after.state.tanks[0].as_mut().unwrap().activity+=1;},
                _=>{after.fields.get_mut("hand0").unwrap().guess=Some("Green Bile Solution".into());},
            }
            memory.restore(&mut after);assert!(memory.names().is_empty(),"change {change}");assert_eq!(memory.revision,1);
        }
    }
}

fn sure(r: &Reading, field: &str) -> bool { r.fields.get(field).is_some_and(|f| f.sure) }
fn tank_known(r: &Reading, i: usize) -> bool {
    sure(r, &format!("tank{i}.empty")) || ["rarity", "archetype", "activity", "quantity"].iter().all(|part| sure(r, &format!("tank{i}.{part}")))
}
fn hand_size(r: &Reading) -> usize { r.hand_ranks.as_ref().map_or(r.hand_size.unwrap_or(3), |h| h.len().max(3)) }
fn refreshes(r: &Reading) -> Option<i32> {
    if r.state.phase == vorax::advisor::ScreenPhase::Potion { r.state.potion_refreshes } else { r.state.instrument_refreshes }
}

impl RoundMemory {
    pub fn clear(&mut self) { self.saved = None; self.reused.clear(); self.revision += 1; }
    pub fn round(&self) -> Option<i32> { self.saved.as_ref().filter(|r| sure(r,"round")).and_then(|r| r.state.round) }
    pub fn names(&self) -> BTreeMap<usize, String> {
        self.saved.as_ref().map_or_else(BTreeMap::new, |r| (0..hand_size(r)).filter_map(|i| {
            r.fields.get(&format!("hand{i}")).filter(|f| f.sure).and_then(|f| f.guess.clone()).map(|n| (i,n))
        }).collect())
    }
    pub fn confirmed(&self) -> Vec<String> {
        self.saved.as_ref().map_or_else(Vec::new, |r| r.fields.iter().filter(|(_,f)| f.sure).map(|(n,_)| n.clone()).collect())
    }

    /// Only positive evidence of an action ends the offer. A missing/weak digit or a moving title
    /// cannot erase an earlier confirmation. Refreshes matter even when the round stays the same.
    pub fn restore(&mut self, current: &mut Reading) {
        self.reused.clear();
        let Some(old) = &self.saved else { return };
        let changed = (sure(old,"round") && sure(current,"round") && old.state.round != current.state.round)
            || (sure(old,"phase") && sure(current,"phase") && old.state.phase != current.state.phase)
            || (sure(old,"refreshes") && sure(current,"refreshes") && (old.state.phase != current.state.phase || refreshes(old) != refreshes(current)))
            || hand_size(old) != hand_size(current)
            || (0..6).any(|i| tank_known(old,i) && tank_known(current,i) && old.state.tanks.get(i) != current.state.tanks.get(i))
            || (0..hand_size(current)).any(|i| {
                let field = format!("hand{i}");
                sure(old,&field) && sure(current,&field) && old.fields[&field].guess != current.fields[&field].guess
            });
        if changed { self.clear(); return; }
        let old = self.saved.as_ref().unwrap();
        let mut copied = Vec::new();
        if sure(old,"round") && !sure(current,"round") {
            current.state.round = old.state.round;
            current.fields.insert("round".into(),old.fields["round"].clone()); copied.push("round".to_string());
        }
        if sure(old,"refreshes") && !sure(current,"refreshes") && old.state.phase == current.state.phase {
            current.state.potion_refreshes = old.state.potion_refreshes;
            current.state.instrument_refreshes = old.state.instrument_refreshes;
            current.fields.insert("refreshes".into(),old.fields["refreshes"].clone()); copied.push("refreshes".to_string());
        }
        for i in 0..6 {
            if tank_known(old,i) && !tank_known(current,i) {
                current.state.tanks[i] = old.state.tanks[i].clone();
                let prefix = format!("tank{i}.");
                current.fields.retain(|k,_| !k.starts_with(&prefix));
                for (k,f) in old.fields.iter().filter(|(k,_)| k.starts_with(&prefix)) { current.fields.insert(k.clone(),f.clone()); copied.push(k.clone()); }
            }
        }
        for i in 0..hand_size(current) {
            let field = format!("hand{i}");
            if sure(old,&field) && !sure(current,&field) {
                current.fields.insert(field.clone(),old.fields[&field].clone()); copied.push(field);
            }
        }
        current.problems.retain(|p| match p {
            Problem::Unreadable{field} | Problem::UnknownArt{field,..} => !copied.contains(field),
            _ => true,
        });
        if copied.iter().any(|f| f.starts_with("hand")) {
            current.state.hand = (0..hand_size(current)).filter_map(|i| current.fields.get(&format!("hand{i}")).and_then(|f| f.guess.clone())).collect();
        }
        self.reused = copied;
    }
    pub fn remember(&mut self, reading: &Reading) { self.saved = Some(reading.clone()); }
}
