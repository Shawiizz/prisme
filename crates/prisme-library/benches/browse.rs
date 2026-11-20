//! Browsing a library of 50 000 tracks: a search typed key by key, a sort, the rows of the
//! window. Budget: a keystroke well under the round trip to the interface (a few ms).

use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use prisme_analysis::{Key, KeyEstimate, Tempo, TrackAnalysis};
use prisme_library::browse::{Browser, Column, Pick, Scope, Source};
use prisme_library::mix::MixReference;
use prisme_library::{FileStamp, TrackRecord};
use prisme_media::TrackTags;
use std::path::PathBuf;

const TRACKS: usize = 50_000;
const WORDS: [&str; 16] = [
    "night", "love", "deep", "house", "dream", "fire", "city", "lights", "summer", "soul", "groove", "dance",
    "heart", "star", "ocean", "time",
];

fn records() -> Vec<TrackRecord> {
    let word = |i: usize| WORDS[i % WORDS.len()];
    let keys: Vec<Key> = Key::all().collect();
    (0..TRACKS)
        .map(|i| {
            let tags = TrackTags {
                title: Some(format!("{} {} {i}", word(i), word(i / 7))),
                artist: Some(format!("Artist {}", i % 3_000)),
                album: Some(format!("{} {}", word(i / 3), word(i / 11))),
                genre: Some(word(i / 13).to_string()),
                ..TrackTags::default()
            };
            let analysis = TrackAnalysis {
                tempo: Some(Tempo {
                    bpm: 110.0 + (i % 400) as f64 / 10.0,
                    first_beat_s: 0.0,
                    confidence: 1.0,
                }),
                key: Some(KeyEstimate { key: keys[i % keys.len()], confidence: 1.0, tuning_cents: 0.0 }),
            };
            let path = PathBuf::from(format!("C:/Music/{}/{i}.mp3", word(i / 17)));
            TrackRecord::analyzed(&path, FileStamp { size: i as u64, modified_ms: 0 }, 300.0, analysis, tags)
        })
        .collect()
}

/// The whole library searched, sorted by title.
fn library() -> Browser {
    let mut browser = Browser::default();
    browser.set_roots(vec![PathBuf::from("C:/Music")]);
    browser.load(records());
    browser.show(Source::Prepare, Vec::new());
    browser.set_scope(Scope::Library);
    browser.set_window(0, 60);
    browser
}

fn browse(c: &mut Criterion) {
    let mut group = c.benchmark_group("browse 50 000 tracks");
    group.bench_function("load the library", |b| {
        b.iter_batched(records, |records| Browser::default().load(records), BatchSize::LargeInput)
    });
    let mut browser = library();
    for query in ["d", "deep", "deep ho", "artist 1234", "zzz"] {
        group.bench_function(format!("search {query:?} from nothing"), |b| {
            b.iter(|| {
                browser.set_filter("");
                browser.set_filter(query);
            })
        });
    }
    group.bench_function("type \"deep house\" key by key", |b| {
        b.iter(|| {
            browser.set_filter("");
            for end in 1..="deep house".len() {
                browser.set_filter(&"deep house"[..end]);
            }
        })
    });
    browser.set_filter("");
    group.bench_function("sort by title", |b| b.iter(|| browser.sort_by(Column::Title, None)));
    group.bench_function("sort by tempo", |b| b.iter(|| browser.sort_by(Column::Bpm, None)));
    let mix = MixReference {
        bpm: Some(126.0),
        key: Key::from_camelot("8A"),
        path: None,
        range: 0.08,
        key_lock: false,
    };
    group.bench_function("sort by mix", |b| b.iter(|| browser.sort_by(Column::Mix, Some(mix.clone()))));
    group.bench_function("the rows of a window", |b| {
        b.iter(|| browser.window().map(|row| row.track.title.len()).sum::<usize>())
    });
    group.bench_function("select every track", |b| b.iter(|| browser.select_all()));
    let last = TRACKS - 1;
    let last = browser
        .find(&format!("C:/Music/{}/{last}.mp3", WORDS[last / 17 % WORDS.len()]), None)
        .expect("listed");
    group.bench_function("pick by range across the library", |b| {
        b.iter(|| {
            browser.clear_selection();
            browser.move_cursor(-100_000);
            browser.select(last, Pick::Range);
        })
    });
    let record = records().swap_remove(25_000);
    group.bench_function("a track analysed again", |b| b.iter(|| browser.put(record.clone())));
    group.finish();
}

criterion_group!(benches, browse);
criterion_main!(benches);
