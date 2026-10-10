//! rekordbox's XML export (File > Export Collection in xml format): tracks with their
//! hot and memory cues, loops, beat grid, colour and rating, and the playlists.

use crate::{join_relative, Import, ImportedCrate, ImportedCue, ImportedGrid, ImportedTrack};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum RekordboxError {
    #[error("rekordbox XML: {0}")]
    Io(#[from] std::io::Error),
    #[error("rekordbox XML: {0}")]
    Xml(#[from] quick_xml::Error),
}

/// Ratings are stored as 0, 51, 102, 153, 204 or 255.
const RATING_STEP: f64 = 51.0;

pub fn read(path: &Path) -> Result<Import, RekordboxError> {
    parse(&std::fs::read_to_string(path)?)
}

pub fn parse(xml: &str) -> Result<Import, RekordboxError> {
    let mut reader = Reader::from_str(xml);
    let mut ids: HashMap<String, PathBuf> = HashMap::new();
    let mut tracks: Vec<ImportedTrack> = Vec::new();
    let mut tempos = 0;
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
                let Some(path) = a.get("Location").and_then(|l| location_path(l)) else { continue };
                if let Some(id) = a.get("TrackID") {
                    ids.insert(id.clone(), path.clone());
                }
                tempos = 0;
                tracks.push(ImportedTrack {
                    path,
                    cues: Vec::new(),
                    grid: None,
                    color: a
                        .get("Colour")
                        .and_then(|c| u32::from_str_radix(c.trim_start_matches("0x"), 16).ok()),
                    rating: number(&a, "Rating").map(|r| (r / RATING_STEP).round() as u8).filter(|r| *r > 0),
                });
            }
            b"TEMPO" => {
                tempos += 1;
                if let (Some(track), Some(bpm), Some(beat_s)) =
                    (tracks.last_mut(), number(&a, "Bpm"), number(&a, "Inizio"))
                {
                    // a grid that changes tempo is not a single grid
                    track.grid = (tempos == 1).then_some(ImportedGrid { bpm, beat_s });
                }
            }
            b"POSITION_MARK" => {
                if let (Some(track), Some(cue)) = (tracks.last_mut(), position_mark(&a)) {
                    track.cues.push(cue);
                }
            }
            b"NODE" if in_playlists => {
                let name = a.get("Name").cloned().unwrap_or_default();
                if a.get("Type").map(String::as_str) == Some("1") {
                    let by_location = a.get("KeyType").map(String::as_str) == Some("1");
                    let full = folders.iter().skip(1).chain([&name]).cloned().collect::<Vec<_>>().join(" / ");
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

/// `file://localhost/C:/Music/a%20b.mp3` -> `C:\Music\a b.mp3`
fn location_path(location: &str) -> Option<PathBuf> {
    let rest = location.strip_prefix("file://localhost/").or_else(|| location.strip_prefix("file:///"))?;
    let decoded = percent_encoding::percent_decode_str(rest).decode_utf8().ok()?;
    let (drive, path) = decoded.split_once('/')?;
    Some(join_relative(&PathBuf::from(format!("{drive}\\")), path))
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

    const XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<DJ_PLAYLISTS Version="1.0.0">
  <PRODUCT Name="rekordbox" Version="6.8.5" Company="AlphaTheta"/>
  <COLLECTION Entries="2">
    <TRACK TrackID="11" Name="Strobe" Artist="deadmau5" Rating="204" Colour="0xFF007F"
           Location="file://localhost/C:/Users/Me/Music/Strobe%20(Club).mp3">
      <TEMPO Inizio="0.050" Bpm="128.00" Metro="4/4" Battito="1"/>
      <POSITION_MARK Name="drop" Type="0" Start="64.000" Num="0" Red="40" Green="226" Blue="20"/>
      <POSITION_MARK Name="" Type="0" Start="1.500" Num="-1"/>
      <POSITION_MARK Name="build" Type="4" Start="32.000" End="40.000" Num="2" Red="255" Green="140" Blue="0"/>
    </TRACK>
    <TRACK TrackID="12" Name="Live" Location="file://localhost/D:/Sets/live.flac" Rating="0">
      <TEMPO Inizio="0.1" Bpm="120.0" Metro="4/4" Battito="1"/>
      <TEMPO Inizio="30.0" Bpm="121.5" Metro="4/4" Battito="1"/>
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
        let import = parse(XML).unwrap();
        let strobe = &import.tracks[0];
        assert_eq!(strobe.path, PathBuf::from(r"C:\Users\Me\Music\Strobe (Club).mp3"));
        assert_eq!(strobe.grid, Some(ImportedGrid { bpm: 128.0, beat_s: 0.05 }));
        assert_eq!((strobe.color, strobe.rating), (Some(0xff007f), Some(4)));
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
        assert_eq!((live.grid, live.rating), (None, None), "two tempos: no single grid");
    }

    #[test]
    fn playlists_become_crates_named_after_their_folders() {
        let import = parse(XML).unwrap();
        let names: Vec<_> = import.crates.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Festivals / Main stage", "Empty"]);
        assert_eq!(
            import.crates[0].paths,
            [PathBuf::from(r"D:\Sets\live.flac"), PathBuf::from(r"C:\Users\Me\Music\Strobe (Club).mp3")]
        );
    }
}
