//! Memory kept in RAM: the system may not write it out to disk to make room for other
//! programs, where the audio thread reading it would wait on the disk. A decoded track is read
//! anywhere at any moment, a needle drop or a scratch away. Where the system refuses (a limit
//! on locked memory), the memory stays as any other, and a warning says so once.

use std::ops::Deref;
use std::sync::Once;

pub struct Resident<T> {
    items: Box<[T]>,
    locked: bool,
}

impl<T> Resident<T> {
    pub fn new(items: Box<[T]>) -> Self {
        let locked = !items.is_empty() && system::lock(region(&items));
        if !items.is_empty() && !locked {
            static WARNED: Once = Once::new();
            WARNED.call_once(|| {
                tracing::warn!(
                    "the system refuses to keep the tracks in RAM: under memory pressure, they can drop out"
                );
            });
        }
        Self { items, locked }
    }
}

impl<T> Drop for Resident<T> {
    fn drop(&mut self) {
        if self.locked {
            system::unlock(region(&self.items));
        }
    }
}

impl<T> Deref for Resident<T> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        &self.items
    }
}

impl<T> FromIterator<T> for Resident<T> {
    fn from_iter<I: IntoIterator<Item = T>>(items: I) -> Self {
        Self::new(items.into_iter().collect())
    }
}

/// The address and length in bytes of `items`.
fn region<T>(items: &[T]) -> (usize, usize) {
    (items.as_ptr() as usize, std::mem::size_of_val(items))
}

#[cfg(windows)]
mod system {
    use std::sync::{Mutex, PoisonError};
    use windows_sys::Win32::System::Memory::{
        SetProcessWorkingSetSizeEx, VirtualLock, VirtualUnlock, QUOTA_LIMITS_HARDWS_MAX_DISABLE,
        QUOTA_LIMITS_HARDWS_MIN_DISABLE,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    const PAGE: usize = 4096;
    /// The working set kept beside the locked memory, for the rest of the process.
    const BESIDE: usize = 256 << 20;

    /// The bytes locked, in whole pages: Windows locks no more than the working set's minimum
    /// holds, which grows and shrinks with them.
    static LOCKED: Mutex<usize> = Mutex::new(0);

    fn pages((address, len): (usize, usize)) -> usize {
        (address + len).next_multiple_of(PAGE) - address / PAGE * PAGE
    }

    fn working_set(locked: usize) -> bool {
        let minimum = BESIDE + locked;
        let flags = QUOTA_LIMITS_HARDWS_MIN_DISABLE | QUOTA_LIMITS_HARDWS_MAX_DISABLE;
        // SAFETY: the pseudo handle of the current process, which needs no closing.
        unsafe { SetProcessWorkingSetSizeEx(GetCurrentProcess(), minimum, minimum + BESIDE, flags) != 0 }
    }

    pub fn lock(region: (usize, usize)) -> bool {
        let mut locked = LOCKED.lock().unwrap_or_else(PoisonError::into_inner);
        let more = pages(region);
        if !working_set(*locked + more) {
            return false;
        }
        // SAFETY: the region is a live allocation of the caller, which unlocks it before
        // freeing it.
        if unsafe { VirtualLock(region.0 as *const _, region.1) } == 0 {
            working_set(*locked);
            return false;
        }
        *locked += more;
        true
    }

    pub fn unlock(region: (usize, usize)) {
        let mut locked = LOCKED.lock().unwrap_or_else(PoisonError::into_inner);
        // SAFETY: as for `lock`. A page shared with another region locked may be unlocked
        // with this one: it is one page, at an edge.
        unsafe { VirtualUnlock(region.0 as *const _, region.1) };
        *locked = locked.saturating_sub(pages(region));
        working_set(*locked);
    }

    #[cfg(test)]
    pub fn locked() -> usize {
        *LOCKED.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(unix)]
mod system {
    pub fn lock((address, len): (usize, usize)) -> bool {
        // SAFETY: the region is a live allocation of the caller, which unlocks it before
        // freeing it.
        unsafe { libc::mlock(address as *const _, len) == 0 }
    }

    pub fn unlock((address, len): (usize, usize)) {
        // SAFETY: as for `lock`.
        unsafe { libc::munlock(address as *const _, len) };
    }
}

#[cfg(not(any(windows, unix)))]
mod system {
    pub fn lock(_: (usize, usize)) -> bool {
        false
    }

    pub fn unlock(_: (usize, usize)) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_are_kept_in_ram_while_they_live() {
        let samples: Resident<f32> = (0..1 << 20).map(|i| i as f32).collect();
        assert!(samples.locked);
        assert_eq!(samples[1000], 1000.0);
        #[cfg(windows)]
        assert!(system::locked() >= 4 << 20);
        drop(samples);
        let none: Resident<f32> = std::iter::empty().collect();
        assert!(!none.locked);
    }
}
