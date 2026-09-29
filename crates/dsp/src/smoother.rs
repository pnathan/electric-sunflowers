//! Parameter smoothing: `Ramp`, a linear ramp to a target over n samples,
//! for control-rate to audio-rate interpolation (design section 3.4).
//! `next` advances first, so after n calls the value equals the target
//! exactly (no accumulated round-off at the end of a ramp).

#[derive(Clone, Copy, Debug, Default)]
pub struct Ramp {
    pub value: f64,
    pub step: f64,
    pub left: u32,
    target: f64,
}

impl Ramp {
    /// A ramp at rest on `value`.
    pub fn new(value: f64) -> Self {
        Ramp {
            value,
            step: 0.0,
            left: 0,
            target: value,
        }
    }

    /// Move to `target` over `n` samples; n = 0 jumps.
    pub fn set_target(&mut self, target: f64, n: u32) {
        self.target = target;
        if n == 0 {
            self.value = target;
            self.step = 0.0;
            self.left = 0;
        } else {
            self.step = (target - self.value) / n as f64;
            self.left = n;
        }
    }

    /// Advance one sample and return the new value.
    #[inline(always)]
    #[allow(clippy::should_implement_trait)] // a ramp never ends; not an Iterator
    pub fn next(&mut self) -> f64 {
        if self.left > 0 {
            self.left -= 1;
            self.value = if self.left == 0 {
                self.target
            } else {
                self.value + self.step
            };
        }
        self.value
    }

    pub fn target(&self) -> f64 {
        self.target
    }

    pub fn is_ramping(&self) -> bool {
        self.left > 0
    }
}
