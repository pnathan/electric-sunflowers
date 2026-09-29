//! Voice-type presets: source, tract and articulation parameters per
//! `song::Voice`. The sung range lives with the voice in `song`.

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
}

/// The preset for `voice`.
pub const fn voice_params(voice: Voice) -> VoiceParams {
    match voice {
        Voice::Baritone => VoiceParams {
            cons_scale: 1.2,
            rd: 1.15,
            fs: 1.0,
            f1s: 1.0,
            breath: 0.10,
            vib_rate: 5.1,
            vib_depth: 0.30,
            tilt: 3800.0,
            jitter: 0.004,
            shimmer: 0.05,
            sf: 0.45,
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
        },
        Voice::Bass => VoiceParams {
            cons_scale: 1.15,
            rd: 1.2,
            fs: 0.94,
            f1s: 0.96,
            breath: 0.1,
            vib_rate: 4.9,
            vib_depth: 0.22,
            tilt: 3200.0,
            jitter: 0.004,
            shimmer: 0.04,
            sf: 0.35,
        },
    }
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
