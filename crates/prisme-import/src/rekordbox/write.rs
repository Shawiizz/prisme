//! The XML rekordbox reads: the collection, each track with its tags, beat grid and cues,
//! then the lists as playlists in folders.

use super::{location, track_colour_index, RATING_STEP, REKORDBOX_COLOURS};
use crate::export::{Export, ExportedList, ExportedTrack};
use prisme_grid::{BeatGrid, BEATS_PER_BAR};
use prisme_library::{Cue, CueSlot};
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, Event};
use quick_xml::Writer;
use std::collections::HashMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// The hot cues of rekordbox and of the CDJs (A to H): Prisme's others go as memory cues.
const HOT_CUES: u8 = 8;

type Attributes = Vec<(&'static str, String)>;

pub fn write(export: &Export, out: impl Write) -> io::Result<()> {
    let mut xml = Writer::new_with_indent(out, b' ', 2);
    xml.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;
    xml.write_event(Event::Start(element("DJ_PLAYLISTS", vec![("Version", "1.0.0".into())])))?;
    let product = vec![
        ("Name", "Prisme".into()),
        ("Version", env!("CARGO_PKG_VERSION").into()),
        ("Company", "Prisme".into()),
    ];
    xml.write_event(Event::Empty(element("PRODUCT", product)))?;
    xml.write_event(Event::Start(element("COLLECTION", vec![("Entries", export.tracks.len().to_string())])))?;
    let mut ids = HashMap::new();
    for (i, track) in export.tracks.iter().enumerate() {
        ids.insert(track.path.as_path(), i + 1);
        write_track(&mut xml, i + 1, track)?;
    }
    xml.write_event(Event::End(BytesEnd::new("COLLECTION")))?;
    xml.write_event(Event::Start(BytesStart::new("PLAYLISTS")))?;
    let lists: Vec<&ExportedList> = export.lists.iter().filter(|l| !l.path.is_empty()).collect();
    write_folder(&mut xml, "ROOT", &lists, 0, &ids)?;
    xml.write_event(Event::End(BytesEnd::new("PLAYLISTS")))?;
    xml.write_event(Event::End(BytesEnd::new("DJ_PLAYLISTS")))
}

fn write_track<W: Write>(xml: &mut Writer<W>, id: usize, track: &ExportedTrack) -> io::Result<()> {
    let record = track.record.as_ref();
    let tags = record.map(|r| &r.tags);
    let analysis = record.map(|r| r.effective_analysis());
    let tempo = analysis.and_then(|a| a.tempo);
    let grid = record.and_then(|r| r.beat_grid());
    let tag = |value: Option<&String>| value.map(|v| text(v)).filter(|v| !v.is_empty());
    let title = tag(tags.and_then(|t| t.title.as_ref()))
        .or_else(|| track.path.file_stem().map(|s| text(&s.to_string_lossy())))
        .unwrap_or_default();
    let attributes = [
        ("TrackID", Some(id.to_string())),
        ("Name", Some(title)),
        ("Artist", tag(tags.and_then(|t| t.artist.as_ref()))),
        ("Album", tag(tags.and_then(|t| t.album.as_ref()))),
        ("Genre", tag(tags.and_then(|t| t.genre.as_ref()))),
        ("Kind", kind(&track.path).map(str::to_owned)),
        ("Size", record.map(|r| r.version.stamp.size.to_string())),
        ("TotalTime", record.and_then(|r| r.duration_s).map(|d| (d.round() as u64).to_string())),
        ("Year", tags.and_then(|t| t.year).map(|y| y.to_string())),
        ("AverageBpm", tempo.map(|t| format!("{:.2}", t.bpm))),
        ("Comments", tag(tags.and_then(|t| t.comment.as_ref()))),
        ("PlayCount", record.map(|r| r.play_count.to_string())),
        ("Rating", record.map(|r| ((f64::from(r.marks.rating) * RATING_STEP) as u32).to_string())),
        ("Location", Some(location(&track.path))),
        ("Tonality", analysis.and_then(|a| a.key).map(|k| k.key.name().to_owned())),
        (
            "Colour",
            record
                .and_then(|r| r.marks.color)
                .map(|c| format!("0x{:06X}", REKORDBOX_COLOURS[track_colour_index(c)])),
        ),
    ];
    let start =
        element("TRACK", attributes.into_iter().filter_map(|(name, value)| Some((name, value?))).collect());
    let marks = position_marks(&track.cues);
    if grid.is_none() && marks.is_empty() {
        return xml.write_event(Event::Empty(start));
    }
    xml.write_event(Event::Start(start))?;
    for (start_s, bpm, beat) in grid.as_ref().map(tempos).unwrap_or_default() {
        let tempo = vec![
            ("Inizio", seconds(start_s)),
            ("Bpm", format!("{bpm:.2}")),
            ("Metro", "4/4".into()),
            ("Battito", format!("{}", beat.rem_euclid(f64::from(BEATS_PER_BAR)) as u32 + 1)),
        ];
        xml.write_event(Event::Empty(element("TEMPO", tempo)))?;
    }
    for mark in marks {
        xml.write_event(Event::Empty(element("POSITION_MARK", mark)))?;
    }
    xml.write_event(Event::End(BytesEnd::new("TRACK")))
}

/// A grid as rekordbox's tempos: where each starts, its BPM and the beat it starts on (counted
/// from bar 1's first). One from bar 1 for a single tempo; one from each anchor of a grid whose
/// tempo changes, holding to the next.
fn tempos(grid: &BeatGrid) -> Vec<(f64, f64, f64)> {
    match grid.anchors() {
        [] => vec![(grid.first_beat_s(), grid.bpm(), 0.0)],
        anchors => anchors.iter().map(|a| (a.time_s, grid.bpm_at(a.time_s), a.beat)).collect(),
    }
}

/// The main cue first (an import takes the first memory cue for it), then the hot cues.
fn position_marks(cues: &[Cue]) -> Vec<Attributes> {
    let main = cues.iter().filter(|c| c.slot == CueSlot::Main);
    let hot = cues.iter().filter(|c| c.slot != CueSlot::Main);
    main.chain(hot).map(position_mark).collect()
}

/// `Type` 0 for a cue, 4 for a loop; `Num` the hot cue, -1 for a memory cue.
fn position_mark(cue: &Cue) -> Attributes {
    let hot = match cue.slot {
        CueSlot::Hot(n) if n < HOT_CUES => Some(n),
        _ => None,
    };
    let mut mark = vec![
        ("Name", text(&cue.label)),
        ("Type", if cue.loop_length_s.is_some() { "4" } else { "0" }.into()),
        ("Start", seconds(cue.position_s)),
    ];
    if let Some(length) = cue.loop_length_s {
        mark.push(("End", seconds(cue.position_s + length)));
    }
    mark.push(("Num", hot.map_or(-1, i32::from).to_string()));
    if hot.is_some() {
        let channel = |shift: u32| (cue.color >> shift & 0xff).to_string();
        mark.extend([("Red", channel(16)), ("Green", channel(8)), ("Blue", channel(0))]);
    }
    mark
}

/// What a folder holds: playlists, and folders named by the next part of the lists' paths.
enum Node<'a> {
    Playlist(&'a ExportedList),
    Folder(&'a str, Vec<&'a ExportedList>),
}

/// The nodes of the lists at `depth` in the tree, in the order the lists come.
fn nodes<'a>(lists: &[&'a ExportedList], depth: usize) -> Vec<Node<'a>> {
    let mut nodes: Vec<Node<'a>> = Vec::new();
    for list in lists {
        let name = list.path[depth].as_str();
        if list.path.len() == depth + 1 {
            nodes.push(Node::Playlist(list));
            continue;
        }
        if nodes.iter().any(|n| matches!(n, Node::Folder(f, _) if *f == name)) {
            continue;
        }
        let inside = lists.iter().copied().filter(|l| l.path.len() > depth + 1 && l.path[depth] == name);
        nodes.push(Node::Folder(name, inside.collect()));
    }
    nodes
}

/// `lists`: those inside the folder, their paths at least `depth + 1` long.
fn write_folder<W: Write>(
    xml: &mut Writer<W>,
    name: &str,
    lists: &[&ExportedList],
    depth: usize,
    ids: &HashMap<&Path, usize>,
) -> io::Result<()> {
    let nodes = nodes(lists, depth);
    let folder =
        element("NODE", vec![("Type", "0".into()), ("Name", text(name)), ("Count", nodes.len().to_string())]);
    if nodes.is_empty() {
        return xml.write_event(Event::Empty(folder));
    }
    xml.write_event(Event::Start(folder))?;
    for node in nodes {
        match node {
            Node::Playlist(list) => write_playlist(xml, list.path[depth].as_str(), &list.tracks, ids)?,
            Node::Folder(name, inside) => write_folder(xml, name, &inside, depth + 1, ids)?,
        }
    }
    xml.write_event(Event::End(BytesEnd::new("NODE")))
}

/// Its tracks by `TrackID` (`KeyType` 0); those not in the collection are left out.
fn write_playlist<W: Write>(
    xml: &mut Writer<W>,
    name: &str,
    tracks: &[PathBuf],
    ids: &HashMap<&Path, usize>,
) -> io::Result<()> {
    let keys: Vec<usize> = tracks.iter().filter_map(|t| ids.get(t.as_path()).copied()).collect();
    let playlist = element(
        "NODE",
        vec![
            ("Name", text(name)),
            ("Type", "1".into()),
            ("KeyType", "0".into()),
            ("Entries", keys.len().to_string()),
        ],
    );
    if keys.is_empty() {
        return xml.write_event(Event::Empty(playlist));
    }
    xml.write_event(Event::Start(playlist))?;
    for key in keys {
        xml.write_event(Event::Empty(element("TRACK", vec![("Key", key.to_string())])))?;
    }
    xml.write_event(Event::End(BytesEnd::new("NODE")))
}

fn element(name: &'static str, attributes: Attributes) -> BytesStart<'static> {
    let mut element = BytesStart::new(name);
    for (key, value) in &attributes {
        element.push_attribute((*key, value.as_str()));
    }
    element
}

/// To the millisecond, as rekordbox writes positions.
fn seconds(s: f64) -> String {
    format!("{s:.3}")
}

/// `text` without the control characters XML cannot hold.
fn text(text: &str) -> String {
    text.chars().filter(|c| !c.is_control() || matches!(c, '\t' | '\n' | '\r')).collect()
}

/// The `Kind` rekordbox gives the files it plays.
fn kind(path: &Path) -> Option<&'static str> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match extension.as_str() {
        "mp3" => "MP3 File",
        "m4a" | "aac" | "mp4" => "M4A File",
        "wav" => "WAV File",
        "aif" | "aiff" => "AIFF File",
        "flac" => "FLAC File",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::list_path;
    use crate::{apply, join_relative, ImportSummary};
    use prisme_analysis::{Key, KeyEstimate, Mode, Tempo, TrackAnalysis};
    use prisme_grid::Anchor;
    use prisme_library::{FileStamp, GridEdit, LibraryDb, TrackEdits, TrackEntry, TrackMarks, TrackRecord};
    use prisme_media::TrackTags;

    /// Where the files of the tests are: on a drive on Windows, at the root elsewhere.
    const ROOT: &str = if cfg!(windows) { "C:/" } else { "" };

    fn file(path: &str) -> PathBuf {
        join_relative(Path::new(if cfg!(windows) { r"C:\" } else { "/" }), path)
    }

    fn record(path: &Path) -> TrackRecord {
        let analysis = TrackAnalysis {
            tempo: Some(Tempo { bpm: 124.996, first_beat_s: 0.05, confidence: 0.9, bars_sure: true }),
            key: Some(KeyEstimate { key: Key::new(9, Mode::Minor), confidence: 0.8, tuning_cents: 0.0 }),
        };
        let tags = TrackTags {
            title: Some("Strobe".into()),
            artist: Some("deadmau5 & co".into()),
            year: Some(2009),
            comment: Some("8A\u{1}".into()),
            ..TrackTags::default()
        };
        let stamp = FileStamp { size: 12_345, modified_ms: 1 };
        let mut record = TrackRecord::analyzed(path, stamp, 634.6, analysis, tags);
        record.marks = TrackMarks { color: Some(0xFF5442), rating: 4 };
        record.play_count = 3;
        record
    }

    fn cues() -> Vec<Cue> {
        let mut drop = Cue::point(CueSlot::Hot(0), 64.0);
        drop.label = "drop".into();
        drop.color = 0x28E214;
        vec![
            Cue::point(CueSlot::Main, 1.5),
            drop,
            Cue::looped(CueSlot::Hot(2), 32.0, 8.0),
            Cue::point(CueSlot::Hot(9), 96.0),
        ]
    }

    #[test]
    fn the_collection_then_the_lists_in_folders() {
        let strobe = file("Music/Strobe.mp3");
        let other = file("Music/Other.wav");
        let export = Export {
            tracks: vec![
                ExportedTrack { path: strobe.clone(), record: Some(record(&strobe)), cues: cues() },
                ExportedTrack { path: other.clone(), record: None, cues: vec![] },
            ],
            lists: vec![
                ExportedList {
                    path: list_path(None, "Festivals / Main stage"),
                    tracks: vec![other.clone(), strobe],
                },
                ExportedList { path: list_path(Some("History"), "2026-11-14 22:00"), tracks: vec![other] },
                ExportedList {
                    path: list_path(None, "Festivals / Empty"),
                    tracks: vec![file("Music/gone.mp3")],
                },
            ],
        };
        let mut xml = Vec::new();
        write(&export, &mut xml).unwrap();
        let expected = EXPECTED.replace("{ROOT}", ROOT).replace("{VERSION}", env!("CARGO_PKG_VERSION"));
        assert_eq!(String::from_utf8(xml).unwrap(), expected);
    }

    const EXPECTED: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<DJ_PLAYLISTS Version="1.0.0">
  <PRODUCT Name="Prisme" Version="{VERSION}" Company="Prisme"/>
  <COLLECTION Entries="2">
    <TRACK TrackID="1" Name="Strobe" Artist="deadmau5 &amp; co" Kind="MP3 File" Size="12345" TotalTime="635" Year="2009" AverageBpm="125.00" Comments="8A" PlayCount="3" Rating="204" Location="file://localhost/{ROOT}Music/Strobe.mp3" Tonality="Am" Colour="0xFF0000">
      <TEMPO Inizio="0.050" Bpm="125.00" Metro="4/4" Battito="1"/>
      <POSITION_MARK Name="" Type="0" Start="1.500" Num="-1"/>
      <POSITION_MARK Name="drop" Type="0" Start="64.000" Num="0" Red="40" Green="226" Blue="20"/>
      <POSITION_MARK Name="" Type="4" Start="32.000" End="40.000" Num="2" Red="245" Green="213" Blue="71"/>
      <POSITION_MARK Name="" Type="0" Start="96.000" Num="-1"/>
    </TRACK>
    <TRACK TrackID="2" Name="Other" Kind="WAV File" Location="file://localhost/{ROOT}Music/Other.wav"/>
  </COLLECTION>
  <PLAYLISTS>
    <NODE Type="0" Name="ROOT" Count="2">
      <NODE Type="0" Name="Festivals" Count="2">
        <NODE Name="Main stage" Type="1" KeyType="0" Entries="2">
          <TRACK Key="2"/>
          <TRACK Key="1"/>
        </NODE>
        <NODE Name="Empty" Type="1" KeyType="0" Entries="0"/>
      </NODE>
      <NODE Type="0" Name="History" Count="1">
        <NODE Name="2026-11-14 22:00" Type="1" KeyType="0" Entries="1">
          <TRACK Key="2"/>
        </NODE>
      </NODE>
    </NODE>
  </PLAYLISTS>
</DJ_PLAYLISTS>"#;

    #[test]
    fn a_grid_whose_tempo_changes_goes_out_as_a_tempo_from_each_anchor() {
        let strobe = file("Music/Strobe.mp3");
        let library = LibraryDb::open_in_memory().unwrap();
        library.save(&record(&strobe)).unwrap();
        // bar 1 on beat 2 of the first anchor's bar: 120 BPM, then 96 from beat 9, 100 from 17
        let anchors = [(1.0, 1.0), (9.0, 5.0), (17.0, 10.0)].map(|(beat, time_s)| Anchor { beat, time_s });
        let grid = BeatGrid::anchored(anchors.to_vec(), 100.0, true).unwrap();
        let edits = TrackEdits { grid: Some(GridEdit::imported(grid.clone())), ..TrackEdits::default() };
        library.save_edits(&strobe, &edits).unwrap();
        let mut xml = Vec::new();
        write(&Export::gather(&library, true, Vec::new()).unwrap(), &mut xml).unwrap();
        let xml = String::from_utf8(xml).unwrap();
        for tempo in [
            r#"<TEMPO Inizio="1.000" Bpm="120.00" Metro="4/4" Battito="2"/>"#,
            r#"<TEMPO Inizio="5.000" Bpm="96.00" Metro="4/4" Battito="2"/>"#,
            r#"<TEMPO Inizio="10.000" Bpm="100.00" Metro="4/4" Battito="2"/>"#,
        ] {
            assert!(xml.contains(tempo), "{tempo} in {xml}");
        }
        let back = super::super::parse(&xml).unwrap().tracks.remove(0).grid.unwrap();
        assert_eq!(back.anchors(), grid.anchors());
        assert_eq!(back.bpm(), 100.0);
    }

    #[test]
    fn an_import_of_the_export_brings_back_the_grids_cues_marks_and_lists() {
        let base = std::env::temp_dir().join(format!("prisme-export-{}", std::process::id()));
        let dir = base.join("Café & Co");
        std::fs::create_dir_all(&dir).unwrap();
        let strobe = dir.join("#1 Strobe (50%).mp3");
        let other = dir.join("other.flac");
        for path in [&strobe, &other] {
            std::fs::write(path, b"x").unwrap();
        }
        let library = LibraryDb::open_in_memory().unwrap();
        library.save(&record(&strobe)).unwrap();
        library.set_color(&[&strobe], Some(0xFF5442)).unwrap();
        library.set_rating(&[&strobe], 4).unwrap();
        let mut cues = cues();
        cues.push(Cue::looped(CueSlot::Hot(12), 120.0, 4.0));
        for cue in &cues {
            library.save_cue(&strobe, cue).unwrap();
        }
        let entry = |path: &Path| TrackEntry { path: path.to_string_lossy().into_owned(), record: None };
        let lists = vec![
            (list_path(None, "Festivals / Main stage"), vec![entry(&other), entry(&strobe)]),
            (list_path(None, "Prepare"), vec![entry(&strobe)]),
            (list_path(Some("History"), "2026-11-14 22:00"), vec![entry(&strobe), entry(&other)]),
        ];
        let mut xml = Vec::new();
        write(&Export::gather(&library, true, lists).unwrap(), &mut xml).unwrap();

        let imported = LibraryDb::open_in_memory().unwrap();
        let import = super::super::parse(std::str::from_utf8(&xml).unwrap()).unwrap();
        let summary = apply(&imported, &import).unwrap();
        assert_eq!(summary, ImportSummary { tracks: 1, cues: 4, grids: 1, crates: 3, missing: 0 });
        assert_eq!(imported.marks(&strobe).unwrap(), TrackMarks { color: Some(0xFF5442), rating: 4 });
        let grid = imported.edits(&strobe).unwrap().grid.unwrap();
        assert_eq!(
            (grid.grid.bpm(), (grid.grid.first_beat_s() * 1000.0).round()),
            (125.0, 50.0),
            "to the hundredth of a BPM"
        );
        let back: Vec<_> = imported
            .cues(&strobe)
            .unwrap()
            .into_iter()
            .map(|c| (c.slot, c.position_s, c.loop_length_s, c.color, c.label))
            .collect();
        let default_colour = |n| Cue::point(CueSlot::Hot(n), 0.0).color;
        assert_eq!(
            back,
            [
                (CueSlot::Main, 1.5, None, Cue::point(CueSlot::Main, 0.0).color, String::new()),
                (CueSlot::Hot(0), 64.0, None, 0x28E214, "drop".into()),
                // past the eighth, a hot loop is a memory loop: it comes back in the first free
                // slot; a memory cue other than the first does not come back
                (CueSlot::Hot(1), 120.0, Some(4.0), default_colour(1), String::new()),
                (CueSlot::Hot(2), 32.0, Some(8.0), Cue::point(CueSlot::Hot(2), 0.0).color, String::new()),
            ]
        );
        let crates: Vec<_> = imported
            .crates()
            .unwrap()
            .into_iter()
            .map(|c| {
                let tracks = imported.crate_tracks(c.id).unwrap().into_iter().map(|e| PathBuf::from(e.path));
                (c.name, tracks.collect::<Vec<_>>())
            })
            .collect();
        assert_eq!(
            crates,
            [
                ("Festivals / Main stage".to_string(), vec![other.clone(), strobe.clone()]),
                ("History / 2026-11-14 22:00".to_string(), vec![strobe.clone(), other.clone()]),
                ("Prepare".to_string(), vec![strobe.clone()]),
            ]
        );
        std::fs::remove_dir_all(base).ok();
    }
}
