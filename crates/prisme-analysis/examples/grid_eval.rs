//! How tight beat grids sit on the kicks of real tracks.
//!
//! `cargo run --release -p prisme-analysis --example grid_eval -- tracks.txt [grids.tsv...]`
//!
//! `tracks.txt`: one audio file per line, only read. Each track is analysed, and the attack
//! of every kick is found near the beats of its grid. For that grid and for the grids of
//! each `grids.tsv` (lines `path<TAB>bpm<TAB>first_beat_s`, from another version or another
//! program), prints the offset of the grid from the attacks: per track, its median and 90th
//! percentile, and over all the kicks. `--dump` prints the grids found, in the same format.
//! Tracks are analysed on every core at once, or one at a time with `--serial` (for the
//! analysis times).

use prisme_analysis::dsp::analysis_signal;
use prisme_analysis::grid::{fit_grid, kick_attacks, Attack};
use prisme_analysis::onset::onset_envelopes;
use prisme_analysis::tempo::estimate_tempo;
use prisme_analysis::{analyze, AnalysisOptions, Tempo, DEFAULT_MIN_BPM, MAX_ANALYSED_S};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// BPM, first beat (seconds).
type Grid = (f64, f64);

struct Track {
    path: String,
    tempo: Tempo,
    attacks: Vec<Attack>,
    analysis: Duration,
    grid: Duration,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dump = args.iter().any(|a| a == "--dump");
    let threads = if args.iter().any(|a| a == "--serial") {
        1
    } else {
        std::thread::available_parallelism().map_or(4, |n| n.get())
    };
    let mut files = args.iter().filter(|a| !a.starts_with("--"));
    let list = std::fs::read_to_string(files.next().expect("usage: grid_eval tracks.txt [grids.tsv...]"))
        .expect("readable track list");
    let references: Vec<(String, HashMap<String, Grid>)> =
        files.map(|f| (f.clone(), read_grids(f))).collect();
    let paths: Vec<&str> = list.lines().map(str::trim).filter(|l| !l.is_empty()).collect();

    let tracks = Mutex::new(Vec::new());
    let next = Mutex::new(paths.iter());
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| loop {
                let Some(path) = next.lock().unwrap().next().copied() else { break };
                if let Some(track) = analyse(path) {
                    tracks.lock().unwrap().push(track);
                }
            });
        }
    });
    let mut tracks = tracks.into_inner().unwrap();
    tracks.sort_by(|a, b| a.path.cmp(&b.path));

    if dump {
        for t in &tracks {
            println!("{}\t{:.6}\t{:.6}", t.path, t.tempo.bpm, t.tempo.first_beat_s);
        }
        return;
    }

    let mut grids: Vec<(String, Vec<Option<Grid>>)> = vec![(
        "this version".into(),
        tracks.iter().map(|t| Some((t.tempo.bpm, t.tempo.first_beat_s))).collect(),
    )];
    for (name, map) in &references {
        grids.push((name.clone(), tracks.iter().map(|t| map.get(&t.path).copied()).collect()));
    }

    for (name, grid) in &grids {
        println!("\n== {name}");
        println!(
            "{:>9} {:>8} {:>8} {:>8} {:>6}  {:<31}  track",
            "bpm", "bias ms", "med |ms|", "p90 |ms|", "kicks", "bias per quarter of the track"
        );
        let mut all = Vec::new();
        for (t, g) in tracks.iter().zip(grid) {
            let Some((bpm, first)) = *g else { continue };
            let offsets: Vec<f64> = t.attacks.iter().map(|a| offset_ms(a.time_s, bpm, first)).collect();
            let mut abs: Vec<f64> = offsets.iter().map(|o| o.abs()).collect();
            let quarters: Vec<String> = offsets
                .chunks(offsets.len().div_ceil(4).max(1))
                .map(|q| format!("{:+7.2}", quantile(&mut q.to_vec(), 0.5)))
                .collect();
            println!(
                "{bpm:>9.3} {:>8.2} {:>8.2} {:>8.2} {:>6}  {:<31}  {}",
                quantile(&mut offsets.clone(), 0.5),
                quantile(&mut abs, 0.5),
                quantile(&mut abs, 0.9),
                abs.len(),
                quarters.join(" "),
                short(&t.path)
            );
            all.extend(abs);
        }
        println!(
            "all kicks: median {:.2} ms, p90 {:.2} ms, p99 {:.2} ms, worst {:.2} ms ({} kicks)",
            quantile(&mut all, 0.5),
            quantile(&mut all, 0.9),
            quantile(&mut all, 0.99),
            quantile(&mut all, 1.0),
            all.len()
        );
        if name != "this version" {
            let changed = tracks
                .iter()
                .zip(grid)
                .filter(|(t, g)| g.is_some_and(|(bpm, _)| (bpm - t.tempo.bpm).abs() >= 0.005))
                .count();
            println!(
                "BPM different from this version: {changed} of {} tracks",
                grid.iter().flatten().count()
            );
        }
    }

    let mut analysis: Vec<f64> = tracks.iter().map(|t| t.analysis.as_secs_f64() * 1000.0).collect();
    let mut grid: Vec<f64> = tracks.iter().map(|t| t.grid.as_secs_f64() * 1000.0).collect();
    println!(
        "\ntempo and grid per track, once decoded: median {:.0} ms, worst {:.0} ms; of which the fit on the attacks: median {:.0} ms, worst {:.0} ms",
        quantile(&mut analysis, 0.5),
        quantile(&mut analysis, 1.0),
        quantile(&mut grid, 0.5),
        quantile(&mut grid, 1.0)
    );
}

fn analyse(path: &str) -> Option<Track> {
    let audio = match prisme_media::decode_file(path.as_ref()) {
        Ok(audio) => audio,
        Err(e) => {
            eprintln!("{path}: {e}");
            return None;
        }
    };
    let signal = analysis_signal(&audio, MAX_ANALYSED_S);
    let started = Instant::now();
    let coarse = estimate_tempo(&onset_envelopes(&signal), DEFAULT_MIN_BPM)?;
    let fitting = Instant::now();
    fit_grid(&signal, coarse);
    let grid = fitting.elapsed();
    let whole = Instant::now();
    let tempo = analyze(&audio, &AnalysisOptions::default()).tempo?;
    let analysis = whole.elapsed();
    let _ = started;
    let attacks = kick_attacks(&signal, tempo.bpm, tempo.first_beat_s);
    Some(Track { path: path.to_string(), tempo, attacks, analysis, grid })
}

/// Signed distance from `t` to the nearest beat of the grid (ms, positive when the grid is
/// late).
fn offset_ms(t: f64, bpm: f64, first: f64) -> f64 {
    let period = 60.0 / bpm;
    let d = (first - t).rem_euclid(period);
    (if d > period / 2.0 { d - period } else { d }) * 1000.0
}

fn quantile(values: &mut [f64], q: f64) -> f64 {
    if values.is_empty() {
        return f64::NAN;
    }
    values.sort_by(f64::total_cmp);
    values[((values.len() - 1) as f64 * q).round() as usize]
}

fn read_grids(path: &str) -> HashMap<String, Grid> {
    std::fs::read_to_string(path)
        .expect("readable grid file")
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let (file, bpm, first) = (fields.next()?, fields.next()?, fields.next()?);
            Some((file.to_string(), (bpm.parse().ok()?, first.parse().ok()?)))
        })
        .collect()
}

fn short(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}
