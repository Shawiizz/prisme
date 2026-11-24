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
