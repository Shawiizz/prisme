//! The network on the CPU: the fallback without a usable GPU, and the reference the GPU
//! implementation is tested against.
//!
//! Activations are kept as one row of `dim` features per token, tokens ordered frame by
//! frame then band by band: the time transformer attends along frames within a band, the
//! frequency transformer along bands within a frame, both by striding over the same rows.

use gemm::Parallelism;
use rayon::prelude::*;

use crate::model::{Config, Geometry, Linear, Weights};
use crate::network::{Axis, MaskNetwork};
use crate::StemsError;

pub struct CpuNetwork {
    weights: Weights,
    /// The threads it computes on: rayon's global ones unless given.
    pool: Option<rayon::ThreadPool>,
    /// The bins of the chunk started, and its frames.
    started: Option<(Vec<f32>, usize)>,
}

impl CpuNetwork {
    /// The weights stay as stored (f16 for the models: half the memory); each layer's are
    /// widened to f32 as it computes, a cost its rows make small.
    pub fn new(weights: Weights) -> Self {
        Self { weights, pool: None, started: None }
    }

    /// Computing on `pool`'s threads: fewer than the cores, at a low priority, so that the
    /// audio and the interface keep theirs.
    pub fn with_pool(weights: Weights, pool: rayon::ThreadPool) -> Self {
        Self { pool: Some(pool), ..Self::new(weights) }
    }
}

impl MaskNetwork for CpuNetwork {
    fn geometry(&self) -> &Geometry {
        &self.weights.geometry
    }

    fn start(&mut self, bins: &[f32], frames: usize) -> Result<(), StemsError> {
        self.started = Some((bins.to_vec(), frames));
        Ok(())
    }

    fn finish(&mut self, out: &mut [f32]) -> Result<(), StemsError> {
        let (bins, frames) = self.started.take().expect("a chunk was started");
        match &self.pool {
            Some(pool) => pool.install(|| self.compute(&bins, frames, out)),
            None => self.compute(&bins, frames, out),
        }
        Ok(())
    }
}

impl CpuNetwork {
    fn compute(&self, bins: &[f32], frames: usize, out: &mut [f32]) {
        let w = &self.weights;
        let g = &w.geometry;
        let features = g.features();
        assert!(bins.len() == frames * features, "bins hold `frames` rows of features");
        assert!(out.len() == g.mask_len(frames), "masks hold `frames` rows per stem");
        let c = &g.config;
        let bands = g.bands.len();
        let tokens = frames * bands;

        let mut x = vec![0.0; tokens * c.dim];
        for (b, (band, proj)) in g.bands.iter().zip(&w.band_proj).enumerate() {
            let mut normed: Vec<f32> = (0..frames)
                .flat_map(|t| bins[t * features + band.offset..][..band.width].iter().copied())
                .collect();
            normed.chunks_exact_mut(band.width).for_each(rms_normalize);
            linear(proj, frames, &normed, band.width, &mut x[b * c.dim..], bands * c.dim, false);
        }

        let mut scratch = Scratch::new(tokens, c);
        let mut earlier = if c.skip { vec![0.0; x.len()] } else { Vec::new() };
        for (i, tr) in w.transformers.iter().enumerate() {
            let axis = if i % 2 == 0 { Axis::Time } else { Axis::Freq };
            if c.skip && axis == Axis::Time {
                x.par_iter_mut().zip(&earlier).for_each(|(x, e)| *x += e);
            }
            let rotary = Rotary::new(
                if axis == Axis::Time { &w.rotary_time } else { &w.rotary_freq },
                axis.len(frames, bands),
            );
            for layer in &tr.layers {
                scratch.normed.copy_from_slice(&x);
                scratch.normed.chunks_exact_mut(c.dim).for_each(rms_normalize);
                let width = c.qkvg_width();
                linear(&layer.qkvg, tokens, &scratch.normed, c.dim, &mut scratch.qkvg, width, false);
                attention(&mut scratch, c, axis, frames, bands, &rotary);
                linear(&layer.out, tokens, &scratch.attended, c.inner_dim(), &mut x, c.dim, true);

                scratch.normed.copy_from_slice(&x);
                scratch.normed.chunks_exact_mut(c.dim).for_each(rms_normalize);
                let hidden = layer.ff1.out;
                scratch.hidden.resize(tokens * hidden, 0.0);
                linear(&layer.ff1, tokens, &scratch.normed, c.dim, &mut scratch.hidden, hidden, false);
                scratch.hidden.par_iter_mut().for_each(|v| *v = gelu(*v));
                linear(&layer.ff2, tokens, &scratch.hidden, hidden, &mut x, c.dim, true);
            }
            x.par_chunks_exact_mut(c.dim).for_each(|row| {
                rms_normalize(row);
                row.iter_mut().zip(&tr.norm).for_each(|(v, g)| *v *= g);
            });
            if c.skip && axis == Axis::Freq {
                earlier.par_iter_mut().zip(&x).for_each(|(e, x)| *e += x);
            }
        }

        let size = frames * features;
        for (stem, out) in w.masks.iter().zip(out.chunks_exact_mut(size)) {
            stem_masks(g, stem, &x, frames, out);
        }
    }
}

/// One stem's masks from the transformers' output.
fn stem_masks(g: &Geometry, stem: &[Vec<Linear>], x: &[f32], frames: usize, out: &mut [f32]) {
    let dim = g.config.dim;
    let bands = g.bands.len();
    let features = g.features();
    let per_band: Vec<Vec<f32>> = g
        .bands
        .par_iter()
        .zip(stem)
        .enumerate()
        .map(|(b, (band, mlp))| {
            let mut input: Vec<f32> =
                (0..frames).flat_map(|t| x[(t * bands + b) * dim..][..dim].iter().copied()).collect();
            for (k, l) in mlp.iter().enumerate() {
                let mut y = vec![0.0; frames * l.out];
                linear_with(l, frames, &input, l.inp, &mut y, l.out, false, Parallelism::None);
                if k + 1 < mlp.len() {
                    y.iter_mut().for_each(|v| *v = v.tanh());
                }
                input = y;
            }
            input
                .chunks_exact(band.width * 2)
                .flat_map(|row| {
                    let (a, gate) = row.split_at(band.width);
                    a.iter().zip(gate).map(|(a, g)| a * sigmoid(*g))
                })
                .collect()
        })
        .collect();
    for (band, mask) in g.bands.iter().zip(per_band) {
        for (t, row) in mask.chunks_exact(band.width).enumerate() {
            out[t * features + band.offset..][..band.width].copy_from_slice(row);
        }
    }
}

struct Scratch {
    normed: Vec<f32>,
    qkvg: Vec<f32>,
    attended: Vec<f32>,
    hidden: Vec<f32>,
}

impl Scratch {
    fn new(tokens: usize, c: &Config) -> Self {
        Self {
            normed: vec![0.0; tokens * c.dim],
            qkvg: vec![0.0; tokens * c.qkvg_width()],
            attended: vec![0.0; tokens * c.inner_dim()],
            hidden: Vec::new(),
        }
    }
}

/// Rotary position angles, `cos` and `sin` per position and frequency pair.
struct Rotary {
    cos: Vec<f32>,
    sin: Vec<f32>,
    pairs: usize,
}

impl Rotary {
    fn new(freqs: &[f32], positions: usize) -> Self {
        let angles = (0..positions).flat_map(|p| freqs.iter().map(move |f| p as f32 * f));
        let (cos, sin) = angles.map(|a| (a.cos(), a.sin())).unzip();
        Self { cos, sin, pairs: freqs.len() }
    }

    /// Rotates each pair `(x[2i], x[2i+1])` of a head by its angle at `pos`.
    fn apply(&self, head: &mut [f32], pos: usize) {
        let cos = &self.cos[pos * self.pairs..][..self.pairs];
        let sin = &self.sin[pos * self.pairs..][..self.pairs];
        for ((pair, c), s) in head.chunks_exact_mut(2).zip(cos).zip(sin) {
            let (a, b) = (pair[0], pair[1]);
            pair[0] = a * c - b * s;
            pair[1] = b * c + a * s;
        }
    }
}

/// Gated multi-head attention along `axis`: rotary queries and keys, softmax, each head's
/// output scaled by the sigmoid of its gate.
fn attention(s: &mut Scratch, c: &Config, axis: Axis, frames: usize, bands: usize, rotary: &Rotary) {
    let width = c.qkvg_width();
    let (inner, dh, heads) = (c.inner_dim(), c.dim_head, c.heads);
    s.qkvg.par_chunks_exact_mut(width).enumerate().for_each(|(token, row)| {
        let pos = axis.position(token, bands);
        for h in 0..2 * heads {
            rotary.apply(&mut row[h * dh..][..dh], pos);
        }
    });

    let len = axis.len(frames, bands);
    let groups = axis.groups(frames, bands);
    let qkvg = &s.qkvg;
    let outputs: Vec<Vec<f32>> = (0..groups)
        .into_par_iter()
        .map(|g| {
            let token = |i: usize| axis.token(g, i, bands);
            let stride = axis.stride(bands) as isize;
            let mut out = vec![0.0; len * inner];
            let mut scores = vec![0.0; len * len];
            for h in 0..heads {
                let q = qkvg[token(0) * width + h * dh..].as_ptr();
                let k = qkvg[token(0) * width + inner + h * dh..].as_ptr();
                let v = qkvg[token(0) * width + 2 * inner + h * dh..].as_ptr();
                let row = stride * width as isize;
                // SAFETY: every row read is a token of this group, inside `qkvg`.
                unsafe {
                    gemm::gemm(
                        len,
                        len,
                        dh,
                        scores.as_mut_ptr(),
                        1,
                        len as isize,
                        false,
                        q,
                        1,
                        row,
                        k,
                        row,
                        1,
                        0.0,
                        1.0,
                        false,
                        false,
                        false,
                        Parallelism::None,
                    );
                }
                scores.chunks_exact_mut(len).for_each(softmax);
                // SAFETY: as above; `out` holds `len` rows of `inner`.
                unsafe {
                    gemm::gemm(
                        len,
                        dh,
                        len,
                        out.as_mut_ptr().add(h * dh),
                        1,
                        inner as isize,
                        false,
                        scores.as_ptr(),
                        1,
                        len as isize,
                        v,
                        1,
                        row,
                        0.0,
                        1.0,
                        false,
                        false,
                        false,
                        Parallelism::None,
                    );
                }
                for i in 0..len {
                    let gate = sigmoid(qkvg[token(i) * width + 3 * inner + h]);
                    out[i * inner + h * dh..][..dh].iter_mut().for_each(|o| *o *= gate);
                }
            }
            out
        })
        .collect();
    for (g, out) in outputs.iter().enumerate() {
        for (i, row) in out.chunks_exact(inner).enumerate() {
            let t = axis.token(g, i, bands);
            s.attended[t * inner..][..inner].copy_from_slice(row);
        }
    }
}

/// `out[r * ldo ..][..l.out] (+)= input[r * ldi ..][..l.inp] · weightᵀ + bias`, for `rows` rows.
fn linear(l: &Linear, rows: usize, input: &[f32], ldi: usize, out: &mut [f32], ldo: usize, accumulate: bool) {
    linear_with(l, rows, input, ldi, out, ldo, accumulate, Parallelism::Rayon(0));
}

#[allow(clippy::too_many_arguments)]
fn linear_with(
    l: &Linear,
    rows: usize,
    input: &[f32],
    ldi: usize,
    out: &mut [f32],
    ldo: usize,
    accumulate: bool,
    parallelism: Parallelism,
) {
    assert!(rows == 0 || (input.len() >= (rows - 1) * ldi + l.inp && out.len() >= (rows - 1) * ldo + l.out));
    let weight = l.weight.to_f32();
    // SAFETY: the bounds were checked above; the weight is `out` rows of `inp` values.
    unsafe {
        gemm::gemm(
            rows,
            l.out,
            l.inp,
            out.as_mut_ptr(),
            1,
            ldo as isize,
            accumulate,
            input.as_ptr(),
            1,
            ldi as isize,
            weight.as_ptr(),
            l.inp as isize,
            1,
            1.0,
            1.0,
            false,
            false,
            false,
            parallelism,
        );
    }
    if let Some(bias) = &l.bias {
        for r in 0..rows {
            out[r * ldo..][..l.out].iter_mut().zip(bias).for_each(|(o, b)| *o += b);
        }
    }
}

/// `x / max(‖x‖, 1e-12)`: the RMS norm without its gain (`F.normalize`).
pub(crate) fn rms_normalize(x: &mut [f32]) {
    let norm = x.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-12);
    x.iter_mut().for_each(|v| *v /= norm);
}

fn softmax(row: &mut [f32]) {
    let max = row.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let mut sum = 0.0;
    for v in row.iter_mut() {
        *v = (*v - max).exp();
        sum += *v;
    }
    row.iter_mut().for_each(|v| *v /= sum);
}

pub(crate) fn gelu(x: f32) -> f32 {
    0.5 * x * (1.0 + libm::erff(x * std::f32::consts::FRAC_1_SQRT_2))
}

pub(crate) fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}
