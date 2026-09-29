//! Stochastic control processes: a leaky second-order random walk and the
//! Ornstein-Uhlenbeck process. `step(&mut Rng)` draws from
//! `sfcore::random::Rng::gauss`; `tick(gauss)` takes any source of standard
//! normal deviates (tests, or a caller with its own stream).
//! They run at a model's control rate, not per audio sample.

use sfcore::random::Rng;

/// Where a `RandomWalk` applies its limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WalkClamp {
    /// Clamp the position state itself; the walk sticks to the wall until
    /// its velocity turns.
    Position,
    /// Leave the state free and clamp only the value returned.
    Output,
}

/// Leaky second-order random walk (integrated, leaky Gaussian velocity):
///
/// ```text
/// v <- (v + step * N(0,1)) * leak
/// w <- (w + gain * v) * w_leak
/// out = clamp(w, -limit, limit)       (state clamped too with WalkClamp::Position)
/// ```
///
/// `step` and `leak` set the velocity's size and correlation time
/// (about 1 / (1 - leak) ticks); `gain` scales velocity into position;
/// `w_leak` < 1 pulls the position back to 0; `limit` bounds the excursion.
/// Uses in the engine (tick rate in brackets):
/// - Formant wobble, voice tract [HOP frames, 689 Hz]: step 0.02, leak 0.97,
///   gain 0.01, w_leak 1, limit 0.02 (F2) and 0.025 (F3), clamp Position.
/// - Pitch wander, violin [control rate SR/16]: step 0.0004, leak 0.97,
///   gain 0.05, w_leak 1, limit 0.0015 (natural-log frequency), clamp Position.
/// - Pitch drift, voice controls [HOP frames]: step 0.004, leak 0.985,
///   gain 1, w_leak 0.998, limit 0.12 semitone, clamp Output.
#[derive(Clone, Debug)]
pub struct RandomWalk {
    pub v: f64,
    pub w: f64,
    pub step: f64,
    pub leak: f64,
    pub gain: f64,
    pub w_leak: f64,
    pub limit: f64,
    pub clamp: WalkClamp,
}

impl RandomWalk {
    /// Walk with a clamped position and no position leak (formant wobble,
    /// violin wander).
    pub fn bounded(step: f64, leak: f64, gain: f64, limit: f64) -> Self {
        RandomWalk {
            v: 0.0,
            w: 0.0,
            step,
            leak,
            gain,
            w_leak: 1.0,
            limit: limit.abs(),
            clamp: WalkClamp::Position,
        }
    }

    /// Walk with a leaky position and a clamp on the output only (voice
    /// pitch drift). `gain` is 1.
    pub fn leaky(step: f64, leak: f64, w_leak: f64, limit: f64) -> Self {
        RandomWalk {
            v: 0.0,
            w: 0.0,
            step,
            leak,
            gain: 1.0,
            w_leak,
            limit: limit.abs(),
            clamp: WalkClamp::Output,
        }
    }

    /// Advance one control tick with one normal deviate from `gauss`; returns
    /// the clamped position.
    #[inline]
    pub fn tick(&mut self, mut gauss: impl FnMut() -> f64) -> f64 {
        self.v = (self.v + self.step * gauss()) * self.leak;
        let w = (self.w + self.gain * self.v) * self.w_leak;
        let out = w.clamp(-self.limit, self.limit);
        self.w = match self.clamp {
            WalkClamp::Position => out,
            WalkClamp::Output => w,
        };
        out
    }

    /// `tick` with one deviate from `rng`.
    #[inline]
    pub fn step(&mut self, rng: &mut Rng) -> f64 {
        self.tick(|| rng.gauss())
    }

    pub fn reset(&mut self) {
        self.v = 0.0;
        self.w = 0.0;
    }
}

/// Ornstein-Uhlenbeck process `dx = theta (mu - x) dt + sigma dW`
/// (Uhlenbeck and Ornstein 1930), advanced by its exact discretisation
/// (Gillespie 1996, "Exact numerical simulation of the Ornstein-Uhlenbeck
/// process and its integral"):
///
/// ```text
/// x <- mu + (x - mu) a + b N(0,1),   a = exp(-theta dt),
///                                    b = sigma sqrt((1 - a^2) / (2 theta))
/// ```
///
/// Stationary law N(mu, sigma^2 / (2 theta)), correlation time 1 / theta,
/// exact for any `dt`.
///
/// The violin's bow speed, pressure, position and vibrato-rate noises are
/// unit-variance OU processes with time constants 0.18, 0.12, 0.35 and 0.25 s
/// at dt = 16 / SR (`unit(tau, dt)`). The earlier closure used Euler-Maruyama,
/// `x <- x - x dt/tau + sqrt(2 dt/tau) N`: its stationary variance is
/// `1 / (1 - dt / (2 tau))`, 0.15% high at tau = 0.12 s, and its step
/// correlation `1 - dt/tau` differs from `exp(-dt/tau)` by under 1e-5.
/// Neither difference is audible; the exact form stays correct if the
/// control rate changes.
#[derive(Clone, Debug)]
pub struct OuProcess {
    pub x: f64,
    pub mu: f64,
    pub theta: f64,
    pub sigma: f64,
    a: f64,
    b: f64,
}

impl OuProcess {
    /// Mean `mu`, reversion rate `theta` (1/s, > 0), volatility `sigma`,
    /// tick interval `dt` (s). Starts at `mu`. A non-positive or non-finite
    /// `theta` or `dt` gives a process frozen at `mu`.
    pub fn new(mu: f64, theta: f64, sigma: f64, dt: f64) -> Self {
        let mut p = OuProcess {
            x: mu,
            mu,
            theta,
            sigma,
            a: 1.0,
            b: 0.0,
        };
        p.set_dt(dt);
        p
    }

    /// Unit stationary variance, zero mean, time constant `tau` seconds:
    /// theta = 1 / tau, sigma = sqrt(2 / tau).
    pub fn unit(tau: f64, dt: f64) -> Self {
        OuProcess::new(0.0, 1.0 / tau, (2.0 / tau).sqrt(), dt)
    }

    /// Recompute the step coefficients for tick interval `dt`.
    pub fn set_dt(&mut self, dt: f64) {
        if self.theta > 0.0 && self.theta.is_finite() && dt > 0.0 && dt.is_finite() {
            self.a = (-self.theta * dt).exp();
            self.b = self.sigma * ((1.0 - self.a * self.a) / (2.0 * self.theta)).sqrt();
        } else {
            self.a = 1.0;
            self.b = 0.0;
            self.x = self.mu;
        }
    }

    /// Stationary variance sigma^2 / (2 theta).
    pub fn stationary_variance(&self) -> f64 {
        self.sigma * self.sigma / (2.0 * self.theta)
    }

    /// Advance one tick with one normal deviate from `gauss`; returns x.
    #[inline]
    pub fn tick(&mut self, mut gauss: impl FnMut() -> f64) -> f64 {
        self.x = self.mu + (self.x - self.mu) * self.a + self.b * gauss();
        self.x
    }

    /// `tick` with one deviate from `rng`.
    #[inline]
    pub fn step(&mut self, rng: &mut Rng) -> f64 {
        self.tick(|| rng.gauss())
    }
}
