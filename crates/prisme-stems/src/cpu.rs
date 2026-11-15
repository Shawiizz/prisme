//! The network on the CPU: the fallback without a usable GPU, and the reference the GPU
//! implementation is tested against.
//!
//! Activations are kept as one row of `dim` features per token, tokens ordered frame by
//! frame then band by band: the time transformer attends along frames within a band, the
//! frequency transformer along bands within a frame, both by striding over the same rows.

use std::borrow::Cow;

use gemm::Parallelism;
use rayon::prelude::*;

use crate::model::{Band, Config, FeedForward, Geometry, Linear, Pool, Weights};
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
        let eps = c.norm_eps;
        let bands = g.bands.len();
        let token_frames = c.token_frames(frames);
        let (seq_frames, seq_bands) = (c.seq_frames(frames), g.seq_bands());
        let tokens = seq_frames * seq_bands;

        let mut x = vec![0.0; tokens * c.dim];
        for (r, register) in w.registers.chunks_exact(c.dim).enumerate() {
            x[g.register_token(frames, r) * c.dim..][..c.dim].copy_from_slice(register);
        }
        let band_frames = |band: &Band| -> Vec<f32> {
            let mut normed: Vec<f32> = (0..frames)
                .flat_map(|t| bins[t * features + band.offset..][..band.width].iter().copied())
                .collect();
            normed.chunks_exact_mut(band.width).for_each(|row| rms_normalize(row, eps));
            normed
        };
        match &w.grouping {
            None => {
                for (b, (band, proj)) in g.bands.iter().zip(&w.band_proj).enumerate() {
                    let normed = band_frames(band);
                    linear(proj, frames, &normed, band.width, &mut x[b * c.dim..], seq_bands * c.dim, false);
                }
            }
            Some(grouping) => {
                let band_dim = w.band_dim();
                let per_band: Vec<Vec<f32>> = g
                    .bands
                    .par_iter()
                    .zip(&w.band_proj)
                    .map(|(band, proj)| {
                        // the frames padding the last token stay silent
                        let mut split = vec![0.0; token_frames * c.group * band_dim];
                        let normed = band_frames(band);
                        linear_with(
                            proj,
                            frames,
                            &normed,
                            band.width,
                            &mut split,
                            band_dim,
                            false,
                            Parallelism::None,
                        );
                        split.chunks_exact_mut(band_dim).for_each(|row| rms_normalize(row, eps));
                        feed_forward(&grouping.input, token_frames, &split, c.silu)
                    })
                    .collect();
                for (b, band_tokens) in per_band.iter().enumerate() {
                    for (t, token) in band_tokens.chunks_exact(c.dim).enumerate() {
                        x[(t * seq_bands + b) * c.dim..][..c.dim].copy_from_slice(token);
                    }
                }
            }
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
                axis.len(token_frames, bands),
                axis.len(seq_frames, seq_bands),
            );
            for layer in &tr.layers {
                scratch.normed.copy_from_slice(&x);
                scratch.normed.chunks_exact_mut(c.dim).for_each(|row| rms_normalize(row, eps));
                let width = c.qkvg_width();
                linear(&layer.qkvg, tokens, &scratch.normed, c.dim, &mut scratch.qkvg, width, false);
                attention(&mut scratch, c, axis, seq_frames, seq_bands, &rotary);
                linear(&layer.out, tokens, &scratch.attended, c.inner_dim(), &mut x, c.dim, true);

                scratch.normed.copy_from_slice(&x);
                scratch.normed.chunks_exact_mut(c.dim).for_each(|row| rms_normalize(row, eps));
                let hidden = layer.ff1.out;
                scratch.hidden.resize(tokens * hidden, 0.0);
                linear(&layer.ff1, tokens, &scratch.normed, c.dim, &mut scratch.hidden, hidden, false);
                if c.ff_glu {
                    gated(&mut scratch.hidden, hidden, c.silu);
                } else {
                    scratch.hidden.par_iter_mut().for_each(|v| *v = gelu(*v));
                }
                linear(&layer.ff2, tokens, &scratch.hidden, hidden, &mut x, c.dim, true);
            }
            if let Some(norm) = &tr.norm {
                x.par_chunks_exact_mut(c.dim).for_each(|row| scale_normalize(row, norm, eps));
            }
            if c.skip && axis == Axis::Freq {
                earlier.par_iter_mut().zip(&x).for_each(|(e, x)| *e += x);
            }
        }
        if let Some(norm) = &w.final_norm {
            x.par_chunks_exact_mut(c.dim).for_each(|row| scale_normalize(row, norm, eps));
        }

        let stems = w.stem_queries.as_ref().map(|queries| {
            let pooled = pool(&queries.bands, c, &x, token_frames, bands, seq_bands);
            pool(&queries.tokens, c, &pooled, 1, token_frames, 0)
        });
        // the rows band `b`'s estimator reads for a stem: its tokens, or the frames they give back
        let band_input = |b: usize, stem: Option<&[f32]>| -> Vec<f32> {
            let mut tokens: Vec<f32> = (0..token_frames)
                .flat_map(|t| x[(t * seq_bands + b) * c.dim..][..c.dim].iter().copied())
                .collect();
            let Some(grouping) = &w.grouping else { return tokens };
            if let Some(stem) = stem {
                tokens
                    .chunks_exact_mut(c.dim)
                    .for_each(|t| t.iter_mut().zip(stem).for_each(|(t, s)| *t += s));
            }
            let mut frames_back = feed_forward(&grouping.output, token_frames, &tokens, c.silu);
            frames_back.truncate(frames * w.band_dim());
            frames_back.chunks_exact_mut(w.band_dim()).for_each(|row| rms_normalize(row, eps));
            frames_back
        };
        // without stem queries, the frames given back are the same for every stem
        let shared: Vec<Vec<f32>> = if w.grouping.is_some() && stems.is_none() {
            (0..g.mask_bands.len()).into_par_iter().map(|b| band_input(b, None)).collect()
        } else {
            Vec::new()
        };
        let size = frames * g.mask_features();
        for (s, out) in out.chunks_exact_mut(size).enumerate() {
            let stem = stems.as_ref().map(|stems| &stems[s * c.dim..][..c.dim]);
            let per_band: Vec<Vec<f32>> = g
                .mask_bands
                .par_iter()
                .zip(w.stem_masks(s))
                .enumerate()
                .map(|(b, (band, mlp))| match shared.get(b) {
                    Some(input) => band_masks(c, mlp, input, band.width),
                    None => band_masks(c, mlp, &band_input(b, stem), band.width),
                })
                .collect();
            for (band, masks) in g.mask_bands.iter().zip(per_band) {
                for (t, row) in masks.chunks_exact(band.width).enumerate() {
                    out[t * g.mask_features() + band.offset..][..band.width].copy_from_slice(row);
                }
            }
        }
    }
}

/// A band's masks from the rows of `input`, one per frame, through its estimator `mlp`.
fn band_masks(c: &Config, mlp: &[Linear], input: &[f32], width: usize) -> Vec<f32> {
    let rows = input.len() / mlp.first().map_or(1, |l| l.inp);
    let mut input = Cow::Borrowed(input);
    for (k, l) in mlp.iter().enumerate() {
        let mut y = vec![0.0; rows * l.out];
        linear_with(l, rows, &input, l.inp, &mut y, l.out, false, Parallelism::None);
        if k + 1 < mlp.len() {
            y.iter_mut().for_each(|v| *v = v.tanh());
        }
        input = Cow::Owned(y);
    }
    if c.mask_glu {
        input
            .chunks_exact(width * 2)
            .flat_map(|row| {
                let (a, gate) = row.split_at(width);
                a.iter().zip(gate).map(|(a, g)| a * sigmoid(*g))
            })
            .collect()
    } else if c.pair_gates {
        input
            .chunks_exact(width + width / 2)
            .flat_map(|row| {
                let (a, gates) = row.split_at(width);
                a.chunks_exact(2).zip(gates).flat_map(|(pair, g)| pair.iter().map(move |a| a * sigmoid(*g)))
            })
            .collect()
    } else {
        input.into_owned()
    }
}

/// The attention of `pool`'s queries over `groups` sequences of `len` rows of `input` (the
/// rows of a sequence follow each other, `stride` rows from those of the one before), then its
/// feed-forward: a row of `dim` per sequence and query.
fn pool(p: &Pool, c: &Config, input: &[f32], groups: usize, len: usize, stride: usize) -> Vec<f32> {
    let (inner, kv, dh) = (c.inner_dim(), c.kv_dim(), c.dim_head);
    let shared = c.heads / c.kv_heads;
    let rows = (groups - 1) * stride + len;
    let mut keys_values = vec![0.0; rows * 2 * kv];
    linear(&p.kv, rows, input, c.dim, &mut keys_values, 2 * kv, false);
    let queries = p.queries.len() / inner;
    let attended: Vec<f32> = (0..groups * queries)
        .into_par_iter()
        .flat_map_iter(|row| {
            let tokens = &keys_values[row / queries * stride * 2 * kv..][..len * 2 * kv];
            let query = &p.queries[row % queries * inner..][..inner];
            (0..c.heads).flat_map(move |h| {
                let q = &query[h * dh..][..dh];
                let at = h / shared * dh;
                let mut scores: Vec<f32> = tokens
                    .chunks_exact(2 * kv)
                    .map(|t| q.iter().zip(&t[at..][..dh]).map(|(a, b)| a * b).sum())
                    .collect();
                softmax(&mut scores);
                (0..dh).map(move |d| {
                    tokens.chunks_exact(2 * kv).zip(&scores).map(|(t, s)| s * t[kv + at + d]).sum::<f32>()
                })
            })
        })
        .collect();
    let n = groups * queries;
    let mut y = vec![0.0; n * c.dim];
    linear(&p.out, n, &attended, inner, &mut y, c.dim, false);
    let mut normed = y.clone();
    normed.chunks_exact_mut(c.dim).for_each(|row| rms_normalize(row, c.norm_eps));
    let ff = feed_forward(&p.ff, n, &normed, c.silu);
    y.iter_mut().zip(ff).for_each(|(y, f)| *y += f);
    y
}

/// The gated feed-forward `ff` of `rows` rows of `input`.
fn feed_forward(ff: &FeedForward, rows: usize, input: &[f32], silu: bool) -> Vec<f32> {
    let hidden = ff.ff1.out;
    let mut h = vec![0.0; rows * hidden];
    linear_with(&ff.ff1, rows, input, ff.ff1.inp, &mut h, hidden, false, Parallelism::None);
    gated(&mut h, hidden, silu);
    let mut y = vec![0.0; rows * ff.ff2.out];
    linear_with(&ff.ff2, rows, &h, hidden, &mut y, ff.ff2.out, false, Parallelism::None);
    y
}

/// `act(gate) * value` in place of the gates of each row of `hidden` values, its gates then
/// its values.
fn gated(hidden: &mut [f32], width: usize, silu: bool) {
    hidden.par_chunks_exact_mut(width).for_each(|row| {
        let (gate, value) = row.split_at_mut(width / 2);
        if silu {
            gate.iter_mut().zip(value).for_each(|(g, v)| *g = *g * sigmoid(*g) * *v);
        } else {
            gate.iter_mut().zip(value).for_each(|(g, v)| *g = gelu(*g) * *v);
        }
    });
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
    /// At `positions` positions, the registers' past the `real` ones left as they are.
    fn new(freqs: &[f32], real: usize, positions: usize) -> Self {
        let angles =
            (0..positions).flat_map(|p| freqs.iter().map(move |f| if p < real { p as f32 * f } else { 0.0 }));
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

/// Multi-head attention along `axis`: rotary queries and keys, softmax, each head's output
/// scaled by the sigmoid of its gate if gated.
fn attention(s: &mut Scratch, c: &Config, axis: Axis, frames: usize, bands: usize, rotary: &Rotary) {
    let width = c.qkvg_width();
    let (inner, kv, dh, heads) = (c.inner_dim(), c.kv_dim(), c.dim_head, c.heads);
    let shared = heads / c.kv_heads;
    s.qkvg.par_chunks_exact_mut(width).enumerate().for_each(|(token, row)| {
        let pos = axis.position(token, bands);
        for h in 0..heads + c.kv_heads {
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
                let k = qkvg[token(0) * width + inner + h / shared * dh..].as_ptr();
                let v = qkvg[token(0) * width + inner + kv + h / shared * dh..].as_ptr();
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
                if c.gates {
                    for i in 0..len {
                        let gate = sigmoid(qkvg[token(i) * width + inner + 2 * kv + h]);
                        out[i * inner + h * dh..][..dh].iter_mut().for_each(|o| *o *= gate);
                    }
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

/// `x / max(√(‖x‖² + len·eps), 1e-12)`: the RMS norm without its gain, nor its `√len`
/// (`F.normalize` when `eps` is 0).
pub(crate) fn rms_normalize(x: &mut [f32], eps: f32) {
    let norm = (x.iter().map(|v| v * v).sum::<f32>() + x.len() as f32 * eps).sqrt().max(1e-12);
    x.iter_mut().for_each(|v| *v /= norm);
}

/// The RMS norm, times `gain`.
fn scale_normalize(x: &mut [f32], gain: &[f32], eps: f32) {
    rms_normalize(x, eps);
    x.iter_mut().zip(gain).for_each(|(v, g)| *v *= g);
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
