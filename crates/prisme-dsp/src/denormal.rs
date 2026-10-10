//! Denormal numbers (values so close to zero that they lose precision) make x86
//! floating-point arithmetic up to a hundred times slower, and filters decaying in
//! silence produce them. [`DenormalGuard`] has the processor treat them as zero (flush
//! to zero, denormals are zero) on the current thread while it is alive: inaudible, and
//! the audio callback keeps its speed.

/// MXCSR bits: flush to zero (results) and denormals are zero (inputs).
#[cfg(target_arch = "x86_64")]
const FTZ_DAZ: u32 = 1 << 15 | 1 << 6;

pub struct DenormalGuard {
    #[cfg(target_arch = "x86_64")]
    previous: u32,
}

impl DenormalGuard {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        #[cfg(target_arch = "x86_64")]
        {
            let previous = read_mxcsr();
            write_mxcsr(previous | FTZ_DAZ);
            Self { previous }
        }
        #[cfg(not(target_arch = "x86_64"))]
        Self {}
    }
}

impl Drop for DenormalGuard {
    fn drop(&mut self) {
        #[cfg(target_arch = "x86_64")]
        write_mxcsr(self.previous);
    }
}

#[cfg(target_arch = "x86_64")]
fn read_mxcsr() -> u32 {
    let mut value = 0u32;
    // SAFETY: stores the MXCSR register into a local u32; touches nothing else.
    unsafe { std::arch::asm!("stmxcsr [{}]", in(reg) &mut value, options(nostack, preserves_flags)) };
    value
}

#[cfg(target_arch = "x86_64")]
fn write_mxcsr(value: u32) {
    // SAFETY: loads MXCSR from a valid u32 built from its current value; only the
    // rounding of floating-point results on this thread changes.
    unsafe { std::arch::asm!("ldmxcsr [{}]", in(reg) &value, options(nostack, readonly, preserves_flags)) };
}

#[cfg(all(test, target_arch = "x86_64"))]
mod tests {
    use super::*;
    use std::hint::black_box;

    #[test]
    fn denormals_count_as_zero_inside_the_guard_only() {
        let denormal = black_box(f32::MIN_POSITIVE / 4.0);
        assert!(black_box(denormal * 2.0) > 0.0);
        {
            let _guard = DenormalGuard::new();
            assert_eq!(black_box(denormal * 2.0), 0.0);
        }
        assert!(black_box(denormal * 2.0) > 0.0, "restored");
    }
}
