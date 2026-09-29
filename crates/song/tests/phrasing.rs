//! Singer phrasing (design 5.1): wire `phrasing` at the top level and under
//! `duet`, the typed `Phrasing` model, and its repairs.

use serde_json::{json, Value};
use song::wire::{normalize_value, to_wire, Repair};
use song::{Delivery, Endings, Phrasing, SingerId};

fn song_with(extra: Value) -> Value {
    let mut v = json!({
        "title": "t", "note": "", "key": "C", "mode": "major", "meter": "4/4", "tempo": 90,
        "guitar": "strum", "voice": "baritone",
        "band": {"drums": "none", "bass": true, "harmonyGuitar": false, "harp": false, "violin": false,
                 "choir": false, "harmonies": false, "doubles": false},
        "sections": [{"type": "verse", "lines": [{"syl": "one *two", "ph": "w ah n|t uw", "chords": ["C"]}]}]
    });
    for (k, x) in extra.as_object().into_iter().flatten() {
        if x.is_null() {
            v.as_object_mut().map(|o| o.remove(k));
        } else {
            v[k] = x.clone();
        }
    }
    v
}

#[test]
fn absent_phrasing_is_silent_and_none() {
    let (s, r) = normalize_value(&song_with(json!({}))).unwrap();
    assert!(r.is_empty(), "{r:?}");
    assert_eq!(s.phrasing, None);
    assert_eq!(s.phrasing_of(SingerId::A), Phrasing::default());
}

#[test]
fn default_is_flowing_released() {
    assert_eq!(
        Phrasing::default(),
        Phrasing {
            delivery: Delivery::Flowing,
            endings: Endings::Released
        }
    );
}

#[test]
fn valid_phrasing_is_kept_with_no_repair() {
    let v = song_with(json!({"phrasing": {"delivery": "parlando", "endings": "clipped"}}));
    let (s, r) = normalize_value(&v).unwrap();
    assert!(r.is_empty(), "{r:?}");
    assert_eq!(
        s.phrasing,
        Some(Phrasing {
            delivery: Delivery::Parlando,
            endings: Endings::Clipped
        })
    );
}

#[test]
fn phrasing_not_an_object_is_none_with_both_fields_reported() {
    for bad in [json!("legato"), json!(5), json!([1]), json!(true)] {
        let v = song_with(json!({"phrasing": bad}));
        let (s, r) = normalize_value(&v).unwrap();
        assert_eq!(s.phrasing, None, "{bad}");
        assert!(
            r.contains(&Repair::DefaultedField {
                field: "phrasing.delivery"
            }),
            "{bad}: {r:?}"
        );
        assert!(
            r.contains(&Repair::DefaultedField {
                field: "phrasing.endings"
            }),
            "{bad}: {r:?}"
        );
    }
}

#[test]
fn one_bad_field_defaults_that_field_only() {
    let v = song_with(json!({"phrasing": {"delivery": "vigorous", "endings": "held"}}));
    let (s, r) = normalize_value(&v).unwrap();
    assert_eq!(
        s.phrasing,
        Some(Phrasing {
            delivery: Delivery::Flowing,
            endings: Endings::Held
        })
    );
    assert_eq!(
        r,
        vec![Repair::DefaultedField {
            field: "phrasing.delivery"
        }]
    );

    let v = song_with(json!({"phrasing": {"delivery": "legato"}}));
    let (s, r) = normalize_value(&v).unwrap();
    assert_eq!(
        s.phrasing,
        Some(Phrasing {
            delivery: Delivery::Legato,
            endings: Endings::Released
        })
    );
    assert_eq!(
        r,
        vec![Repair::DefaultedField {
            field: "phrasing.endings"
        }]
    );
}

#[test]
fn duet_phrasing_absent_is_none_duets_phrasing_of_falls_back() {
    let v = song_with(json!({
        "duet": {"voice": "alto"},
        "sections": [{"type": "verse", "sing": "B", "lines": [
            {"syl": "one *two", "ph": "w ah n|t uw", "chords": ["C"]}]}]
    }));
    let (s, r) = normalize_value(&v).unwrap();
    assert!(r.is_empty(), "{r:?}");
    assert_eq!(s.duet.unwrap().phrasing, None);
    // No song-level phrasing either: B falls back to the default.
    assert_eq!(s.phrasing_of(SingerId::B), Phrasing::default());
}

#[test]
fn duet_phrasing_falls_back_to_song_phrasing_then_default() {
    let base = json!({
        "duet": {"voice": "alto"},
        "phrasing": {"delivery": "legato", "endings": "held"},
        "sections": [{"type": "verse", "sing": "B", "lines": [
            {"syl": "one *two", "ph": "w ah n|t uw", "chords": ["C"]}]}]
    });
    let (s, r) = normalize_value(&song_with(base)).unwrap();
    assert!(r.is_empty(), "{r:?}");
    assert_eq!(
        s.phrasing_of(SingerId::A),
        Phrasing {
            delivery: Delivery::Legato,
            endings: Endings::Held
        }
    );
    // B has no phrasing of its own: falls back to the song's.
    assert_eq!(
        s.phrasing_of(SingerId::B),
        Phrasing {
            delivery: Delivery::Legato,
            endings: Endings::Held
        }
    );

    let mut with_b = song_with(json!({}));
    with_b["duet"] =
        json!({"voice": "alto", "phrasing": {"delivery": "detached", "endings": "clipped"}});
    with_b["phrasing"] = json!({"delivery": "legato", "endings": "held"});
    with_b["sections"][0]["sing"] = json!("B");
    let (s, r) = normalize_value(&with_b).unwrap();
    assert!(r.is_empty(), "{r:?}");
    assert_eq!(
        s.phrasing_of(SingerId::B),
        Phrasing {
            delivery: Delivery::Detached,
            endings: Endings::Clipped
        }
    );
    assert_eq!(
        s.phrasing_of(SingerId::A),
        Phrasing {
            delivery: Delivery::Legato,
            endings: Endings::Held
        }
    );
}

#[test]
fn duet_phrasing_not_an_object_reports_duet_prefixed_fields() {
    let mut v = song_with(json!({}));
    v["duet"] = json!({"voice": "alto", "phrasing": "legato"});
    v["sections"][0]["sing"] = json!("B");
    let (s, r) = normalize_value(&v).unwrap();
    assert_eq!(s.duet.unwrap().phrasing, None);
    assert!(
        r.contains(&Repair::DefaultedField {
            field: "duet.phrasing.delivery"
        }),
        "{r:?}"
    );
    assert!(
        r.contains(&Repair::DefaultedField {
            field: "duet.phrasing.endings"
        }),
        "{r:?}"
    );
}

#[test]
fn to_wire_round_trips_phrasing() {
    let v = song_with(json!({
        "phrasing": {"delivery": "detached", "endings": "clipped"},
        "duet": {"voice": "alto", "phrasing": {"delivery": "legato", "endings": "held"}},
        "sections": [{"type": "verse", "sing": "B", "lines": [
            {"syl": "one *two", "ph": "w ah n|t uw", "chords": ["C"]}]}]
    }));
    let (s, _) = normalize_value(&v).unwrap();
    let w = to_wire(&s);
    let (s2, r2) = normalize_value(&w).unwrap();
    assert!(r2.is_empty(), "{r2:?}");
    assert_eq!(s2, s);
}

#[test]
fn to_wire_omits_absent_phrasing_and_duet() {
    let (s, _) = normalize_value(&song_with(json!({}))).unwrap();
    let w = to_wire(&s);
    assert!(w.get("phrasing").is_none());
    assert!(w.get("duet").is_none());
}
