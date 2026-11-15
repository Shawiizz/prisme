//! Android's performance hints for the audio thread (Android 13 and later): told how long each
//! period took against its budget, the system keeps the processor fast enough for it, rather
//! than letting it slow down between periods and come back late. Elsewhere, nothing.

use std::time::Duration;

/// The hints of the thread that made it.
pub(crate) struct PerformanceHint {
    #[cfg(target_os = "android")]
    session: android::Session,
}

/// The system has hints, but gave the thread none.
pub(crate) struct Refused;

impl PerformanceHint {
    /// Looks the system's functions up, away from the audio thread that makes its session.
    pub(crate) fn prepare() {
        #[cfg(target_os = "android")]
        android::api();
    }

    /// A session for the calling thread, whose periods last `budget`; `None` where the system
    /// has none. Nothing is logged: the thread is the audio's.
    pub(crate) fn for_this_thread(budget: Duration) -> Result<Option<Self>, Refused> {
        #[cfg(target_os = "android")]
        return android::Session::new(budget).map(|session| session.map(|session| Self { session }));
        #[cfg(not(target_os = "android"))]
        {
            let _ = budget;
            Ok(None)
        }
    }

    /// How long the period just done took.
    pub(crate) fn report(&mut self, took: Duration) {
        #[cfg(target_os = "android")]
        self.session.report(took);
        #[cfg(not(target_os = "android"))]
        let _ = took;
    }
}

#[cfg(target_os = "android")]
mod android {
    use super::Refused;
    use std::ffi::{c_int, c_void, CStr};
    use std::sync::OnceLock;
    use std::time::Duration;

    type GetManager = unsafe extern "C" fn() -> *mut c_void;
    type CreateSession = unsafe extern "C" fn(*mut c_void, *const i32, usize, i64) -> *mut c_void;
    type Report = unsafe extern "C" fn(*mut c_void, i64) -> c_int;
    type Close = unsafe extern "C" fn(*mut c_void);

    pub(super) struct Api {
        get_manager: GetManager,
        create_session: CreateSession,
        report: Report,
        close: Close,
    }

    /// The functions of `libandroid`, looked up rather than linked: they came with Android 13,
    /// and Prisme runs from Android 10.
    pub(super) fn api() -> Option<&'static Api> {
        static API: OnceLock<Option<Api>> = OnceLock::new();
        API.get_or_init(|| {
            // SAFETY: a library of the system, opened by name; the symbols, when there, have
            // the signatures of <android/performance_hint.h>.
            unsafe {
                let library = libc::dlopen(c"libandroid.so".as_ptr(), libc::RTLD_NOW);
                if library.is_null() {
                    return None;
                }
                let symbol = |name: &CStr| {
                    let s = libc::dlsym(library, name.as_ptr());
                    (!s.is_null()).then_some(s)
                };
                Some(Api {
                    get_manager: std::mem::transmute::<*mut c_void, GetManager>(symbol(
                        c"APerformanceHint_getManager",
                    )?),
                    create_session: std::mem::transmute::<*mut c_void, CreateSession>(symbol(
                        c"APerformanceHint_createSession",
                    )?),
                    report: std::mem::transmute::<*mut c_void, Report>(symbol(
                        c"APerformanceHint_reportActualWorkDuration",
                    )?),
                    close: std::mem::transmute::<*mut c_void, Close>(symbol(
                        c"APerformanceHint_closeSession",
                    )?),
                })
            }
        })
        .as_ref()
    }

    pub(super) struct Session {
        api: &'static Api,
        session: *mut c_void,
    }

    // SAFETY: a session may be reported to and closed from any thread.
    unsafe impl Send for Session {}

    impl Session {
        pub(super) fn new(budget: Duration) -> Result<Option<Self>, Refused> {
            let Some(api) = api() else { return Ok(None) };
            // SAFETY: the calls of the API, with the calling thread's id.
            unsafe {
                let manager = (api.get_manager)();
                if manager.is_null() {
                    return Ok(None);
                }
                let thread = libc::gettid();
                let session = (api.create_session)(manager, &thread, 1, budget.as_nanos() as i64);
                if session.is_null() {
                    return Err(Refused);
                }
                Ok(Some(Self { api, session }))
            }
        }

        pub(super) fn report(&mut self, took: Duration) {
            // SAFETY: a session open until dropped; a positive duration.
            unsafe { (self.api.report)(self.session, took.as_nanos().max(1) as i64) };
        }
    }

    impl Drop for Session {
        fn drop(&mut self) {
            // SAFETY: the session, closed once.
            unsafe { (self.api.close)(self.session) };
        }
    }
}
