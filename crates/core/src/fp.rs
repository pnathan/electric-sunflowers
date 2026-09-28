//! Floating-point environment: flush-to-zero and denormals-are-zero on the render thread (x86_64 MXCSR 0x8040, aarch64 FPCR.FZ).
//!
//! Recursive filters fed exact zeros decay into subnormal numbers, and an
//! arithmetic operation on a subnormal costs about 100 cycles on x86. With
//! FTZ (results below the normal range become 0) and DAZ (subnormal inputs
//! read as 0) the cost goes away. Only values below 1.2e-38 (f32) or
//! 2.2e-308 (f64) change, which is far below any audible level.
//!
//! The floating-point control register is per thread, so every thread that
//! renders must call `flush_denormals`; `init_pool` does it for the rayon
//! workers.

/// Sets flush-to-zero and denormals-are-zero on the calling thread.
///
/// - x86_64: MXCSR |= 0x8040 (bit 15 FTZ, bit 6 DAZ). Affects SSE/AVX, which
///   is all the scalar and vector float code rustc emits on x86_64.
/// - aarch64: FPCR bit 24 (FZ), which flushes both inputs and outputs.
/// - other targets: no-op.
#[inline]
pub fn flush_denormals() {
    #[cfg(target_arch = "x86_64")]
    #[allow(deprecated)]
    // SAFETY: SSE is always present on x86_64; setting FTZ/DAZ changes no
    // memory and no exception masks.
    unsafe {
        use std::arch::x86_64::{_mm_getcsr, _mm_setcsr};
        _mm_setcsr(_mm_getcsr() | 0x8040);
    }
    #[cfg(target_arch = "aarch64")]
    // SAFETY: reads and writes only the FPCR control register.
    unsafe {
        let mut fpcr: u64;
        std::arch::asm!("mrs {0}, fpcr", out(reg) fpcr, options(nomem, nostack, preserves_flags));
        fpcr |= 1 << 24;
        std::arch::asm!("msr fpcr, {0}", in(reg) fpcr, options(nomem, nostack, preserves_flags));
    }
}

/// Builds the rayon global pool with `flush_denormals` as the start handler
/// of every worker. `threads`: `None` lets rayon choose (RAYON_NUM_THREADS,
/// else the number of logical CPUs); `Some(n)` fixes it (`Some(0)` is the
/// same as `None`). Does nothing if a global pool exists already (the error
/// from rayon is ignored; that pool's workers then keep their own FP mode).
pub fn init_pool(threads: Option<usize>) {
    let mut b = rayon::ThreadPoolBuilder::new().start_handler(|_| flush_denormals());
    if let Some(n) = threads {
        b = b.num_threads(n);
    }
    let _ = b.build_global();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::hint::black_box;

    #[test]
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    fn subnormal_flushes_to_zero() {
        flush_denormals();
        let x = black_box(1e-310_f64) * black_box(1.0);
        assert_eq!(x, 0.0);
        let y = black_box(1e-40_f32) * black_box(1.0);
        assert_eq!(y, 0.0);
    }
}
