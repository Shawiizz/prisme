//! Windows: the page handles drops itself, and a file dropped there has no path: the page
//! posts it to WebView2 with what it was dropped on, its path is read here and the page is
//! told with `files-dropped`.
//!
//! The message is text: WebView2 stops at the first listener that fails, and Tauri's, which
//! comes first, fails on any other message. Tauri takes it for a call it cannot read and says
//! so in the page's console, then this listener reads it.

use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager};
use tracing::warn;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2File, ICoreWebView2WebMessageReceivedEventArgs, ICoreWebView2WebMessageReceivedEventArgs2,
};
use webview2_com::{take_pwstr, WebMessageReceivedEventHandler};
use windows_core::{Interface, PWSTR};

pub const FILES_DROPPED_EVENT: &str = "files-dropped";

/// What the page posts with the files, as JSON text: where they were dropped.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DropMessage {
    files_dropped: serde_json::Value,
}

/// Files dropped on `target`, as the page named it.
#[derive(Serialize, Clone)]
struct FilesDropped {
    target: serde_json::Value,
    paths: Vec<String>,
}

/// Reads the paths of the files the page posts, for as long as the window lives.
pub fn listen(app: &tauri::App) {
    let Some(window) = app.get_webview_window("main") else { return };
    let handle = app.handle().clone();
    let done = window.with_webview(move |webview| {
        let handler = WebMessageReceivedEventHandler::create(Box::new(move |_, args| {
            if let Some(dropped) = args.as_ref().and_then(dropped) {
                let _ = handle.emit(FILES_DROPPED_EVENT, dropped);
            }
            Ok(())
        }));
        let mut token = 0;
        // SAFETY: the controller of the window's WebView2, alive during this call on the main
        // thread; the handler is its own once added, released with the web view
        let listening = unsafe {
            webview
                .controller()
                .CoreWebView2()
                .and_then(|core| core.add_WebMessageReceived(&handler, &mut token))
        };
        if let Err(e) = listening {
            warn!("files dropped from the file manager are not read: {e}");
        }
    });
    if let Err(e) = done {
        warn!("files dropped from the file manager are not read: {e}");
    }
}

/// The files of a message the page posted on a drop; `None` for any other message.
fn dropped(args: &ICoreWebView2WebMessageReceivedEventArgs) -> Option<FilesDropped> {
    // SAFETY: the arguments of the message being received, alive during its handler; each
    // string WebView2 hands out is taken (and freed) at once
    unsafe {
        let mut json = PWSTR::null();
        args.WebMessageAsJson(&mut json).ok()?;
        let text: String = serde_json::from_str(&take_pwstr(json)).ok()?;
        let message: DropMessage = serde_json::from_str(&text).ok()?;
        let files =
            args.cast::<ICoreWebView2WebMessageReceivedEventArgs2>().ok()?.AdditionalObjects().ok()?;
        let mut count = 0;
        files.Count(&mut count).ok()?;
        let paths = (0..count)
            .filter_map(|i| {
                let file = files.GetValueAtIndex(i).ok()?.cast::<ICoreWebView2File>().ok()?;
                let mut path = PWSTR::null();
                file.Path(&mut path).ok()?;
                Some(take_pwstr(path))
            })
            .collect();
        Some(FilesDropped { target: message.files_dropped, paths })
    }
}
