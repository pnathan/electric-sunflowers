//! Drums: per-kit patterns per bar and fills into louder sections, as hits.
//!
//! Level per bar: the section intensity, one lower for kits other than
//! full, at most 1 in a bridge; bars below 1 are silent. Brushes: a swish
//! per beat (0.95 of the beat long), taps on the backbeats, kicks from level
//! 2. Soft and full: kicks (4/4: beats 1 and 3, plus the and of 2 at level
//! 3, or every beat at level 3 from 112 bpm, the dance stomp; 6/8 full:
//! both beats, the jig's pulse), snare (rim for soft) on the backbeats,
//! hat (shaker for soft) on every grid step from level 2, ride on every
//! beat at level 3 (full only), a ride on the downbeat of a level-3 section. Fill: the last beat of a
//! section before a louder one, four (three in compound time) toms from 180
//! Hz down to 95 Hz (taps for brushes), panned left to right, rising in
//! level; the snare leaves the fill's beat. The last bar: a kick and a long
//! swish (brushes) or ride.
//!
//! Randomness: hit j of bar b draws from `Rng::event(seed, DRUM_TIMING,
//! b << 8 | j)`: onset jitter +-4 ms, velocity x (0.9..1.1).
//!
//! `DrumKit::None` plans no drums at all (`None`, not an empty part).

use compose::form::Form;
use compose::timeline::Timeline;
use sfcore::random::{tag, Rng, Tag};
use song::events::{DrumHit, DrumKind};
use song::{DrumKit, Meter, SectionKind};

const DRUM_TIMING: Tag = tag("drums.timing");

/// Onset jitter, seconds (+-).
const JITTER: f64 = 0.004;
/// Last-bar swish length, seconds.
const LAST_SWISH: f32 = 1.2;
/// Tempo from which a level-3 bar in 4/4 stomps (kick on every beat), bpm.
const STOMP_BPM: f64 = 112.0;
/// Fill tom tunings, Hz.
const FILL_HZ: [f32; 4] = [180.0, 150.0, 120.0, 95.0];

/// Collects the hits of one bar with their per-hit streams.
struct Bar<'a> {
    tl: &'a Timeline,
    seed: u64,
    bar: u64,
    j: u64,
    out: &'a mut Vec<DrumHit>,
}

impl Bar<'_> {
    fn hit(&mut self, kind: DrumKind, beat: f64, vel: f64, pan: f64) {
        let mut r = Rng::event(self.seed, DRUM_TIMING, (self.bar << 8) | self.j);
        self.j += 1;
        let t = self.tl.to_time(beat) + JITTER * r.bipolar();
        let vel = vel * (0.9 + 0.2 * r.uniform());
        self.out.push(DrumHit {
            t,
            kind,
            vel: vel as f32,
            pan: pan as f32,
        });
    }
}

/// The drum part for `kit`; `None` for `DrumKit::None`.
pub fn plan(kit: DrumKit, form: &Form, tl: &Timeline, seed: u64) -> Option<Vec<DrumHit>> {
    if kit == DrumKit::None {
        return None;
    }
    let bpb = form.bpb();
    let sub = form.sub();
    let meter = form.meter;
    let nb = form.bars.len();
    let mut hits: Vec<DrumHit> = Vec::new();

    for (bi, bar) in form.bars.iter().enumerate() {
        let sec = &form.sections[bar.sec];
        let mut level = sec.intensity.level();
        if kit != DrumKit::Full {
            level -= 1;
        }
        if sec.kind == SectionKind::Bridge {
            level = level.min(1);
        }
        if level < 1 {
            continue;
        }
        let b0 = (bi as i32 * bpb) as f64;
        let mut h = Bar {
            tl,
            seed,
            bar: bi as u64,
            j: 0,
            out: &mut hits,
        };
        if bi + 1 == nb {
            h.hit(DrumKind::Kick, b0, 0.7, 0.0);
            let tail = if kit == DrumKit::Brushes {
                DrumKind::Swish { dur: LAST_SWISH }
            } else {
                DrumKind::Ride
            };
            h.hit(tail, b0, 0.5, 0.35);
            continue;
        }
        let sec_end = bi + 1 == sec.start_bar + sec.n_bars;
        let fill = sec_end
            && form
                .sections
                .get(bar.sec + 1)
                .is_some_and(|nx| nx.intensity > sec.intensity);

        if kit == DrumKit::Brushes {
            for b in 0..bpb {
                let beat = b0 + b as f64;
                let dur = (tl.beat_dur(beat) * 0.95) as f32;
                h.hit(
                    DrumKind::Swish { dur },
                    beat,
                    0.35 + 0.1 * level as f64,
                    -0.1,
                );
            }
            let (taps, tap_vel, kicks): (&[f64], f64, &[(f64, f64)]) = match meter {
                Meter::Four4 => (&[1.0, 3.0], 0.5, &[(0.0, 0.45), (2.0, 0.35)]),
                Meter::Three4 => (&[1.0, 2.0], 0.35, &[(0.0, 0.45)]),
                Meter::Six8 => (&[1.0], 0.5, &[(0.0, 0.45)]),
            };
            for &k in taps {
                h.hit(DrumKind::Tap, b0 + k, tap_vel, -0.15);
            }
            if level >= 2 {
                for &(k, v) in kicks {
                    h.hit(DrumKind::Kick, b0 + k, v, 0.0);
                }
            }
        } else {
            let stomp = level >= 3 && tl.beat_dur(b0) <= 60.0 / STOMP_BPM;
            let kicks: &[f64] = match meter {
                Meter::Four4 if stomp => &[0.0, 1.0, 2.0, 3.0],
                Meter::Four4 if level >= 3 => &[0.0, 1.5, 2.0],
                Meter::Four4 => &[0.0, 2.0],
                Meter::Six8 if kit == DrumKit::Full => &[0.0, 1.0],
                Meter::Three4 | Meter::Six8 => &[0.0],
            };
            let snares: &[f64] = match meter {
                Meter::Four4 => &[1.0, 3.0],
                Meter::Three4 => &[1.0, 2.0],
                Meter::Six8 => &[1.0],
            };
            for &k in kicks {
                h.hit(DrumKind::Kick, b0 + k, 0.75, 0.0);
            }
            let sn = if kit == DrumKit::Soft {
                DrumKind::Rim
            } else {
                DrumKind::Snare
            };
            let sn_vel = if meter == Meter::Three4 { 0.45 } else { 0.6 };
            for &k in snares {
                if fill && k >= (bpb - 1) as f64 {
                    continue;
                }
                h.hit(sn, b0 + k, sn_vel, -0.12);
            }
            if level >= 2 {
                let kind = if kit == DrumKit::Soft {
                    DrumKind::Shaker
                } else {
                    DrumKind::Hat
                };
                for s in 0..(bpb * sub) {
                    let vel = if s % sub == 0 { 0.55 } else { 0.35 };
                    h.hit(kind, b0 + s as f64 / sub as f64, vel, 0.45);
                }
            }
            if level >= 3 && kit == DrumKit::Full {
                for b in 0..bpb {
                    h.hit(DrumKind::Ride, b0 + b as f64, 0.35, 0.4);
                }
            }
        }

        if fill {
            let fb = b0 + (bpb - 1) as f64;
            let steps = if sub == 3 { 3 } else { 4 };
            for k in 0..steps {
                let kind = if kit == DrumKit::Brushes {
                    DrumKind::Tap
                } else {
                    DrumKind::Tom { hz: FILL_HZ[k % 4] }
                };
                h.hit(
                    kind,
                    fb + k as f64 / steps as f64,
                    0.45 + k as f64 * 0.08,
                    -0.3 + k as f64 * 0.2,
                );
            }
        }
        if bi == sec.start_bar && sec.intensity.level() >= 3 && kit != DrumKit::Brushes {
            h.hit(DrumKind::Ride, b0, 0.55, 0.4);
        }
    }
    Some(hits)
}
