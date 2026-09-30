//! The accompaniment guitar: a fretboard voicing per chord and a strum or
//! picking pattern per bar, planned as note lists for the six strings.
//!
//! Voicing. Exhaustive search over hand positions 0-9: at each position a
//! string can sound open or at one of the four frets from the position, or
//! be muted. The bass note (the chord's bass pitch class) is on one of the
//! three lowest strings and the strings below it are muted. A shape is
//! playable with at most four fretted notes spanning at most three frets.
//! Cost terms (higher is better): -6 per missing essential tone, -0.6
//! without the fifth, -6 for a slash chord without its root, -0.3 per
//! position, +0.45 per open string, -1.6 per muted string above the bass
//! (-1.2 for the top string), -0.25 per bass-string index, -0.5 for a
//! doubled third, -2 with fewer than four notes. The first shape to beat
//! the best score strictly wins, in search order (position, bass string,
//! bass fret, then per string each fret option before muting it).
//!
//! Patterns. A const table of (slot, stroke, velocity) per pattern and
//! meter; slots are grid steps of the meter. Quiet sections (intensity <= 1)
//! strum lightly, a strummed bridge is fingerpicked, the last bar is one
//! slow down strum. Strums spread their strings by 9 ms (down) or 7 ms (up)
//! per string, 28 ms on the last bar, with +-20% jitter.
//!
//! Damping. At every chord change a string stops 30 ms after the change
//! (the change is anticipated by 15 ms) unless the new voicing keeps its
//! note; a string never rings longer than `MAX_RING`. A new note on the
//! same string ends the previous one (the instrument does this).
//!
//! Randomness: stroke j of bar b draws from `Rng::event(seed, GUITAR_STROKE,
//! b << 8 | j)`: onset jitter +-6 ms, velocity x (0.92..1.08), per-string
//! strum spread.

use std::collections::{BTreeMap, HashMap};

use compose::form::Form;
use compose::timeline::Timeline;
use sfcore::random::{tag, Rng, Tag};
use song::events::StringNote;
use song::{Chord, GuitarPattern, Meter, Pc, PcSet, SectionKind, Song};

/// Open strings, MIDI: E2 A2 D3 G3 B3 E4.
pub const OPEN_STRINGS: [u8; 6] = [40, 45, 50, 55, 59, 64];

/// Per-stroke stream.
const GUITAR_STROKE: Tag = tag("guitar.stroke");

/// Longest a string rings without a new note or a chord change, seconds.
pub const MAX_RING: f64 = 6.0;
/// A chord change damps strings this long before the change's beat, seconds.
const CHANGE_LEAD: f64 = 0.015;
/// A damped string stops this long after the damping event, seconds.
const DAMP_TIME: f64 = 0.03;

/// Highest hand position searched.
const MAX_POS: u8 = 9;
/// At most this many fretted (non-open) notes.
const MAX_FRETTED: usize = 4;
/// Largest fret span of the fretted notes.
const MAX_SPAN: u8 = 3;

/// A chord shape: fret and MIDI note per string (0 = low E), `None` muted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Voicing {
    pub frets: [Option<u8>; 6],
    pub notes: [Option<u8>; 6],
}

impl Voicing {
    fn from_frets(frets: [Option<u8>; 6]) -> Voicing {
        let mut notes = [None; 6];
        for s in 0..6 {
            notes[s] = frets[s].map(|f| OPEN_STRINGS[s] + f);
        }
        Voicing { frets, notes }
    }

    /// Lowest sounding string.
    pub fn bass_string(&self) -> Option<usize> {
        self.notes.iter().position(|n| n.is_some())
    }
}

/// What the voicing cost reads from a chord; the memo key.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct VoicingKey {
    tones: PcSet,
    root: Pc,
    bass: Pc,
    third: Option<Pc>,
    fifth: Option<Pc>,
    essential: PcSet,
}

impl VoicingKey {
    fn of(ch: &Chord) -> VoicingKey {
        VoicingKey {
            tones: ch.tones,
            root: ch.root,
            bass: ch.bass,
            third: ch.third,
            fifth: ch.fifth,
            essential: ch.essential,
        }
    }
}

/// Fret options of one string at one position: up to five, in search order.
#[derive(Clone, Copy, Default)]
struct Opts {
    f: [u8; 5],
    n: usize,
}

impl Opts {
    fn as_slice(&self) -> &[u8] {
        &self.f[..self.n]
    }
}

/// Frets on string `s` that sound a tone of `pcs` at hand position `pos`:
/// open, then the four frets from `pos`, without repeats.
fn opts_for(s: usize, pos: u8, pcs: PcSet) -> Opts {
    let mut o = Opts::default();
    for f in [0, pos, pos + 1, pos + 2, pos + 3] {
        if pcs.contains(Pc::new((OPEN_STRINGS[s] + f) as i32)) && !o.as_slice().contains(&f) {
            o.f[o.n] = f;
            o.n += 1;
        }
    }
    o
}

struct Search<'a> {
    key: &'a VoicingKey,
    opts: [Opts; 6],
    bass_string: usize,
    pos: u8,
    cur: [Option<u8>; 6],
    best: Option<[Option<u8>; 6]>,
    best_score: f64,
}

impl Search<'_> {
    /// Tries every option on strings `s..6` (each fret, then muted).
    fn rec(&mut self, s: usize) {
        if s == 6 {
            self.score();
            return;
        }
        for k in 0..self.opts[s].n {
            self.cur[s] = Some(self.opts[s].f[k]);
            self.rec(s + 1);
        }
        self.cur[s] = None;
        self.rec(s + 1);
    }

    fn score(&mut self) {
        let cur = &self.cur;
        let (mut fretted, mut lo, mut hi) = (0usize, u8::MAX, 0u8);
        for f in cur.iter().flatten().filter(|&&f| f > 0) {
            fretted += 1;
            lo = lo.min(*f);
            hi = hi.max(*f);
        }
        if fretted > MAX_FRETTED || (fretted > 0 && hi - lo > MAX_SPAN) {
            return;
        }
        let k = self.key;
        let mut have = PcSet::EMPTY;
        let mut n_notes = 0;
        let mut thirds = 0;
        for (s, f) in cur.iter().enumerate() {
            if let Some(f) = f {
                let pc = Pc::new((OPEN_STRINGS[s] + f) as i32);
                have.insert(pc);
                n_notes += 1;
                if Some(pc) == k.third {
                    thirds += 1;
                }
            }
        }
        let mut sc = -6.0 * k.essential.difference(have).len() as f64;
        if k.fifth.is_some_and(|p| !have.contains(p)) {
            sc -= 0.6;
        }
        if k.bass != k.root && !have.contains(k.root) {
            sc -= 6.0;
        }
        sc -= 0.3 * self.pos as f64;
        sc += 0.45 * cur.iter().filter(|x| **x == Some(0)).count() as f64;
        for (i, x) in cur.iter().enumerate().skip(self.bass_string + 1) {
            if x.is_none() {
                sc -= if i == 5 { 1.2 } else { 1.6 };
            }
        }
        sc -= 0.25 * self.bass_string as f64;
        if thirds > 1 {
            sc -= 0.5;
        }
        if n_notes < 4 {
            sc -= 2.0;
        }
        if sc > self.best_score {
            self.best_score = sc;
            self.best = Some(*cur);
        }
    }
}

/// The best open-position or barre voicing of `ch` (see the module docs).
pub fn voicing(ch: &Chord) -> Voicing {
    search(&VoicingKey::of(ch))
}

fn search(key: &VoicingKey) -> Voicing {
    let pcs = key.tones.with(key.bass);
    let mut sr = Search {
        key,
        opts: [Opts::default(); 6],
        bass_string: 0,
        pos: 0,
        cur: [None; 6],
        best: None,
        best_score: f64::NEG_INFINITY,
    };
    for pos in 0..=MAX_POS {
        for s in 0..6 {
            sr.opts[s] = opts_for(s, pos, pcs);
        }
        sr.pos = pos;
        for (bs, &open) in OPEN_STRINGS.iter().enumerate().take(3) {
            let bass_opts = sr.opts[bs];
            for &bf in bass_opts.as_slice() {
                if Pc::new((open + bf) as i32) != key.bass {
                    continue;
                }
                sr.bass_string = bs;
                sr.cur = [None; 6];
                sr.cur[bs] = Some(bf);
                sr.rec(bs + 1);
            }
        }
    }
    match sr.best {
        Some(f) => Voicing::from_frets(f),
        // Unreachable: frets 0-12 on the low E string hold every pitch
        // class, and the bass note alone is a playable shape. Kept total:
        // the bass on the A string.
        None => {
            let mut f = [None; 6];
            f[1] = Some(key.bass.transpose(-(OPEN_STRINGS[1] as i32)).get());
            Voicing::from_frets(f)
        }
    }
}

/// One stroke of a pattern.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stroke {
    /// Down strum over every sounding string.
    Down,
    /// Up strum over the top four sounding strings from string 2 up.
    Up,
    /// Light down strum from string 3 up.
    DownLite,
    /// Light up strum over the top four sounding strings from string 3 up.
    UpLite,
    /// The bass note: the lowest sounding string.
    Bass,
    /// Down strum over the sounding strings 0-2 (the low three).
    DownBass,
    /// Up strum over the sounding strings 0-2, top first, at most four.
    UpBass,
    /// Alternate bass: the next sounding string above the bass, at most string 3.
    AltBass,
    /// Pick string 3 (G), 4 (B) or 5 (high E), or the nearest sounding one below.
    G,
    B,
    E,
}

/// Pattern played in a bar: the song's pattern, or a lighter strum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pat {
    Strum = 0,
    StrumLite = 1,
    Fingerpick = 2,
    Travis = 3,
}

impl Pat {
    fn of(g: GuitarPattern) -> Pat {
        match g {
            GuitarPattern::Strum => Pat::Strum,
            GuitarPattern::Fingerpick | GuitarPattern::Arpeggio => Pat::Fingerpick,
            GuitarPattern::Travis => Pat::Travis,
        }
    }
}

fn meter_index(m: Meter) -> usize {
    match m {
        Meter::Four4 => 0,
        Meter::Three4 => 1,
        Meter::Six8 => 2,
    }
}

/// (slot, stroke, velocity); slots are grid steps of the meter.
type Pattern = &'static [(u8, Stroke, f32)];

use Stroke::{
    AltBass as B2, Bass as Bs, Down as D, DownLite as Dl, Up as U, UpLite as Ul, B, E, G,
};

/// Patterns indexed by `[Pat as usize][meter_index]` (4/4, 3/4, 6/8).
const PATTERNS: [[Pattern; 3]; 4] = [
    // Strum
    [
        &[
            (0, D, 0.85),
            (2, D, 0.7),
            (3, U, 0.45),
            (5, U, 0.5),
            (6, D, 0.65),
            (7, U, 0.45),
        ],
        &[(0, Bs, 0.9), (2, D, 0.6), (4, D, 0.6), (5, U, 0.4)],
        &[
            (0, D, 0.85),
            (2, U, 0.4),
            (3, D, 0.7),
            (4, U, 0.4),
            (5, U, 0.45),
        ],
    ],
    // StrumLite
    [
        &[
            (0, Bs, 0.85),
            (2, Dl, 0.55),
            (4, B2, 0.75),
            (6, Dl, 0.55),
            (7, Ul, 0.35),
        ],
        &[(0, Bs, 0.85), (2, Dl, 0.5), (4, Dl, 0.5)],
        &[(0, Bs, 0.85), (2, Dl, 0.45), (3, B2, 0.7), (5, Dl, 0.45)],
    ],
    // Fingerpick
    [
        &[
            (0, Bs, 0.85),
            (1, G, 0.5),
            (2, B, 0.55),
            (3, E, 0.6),
            (4, B2, 0.75),
            (5, B, 0.5),
            (6, G, 0.5),
            (7, B, 0.5),
        ],
        &[
            (0, Bs, 0.85),
            (1, G, 0.5),
            (2, B, 0.55),
            (3, E, 0.6),
            (4, B, 0.5),
            (5, G, 0.5),
        ],
        &[
            (0, Bs, 0.85),
            (1, G, 0.5),
            (2, B, 0.55),
            (3, E, 0.6),
            (4, B, 0.5),
            (5, G, 0.5),
        ],
    ],
    // Travis
    [
        &[
            (0, Bs, 0.85),
            (0, E, 0.55),
            (1, B, 0.45),
            (2, B2, 0.75),
            (3, G, 0.5),
            (4, Bs, 0.8),
            (5, E, 0.5),
            (6, B2, 0.75),
            (7, B, 0.45),
        ],
        &[
            (0, Bs, 0.85),
            (0, E, 0.55),
            (1, B, 0.45),
            (2, B2, 0.7),
            (3, G, 0.5),
            (4, B2, 0.7),
            (5, B, 0.45),
        ],
        &[
            (0, Bs, 0.85),
            (0, E, 0.5),
            (1, G, 0.45),
            (2, B, 0.5),
            (3, B2, 0.75),
            (4, B, 0.45),
            (5, G, 0.45),
        ],
    ],
];

/// The last bar: one slow down strum.
const LAST_BAR: Pattern = &[(0, D, 0.8)];

/// Strum spread per string, seconds: down, up, last bar.
const SPREAD_DOWN: f64 = 0.009;
const SPREAD_UP: f64 = 0.007;
const SPREAD_LAST: f64 = 0.028;

/// Direction of a stroke.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Down,
    Up,
}

/// Which strings of the voicing a stroke sounds. `All`, `Bass` (strings 0-2
/// of the voicing, the low three) and `Treble` (strings 3-5, the high three)
/// are strums in the stroke's direction; `Low` (the lowest sounding string),
/// `Alt` (the next above it, at most string 3), `G`, `B` and `E` (string 3,
/// 4 or 5, or the nearest sounding one below) are single picked notes and
/// ignore the direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strings {
    All,
    Bass,
    Treble,
    Low,
    Alt,
    G,
    B,
    E,
}

impl Strings {
    pub const ALL: [Strings; 8] = [
        Strings::All,
        Strings::Bass,
        Strings::Treble,
        Strings::Low,
        Strings::Alt,
        Strings::G,
        Strings::B,
        Strings::E,
    ];

    /// The name the arranger edits use.
    pub fn name(self) -> &'static str {
        match self {
            Strings::All => "all",
            Strings::Bass => "bass",
            Strings::Treble => "treble",
            Strings::Low => "low",
            Strings::Alt => "alt",
            Strings::G => "g",
            Strings::B => "b",
            Strings::E => "e",
        }
    }

    pub fn parse(s: &str) -> Option<Strings> {
        let s = s.trim().to_ascii_lowercase();
        Strings::ALL.into_iter().find(|x| x.name() == s)
    }
}

impl Dir {
    pub fn name(self) -> &'static str {
        match self {
            Dir::Down => "down",
            Dir::Up => "up",
        }
    }

    pub fn parse(s: &str) -> Option<Dir> {
        match s.trim().to_ascii_lowercase().as_str() {
            "down" => Some(Dir::Down),
            "up" => Some(Dir::Up),
            _ => None,
        }
    }
}

/// One stroke of a bar: the form the planner and the arranger pass share.
/// `beat` counts from the start of the bar; `vel` is the stroke's velocity
/// before the random spread of 0.92-1.08; `damp` makes the struck strings a
/// muted chop that stops `CHOP_TIME` after each onset.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GuitarStroke {
    pub beat: f64,
    pub dir: Dir,
    pub strings: Strings,
    pub damp: bool,
    pub vel: f64,
}

impl Stroke {
    fn of(dir: Dir, strings: Strings) -> Stroke {
        match (strings, dir) {
            (Strings::All, Dir::Down) => Stroke::Down,
            (Strings::All, Dir::Up) => Stroke::Up,
            (Strings::Treble, Dir::Down) => Stroke::DownLite,
            (Strings::Treble, Dir::Up) => Stroke::UpLite,
            (Strings::Bass, Dir::Down) => Stroke::DownBass,
            (Strings::Bass, Dir::Up) => Stroke::UpBass,
            (Strings::Low, _) => Stroke::Bass,
            (Strings::Alt, _) => Stroke::AltBass,
            (Strings::G, _) => Stroke::G,
            (Strings::B, _) => Stroke::B,
            (Strings::E, _) => Stroke::E,
        }
    }

    /// The (direction, strings) pair that names this stroke.
    fn spec(self) -> (Dir, Strings) {
        match self {
            Stroke::Down => (Dir::Down, Strings::All),
            Stroke::Up => (Dir::Up, Strings::All),
            Stroke::DownLite => (Dir::Down, Strings::Treble),
            Stroke::UpLite => (Dir::Up, Strings::Treble),
            Stroke::DownBass => (Dir::Down, Strings::Bass),
            Stroke::UpBass => (Dir::Up, Strings::Bass),
            Stroke::Bass => (Dir::Down, Strings::Low),
            Stroke::AltBass => (Dir::Down, Strings::Alt),
            Stroke::G => (Dir::Down, Strings::G),
            Stroke::B => (Dir::Down, Strings::B),
            Stroke::E => (Dir::Down, Strings::E),
        }
    }
}

/// A muted chop stops this long after each string's onset, seconds.
pub const CHOP_TIME: f64 = 0.06;

/// A note or a damping event on one string, before note ends are resolved.
#[derive(Clone, Copy)]
struct Ev {
    t: f64,
    midi: Option<u8>,
    vel: f32,
    stop: bool,
    /// A muted chop: the note stops `CHOP_TIME` after its onset.
    chop: bool,
}

/// The pattern the rule-based planner plays in bar `bi`, as strokes with
/// beats counted from the start of the bar and velocities after the
/// section's level (`0.72 + 0.1 x intensity`).
pub fn bar_strokes(song: &Song, form: &Form, bi: usize) -> Vec<GuitarStroke> {
    let sub = form.sub() as f64;
    let mi = meter_index(form.meter);
    let bar = &form.bars[bi];
    let sec = &form.sections[bar.sec];
    let intensity = sec.intensity.level();
    let mut pat = Pat::of(song.guitar);
    if pat == Pat::Strum && intensity <= 1 {
        pat = Pat::StrumLite;
    }
    if sec.kind == SectionKind::Bridge && pat == Pat::Strum {
        pat = Pat::Fingerpick;
    }
    let strokes = if bi + 1 == form.bars.len() {
        LAST_BAR
    } else {
        PATTERNS[pat as usize][mi]
    };
    let vel_sec = 0.72 + 0.1 * intensity as f64;
    strokes
        .iter()
        .map(|&(slot, kind, vel)| {
            let (dir, strings) = kind.spec();
            GuitarStroke {
                beat: slot as f64 / sub,
                dir,
                strings,
                damp: false,
                vel: vel as f64 * vel_sec,
            }
        })
        .collect()
}

/// The accompaniment guitar: one note list per string, sorted by onset.
pub fn plan(song: &Song, form: &Form, tl: &Timeline, seed: u64) -> [Vec<StringNote>; 6] {
    plan_with(song, form, tl, seed, &BTreeMap::new())
}

/// `plan`, with the pattern of some bars replaced: `overrides` maps a bar
/// to its strokes (beats from the start of the bar). With no override this
/// is exactly `plan`. The voicing of each stroke is the search result for
/// the chord sounding at its beat.
pub fn plan_with(
    song: &Song,
    form: &Form,
    tl: &Timeline,
    seed: u64,
    overrides: &BTreeMap<usize, Vec<GuitarStroke>>,
) -> [Vec<StringNote>; 6] {
    let bpb = form.bpb() as f64;
    let nbars = form.bars.len();
    let mut memo: HashMap<VoicingKey, Voicing> = HashMap::new();
    let mut voicing_at = |ch: &Chord| *memo.entry(VoicingKey::of(ch)).or_insert_with_key(search);
    let mut ev: [Vec<Ev>; 6] = Default::default();

    for bi in 0..nbars {
        let (strokes, last) = match overrides.get(&bi) {
            // An edited last bar strums like any other bar.
            Some(s) => (s.clone(), false),
            None => (bar_strokes(song, form, bi), bi + 1 == nbars),
        };
        for (j, st) in strokes.iter().enumerate() {
            let mut r = Rng::event(seed, GUITAR_STROKE, ((bi as u64) << 8) | j as u64);
            let beat = bi as f64 * bpb + st.beat;
            let v = voicing_at(tl.chord_at(form, beat + 0.01));
            let t = tl.to_time(beat) + 0.006 * r.bipolar();
            let vv = st.vel * (0.92 + 0.16 * r.uniform());
            stroke_events(&mut ev, &v, st, t, vv, last, &mut r);
        }
    }

    // Chord-change damping.
    for sg in &tl.segs {
        let v = voicing_at(form.chord(sg.chord));
        let t = tl.to_time(sg.b0) - CHANGE_LEAD;
        for (s, e) in ev.iter_mut().enumerate() {
            e.push(Ev {
                t,
                midi: v.notes[s],
                vel: 0.0,
                stop: true,
                chop: false,
            });
        }
    }

    let mut out: [Vec<StringNote>; 6] = Default::default();
    for (s, e) in ev.iter_mut().enumerate() {
        e.sort_by(|a, b| a.t.total_cmp(&b.t));
        for (k, x) in e.iter().enumerate() {
            let (false, Some(midi)) = (x.stop, x.midi) else {
                continue;
            };
            let mut stop = x.t + MAX_RING;
            if x.chop {
                stop = stop.min(x.t + CHOP_TIME);
            }
            for y in &e[k + 1..] {
                if !y.stop {
                    break;
                }
                if y.midi != x.midi {
                    stop = stop.min(y.t + DAMP_TIME);
                    break;
                }
            }
            out[s].push(StringNote {
                t: x.t,
                stop,
                string: s as u8,
                midi,
                vel: x.vel,
            });
        }
    }
    out
}

/// The events of one stroke on `v`, the chord's voicing: a picked note or a
/// strum whose strings start 9 ms (down) or 7 ms (up) apart, 28 ms in the
/// last bar, each spread drawn from `r` (0.8-1.2 of the nominal). `t` is
/// the stroke's onset and `vv` its velocity after the random spread.
fn stroke_events(
    ev: &mut [Vec<Ev>; 6],
    v: &Voicing,
    st: &GuitarStroke,
    t: f64,
    vv: f64,
    last: bool,
    r: &mut Rng,
) {
    let kind = Stroke::of(st.dir, st.strings);
    let chop = st.damp;
    let Some(bass) = v.bass_string() else {
        return;
    };
    // The next sounding string above the bass, at most string 3.
    let alt = (bass + 1..=3.min(bass + 2))
        .find(|&s| v.notes[s].is_some())
        .unwrap_or(bass);
    // A picked string, or the nearest sounding one below it.
    let mut pick = |s: usize| {
        if let Some(ss) = (0..=s).rev().find(|&ss| v.notes[ss].is_some()) {
            ev[ss].push(Ev {
                t,
                midi: v.notes[ss],
                vel: vv as f32,
                stop: false,
                chop,
            });
        }
    };
    match kind {
        Stroke::Bass => pick(bass),
        Stroke::AltBass => pick(alt),
        Stroke::G => pick(3),
        Stroke::B => pick(4),
        Stroke::E => pick(5),
        Stroke::Down
        | Stroke::Up
        | Stroke::DownLite
        | Stroke::UpLite
        | Stroke::DownBass
        | Stroke::UpBass => {
            let down = matches!(kind, Stroke::Down | Stroke::DownLite | Stroke::DownBass);
            let (lowest, top) = match kind {
                Stroke::Down => (0, 6),
                Stroke::Up => (2, 6),
                Stroke::DownBass | Stroke::UpBass => (0, 3),
                _ => (3, 6),
            };
            let mut strs = [0usize; 6];
            let mut n = 0;
            for s in lowest..top {
                if v.notes[s].is_some() {
                    strs[n] = s;
                    n += 1;
                }
            }
            let strs = &mut strs[..n];
            if !down {
                strs.reverse();
            }
            let strs = if down { &strs[..] } else { &strs[..n.min(4)] };
            let spread = if last {
                SPREAD_LAST
            } else if down {
                SPREAD_DOWN
            } else {
                SPREAD_UP
            };
            for (k, &s) in strs.iter().enumerate() {
                let tt = t + k as f64 * spread * (0.8 + 0.4 * r.uniform());
                let accent = if k == 0 && down { 1.05 } else { 1.0 };
                let vs = vv * if down { 1.0 } else { 0.75 } * accent;
                ev[s].push(Ev {
                    t: tt,
                    midi: v.notes[s],
                    vel: vs as f32,
                    stop: false,
                    chop,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use compose::prepare::prepare;
    use serde_json::json;

    // The rule-based planner before the stroke refactor.
    #[allow(clippy::all)]
    mod legacy {
        use super::super::*;

        #[derive(Clone, Copy)]
        struct OldEv {
            t: f64,
            midi: Option<u8>,
            vel: f32,
            stop: bool,
        }

        /// The planner as it was before `plan_with` (verbatim, for the identity test).
        pub fn old_plan(
            song: &Song,
            form: &Form,
            tl: &Timeline,
            seed: u64,
        ) -> [Vec<StringNote>; 6] {
            let bpb = form.bpb() as f64;
            let sub = form.sub() as f64;
            let mi = meter_index(form.meter);
            let nbars = form.bars.len();
            let mut memo: HashMap<VoicingKey, Voicing> = HashMap::new();
            let mut voicing_at =
                |ch: &Chord| *memo.entry(VoicingKey::of(ch)).or_insert_with_key(search);
            let mut ev: [Vec<OldEv>; 6] = Default::default();

            for (bi, bar) in form.bars.iter().enumerate() {
                let sec = &form.sections[bar.sec];
                let intensity = sec.intensity.level();
                let mut pat = Pat::of(song.guitar);
                if pat == Pat::Strum && intensity <= 1 {
                    pat = Pat::StrumLite;
                }
                if sec.kind == SectionKind::Bridge && pat == Pat::Strum {
                    pat = Pat::Fingerpick;
                }
                let last = bi + 1 == nbars;
                let strokes = if last {
                    LAST_BAR
                } else {
                    PATTERNS[pat as usize][mi]
                };
                let vel_sec = 0.72 + 0.1 * intensity as f64;

                for (j, &(slot, kind, vel)) in strokes.iter().enumerate() {
                    let mut r = Rng::event(seed, GUITAR_STROKE, ((bi as u64) << 8) | j as u64);
                    let beat = bi as f64 * bpb + slot as f64 / sub;
                    let v = voicing_at(tl.chord_at(form, beat + 0.01));
                    let t = tl.to_time(beat) + 0.006 * r.bipolar();
                    let vv = vel as f64 * vel_sec * (0.92 + 0.16 * r.uniform());
                    let Some(bass) = v.bass_string() else {
                        continue;
                    };
                    // The next sounding string above the bass, at most string 3.
                    let alt = (bass + 1..=3.min(bass + 2))
                        .find(|&s| v.notes[s].is_some())
                        .unwrap_or(bass);
                    // A picked string, or the nearest sounding one below it.
                    let mut pick = |s: usize| {
                        if let Some(ss) = (0..=s).rev().find(|&ss| v.notes[ss].is_some()) {
                            ev[ss].push(OldEv {
                                t,
                                midi: v.notes[ss],
                                vel: vv as f32,
                                stop: false,
                            });
                        }
                    };
                    match kind {
                        Stroke::Bass => pick(bass),
                        Stroke::AltBass => pick(alt),
                        Stroke::G => pick(3),
                        Stroke::B => pick(4),
                        Stroke::E => pick(5),
                        Stroke::Down
                        | Stroke::Up
                        | Stroke::DownLite
                        | Stroke::UpLite
                        | Stroke::DownBass
                        | Stroke::UpBass => {
                            let down = matches!(kind, Stroke::Down | Stroke::DownLite);
                            let lowest = match kind {
                                Stroke::Down => 0,
                                Stroke::Up => 2,
                                _ => 3,
                            };
                            let mut strs = [0usize; 6];
                            let mut n = 0;
                            for s in lowest..6 {
                                if v.notes[s].is_some() {
                                    strs[n] = s;
                                    n += 1;
                                }
                            }
                            let strs = &mut strs[..n];
                            if !down {
                                strs.reverse();
                            }
                            let strs = if down { &strs[..] } else { &strs[..n.min(4)] };
                            let spread = if last {
                                SPREAD_LAST
                            } else if down {
                                SPREAD_DOWN
                            } else {
                                SPREAD_UP
                            };
                            for (k, &s) in strs.iter().enumerate() {
                                let tt = t + k as f64 * spread * (0.8 + 0.4 * r.uniform());
                                let accent = if k == 0 && down { 1.05 } else { 1.0 };
                                let vs = vv * if down { 1.0 } else { 0.75 } * accent;
                                ev[s].push(OldEv {
                                    t: tt,
                                    midi: v.notes[s],
                                    vel: vs as f32,
                                    stop: false,
                                });
                            }
                        }
                    }
                }
            }

            // Chord-change damping.
            for sg in &tl.segs {
                let v = voicing_at(form.chord(sg.chord));
                let t = tl.to_time(sg.b0) - CHANGE_LEAD;
                for (s, e) in ev.iter_mut().enumerate() {
                    e.push(OldEv {
                        t,
                        midi: v.notes[s],
                        vel: 0.0,
                        stop: true,
                    });
                }
            }

            let mut out: [Vec<StringNote>; 6] = Default::default();
            for (s, e) in ev.iter_mut().enumerate() {
                e.sort_by(|a, b| a.t.total_cmp(&b.t));
                for (k, x) in e.iter().enumerate() {
                    let (false, Some(midi)) = (x.stop, x.midi) else {
                        continue;
                    };
                    let mut stop = x.t + MAX_RING;
                    for y in &e[k + 1..] {
                        if !y.stop {
                            break;
                        }
                        if y.midi != x.midi {
                            stop = stop.min(y.t + DAMP_TIME);
                            break;
                        }
                    }
                    out[s].push(StringNote {
                        t: x.t,
                        stop,
                        string: s as u8,
                        midi,
                        vel: x.vel,
                    });
                }
            }
            out
        }
    }

    fn song_of(meter: &str, guitar: &str) -> Song {
        let v = json!({
            "key": "D", "mode": "major", "meter": meter, "tempo": 108, "guitar": guitar,
            "voice": "alto",
            "sections": [
                {"type": "intro", "chords": ["D", "G/B", "A", "D"]},
                {"type": "verse", "energy": "quiet", "lines": [
                    {"syl": "*walk the *line a-*long the *ridge", "chords": ["D", "Bm7"]},
                    {"syl": "*coun-ting *stones be-*neath the *bridge", "chords": ["G", "A7sus4"]}]},
                {"type": "chorus", "energy": "high", "lines": [
                    {"syl": "*hold *on, *hold *on to the *light", "chords": ["G", "D/F#", "Em7", "A"]}]},
                {"type": "bridge", "lines": [
                    {"syl": "*ev-ery *road *bends *home", "chords": ["Bm", "G", "Em", "A"]}]},
                {"type": "outro", "chords": ["G", "A", "D"]}
            ]
        });
        song::normalize_value(&v).unwrap().0
    }

    #[test]
    fn the_stroke_planner_equals_the_old_planner_bit_for_bit() {
        for meter in ["4/4", "3/4", "6/8"] {
            for guitar in ["strum", "fingerpick", "travis", "arpeggio"] {
                let s = song_of(meter, guitar);
                for seed in [1u64, 1234] {
                    let p = prepare(&s, seed, None);
                    let new = plan(&s, &p.form, &p.timeline, seed);
                    let old = legacy::old_plan(&s, &p.form, &p.timeline, seed);
                    assert!(new.iter().any(|l| !l.is_empty()));
                    assert_eq!(new, old, "{meter} {guitar} seed {seed}");
                }
            }
        }
    }

    #[test]
    fn a_stroke_list_names_every_rule_based_pattern_stroke() {
        let s = song_of("4/4", "strum");
        let p = prepare(&s, 3, None);
        // Re-planning each bar (but the last) with its own strokes as an
        // override changes nothing.
        let ov: BTreeMap<usize, Vec<GuitarStroke>> = (0..p.form.bars.len() - 1)
            .map(|b| (b, bar_strokes(&s, &p.form, b)))
            .collect();
        assert_eq!(
            plan_with(&s, &p.form, &p.timeline, 3, &ov),
            plan(&s, &p.form, &p.timeline, 3)
        );
    }

    #[test]
    fn boom_chuck_and_chops() {
        let s = song_of("4/4", "strum");
        let p = prepare(&s, 3, None);
        let mut ov = BTreeMap::new();
        let st = |beat: f64, dir, strings, damp| GuitarStroke {
            beat,
            dir,
            strings,
            damp,
            vel: 0.8,
        };
        // Bar 4 (the verse's first bar): boom on 0 and 2, chuck on 1 and 3.
        ov.insert(
            4,
            vec![
                st(0.0, Dir::Down, Strings::Bass, false),
                st(1.0, Dir::Down, Strings::Treble, false),
                st(2.0, Dir::Down, Strings::Bass, false),
                st(3.0, Dir::Down, Strings::Treble, true),
            ],
        );
        let g = plan_with(&s, &p.form, &p.timeline, 3, &ov);
        let t = |b: f64| p.timeline.to_time(16.0 + b);
        let near = |n: &StringNote, b: f64| (n.t - t(b)).abs() < 0.06;
        let in_bar: Vec<&StringNote> = g
            .iter()
            .flatten()
            .filter(|n| n.t >= t(0.0) - 0.05 && n.t < t(4.0) - 0.05)
            .collect();
        // Boom: only strings 0-2 sound on beats 0 and 2; chuck: only 3-5 on 1 and 3.
        for n in &in_bar {
            let b = (0..4).find(|&b| near(n, b as f64)).expect("on a beat");
            if b % 2 == 0 {
                assert!(n.string <= 2, "beat {b} string {}", n.string);
            } else {
                assert!(n.string >= 3, "beat {b} string {}", n.string);
            }
            if b == 3 {
                assert!(n.stop - n.t <= CHOP_TIME + 1e-9, "a chop stops at once");
            }
        }
        assert!(in_bar.iter().any(|n| near(n, 3.0)));
        let ringing = in_bar.iter().find(|n| near(n, 0.0)).unwrap();
        assert!(ringing.stop - ringing.t > CHOP_TIME);
        // Bars before the edit are as the rules made them.
        let base = plan(&s, &p.form, &p.timeline, 3);
        let before =
            |v: &[Vec<StringNote>; 6]| v.iter().flatten().filter(|n| n.t < t(0.0) - 0.05).count();
        assert_eq!(before(&g), before(&base));
    }
}
