//! Real Korean windows supplied with the bug report; names and values are assertions, never OCR output.
use std::path::PathBuf;
use vorax_scan::{scan, scan_data, Img};
use vorax_scan::window::{crop_to, windowed_game};

fn load(file: &str) -> Img {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/korean").join(file);
    let mut decoder = png::Decoder::new(std::fs::File::open(path).unwrap());
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().unwrap();
    let mut bytes = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut bytes).unwrap();
    let mut img = Img::new(info.width as usize, info.height as usize);
    let channels = info.color_type.samples();
    for (src, dest) in bytes[..info.buffer_size()].chunks_exact(channels).zip(img.data.chunks_exact_mut(4)) {
        dest.copy_from_slice(&[src[0], src[1], src[2], 255]);
    }
    img
}

#[test]
fn korean_windows_have_verified_tank_values_and_rounds() {
    for (file, round, values, refreshes) in [
        ("instrument-no-reroll.png", 1, vec![(1, 36), (5, 24), (1, 36), (1, 36)], None),
        ("potion-round1.png", 1, vec![(1, 36), (5, 24), (1, 36), (1, 36)], Some(3)),
        ("potion-round4-ultrawide.png", 4, vec![(1, 186), (5, 328), (1, 109)], Some(1)),
        ("potion-round6-ultrawide.png", 6, vec![(1, 186), (65, 328), (1, 109), (300, 1)], Some(1)),
    ] {
        let frame = load(file);
        let img = windowed_game(&frame).map_or_else(|| frame.clone(), |r| crop_to(&frame, &r));
        let reading = scan(&img, scan_data(), false);
        assert_eq!(reading.state.round, Some(round), "{file}: {}", serde_json::to_string(&reading).unwrap());
        assert!(reading.fields["round"].sure, "{file}: round");
        for (i, (a, q)) in values.iter().enumerate() {
            let tank = reading.state.tanks[i].as_ref().unwrap_or_else(|| panic!("{file}: tank {} empty: {}", i+1, serde_json::to_string(&reading).unwrap()));
            assert_eq!((tank.activity, tank.quantity), (*a, *q), "{file}: tank {}: {}", i+1, serde_json::to_string(&reading).unwrap());
            assert!(reading.fields[&format!("tank{i}.activity")].sure, "{file}: tank {} unreadable", i+1);
        }
        assert!(reading.state.tanks[values.len()..].iter().all(Option::is_none), "{file}: empty tanks");
        if let Some(n) = refreshes {
            assert!(reading.fields["refreshes"].sure, "{file}: rerolls");
            assert_eq!(reading.state.potion_refreshes, Some(n), "{file}: rerolls");
            let instruments = if round == 6 { vec!["Contracted Claws", "Black Goat Gut Suture"] } else { vec!["Contracted Claws"] };
            assert_eq!(reading.state.instruments, instruments, "{file}: equipped instruments");
        } else {
            assert!(reading.state.instruments.is_empty(), "{file}: starting map item is not an equipped instrument");
        }
    }
}

#[test]
fn localized_title_samples_survive_resizing_without_guessing_unknown_titles() {
    use vorax_scan::layout::layout;
    use vorax_scan::pixels::resize;
    use vorax_scan::titles::localized_title;
    for file in ["instrument-no-reroll.png", "potion-round1.png", "potion-round4-ultrawide.png", "potion-round6-ultrawide.png"] {
        let frame = load(file);
        let img = windowed_game(&frame).map_or_else(|| frame.clone(), |r| crop_to(&frame, &r));
        let base = layout(img.width, img.height);
        assert!(vorax_scan::titles::localized_hand_geometry(&img, &base), "{file}: Korean geometry");
        let known: [&str; 3] = match file {
            "instrument-no-reroll.png" => ["Incubation Sac", "Hypertrophic Pituitary", "Contracted Claws"],
            "potion-round1.png" => ["Spinal Solution - Aemberon", "Slender-Limb Powder - Pest", "Anesthetic Tincture - Aemberon"],
            "potion-round4-ultrawide.png" => ["Osteolytic Ointment", "Twin Hormone - Pest", "Potion Kit (Small)"],
            _ => ["Digestive Enzyme Solution", "Spinal Solution - Aemberon", "Viscous Bile Solution"],
        };
        let larger = resize(&img, (img.width as f64 * 1.25).round() as usize, (img.height as f64 * 1.25).round() as usize);
        let l = layout(larger.width, larger.height);
        for (j, name) in known.iter().enumerate() {
            let candidates = vec![(name.to_string(), 0.99)];
            assert_eq!(localized_title(&larger, &l.cards[j], l.k, &candidates).map(|x| x.0), Some(name.to_string()), "{file}: resized title {j}");
            assert!(localized_title(&img, &base.cards[j], base.k, &[("unknown card".into(), 0.99)]).is_none());
        }
    }
}

#[test]
fn translated_cards_use_art_and_korean_titles_without_english_text_shapes() {
    for (file, hand) in [
        ("instrument-no-reroll.png", vec!["Incubation Sac", "Hypertrophic Pituitary", "Contracted Claws"]),
        ("potion-round1.png", vec!["Spinal Solution - Aemberon", "Slender-Limb Powder - Pest", "Anesthetic Tincture - Aemberon"]),
        ("potion-round4-ultrawide.png", vec!["Osteolytic Ointment", "Twin Hormone - Pest", "Potion Kit (Small)"]),
        ("potion-round6-ultrawide.png", vec!["Digestive Enzyme Solution", "Spinal Solution - Aemberon", "Viscous Bile Solution"]),
    ] {
        let frame = load(file);
        let img = windowed_game(&frame).map_or_else(|| frame.clone(), |r| crop_to(&frame, &r));
        let reading = scan(&img, scan_data(), true);
        assert_eq!(reading.state.hand, hand, "{file}: {}", serde_json::to_string(&reading).unwrap());
    }
}

#[test]
fn isolated_korean_headings_are_read_before_the_picture_and_description() {
    let frame = load("potion-round6-ultrawide.png");
    let img = windowed_game(&frame).map_or_else(|| frame.clone(), |r| crop_to(&frame, &r));
    let data = scan_data();
    let names = scan(&img, data, false);
    let mut titles = ["소화 효소 용액", "척수 용액-변이 악마", "끈적한 담즙 용액"].into_iter();
    let read = vorax_scan::titles::read_titles_styled(&img, names, data, &mut |crop| {
        assert_eq!((crop.width, crop.height), (464, 120), "only a padded heading, no description or icon");
        titles.next().expect("one OCR call per readable heading").to_string()
    }, None, true);
    assert_eq!(read.state.hand, ["Digestive Enzyme Solution", "Spinal Solution - Aemberon", "Viscous Bile Solution"]);
    assert!(titles.next().is_none());
}

#[test]
fn a_single_unread_title_does_not_discard_the_other_verified_cards() {
    use vorax_scan::reading::Problem;
    let frame = load("potion-round6-ultrawide.png");
    let img = windowed_game(&frame).map_or_else(|| frame.clone(), |r| crop_to(&frame, &r));
    let mut reading = scan(&img, scan_data(), true);
    reading.fields.get_mut("hand1").unwrap().sure = false;
    reading.problems.push(Problem::UnknownArt { field: "hand1".into(), crop: vorax_scan::layout::layout(img.width, img.height).card_face[1], hand: None, text: None });
    let mut calls = 0;
    let read = vorax_scan::titles::read_titles_styled(&img, reading, scan_data(), &mut |_| {
        calls += 1;
        "척수 용액-변이 악마".to_string()
    }, None, true);
    assert_eq!(calls, 1);
    assert_eq!(read.state.hand, ["Digestive Enzyme Solution", "Spinal Solution - Aemberon", "Viscous Bile Solution"]);
}
