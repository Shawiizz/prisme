//! `cover://` serves the cover thumbnails to the UI, so that an `<img>` loads them
//! lazily and the webview caches them (frontend: `convertFileSrc(path, "cover")`).

use crate::library_service::LibraryService;
use std::path::Path;
use std::sync::Arc;
use tauri::http::{header, Request, Response, StatusCode};
use tauri::{Manager, Runtime, UriSchemeContext, UriSchemeResponder};

pub const SCHEME: &str = "cover";

pub fn handle<R: Runtime>(
    ctx: UriSchemeContext<'_, R>,
    request: Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    let Some(library) = ctx.app_handle().try_state::<Arc<LibraryService>>().map(|s| s.inner().clone()) else {
        return responder.respond(empty(StatusCode::SERVICE_UNAVAILABLE));
    };
    let encoded = request.uri().path().trim_start_matches('/');
    let Ok(path) = percent_encoding::percent_decode_str(encoded).decode_utf8() else {
        return responder.respond(empty(StatusCode::BAD_REQUEST));
    };
    let path = path.into_owned();
    tauri::async_runtime::spawn_blocking(move || {
        let response = match library.db.cover(Path::new(&path)) {
            Ok(Some(jpeg)) => Response::builder()
                .header(header::CONTENT_TYPE, "image/jpeg")
                .header(header::CACHE_CONTROL, "max-age=3600")
                .body(jpeg)
                .expect("static headers are valid"),
            _ => empty(StatusCode::NOT_FOUND),
        };
        responder.respond(response);
    });
}

fn empty(status: StatusCode) -> Response<Vec<u8>> {
    Response::builder().status(status).body(Vec::new()).expect("static response is valid")
}
