//! Korean display names and OCR aliases. Game terms, independent of the solver's stable English identifiers.
use std::{collections::HashMap, sync::OnceLock};

pub fn names() -> &'static HashMap<String, String> {
    static NAMES: OnceLock<HashMap<String, String>> = OnceLock::new();
    NAMES.get_or_init(|| serde_json::from_str(include_str!("../../../data/names-ko.json")).expect("Korean game names"))
}

pub fn name(english: &str) -> &str {
    names().get(english).map(String::as_str).unwrap_or(english)
}

pub fn hangul(c: char) -> bool { ('\u{ac00}'..='\u{d7a3}').contains(&c) }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_are_unique_and_known() {
        let data = crate::scan_data();
        let all: Vec<_> = data.names.potion.iter().chain(&data.names.instrument).chain(&data.names.protocol).collect();
        let mut values = std::collections::HashSet::new();
        for (en, ko) in names() {
            assert!(all.contains(&en), "unknown card: {en}");
            assert!(ko.chars().any(hangul));
            assert!(values.insert(ko), "duplicate translation: {ko}");
        }
        assert_eq!(name("Incubation Sac"), "부화낭");
        assert_eq!(name("unseen card"), "unseen card");
    }

    #[test]
    fn every_korean_title_ranks_its_english_identifier_first() {
        use crate::titles::name_fit;
        let d = crate::scan_data();
        for phase in [&d.names.potion, &d.names.instrument, &d.names.protocol] {
            for english in phase {
                let korean = name(english);
                if korean == english { continue; }
                let ocr = korean.replace(' ', "").replace('-', " ");
                let actual = name_fit(english, &ocr);
                let other = phase.iter().filter(|n| *n != english).map(|n| name_fit(n, &ocr)).fold(f64::NEG_INFINITY, f64::max);
                assert!(actual >= 6.0 && actual - other >= 3.0, "ambiguous {english} ({korean}): {actual} vs {other}");
            }
        }
        assert!(name_fit("Incubation Sac", "잘못된 화면") < 6.0);
        assert!(name_fit("Incubation Sac", "") < 6.0);
    }
}
