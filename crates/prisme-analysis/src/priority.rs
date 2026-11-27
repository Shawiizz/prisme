//! How much of the computer the analysis takes: the background while a deck plays, so that the
//! audio and the interface come first; all of it otherwise, the DJ waiting on it. The analysis
//! of a track a deck waits for comes before any other: the others wait for it.

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

/// No deck plays: the analysis may take the whole computer.
static FULL_SPEED: AtomicBool = AtomicBool::new(false);
/// Analyses a deck waits for, running now (see [`for_a_deck`]).
static AWAITED: AtomicUsize = AtomicUsize::new(0);
/// How often an analysis that gives way looks whether it may go on.
const GIVE_WAY_POLL: Duration = Duration::from_millis(20);

/// Whether the analysis may take the whole computer (no deck plays), or keeps to the background.
pub fn set_full_speed(full: bool) {
    FULL_SPEED.store(full, Ordering::Relaxed);
}

pub fn full_speed() -> bool {
    FULL_SPEED.load(Ordering::Relaxed)
}

thread_local! {
    /// Whether [`analysis_priority`] last left the thread at full speed.
    static AT_FULL_SPEED: Cell<Option<bool>> = const { Cell::new(None) };
    /// The thread analyses a track a deck waits for (see [`for_a_deck`]).
    static FOR_A_DECK: Cell<bool> = const { Cell::new(false) };
}

/// `work`, the analysis of a track a deck waits for: until it is done, every other analysis
/// stops where it next asks its priority (see [`analysis_priority`]), so that this one has the
/// processor to itself.
pub fn for_a_deck<R>(work: impl FnOnce() -> R) -> R {
    struct Awaited(bool);
    impl Drop for Awaited {
        fn drop(&mut self) {
            FOR_A_DECK.set(self.0);
            AWAITED.fetch_sub(1, Ordering::SeqCst);
        }
    }
    AWAITED.fetch_add(1, Ordering::SeqCst);
    let _awaited = Awaited(FOR_A_DECK.replace(true));
    analysis_priority();
    work()
}

/// Whether the calling thread analyses a track a deck waits for: the threads it hands work to
/// are told so (see [`as_for_a_deck`]).
pub(crate) fn analysing_for_a_deck() -> bool {
    FOR_A_DECK.get()
}

/// The calling thread, which works for another, put as that one: for a deck or not.
pub(crate) fn as_for_a_deck(for_a_deck: bool) {
    FOR_A_DECK.set(for_a_deck);
    analysis_priority();
}

/// Puts the calling thread at the priority the analysis runs at now (see [`set_full_speed`]):
/// asked as often as the work allows, a deck that starts is given way at once. While a deck
/// waits for the analysis of its track, any other analysis waits here. The system is told only
/// when the priority changes.
pub fn analysis_priority() {
    if !FOR_A_DECK.get() {
        while AWAITED.load(Ordering::SeqCst) > 0 {
            std::thread::sleep(GIVE_WAY_POLL);
        }
    }
    let full = full_speed();
    if AT_FULL_SPEED.replace(Some(full)) == Some(full) {
        return;
    }
    if full {
        normal_priority();
    } else {
        background_priority();
    }
}

/// Puts the calling thread in background mode: lowest CPU and disk priority, each system its
/// own way.
pub fn background_priority() {
    #[cfg(windows)]
    // SAFETY: changes the priority of the calling thread only; no pointers involved.
    unsafe {
        use windows_sys::Win32::System::Threading::{
            GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN,
        };
        SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN);
    }
    // Linux: run when nothing else would (0 is the calling thread), read the disk when
    // nobody else does
    #[cfg(target_os = "linux")]
    // SAFETY: changes the scheduling of the calling thread only; the parameter outlives the
    // call.
    unsafe {
        const IOPRIO_WHO_PROCESS: libc::c_int = 1;
        const IOPRIO_CLASS_IDLE: libc::c_int = 3 << 13;
        let param = libc::sched_param { sched_priority: 0 };
        libc::sched_setscheduler(0, libc::SCHED_IDLE, &param);
        libc::syscall(libc::SYS_ioprio_set, IOPRIO_WHO_PROCESS, 0, IOPRIO_CLASS_IDLE);
    }
    // macOS: the class of work nobody waits for, on the efficiency cores and after the others
    // on the disk
    #[cfg(target_os = "macos")]
    // SAFETY: changes the class of the calling thread only.
    unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_BACKGROUND, 0);
    }
}

/// Puts the calling thread back to the priority of ordinary work, out of background mode.
pub fn normal_priority() {
    #[cfg(windows)]
    // SAFETY: changes the priority of the calling thread only; no pointers involved.
    unsafe {
        use windows_sys::Win32::System::Threading::{
            GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_END,
        };
        SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_END);
    }
    #[cfg(target_os = "linux")]
    // SAFETY: changes the scheduling of the calling thread only; the parameter outlives the
    // call.
    unsafe {
        const IOPRIO_WHO_PROCESS: libc::c_int = 1;
        const IOPRIO_CLASS_BEST_EFFORT: libc::c_int = (2 << 13) | 4;
        let param = libc::sched_param { sched_priority: 0 };
        libc::sched_setscheduler(0, libc::SCHED_OTHER, &param);
        libc::syscall(libc::SYS_ioprio_set, IOPRIO_WHO_PROCESS, 0, IOPRIO_CLASS_BEST_EFFORT);
    }
    #[cfg(target_os = "macos")]
    // SAFETY: changes the class of the calling thread only.
    unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_UTILITY, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Instant;

    #[test]
    fn another_analysis_waits_for_the_one_a_deck_waits_for_which_never_waits() {
        let (asked, deck_started) = mpsc::channel();
        let deck = std::thread::spawn(move || {
            for_a_deck(|| {
                asked.send(()).unwrap();
                // asking again on the deck's own thread does not wait
                analysis_priority();
                std::thread::sleep(Duration::from_millis(200));
            });
        });
        deck_started.recv().unwrap();
        let waited = Instant::now();
        analysis_priority();
        assert!(waited.elapsed() >= Duration::from_millis(100), "{:?}", waited.elapsed());
        deck.join().unwrap();
        assert_eq!(AWAITED.load(Ordering::SeqCst), 0);
        assert!(!FOR_A_DECK.get());
    }
}
