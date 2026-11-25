//! How much of the computer the analysis takes: the background while a deck plays, so that the
//! audio and the interface come first; all of it otherwise, the DJ waiting on it.

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};

/// No deck plays: the analysis may take the whole computer.
static FULL_SPEED: AtomicBool = AtomicBool::new(false);

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
}

/// Puts the calling thread at the priority the analysis runs at now (see [`set_full_speed`]):
/// asked as often as the work allows, a deck that starts is given way at once. The system is
/// told only when that changes.
pub fn analysis_priority() {
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
