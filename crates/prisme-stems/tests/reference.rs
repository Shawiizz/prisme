//! The separation against the reference implementation (PyTorch), on a tiny random model
//! written by `tools/export_roformer.py --fixture`.

use std::path::Path;

use prisme_stems::{CpuNetwork, GpuNetwork, MaskNetwork, Separator, StemsError, Weights};

fn fixture() -> Weights {
    Weights::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/tiny_roformer.safetensors"))
        .expect("fixture loads")
}

pub fn relative_error(a: &[f32], b: &[f32]) -> f64 {
    let num: f64 = a.iter().zip(b).map(|(a, b)| f64::from(a - b).powi(2)).sum();
    let den: f64 = b.iter().map(|b| f64::from(*b).powi(2)).sum();
    (num / den).sqrt()
}

fn separates_like_the_reference(net: impl MaskNetwork, audio: &[f32], expected: &[f32]) {
    let n = audio.len() / 2;
    let mut separator = Separator::new(net);
    let (mut left, mut right) = (vec![0.0; n], vec![0.0; n]);
    separator.chunk_vocals(&audio[..n], &audio[n..], &mut left, &mut right).unwrap();
    let got: Vec<f32> = left.into_iter().chain(right).collect();
    let err = relative_error(&got, expected);
    assert!(err < 1e-4, "relative error {err:e}");
}

#[test]
fn the_cpu_network_separates_like_the_reference() {
    let weights = fixture();
    let (audio, expected) = (weights.extra["test.audio"].clone(), weights.extra["test.vocals"].clone());
    separates_like_the_reference(CpuNetwork::new(weights), &audio, &expected);
}

fn gpu(weights: Weights) -> Option<GpuNetwork> {
    match GpuNetwork::new_allowing_software(weights) {
        Ok(net) => Some(net),
        Err(StemsError::NoGpu(e)) => {
            eprintln!("skipped: {e}");
            None
        }
        Err(e) => panic!("{e}"),
    }
}

#[test]
fn the_gpu_network_separates_like_the_reference() {
    let weights = fixture();
    let (audio, expected) = (weights.extra["test.audio"].clone(), weights.extra["test.vocals"].clone());
    if let Some(net) = gpu(weights) {
        separates_like_the_reference(net, &audio, &expected);
    }
}

/// Long enough for several query blocks and key tiles along the frames, and rows that do not
/// fill the last matrix tiles.
#[test]
fn the_gpu_network_matches_the_cpu_on_longer_chunks() {
    let weights = fixture();
    let Some(mut gpu) = gpu(weights.clone()) else { return };
    let mut cpu = CpuNetwork::new(weights);
    let frames = 150;
    let features = cpu.geometry().features();
    let bins: Vec<f32> =
        (0..frames * features).map(|i| ((i * 7919 % 2003) as f32 / 1001.5 - 1.0) * 3.0).collect();
    let (mut a, mut b) = (vec![0.0; bins.len()], vec![0.0; bins.len()]);
    cpu.masks(&bins, frames, &mut a).unwrap();
    gpu.masks(&bins, frames, &mut b).unwrap();
    let err = relative_error(&b, &a);
    assert!(err < 1e-4, "relative error {err:e}");
}
