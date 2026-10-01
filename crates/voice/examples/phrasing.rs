//! Phrasing probe: the demo song's lead melody sung with each of the 12
//! phrasings (4 deliveries x 3 endings), from the voicing (`av`) control
//! track directly (no audio render needed). For each inter-onset interval
//! (one note's onset to the next), prints the fraction of frames voiced
//! (`av` above 5% of the whole song's peak `av`) and the silent gap right
//! before the next onset, both averaged over the song, then whether
//! detached < parlando < flowing <= legato in mean voiced fraction across
//! the three endings (design 5.2: legato sustains and leads in least,
//! detached and parlando clip more and lead in less).

use sfcore::random::Rng;
use song::events::{SingStyle, VocalNote};
use song::{Delivery, Endings, Phrasing, Voice};
use voice::articulation::syllables;
use voice::controls::{control_tracks, Window, FRAME_RATE};
use voice::{voice_params, VoiceSettings};

/// The demo song's lead melody as sung notes (`compose::prepare` plus the
/// `arrange::vocals::notes` conversion, duplicated here so `voice` need not
/// depend on `arrange`).
fn demo_lead_notes(seed: u64) -> (Voice, Vec<VocalNote>) {
    let raw: serde_json::Value = serde_json::from_str(include_str!("../../engine/src/demo.json")).expect("demo.json is JSON");
    let (song, _) = song::normalize_value(&raw).expect("demo.json normalises");
    let p = compose::prepare::prepare(&song, seed, None);
    let notes = compose::prepare::vocal_notes(&p.comp.lead, 1.0)
        .iter()
        .map(|n| VocalNote {
            t0: n.t0,
            t1: n.t1,
            midi: n.midi as f32,
            phones: n.ph.clone(),
            amp: n.amp as f32,
            stress: n.stress,
            phrase_start: n.phrase_start,
            phrase_end: n.phrase_end,
            grace: n.grace.map(|g| g as f32),
            legato: false,
            expr: Default::default(),
        })
        .collect();
    (p.voice, notes)
}

/// Mean voiced fraction of the inter-onset intervals, and the mean silent
/// gap right before an onset (the trailing run of frames at or below
/// `thresh`), seconds. `syl` from `articulation::syllables` for `notes`.
fn measure(av: &[f32], syl: &[voice::articulation::Syllable], thresh: f32) -> (f64, f64) {
    let mut frac_sum = 0.0;
    let mut gap_sum = 0.0;
    let mut n = 0usize;
    for k in 0..syl.len().saturating_sub(1) {
        let a = Window::frame(syl[k].onset_start).max(0) as usize;
        let b = (Window::frame(syl[k + 1].onset_start).max(0) as usize).min(av.len());
        if b <= a {
            continue;
        }
        let seg = &av[a..b];
        let voiced = seg.iter().filter(|&&x| x > thresh).count();
        frac_sum += voiced as f64 / seg.len() as f64;
        let gap = seg.iter().rev().take_while(|&&x| x <= thresh).count();
        gap_sum += gap as f64 / FRAME_RATE;
        n += 1;
    }
    if n == 0 {
        (0.0, 0.0)
    } else {
        (frac_sum / n as f64, gap_sum / n as f64)
    }
}

fn main() {
    let (voice, notes) = demo_lead_notes(1234);
    let frames = Window::frame(notes.last().map_or(0.0, |n| n.t1) + 1.0).max(0) as usize;

    let deliveries = [Delivery::Legato, Delivery::Flowing, Delivery::Parlando, Delivery::Detached];
    let endings = [Endings::Held, Endings::Released, Endings::Clipped];

    let mut mean_voiced = [0.0f64; 4];
    for (i, &delivery) in deliveries.iter().enumerate() {
        let mut sum = 0.0;
        for &ending in &endings {
            let style = SingStyle { phrasing: Phrasing { delivery, endings: ending }, ..SingStyle::LEAD };
            let settings = VoiceSettings::from(&style);
            let p = settings.apply(voice_params(voice));
            let ct = control_tracks(&notes, &p, &settings, frames, Rng::from_seed(1));
            let max_av = ct.av.iter().copied().fold(0.0f32, f32::max);
            let thresh = 0.05 * max_av;
            let syl = syllables(&notes, &p, &settings.phrasing);
            let (voiced_frac, gap) = measure(&ct.av, &syl, thresh);
            println!("{delivery} {ending}: voiced fraction {voiced_frac:.3} mean gap before onset {gap:.3} s");
            sum += voiced_frac;
        }
        mean_voiced[i] = sum / endings.len() as f64;
    }

    let (legato, flowing, parlando, detached) = (mean_voiced[0], mean_voiced[1], mean_voiced[2], mean_voiced[3]);
    println!("mean voiced fraction: legato {legato:.3} flowing {flowing:.3} parlando {parlando:.3} detached {detached:.3}");
    let ok = detached < parlando && parlando < flowing && flowing <= legato;
    println!("{}", if ok { "PASS" } else { "FAIL" });
}
