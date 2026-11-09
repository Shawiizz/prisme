//! The separation against the reference implementation (PyTorch), on tiny random models
//! written by `tools/export_roformer.py --fixture`: one of the vocals alone, as the first
//! vocals model is laid out, and one of two stems with skip connections.

use std::path::Path;

use prisme_stems::{CpuNetwork, GpuNetwork, MaskNetwork, Separator, StemsError, Weights};

fn load(name: &str) -> Weights {
    Weights::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data").join(name))
        .expect("fixture loads")
}

fn fixture() -> Weights {
    load("tiny_roformer.safetensors")
}

fn stems_fixture() -> Weights {
    load("tiny_roformer_stems.safetensors")
}

/// The fixture's input and the reference's stems, one after the other.
fn reference(weights: &Weights) -> (Vec<f32>, Vec<f32>) {
    let expected =
        weights.extra.get("test.stems").or(weights.extra.get("test.vocals")).expect("reference output");
    (weights.extra["test.audio"].clone(), expected.clone())
}

pub fn relative_error(a: &[f32], b: &[f32]) -> f64 {
    let num: f64 = a.iter().zip(b).map(|(a, b)| f64::from(a - b).powi(2)).sum();
    let den: f64 = b.iter().map(|b| f64::from(*b).powi(2)).sum();
    (num / den).sqrt()
}

fn separates_like_the_reference(net: impl MaskNetwork, audio: &[f32], expected: &[f32]) {
    let n = audio.len() / 2;
    let mut separator = Separator::new(net);
    let mut out = vec![[vec![0.0; n], vec![0.0; n]]; separator.stems().len()];
    separator.chunk(&audio[..n], &audio[n..], &mut out).unwrap();
    let got: Vec<f32> = out.into_iter().flatten().flatten().collect();
    let err = relative_error(&got, expected);
    assert!(err < 1e-4, "relative error {err:e}");
}

#[test]
fn the_cpu_network_separates_like_the_reference() {
    for weights in [fixture(), stems_fixture()] {
        let (audio, expected) = reference(&weights);
        separates_like_the_reference(CpuNetwork::new(weights), &audio, &expected);
    }
}

#[test]
fn a_file_names_its_stems() {
    assert_eq!(fixture().geometry.stems, ["vocals"]);
    assert_eq!(stems_fixture().geometry.stems, ["drums", "bass"]);
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
    for weights in [fixture(), stems_fixture()] {
        let (audio, expected) = reference(&weights);
        let Some(net) = gpu(weights) else { return };
        separates_like_the_reference(net, &audio, &expected);
    }
}

/// Long enough for several query blocks and key tiles along the frames, and rows that do not
/// fill the last matrix tiles.
#[test]
fn the_gpu_network_matches_the_cpu_on_longer_chunks() {
    for weights in [fixture(), stems_fixture()] {
        let Some(mut gpu) = gpu(weights.clone()) else { return };
        let mut cpu = CpuNetwork::new(weights);
        let frames = 150;
        let features = cpu.geometry().features();
        let bins: Vec<f32> =
            (0..frames * features).map(|i| ((i * 7919 % 2003) as f32 / 1001.5 - 1.0) * 3.0).collect();
        let len = cpu.geometry().mask_len(frames);
        let (mut a, mut b) = (vec![0.0; len], vec![0.0; len]);
        cpu.masks(&bins, frames, &mut a).unwrap();
        gpu.masks(&bins, frames, &mut b).unwrap();
        let err = relative_error(&b, &a);
        assert!(err < 1e-4, "relative error {err:e}");
    }
}
