//! How well grids follow tracks whose tempo drifts, and that steady tracks keep steady grids.
//!
//! `cargo run --release -p prisme-analysis --example drift_eval -- tracks.txt [--cache grids.tsv]`
//!
//! `tracks.txt`: one audio file per line, only read. A file `<name>.truth` beside a track gives
//! its true beats (lines `beat<TAB>seconds`): the grid's distance from them is printed, for the
//! steady grid and for the grid that follows the drift. Without one, only how far the kicks stray
//! from a steady grid and the anchors. `--cache` keeps the steady grids found (lines
//! `path<TAB>bpm<TAB>first_beat_s`) and takes them from there the next time. `DRIFT_TRACE=<part of
//! a path>` prints that track's kicks (`K`) and anchors (`A`): beat, seconds, ms off the steady
//! grid.

use prisme_analysis::drift::{follow, DRIFT_S};
use prisme_analysis::dsp::analysis_signal;
use prisme_analysis::{analyze, AnalysisOptions, Tempo, MAX_ANALYSED_S};
use prisme_grid::BeatGrid;
use std::collections::HashMap;
use std::io::Write;
use std::sync::Mutex;
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    prisme_analysis::set_full_speed(true);
    let cache_path = args.iter().position(|a| a == "--cache").map(|i| args[i + 1].clone());
    let list = std::fs::read_to_string(&args[0]).expect("readable track list");
    let cache: HashMap<String, (f64, f64)> = cache_path
        .as_deref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|text| {
            text.lines()
                .filter_map(|l| {
                    let mut f = l.split('\t');
                    Some((f.next()?.to_string(), (f.next()?.parse().ok()?, f.next()?.parse().ok()?)))
                })
                .collect()
        })
        .unwrap_or_default();
    let paths: Vec<&str> = list.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let rows = Mutex::new(Vec::new());
    let found = Mutex::new(Vec::new());
    let next = Mutex::new(paths.iter());
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| loop {
                let Some(path) = next.lock().unwrap().next().copied() else { break };
                match evaluate(path, cache.get(path).copied()) {
                    Some((row, steady)) => {
                        if !cache.contains_key(path) {
                            found.lock().unwrap().push(format!("{path}\t{:.6}\t{:.6}", steady.0, steady.1));
                        }
                        rows.lock().unwrap().push(row);
                    }
                    None => eprintln!("{path}: no tempo"),
                }
            });
        }
    });
    if let Some(p) = cache_path {
        let mut file = std::fs::OpenOptions::new().create(true).append(true).open(p).unwrap();
        for line in found.into_inner().unwrap() {
            writeln!(file, "{line}").unwrap();
        }
    }
    let mut rows = rows.into_inner().unwrap();
    rows.sort_by(|a: &Row, b| a.path.cmp(&b.path));
    println!(
        "{:>7} {:>6} {:>5} {:>7}  {:>21}  {:>21}  track",
        "bpm", "stray", "kicks", "anchors", "steady p50/p90/max", "drift p50/p90/max"
    );
    let mut drifting = 0;
    let mut times: Vec<f64> = Vec::new();
    for r in &rows {
        drifting += usize::from(r.anchors > 0);
        times.push(r.time.as_secs_f64() * 1000.0);
        let stats = |s: &Option<[f64; 3]>| match s {
            Some([a, b, c]) => format!("{a:6.1} {b:6.1} {c:7.1}"),
            None => String::new(),
        };
        println!(
            "{:>7.2} {:>6.1} {:>5} {:>7}  {:>21}  {:>21}  {}",
            r.bpm,
            r.straying_ms,
            r.kicks,
            r.anchors,
            stats(&r.steady),
            stats(&r.drift),
            short(&r.path)
        );
    }
    times.sort_by(f64::total_cmp);
    println!(
        "\n{drifting} of {} tracks drift (straying past {:.0} ms); drift's time per track: median {:.1} ms, worst {:.1} ms",
        rows.len(),
        DRIFT_S * 1000.0,
        times.get(times.len() / 2).copied().unwrap_or(0.0),
        times.last().copied().unwrap_or(0.0)
    );
}

struct Row {
    path: String,
    bpm: f64,
    straying_ms: f64,
    kicks: usize,
    anchors: usize,
    steady: Option<[f64; 3]>,
    drift: Option<[f64; 3]>,
    time: Duration,
}

fn evaluate(path: &str, cached: Option<(f64, f64)>) -> Option<(Row, (f64, f64))> {
    let audio = prisme_media::decode_file(path.as_ref()).map_err(|e| eprintln!("{path}: {e}")).ok()?;
    let (bpm, first_beat_s) = match cached {
        Some(grid) => grid,
        None => {
            let t = analyze(&audio, &AnalysisOptions::default()).tempo?;
            (t.bpm, t.first_beat_s)
        }
    };
    let tempo = Tempo { bpm, first_beat_s, confidence: 1.0, bars_sure: true };
    let signal = analysis_signal(&audio, MAX_ANALYSED_S);
    let attacks: Vec<f64> =
        prisme_analysis::grid::kick_attacks(&signal, bpm, first_beat_s).iter().map(|a| a.time_s).collect();
    let started = Instant::now();
    let drift = follow(&signal, &tempo, &attacks);
    let time = started.elapsed();
    if std::env::var("DRIFT_TRACE").is_ok_and(|t| path.contains(&t)) {
        let period = 60.0 / bpm;
        for k in &drift.kicks {
            let o = (k.time_s - first_beat_s - k.beat as f64 * period) * 1000.0;
            eprintln!("K\t{}\t{:.3}\t{o:.1}", k.beat, k.time_s);
        }
        for a in drift.anchors.iter().flatten() {
            let o = (a.time_s - first_beat_s - a.beat * period) * 1000.0;
            eprintln!("A\t{}\t{:.3}\t{o:.1}", a.beat, a.time_s);
        }
    }
    let steady = BeatGrid::steady(bpm, first_beat_s, true);
    let followed = drift
        .anchors
        .as_ref()
        .and_then(|a| {
            let after =
                a.windows(2).last().map(|w| (w[1].beat - w[0].beat) / (w[1].time_s - w[0].time_s) * 60.0)?;
            BeatGrid::anchored(a.clone(), after, true)
        })
        .unwrap_or_else(|| steady.clone());
    let truth = read_truth(path);
    let error = |grid: &BeatGrid| {
        truth.as_ref().map(|beats| {
            // the decoders apart: an MP3's encoder delay, kept by one and not the other
            let signed: Vec<f64> =
                beats.iter().map(|&u| (u - grid.time_of(grid.beats_at(u).round())) * 1000.0).collect();
            let mut sorted = signed.clone();
            sorted.sort_by(f64::total_cmp);
            let bias = sorted[sorted.len() / 2];
            let mut e: Vec<f64> = signed.iter().map(|s| (s - bias).abs()).collect();
            e.sort_by(f64::total_cmp);
            let q = |q: f64| e[((e.len() - 1) as f64 * q).round() as usize];
            [q(0.5), q(0.9), q(1.0)]
        })
    };
    let row = Row {
        path: path.to_string(),
        bpm,
        straying_ms: drift.straying_s * 1000.0,
        kicks: drift.kicks.len(),
        anchors: drift.anchors.as_ref().map_or(0, Vec::len),
        steady: error(&steady),
        drift: error(&followed),
        time,
    };
    Some((row, (bpm, first_beat_s)))
}

/// The true beats of a track, from `<name>.truth` beside it.
fn read_truth(path: &str) -> Option<Vec<f64>> {
    let stem = path.rsplit_once('.').map_or(path, |(s, _)| s);
    let text = std::fs::read_to_string(format!("{stem}.truth")).ok()?;
    Some(text.lines().filter_map(|l| l.split('\t').nth(1)?.parse().ok()).collect())
}

fn short(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}
