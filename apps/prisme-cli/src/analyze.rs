//! `prisme analyze`: tempo and key of files or whole folders, on all CPU cores.

use anyhow::Result;
use prisme_analysis::{AnalysisOptions, TrackAnalysis};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

const AUDIO_EXTENSIONS: [&str; 9] = ["mp3", "flac", "wav", "ogg", "m4a", "aac", "mp4", "aif", "aiff"];

struct Row {
    path: PathBuf,
    result: Result<(TrackAnalysis, u128, u128), String>,
}

pub fn run(paths: &[PathBuf], options: AnalysisOptions, csv: Option<&Path>) -> Result<()> {
    let mut files = Vec::new();
    for p in paths {
        collect(p, &mut files);
    }
    files.sort();
    let started = Instant::now();
    let next = AtomicUsize::new(0);
    let rows = Mutex::new(Vec::with_capacity(files.len()));
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get());
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some(path) = files.get(i) else { break };
                let row = Row { path: path.clone(), result: analyze_file(path, &options) };
                print_row(&row);
                rows.lock().unwrap().push(row);
            });
        }
    });
    println!("{} files in {:.1} s", files.len(), started.elapsed().as_secs_f64());

    if let Some(csv) = csv {
        let mut rows = rows.into_inner().unwrap();
        rows.sort_by(|a, b| a.path.cmp(&b.path));
        let mut out = String::from("path,bpm,first_beat_s,tempo_confidence,key,camelot,key_confidence,tuning_cents\n");
        for r in &rows {
            if let Ok((a, _, _)) = &r.result {
                let t = a.tempo;
                let k = a.key;
                out += &format!(
                    "\"{}\",{},{},{},{},{},{},{}\n",
                    r.path.display(),
                    t.map_or(String::new(), |t| format!("{:.3}", t.bpm)),
                    t.map_or(String::new(), |t| format!("{:.4}", t.first_beat_s)),
                    t.map_or(String::new(), |t| format!("{:.3}", t.confidence)),
                    k.map_or(String::new(), |k| k.key.name().to_string()),
                    k.map_or(String::new(), |k| k.key.camelot()),
                    k.map_or(String::new(), |k| format!("{:.3}", k.confidence)),
                    k.map_or(String::new(), |k| format!("{:.1}", k.tuning_cents)),
                );
            }
        }
        std::fs::write(csv, out)?;
        println!("written {}", csv.display());
    }
    Ok(())
}

fn collect(path: &Path, files: &mut Vec<PathBuf>) {
    if path.is_dir() {
        if let Ok(entries) = std::fs::read_dir(path) {
            for e in entries.flatten() {
                collect(&e.path(), files);
            }
        }
    } else if path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| AUDIO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
    {
        files.push(path.to_path_buf());
    }
}

/// Analysis, decode time and analysis time (ms).
fn analyze_file(path: &Path, options: &AnalysisOptions) -> Result<(TrackAnalysis, u128, u128), String> {
    let t0 = Instant::now();
    let track = prisme_media::decode_file(path).map_err(|e| e.to_string())?;
    let decode_ms = t0.elapsed().as_millis();
    let t1 = Instant::now();
    let analysis = prisme_analysis::analyze(&track, options);
    Ok((analysis, decode_ms, t1.elapsed().as_millis()))
}

fn print_row(row: &Row) {
    let name = row.path.file_name().map_or_else(|| row.path.display().to_string(), |n| n.to_string_lossy().into_owned());
    match &row.result {
        Ok((a, decode_ms, analysis_ms)) => {
            let tempo = a.tempo.map_or("   --  ".into(), |t| format!("{:7.2}", t.bpm));
            let grid = a.tempo.map_or("  -- ".into(), |t| format!("{:5.3}", t.first_beat_s));
            let tconf = a.tempo.map_or(" -- ".into(), |t| format!("{:4.2}", t.confidence));
            let key = a.key.map_or("  --      ".into(), |k| format!("{:>3} {:<4}", k.key.camelot(), k.key.name()));
            let kconf = a.key.map_or(" -- ".into(), |k| format!("{:4.2}", k.confidence));
            println!("{tempo} BPM ({tconf}) grid {grid}s  {key} ({kconf})  {decode_ms:4}+{analysis_ms:3} ms  {name}");
        }
        Err(e) => println!("error: {e}  {name}"),
    }
}
