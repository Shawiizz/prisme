//! Writing the tempo and key into a file's tags, where DJ software reads them: ID3v2
//! TBPM and TKEY, Vorbis BPM and INITIALKEY, MP4 tmpo and iTunes initialkey.
//!
//! Each format goes through its own tag type: lofty's generic tag would drop what it
//! does not know, such as the ID3v2 GEOB frames where Serato keeps cues and beat grids.

use crate::MediaError;
use lofty::config::{ParseOptions, WriteOptions};
use lofty::file::AudioFile;
use lofty::flac::FlacFile;
use lofty::id3::v2::{Frame, FrameId, Id3v2Tag, Id3v2Version, TextInformationFrame};
use lofty::iff::wav::WavFile;
use lofty::mp4::{Atom, AtomData, AtomIdent, DataType, Ilst, Mp4File};
use lofty::mpeg::MpegFile;
use lofty::ogg::tag::VorbisComments;
use lofty::ogg::VorbisFile;
use lofty::TextEncoding;
use std::borrow::Cow;
use std::fs::File;
use std::path::Path;

/// What to write: a missing value leaves the file's own as it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TempoAndKey<'a> {
    pub bpm: Option<f64>,
    /// Musical notation ("Am", "F#"), as most software expects it.
    pub key: Option<&'a str>,
}

/// Writes `values` into the tags of `path`; returns whether the file changed (values
/// already there are not written again, formats without such tags are left alone).
pub fn write_tempo_and_key(path: &Path, values: TempoAndKey<'_>) -> Result<bool, MediaError> {
    let extension = path.extension().map(|e| e.to_string_lossy().to_lowercase());
    match extension.as_deref() {
        Some("mp3") => update(path, |f: &mut MpegFile| {
            let version = f.id3v2().map(|t| t.original_version());
            if f.id3v2().is_none() {
                f.set_id3v2(Id3v2Tag::new());
            }
            (set_id3v2(f.id3v2_mut().expect("set above"), values), version)
        }),
        Some("wav") => update(path, |f: &mut WavFile| {
            let version = f.id3v2().map(|t| t.original_version());
            if f.id3v2().is_none() {
                f.set_id3v2(Id3v2Tag::new());
            }
            (set_id3v2(f.id3v2_mut().expect("set above"), values), version)
        }),
        Some("flac") => update(path, |f: &mut FlacFile| {
            if f.vorbis_comments().is_none() {
                f.set_vorbis_comments(VorbisComments::new());
            }
            (set_vorbis(f.vorbis_comments_mut().expect("set above"), values), None)
        }),
        Some("ogg" | "oga") => {
            update(path, |f: &mut VorbisFile| (set_vorbis(f.vorbis_comments_mut(), values), None))
        }
        Some("m4a" | "mp4") => update(path, |f: &mut Mp4File| {
            if f.ilst().is_none() {
                f.set_ilst(Ilst::new());
            }
            (set_ilst(f.ilst_mut().expect("set above"), values), None)
        }),
        _ => Ok(false),
    }
}

/// Reads `path` as `F`, lets `change` edit its tags, saves it if they changed. `change`
/// also gives the ID3v2 version the file had, kept so that older readers still read it.
fn update<F: AudioFile>(
    path: &Path,
    change: impl FnOnce(&mut F) -> (bool, Option<Id3v2Version>),
) -> Result<bool, MediaError> {
    let name = || path.display().to_string();
    let error = |e: &dyn std::fmt::Display| MediaError::WriteTags { path: name(), message: e.to_string() };
    let mut reader = File::open(path).map_err(|source| MediaError::Open { path: name(), source })?;
    let mut file = F::read_from(&mut reader, ParseOptions::new()).map_err(|e| error(&e))?;
    drop(reader);
    let (changed, version) = change(&mut file);
    if changed {
        let options = WriteOptions::new().use_id3v23(version == Some(Id3v2Version::V3));
        file.save_to_path(path, options).map_err(|e| error(&e))?;
    }
    Ok(changed)
}

fn set_id3v2(tag: &mut Id3v2Tag, values: TempoAndKey<'_>) -> bool {
    let mut set = |id: &'static str, value: String| {
        let id = FrameId::Valid(Cow::Borrowed(id));
        if tag.get_text(&id) == Some(value.as_str()) {
            return false;
        }
        tag.insert(Frame::Text(TextInformationFrame::new(id, TextEncoding::Latin1, value)));
        true
    };
    // TBPM holds a whole number of beats per minute
    let bpm = values.bpm.is_some_and(|bpm| set("TBPM", format!("{}", bpm.round())));
    let key = values.key.is_some_and(|key| set("TKEY", key.to_owned()));
    bpm | key
}

fn set_vorbis(comments: &mut VorbisComments, values: TempoAndKey<'_>) -> bool {
    let mut set = |key: &str, value: String| {
        if comments.get(key) == Some(value.as_str()) {
            return false;
        }
        comments.insert(key.to_owned(), value);
        true
    };
    let bpm = values.bpm.is_some_and(|bpm| set("BPM", format!("{bpm:.2}")));
    let key = values.key.is_some_and(|key| set("INITIALKEY", key.to_owned()));
    bpm | key
}

fn set_ilst(ilst: &mut Ilst, values: TempoAndKey<'_>) -> bool {
    let mut bpm_changed = false;
    if let Some(bpm) = values.bpm.map(|bpm| bpm.round() as i16) {
        let ident = AtomIdent::Fourcc(*b"tmpo");
        if ilst.get(&ident).and_then(|a| a.data().next()) != Some(&AtomData::SignedInteger(i32::from(bpm))) {
            // tmpo holds 2 bytes, as iTunes writes it (lofty would write 4)
            let data =
                AtomData::Unknown { code: DataType::BeSignedInteger, data: bpm.to_be_bytes().to_vec() };
            ilst.replace_atom(Atom::new(ident, data));
            bpm_changed = true;
        }
    }
    let mut key_changed = false;
    if let Some(key) = values.key {
        let ident = AtomIdent::Freeform { mean: "com.apple.iTunes".into(), name: "initialkey".into() };
        let data = AtomData::UTF8(key.to_owned());
        if ilst.get(&ident).and_then(|a| a.data().next()) != Some(&data) {
            ilst.replace_atom(Atom::new(ident, data));
            key_changed = true;
        }
    }
    bpm_changed | key_changed
}
