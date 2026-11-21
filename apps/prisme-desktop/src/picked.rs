//! Files the DJ picks in the system's dialogs. A computer gives their path; Android gives the
//! `content://` URI of the application holding the file, which the system opens for Prisme:
//! both are opened through the fs plugin.

use percent_encoding::percent_decode_str;
use std::fs::File;
use std::io;
use tauri::{AppHandle, Url};
use tauri_plugin_fs::{FilePath, FsExt, OpenOptions};

/// The file picked to save into, emptied.
pub fn create(app: &AppHandle, file: FilePath) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    app.fs().open(file, options)
}

/// The file picked to read.
pub fn open(app: &AppHandle, file: FilePath) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    app.fs().open(file, options)
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
        let text = env.new_string(uri.as_str())?;
        let uri = env
            .call_static_method(
                "android/net/Uri",
                "parse",
                "(Ljava/lang/String;)Landroid/net/Uri;",
                &[JValue::Object(&text)],
            )?
            .l()?;
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
}
