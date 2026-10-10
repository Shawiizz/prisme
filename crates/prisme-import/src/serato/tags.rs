//! Serato's markers and beat grid in a file's tags: ID3 `GEOB` frames in MP3 files; in
//! FLAC and MP4 files, base64 of `application/octet-stream\0\0<name>\0` and the same
//! content.

use super::markers::{parse_beatgrid, parse_markers2, Markers};
use crate::ImportedGrid;
use base64::engine::general_purpose::STANDARD_NO_PAD;
use base64::Engine;
use lofty::config::ParseOptions;
use lofty::file::{AudioFile, FileType};
use lofty::id3::v2::{Frame, GeneralEncapsulatedObject, Id3v2Tag};
use lofty::mp4::{AtomData, AtomIdent};
use std::borrow::Cow;
use std::fs::File;
use std::path::Path;

const MARKERS: Names = Names { geob: "Serato Markers2", vorbis: "SERATO_MARKERS_V2", mp4: "markersv2" };
const BEATGRID: Names = Names { geob: "Serato BeatGrid", vorbis: "SERATO_BEATGRID", mp4: "beatgrid" };

/// What a tag is called in each format.
struct Names {
    geob: &'static str,
    vorbis: &'static str,
    mp4: &'static str,
}

#[derive(Debug, Default, PartialEq)]
pub struct SeratoTags {
    pub markers: Option<Markers>,
    pub grid: Option<ImportedGrid>,
}

/// `None` if the file cannot be read or has no Serato tags.
pub fn read_serato_tags(path: &Path) -> Option<SeratoTags> {
    let file_type = lofty::probe::Probe::open(path).ok()?.guess_file_type().ok()?.file_type()?;
    let mut file = File::open(path).ok()?;
    let options = ParseOptions::new().read_properties(false).read_cover_art(false);
    let (markers, grid) = match file_type {
        FileType::Mpeg => {
            let tag = lofty::mpeg::MpegFile::read_from(&mut file, options).ok()?.id3v2()?.clone();
            (geob(&tag, MARKERS.geob), geob(&tag, BEATGRID.geob))
        }
        FileType::Flac => {
            let flac = lofty::flac::FlacFile::read_from(&mut file, options).ok()?;
            let comments = flac.vorbis_comments()?;
            let read = |names: &Names| comments.get(names.vorbis).and_then(|v| unwrap_base64(v, names.geob));
            (read(&MARKERS), read(&BEATGRID))
        }
        FileType::Mp4 => {
            let mp4 = lofty::mp4::Mp4File::read_from(&mut file, options).ok()?;
            let ilst = mp4.ilst()?;
            let read = |names: &Names| {
                let ident = AtomIdent::Freeform {
                    mean: Cow::Borrowed("com.serato.dj"),
                    name: Cow::Borrowed(names.mp4),
                };
                ilst.get(&ident)?.data().find_map(|d| match d {
                    AtomData::UTF8(text) => unwrap_base64(text, names.geob),
                    _ => None,
                })
            };
            (read(&MARKERS), read(&BEATGRID))
        }
        _ => return None,
    };
    let tags = SeratoTags {
        markers: markers.and_then(|d| parse_markers2(&d)),
        grid: grid.and_then(|d| parse_beatgrid(&d)),
    };
    (tags != SeratoTags::default()).then_some(tags)
}

fn geob(tag: &Id3v2Tag, descriptor: &str) -> Option<Vec<u8>> {
    tag.into_iter().find_map(|frame| match frame {
        Frame::Binary(b) if b.id().as_str() == "GEOB" => GeneralEncapsulatedObject::parse(&b.data, b.flags())
            .ok()
            .filter(|o| o.descriptor.as_deref() == Some(descriptor))
            .map(|o| o.data),
        _ => None,
    })
}

fn unwrap_base64(value: &str, descriptor: &str) -> Option<Vec<u8>> {
    let text: String =
        value.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/')).collect();
    let decoded = STANDARD_NO_PAD.decode(text).ok()?;
    let header = format!("application/octet-stream\0\0{descriptor}\0");
    decoded.strip_prefix(header.as_bytes()).map(<[u8]>::to_vec)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serato::markers::tests::{cue, markers2};
    use lofty::config::WriteOptions;
    use lofty::id3::v2::{BinaryFrame, FrameId};
    use lofty::tag::TagExt;
    use lofty::TextEncoding;

    #[test]
    fn markers_are_read_from_an_mp3() {
        let geob = GeneralEncapsulatedObject::new(
            TextEncoding::Latin1,
            Some("application/octet-stream".into()),
            None,
            Some("Serato Markers2".into()),
            markers2(&[("CUE", cue(2, 30_000, [0, 0, 0xcc], "drop"))]),
        );
        let mut tag = Id3v2Tag::new();
        let frame = geob.as_bytes(WriteOptions::default()).unwrap();
        tag.insert(Frame::Binary(BinaryFrame::new(FrameId::new("GEOB").unwrap(), frame)));
        let mut mp3 = Vec::new();
        tag.dump_to(&mut mp3, WriteOptions::default()).unwrap();
        // silent MPEG-1 layer III frames, 128 kb/s at 44.1 kHz: 417 bytes each
        for _ in 0..8 {
            mp3.extend([0xff, 0xfb, 0x90, 0x64]);
            mp3.extend([0; 413]);
        }
        let path = std::env::temp_dir().join(format!("prisme-serato-{}.mp3", std::process::id()));
        std::fs::write(&path, mp3).unwrap();

        let tags = read_serato_tags(&path).expect("Serato tags");
        std::fs::remove_file(&path).ok();
        let cues = tags.markers.unwrap().cues;
        assert_eq!(
            (cues.len(), cues[0].hot, cues[0].position_s, cues[0].label.as_str()),
            (1, Some(2), 30.0, "drop")
        );
        assert_eq!(tags.grid, None);
    }

    #[test]
    fn flac_and_mp4_values_are_unwrapped() {
        let mut raw = b"application/octet-stream\0\0Serato Markers2\0".to_vec();
        raw.extend([1, 1, b'A', b'Q']);
        let value = base64::engine::general_purpose::STANDARD.encode(&raw);
        assert_eq!(unwrap_base64(&value, "Serato Markers2"), Some(vec![1, 1, b'A', b'Q']));
        assert_eq!(unwrap_base64(&value, "Serato BeatGrid"), None);
    }
}
