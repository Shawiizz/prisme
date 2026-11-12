//! The separation against the reference implementation (PyTorch), on tiny random models
//! written by `tools/export_roformer.py --fixture`: one of the vocals alone, as the first
//! vocals model is laid out, and one of two stems with skip connections.

use std::path::Path;

use prisme_stems::model::{Geometry, Layer, Linear, Transformer, Values};
use prisme_stems::{CpuNetwork, Gpu, GpuNetwork, GpuOptions, MaskNetwork, Separator, StemsError, Weights};

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

fn separates_like_the_reference(net: impl MaskNetwork, audio: &[f32], expected: &[f32], tolerance: f64) {
    let n = audio.len() / 2;
    let mut separator = Separator::new(net);
    let mut out = vec![[vec![0.0; n], vec![0.0; n]]; separator.stems().len()];
    separator.chunk(&audio[..n], &audio[n..], &mut out).unwrap();
    let got: Vec<f32> = out.into_iter().flatten().flatten().collect();
    let err = relative_error(&got, expected);
    assert!(err < tolerance, "relative error {err:e}");
}

#[test]
fn the_cpu_network_separates_like_the_reference() {
    for weights in [fixture(), stems_fixture()] {
        let (audio, expected) = reference(&weights);
        separates_like_the_reference(CpuNetwork::new(weights), &audio, &expected, 1e-4);
    }
}

#[test]
fn a_file_names_its_stems() {
    assert_eq!(fixture().geometry.stems, ["vocals"]);
    assert_eq!(stems_fixture().geometry.stems, ["drums", "bass"]);
}

fn gpu(weights: Weights, without_matrix_units: bool) -> Option<GpuNetwork> {
    let options = GpuOptions { software: true, without_matrix_units };
    match GpuNetwork::with_options(weights, options) {
        Ok(net) => Some(net),
        Err(StemsError::NoGpu(e)) => {
            eprintln!("skipped: {e}");
            None
        }
        Err(e) => panic!("{e}"),
    }
}

/// The network on the GPU's matrix units, if it has some.
fn matrix_units(weights: Weights) -> Option<GpuNetwork> {
    let net = gpu(weights, false)?;
    if !net.uses_matrix_units() {
        eprintln!("skipped: {} has no matrix units", net.adapter());
        return None;
    }
    Some(net)
}

#[test]
fn the_gpu_network_separates_like_the_reference() {
    for weights in [fixture(), stems_fixture()] {
        let (audio, expected) = reference(&weights);
        let Some(net) = gpu(weights, true) else { return };
        separates_like_the_reference(net, &audio, &expected, F16_TOLERANCE);
    }
}

/// Bins for `frames` frames of a network, the same for every run.
fn bins(geometry: &Geometry, frames: usize) -> Vec<f32> {
    (0..frames * geometry.features()).map(|i| ((i * 7919 % 2003) as f32 / 1001.5 - 1.0) * 3.0).collect()
}

/// Its masks for `bins` of `frames` frames, started and finished.
fn masks_of(net: &mut GpuNetwork, bins: &[f32], frames: usize) -> Vec<f32> {
    let mut out = vec![0.0; net.geometry().mask_len(frames)];
    net.start(bins, frames).unwrap();
    net.finish(&mut out).unwrap();
    out
}

/// Networks on one GPU share the buffers of their activations, as their chunks take turns:
/// interleaved, each gives what it gives alone.
#[test]
fn networks_sharing_a_gpu_give_what_they_give_alone() {
    let options = GpuOptions { software: true, without_matrix_units: true };
    let shared = match Gpu::open(options) {
        Ok(gpu) => gpu,
        Err(StemsError::NoGpu(e)) => return eprintln!("skipped: {e}"),
        Err(e) => panic!("{e}"),
    };
    let frames = 150;
    let weights = [fixture(), stems_fixture()];
    let alone: Vec<Vec<f32>> = weights
        .iter()
        .map(|w| {
            let mut net = gpu(w.clone(), true).expect("a GPU");
            masks_of(&mut net, &bins(&w.geometry, frames), frames)
        })
        .collect();
    // the second grows the buffers the first planned with
    let mut nets = weights.map(|w| GpuNetwork::on(&shared, &w).unwrap());
    let inputs: Vec<Vec<f32>> = nets.iter().map(|n| bins(n.geometry(), frames)).collect();
    for order in [[0, 1], [1, 0], [0, 1]] {
        let mut outs: Vec<Vec<f32>> = nets.iter().map(|n| vec![0.0; n.geometry().mask_len(frames)]).collect();
        for &i in &order {
            nets[i].start(&inputs[i], frames).unwrap();
        }
        for &i in &order {
            nets[i].finish(&mut outs[i]).unwrap();
        }
        for (got, expected) in outs.iter().zip(&alone) {
            assert_eq!(got, expected, "order {order:?}");
        }
    }
}

/// The weights rounded to f16, as the GPU stores them.
fn in_f16(mut weights: Weights) -> Weights {
    for l in weights.linears_mut() {
        l.weight = Values::F16(l.weight.to_f16().into_owned());
    }
    weights
}

/// Masks of the GPU and of the CPU for the same bins, long enough for several query blocks
/// and key tiles along the frames, and rows that do not fill the last matrix tiles.
fn gpu_and_cpu_masks(gpu: &mut GpuNetwork, weights: Weights) -> (Vec<f32>, Vec<f32>) {
    let mut cpu = CpuNetwork::new(weights);
    let frames = 150;
    let features = cpu.geometry().features();
    let bins: Vec<f32> =
        (0..frames * features).map(|i| ((i * 7919 % 2003) as f32 / 1001.5 - 1.0) * 3.0).collect();
    let len = cpu.geometry().mask_len(frames);
    let (mut a, mut b) = (vec![0.0; len], vec![0.0; len]);
    cpu.masks(&bins, frames, &mut a).unwrap();
    gpu.masks(&bins, frames, &mut b).unwrap();
    (b, a)
}

#[test]
fn the_gpu_network_matches_the_cpu_on_longer_chunks() {
    for weights in [fixture(), stems_fixture()].map(in_f16) {
        let Some(mut gpu) = gpu(weights.clone(), true) else { return };
        // a GPU is held to the CPU; Windows' software renderer, whose arithmetic on the
        // shaders DXC compiles is looser (1.2e-3 here), to the tolerance of the f16 path
        let tolerance = if gpu.on_software_renderer() { F16_TOLERANCE } else { 1e-4 };
        let (got, expected) = gpu_and_cpu_masks(&mut gpu, weights);
        let err = relative_error(&got, &expected);
        assert!(err < tolerance, "relative error {err:e}");
    }
}

/// The weights rounded to f16, and on the matrix units the activations they read too: within
/// a few thousandths of the reference.
const F16_TOLERANCE: f64 = 5e-3;

#[test]
fn the_matrix_units_match_the_cpu() {
    for weights in [fixture(), stems_fixture(), wide_heads(&fixture())] {
        let Some(mut gpu) = matrix_units(weights.clone()) else { return };
        let (got, expected) = gpu_and_cpu_masks(&mut gpu, weights);
        let err = relative_error(&got, &expected);
        assert!(err < F16_TOLERANCE, "relative error {err:e}");
    }
}

#[test]
fn the_matrix_units_separate_like_the_reference() {
    for weights in [fixture(), stems_fixture()] {
        let (audio, expected) = reference(&weights);
        let Some(net) = matrix_units(weights) else { return };
        separates_like_the_reference(net, &audio, &expected, F16_TOLERANCE);
    }
}

/// The fixture's bands under a network with the separation models' heads, which the
/// attention on the matrix units takes (4 heads of 64 values): weights from a fixed sequence,
/// rounded to f16 as the GPU stores them.
fn wide_heads(base: &Weights) -> Weights {
    let mut state = 0x2545_f491_4f6c_dd1du64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 40) as f32 / (1u64 << 24) as f32 - 0.5
    };
    let mut linear = |out: usize, inp: usize| {
        let scale = 2.0 / (inp as f32).sqrt();
        Linear {
            weight: Values::F16((0..out * inp).map(|_| half::f16::from_f32(next() * scale)).collect()),
            bias: Some((0..out).map(|_| next() * 0.2).collect()),
            out,
            inp,
        }
    };
    let mut config = base.geometry.config.clone();
    (config.dim, config.heads, config.dim_head, config.depth) = (64, 4, 64, 1);
    let (dim, hidden) = (config.dim, 2 * config.dim);
    let band_proj = base.geometry.bands.iter().map(|b| linear(dim, b.width)).collect();
    let transformers = (0..2)
        .map(|_| Transformer {
            layers: vec![Layer {
                qkvg: linear(config.qkvg_width(), dim),
                out: linear(dim, config.inner_dim()),
                ff1: linear(hidden, dim),
                ff2: linear(dim, hidden),
            }],
            norm: (0..dim).map(|i| 6.0 + (i % 5) as f32).collect(),
        })
        .collect();
    let masks = base
        .geometry
        .stems
        .iter()
        .map(|_| {
            let bands = base.geometry.bands.iter();
            bands
                .map(|b| vec![linear(hidden, dim), linear(hidden, hidden), linear(2 * b.width, hidden)])
                .collect()
        })
        .collect();
    let rotary: Vec<f32> = (0..config.dim_head / 2).map(|i| 10_000f32.powf(-2.0 * i as f32 / 64.0)).collect();
    Weights {
        geometry: Geometry { config, ..base.geometry.clone() },
        rotary_time: rotary.clone(),
        rotary_freq: rotary,
        band_proj,
        transformers,
        masks,
        extra: Default::default(),
    }
}
