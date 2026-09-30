//! Choir lines (schema 2, issue #7) through the engine: the choir stem
//! carries the line, the lead stem is silent over it, the ducker keys on the
//! choir there, and a song with no choir line keys on the leads alone.

use engine::{mix, render, MixSettings, NoProgress, TrackId};
use sfcore::SR_F;

fn song_json(choir_line: bool) -> serde_json::Value {
    let mut chorus = serde_json::json!([
        {"syl": "*heave *ho *heave *ho", "ph": "hh iy v|hh ow|hh iy v|hh ow", "chords": ["G", "C"]},
        {"syl": "*roll *ye *bold", "ph": "r ow l|y iy|b ow l d", "chords": ["C", "D"]}]);
    if choir_line {
        chorus[1]["sing"] = "choir".into();
    }
    serde_json::json!({
        "schema_version": 2,
        "title": "Shanty", "note": "", "key": "G", "mode": "major", "meter": "4/4", "tempo": 100,
        "guitar": "strum", "voice": "baritone",
        "band": {"drums": "none", "bass": true, "harmonyGuitar": false, "harp": false,
                 "violin": false, "choir": choir_line, "harmonies": false, "doubles": false},
        "sections": [
            {"type": "verse", "lines": [
                {"syl": "*wind *fills the *sail", "ph": "w ih n d|f ih l z|dh ah|s ey l", "chords": ["G", "C"]}]},
            {"type": "chorus", "lines": chorus}
        ]
    })
}

fn render_song(choir_line: bool) -> (compose::prepare::Prepared, engine::Stems, song::Song) {
    let (s, _) = song::normalize_value(&song_json(choir_line)).expect("song reads");
    let (p, stems) = render(&s, 7, None, &NoProgress);
    (p, stems, s)
}

fn rms(x: &[f32]) -> f64 {
    (x.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / x.len().max(1) as f64).sqrt()
}

#[test]
fn a_choir_line_renders_on_the_choir_stem() {
    let (p, stems, s) = render_song(true);
    let li = (0..p.form.lines.len())
        .find(|&i| p.form.lines[i].part.is_choir())
        .expect("a choir line");
    let ns: Vec<_> = p.comp.lead.iter().filter(|n| n.line_idx == li).collect();
    let t0 = ns.iter().map(|n| n.t0).fold(f64::INFINITY, f64::min);
    let t1 = ns.iter().map(|n| n.t1).fold(f64::NEG_INFINITY, f64::max);
    let (a, b) = ((t0 * SR_F) as usize, (t1 * SR_F) as usize);

    let choir = stems.get(TrackId::Choir).expect("choir stem");
    assert_eq!(choir.key.len(), 1, "one keyed span");
    for ch in choir.audio.channels() {
        let d = ch.to_dense();
        assert!(d.iter().all(|v| v.is_finite()));
        assert!(rms(&d[a..b]) > 1e-3, "choir sings the line");
    }
    // The lead rests from a beat into the line (the tail of the line before
    // may ring into the start).
    let lead = stems
        .get(TrackId::Lead)
        .expect("lead stem")
        .audio
        .channels()[0]
        .to_dense();
    let a2 = a + (0.4 * SR_F) as usize;
    assert!(rms(&lead[a2..b]) < 1e-4, "lead silent over the line");
    assert!(rms(&lead[..a]) > 1e-3, "lead sings before it");

    // The duck curve dips under the choir, and the choir is not ducked.
    let duck = mix::duck_gains(&stems, &s.band, &MixSettings::default_for(&stems)).expect("duck");
    let low = duck[a2..b].iter().cloned().fold(1.0f32, f32::min);
    assert!(low < 0.9, "accompaniment ducks under the choir: {low}");
    assert!(
        low >= 1.0 - (1.0 - 10f32.powf(-5.0 / 20.0)) - 1e-3,
        "at most 5 dB"
    );
    let out = mix::mix(&stems, &s.band, 7);
    assert!(out.l.iter().chain(&out.r).all(|v| v.is_finite()));
}

#[test]
fn no_choir_line_means_lead_keyed_ducking() {
    let (p, stems, s) = render_song(false);
    assert!(p.form.lines.iter().all(|l| !l.part.is_choir()));
    assert!(stems.tracks.iter().flatten().all(|t| t.key.is_empty()));
    let duck = mix::duck_gains(&stems, &s.band, &MixSettings::default_for(&stems)).expect("duck");
    let lead = stems.get(TrackId::Lead).expect("lead").audio.channels()[0].to_dense();
    assert!(rms(&lead) > 1e-3);
    let low = duck.iter().cloned().fold(1.0f32, f32::min);
    assert!(low < 0.9, "ducks under the lead: {low}");
    assert!(
        low >= 1.0 - (1.0 - 10f32.powf(-5.0 / 20.0)) - 1e-3,
        "at most 5 dB"
    );
}
