//! Singer phrasing (design 5.2): the default is the legacy path, and every
//! delivery and ending gives sensible note lengths.

use sfcore::SR_F;
use song::events::{SingStyle, VocalNote};
use song::{Delivery, Endings, Phoneme, Phrasing, Voice};
use voice::{phrase_notes, render_phrases, PhrasingParams, VoiceSettings};

const DELIVERIES: [Delivery; 4] = [
    Delivery::Legato,
    Delivery::Flowing,
    Delivery::Parlando,
    Delivery::Detached,
];
const ENDINGS: [Endings; 3] = [Endings::Held, Endings::Released, Endings::Clipped];

fn note(
    t0: f64,
    t1: f64,
    midi: f32,
    phones: Vec<Phoneme>,
    phrase_start: bool,
    phrase_end: bool,
) -> VocalNote {
    VocalNote {
        t0,
        t1,
        midi,
        phones,
        amp: 1.0,
        stress: true,
        phrase_start,
        phrase_end,
        grace: None,
        legato: false,
        expr: Default::default(),
    }
}

/// Two phrases of connected notes (4 ms legato gaps, as compose writes
/// them) with onset and coda consonants.
fn phrase() -> Vec<VocalNote> {
    let n = |t0: f64, m: f32, ph: Vec<Phoneme>, s: bool, e: bool| note(t0, t0 + 0.496, m, ph, s, e);
    vec![
        n(
            0.5,
            55.0,
            vec![Phoneme::B, Phoneme::Aa, Phoneme::T],
            true,
            false,
        ),
        n(1.0, 57.0, vec![Phoneme::S, Phoneme::Iy], false, false),
        n(1.5, 59.0, vec![Phoneme::D, Phoneme::Aa], false, false),
        note(
            2.0,
            2.9,
            55.0,
            vec![Phoneme::T, Phoneme::Aa, Phoneme::S],
            false,
            true,
        ),
        n(4.0, 57.0, vec![Phoneme::B, Phoneme::Iy], true, false),
        note(4.5, 5.4, 55.0, vec![Phoneme::K, Phoneme::Aa], false, true),
    ]
}

fn render(notes: &[VocalNote], settings: &VoiceSettings) -> Vec<f32> {
    let len = (notes.last().unwrap().t1 * SR_F) as usize + (1.5 * SR_F) as usize;
    let mut out = vec![0.0f32; len];
    render_phrases(notes, Voice::Baritone, settings, 7, len, |s, x| {
        out[s..s + x.len()].copy_from_slice(x)
    });
    out
}

fn bits(x: &[f32]) -> Vec<u32> {
    x.iter().map(|v| v.to_bits()).collect()
}

/// The pre-phrasing pipeline no longer exists as a separate path, so the
/// comparison is as close as the API allows: (a) `VoiceSettings::default()`
/// (a `SingStyle::LEAD`), (b) a style with Flowing + Released set
/// explicitly, (c) settings whose `phrasing` holds the numbers the old code
/// hard-coded (ONSET_SHARE 0.45, swell 0.16, fade 0.4 from 0.55, breath
/// 1.0, no sustain or end trimming, vowel on the beat). All three renders
/// are bit-identical, and `phrase_notes` borrows for them.
#[test]
fn flowing_released_is_bit_identical_to_the_legacy_constants() {
    let notes = phrase();
    let default = VoiceSettings::default();

    let style = SingStyle {
        phrasing: Phrasing {
            delivery: Delivery::Flowing,
            endings: Endings::Released,
        },
        ..SingStyle::LEAD
    };
    let explicit = VoiceSettings::from(&style);

    let legacy = VoiceSettings {
        phrasing: PhrasingParams {
            sustain: 1.0,
            onset_share: 0.45,
            lead_in: 1.0,
            vibrato: 1.0,
            glide: 1.0,
            swell: 0.16,
            breath: 1.0,
            end_len: 1.0,
            fade_depth: 0.4,
            fade_from: 0.55,
        },
        ..VoiceSettings::default()
    };

    assert!(matches!(
        phrase_notes(&notes, &explicit.phrasing),
        std::borrow::Cow::Borrowed(_)
    ));
    assert!(matches!(
        phrase_notes(&notes, &legacy.phrasing),
        std::borrow::Cow::Borrowed(_)
    ));

    let a = render(&notes, &default);
    assert!(a.iter().any(|x| x.abs() > 1e-3), "render is silent");
    assert_eq!(
        bits(&a),
        bits(&render(&notes, &explicit)),
        "explicit Flowing+Released differs from default"
    );
    assert_eq!(
        bits(&a),
        bits(&render(&notes, &legacy)),
        "legacy constants differ from default"
    );
}

/// Other phrasings do change the sound (the test above is not vacuous).
#[test]
fn other_phrasings_change_the_render() {
    let notes = phrase();
    let a = render(&notes, &VoiceSettings::default());
    for (delivery, endings) in [
        (Delivery::Parlando, Endings::Clipped),
        (Delivery::Detached, Endings::Held),
        (Delivery::Legato, Endings::Released),
    ] {
        let s = VoiceSettings::from(&SingStyle {
            phrasing: Phrasing { delivery, endings },
            ..SingStyle::LEAD
        });
        let b = render(&notes, &s);
        assert!(b.iter().all(|x| x.is_finite()));
        assert_ne!(
            bits(&a),
            bits(&b),
            "{delivery:?}/{endings:?} rendered like the default"
        );
    }
}

/// Sounding lengths after `phrase_notes`, for every delivery and ending:
/// positive, finite, within the written slot, and ordered.
#[test]
fn note_lengths_are_sensible_and_ordered() {
    let notes = phrase();
    let len_of = |d: Delivery, e: Endings| -> Vec<f64> {
        let p = PhrasingParams::of(Phrasing {
            delivery: d,
            endings: e,
        });
        let out = phrase_notes(&notes, &p);
        assert_eq!(out.len(), notes.len());
        out.iter()
            .zip(&notes)
            .map(|(o, n)| {
                let l = o.t1 - o.t0;
                assert_eq!(o.t0, n.t0);
                assert!(l.is_finite() && l > 0.0, "{d:?}/{e:?}: length {l}");
                assert!(
                    o.t1 <= n.t1 + 1e-12,
                    "{d:?}/{e:?}: note lengthened past its written end"
                );
                l
            })
            .collect()
    };
    for e in ENDINGS {
        let det = len_of(Delivery::Detached, e);
        let par = len_of(Delivery::Parlando, e);
        let flo = len_of(Delivery::Flowing, e);
        let leg = len_of(Delivery::Legato, e);
        for (k, n) in notes.iter().enumerate() {
            if n.phrase_end {
                // Deliveries do not touch phrase-final notes; endings do.
                assert_eq!(det[k], par[k]);
                assert_eq!(par[k], flo[k]);
                assert_eq!(flo[k], leg[k]);
            } else {
                assert!(
                    det[k] < par[k],
                    "note {k} {e:?}: detached {} parlando {}",
                    det[k],
                    par[k]
                );
                assert!(par[k] <= flo[k] && flo[k] <= leg[k], "note {k} {e:?}");
            }
        }
    }
    // Endings: clipped < held = released on phrase-final notes; others alone.
    for d in DELIVERIES {
        let held = len_of(d, Endings::Held);
        let rel = len_of(d, Endings::Released);
        let clip = len_of(d, Endings::Clipped);
        for (k, n) in notes.iter().enumerate() {
            assert_eq!(held[k], rel[k]);
            if n.phrase_end {
                assert!(clip[k] < held[k]);
                assert!((clip[k] - 0.6 * (n.t1 - n.t0)).abs() < 1e-12);
            } else {
                assert_eq!(clip[k], held[k]);
            }
        }
    }
}

/// Degenerate inputs neither panic nor lengthen notes: empty, one note,
/// zero-length and very short notes, an unsorted pair.
#[test]
fn degenerate_notes_are_safe() {
    let ph = |d, e| {
        PhrasingParams::of(Phrasing {
            delivery: d,
            endings: e,
        })
    };
    for d in DELIVERIES {
        for e in ENDINGS {
            let p = ph(d, e);
            assert!(phrase_notes(&[], &p).is_empty());
            let cases = [
                vec![note(1.0, 1.3, 60.0, vec![Phoneme::Aa], true, true)],
                vec![note(1.0, 1.0, 60.0, vec![Phoneme::Aa], true, true)],
                vec![
                    note(1.0, 1.01, 60.0, vec![Phoneme::Aa], true, false),
                    note(1.02, 1.03, 60.0, vec![Phoneme::Aa], false, true),
                ],
                vec![
                    note(2.0, 2.5, 60.0, vec![Phoneme::Aa], true, false),
                    note(1.0, 1.5, 60.0, vec![Phoneme::Aa], false, true),
                ],
            ];
            for notes in cases {
                let out = phrase_notes(&notes, &p);
                for (o, n) in out.iter().zip(&notes) {
                    assert!(o.t1.is_finite() && o.t1 <= n.t1 + 1e-12, "{d:?}/{e:?}");
                }
                // Rendering short and odd notes must not panic and must be finite.
                let s = VoiceSettings::from(&SingStyle {
                    phrasing: Phrasing {
                        delivery: d,
                        endings: e,
                    },
                    ..SingStyle::LEAD
                });
                let mut sorted = notes.clone();
                sorted.sort_by(|a, b| a.t0.total_cmp(&b.t0));
                let len = (3.5 * SR_F) as usize;
                let mut buf = vec![0.0f32; len];
                render_phrases(&sorted, Voice::Alto, &s, 3, len, |st, x| {
                    buf[st..st + x.len()].copy_from_slice(x)
                });
                assert!(buf.iter().all(|x| x.is_finite()), "{d:?}/{e:?}: non-finite");
            }
        }
    }
}
