//! macOS: WebKit gives the page the files dragged from the Finder without their paths. The
//! web view's drag methods are replaced (wry's, which hand every drag to WebKit while the
//! window's drag and drop is off): files dragged from another application are told to the page
//! as the window tells them on Linux, by the `tauri://drag-*` events `onDragDropEvent` follows,
//! and any other drag, the page's own tracks, goes on to WebKit as before.

use std::sync::OnceLock;

use objc2::runtime::{AnyClass, Bool, Imp, ProtocolObject, Sel};
use objc2::sel;
use objc2_app_kit::{NSDragOperation, NSDraggingInfo, NSPasteboardType, NSPasteboardTypeFileURL, NSView};
use objc2_foundation::{NSArray, NSURL};
use serde::Serialize;
use tauri::{Emitter, EventTarget, Manager, WebviewWindow};
use tracing::warn;

const ENTER: &str = "tauri://drag-enter";
const OVER: &str = "tauri://drag-over";
const DROP: &str = "tauri://drag-drop";
const LEAVE: &str = "tauri://drag-leave";

type Drag = ProtocolObject<dyn NSDraggingInfo>;
type Operation = unsafe extern "C-unwind" fn(&NSView, Sel, &Drag) -> NSDragOperation;
type Perform = unsafe extern "C-unwind" fn(&NSView, Sel, &Drag) -> Bool;
type Exited = unsafe extern "C-unwind" fn(&NSView, Sel, Option<&Drag>);

/// The web view's own drag methods, for the drags that are not files from outside.
struct WebKit {
    entered: Operation,
    updated: Operation,
    perform: Perform,
    exited: Exited,
}

static WEBKIT: OnceLock<WebKit> = OnceLock::new();
static WINDOW: OnceLock<WebviewWindow> = OnceLock::new();

/// What Tauri tells of a drag: where it is, in the page's pixels, and the files when known.
#[derive(Serialize, Clone)]
struct DragDrop {
    #[serde(skip_serializing_if = "Option::is_none")]
    paths: Option<Vec<String>>,
    position: Position,
}

#[derive(Serialize, Clone, Copy)]
struct Position {
    x: f64,
    y: f64,
}

/// Reads the files dragged from outside onto the main window's web view, for as long as the
/// application runs.
pub fn listen(app: &tauri::App) {
    let Some(window) = app.get_webview_window("main") else { return };
    let _ = WINDOW.set(window.clone());
    let done = window.with_webview(|webview| {
        // SAFETY: `inner` is the window's WKWebView, alive during this call on the main thread
        let view = unsafe { &*webview.inner().cast::<NSView>() };
        if !take_drags(view.class()) {
            warn!("files dropped from the Finder are not read: the web view takes no drags");
        }
    });
    if let Err(e) = done {
        warn!("files dropped from the Finder are not read: {e}");
    }
}

/// Puts this module's drag methods in the place of the class's, kept for the other drags;
/// `false` if the class lacks one.
fn take_drags(class: &AnyClass) -> bool {
    let method = |name| class.instance_method(name);
    let (Some(entered), Some(updated), Some(perform), Some(exited)) = (
        method(sel!(draggingEntered:)),
        method(sel!(draggingUpdated:)),
        method(sel!(performDragOperation:)),
        method(sel!(draggingExited:)),
    ) else {
        return false;
    };
    // SAFETY: these are NSDraggingDestination's methods, with the signatures given here, and
    // so have the functions put in their place; they are kept before they are replaced, once
    unsafe {
        let webkit = WebKit {
            entered: std::mem::transmute::<Imp, Operation>(entered.implementation()),
            updated: std::mem::transmute::<Imp, Operation>(updated.implementation()),
            perform: std::mem::transmute::<Imp, Perform>(perform.implementation()),
            exited: std::mem::transmute::<Imp, Exited>(exited.implementation()),
        };
        if WEBKIT.set(webkit).is_ok() {
            entered.set_implementation(std::mem::transmute::<Operation, Imp>(dragging_entered));
            updated.set_implementation(std::mem::transmute::<Operation, Imp>(dragging_updated));
            perform.set_implementation(std::mem::transmute::<Perform, Imp>(perform_drag_operation));
            exited.set_implementation(std::mem::transmute::<Exited, Imp>(dragging_exited));
        }
    }
    true
}

extern "C-unwind" fn dragging_entered(view: &NSView, sel: Sel, drag: &Drag) -> NSDragOperation {
    let Some(webkit) = WEBKIT.get() else { return NSDragOperation::None };
    if !files_from_outside(drag) {
        // SAFETY: the web view's own method, called as AppKit called this one
        return unsafe { (webkit.entered)(view, sel, drag) };
    }
    tell(ENTER, DragDrop { paths: Some(paths(drag)), position: position(view, drag) });
    NSDragOperation::Copy
}

extern "C-unwind" fn dragging_updated(view: &NSView, sel: Sel, drag: &Drag) -> NSDragOperation {
    let Some(webkit) = WEBKIT.get() else { return NSDragOperation::None };
    if !files_from_outside(drag) {
        // SAFETY: the web view's own method, called as AppKit called this one
        return unsafe { (webkit.updated)(view, sel, drag) };
    }
    tell(OVER, DragDrop { paths: None, position: position(view, drag) });
    NSDragOperation::Copy
}

extern "C-unwind" fn perform_drag_operation(view: &NSView, sel: Sel, drag: &Drag) -> Bool {
    let Some(webkit) = WEBKIT.get() else { return Bool::NO };
    if !files_from_outside(drag) {
        // SAFETY: the web view's own method, called as AppKit called this one
        return unsafe { (webkit.perform)(view, sel, drag) };
    }
    tell(DROP, DragDrop { paths: Some(paths(drag)), position: position(view, drag) });
    Bool::YES
}

extern "C-unwind" fn dragging_exited(view: &NSView, sel: Sel, drag: Option<&Drag>) {
    let Some(webkit) = WEBKIT.get() else { return };
    match drag {
        Some(drag) if files_from_outside(drag) => tell(LEAVE, ()),
        // SAFETY: the web view's own method, called as AppKit called this one
        _ => unsafe { (webkit.exited)(view, sel, drag) },
    }
}

fn file_url() -> &'static NSPasteboardType {
    // SAFETY: a constant of AppKit's
    unsafe { NSPasteboardTypeFileURL }
}

/// Files dragged from another application, the Finder: the page's own drags have a source.
fn files_from_outside(drag: &Drag) -> bool {
    drag.draggingSource().is_none()
        && drag.draggingPasteboard().availableTypeFromArray(&NSArray::from_slice(&[file_url()])).is_some()
}

/// The paths of the files and folders dragged, the Finder's file references resolved.
fn paths(drag: &Drag) -> Vec<String> {
    let Some(items) = drag.draggingPasteboard().pasteboardItems() else { return Vec::new() };
    items
        .iter()
        .filter_map(|item| {
            let url = item.stringForType(file_url())?;
            let url = NSURL::URLWithString(&url)?;
            Some(url.filePathURL()?.path()?.to_string())
        })
        .collect()
}

/// Where the drag is in the web view, from its top left corner.
fn position(view: &NSView, drag: &Drag) -> Position {
    let point = view.convertPoint_fromView(drag.draggingLocation(), None);
    let y = if view.isFlipped() { point.y } else { view.bounds().size.height - point.y };
    Position { x: point.x, y }
}

fn tell(event: &str, payload: impl Serialize + Clone) {
    if let Some(window) = WINDOW.get() {
        let _ = window.emit_to(EventTarget::labeled(window.label()), event, payload);
    }
}
