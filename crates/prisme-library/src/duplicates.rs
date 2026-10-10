//! Duplicates: tracks that are the same song, by their artist and title tags (or file
//! name when they have none), ignoring case, accents aside, spaces and punctuation.

use crate::db::TrackRecord;
use std::collections::BTreeMap;
use std::path::Path;

/// Groups of two or more tracks that look like the same song, in a stable order.
pub fn duplicate_groups(records: Vec<TrackRecord>) -> Vec<Vec<TrackRecord>> {
    let mut groups: BTreeMap<String, Vec<TrackRecord>> = BTreeMap::new();
    for record in records {
        if let Some(key) = song_key(&record) {
            groups.entry(key).or_default().push(record);
        }
    }
    groups.into_values().filter(|g| g.len() > 1).collect()
}

fn song_key(record: &TrackRecord) -> Option<String> {
    let tags = &record.tags;
    let text = match (&tags.artist, &tags.title) {
        (Some(artist), Some(title)) => format!("{artist} {title}"),
        _ => Path::new(&record.path).file_stem()?.to_string_lossy().into_owned(),
    };
    let key: String = text.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect();
    (!key.is_empty()).then_some(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::FileStamp;
    use prisme_analysis::TrackAnalysis;
    use prisme_media::TrackTags;

    fn record(path: &str, artist: Option<&str>, title: Option<&str>) -> TrackRecord {
        let tags = TrackTags {
            artist: artist.map(Into::into),
            title: title.map(Into::into),
            ..TrackTags::default()
        };
        let analysis = TrackAnalysis { tempo: None, key: None };
        TrackRecord::analyzed(Path::new(path), FileStamp { size: 1, modified_ms: 1 }, 1.0, analysis, tags)
    }

    #[test]
    fn same_song_by_tags_or_file_name() {
        let groups = duplicate_groups(vec![
            record("C:/a/strobe.mp3", Some("deadmau5"), Some("Strobe")),
            record("C:/b/Strobe (copy).flac", Some("Deadmau5"), Some("strobe!")),
            record("C:/c/other.mp3", Some("deadmau5"), Some("Ghosts n Stuff")),
            record("C:/d/Untagged Song.mp3", None, None),
            record("C:/e/untagged_song.wav", None, None),
        ]);
        let paths: Vec<Vec<&str>> =
            groups.iter().map(|g| g.iter().map(|r| r.path.as_str()).collect()).collect();
        assert_eq!(
            paths,
            [
                vec!["C:/a/strobe.mp3", "C:/b/Strobe (copy).flac"],
                vec!["C:/d/Untagged Song.mp3", "C:/e/untagged_song.wav"]
            ]
        );
    }
}
