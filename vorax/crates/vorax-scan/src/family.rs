//! Species variants share artwork and a long title prefix. The distinguishing species must be read.
pub fn is_variant(name:&str)->bool {name.contains(" - ")}
fn clean_ko(s:&str)->Vec<char> {s.chars().filter(|c|crate::korean::hangul(*c)).collect()}
fn clean_en(s:&str)->String {s.chars().filter(|c|c.is_ascii_alphabetic()).flat_map(char::to_lowercase).collect()}
fn distance(a:&[char],b:&[char])->usize {
    let mut row:Vec<_>=(0..=b.len()).collect();
    for (i,c) in a.iter().enumerate() {let mut next=vec![i+1;b.len()+1];for j in 1..=b.len() {next[j]=(row[j]+1).min(next[j-1]+1).min(row[j-1]+usize::from(c!=&b[j-1]));}row=next;}row[b.len()]
}
fn group<'a>(name:&str,names:&'a [String])->Vec<&'a String> {
    let Some((stem,_))=name.split_once(" - ") else {return vec![]};
    names.iter().filter(|n|n.split_once(" - ").is_some_and(|(base,_)|base==stem)).collect()
}
fn noun(name:&str)->Vec<char> {
    let local=crate::korean::name(name).rsplit('-').next().unwrap_or("");
    clean_ko(local.split_whitespace().last().unwrap_or(local))
}
fn resolved<'a>(name:&str,text:&str,names:&'a [String])->Option<&'a String> {
    let group=group(name,names);if group.len()<2 {return None;}
    let ko=clean_ko(text);let en=clean_en(text);
    let exact:Vec<_>=group.iter().copied().filter(|n| {
        let token=noun(n);let suffix=n.split_once(" - ").unwrap().1;
        (token.len()>=2 && ko.windows(token.len()).any(|w|w==token.as_slice())) || en.contains(&clean_en(suffix))
    }).collect();
    if exact.len()==1 {return exact.first().copied();} if exact.len()>1 {return None;}
    // A one-syllable OCR error is allowed only at the end of a title-like line and only when
    // one family member fits. A clipped one-character suffix or generic description cannot qualify.
    let approximate:Vec<_>=group.into_iter().filter(|n| {
        let token=noun(n);if token.len()<2 {return false;}
        text.lines().any(|line| {
            let line_ko=clean_ko(line);
            line_ko.len()>=token.len() && crate::titles::name_fit(n,line)>=6.0
                && distance(&line_ko[line_ko.len()-token.len()..],&token)<=1
        })
    }).collect();
    (approximate.len()==1).then(||approximate[0])
}
pub fn verified(name:&str,text:&str,names:&[String])->bool {
    !is_variant(name) || resolved(name,text,names).is_some_and(|n|n==name)
}
pub fn rerank(ranks:&mut Vec<(String,f64)>,text:&str,names:&[String]) {
    ranks.sort_by(|a,b|b.1.total_cmp(&a.1));
    let Some(top)=ranks.first().map(|r|r.0.clone()) else {return};
    let Some(resolved)=resolved(&top,text,names).cloned() else {return};
    let stem=top.split_once(" - ").unwrap().0;
    for (name,score) in ranks.iter_mut() {if name!=&resolved && name.split_once(" - ").is_some_and(|(s,_)|s==stem) {*score=f64::NEG_INFINITY;}}
    ranks.sort_by(|a,b|b.1.total_cmp(&a.1));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_shared_prefix_cannot_prove_a_species_variant() {
        let names=&crate::scan_data().names.potion;
        for text in ["마이크로엘 리다 가루와•테","마이크로멜리아 가루약","마이크로멜리아 가루약-해"] {
            assert!(!verified("Slender-Limb Powder - Pest",text,names),"{text}");
            assert!(!verified("Slender-Limb Powder - Bone Sentry",text,names),"{text}");
        }
        assert!(verified("Slender-Limb Powder - Bone Sentry","마이크로멜리아 가루약\n1 그룹 해골 경비병을 첨가",names));
        assert!(!verified("Slender-Limb Powder - Pest","마이크로멜리아 가루약\n1 그룹 해골 경비병을 첨가",names));
        assert!(verified("Twin Hormone - Pest","쌍등이 호르몬•독층",names));
        assert!(!verified("Twin Hormone - Pest","쌍둥이 호르몬 독충 해골 경비병",names));
        assert!(verified("Spinal Solution - Agito","척수 용액-각성체\n몬스터를 변이시킨다",names));
    }
}
