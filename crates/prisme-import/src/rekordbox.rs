//! rekordbox's XML (File > Export Collection in xml format): tracks with their hot and
//! memory cues, loops, beat grid, colour and rating, and the playlists. Read here, written
//! by [`write`] (which rekordbox reads as its "rekordbox xml" library).
//!
//! The format is rekordbox's "rekordbox xml format" document (rekordbox.com, for developers);
//! the hot cues' `Red`, `Green` and `Blue`, which it does not describe, are as rekordbox
//! writes them.

mod write;

pub use write::write;

use crate::{join_relative, Import, ImportedCrate, ImportedCue, ImportedTrack, FOLDER_SEPARATOR};
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use prisme_grid::{Anchor, BeatGrid};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf, MAIN_SEPARATOR};

#[derive(Debug, thiserror::Error)]
pub enum RekordboxError {
    #[error("rekordbox XML: {0}")]
    Io(#[from] std::io::Error),
    #[error("rekordbox XML: {0}")]
    Xml(#[from] quick_xml::Error),
}

/// Ratings are stored as 0, 51, 102, 153, 204 or 255.
const RATING_STEP: f64 = 51.0;

/// rekordbox's eight track colours (rose, red, orange, lemon, green, turquoise, blue,
/// violet), and Prisme's in the same order (frontend/src/app/shared/track-colors.ts).
const REKORDBOX_COLOURS: [u32; 8] =
    [0xFF007F, 0xFF0000, 0xFFA500, 0xFFFF00, 0x00FF00, 0x25FDE9, 0x0000FF, 0x660099];
const PRISME_COLOURS: [u32; 8] =
    [0xE86BD6, 0xFF5442, 0xFF9A3C, 0xF5D547, 0xA6E25C, 0x3FD3B5, 0x4AA8FF, 0x8F7BFF];

/// The place in both palettes of the colour of either nearest to `colour`.
fn track_colour_index(colour: u32) -> usize {
    let channels = |c: u32| [c >> 16 & 0xff, c >> 8 & 0xff, c & 0xff].map(|v| v as i32);
    let distance =
        |c: u32| channels(c).iter().zip(channels(colour)).map(|(a, b)| (a - b) * (a - b)).sum::<i32>();
    let nearest =
        REKORDBOX_COLOURS.iter().chain(&PRISME_COLOURS).enumerate().min_by_key(|(_, c)| distance(**c));
    nearest.map_or(0, |(i, _)| i % REKORDBOX_COLOURS.len())
}

pub fn read(mut xml: impl Read) -> Result<Import, RekordboxError> {
    let mut text = String::new();
    xml.read_to_string(&mut text)?;
    parse(&text)
}

pub fn parse(xml: &str) -> Result<Import, RekordboxError> {
    let mut reader = Reader::from_str(xml);
    let mut ids: HashMap<String, PathBuf> = HashMap::new();
    let mut tracks: Vec<ImportedTrack> = Vec::new();
    // the tempos of the collection's track being read: where each starts, its BPM, its beat of
    // the bar
    let mut tempos: Vec<(f64, f64, f64)> = Vec::new();
    // whether the collection's track being read is imported: the marks of one that is not
    // (a streaming service's) are not the previous one's
    let mut kept = false;
    let mut folders: Vec<String> = Vec::new();
    let mut playlist: Option<(ImportedCrate, bool)> = None;
    let mut crates = Vec::new();
    let mut in_playlists = false;
    loop {
        let event = reader.read_event()?;
        let (element, empty) = match &event {
            Event::Start(e) => (e, false),
            Event::Empty(e) => (e, true),
            Event::End(e) => {
                match e.name().as_ref() {
                    b"PLAYLISTS" => in_playlists = false,
                    b"NODE" if playlist.is_some() => crates.extend(playlist.take().map(|(c, _)| c)),
                    b"NODE" => {
                        folders.pop();
                    }
                    _ => {}
                }
                continue;
            }
            Event::Eof => break,
            _ => continue,
        };
        let a = attributes(element);
        match element.name().as_ref() {
            b"PLAYLISTS" => in_playlists = true,
            b"TRACK" if !in_playlists => {
                tempos.clear();
                kept = false;
                let Some(path) = a.get("Location").and_then(|l| location_path(l)) else { continue };
                if let Some(id) = a.get("TrackID") {
                    ids.insert(id.clone(), path.clone());
                }
                kept = true;
                tracks.push(ImportedTrack {
                    path,
                    cues: Vec::new(),
                    grid: None,
                    color: a
                        .get("Colour")
                        .and_then(|c| u32::from_str_radix(c.trim_start_matches("0x"), 16).ok())
                        .map(|c| PRISME_COLOURS[track_colour_index(c)]),
                    rating: number(&a, "Rating").map(|r| (r / RATING_STEP).round() as u8).filter(|r| *r > 0),
                });
            }
            b"TEMPO" => {
                if let (Some(track), Some(bpm), Some(start_s)) =
                    (tracks.last_mut().filter(|_| kept), number(&a, "Bpm"), number(&a, "Inizio"))
                {
                    let beat_of_bar =
                        number(&a, "Battito").filter(|b| (1.0..=4.0).contains(b)).unwrap_or(1.0);
                    tempos.push((start_s, bpm, beat_of_bar));
                    track.grid = grid_of(&tempos);
                }
            }
            b"POSITION_MARK" => {
                if let (Some(track), Some(cue)) = (tracks.last_mut().filter(|_| kept), position_mark(&a)) {
                    track.cues.push(cue);
                }
            }
            b"NODE" if in_playlists => {
                let name = a.get("Name").cloned().unwrap_or_default();
                if a.get("Type").map(String::as_str) == Some("1") {
                    let by_location = a.get("KeyType").map(String::as_str) == Some("1");
                    let full = folders
                        .iter()
                        .skip(1)
                        .chain([&name])
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(FOLDER_SEPARATOR);
                    let crate_ = ImportedCrate { name: full, paths: Vec::new() };
                    if empty {
                        crates.push(crate_);
                    } else {
                        playlist = Some((crate_, by_location));
                    }
                } else if !empty {
                    folders.push(name);
                }
            }
            b"TRACK" => {
                if let (Some((crate_, by_location)), Some(key)) = (playlist.as_mut(), a.get("Key")) {
                    let path = if *by_location { location_path(key) } else { ids.get(key).cloned() };
                    crate_.paths.extend(path);
                }
            }
            _ => {}
        }
    }
    Ok(Import { tracks, crates })
}

/// The grid of a track's tempos: where each starts, its BPM and its beat of the bar
/// (`Battito`, 1 to 4), the first in the bar that makes bar 1. Several pin a grid whose tempo
/// changes (a track played live), each tempo holding the beats to the next.
fn grid_of(tempos: &[(f64, f64, f64)]) -> Option<BeatGrid> {
    let &[(start_s, bpm, beat_of_bar), ..] = tempos else { return None };
    if tempos.len() == 1 {
        return (bpm > 0.0).then(|| BeatGrid::steady(bpm, start_s - (beat_of_bar - 1.0) * 60.0 / bpm, true));
    }
    let mut beat = beat_of_bar - 1.0;
    let mut anchors = vec![Anchor { beat, time_s: start_s }];
    for pair in tempos.windows(2) {
        let ((from_s, bpm, _), (to_s, _, _)) = (pair[0], pair[1]);
        beat += ((to_s - from_s) * bpm / 60.0).round();
        anchors.push(Anchor { beat, time_s: to_s });
    }
    BeatGrid::anchored(anchors, tempos[tempos.len() - 1].1, true)
}

/// `Type` 0 is a cue, 4 a loop (with an `End`); `Num` is the hot cue, -1 for a memory cue.
fn position_mark(a: &HashMap<String, String>) -> Option<ImportedCue> {
    let start = number(a, "Start")?;
    let loop_length_s = match a.get("Type").map(String::as_str) {
        Some("4") => Some(number(a, "End")? - start).filter(|l| *l > 0.0),
        _ => None,
    };
    let channel = |name| number(a, name).map_or(0, |v| v as u32 & 0xff);
    Some(ImportedCue {
        hot: number(a, "Num").filter(|n| *n >= 0.0).map(|n| n as u8),
        position_s: start,
        loop_length_s,
        color: channel("Red") << 16 | channel("Green") << 8 | channel("Blue"),
        label: a.get("Name").cloned().unwrap_or_default(),
    })
}

/// `file://localhost/C:/Music/a%20b.mp3` -> `C:\Music\a b.mp3` (rekordbox on Windows);
/// `file://localhost/Users/me/Music/a%20b.mp3` -> `/Users/me/Music/a b.mp3` (on macOS).
fn location_path(location: &str) -> Option<PathBuf> {
    let rest = location.strip_prefix("file://localhost/").or_else(|| location.strip_prefix("file:///"))?;
    let decoded = percent_encoding::percent_decode_str(rest).decode_utf8().ok()?;
    let on_a_drive = decoded.as_bytes().get(1) == Some(&b':');
    let (root, path) = match decoded.split_once('/') {
        Some((drive, path)) if on_a_drive => (format!("{drive}{MAIN_SEPARATOR}"), path),
        _ => (MAIN_SEPARATOR.to_string(), &*decoded),
    };
    Some(join_relative(Path::new(&root), path))
}

/// What a `Location` may hold as it is, as pyrekordbox writes it for rekordbox: letters,
/// digits, `-._~` and the separators `/` and `:`; the rest is percent-encoded (UTF-8).
const LOCATION: &AsciiSet =
    &NON_ALPHANUMERIC.remove(b'-').remove(b'.').remove(b'_').remove(b'~').remove(b'/').remove(b':');

/// The `Location` of a file: the reverse of [`location_path`].
fn location(path: &Path) -> String {
    let text = path.to_string_lossy();
    let text = if MAIN_SEPARATOR == '\\' { text.replace('\\', "/") } else { text.into_owned() };
    format!("file://localhost/{}", utf8_percent_encode(text.trim_start_matches('/'), LOCATION))
}

fn attributes(element: &BytesStart<'_>) -> HashMap<String, String> {
    element
        .attributes()
        .flatten()
        .filter_map(|a| {
            let key = String::from_utf8(a.key.as_ref().to_vec()).ok()?;
            Some((key, a.unescape_value().ok()?.into_owned()))
        })
        .collect()
}

fn number(a: &HashMap<String, String>, name: &str) -> Option<f64> {
    a.get(name)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Where the library of the tests is: on a drive on Windows, at the root elsewhere.
    const ROOT: &str = if cfg!(windows) { "C:/" } else { "" };

    /// The export of that library.
    fn xml() -> String {
        XML.replace("{ROOT}", ROOT)
    }

    /// A file of that library.
    fn file(path: &str) -> PathBuf {
        join_relative(Path::new(if cfg!(windows) { r"C:\" } else { "/" }), path)
    }

    const XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<DJ_PLAYLISTS Version="1.0.0">
  <PRODUCT Name="rekordbox" Version="6.8.5" Company="AlphaTheta"/>
  <COLLECTION Entries="2">
    <TRACK TrackID="11" Name="Strobe" Artist="deadmau5" Rating="204" Colour="0xFF007F"
           Location="file://localhost/{ROOT}Users/Me/Music/Strobe%20(Club).mp3">
      <TEMPO Inizio="0.050" Bpm="128.00" Metro="4/4" Battito="1"/>
      <POSITION_MARK Name="drop" Type="0" Start="64.000" Num="0" Red="40" Green="226" Blue="20"/>
      <POSITION_MARK Name="" Type="0" Start="1.500" Num="-1"/>
      <POSITION_MARK Name="build" Type="4" Start="32.000" End="40.000" Num="2" Red="255" Green="140" Blue="0"/>
    </TRACK>
    <TRACK TrackID="12" Name="Live" Location="file://localhost/{ROOT}Sets/live.flac" Rating="0">
      <TEMPO Inizio="0.1" Bpm="120.0" Metro="4/4" Battito="1"/>
      <TEMPO Inizio="30.1" Bpm="121.5" Metro="4/4" Battito="1"/>
    </TRACK>
  </COLLECTION>
  <PLAYLISTS>
    <NODE Type="0" Name="ROOT" Count="2">
      <NODE Type="0" Name="Festivals" Count="1">
        <NODE Name="Main stage" Type="1" KeyType="0" Entries="2">
          <TRACK Key="12"/>
          <TRACK Key="11"/>
        </NODE>
      </NODE>
      <NODE Name="Empty" Type="1" KeyType="0" Entries="0"/>
    </NODE>
  </PLAYLISTS>
</DJ_PLAYLISTS>"#;

    #[test]
    fn tracks_cues_grids_and_marks() {
        let import = parse(&xml()).unwrap();
        let strobe = &import.tracks[0];
        assert_eq!(strobe.path, file("Users/Me/Music/Strobe (Club).mp3"));
        assert_eq!(strobe.grid, Some(BeatGrid::steady(128.0, 0.05, true)));
        assert_eq!((strobe.color, strobe.rating), (Some(0xe86bd6), Some(4)), "rose is Prisme's pink");
        assert_eq!(
            strobe.cues,
            [
                ImportedCue {
                    hot: Some(0),
                    position_s: 64.0,
                    loop_length_s: None,
                    color: 0x28e214,
                    label: "drop".into()
                },
                ImportedCue {
                    hot: None,
                    position_s: 1.5,
                    loop_length_s: None,
                    color: 0,
                    label: String::new()
                },
                ImportedCue {
                    hot: Some(2),
                    position_s: 32.0,
                    loop_length_s: Some(8.0),
                    color: 0xff8c00,
                    label: "build".into()
                },
            ]
        );
        let live = &import.tracks[1];
        assert_eq!(live.rating, None);
        let live = live.grid.as_ref().expect("two tempos: a grid whose tempo changes");
        assert_eq!(live.anchors().len(), 2);
        assert!((live.bpm_at(10.0) - 120.0).abs() < 0.01 && (live.bpm_at(40.0) - 121.5).abs() < 1e-9);
        assert!((live.first_beat_s() - 0.1).abs() < 1e-9);
    }

    #[test]
    fn the_marks_of_a_track_not_imported_are_not_the_previous_ones() {
        let xml = r#"<DJ_PLAYLISTS Version="1.0.0"><COLLECTION Entries="2">
            <TRACK TrackID="1" Location="file://localhost/{ROOT}Music/local.mp3">
              <TEMPO Inizio="0.1" Bpm="124.0" Metro="4/4" Battito="1"/>
              <POSITION_MARK Name="" Type="0" Start="8.0" Num="0"/>
            </TRACK>
            <TRACK TrackID="2" Location="file://localhostsoundcloud:tracks:123">
              <TEMPO Inizio="0.3" Bpm="126.0" Metro="4/4" Battito="1"/>
              <POSITION_MARK Name="" Type="0" Start="90.0" Num="2"/>
            </TRACK>
        </COLLECTION></DJ_PLAYLISTS>"#;
        let import = parse(&xml.replace("{ROOT}", ROOT)).unwrap();
        let [local] = &import.tracks[..] else { panic!("{:?}", import.tracks) };
        assert_eq!(local.grid, Some(BeatGrid::steady(124.0, 0.1, true)));
        assert_eq!(local.cues.iter().map(|c| c.position_s).collect::<Vec<_>>(), [8.0]);
    }

    #[test]
    fn a_location_is_read_back_as_the_file() {
        let path = file("Users/Me/Musique/Café & Co/#1 (50%) + 1.mp3");
        let encoded = location(&path);
        assert_eq!(
            encoded,
            format!("file://localhost/{ROOT}Users/Me/Musique/Caf%C3%A9%20%26%20Co/%231%20%2850%25%29%20%2B%201.mp3")
        );
        assert_eq!(location_path(&encoded), Some(path));
    }

    #[test]
    fn track_colours_go_to_the_same_place_in_the_other_palette() {
        for (prisme, rekordbox) in PRISME_COLOURS.iter().zip(REKORDBOX_COLOURS) {
            assert_eq!(REKORDBOX_COLOURS[track_colour_index(*prisme)], rekordbox);
            assert_eq!(PRISME_COLOURS[track_colour_index(rekordbox)], *prisme);
        }
        assert_eq!(REKORDBOX_COLOURS[track_colour_index(0xEE1111)], 0xFF0000, "the nearest");
    }

    #[test]
    fn playlists_become_crates_named_after_their_folders() {
        let import = parse(&xml()).unwrap();
        let names: Vec<_> = import.crates.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Festivals / Main stage", "Empty"]);
        assert_eq!(
            import.crates[0].paths,
            [file("Sets/live.flac"), file("Users/Me/Music/Strobe (Club).mp3")]
        );
    }
}
