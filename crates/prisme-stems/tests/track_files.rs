//! Separating audio files and reading their stored vocals back.

use std::path::{Path, PathBuf};

use prisme_stems::track::{has_vocals, load_vocals, separate_file, vocals_path};
use prisme_stems::{CpuNetwork, Separator, Weights};

fn separator() -> Separator<CpuNetwork> {
    let weights =
        Weights::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/tiny_roformer.safetensors"))
            .unwrap();
    Separator::new(CpuNetwork::new(weights))
}

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("prisme-stems-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A loud stereo WAV, peaking over full scale like a hot master.
fn write_track(path: &Path, rate: u32, frames: usize) -> Vec<f32> {
    let samples: Vec<f32> =
        (0..frames * 2).map(|i| ((i * 7919 % 2003) as f32 / 1001.5 - 1.0) * 1.3).collect();
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    samples.iter().for_each(|s| w.write_sample(*s).unwrap());
    w.finalize().unwrap();
    samples
}

#[test]
fn stored_vocals_read_back_as_separated() {
    let dir = folder("same-rate");
    let track = dir.join("track.wav");
    let samples = write_track(&track, 44_100, 3000);
    let mut separator = separator();
    let (left, right): (Vec<f32>, Vec<f32>) = samples.chunks_exact(2).map(|f| (f[0], f[1])).unzip();
    let [l, r] = separator.vocals(&left, &right, |_, _| true).unwrap();

    assert!(!has_vocals(&track));
    separate_file(&mut separator, &track, |_, _| true).unwrap();
    assert!(has_vocals(&track));
    let vocals = load_vocals(&track, 3000, 44_100).unwrap().expect("vocals stored");
    assert_eq!((vocals.frames(), vocals.sample_rate()), (3000, 44_100));
    // 16 bits at half level: steps of 2 / 32768, and nothing beyond twice full scale
    let stored = |v: f32| v.clamp(-2.0, 32767.0 / 16384.0);
    let err = (0..3000usize)
        .map(|i| {
            let (a, b) = vocals.frame(i as isize);
            (a - stored(l[i])).abs().max((b - stored(r[i])).abs())
        })
        .fold(0.0, f32::max);
    assert!(err <= 1.0 / 32768.0 + 1e-7, "max error {err}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn vocals_keep_the_rate_and_length_of_their_track() {
    let dir = folder("other-rate");
    let track = dir.join("track.wav");
    write_track(&track, 48_000, 4321);
    separate_file(&mut separator(), &track, |_, _| true).unwrap();
    let vocals = load_vocals(&track, 4321, 48_000).unwrap().expect("vocals stored");
    assert_eq!((vocals.frames(), vocals.sample_rate()), (4321, 48_000));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn vocals_of_another_version_of_the_file_are_ignored() {
    let dir = folder("stale");
    let track = dir.join("track.wav");
    write_track(&track, 44_100, 3000);
    separate_file(&mut separator(), &track, |_, _| true).unwrap();
    assert!(load_vocals(&track, 2999, 44_100).unwrap().is_none());
    assert!(load_vocals(&track, 3000, 48_000).unwrap().is_none());
    assert!(vocals_path(&track).unwrap().is_file());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_cancelled_separation_stores_nothing() {
    let dir = folder("cancelled");
    let track = dir.join("track.wav");
    write_track(&track, 44_100, 3000);
    assert!(separate_file(&mut separator(), &track, |_, _| false).is_err());
    assert!(!has_vocals(&track));
    std::fs::remove_dir_all(dir).unwrap();
}
