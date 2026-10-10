//! Background analysis of a real folder: WAV files with a known tempo, a corrupt
//! file, the cache, and re-analysis after a change.

use prisme_analysis::AnalysisOptions;
use prisme_library::{
    AnalysisListener, AnalysisProgress, BackgroundAnalyzer, Cue, CueSlot, LibraryDb, TrackRecord,
};
use std::f64::consts::TAU;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Collector {
    records: Mutex<Vec<TrackRecord>>,
    progress: Mutex<Vec<AnalysisProgress>>,
}

impl AnalysisListener for Collector {
    fn track_analyzed(&self, record: &TrackRecord) {
        self.records.lock().unwrap().push(record.clone());
    }
    fn progress(&self, progress: AnalysisProgress) {
        self.progress.lock().unwrap().push(progress);
    }
}

/// 16-bit stereo WAV with kicks at `bpm`.
fn write_kicks(path: &Path, bpm: f64, seconds: f64) {
    let sr = 44_100u32;
    let n = (seconds * sr as f64) as usize;
    let period = 60.0 / bpm;
    let mut pcm = Vec::with_capacity(n * 4);
    for i in 0..n {
        let t = i as f64 / sr as f64;
        let tt = t % period;
        let v = (-tt * 12.0).exp() * (TAU * (50.0 * tt + 3.3 * (1.0 - (-tt * 30.0).exp()))).sin() * 0.8;
        let s = (v * 32_000.0) as i16;
        pcm.extend_from_slice(&s.to_le_bytes());
        pcm.extend_from_slice(&s.to_le_bytes());
    }
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&2u16.to_le_bytes()); // stereo
    wav.extend_from_slice(&sr.to_le_bytes());
    wav.extend_from_slice(&(sr * 4).to_le_bytes());
    wav.extend_from_slice(&4u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    wav.extend_from_slice(&pcm);
    std::fs::write(path, wav).unwrap();
}

fn run(db: &Arc<LibraryDb>, root: &Path) -> Arc<Collector> {
    let collector = Arc::new(Collector::default());
    BackgroundAnalyzer::start(
        db.clone(),
        root.to_path_buf(),
        2,
        AnalysisOptions::default(),
        collector.clone(),
    )
    .wait();
    collector
}

fn bpm_of(records: &[TrackRecord], name: &str) -> Option<f64> {
    records.iter().find(|r| r.path.ends_with(name)).and_then(|r| r.analysis.tempo).map(|t| t.bpm)
}

#[test]
fn analyses_new_and_changed_files_only() {
    let root: PathBuf = std::env::temp_dir().join(format!("prisme-analyzer-test-{}", std::process::id()));
    std::fs::create_dir_all(root.join("Techno")).unwrap();
    write_kicks(&root.join("house.wav"), 124.0, 20.0);
    write_kicks(&root.join("Techno/techno.wav"), 140.0, 20.0);
    std::fs::write(root.join("broken.mp3"), b"not audio at all").unwrap();
    let db = Arc::new(LibraryDb::open_in_memory().unwrap());

    // first run: everything
    let first = run(&db, &root);
    let records = first.records.lock().unwrap().clone();
    assert_eq!(records.len(), 3);
    assert_eq!(bpm_of(&records, "house.wav"), Some(124.0));
    assert_eq!(bpm_of(&records, "techno.wav"), Some(140.0));
    let broken = records.iter().find(|r| r.path.ends_with("broken.mp3")).unwrap();
    assert!(broken.error.is_some());
    let last = *first.progress.lock().unwrap().last().unwrap();
    assert_eq!(last, AnalysisProgress { total: 3, done: 3, failed: 1, running: false });
    assert_eq!(db.track_count().unwrap(), 3);
    assert!((db.get(&root.join("house.wav")).unwrap().unwrap().duration_s.unwrap() - 20.0).abs() < 0.01);

    // nothing changed: nothing to do
    let second = run(&db, &root);
    assert!(second.records.lock().unwrap().is_empty());
    assert_eq!(second.progress.lock().unwrap().last().unwrap().total, 0);

    // a file replaced by another tempo: only that one
    std::thread::sleep(std::time::Duration::from_millis(20));
    write_kicks(&root.join("house.wav"), 126.0, 21.0);
    let third = run(&db, &root);
    let records = third.records.lock().unwrap().clone();
    assert_eq!(records.len(), 1);
    assert_eq!(bpm_of(&records, "house.wav"), Some(126.0));

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn a_moved_file_keeps_what_it_had_without_a_new_analysis() {
    let root: PathBuf = std::env::temp_dir().join(format!("prisme-move-test-{}", std::process::id()));
    std::fs::create_dir_all(root.join("Sorted")).unwrap();
    let before = root.join("track.wav");
    write_kicks(&before, 124.0, 10.0);
    let db = Arc::new(LibraryDb::open_in_memory().unwrap());
    run(&db, &root);
    db.save_cue(&before, &Cue::point(CueSlot::Hot(0), 2.0)).unwrap();

    let after = root.join("Sorted").join("Artist - Track.wav");
    std::fs::rename(&before, &after).unwrap();
    let moved = run(&db, &root);

    assert_eq!(moved.progress.lock().unwrap().last().unwrap().total, 0, "nothing to analyse");
    let announced: Vec<_> = moved.records.lock().unwrap().iter().map(|r| r.path.clone()).collect();
    assert_eq!(announced, [after.display().to_string()]);
    assert_eq!(db.get(&before).unwrap(), None);
    assert_eq!(db.cues(&after).unwrap().len(), 1);
    std::fs::remove_dir_all(root).ok();
}
