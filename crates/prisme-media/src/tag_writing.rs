//! Writing into a file's tags, where DJ software reads them: the tempo and key (ID3v2
//! TBPM and TKEY, Vorbis BPM and INITIALKEY, MP4 tmpo and iTunes initialkey) and the
//! genre.
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
use std::cell::Cell;
use std::fs::File;
use std::path::Path;

/// What to write: a missing value leaves the file's own as it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TempoAndKey<'a> {
    pub bpm: Option<f64>,
    /// Musical notation ("Am", "F#"), as most software expects it.
    pub key: Option<&'a str>,
}

/// Which of the values a file already holds are written over.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Replacing<'a> {
    /// Whatever the file holds.
    Any,
    /// Those Prisme wrote before, still as it wrote them: what another program put in the
    /// file (Mixed In Key, say) is kept.
    Own(TempoAndKey<'a>),
}

/// The tempo and the key, one flag each.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TagFields {
    pub bpm: bool,
    pub key: bool,
}

impl TagFields {
    pub const NONE: Self = Self { bpm: false, key: false };
    pub const BOTH: Self = Self { bpm: true, key: true };

    pub fn any(self) -> bool {
        self.bpm || self.key
    }
}

/// Writes `values` into the tags of `path` where the file holds none, or one `replacing`
/// lets go; returns those written. Values already there are not written again, formats
/// without such tags are left alone.
pub fn write_tempo_and_key(
    path: &Path,
    values: TempoAndKey<'_>,
    replacing: Replacing<'_>,
) -> Result<TagFields, MediaError> {
    let written = Cell::new(TagFields::NONE);
    let record = |fields: TagFields| {
        written.set(fields);
        fields.any()
    };
    write(
        path,
        Setters {
            id3v2: &|tag| record(set_id3v2(tag, values, replacing)),
            vorbis: &|comments| record(set_vorbis(comments, values, replacing)),
            ilst: &|ilst| record(set_ilst(ilst, values, replacing)),
        },
    )?;
    Ok(written.get())
}

/// A value to write, as a kind of tag spells it.
struct Field {
    value: Option<String>,
    /// What Prisme wrote before, spelt the same way.
    own: Option<String>,
    replace_any: bool,
}

impl Field {
    /// The tempo, spelt by `tempo`, and the key.
    fn pair(values: TempoAndKey<'_>, replacing: Replacing<'_>, tempo: impl Fn(f64) -> String) -> [Self; 2] {
        let (own, replace_any) = match replacing {
            Replacing::Any => (TempoAndKey { bpm: None, key: None }, true),
            Replacing::Own(own) => (own, false),
        };
        [
            Self { value: values.bpm.map(&tempo), own: own.bpm.map(&tempo), replace_any },
            Self { value: values.key.map(str::to_owned), own: own.key.map(str::to_owned), replace_any },
        ]
    }

    /// The value, if it goes in where the file holds `current`.
    fn in_place_of(self, current: Option<&str>) -> Option<String> {
        let value = self.value?;
        let goes_in = match current.map(str::trim) {
            Some(current) if current == value => false,
            Some(current) if !current.is_empty() => self.replace_any || self.own.as_deref() == Some(current),
            _ => true,
        };
        goes_in.then_some(value)
    }
}

/// Writes the genre of `path` (`None` takes it off); returns whether the file changed:
/// ID3v2 TCON, Vorbis GENRE, MP4 ©gen.
pub fn write_genre(path: &Path, genre: Option<&str>) -> Result<bool, MediaError> {
    write(
        path,
        Setters {
            id3v2: &|tag| {
                let id = FrameId::Valid(Cow::Borrowed("TCON"));
                match genre {
                    Some(genre) if tag.get_text(&id) == Some(genre) => false,
                    // UTF-16: accents survive in ID3v2.3, which has no UTF-8
                    Some(genre) => {
                        tag.insert(Frame::Text(TextInformationFrame::new(
                            id,
                            TextEncoding::UTF16,
                            genre.to_owned(),
                        )));
                        true
                    }
                    None => tag.remove(&id).count() > 0,
                }
            },
            vorbis: &|comments| match genre {
                Some(genre) if comments.get("GENRE") == Some(genre) => false,
                Some(genre) => {
                    comments.insert("GENRE".into(), genre.into());
                    true
                }
                None => comments.remove("GENRE").count() > 0,
            },
            ilst: &|ilst| {
                let ident = AtomIdent::Fourcc(*b"\xa9gen");
                // iTunes' older numbered genre would contradict the new one
                let numbered = ilst.remove(&AtomIdent::Fourcc(*b"gnre")).count() > 0;
                match genre {
                    Some(genre) => {
                        let data = AtomData::UTF8(genre.into());
                        if ilst.get(&ident).and_then(|a| a.data().next()) == Some(&data) {
                            return numbered;
                        }
                        ilst.replace_atom(Atom::new(ident, data));
                        true
                    }
                    None => ilst.remove(&ident).count() > 0 || numbered,
                }
            },
        },
    )
}

/// How each kind of tag is changed; each returns whether it changed something.
struct Setters<'a> {
    id3v2: &'a dyn Fn(&mut Id3v2Tag) -> bool,
    vorbis: &'a dyn Fn(&mut VorbisComments) -> bool,
    ilst: &'a dyn Fn(&mut Ilst) -> bool,
}

fn write(path: &Path, set: Setters<'_>) -> Result<bool, MediaError> {
    let extension = path.extension().map(|e| e.to_string_lossy().to_lowercase());
    match extension.as_deref() {
        Some("mp3") => update(path, |f: &mut MpegFile| {
            let version = f.id3v2().map(|t| t.original_version());
            if f.id3v2().is_none() {
                f.set_id3v2(Id3v2Tag::new());
            }
            ((set.id3v2)(f.id3v2_mut().expect("set above")), version)
        }),
        Some("wav") => update(path, |f: &mut WavFile| {
            let version = f.id3v2().map(|t| t.original_version());
            if f.id3v2().is_none() {
                f.set_id3v2(Id3v2Tag::new());
            }
            ((set.id3v2)(f.id3v2_mut().expect("set above")), version)
        }),
        Some("flac") => update(path, |f: &mut FlacFile| {
            if f.vorbis_comments().is_none() {
                f.set_vorbis_comments(VorbisComments::new());
            }
            ((set.vorbis)(f.vorbis_comments_mut().expect("set above")), None)
        }),
        Some("ogg" | "oga") => {
            update(path, |f: &mut VorbisFile| ((set.vorbis)(f.vorbis_comments_mut()), None))
        }
        Some("m4a" | "mp4") => update(path, |f: &mut Mp4File| {
            if f.ilst().is_none() {
                f.set_ilst(Ilst::new());
            }
            ((set.ilst)(f.ilst_mut().expect("set above")), None)
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
        let copy = path.with_file_name(format!(
            ".{}.prisme-tags",
            path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()
        ));
        let saved = save_in_copy(&file, path, &copy, options);
        if saved.is_err() {
            let _ = std::fs::remove_file(&copy);
        }
        saved.map_err(|e| error(&e))?;
    }
    Ok(changed)
}

/// Saves `file`'s tags into `copy`, a copy of `path`, then puts it in `path`'s place once it
/// reads back and is on the disk: a crash, a full disk or a file held open by another
/// program leaves the track as it was, never half written (lofty saves in place, moving the
/// audio behind the tags it grows).
fn save_in_copy<F: AudioFile>(
    file: &F,
    path: &Path,
    copy: &Path,
    options: WriteOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::copy(path, copy)?;
    file.save_to_path(copy, options)?;
    F::read_from(&mut File::open(copy)?, ParseOptions::new())?;
    std::fs::OpenOptions::new().write(true).open(copy)?.sync_all()?;
    std::fs::rename(copy, path)?;
    Ok(())
}

fn set_id3v2(tag: &mut Id3v2Tag, values: TempoAndKey<'_>, replacing: Replacing<'_>) -> TagFields {
    // TBPM holds a whole number of beats per minute
    let [bpm, key] = Field::pair(values, replacing, |bpm| format!("{}", bpm.round()));
    let mut set = |id: &'static str, field: Field| {
        let id = FrameId::Valid(Cow::Borrowed(id));
        let Some(value) = field.in_place_of(tag.get_text(&id)) else { return false };
        tag.insert(Frame::Text(TextInformationFrame::new(id, TextEncoding::Latin1, value)));
        true
    };
    TagFields { bpm: set("TBPM", bpm), key: set("TKEY", key) }
}

fn set_vorbis(comments: &mut VorbisComments, values: TempoAndKey<'_>, replacing: Replacing<'_>) -> TagFields {
    let [bpm, key] = Field::pair(values, replacing, |bpm| format!("{bpm:.2}"));
    let mut set = |name: &str, field: Field| {
        let Some(value) = field.in_place_of(comments.get(name)) else { return false };
        comments.insert(name.to_owned(), value);
        true
    };
    TagFields { bpm: set("BPM", bpm), key: set("INITIALKEY", key) }
}

fn set_ilst(ilst: &mut Ilst, values: TempoAndKey<'_>, replacing: Replacing<'_>) -> TagFields {
    let whole = |bpm: f64| bpm.round() as i16;
    let [bpm, key] = Field::pair(values, replacing, |bpm| whole(bpm).to_string());
    let mut written = TagFields::NONE;

    let ident = AtomIdent::Fourcc(*b"tmpo");
    let current = match ilst.get(&ident).and_then(|a| a.data().next()) {
        Some(AtomData::SignedInteger(0)) | None => None,
        Some(AtomData::SignedInteger(n)) => Some(n.to_string()),
        Some(_) => Some(String::from("?")),
    };
    if let (Some(_), Some(tempo)) = (bpm.in_place_of(current.as_deref()), values.bpm) {
        // tmpo holds 2 bytes, as iTunes writes it (lofty would write 4)
        let data =
            AtomData::Unknown { code: DataType::BeSignedInteger, data: whole(tempo).to_be_bytes().to_vec() };
        ilst.replace_atom(Atom::new(ident, data));
        written.bpm = true;
    }

    let ident = AtomIdent::Freeform { mean: "com.apple.iTunes".into(), name: "initialkey".into() };
    let current = match ilst.get(&ident).and_then(|a| a.data().next()) {
        Some(AtomData::UTF8(text)) => Some(text.clone()),
        Some(_) => Some(String::from("?")),
        None => None,
    };
    if let Some(value) = key.in_place_of(current.as_deref()) {
        ilst.replace_atom(Atom::new(ident, AtomData::UTF8(value)));
        written.key = true;
    }
    written
}
