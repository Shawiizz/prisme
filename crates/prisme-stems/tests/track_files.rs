//! Separating audio files and reading their stored stems back.

use std::path::{Path, PathBuf};

use prisme_stems::track::{has_stem, load_stem, separate_file, stem_path, TrackSeparation};
use prisme_stems::{CpuNetwork, Separator, Weights};

fn load(name: &str) -> Separator<CpuNetwork> {
    let weights =
        Weights::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data").join(name)).unwrap();
    Separator::new(CpuNetwork::new(weights))
}

fn separator() -> Separator<CpuNetwork> {
    load("tiny_roformer.safetensors")
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
    let [l, r] = separator.separate(&left, &right, |_, _| true).unwrap().remove(0);

    assert!(!has_stem(&track, "vocals"));
    separate_file(&mut [&mut separator], &track, |_, _| true).unwrap();
    assert!(has_stem(&track, "vocals"));
    let vocals = load_stem(&track, "vocals", 3000, 44_100).unwrap().expect("vocals stored");
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
    separate_file(&mut [&mut separator()], &track, |_, _| true).unwrap();
    let vocals = load_stem(&track, "vocals", 4321, 48_000).unwrap().expect("vocals stored");
    assert_eq!((vocals.frames(), vocals.sample_rate()), (4321, 48_000));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn vocals_of_another_version_of_the_file_are_ignored() {
    let dir = folder("stale");
    let track = dir.join("track.wav");
    write_track(&track, 44_100, 3000);
    separate_file(&mut [&mut separator()], &track, |_, _| true).unwrap();
    assert!(load_stem(&track, "vocals", 2999, 44_100).unwrap().is_none());
    assert!(load_stem(&track, "vocals", 3000, 48_000).unwrap().is_none());
    assert!(stem_path(&track, "vocals").unwrap().is_file());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_cancelled_separation_stores_nothing() {
    let dir = folder("cancelled");
    let track = dir.join("track.wav");
    write_track(&track, 44_100, 3000);
    assert!(separate_file(&mut [&mut separator()], &track, |_, _| false).is_err());
    assert!(!has_stem(&track, "vocals"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn several_separators_store_all_their_stems_and_count_all_their_chunks() {
    let dir = folder("several");
    let track = dir.join("track.wav");
    write_track(&track, 44_100, 3000);
    let (mut vocals, mut others) = (separator(), load("tiny_roformer_stems.safetensors"));
    let chunks = vocals.chunks(3000) + others.chunks(3000);
    let mut seen = Vec::new();
    let stored = separate_file(&mut [&mut vocals, &mut others], &track, |done, total| {
        seen.push((done, total));
        true
    })
    .unwrap();
    assert_eq!(stored.len(), 3);
    assert_eq!(seen.last(), Some(&(chunks, chunks)));
    assert!(seen.windows(2).all(|w| w[1].0 == w[0].0 + 1));
    for stem in ["vocals", "drums", "bass"] {
        assert!(load_stem(&track, stem, 3000, 44_100).unwrap().is_some(), "{stem}");
    }
    std::fs::remove_dir_all(dir).unwrap();
}

/// Played from its middle, a track gets its stems there first, then on either side: what is
/// final reads as it ends up, and as it is stored.
#[test]
fn a_played_track_gets_its_stems_around_where_it_plays() {
    for rate in [44_100, 48_000] {
        let dir = folder(&format!("played-{rate}"));
        let track = dir.join("track.wav");
        let frames = 90_000;
        write_track(&track, rate, frames);
        let mut overlapping = Weights::load(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/tiny_roformer_stems.safetensors"),
        )
        .unwrap();
        overlapping.geometry.config.overlap = 4;
        let (mut vocals, mut others) = (separator(), Separator::new(CpuNetwork::new(overlapping)));
        let separators = &mut [&mut vocals, &mut others];
        let around = 50_000;
        let mut separation = TrackSeparation::open(separators, &track, None, Some(around)).unwrap();

        let mut seen: Vec<Vec<(std::ops::Range<usize>, Vec<f32>)>> = vec![Vec::new(); 3];
        let mut reached = [None; 3];
        while !separation.is_done() {
            separation.step(separators).unwrap();
            for (stem, (_, finished)) in separation.stems().into_iter().enumerate() {
                if let Some((before, _)) = seen[stem].last() {
                    assert!(
                        finished.start <= before.start && finished.end >= before.end,
                        "the final frames grow"
                    );
                }
                if finished.contains(&around) && reached[stem].is_none() {
                    reached[stem] = Some(separation.done());
                }
                if !finished.is_empty() {
                    let read = separation.read(stem, finished.clone()).unwrap();
                    seen[stem].push((finished, read));
                }
            }
        }
        for done in reached {
            let done = done.expect("the frame played gets its stems");
            assert!(
                done * 4 < separation.chunks(),
                "the frame played first: {done} of {}",
                separation.chunks()
            );
        }
        let names: Vec<String> = separation.stems().iter().map(|(name, _)| name.to_string()).collect();
        assert_eq!(names, ["vocals", "drums", "bass"]);
        for (stem, reads) in seen.iter().enumerate() {
            assert!(reads.len() > 2, "{} came in parts", names[stem]);
            let whole = separation.read(stem, 0..frames).unwrap();
            for (range, read) in reads {
                let err = read
                    .iter()
                    .zip(&whole[range.start * 2..range.end * 2])
                    .map(|(a, b)| (a - b).abs())
                    .fold(0.0, f32::max);
                assert!(err < 1e-5, "{} at {rate} Hz over {range:?}: {err}", names[stem]);
            }
        }
        let wholes: Vec<Vec<f32>> = (0..3).map(|stem| separation.read(stem, 0..frames).unwrap()).collect();
        assert_eq!(separation.store_through().unwrap().len(), 3);
        for (name, whole) in names.iter().zip(&wholes) {
            let stored = load_stem(&track, name, frames, rate).unwrap().expect("stored");
            let err = (0..frames)
                .map(|i| {
                    let (a, b) = stored.frame(i as isize);
                    let clamp = |v: f32| v.clamp(-2.0, 32767.0 / 16384.0);
                    (a - clamp(whole[2 * i])).abs().max((b - clamp(whole[2 * i + 1])).abs())
                })
                .fold(0.0, f32::max);
            assert!(err <= 1.0 / 32768.0 + 1e-6, "{name} at {rate} Hz stored: {err}");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
