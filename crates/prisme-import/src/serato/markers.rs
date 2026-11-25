//! Serato's cue points, saved loops and track colour ("Serato Markers2"), and its beat
//! grid ("Serato BeatGrid"), as stored in the tags of a file.
//!
//! Markers2 is base64 text whose decoded form is a list of entries, each a name ending
//! with NUL, a big-endian `u32` length and the entry's bytes.

use crate::ImportedCue;
use base64::engine::general_purpose::STANDARD_NO_PAD;
use base64::Engine;
use prisme_grid::{Anchor, BeatGrid};

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

/// The grid of a "Serato BeatGrid" tag: `01 00 | marker count (u32) | markers`, each marker
/// a position (f32 seconds) then the beats to the next marker (u32), or for the last one the
/// tempo (f32); the first marker on bar 1. Markers pin a grid whose tempo changes (a track
/// played live).
pub fn parse_beatgrid(data: &[u8]) -> Option<BeatGrid> {
    let rest = data.strip_prefix(&[1, 0])?;
    let count = u32::from_be_bytes(rest.get(..4)?.try_into().ok()?) as usize;
    let markers: Vec<[u8; 8]> = rest
        .get(4..4usize.checked_add(count.checked_mul(8)?)?)?
        .chunks_exact(8)
        .map(|m| m.try_into().unwrap_or_default())
        .collect();
    let (last, before) = markers.split_last()?;
    let half = |m: &[u8; 8], at: usize| -> [u8; 4] { m[at..at + 4].try_into().unwrap_or_default() };
    let position = |m: &[u8; 8]| f64::from(f32::from_be_bytes(half(m, 0)));
    let bpm = f64::from(f32::from_be_bytes(half(last, 4)));
    if before.is_empty() {
        return (bpm > 0.0 && position(last).is_finite())
            .then(|| BeatGrid::steady(bpm, position(last), true));
    }
    let mut beat = 0.0;
    let mut anchors = Vec::with_capacity(markers.len());
    for marker in before {
        anchors.push(Anchor { beat, time_s: position(marker) });
        beat += f64::from(u32::from_be_bytes(half(marker, 4)));
    }
    anchors.push(Anchor { beat, time_s: position(last) });
    BeatGrid::anchored(anchors, bpm, true)
}

/// The grid of a "Serato Tempo Map" tag, as Serato DJ Pro 3 writes it: `02 00 00 01 | first
/// beat, on bar 1 (f64 seconds) | marker count (u32) | markers`, each 33 bytes and starting
/// with its place (u64) and its tempo (f64). Only grids with a single tempo are kept: what the
/// place of a marker after the first counts is not known.
pub fn parse_tempo_map(data: &[u8]) -> Option<BeatGrid> {
    let rest = data.strip_prefix(&[2, 0])?;
    let bar_s = f64::from_be_bytes(rest.get(2..10)?.try_into().ok()?);
    let count = u32::from_be_bytes(rest.get(10..14)?.try_into().ok()?);
    if count != 1 {
        return None;
    }
    let bpm = f64::from_be_bytes(rest.get(22..30)?.try_into().ok()?);
    (bpm > 0.0 && bar_s.is_finite()).then(|| BeatGrid::steady(bpm, bar_s, true))
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
        assert_eq!(parse_beatgrid(&data), Some(BeatGrid::steady(124.0, 0.125, true)));
        assert_eq!(parse_beatgrid(&[1, 0, 0, 0, 0, 2]), None, "markers missing");
    }

    #[test]
    fn a_grid_whose_tempo_changes() {
        // 8 beats from 0.5 s to 4.5 s (120 BPM), 8 more to 9.5 s (96 BPM), then 100 BPM
        let mut data = vec![1, 0];
        data.extend(3u32.to_be_bytes());
        for (position, beats) in [(0.5f32, 8u32), (4.5, 8)] {
            data.extend(position.to_be_bytes());
            data.extend(beats.to_be_bytes());
        }
        data.extend(9.5f32.to_be_bytes());
        data.extend(100.0f32.to_be_bytes());
        data.push(0);
        let grid = parse_beatgrid(&data).unwrap();
        assert_eq!(grid.anchors().len(), 3);
        assert!((grid.first_beat_s() - 0.5).abs() < 1e-6);
        assert!((grid.bpm_at(2.0) - 120.0).abs() < 1e-3);
        assert!((grid.bpm_at(6.0) - 96.0).abs() < 1e-3);
        assert!((grid.bpm_at(12.0) - 100.0).abs() < 1e-3);
    }

    /// As Serato DJ Pro wrote it for a track at 132 BPM.
    const TEMPO_MAP: &str = "02 00 00 01 3f af 20 0a 5f 6a 53 6d 00 00 00 01 00 00 00 00 00 00 00 00 \
        40 60 7f fe 6c 06 81 a0 3f d0 00 00 00 00 00 00 01 00 00 00 00 00 00 00 00";

    fn bytes(hex: &str) -> Vec<u8> {
        hex.split_whitespace().map(|h| u8::from_str_radix(h, 16).unwrap()).collect()
    }

    #[test]
    fn a_single_tempo_map() {
        let grid = parse_tempo_map(&bytes(TEMPO_MAP)).unwrap();
        let (bpm, bar_s) = (grid.bpm(), grid.first_beat_s());
        assert!((bpm - 132.0).abs() < 0.001 && (bar_s - 0.0608).abs() < 0.0001, "{grid:?}");
        let mut dynamic = bytes(TEMPO_MAP);
        dynamic[15] = 2;
        assert_eq!(parse_tempo_map(&dynamic), None);
        assert_eq!(parse_tempo_map(b"\x01\x00"), None);
    }
}
