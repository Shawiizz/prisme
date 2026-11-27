//! How long a deck waits for the grid of a track loaded on it, never analysed: the file decoded
//! whole as a deck does, then analysed as a deck's track is, alone or while the library
//! analyses other tracks, as many at once as it does while no deck plays.
//!
//! usage: deck_wait <track> [<track the library analyses>...]

use prisme_analysis::{analyze, analyze_file, AnalysisOptions};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (track, others) = args.split_first().expect("usage: deck_wait <track> [<other track>...]");
    // no deck plays: the library's analysis takes the whole computer
    prisme_analysis::set_full_speed(true);

    let started = Instant::now();
    let audio = prisme_media::decode_file(Path::new(track)).expect("a readable track");
    let decoded = started.elapsed();

    let stop = AtomicBool::new(false);
    let analysed = std::thread::scope(|s| {
        for other in others {
            let stop = &stop;
            s.spawn(move || {
                prisme_analysis::background_priority();
                while !stop.load(Ordering::Relaxed) {
                    prisme_analysis::analysis_priority();
                    let _ = analyze_file(Path::new(other), &AnalysisOptions::default());
                }
            });
        }
        // the library well into its tracks when the deck's comes
        if !others.is_empty() {
            std::thread::sleep(Duration::from_secs(2));
        }
        let started = Instant::now();
        let options =
            AnalysisOptions { network_threads: prisme_analysis::awaited_threads(), ..Default::default() };
        let analysis = prisme_analysis::for_a_deck(|| analyze(&audio, &options));
        let took = started.elapsed();
        stop.store(true, Ordering::Relaxed);
        (analysis, took)
    });

    let (analysis, took) = analysed;
    let bpm = analysis.tempo.map_or(0.0, |t| t.bpm);
    println!(
        "decoded in {:.2} s, analysed in {:.2} s ({} other tracks analysed meanwhile), {bpm:.2} BPM",
        decoded.as_secs_f64(),
        took.as_secs_f64(),
        others.len(),
    );
}
