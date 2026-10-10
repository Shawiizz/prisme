//! Compares key detection settings on reference tracks.
//!
//! `cargo run --release -p prisme-analysis --example key_eval -- refs.csv`
//!
//! `refs.csv`: a header line, then `"path",Key|OtherAcceptedKey` per track (keys spelled
//! as `Key::name()`: "Am", "F#m", "Eb"...).

use prisme_analysis::dsp::analysis_signal;
use prisme_analysis::key::SpectralPeaks;
use prisme_analysis::{KeyConfig, Profiles};
use std::sync::Mutex;

fn main() {
    let path = std::env::args().nth(1).expect("usage: key_eval refs.csv");
    let refs: Vec<(String, Vec<String>)> = std::fs::read_to_string(path)
        .expect("readable csv")
        .lines()
        .skip(1)
        .filter_map(|line| {
            let (file, keys) = line.rsplit_once(',')?;
            Some((file.trim_matches('"').to_string(), keys.split('|').map(str::to_string).collect()))
        })
        .collect();

    // peaks of every track, extracted once over the widest range
    let tracks = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for (file, keys) in &refs {
            let tracks = &tracks;
            s.spawn(move || {
                let audio = prisme_media::decode_file(file.as_ref()).expect("decodable track");
                let peaks = SpectralPeaks::extract(&analysis_signal(&audio), 30.0, 4_000.0);
                tracks.lock().unwrap().push((file.clone(), keys.clone(), peaks));
            });
        }
    });
    let mut tracks = tracks.into_inner().unwrap();
    tracks.sort_by(|a, b| a.0.cmp(&b.0));

    let base = KeyConfig::default();
    let variants: Vec<(&str, KeyConfig)> = vec![
        ("default", base),
        ("no tuning", KeyConfig { estimate_tuning: false, ..base }),
        ("tuning >= 100 Hz", KeyConfig { tuning_min_hz: 100.0, ..base }),
        ("tuning >= 200 Hz", KeyConfig { tuning_min_hz: 200.0, ..base }),
        ("min 60 Hz, tuning >= 100", KeyConfig { min_hz: 60.0, tuning_min_hz: 100.0, ..base }),
        ("min 80 Hz, tuning >= 100", KeyConfig { min_hz: 80.0, tuning_min_hz: 100.0, ..base }),
        ("min 100 Hz, tuning >= 100", KeyConfig { min_hz: 100.0, tuning_min_hz: 100.0, ..base }),
        ("min 90 Hz, tuning >= 90", KeyConfig { min_hz: 90.0, tuning_min_hz: 90.0, ..base }),
        ("min 120 Hz, tuning >= 120", KeyConfig { min_hz: 120.0, tuning_min_hz: 120.0, ..base }),
        ("min 150 Hz, tuning >= 150", KeyConfig { min_hz: 150.0, tuning_min_hz: 150.0, ..base }),
        ("min 100, max 1500", KeyConfig { min_hz: 100.0, tuning_min_hz: 100.0, max_hz: 1_500.0, ..base }),
        ("min 100, max 3000", KeyConfig { min_hz: 100.0, tuning_min_hz: 100.0, max_hz: 3_000.0, ..base }),
        ("min 100, log", KeyConfig { min_hz: 100.0, tuning_min_hz: 100.0, log_magnitude: true, ..base }),
        ("min 100, no tuning", KeyConfig { min_hz: 100.0, estimate_tuning: false, ..base }),
        ("log, tuning >= 100", KeyConfig { log_magnitude: true, tuning_min_hz: 100.0, ..base }),
        ("log, min 60, tuning >= 100", KeyConfig { log_magnitude: true, min_hz: 60.0, tuning_min_hz: 100.0, ..base }),
        ("krumhansl, tuning >= 100", KeyConfig { profiles: Profiles::KrumhanslKessler, tuning_min_hz: 100.0, ..base }),
        (
            "krumhansl, log, tuning >= 100",
            KeyConfig { profiles: Profiles::KrumhanslKessler, log_magnitude: true, tuning_min_hz: 100.0, ..base },
        ),
        ("max 1000 Hz, tuning >= 100", KeyConfig { max_hz: 1_000.0, tuning_min_hz: 100.0, ..base }),
        ("max 4000 Hz, tuning >= 100", KeyConfig { max_hz: 4_000.0, tuning_min_hz: 100.0, ..base }),
    ];

    for (name, config) in &variants {
        let mut exact = 0;
        let mut misses = Vec::new();
        for (file, keys, peaks) in &tracks {
            let found = peaks.estimate(config);
            let label = found.map_or("--".to_string(), |k| format!("{} ({:+.0}c)", k.key.name(), k.tuning_cents));
            if found.is_some_and(|k| keys.iter().any(|r| r == k.key.name())) {
                exact += 1;
            } else {
                let short = std::path::Path::new(file).file_stem().unwrap().to_string_lossy().chars().take(28).collect::<String>();
                misses.push(format!("{short}: {label} vs {}", keys.join("/")));
            }
        }
        println!("{exact:2}/{}  {name}", tracks.len());
        if std::env::var("KEY_EVAL_VERBOSE").is_ok() {
            for m in misses {
                println!("        {m}");
            }
        }
    }
}
