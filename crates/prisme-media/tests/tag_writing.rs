//! Tempo and key written into each format's tags, read back the way other software reads
//! them, without losing what the file already held.

use lofty::config::{ParseOptions, WriteOptions};
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::id3::v2::{BinaryFrame, Frame, FrameId, GeneralEncapsulatedObject};
use lofty::mp4::{AtomData, AtomIdent, Mp4File};
use lofty::mpeg::MpegFile;
use lofty::tag::ItemKey;
use lofty::TextEncoding;
use prisme_media::{write_genre, write_tempo_and_key, Replacing, TagFields, TempoAndKey};
use std::borrow::Cow;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

const VALUES: TempoAndKey<'static> = TempoAndKey { bpm: Some(124.03), key: Some("Am") };

/// A copy of a fixture in the temp folder, removed afterwards.
struct Scratch(PathBuf);

impl Scratch {
    fn of(fixture: &str) -> Self {
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(fixture);
        let copy = Self::path(fixture);
        std::fs::copy(source, &copy.0).unwrap();
        copy
    }

    fn path(name: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        Self(std::env::temp_dir().join(format!("prisme-tags-{}-{n}-{name}", std::process::id())))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn read(path: &Path, key: ItemKey) -> Option<String> {
    let file = lofty::read_from_path(path).unwrap();
    file.primary_tag().and_then(|t| t.get_string(key)).map(str::to_owned)
}

/// The tempo of `path`, where `key` holds it (the `tmpo` atom of an MP4 file).
fn read_tempo(path: &Path, key: ItemKey) -> Option<String> {
    if path.extension().is_none_or(|e| e != "m4a") {
        return read(path, key);
    }
    let m4a = Mp4File::read_from(&mut File::open(path).unwrap(), ParseOptions::new()).unwrap();
    let tempo = m4a.ilst()?.get(&AtomIdent::Fourcc(*b"tmpo"))?.data().next().cloned();
    match tempo? {
        AtomData::SignedInteger(tempo) => Some(tempo.to_string()),
        _ => None,
    }
}

#[track_caller]
fn assert_written(fixture: &str, bpm_key: ItemKey, bpm: &str) {
    let file = Scratch::of(fixture);
    assert!(write_tempo_and_key(&file.0, VALUES, Replacing::Any).unwrap().any());
    assert_eq!(read(&file.0, bpm_key).as_deref(), Some(bpm), "{fixture}");
    assert_eq!(read(&file.0, ItemKey::InitialKey).as_deref(), Some("Am"), "{fixture}");

    let before = std::fs::read(&file.0).unwrap();
    assert!(!write_tempo_and_key(&file.0, VALUES, Replacing::Any).unwrap().any(), "{fixture} written again");
    assert_eq!(std::fs::read(&file.0).unwrap(), before);
}

#[test]
fn mp3_gets_tbpm_and_tkey() {
    assert_written("silence.mp3", ItemKey::IntegerBpm, "124");
}

#[test]
fn flac_and_ogg_get_vorbis_comments() {
    assert_written("silence.flac", ItemKey::Bpm, "124.03");
    assert_written("silence.ogg", ItemKey::Bpm, "124.03");
}

#[test]
fn m4a_gets_tmpo_and_initialkey() {
    let file = Scratch::of("silence.m4a");
    assert!(write_tempo_and_key(&file.0, VALUES, Replacing::Any).unwrap().any());
    assert_eq!(read(&file.0, ItemKey::InitialKey).as_deref(), Some("Am"));
    let m4a = Mp4File::read_from(&mut File::open(&file.0).unwrap(), ParseOptions::new()).unwrap();
    let tmpo = m4a.ilst().unwrap().get(&AtomIdent::Fourcc(*b"tmpo")).unwrap();
    assert_eq!(tmpo.data().next(), Some(&AtomData::SignedInteger(124)));
    assert!(!write_tempo_and_key(&file.0, VALUES, Replacing::Any).unwrap().any(), "written again");
}

#[test]
fn wav_gets_an_id3v2_chunk() {
    let file = Scratch::path("silence.wav");
    std::fs::write(&file.0, silent_wav()).unwrap();
    assert!(write_tempo_and_key(&file.0, VALUES, Replacing::Any).unwrap().any());
    assert_eq!(read(&file.0, ItemKey::InitialKey).as_deref(), Some("Am"));
}

#[test]
fn only_the_values_prisme_wrote_are_replaced() {
    let other = TempoAndKey { bpm: Some(128.0), key: Some("Cm") };
    let nothing = TempoAndKey { bpm: None, key: None };
    for (fixture, bpm_key, bpm) in [
        ("silence.mp3", ItemKey::IntegerBpm, "124"),
        ("silence.flac", ItemKey::Bpm, "124.03"),
        ("silence.m4a", ItemKey::IntegerBpm, "124"),
    ] {
        let file = Scratch::of(fixture);
        write_tempo_and_key(&file.0, VALUES, Replacing::Any).unwrap();
        // as if another program had written them
        let kept = write_tempo_and_key(&file.0, other, Replacing::Own(nothing)).unwrap();
        assert_eq!(kept, TagFields::NONE, "{fixture}");
        assert_eq!(read_tempo(&file.0, bpm_key).as_deref(), Some(bpm), "{fixture}");
        // the tempo Prisme's own, the key changed by another program since
        let own = TempoAndKey { bpm: VALUES.bpm, key: Some("Gm") };
        let written = write_tempo_and_key(&file.0, other, Replacing::Own(own)).unwrap();
        assert_eq!(written, TagFields { bpm: true, key: false }, "{fixture}");
        assert_ne!(read_tempo(&file.0, bpm_key).as_deref(), Some(bpm), "{fixture}");
        assert_eq!(read(&file.0, ItemKey::InitialKey).as_deref(), Some("Am"), "{fixture}");
        assert!(write_tempo_and_key(&file.0, other, Replacing::Any).unwrap().key, "{fixture}");
    }
}

#[test]
fn empty_values_are_filled() {
    let file = Scratch::of("silence.mp3");
    let blank = TempoAndKey { bpm: None, key: Some(" ") };
    write_tempo_and_key(&file.0, blank, Replacing::Any).unwrap();
    let nothing = TempoAndKey { bpm: None, key: None };
    assert_eq!(write_tempo_and_key(&file.0, VALUES, Replacing::Own(nothing)).unwrap(), TagFields::BOTH);
}

#[test]
fn serato_markers_are_kept() {
    let file = Scratch::of("silence.mp3");
    let markers = GeneralEncapsulatedObject::new(
        TextEncoding::Latin1,
        Some("application/octet-stream".into()),
        None,
        Some("Serato Markers2".into()),
        vec![1, 2, 3, 4],
    );
    let bytes = markers.as_bytes(WriteOptions::new()).unwrap();
    let geob = FrameId::Valid(Cow::Borrowed("GEOB"));
    let mut mp3 = MpegFile::read_from(&mut File::open(&file.0).unwrap(), ParseOptions::new()).unwrap();
    let mut tag = mp3.id3v2().cloned().unwrap_or_default();
    tag.insert(Frame::Binary(BinaryFrame::new(geob.clone(), bytes.clone())));
    mp3.set_id3v2(tag);
    mp3.save_to_path(&file.0, WriteOptions::new()).unwrap();

    write_tempo_and_key(&file.0, VALUES, Replacing::Any).unwrap();

    let mp3 = MpegFile::read_from(&mut File::open(&file.0).unwrap(), ParseOptions::new()).unwrap();
    let kept = mp3.id3v2().unwrap().get(&geob);
    assert!(matches!(kept, Some(Frame::Binary(b)) if b.data == bytes.clone()), "{kept:?}");
}

#[test]
fn the_file_is_replaced_whole_and_no_copy_is_left() {
    let file = Scratch::of("silence.mp3");
    assert!(write_tempo_and_key(&file.0, VALUES, Replacing::Any).unwrap().any());
    let folder = file.0.parent().unwrap();
    let name = file.0.file_name().unwrap().to_string_lossy().into_owned();
    assert!(!folder.join(format!(".{name}.prisme-tags")).exists());
}

#[test]
fn a_file_that_cannot_be_written_stays_as_it_was() {
    let file = Scratch::of("silence.mp3");
    let before = std::fs::read(&file.0).unwrap();
    let mut permissions = std::fs::metadata(&file.0).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&file.0, permissions.clone()).unwrap();
    let written = write_tempo_and_key(&file.0, VALUES, Replacing::Any);
    #[allow(clippy::permissions_set_readonly_false)]
    permissions.set_readonly(false);
    std::fs::set_permissions(&file.0, permissions).unwrap();
    assert!(written.is_err());
    assert_eq!(std::fs::read(&file.0).unwrap(), before);
    let name = file.0.file_name().unwrap().to_string_lossy().into_owned();
    assert!(!file.0.parent().unwrap().join(format!(".{name}.prisme-tags")).exists());
}

#[test]
fn other_files_are_left_alone() {
    let file = Scratch::path("notes.txt");
    std::fs::write(&file.0, "not audio").unwrap();
    assert!(!write_tempo_and_key(&file.0, VALUES, Replacing::Any).unwrap().any());
}

#[test]
fn the_genre_is_written_replaced_and_taken_off() {
    for fixture in ["silence.mp3", "silence.flac", "silence.ogg", "silence.m4a"] {
        let file = Scratch::of(fixture);
        assert!(write_genre(&file.0, Some("Électro")).unwrap(), "{fixture}");
        assert_eq!(read(&file.0, ItemKey::Genre).as_deref(), Some("Électro"), "{fixture}");
        assert!(!write_genre(&file.0, Some("Électro")).unwrap(), "{fixture} written again");
        assert!(write_genre(&file.0, Some("House")).unwrap(), "{fixture}");
        assert_eq!(read(&file.0, ItemKey::Genre).as_deref(), Some("House"), "{fixture}");
        // the tempo and key written before stay
        write_tempo_and_key(&file.0, VALUES, Replacing::Any).unwrap();
        assert!(write_genre(&file.0, None).unwrap(), "{fixture}");
        assert_eq!(read(&file.0, ItemKey::Genre), None, "{fixture}");
        assert_eq!(read(&file.0, ItemKey::InitialKey).as_deref(), Some("Am"), "{fixture}");
        assert!(!write_genre(&file.0, None).unwrap(), "{fixture} taken off again");
    }
}

/// Half a second of 16-bit mono silence.
fn silent_wav() -> Vec<u8> {
    let data = vec![0u8; 22_050 * 2];
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&44_100u32.to_le_bytes());
    wav.extend_from_slice(&88_200u32.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
    wav.extend_from_slice(&data);
    wav
}
