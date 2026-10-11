//! Tempo and key written into each format's tags, read back the way other software reads
//! them, without losing what the file already held.

use lofty::config::{ParseOptions, WriteOptions};
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::id3::v2::{BinaryFrame, Frame, FrameId, GeneralEncapsulatedObject};
use lofty::mp4::{AtomData, AtomIdent, Mp4File};
use lofty::mpeg::MpegFile;
use lofty::tag::ItemKey;
use lofty::TextEncoding;
use prisme_media::{write_tempo_and_key, TempoAndKey};
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

#[track_caller]
fn assert_written(fixture: &str, bpm_key: ItemKey, bpm: &str) {
    let file = Scratch::of(fixture);
    assert!(write_tempo_and_key(&file.0, VALUES).unwrap());
    assert_eq!(read(&file.0, bpm_key).as_deref(), Some(bpm), "{fixture}");
    assert_eq!(read(&file.0, ItemKey::InitialKey).as_deref(), Some("Am"), "{fixture}");

    let before = std::fs::read(&file.0).unwrap();
    assert!(!write_tempo_and_key(&file.0, VALUES).unwrap(), "{fixture} written again");
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
    assert!(write_tempo_and_key(&file.0, VALUES).unwrap());
    assert_eq!(read(&file.0, ItemKey::InitialKey).as_deref(), Some("Am"));
    let m4a = Mp4File::read_from(&mut File::open(&file.0).unwrap(), ParseOptions::new()).unwrap();
    let tmpo = m4a.ilst().unwrap().get(&AtomIdent::Fourcc(*b"tmpo")).unwrap();
    assert_eq!(tmpo.data().next(), Some(&AtomData::SignedInteger(124)));
    assert!(!write_tempo_and_key(&file.0, VALUES).unwrap(), "written again");
}

#[test]
fn wav_gets_an_id3v2_chunk() {
    let file = Scratch::path("silence.wav");
    std::fs::write(&file.0, silent_wav()).unwrap();
    assert!(write_tempo_and_key(&file.0, VALUES).unwrap());
    assert_eq!(read(&file.0, ItemKey::InitialKey).as_deref(), Some("Am"));
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

    write_tempo_and_key(&file.0, VALUES).unwrap();

    let mp3 = MpegFile::read_from(&mut File::open(&file.0).unwrap(), ParseOptions::new()).unwrap();
    let kept = mp3.id3v2().unwrap().get(&geob);
    assert!(matches!(kept, Some(Frame::Binary(b)) if b.data == bytes.clone()), "{kept:?}");
}

#[test]
fn other_files_are_left_alone() {
    let file = Scratch::path("notes.txt");
    std::fs::write(&file.0, "not audio").unwrap();
    assert!(!write_tempo_and_key(&file.0, VALUES).unwrap());
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
