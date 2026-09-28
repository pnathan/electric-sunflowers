//! WAV input through `hound`: integer PCM (8-32 bit) and 32-bit float,
//! de-interleaved to one f64 vector per channel, full scale 1.0.

use std::path::Path;

/// A decoded file: channels of equal length, and the sample rate.
pub struct Audio {
    pub channels: Vec<Vec<f64>>,
    pub sample_rate: u32,
}

impl Audio {
    pub fn frames(&self) -> usize {
        self.channels.first().map_or(0, |c| c.len())
    }
}

/// Reads a WAV file. Errors are returned as text; nothing here panics on a
/// malformed file.
pub fn read(path: &Path) -> Result<Audio, String> {
    let mut r = hound::WavReader::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let spec = r.spec();
    let nch = spec.channels as usize;
    if nch == 0 {
        return Err(format!("{}: no channels", path.display()));
    }
    let frames = r.duration() as usize;
    let mut channels = vec![Vec::with_capacity(frames); nch];
    let mut i = 0usize;
    match spec.sample_format {
        hound::SampleFormat::Float => {
            if spec.bits_per_sample != 32 {
                return Err(format!("{}: float WAV with {} bits", path.display(), spec.bits_per_sample));
            }
            for s in r.samples::<f32>() {
                let v = s.map_err(|e| format!("{}: {e}", path.display()))?;
                channels[i % nch].push(v as f64);
                i += 1;
            }
        }
        hound::SampleFormat::Int => {
            let bits = spec.bits_per_sample;
            if bits == 0 || bits > 32 {
                return Err(format!("{}: {bits}-bit PCM", path.display()));
            }
            let scale = 1.0 / (1u64 << (bits - 1)) as f64;
            for s in r.samples::<i32>() {
                let v = s.map_err(|e| format!("{}: {e}", path.display()))?;
                channels[i % nch].push(v as f64 * scale);
                i += 1;
            }
        }
    }
    let n = channels.iter().map(|c| c.len()).min().unwrap_or(0);
    for c in channels.iter_mut() {
        c.truncate(n);
    }
    Ok(Audio { channels, sample_rate: spec.sample_rate })
}
