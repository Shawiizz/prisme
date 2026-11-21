//! Files the DJ picks in the system's dialogs. A computer gives their path; Android gives the
//! `content://` URI of the application holding the file, which the system opens for Prisme on
//! the calling thread (never the window's: the application holding the file may be slow).

use percent_encoding::percent_decode_str;
use std::fs::File;
use std::io;
use std::path::Path;
use tauri::Url;
use tauri_plugin_dialog::FilePath;

/// The file picked to save into, emptied.
pub fn create(file: FilePath) -> io::Result<File> {
    opened(file, Access::Write)
}

/// The file picked to read.
pub fn open(file: FilePath) -> io::Result<File> {
    opened(file, Access::Read)
}

#[derive(Clone, Copy)]
enum Access {
    Read,
    /// The file emptied first.
    Write,
}

impl Access {
    fn open(self, path: &Path) -> io::Result<File> {
        match self {
            Access::Read => File::open(path),
            Access::Write => File::create(path),
        }
    }
}

fn opened(file: FilePath, access: Access) -> io::Result<File> {
    match file {
        FilePath::Path(path) => access.open(&path),
        FilePath::Url(uri) => open_uri(&uri, access),
    }
}

/// The file of `uri`, opened by the content provider holding it.
#[cfg(target_os = "android")]
fn open_uri(uri: &Url, access: Access) -> io::Result<File> {
    use jni::objects::JValue;
    use std::os::fd::FromRawFd;
    let mode = match access {
        Access::Read => "r",
        Access::Write => "wt",
    };
    let fd = crate::android_jni::call("open a file picked", |env, context| {
        let resolver = env
            .call_method(context, "getContentResolver", "()Landroid/content/ContentResolver;", &[])?
            .l()?;
        let uri = android_uri(env, uri)?;
        let mode = env.new_string(mode)?;
        let descriptor = env
            .call_method(
                &resolver,
                "openFileDescriptor",
                "(Landroid/net/Uri;Ljava/lang/String;)Landroid/os/ParcelFileDescriptor;",
                &[JValue::Object(&uri), JValue::Object(&mode)],
            )?
            .l()?;
        if descriptor.is_null() {
            return Ok(None);
        }
        Ok(Some(env.call_method(&descriptor, "detachFd", "()I", &[])?.i()?))
    })
    .flatten()
    .ok_or_else(|| io::Error::other(format!("{uri} could not be opened")))?;
    // SAFETY: a descriptor detached from its `ParcelFileDescriptor` is Prisme's alone to close.
    Ok(unsafe { File::from_raw_fd(fd) })
}

/// The file of a `file://` URI; no other is reached on a computer.
#[cfg(not(target_os = "android"))]
fn open_uri(uri: &Url, access: Access) -> io::Result<File> {
    let path = uri
        .to_file_path()
        .map_err(|()| io::Error::new(io::ErrorKind::Unsupported, format!("{uri} is not a file")))?;
    access.open(&path)
}

/// What the DJ knows the file picked by: its path, else its name.
pub fn name(file: &FilePath) -> String {
    match file {
        FilePath::Path(path) => path.display().to_string(),
        FilePath::Url(uri) => file_name(uri),
    }
}

/// The name of the file of `uri`, as the application holding it gives it, else as the end of
/// the URI says it.
pub fn file_name(uri: &Url) -> String {
    name_of(given_name(uri).as_deref(), uri)
}

/// `given`, else the end of `uri` (`primary:Music/Kick.wav`: `Kick.wav`); a file name either
/// way, never a path.
fn name_of(given: Option<&str>, uri: &Url) -> String {
    let end = uri
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .map(|end| percent_decode_str(end).decode_utf8_lossy());
    let name = match (given, &end) {
        (Some(given), _) => given.rsplit(['/', '\\']).next(),
        (None, Some(end)) => end.rsplit(['/', ':']).next(),
        (None, None) => None,
    };
    name.filter(|n| !n.is_empty() && *n != "." && *n != "..").unwrap_or("file").to_owned()
}

/// The display name the content provider of `uri` gives its file.
#[cfg(target_os = "android")]
fn given_name(uri: &Url) -> Option<String> {
    use jni::objects::{JObject, JValue};
    crate::android_jni::call("read the name of a file picked", |env, context| {
        let resolver =
            env.call_method(context, "getContentResolver", "()Landroid/content/ContentResolver;", &[])?.l()?;
        let uri = android_uri(env, uri)?;
        let column = env.new_string("_display_name")?;
        let columns = env.new_object_array(1, "java/lang/String", &column)?;
        let none = JObject::null();
        let cursor = env
            .call_method(
                &resolver,
                "query",
                "(Landroid/net/Uri;[Ljava/lang/String;Ljava/lang/String;[Ljava/lang/String;Ljava/lang/String;)Landroid/database/Cursor;",
                &[
                    JValue::Object(&uri),
                    JValue::Object(&columns),
                    JValue::Object(&none),
                    JValue::Object(&none),
                    JValue::Object(&none),
                ],
            )?
            .l()?;
        if cursor.is_null() {
            return Ok(None);
        }
        let name = first_text(env, &cursor);
        env.call_method(&cursor, "close", "()V", &[])?;
        name
    })
    .flatten()
}

/// `uri` as Android's `Uri`.
#[cfg(target_os = "android")]
fn android_uri<'local>(
    env: &mut jni::JNIEnv<'local>,
    uri: &Url,
) -> jni::errors::Result<jni::objects::JObject<'local>> {
    let text = env.new_string(uri.as_str())?;
    env.call_static_method(
        "android/net/Uri",
        "parse",
        "(Ljava/lang/String;)Landroid/net/Uri;",
        &[jni::objects::JValue::Object(&text)],
    )?
    .l()
}

/// The text of the first row of `cursor`, if it has one.
#[cfg(target_os = "android")]
fn first_text(env: &mut jni::JNIEnv, cursor: &jni::objects::JObject) -> jni::errors::Result<Option<String>> {
    use jni::objects::{JString, JValue};
    if !env.call_method(cursor, "moveToFirst", "()Z", &[])?.z()? {
        return Ok(None);
    }
    let text =
        JString::from(env.call_method(cursor, "getString", "(I)Ljava/lang/String;", &[JValue::Int(0)])?.l()?);
    if text.is_null() {
        return Ok(None);
    }
    let text: String = env.get_string(&text)?.into();
    Ok(Some(text))
}

#[cfg(not(target_os = "android"))]
fn given_name(_uri: &Url) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    fn uri(text: &str) -> Url {
        Url::parse(text).unwrap()
    }

    #[test]
    fn a_computer_picks_a_path_and_android_a_uri() {
        let picked = |text: &str| serde_json::from_value::<FilePath>(text.into()).unwrap();
        assert!(matches!(picked(r"C:\Users\dj\prisme-diagnostic.txt"), FilePath::Path(_)));
        assert!(matches!(picked("/home/dj/prisme-diagnostic.txt"), FilePath::Path(_)));
        let android = "content://com.android.externalstorage.documents/document/primary%3ADownload%2Fprisme-diagnostic.txt";
        assert!(matches!(picked(android), FilePath::Url(_)));
        assert_eq!(name(&picked(r"C:\Users\dj\set.m3u8")), r"C:\Users\dj\set.m3u8");
    }

    #[test]
    fn a_uri_is_named_as_its_application_names_it_else_by_its_end() {
        let stored =
            uri("content://com.android.externalstorage.documents/document/primary%3AMusic%2FKick.wav");
        assert_eq!(name_of(None, &stored), "Kick.wav");
        assert_eq!(name_of(Some("Snare: tight.wav"), &stored), "Snare: tight.wav");
        assert_eq!(name_of(Some("odd/Clap.wav"), &stored), "Clap.wav");
        let downloaded = uri("content://com.android.providers.downloads.documents/document/msf%3A1000");
        assert_eq!(name_of(None, &downloaded), "1000");
        assert_eq!(name_of(Some(".."), &downloaded), "file");
        assert_eq!(name_of(None, &uri("content://media/")), "file");
    }

    #[test]
    fn a_file_picked_to_save_into_is_emptied_first_by_its_path_or_uri() {
        let path = std::env::temp_dir().join(format!("prisme-picked-{}.txt", std::process::id()));
        let by_uri = || FilePath::Url(Url::from_file_path(&path).unwrap());
        for picked in [FilePath::Path(path.clone()), by_uri()] {
            std::fs::write(&path, "a longer text before").unwrap();
            create(picked).unwrap().write_all(b"short").unwrap();
            let mut read = String::new();
            open(by_uri()).unwrap().read_to_string(&mut read).unwrap();
            assert_eq!(read, "short");
        }
        std::fs::remove_file(&path).unwrap();
        assert!(open(FilePath::Path(path)).is_err());
        assert!(open(FilePath::Url(uri("https://example.com/set.m3u8"))).is_err());
    }
}
