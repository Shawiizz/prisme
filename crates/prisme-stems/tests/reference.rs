//! The separation against the reference implementation (PyTorch), on tiny random models
//! written by `tools/export_roformer.py --fixture`: one of the vocals alone, as the first
//! vocals model is laid out, and one of two stems with skip connections; and by
//! `tools/export_bs_roformer.py --fixture`: a BS-RoFormer with grouped heads and registers, and
//! one of the V2.5 models, its frames grouped in tokens, its stems queries.

use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use prisme_stems::gpu::{Attention, Pacing};
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

fn bs_fixture() -> Weights {
    load("tiny_bs_roformer.safetensors")
}

fn v25_fixture() -> Weights {
    load("tiny_bs_roformer_v25.safetensors")
}

/// The V2.5 fixture without its stem queries: each stem has estimators of its own, and the
/// frames its tokens give back are the same for every stem.
fn grouped_fixture() -> Weights {
    let mut weights = v25_fixture();
    weights.stem_queries = None;
    weights.geometry.config.stem_queries = false;
    let estimators = weights.masks.remove(0);
    weights.masks = (0..weights.geometry.stems.len())
        .map(|s| {
            let mut own = estimators.clone();
            for bias in own.iter_mut().flatten().filter_map(|l| l.bias.as_mut()) {
                bias.iter_mut().for_each(|b| *b += 0.25 * s as f32);
            }
            own
        })
        .collect();
    weights
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
    for weights in [fixture(), stems_fixture(), bs_fixture(), v25_fixture()] {
        let (audio, expected) = reference(&weights);
        separates_like_the_reference(CpuNetwork::new(weights), &audio, &expected, 1e-4);
    }
}

#[test]
fn a_file_names_its_stems() {
    assert_eq!(fixture().geometry.stems, ["vocals"]);
    assert_eq!(stems_fixture().geometry.stems, ["drums", "bass"]);
    assert_eq!(bs_fixture().geometry.stems, ["drums", "bass"]);
}

fn gpu(weights: Weights, without_matrix_units: bool) -> Option<GpuNetwork> {
    shader_cores_or_units(weights, GpuOptions { without_matrix_units, ..SOFTWARE_TOO })
}

/// Accepts a software renderer, where there is no GPU.
const SOFTWARE_TOO: GpuOptions = GpuOptions {
    software: true,
    without_matrix_units: false,
    max_binding: None,
    profile: false,
    pacing: Some(Pacing::Whole),
    attention: None,
};

/// The GPU, opened as `options`; `None` without one.
///
/// Windows' software renderer (WARP), on which the tests run where there is no GPU, takes
/// minutes to compile `attention.wgsl` for each shape of heads (6 on 4 cores), and compiles it
/// again once no pipeline uses it. There, the tests take the BS-RoFormers, whose heads all
/// share one, compiled once for them all (see [`compiled_once`]).
fn open(options: GpuOptions) -> Option<Arc<Gpu>> {
    match Gpu::open(options) {
        Ok(gpu) => {
            if gpu.is_software_renderer() {
                compiled_once();
            }
            Some(gpu)
        }
        Err(StemsError::NoGpu(e)) => {
            eprintln!("skipped: {e}");
            None
        }
        Err(e) => panic!("{e}"),
    }
}

/// A network of the BS-RoFormer fixture run once, kept to the end of the tests for WARP to
/// keep its shaders: the first test to ask compiles them, the others wait for them.
fn compiled_once() {
    static KEPT: OnceLock<Mutex<GpuNetwork>> = OnceLock::new();
    KEPT.get_or_init(|| {
        let weights = bs_fixture();
        let options = GpuOptions { without_matrix_units: true, ..SOFTWARE_TOO };
        let mut net = GpuNetwork::with_options(weights.clone(), options).unwrap();
        let frames = weights.geometry.config.chunk_frames;
        masks_of(&mut net, &bins(&weights.geometry, frames), frames);
        Mutex::new(net)
    });
}

/// The fixtures, but on a software renderer the BS-RoFormers alone (see [`open`]).
fn fixtures(software: bool) -> Vec<Weights> {
    if software {
        vec![bs_fixture(), v25_fixture()]
    } else {
        vec![fixture(), stems_fixture(), bs_fixture(), v25_fixture()]
    }
}

fn shader_cores_or_units(weights: Weights, options: GpuOptions) -> Option<GpuNetwork> {
    match GpuNetwork::with_options(weights, options) {
        Ok(net) => Some(net),
        Err(StemsError::NoGpu(e)) => {
            eprintln!("skipped: {e}");
            None
        }
        Err(e) => panic!("{e}"),
    }
}

/// The network on the shader cores, its attention as `attention`.
fn shader_cores(weights: Weights, attention: Attention) -> Option<GpuNetwork> {
    let options = GpuOptions { without_matrix_units: true, attention: Some(attention), ..SOFTWARE_TOO };
    shader_cores_or_units(weights, options)
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
    let Some(gpu) = open(GpuOptions { without_matrix_units: true, ..SOFTWARE_TOO }) else { return };
    for weights in fixtures(gpu.is_software_renderer()) {
        let (audio, expected) = reference(&weights);
        let net = GpuNetwork::on(&gpu, &weights).unwrap();
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
    let Some(shared) = open(GpuOptions { without_matrix_units: true, ..SOFTWARE_TOO }) else { return };
    let frames = 150;
    let weights = fixtures(shared.is_software_renderer());
    let alone: Vec<Vec<f32>> = weights
        .iter()
        .map(|w| {
            let mut net = gpu(w.clone(), true).expect("a GPU");
            masks_of(&mut net, &bins(&w.geometry, frames), frames)
        })
        .collect();
    // the later ones grow the buffers the first planned with
    let mut nets: Vec<GpuNetwork> = weights.iter().map(|w| GpuNetwork::on(&shared, w).unwrap()).collect();
    let inputs: Vec<Vec<f32>> = nets.iter().map(|n| bins(n.geometry(), frames)).collect();
    // each first and each last, and in between in another order
    let n = nets.len();
    let orders: [Vec<usize>; 3] =
        [(0..n).collect(), (0..n).rev().collect(), (1..n).step_by(2).chain((0..n).step_by(2)).collect()];
    for order in orders {
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

/// The work whole, in packets, and in packets leaving the GPU half its time.
#[test]
fn every_pacing_separates_like_the_reference() {
    for pacing in [Pacing::Whole, Pacing::Packets, Pacing::Share(0.5)] {
        for without_matrix_units in [true, false] {
            let options = GpuOptions { without_matrix_units, pacing: Some(pacing), ..SOFTWARE_TOO };
            let Some(gpu) = open(options) else { return };
            for weights in fixtures(gpu.is_software_renderer()) {
                let (audio, expected) = reference(&weights);
                let net = GpuNetwork::on(&gpu, &weights).unwrap();
                separates_like_the_reference(net, &audio, &expected, F16_TOLERANCE);
            }
        }
    }
}

#[test]
fn the_gpu_network_matches_the_cpu_on_longer_chunks() {
    let Some(gpu) = open(GpuOptions { without_matrix_units: true, ..SOFTWARE_TOO }) else { return };
    let software = gpu.is_software_renderer();
    // a GPU is held to the CPU; Windows' software renderer, whose arithmetic on the shaders DXC
    // compiles is looser (1.2e-3 here), to the tolerance of the f16 path
    let tolerance = if software { F16_TOLERANCE } else { 1e-4 };
    for weights in fixtures(software).into_iter().chain([grouped_fixture()]).map(in_f16) {
        let mut net = GpuNetwork::on(&gpu, &weights).unwrap();
        let (got, expected) = gpu_and_cpu_masks(&mut net, weights);
        let err = relative_error(&got, &expected);
        assert!(err < tolerance, "relative error {err:e}");
    }
}

/// The weights rounded to f16, and on the matrix units the activations they read too: within
/// a few thousandths of the reference.
const F16_TOLERANCE: f64 = 5e-3;

/// The heads of the separation models: of 64 values, gated, each with its own keys and values,
/// as the Mel-Band RoFormers'; of 32, without gates, pairs of them sharing their keys and
/// values, as the Mini-BS-RoFormer-18M's; of 64 so shared, without gates, as the V2.5's.
const MEL_BAND_HEADS: (usize, usize, bool) = (64, 4, true);
const BS_HEADS: (usize, usize, bool) = (32, 2, false);
const V25_HEADS: (usize, usize, bool) = (64, 2, false);

/// Both attentions of the shader cores, whichever their heads would take: on the BS-RoFormer
/// fixtures (heads of 8 values sharing their keys and values, gated feed-forwards, registers;
/// for the V2.5's, through SiLU, frames grouped in tokens and stem queries) and on the heads of
/// the separation models; on a software renderer, the attention in one pass on the fixtures
/// alone (see [`open`]).
#[test]
fn both_attentions_of_the_shader_cores_match_the_cpu() {
    let fixtures = [in_f16(bs_fixture()), in_f16(v25_fixture())];
    let wide = [MEL_BAND_HEADS, BS_HEADS, V25_HEADS].map(|heads| wide_heads(&fixture(), heads));
    for attention in [Attention::OnePass, Attention::Products] {
        let options = GpuOptions { without_matrix_units: true, attention: Some(attention), ..SOFTWARE_TOO };
        let Some(gpu) = open(options) else { return };
        let software = gpu.is_software_renderer();
        let wide = if software && attention == Attention::OnePass { &wide[..0] } else { &wide[..] };
        let tolerance = if software { F16_TOLERANCE } else { 1e-4 };
        for weights in fixtures.iter().chain(wide) {
            let mut net = GpuNetwork::on(&gpu, weights).unwrap();
            let (got, expected) = gpu_and_cpu_masks(&mut net, weights.clone());
            let err = relative_error(&got, &expected);
            assert!(err < tolerance, "{attention:?}: relative error {err:e}");
        }
    }
}

#[test]
fn the_attention_as_products_separates_like_the_reference() {
    for weights in [bs_fixture(), v25_fixture()] {
        let (audio, expected) = reference(&weights);
        let Some(net) = shader_cores(weights, Attention::Products) else { return };
        separates_like_the_reference(net, &audio, &expected, F16_TOLERANCE);
    }
}

#[test]
fn the_matrix_units_match_the_cpu() {
    let wide = [MEL_BAND_HEADS, BS_HEADS, V25_HEADS].map(|heads| wide_heads(&fixture(), heads));
    let all = [fixture(), stems_fixture(), bs_fixture(), v25_fixture(), grouped_fixture()];
    for weights in all.into_iter().chain(wide) {
        let Some(mut gpu) = matrix_units(weights.clone()) else { return };
        let (got, expected) = gpu_and_cpu_masks(&mut gpu, weights);
        let err = relative_error(&got, &expected);
        assert!(err < F16_TOLERANCE, "relative error {err:e}");
    }
}

#[test]
fn the_matrix_units_separate_like_the_reference() {
    for weights in [fixture(), stems_fixture(), bs_fixture(), v25_fixture()] {
        let (audio, expected) = reference(&weights);
        let Some(net) = matrix_units(weights) else { return };
        separates_like_the_reference(net, &audio, &expected, F16_TOLERANCE);
    }
}

/// The fixture's bands under a network of 4 heads shaped as `heads` (as [`MEL_BAND_HEADS`]),
/// which the attention on the matrix units takes: weights from a fixed sequence, rounded to
/// f16 as the GPU stores them.
fn wide_heads(base: &Weights, (dim_head, kv_heads, gates): (usize, usize, bool)) -> Weights {
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
    (config.dim, config.heads, config.depth) = (64, 4, 1);
    (config.dim_head, config.kv_heads, config.gates) = (dim_head, kv_heads, gates);
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
            norm: Some((0..dim).map(|i| 6.0 + (i % 5) as f32).collect()),
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
    let rotary: Vec<f32> =
        (0..dim_head / 2).map(|i| 10_000f32.powf(-2.0 * i as f32 / dim_head as f32)).collect();
    Weights {
        geometry: Geometry { config, ..base.geometry.clone() },
        rotary_time: rotary.clone(),
        rotary_freq: rotary,
        band_proj,
        grouping: None,
        transformers,
        final_norm: None,
        registers: Vec::new(),
        stem_queries: None,
        masks,
        extra: Default::default(),
    }
}
