//! A list as an extended M3U playlist in UTF-8 (`.m3u8`), which almost any player and DJ
//! software opens: absolute paths, each after its length and its artist and title.

use prisme_library::TrackEntry;
use std::path::Path;

pub fn write(entries: &[TrackEntry]) -> String {
    let mut m3u = String::from("#EXTM3U\n");
    for entry in entries {
        let record = entry.record.as_ref();
        let seconds = record.and_then(|r| r.duration_s).map_or(-1, |d| d.round() as i64);
        let tags = record.map(|r| &r.tags);
        let title = tags.and_then(|t| t.title.as_deref()).filter(|t| !t.is_empty()).map_or_else(
            || {
                Path::new(&entry.path)
                    .file_stem()
                    .map_or_else(|| entry.path.clone(), |s| s.to_string_lossy().into())
            },
            str::to_owned,
        );
        let name = match tags.and_then(|t| t.artist.as_deref()).filter(|a| !a.is_empty()) {
            Some(artist) => format!("{artist} - {title}"),
            None => title,
        };
        m3u.push_str(&format!("#EXTINF:{seconds},{}\n{}\n", one_line(&name), entry.path));
    }
    m3u
}

/// A tag on a single line, as each line of the file is one thing.
fn one_line(text: &str) -> String {
    text.split(['\r', '\n']).filter(|part| !part.is_empty()).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use prisme_library::{FileStamp, TrackRecord};
    use prisme_media::TrackTags;

    #[test]
    fn each_track_after_its_length_artist_and_title() {
        let tags = TrackTags {
            title: Some("Strobe\n(Club Edit)".into()),
            artist: Some("deadmau5".into()),
            ..TrackTags::default()
        };
        let analysis = prisme_analysis::TrackAnalysis::default();
        let path = "C:\\Music\\Café\\strobe.flac";
        let stamp = FileStamp { size: 1, modified_ms: 1 };
        let record = TrackRecord::analyzed(Path::new(path), stamp, 634.6, analysis, tags);
        let entries = [
            TrackEntry { path: path.into(), record: Some(record) },
            TrackEntry { path: "/Users/me/Music/untagged.mp3".into(), record: None },
        ];
        assert_eq!(
            write(&entries),
            "#EXTM3U\n\
             #EXTINF:635,deadmau5 - Strobe (Club Edit)\nC:\\Music\\Café\\strobe.flac\n\
             #EXTINF:-1,untagged\n/Users/me/Music/untagged.mp3\n"
        );
    }
}
