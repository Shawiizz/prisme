//! The music Android shares between applications, which Prisme reads once the DJ allows it
//! (`MainActivity.kt`). The DJ is asked as Prisme starts, while the music folders are first
//! scanned and listed: they are scanned and listed again with the answer.

use crate::folder_watch::FILES_CHANGED_EVENT;
use crate::library_service::LibraryService;
use jni::objects::JObject;
use jni::JNIEnv;
use std::sync::{Arc, OnceLock};
use tauri::{AppHandle, Emitter, Manager};

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
        let roots: Vec<String> = library.roots().iter().map(|root| root.display().to_string()).collect();
        let _ = app.emit(FILES_CHANGED_EVENT, roots);
    });
}
