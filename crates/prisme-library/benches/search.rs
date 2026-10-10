//! Search-as-you-type in a large library. Budget: under 20 ms per keystroke in 50 000
//! tracks.

use criterion::{criterion_group, criterion_main, Criterion};
use prisme_analysis::TrackAnalysis;
use prisme_library::{FileStamp, LibraryDb, TrackRecord};
use prisme_media::TrackTags;
use std::path::PathBuf;

const TRACKS: usize = 50_000;
const WORDS: [&str; 16] = [
    "night", "love", "deep", "house", "dream", "fire", "city", "lights", "summer", "soul", "groove", "dance",
    "heart", "star", "ocean", "time",
];

fn library() -> LibraryDb {
    let db = LibraryDb::open_in_memory().expect("database");
    let word = |i: usize| WORDS[i % WORDS.len()];
    for i in 0..TRACKS {
        let tags = TrackTags {
            title: Some(format!("{} {} {i}", word(i), word(i / 7))),
            artist: Some(format!("Artist {}", i % 3_000)),
            album: Some(format!("{} {}", word(i / 3), word(i / 11))),
            genre: Some(word(i / 13).to_string()),
            ..TrackTags::default()
        };
        let path = PathBuf::from(format!("C:/Music/{}/{i}.mp3", word(i / 17)));
        let record = TrackRecord::analyzed(
            &path,
            FileStamp { size: i as u64, modified_ms: 0 },
            300.0,
            TrackAnalysis::default(),
            tags,
        );
        db.save(&record).expect("save");
    }
    db
}

fn search(c: &mut Criterion) {
    let db = library();
    let mut group = c.benchmark_group("search in 50 000 tracks");
    for query in ["d", "deep", "deep ho", "artist 1234"] {
        group.bench_function(query, |b| b.iter(|| db.search(query, 500).expect("search")));
    }
    group.finish();
}

criterion_group!(benches, search);
criterion_main!(benches);
