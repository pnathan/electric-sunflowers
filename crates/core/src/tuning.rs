//! Tuning switches that engine.js keeps as mutable globals (line 538) and the test
//! harnesses change through setters. Defaults are the shipped values.

#[derive(Clone, Debug)]
pub struct Tuning {
    /// CHOIR_N: singers per choir part.
    pub choir_n: usize,
    /// CHH: `nHigh` passed to choir voices.
    pub choir_nhigh: f64,
    /// CHV: choir vowel per section. Only /aa/ is used.
    pub choir_vowel: &'static str,
    pub gt: GuitarTuning,
    pub vf: VoiceFlags,
    pub bst: f64,
    pub aspg: f64,
    pub bd_v: f64,
    pub bd_t: f64,
    pub vbg: f64,
    pub hfg: f64,
    pub frg: f64,
    pub shg: f64,
    pub tls: f64,
}

/// GT in engine.js.
#[derive(Clone, Debug)]
pub struct GuitarTuning {
    pub damp: f64,
    pub atk: f64,
    pub glide: f64,
    pub symp: f64,
}

/// VF in engine.js. Values are numbers in JS; 0/1 act as switches.
#[derive(Clone, Debug)]
pub struct VoiceFlags {
    pub legacy: f64,
    pub vbar: f64,
    pub trans: f64,
    pub b1x: f64,
    pub nlp: f64,
    pub burst: f64,
    pub asp: f64,
}

impl Default for Tuning {
    fn default() -> Self {
        Tuning {
            choir_n: 3,
            choir_nhigh: 2.0,
            choir_vowel: "aa",
            gt: GuitarTuning { damp: 0.18, atk: 1.0, glide: 5.0, symp: 1.0 },
            vf: VoiceFlags { legacy: 1.0, vbar: 1.0, trans: 1.0, b1x: 1.0, nlp: 1.0, burst: 0.8, asp: 1.0 },
            bst: 1.0,
            aspg: 0.8,
            bd_v: 0.35,
            bd_t: 0.6,
            vbg: 0.5,
            hfg: 1.0,
            frg: 2.2,
            shg: 16.0,
            tls: 1.25,
        }
    }
}
