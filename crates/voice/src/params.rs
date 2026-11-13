//! Voice-type presets: source, tract and articulation parameters per
//! `song::Voice`. The sung range lives with the voice in `song`.

use std::sync::RwLock;

use song::Voice;

/// Synthesis parameters of one voice type.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VoiceParams {
    /// Consonant duration scale.
    pub cons_scale: f64,
    /// Liljencrants-Fant shape parameter Rd (Fant 1995): higher is laxer.
    pub rd: f64,
    /// Formant frequency scale for F2 and above (vocal tract length).
    pub fs: f64,
    /// Formant frequency scale for F1.
    pub f1s: f64,
    /// Breath noise level.
    pub breath: f64,
    /// Vibrato rate in Hz.
    pub vib_rate: f64,
    /// Vibrato depth in semitones.
    pub vib_depth: f64,
    /// Spectral tilt corner frequency in Hz.
    pub tilt: f64,
    /// Period jitter (relative standard deviation).
    pub jitter: f64,
    /// Amplitude shimmer (relative standard deviation).
    pub shimmer: f64,
    /// Singer's-formant cluster level.
    pub sf: f64,
    /// Return-phase corner of the glottal source at mid loudness, Hz (see
    /// `glottal`); 0 is the legacy source, whose return phase follows Rd.
    pub fa_hz: f64,
    /// Gain of the high shelf at 5.2 kHz after the cascade, dB.
    pub shelf_db: f64,
    /// Gain of the voiced high-frequency bed (flow-shaped noise high-passed
    /// at `hf_hz`); 0 is off.
    pub hf_bed: f64,
    /// Corner of the HF bed's high-pass, Hz (legacy 3800).
    pub hf_hz: f64,
    /// Gain on the frication path (bursts included) and on aspiration; 1 is
    /// the legacy level. Breath noise and the HF bed are not scaled.
    pub cons_gain: f64,
}

/// The preset for `voice`.
pub const fn voice_params(voice: Voice) -> VoiceParams {
    match voice {
        // Baritone: fitted to the harmonic envelope of male-range vocadito
        // singers (tools/voice_target.py; engine-design 5.1). Source with
        // capped return phase (`fa_hz`), a voiced HF bed and a consonant
        // gain that restores the consonants' prominence over the bed.
        Voice::Baritone => VoiceParams {
            cons_scale: 1.2,
            rd: 1.9,
            fs: 1.0,
            f1s: 1.0,
            breath: 0.10,
            vib_rate: 5.1,
            vib_depth: 0.20,
            tilt: 6000.0,
            jitter: 0.004,
            shimmer: 0.05,
            sf: 0.15,
            fa_hz: 1400.0,
            shelf_db: 16.0,
            hf_bed: 0.2,
            hf_hz: 3800.0,
            cons_gain: 2.74,
        },
        Voice::Tenor => VoiceParams {
            cons_scale: 1.25,
            rd: 1.0,
            fs: 1.04,
            f1s: 1.02,
            breath: 0.07,
            vib_rate: 5.6,
            vib_depth: 0.36,
            tilt: 5000.0,
            jitter: 0.003,
            shimmer: 0.035,
            sf: 0.6,
            fa_hz: 0.0,
            shelf_db: 16.0,
            hf_bed: 0.0,
            hf_hz: 3800.0,
            cons_gain: 1.0,
        },
        Voice::Alto => VoiceParams {
            cons_scale: 1.35,
            rd: 1.25,
            fs: 1.16,
            f1s: 1.08,
            breath: 0.16,
            vib_rate: 5.3,
            vib_depth: 0.32,
            tilt: 4300.0,
            jitter: 0.003,
            shimmer: 0.04,
            sf: 0.2,
            fa_hz: 0.0,
            shelf_db: 16.0,
            hf_bed: 0.0,
            hf_hz: 3800.0,
            cons_gain: 1.0,
        },
        Voice::Soprano => VoiceParams {
            cons_scale: 1.4,
            rd: 1.05,
            fs: 1.22,
            f1s: 1.12,
            breath: 0.09,
            vib_rate: 5.8,
            vib_depth: 0.42,
            tilt: 5600.0,
            jitter: 0.0025,
            shimmer: 0.03,
            sf: 0.25,
            fa_hz: 0.0,
            shelf_db: 16.0,
            hf_bed: 0.0,
            hf_hz: 3800.0,
            cons_gain: 1.0,
        },
        // Bass: the baritone's fitted `fa_hz`, `shelf_db`, `hf_bed`, `hf_hz`
        // and `cons_gain`; `rd` picked for the same open-vowel H1-H2; its own
        // `sf` and `tilt` (not fitted).
        Voice::Bass => VoiceParams {
            cons_scale: 1.15,
            rd: 1.9,
            fs: 0.94,
            f1s: 0.96,
            breath: 0.1,
            vib_rate: 4.9,
            vib_depth: 0.16,
            tilt: 3200.0,
            jitter: 0.004,
            shimmer: 0.04,
            sf: 0.35,
            fa_hz: 1400.0,
            shelf_db: 16.0,
            hf_bed: 0.2,
            hf_hz: 3800.0,
            cons_gain: 2.74,
        },
    }
}

/// Preset replacement used only by the fit tools (`set_fit_override`).
static FIT: RwLock<Option<(Voice, VoiceParams)>> = RwLock::new(None);

/// Make `render_phrases` use `p` in place of the preset of `voice`. For the
/// fit tools (`examples/vow.rs`, `engine/examples/stems.rs`); production
/// code never calls it and nothing reads the environment.
#[doc(hidden)]
pub fn set_fit_override(voice: Voice, p: VoiceParams) {
    if let Ok(mut g) = FIT.write() {
        *g = Some((voice, p));
    }
}

/// `voice_params`, or the fit override when one is set for `voice`.
pub(crate) fn preset(voice: Voice) -> VoiceParams {
    match FIT.read().ok().and_then(|g| *g) {
        Some((v, p)) if v == voice => p,
        _ => voice_params(voice),
    }
}

/// Parse a fit spec "key=value,..." (keys: voice, fa_hz, shelf_db, sf, tilt,
/// rd, hf_bed, hf_hz, cons_gain) into a voice and its preset with those values replaced;
/// the voice defaults to baritone. For the fit tools.
#[doc(hidden)]
pub fn parse_fit(spec: &str) -> Result<(Voice, VoiceParams), String> {
    let mut voice = Voice::Baritone;
    let kv: Vec<(&str, &str)> = spec
        .split(',')
        .filter(|t| !t.trim().is_empty())
        .map(|t| {
            t.split_once('=')
                .map_or((t.trim(), ""), |(k, v)| (k.trim(), v.trim()))
        })
        .collect();
    for (k, v) in &kv {
        if *k == "voice" {
            voice = v.parse().map_err(|_| format!("bad voice {v}"))?;
        }
    }
    let mut p = voice_params(voice);
    for (k, v) in kv {
        let num = || v.parse::<f64>().map_err(|_| format!("bad value {k}={v}"));
        match k {
            "voice" => {}
            "fa_hz" => p.fa_hz = num()?,
            "shelf_db" => p.shelf_db = num()?,
            "sf" => p.sf = num()?,
            "tilt" => p.tilt = num()?,
            "rd" => p.rd = num()?,
            "hf_bed" => p.hf_bed = num()?,
            "hf_hz" => p.hf_hz = num()?,
            "cons_gain" => p.cons_gain = num()?,
            _ => return Err(format!("unknown fit key {k}")),
        }
    }
    Ok((voice, p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consonant_scale_rises_with_the_voice() {
        let order = [
            Voice::Bass,
            Voice::Baritone,
            Voice::Tenor,
            Voice::Alto,
            Voice::Soprano,
        ];
        let cs: Vec<f64> = order.iter().map(|&v| voice_params(v).cons_scale).collect();
        assert_eq!(cs, vec![1.15, 1.2, 1.25, 1.35, 1.4]);
    }
}
