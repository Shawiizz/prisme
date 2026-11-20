//! Every track the library lists, in memory: what the database knows of each, with what a
//! search and a sort read of it made once, so that a list of tens of thousands of tracks is
//! searched as fast as a key is typed. A track is known by a [`TrackId`] for as long as the
//! application runs, whatever becomes of its record.

use crate::db::TrackRecord;
use prisme_analysis::Key;
use std::collections::HashMap;
use std::path::Path;

/// A track of the [`Catalogue`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TrackId(u32);

/// A track as the lists show it: its file, and what the library knows of it.
#[derive(Debug, Clone)]
pub struct Track {
    pub path: String,
    /// Its title tag, else its file name without the extension.
    pub title: String,
    /// None until it is analysed, or once it left the library.
    pub record: Option<TrackRecord>,
    /// Its tempo and key, the corrections made by hand applied.
    pub bpm: Option<f64>,
    pub key: Option<Key>,
    /// Its file name and tags, folded (see [`folded`]): what a search looks into.
    pub(crate) words: String,
    /// Its title, artist, album and genre in lower case, as they are sorted.
    pub(crate) sorted: [Option<String>; 4],
}

impl Track {
    fn new(path: String, record: Option<TrackRecord>) -> Self {
        let name =
            Path::new(&path).file_name().map_or_else(|| path.clone(), |n| n.to_string_lossy().into_owned());
        let stem =
            Path::new(&path).file_stem().map_or_else(|| path.clone(), |s| s.to_string_lossy().into_owned());
        let tags = record.as_ref().map(|r| &r.tags);
        let tag = |pick: fn(&prisme_media::TrackTags) -> &Option<String>| tags.and_then(|t| pick(t).clone());
        let (artist, album, genre) = (tag(|t| &t.artist), tag(|t| &t.album), tag(|t| &t.genre));
        let title = tag(|t| &t.title).unwrap_or(stem);
        let words = [Some(&name), Some(&title), artist.as_ref(), album.as_ref(), genre.as_ref()]
            .into_iter()
            .flatten()
            .map(|text| folded(text))
            .collect::<Vec<_>>()
            .join(" ");
        let lower = |text: &Option<String>| text.as_ref().map(|t| t.to_lowercase());
        let sorted = [Some(title.to_lowercase()), lower(&artist), lower(&album), lower(&genre)];
        let analysis = record.as_ref().map(TrackRecord::effective_analysis);
        let bpm = analysis.and_then(|a| a.tempo).map(|t| t.bpm);
        let key = analysis.and_then(|a| a.key).map(|k| k.key);
        Self { path, title, record, bpm, key, words, sorted }
    }

    /// Analysed, and readable: one of the library's tracks a search of the whole library finds.
    pub fn analysed(&self) -> bool {
        self.record.as_ref().is_some_and(|r| r.error.is_none())
    }

    /// Its file could not be decoded.
    pub fn failed(&self) -> bool {
        self.record.as_ref().is_some_and(|r| r.error.is_some())
    }

    /// Whether every one of `words` (folded) is in its file name or its tags.
    pub fn matches(&self, words: &[String]) -> bool {
        words.iter().all(|word| self.words.contains(word.as_str()))
    }
}

/// The tracks of the library, and the files listed beside them not analysed yet.
#[derive(Debug, Default)]
pub struct Catalogue {
    tracks: Vec<Track>,
    ids: HashMap<String, TrackId>,
}

impl Catalogue {
    pub fn get(&self, id: TrackId) -> &Track {
        &self.tracks[id.0 as usize]
    }

    pub fn find(&self, path: &str) -> Option<TrackId> {
        self.ids.get(path).copied()
    }

    /// The track at `path`, listed without a record if it was not yet.
    pub fn id(&mut self, path: &str) -> TrackId {
        if let Some(id) = self.find(path) {
            return id;
        }
        self.push(Track::new(path.to_owned(), None))
    }

    /// `record`, in place of what was known of its track: its id.
    pub fn put(&mut self, record: TrackRecord) -> TrackId {
        let track = Track::new(record.path.clone(), Some(record));
        match self.find(&track.path) {
            Some(id) => {
                self.tracks[id.0 as usize] = track;
                id
            }
            None => self.push(track),
        }
    }

    /// The track at `path` out of the library, still listed where it is.
    pub fn forget(&mut self, path: &str) -> Option<TrackId> {
        let id = self.find(path)?;
        let track = &mut self.tracks[id.0 as usize];
        *track = Track::new(std::mem::take(&mut track.path), None);
        Some(id)
    }

    /// The library as the database holds it all, `records`: the tracks it no longer holds left
    /// without a record.
    pub fn load(&mut self, records: Vec<TrackRecord>) {
        let mut known = vec![false; self.tracks.len()];
        for record in records {
            let id = self.put(record);
            match known.get_mut(id.0 as usize) {
                Some(seen) => *seen = true,
                None => known.push(true),
            }
        }
        for (at, seen) in known.into_iter().enumerate() {
            if !seen && self.tracks[at].record.is_some() {
                let path = std::mem::take(&mut self.tracks[at].path);
                self.tracks[at] = Track::new(path, None);
            }
        }
    }

    /// Every track with a record, in the order they came.
    pub fn ids(&self) -> impl Iterator<Item = TrackId> + '_ {
        self.tracks.iter().enumerate().filter(|(_, t)| t.record.is_some()).map(|(at, _)| TrackId(at as u32))
    }

    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    fn push(&mut self, track: Track) -> TrackId {
        let id = TrackId(self.tracks.len() as u32);
        self.ids.insert(track.path.clone(), id);
        self.tracks.push(track);
        id
    }
}

/// `text` in lower case, the accents of the Latin letters taken off: "Café Noir" and "cafe
/// noir" are searched alike.
pub fn folded(text: &str) -> String {
    text.chars().flat_map(char::to_lowercase).map(unaccented).collect()
}

fn unaccented(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => 'a',
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => 'c',
        'ď' | 'đ' => 'd',
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => 'e',
        'ĝ' | 'ğ' | 'ġ' | 'ģ' => 'g',
        'ĥ' | 'ħ' => 'h',
        'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => 'i',
        'ĵ' => 'j',
        'ķ' => 'k',
        'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' => 'l',
        'ñ' | 'ń' | 'ņ' | 'ň' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => 'o',
        'ŕ' | 'ŗ' | 'ř' => 'r',
        'ś' | 'ŝ' | 'ş' | 'š' => 's',
        'ţ' | 'ť' | 'ŧ' => 't',
        'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => 'u',
        'ŵ' => 'w',
        'ý' | 'ÿ' | 'ŷ' => 'y',
        'ź' | 'ż' | 'ž' => 'z',
        c => c,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::db::FileStamp;
    use prisme_analysis::{KeyEstimate, Tempo, TrackAnalysis};
    use prisme_media::TrackTags;

    /// A track analysed at `path`, with `tags`, at `bpm` in `camelot`.
    pub fn record(path: &str, tags: TrackTags, bpm: Option<f64>, camelot: Option<&str>) -> TrackRecord {
        let analysis = TrackAnalysis {
            tempo: bpm.map(|bpm| Tempo { bpm, first_beat_s: 0.0, confidence: 1.0 }),
            key: camelot.and_then(Key::from_camelot).map(|key| KeyEstimate {
                key,
                confidence: 1.0,
                tuning_cents: 0.0,
            }),
        };
        TrackRecord::analyzed(Path::new(path), FileStamp { size: 1, modified_ms: 1 }, 300.0, analysis, tags)
    }

    pub fn tagged(path: &str, title: &str, artist: &str) -> TrackRecord {
        let tags =
            TrackTags { title: Some(title.into()), artist: Some(artist.into()), ..TrackTags::default() };
        record(path, tags, None, None)
    }

    #[test]
    fn a_track_is_titled_by_its_tag_else_its_file_name() {
        let mut catalogue = Catalogue::default();
        let tagged = catalogue.put(tagged("C:/Music/01 - a.mp3", "Strobe", "deadmau5"));
        let bare = catalogue.id("C:/Music/02 - b.flac");
        assert_eq!(catalogue.get(tagged).title, "Strobe");
        assert_eq!(catalogue.get(bare).title, "02 - b");
        assert!(catalogue.get(tagged).analysed() && !catalogue.get(bare).analysed());
    }

    #[test]
    fn a_search_finds_every_word_in_the_file_name_or_the_tags_accents_aside() {
        let mut catalogue = Catalogue::default();
        let id = catalogue.put(record(
            "C:/Music/Café Noir.mp3",
            TrackTags { genre: Some("Progressive House".into()), ..TrackTags::default() },
            None,
            None,
        ));
        let words = |query: &str| folded(query).split_whitespace().map(String::from).collect::<Vec<_>>();
        let track = catalogue.get(id);
        assert!(track.matches(&words("cafe ouse")), "a word inside another");
        assert!(track.matches(&words("NOIR.mp3")));
        assert!(!track.matches(&words("cafe techno")));
        assert!(track.matches(&[]));
    }

    #[test]
    fn a_track_keeps_its_id_and_loses_its_record_when_the_library_forgets_it() {
        let mut catalogue = Catalogue::default();
        let a = catalogue.put(tagged("C:/a.mp3", "A", "x"));
        let b = catalogue.put(tagged("C:/b.mp3", "B", "x"));
        catalogue.load(vec![tagged("C:/b.mp3", "B again", "x"), tagged("C:/c.mp3", "C", "x")]);
        assert!(catalogue.get(a).record.is_none(), "gone from the database");
        assert_eq!(catalogue.get(b).title, "B again");
        assert_eq!(catalogue.ids().count(), 2);
        assert_eq!(catalogue.forget("C:/b.mp3"), Some(b));
        assert_eq!(catalogue.get(b).title, "b");
        assert_eq!(catalogue.find("C:/b.mp3"), Some(b));
    }
}
