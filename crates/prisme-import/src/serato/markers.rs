//! Serato's cue points, saved loops and track colour ("Serato Markers2"), and its beat
//! grid ("Serato BeatGrid"), as stored in the tags of a file.
//!
//! Markers2 is base64 text whose decoded form is a list of entries, each a name ending
//! with NUL, a big-endian `u32` length and the entry's bytes.

use crate::{ImportedCue, ImportedGrid};
use base64::engine::general_purpose::STANDARD_NO_PAD;
use base64::Engine;

#[derive(Debug, Default, PartialEq)]
pub struct Markers {
    pub cues: Vec<ImportedCue>,
    pub color: Option<u32>,
}

/// Serato's colour for "no colour".
const NO_COLOR: u32 = 0xffffff;

/// The content of a Markers2 tag (the GEOB data, or what follows the header in MP4 and
/// FLAC tags): `01 01` then base64 text.
pub fn parse_markers2(data: &[u8]) -> Option<Markers> {
    let text: Vec<u8> = data
        .strip_prefix(&[1, 1])?
        .iter()
        .copied()
        .filter(|b| b.is_ascii_alphanumeric() || *b == b'+' || *b == b'/')
        .collect();
    let decoded = STANDARD_NO_PAD.decode(trim_to_whole_quantum(&text)).ok()?;
    let mut entries = decoded.strip_prefix(&[1, 1])?;
    let mut markers = Markers::default();
    while let Some(name_end) = entries.iter().position(|b| *b == 0) {
        if name_end == 0 {
            break;
        }
        let name = &entries[..name_end];
        let rest = &entries[name_end + 1..];
        let length = u32::from_be_bytes(rest.get(..4)?.try_into().ok()?) as usize;
        let body = rest.get(4..4 + length)?;
        match name {
            b"CUE" => markers.cues.extend(parse_cue(body)),
            b"LOOP" => markers.cues.extend(parse_loop(body)),
            b"COLOR" => markers.color = body.get(1..4).map(rgb).filter(|c| *c != NO_COLOR),
            _ => {}
        }
        entries = &rest[4 + length..];
    }
    Some(markers)
}

/// `00 | index | position ms (u32) | 00 | RGB | 00 00 | name NUL`
fn parse_cue(body: &[u8]) -> Option<ImportedCue> {
    Some(ImportedCue {
        hot: Some(*body.get(1)?),
        position_s: ms(body.get(2..6)?),
        loop_length_s: None,
        color: rgb(body.get(7..10)?),
        label: text(body.get(12..)?),
    })
}

/// `00 | index | start ms | end ms | FF FF FF FF | 00 RGB | 00 | locked | name NUL`
fn parse_loop(body: &[u8]) -> Option<ImportedCue> {
    let start_s = ms(body.get(2..6)?);
    let end_s = ms(body.get(6..10)?);
    Some(ImportedCue {
        // saved loops have their own slots in Serato: they take free hot cues here
        hot: None,
        position_s: start_s,
        loop_length_s: Some(end_s - start_s).filter(|l| *l > 0.0),
        color: rgb(body.get(15..18)?),
        label: text(body.get(20..)?),
    })
    .filter(|c| c.loop_length_s.is_some())
}

/// The grid of a "Serato BeatGrid" tag: `01 00 | marker count (u32) | markers`, each
/// marker a position (f32 seconds) and, for the last one, the tempo (f32). Only grids
/// with a single tempo are kept.
pub fn parse_beatgrid(data: &[u8]) -> Option<ImportedGrid> {
    let rest = data.strip_prefix(&[1, 0])?;
    let count = u32::from_be_bytes(rest.get(..4)?.try_into().ok()?);
    if count != 1 {
        return None;
    }
    let marker = rest.get(4..12)?;
    let position = f32::from_be_bytes(marker[..4].try_into().ok()?) as f64;
    let bpm = f32::from_be_bytes(marker[4..8].try_into().ok()?) as f64;
    (bpm > 0.0).then_some(ImportedGrid { bpm, beat_s: position })
}

fn ms(bytes: &[u8]) -> f64 {
    u32::from_be_bytes(bytes.try_into().unwrap_or_default()) as f64 / 1000.0
}

fn rgb(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0, |acc, b| acc << 8 | *b as u32)
}

fn text(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

/// Serato pads the base64 text oddly; the trailing partial quantum is dropped.
fn trim_to_whole_quantum(text: &[u8]) -> &[u8] {
    match text.len() % 4 {
        1 => &text[..text.len() - 1],
        _ => text,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Writes a Markers2 tag the way Serato does.
    pub fn markers2(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let mut raw = vec![1, 1];
        for (name, body) in entries {
            raw.extend(name.as_bytes());
            raw.push(0);
            raw.extend((body.len() as u32).to_be_bytes());
            raw.extend(body);
        }
        raw.push(0);
        let text = STANDARD_NO_PAD.encode(&raw);
        let mut data = vec![1, 1];
        for line in text.as_bytes().chunks(72) {
            data.extend(line);
            data.push(b'\n');
        }
        data
    }

    pub fn cue(index: u8, ms: u32, rgb: [u8; 3], name: &str) -> Vec<u8> {
        let mut b = vec![0, index];
        b.extend(ms.to_be_bytes());
        b.push(0);
        b.extend(rgb);
        b.extend([0, 0]);
        b.extend(name.as_bytes());
        b.push(0);
        b
    }

    pub fn saved_loop(index: u8, start_ms: u32, end_ms: u32, name: &str) -> Vec<u8> {
        let mut b = vec![0, index];
        b.extend(start_ms.to_be_bytes());
        b.extend(end_ms.to_be_bytes());
        b.extend([0xff; 4]);
        b.extend([0, 0x27, 0xaa, 0xe1]);
        b.extend([0, 0]);
        b.extend(name.as_bytes());
        b.push(0);
        b
    }

    #[test]
    fn cues_loops_and_colour() {
        let data = markers2(&[
            ("COLOR", vec![0, 0xcc, 0x00, 0x00]),
            ("CUE", cue(0, 1500, [0xcc, 0, 0], "intro")),
            ("CUE", cue(3, 64_250, [0x00, 0xcc, 0x88], "")),
            ("LOOP", saved_loop(0, 32_000, 40_000, "build")),
            ("BPMLOCK", vec![0]),
        ]);
        let markers = parse_markers2(&data).unwrap();
        assert_eq!(markers.color, Some(0xcc0000));
        assert_eq!(
            markers.cues,
            [
                ImportedCue {
                    hot: Some(0),
                    position_s: 1.5,
                    loop_length_s: None,
                    color: 0xcc0000,
                    label: "intro".into()
                },
                ImportedCue {
                    hot: Some(3),
                    position_s: 64.25,
                    loop_length_s: None,
                    color: 0x00cc88,
                    label: String::new()
                },
                ImportedCue {
                    hot: None,
                    position_s: 32.0,
                    loop_length_s: Some(8.0),
                    color: 0x27aae1,
                    label: "build".into()
                },
            ]
        );
    }

    #[test]
    fn white_means_no_colour_and_garbage_is_refused() {
        let data = markers2(&[("COLOR", vec![0, 0xff, 0xff, 0xff])]);
        assert_eq!(parse_markers2(&data).unwrap().color, None);
        assert_eq!(parse_markers2(b"not markers"), None);
    }

    #[test]
    fn a_single_tempo_grid() {
        let mut data = vec![1, 0];
        data.extend(1u32.to_be_bytes());
        data.extend(0.125f32.to_be_bytes());
        data.extend(124.0f32.to_be_bytes());
        data.push(0);
        assert_eq!(parse_beatgrid(&data), Some(ImportedGrid { bpm: 124.0, beat_s: 0.125 }));

        let mut dynamic = vec![1, 0];
        dynamic.extend(2u32.to_be_bytes());
        assert_eq!(parse_beatgrid(&dynamic), None);
    }
}
