//! Musical properties of the composer on the demo song.

use compose::melody::{cadence_for, Cadence};
use compose::prepare::{prepare, Prepared};
use song::{Pc, SectionKind, Song, Voice};

fn demo() -> Song {
    let raw: serde_json::Value =
        serde_json::from_str(include_str!("../../engine/src/demo.json")).expect("demo.json is JSON");
    let (s, repairs) = song::normalize_value(&raw).expect("demo normalises");
    assert!(repairs.is_empty(), "{repairs:?}");
    s
}

const SEEDS: u64 = 200;

#[test]
fn stressed_syllables_land_on_strong_slots() {
    let s = demo();
    let (mut on, mut all) = (0usize, 0usize);
    for seed in 0..SEEDS {
        let p = prepare(&s, seed, None);
        for l in &p.form.lines {
            let rh = l.rh.as_ref().expect("composed");
            for (syl, w) in l.syls.iter().zip(&rh.weights) {
                if syl.stress {
                    all += 1;
                    on += usize::from(*w >= 0.5);
                }
            }
        }
    }
    let rate = on as f64 / all as f64;
    eprintln!("stressed on weight >= 0.5: {on}/{all} = {rate:.3}");
    assert!(rate >= 0.8, "{rate}");
}

/// (lines ending on the tonic, tonic-cadence lines) over chords that hold
/// the tonic.
fn tonic_cadences(p: &Prepared) -> (usize, usize) {
    let (mut hit, mut n) = (0, 0);
    let tonic = Pc::new(p.tonic);
    for l in &p.form.lines {
        let sec = &p.form.sections[l.sec];
        if cadence_for(sec.kind, l.li, sec.lines.len()) != Cadence::Tonic {
            continue;
        }
        let rh = l.rh.as_ref().expect("composed");
        let (Some(&last), Some(&on)) = (l.pitches.as_ref().expect("composed").last(), rh.onsets.last()) else {
            continue;
        };
        let beat = (l.start_bar as i32 * p.form.bpb()) as f64 + on + 0.01;
        if !p.timeline.chord_at(&p.form, beat).tones.contains(tonic) {
            continue;
        }
        n += 1;
        hit += usize::from(Pc::new(last) == tonic);
    }
    (hit, n)
}

#[test]
fn tonic_cadences_end_on_the_tonic() {
    let s = demo();
    let (mut hit, mut n) = (0, 0);
    for seed in 0..SEEDS {
        let (h, k) = tonic_cadences(&prepare(&s, seed, None));
        hit += h;
        n += k;
    }
    eprintln!("tonic cadences on the tonic: {hit}/{n}");
    assert!(n > SEEDS as usize);
    assert_eq!(hit, n);
}

#[test]
fn melody_fits_the_voice() {
    let s = demo();
    for &v in Voice::ALL {
        let (mut inside, mut all) = (0usize, 0usize);
        for seed in 0..SEEDS / 4 {
            let p = prepare(&s, seed, Some(v));
            let r = v.range();
            let mut ms: Vec<i32> = p.comp.lead.iter().map(|n| n.midi).collect();
            ms.sort_unstable();
            let lo = ms[ms.len() / 20];
            let hi = ms[ms.len() * 19 / 20];
            // The 5th-95th percentile span sits inside the range, give or take a tone.
            assert!(lo >= r.lo as i32 - 2 && hi <= r.hi as i32 + 2, "{v} seed {seed}: {lo}-{hi} vs {r:?}");
            inside += ms.iter().filter(|&&m| (r.lo as i32..=r.hi as i32).contains(&m)).count();
            all += ms.len();
        }
        let rate = inside as f64 / all as f64;
        eprintln!("{v}: notes inside the range {rate:.3}");
        assert!(rate >= 0.9, "{v}: {rate}");
    }
}

#[test]
fn same_seed_same_melody() {
    let s = demo();
    let a = prepare(&s, 1234, None);
    let b = prepare(&s, 1234, None);
    let key = |p: &Prepared| {
        p.comp
            .lead
            .iter()
            .map(|n| (n.midi, n.grace, n.beat.to_bits(), n.dur.to_bits(), n.t0.to_bits(), n.t1.to_bits()))
            .collect::<Vec<_>>()
    };
    assert_eq!(key(&a), key(&b));
    let inst = |p: &Prepared| p.comp.inst.iter().map(|n| (n.midi, n.beat.to_bits())).collect::<Vec<_>>();
    assert_eq!(inst(&a), inst(&b));
    let c = prepare(&s, 1235, None);
    assert_ne!(key(&a), key(&c));
}

#[test]
fn repeated_chorus_repeats_its_pitches() {
    let s = demo();
    for seed in 0..20 {
        let p = prepare(&s, seed, None);
        let choruses: Vec<&compose::form::Sec> =
            p.form.sections.iter().filter(|x| x.kind == SectionKind::Chorus && x.is_sung()).collect();
        assert!(choruses.len() >= 2);
        let pitches = |sec: &compose::form::Sec| {
            sec.lines.iter().map(|&i| p.form.lines[i].pitches.clone().expect("composed")).collect::<Vec<_>>()
        };
        let first = pitches(choruses[0]);
        for c in &choruses[1..] {
            let same_text = c.lines.iter().zip(&choruses[0].lines).all(|(&a, &b)| p.form.lines[a].text == p.form.lines[b].text);
            if same_text && c.lines.len() == choruses[0].lines.len() {
                assert_eq!(pitches(c), first, "seed {seed}");
            }
        }
    }
}
