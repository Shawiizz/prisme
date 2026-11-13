//! Denormal numbers (values so close to zero that they lose precision) make x86
//! floating-point arithmetic up to a hundred times slower, and cost ARM processors too;
//! filters decaying in silence produce them. [`DenormalGuard`] has the processor treat
//! them as zero on the current thread while it is alive: inaudible, and the audio
//! callback keeps its speed.

pub struct DenormalGuard {
    previous: u64,
}

impl DenormalGuard {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let previous = register::read();
        register::write(previous | register::DENORMALS_ARE_ZERO);
        Self { previous }
    }
}

impl Drop for DenormalGuard {
    fn drop(&mut self) {
        register::write(self.previous);
    }
}

/// The register that holds how the floating-point arithmetic of the thread behaves.
#[cfg(target_arch = "x86_64")]
mod register {
    /// MXCSR bits: flush to zero (results) and denormals are zero (inputs).
    pub const DENORMALS_ARE_ZERO: u64 = 1 << 15 | 1 << 6;

    pub fn read() -> u64 {
        let mut value = 0u32;
        // SAFETY: stores the MXCSR register into a local u32; touches nothing else.
        unsafe { std::arch::asm!("stmxcsr [{}]", in(reg) &mut value, options(nostack, preserves_flags)) };
        value.into()
    }

    pub fn write(value: u64) {
        let value = value as u32;
        // SAFETY: loads MXCSR from a valid u32 built from its current value; only the
        // rounding of floating-point results on this thread changes.
        unsafe {
            std::arch::asm!("ldmxcsr [{}]", in(reg) &value, options(nostack, readonly, preserves_flags))
        };
    }
}

#[cfg(target_arch = "aarch64")]
mod register {
    /// FPCR bit: flush to zero (inputs and results).
    pub const DENORMALS_ARE_ZERO: u64 = 1 << 24;

    pub fn read() -> u64 {
        let value: u64;
        // SAFETY: copies the FPCR register into a local; touches nothing else.
        unsafe { std::arch::asm!("mrs {}, fpcr", out(reg) value, options(nomem, nostack, preserves_flags)) };
        value
    }

    pub fn write(value: u64) {
        // SAFETY: sets FPCR to its current value with one more bit; only the rounding of
        // floating-point results on this thread changes.
        unsafe { std::arch::asm!("msr fpcr, {}", in(reg) value, options(nomem, nostack, preserves_flags)) };
    }
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
mod register {
    pub const DENORMALS_ARE_ZERO: u64 = 0;

    pub fn read() -> u64 {
        0
    }

    pub fn write(_: u64) {}
}

#[cfg(all(test, any(target_arch = "x86_64", target_arch = "aarch64")))]
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
