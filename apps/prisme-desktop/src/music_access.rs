//! The music Android shares between applications, which Prisme reads once the DJ allows it
//! (`MainActivity.kt`). The DJ is asked as Prisme starts, while the music folders are first
//! scanned and listed: they are scanned and listed again with the answer.

use crate::library_service::LibraryService;
use crate::state::AppState;
use jni::objects::JObject;
use jni::JNIEnv;
use std::sync::{Arc, OnceLock};
use tauri::{AppHandle, Manager};

static APP: OnceLock<AppHandle> = OnceLock::new();

/// Lets the answer reach the library, which is to be managed already.
pub fn install(app: &AppHandle) {
    let _ = APP.set(app.clone());
}

/// `MainActivity.musicAllowed`: the DJ said yes. The scan is started off Android's main
/// thread, as it waits for the one before to stop.
#[no_mangle]
pub extern "system" fn Java_com_shawiizz_prisme_MainActivity_musicAllowed(_env: JNIEnv, _activity: JObject) {
    let Some(app) = APP.get().cloned() else { return };
    tauri::async_runtime::spawn_blocking(move || {
        let library = app.state::<Arc<LibraryService>>();
        library.start();
        crate::actions::browse::files_changed(&app.state::<AppState>(), &library.db, &library.roots());
    });
}
