//! The network's weights, as written by `tools/export_roformer.py` or `tools/export_bs_roformer.py`.

use std::borrow::Cow;
use std::collections::HashMap;
use std::fs::File;
use std::path::Path;

use safetensors::{Dtype, SafeTensors};

use crate::StemsError;

/// Shape of the network, from the file's metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// Width of the features between the band split and the mask estimators.
    pub dim: usize,
    pub heads: usize,
    /// Heads of keys and values, each shared by `heads / kv_heads` heads of queries.
    pub kv_heads: usize,
    pub dim_head: usize,
    /// Each head's output is scaled by the sigmoid of its gate.
    pub gates: bool,
    /// Pairs of (time, frequency) transformers.
    pub depth: usize,
    pub n_fft: usize,
    /// The STFT the masks apply to: `n_fft` unless the file says.
    pub n_fft_out: usize,
    pub hop: usize,
    pub sample_rate: u32,
    /// STFT frames the network sees at once.
    pub chunk_frames: usize,
    /// Each transformer pair also takes the sum of the outputs of the pairs before it.
    pub skip: bool,
    /// How many chunks cover each sample of a track: 1 cuts it back to back.
    pub overlap: usize,
    /// Added to the mean square under the RMS norms' root; without it, they only keep off zero.
    pub norm_eps: f32,
    /// The first product of each feed-forward gives a gate and a value per hidden feature,
    /// `gelu(gate) * value`; else the hidden features themselves, through `gelu`.
    pub ff_glu: bool,
    /// The gates of the gated feed-forwards go through SiLU instead of GELU.
    pub silu: bool,
    /// The mask estimators close on a GLU; else on their last product.
    pub mask_glu: bool,
    /// The mask estimators' last product gives, after the masks, a gate per pair of real and
    /// imaginary parts: the sigmoid of the gate scales them.
    pub pair_gates: bool,
    /// Side of the square of learned tokens past the last frame and the last band, which the
    /// transformers' sequences carry along.
    pub registers: usize,
    /// Frames to a token of the transformers ([`Grouping`]).
    pub group: usize,
    /// The stems are queries ([`StemQueries`]): one mask estimator serves them all.
    pub stem_queries: bool,
}

impl Config {
    /// Width of one token's queries, keys and values together, then its gates.
    pub fn qkvg_width(&self) -> usize {
        self.inner_dim() + 2 * self.kv_dim() + if self.gates { self.heads } else { 0 }
    }

    pub fn inner_dim(&self) -> usize {
        self.heads * self.dim_head
    }

    /// Width of one token's keys, or of its values.
    pub fn kv_dim(&self) -> usize {
        self.kv_heads * self.dim_head
    }

    /// Tokens along the time over `frames` frames, the last one padded with silence.
    pub fn token_frames(&self, frames: usize) -> usize {
        frames.div_ceil(self.group)
    }

    /// Tokens of the transformers' sequences along the time over `frames` frames: the
    /// registers' follow.
    pub fn seq_frames(&self, frames: usize) -> usize {
        self.token_frames(frames) + self.registers
    }

    /// Samples per chunk: the frames span `chunk_frames - 1` hops (the STFT is centred).
    pub fn chunk_samples(&self) -> usize {
        self.hop * (self.chunk_frames - 1)
    }

    pub fn freq_bins(&self) -> usize {
        self.n_fft / 2 + 1
    }

    /// Bins of the STFT the masks apply to.
    pub fn mask_bins(&self) -> usize {
        self.n_fft_out / 2 + 1
    }

    /// The values of the last product of a band's mask estimator, for `width` masks.
    pub fn mask_out(&self, width: usize) -> usize {
        if self.mask_glu {
            2 * width
        } else if self.pair_gates {
            width + width / 2
        } else {
            width
        }
    }

    /// The masks of a band, its estimator's last product giving `out` values.
    fn mask_width(&self, out: usize) -> usize {
        if self.mask_glu {
            out / 2
        } else if self.pair_gates {
            out / 3 * 2
        } else {
            out
        }
    }
}

/// `y = x · weightᵀ + bias`, the weight stored `out` rows of `inp` values.
#[derive(Debug, Clone)]
pub struct Linear {
    pub weight: Values,
    pub bias: Option<Vec<f32>>,
    pub out: usize,
    pub inp: usize,
}

impl Linear {
    /// The layer giving its outputs from `first` on, `count` of them.
    pub fn rows(&self, first: usize, count: usize) -> Linear {
        let values = first * self.inp..(first + count) * self.inp;
        let weight = match &self.weight {
            Values::F32(v) => Values::F32(v[values].to_vec()),
            Values::F16(v) => Values::F16(v[values].to_vec()),
        };
        let bias = self.bias.as_ref().map(|b| b[first..first + count].to_vec());
        Linear { weight, bias, out: count, inp: self.inp }
    }
}

/// A weight in the precision of the model file: the GPU takes f16 weights as they are.
#[derive(Debug, Clone)]
pub enum Values {
    F32(Vec<f32>),
    F16(Vec<half::f16>),
}

impl Values {
    pub fn len(&self) -> usize {
        match self {
            Values::F32(v) => v.len(),
            Values::F16(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The values as f32, widened if stored as f16.
    pub fn to_f32(&self) -> Cow<'_, [f32]> {
        match self {
            Values::F32(v) => Cow::Borrowed(v),
            Values::F16(v) => Cow::Owned(v.iter().map(|h| h.to_f32()).collect()),
        }
    }

    /// The values as f16, rounded if stored as f32.
    pub fn to_f16(&self) -> Cow<'_, [half::f16]> {
        match self {
            Values::F32(v) => Cow::Owned(v.iter().map(|&x| half::f16::from_f32(x)).collect()),
            Values::F16(v) => Cow::Borrowed(v),
        }
    }
}

/// One band of the band split: its slice of a frame of gathered STFT bins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Band {
    /// Its first feature.
    pub offset: usize,
    /// Its features: real and imaginary parts of its bins, both channels.
    pub width: usize,
}

/// What the separation needs to know around the network: its shape, the stems it separates
/// and how the STFT bins map to its bands.
#[derive(Debug, Clone)]
pub struct Geometry {
    pub config: Config,
    /// The stems a mask is estimated for, in the order of the masks.
    pub stems: Vec<String>,
    /// For each gathered bin, its index in the stereo STFT (`bin * 2 + channel`).
    pub freq_indices: Vec<u32>,
    /// For each bin of the masks, its index in the stereo STFT they apply to: as
    /// `freq_indices` unless the masks have an STFT of their own.
    pub mask_indices: Vec<u32>,
    /// How many bands' masks each bin of the masks' STFT gets.
    pub bands_per_freq: Vec<u32>,
    pub bands: Vec<Band>,
    /// Each band's slice of a frame of masks.
    pub mask_bands: Vec<Band>,
}

impl Geometry {
    /// The features of a frame: every gathered bin, real and imaginary.
    pub fn features(&self) -> usize {
        self.freq_indices.len() * 2
    }

    /// The masks of a frame: every bin of the masks, real and imaginary.
    pub fn mask_features(&self) -> usize {
        self.mask_indices.len() * 2
    }

    /// The values of the masks of `frames` frames: [`Self::mask_features`] per frame and stem.
    pub fn mask_len(&self, frames: usize) -> usize {
        self.stems.len() * frames * self.mask_features()
    }

    /// Bands of the transformers' sequences: the registers' follow.
    pub fn seq_bands(&self) -> usize {
        self.bands.len() + self.config.registers
    }

    /// Token of register `r` (row after row of the square) in the sequences over `frames`
    /// frames: the square follows the last token along the time and the last band.
    pub fn register_token(&self, frames: usize, r: usize) -> usize {
        let side = self.config.registers;
        (self.config.token_frames(frames) + r / side) * self.seq_bands() + self.bands.len() + r % side
    }
}

#[derive(Debug, Clone)]
pub struct Layer {
    /// Normalised token → queries (pre-scaled), keys, values, then one gate per head.
    pub qkvg: Linear,
    pub out: Linear,
    pub ff1: Linear,
    pub ff2: Linear,
}

/// A gated feed-forward: `ff1` gives a gate then a value per hidden feature, `ff2` takes
/// `act(gate) * value`.
#[derive(Debug, Clone)]
pub struct FeedForward {
    pub ff1: Linear,
    pub ff2: Linear,
}

/// The band split's features of [`Config::group`] frames make a token of the transformers,
/// frame after frame, and the tokens give them back. Without biases: the silent frames
/// padding the last token, and the registers' columns, stay zero.
#[derive(Debug, Clone)]
pub struct Grouping {
    /// The frames of a token, each normed (the gain folded in), to the token.
    pub input: FeedForward,
    /// A token to its frames, each normed after (the gain folded into the mask estimators).
    pub output: FeedForward,
}

/// An attention of learned queries over a sequence of tokens, then a gated feed-forward on
/// its output, normed, added to it: a row of `dim` per query.
#[derive(Debug, Clone)]
pub struct Pool {
    /// The queries, pre-scaled: `heads * dim_head` values each.
    pub queries: Vec<f32>,
    /// A token to its keys, then its values.
    pub kv: Linear,
    pub out: Linear,
    pub ff: FeedForward,
}

/// The stems as queries: one query pools the bands of each token along the time of the
/// transformers' output, then a query per stem pools what it gives. The row of each stem is
/// added to every token on its way back to frames ([`Grouping::output`]).
#[derive(Debug, Clone)]
pub struct StemQueries {
    pub bands: Pool,
    pub tokens: Pool,
}

#[derive(Debug, Clone)]
pub struct Transformer {
    pub layers: Vec<Layer>,
    /// Gain of the output RMS norm, if there is one.
    pub norm: Option<Vec<f32>>,
}

#[derive(Debug, Clone)]
pub struct Weights {
    pub geometry: Geometry,
    pub rotary_time: Vec<f32>,
    pub rotary_freq: Vec<f32>,
    /// Each band's projection to `dim` features (or to those a [`Grouping`] takes), its RMS
    /// norm gain folded in.
    pub band_proj: Vec<Linear>,
    /// With [`Config::group`] frames to a token.
    pub grouping: Option<Grouping>,
    /// Time then frequency transformer, `depth` times.
    pub transformers: Vec<Transformer>,
    /// Gain of the RMS norm after the last transformer, if there is one.
    pub final_norm: Option<Vec<f32>>,
    /// `registers` rows of `registers` tokens of `dim` features.
    pub registers: Vec<f32>,
    /// With [`Config::stem_queries`].
    pub stem_queries: Option<StemQueries>,
    /// For each stem (one for them all, with stem queries), then each band, its mask MLP (tanh
    /// between layers, gated after the last if `mask_glu` or `pair_gates`).
    pub masks: Vec<Vec<Vec<Linear>>>,
    /// Tensors of the file the network does not use (test vectors of the fixtures).
    pub extra: HashMap<String, Vec<f32>>,
}

impl Weights {
    /// Every linear layer: band split, grouping, transformers, stem queries, mask estimators.
    pub fn linears(&self) -> impl Iterator<Item = &Linear> {
        let layers = self.transformers.iter().flat_map(|t| &t.layers);
        let grouping = self.grouping.iter().flat_map(|g| [&g.input, &g.output]);
        let pools = self.stem_queries.iter().flat_map(|q| [&q.bands, &q.tokens]);
        self.band_proj
            .iter()
            .chain(grouping.flat_map(|f| [&f.ff1, &f.ff2]))
            .chain(layers.flat_map(|l| [&l.qkvg, &l.out, &l.ff1, &l.ff2]))
            .chain(pools.flat_map(|p| [&p.kv, &p.out, &p.ff.ff1, &p.ff.ff2]))
            .chain(self.masks.iter().flatten().flatten())
    }

    pub fn linears_mut(&mut self) -> impl Iterator<Item = &mut Linear> {
        let layers = self.transformers.iter_mut().flat_map(|t| &mut t.layers);
        let grouping = self.grouping.iter_mut().flat_map(|g| [&mut g.input, &mut g.output]);
        let pools = self.stem_queries.iter_mut().flat_map(|q| [&mut q.bands, &mut q.tokens]);
        self.band_proj
            .iter_mut()
            .chain(grouping.flat_map(|f| [&mut f.ff1, &mut f.ff2]))
            .chain(layers.flat_map(|l| [&mut l.qkvg, &mut l.out, &mut l.ff1, &mut l.ff2]))
            .chain(pools.flat_map(|p| [&mut p.kv, &mut p.out, &mut p.ff.ff1, &mut p.ff.ff2]))
            .chain(self.masks.iter_mut().flatten().flatten())
    }

    /// The features of each band the band split gives.
    pub fn band_dim(&self) -> usize {
        self.band_proj.first().map_or(0, |p| p.out)
    }

    /// The mask estimators of stem `stem`.
    pub fn stem_masks(&self, stem: usize) -> &[Vec<Linear>] {
        &self.masks[if self.geometry.config.stem_queries { 0 } else { stem }]
    }

    pub fn load(path: &Path) -> Result<Self, StemsError> {
        let file = File::open(path)?;
        // SAFETY: the file is only read, and Prisme never writes a model file while running.
        let map = unsafe { memmap2::Mmap::map(&file)? };
        Self::parse(&map)
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, StemsError> {
        let (_, header) = SafeTensors::read_metadata(bytes)?;
        let meta = header.metadata().clone().unwrap_or_default();
        let tensors = SafeTensors::deserialize(bytes)?;
        let file = Reader { tensors: &tensors, meta: &meta };

        let heads = file.meta("heads")?;
        let n_fft = file.meta("n_fft")?;
        let config = Config {
            dim: file.meta("dim")?,
            heads,
            kv_heads: file.meta_or("kv_heads", heads)?,
            dim_head: file.meta("dim_head")?,
            gates: file.meta_or("gates", 1u8)? == 1,
            depth: file.meta("depth")?,
            n_fft,
            n_fft_out: file.meta_or("n_fft_out", n_fft)?,
            hop: file.meta("hop")?,
            sample_rate: file.meta("sample_rate")?,
            chunk_frames: file.meta("chunk_frames")?,
            skip: file.meta_or("skip_connection", 0u8)? == 1,
            overlap: file.meta_or("overlap", 1)?,
            norm_eps: file.meta_or("norm_eps", 0.0)?,
            ff_glu: file.meta_or("ff_glu", 0u8)? == 1,
            silu: file.meta_or("ff_silu", 0u8)? == 1,
            mask_glu: file.meta_or("mask_glu", 1u8)? == 1,
            pair_gates: file.meta_or("mask_pair_gates", 0u8)? == 1,
            registers: file.meta_or("registers", 0)?,
            group: file.meta_or("time_group", 1)?,
            stem_queries: file.meta_or("stem_queries", 0u8)? == 1,
        };
        if config.overlap == 0 || config.overlap > config.chunk_samples() {
            return Err(StemsError::Model("overlap out of range".into()));
        }
        if config.group == 0 {
            return Err(StemsError::Model("time_group out of range".into()));
        }
        let time_depth: usize = file.meta("time_depth")?;
        let freq_depth: usize = file.meta("freq_depth")?;
        let mask_layers: usize = file.meta("mask_layers")?;

        let freq_indices = file.u32s("freq_indices")?;
        let mask_indices = if file.has("mask_freq_indices") {
            file.u32s("mask_freq_indices")?
        } else {
            freq_indices.clone()
        };
        let bands_per_freq = file.u32s("bands_per_freq")?;
        if bands_per_freq.len() != config.mask_bins() {
            return Err(StemsError::Model("bands_per_freq does not match the masks' STFT".into()));
        }

        let mut bands = Vec::new();
        let mut band_proj = Vec::new();
        let mut offset = 0;
        while file.has(&format!("band.{}.weight", bands.len())) {
            let proj = file.linear(&format!("band.{}", bands.len()))?;
            bands.push(Band { offset, width: proj.inp });
            offset += proj.inp;
            band_proj.push(proj);
        }
        if offset != freq_indices.len() * 2 {
            return Err(StemsError::Model("the bands do not cover the gathered bins".into()));
        }
        let grouping = if config.group > 1 {
            Some(Grouping { input: file.feed_forward("group.in")?, output: file.feed_forward("group.out")? })
        } else {
            None
        };

        let mut transformers = Vec::new();
        for i in 0..2 * config.depth {
            let depth = if i % 2 == 0 { time_depth } else { freq_depth };
            let layers = (0..depth)
                .map(|j| {
                    let p = format!("transformer.{i}.layer.{j}");
                    Ok(Layer {
                        qkvg: file.linear(&format!("{p}.qkvg"))?,
                        out: file.linear(&format!("{p}.out"))?,
                        ff1: file.linear(&format!("{p}.ff1"))?,
                        ff2: file.linear(&format!("{p}.ff2"))?,
                    })
                })
                .collect::<Result<_, StemsError>>()?;
            transformers
                .push(Transformer { layers, norm: file.optional_f32s(&format!("transformer.{i}.norm"))? });
        }

        // A file without a list of stems holds the vocals' masks, unnamed.
        let stems: Vec<String> = match file.meta.get("stems") {
            Some(list) => list.split(',').map(str::to_owned).collect(),
            None => vec!["vocals".to_owned()],
        };
        let named = file.meta.contains_key("stems");
        let estimators = |prefix: &str| -> Result<Vec<Vec<Linear>>, StemsError> {
            (0..bands.len())
                .map(|b| (0..mask_layers).map(|k| file.linear(&format!("{prefix}{b}.{k}"))).collect())
                .collect()
        };
        let (stem_queries, masks) = if config.stem_queries {
            let queries = StemQueries { bands: file.pool("pool.freq")?, tokens: file.pool("pool.time")? };
            (Some(queries), vec![estimators("mask.")?])
        } else {
            let masks = stems
                .iter()
                .map(|stem| estimators(&if named { format!("mask.{stem}.") } else { "mask.".to_owned() }))
                .collect::<Result<Vec<_>, _>>()?;
            (None, masks)
        };
        let mut offset = 0;
        let mask_bands = masks[0]
            .iter()
            .map(|mlp| {
                let width = config.mask_width(mlp.last().map_or(0, |l| l.out));
                offset += width;
                Band { offset: offset - width, width }
            })
            .collect();

        let mut extra = HashMap::new();
        for (name, _) in tensors.tensors() {
            if name.starts_with("test.") {
                extra.insert(name.clone(), file.f32s(&name)?);
            }
        }

        let weights = Self {
            geometry: Geometry {
                config,
                stems,
                freq_indices,
                mask_indices,
                bands_per_freq,
                bands,
                mask_bands,
            },
            rotary_time: file.f32s("rotary.time")?,
            rotary_freq: file.f32s("rotary.freq")?,
            band_proj,
            grouping,
            transformers,
            final_norm: file.optional_f32s("final_norm")?,
            registers: file.optional_f32s("registers")?.unwrap_or_default(),
            stem_queries,
            masks,
            extra,
        };
        weights.check()?;
        Ok(weights)
    }

    fn check(&self) -> Result<(), StemsError> {
        let g = &self.geometry;
        let c = &g.config;
        let bad = |what: &str| Err(StemsError::Model(format!("unexpected shape: {what}")));
        if self.rotary_time.len() * 2 != c.dim_head || self.rotary_freq.len() * 2 != c.dim_head {
            return bad("rotary frequencies");
        }
        let band_dim = self.band_dim();
        if self.band_proj.iter().any(|p| p.out != band_dim) || (self.grouping.is_none() && band_dim != c.dim)
        {
            return bad("band split");
        }
        let gated = |f: &FeedForward, inp: usize, out: usize| {
            f.ff1.inp == inp && f.ff1.out.is_multiple_of(2) && f.ff2.inp == f.ff1.out / 2 && f.ff2.out == out
        };
        if let Some(grouping) = &self.grouping {
            let biased = [&grouping.input, &grouping.output]
                .into_iter()
                .any(|f| f.ff1.bias.is_some() || f.ff2.bias.is_some());
            if biased
                || !gated(&grouping.input, c.group * band_dim, c.dim)
                || !gated(&grouping.output, c.dim, c.group * band_dim)
            {
                return bad("grouping");
            }
        }
        if let Some(queries) = &self.stem_queries {
            let pool = |p: &Pool, count: usize| {
                p.queries.len() == count * c.inner_dim()
                    && p.kv.inp == c.dim
                    && p.kv.out == 2 * c.kv_dim()
                    && p.out.inp == c.inner_dim()
                    && p.out.out == c.dim
                    && gated(&p.ff, c.dim, c.dim)
            };
            if self.grouping.is_none() || !pool(&queries.bands, 1) || !pool(&queries.tokens, g.stems.len()) {
                return bad("stem queries");
            }
        }
        if c.kv_heads == 0 || !c.heads.is_multiple_of(c.kv_heads) {
            return bad("heads of keys and values");
        }
        let hidden_of = |l: &Layer| if c.ff_glu { l.ff1.out / 2 } else { l.ff1.out };
        for tr in &self.transformers {
            for l in &tr.layers {
                if l.qkvg.inp != c.dim
                    || l.qkvg.out != c.qkvg_width()
                    || l.out.inp != c.inner_dim()
                    || l.out.out != c.dim
                    || l.ff1.inp != c.dim
                    || (c.ff_glu && !l.ff1.out.is_multiple_of(2))
                    || l.ff2.inp != hidden_of(l)
                    || l.ff2.out != c.dim
                {
                    return bad("transformer layer");
                }
            }
        }
        let mut norms = self.transformers.iter().filter_map(|t| t.norm.as_ref()).chain(&self.final_norm);
        if norms.any(|n| n.len() != c.dim) {
            return bad("transformer norm");
        }
        if self.registers.len() != c.registers * c.registers * c.dim {
            return bad("registers");
        }
        if g.stems.is_empty() || g.stems.iter().any(|s| s.is_empty()) {
            return bad("stems");
        }
        if c.mask_glu && c.pair_gates {
            return bad("mask gates");
        }
        let mask_in = if self.grouping.is_some() { band_dim } else { c.dim };
        for (band, mlp) in self.masks.iter().flat_map(|stem| g.mask_bands.iter().zip(stem)) {
            let mut inp = mask_in;
            for l in mlp {
                if l.inp != inp {
                    return bad("mask estimator");
                }
                inp = l.out;
            }
            if inp != c.mask_out(band.width) {
                return bad("mask estimator output");
            }
        }
        if g.mask_bands.iter().map(|b| b.width).sum::<usize>() != g.mask_features() {
            return bad("the masks do not cover their bins");
        }
        if g.freq_indices.iter().any(|&i| i as usize >= c.freq_bins() * 2) {
            return bad("freq_indices");
        }
        if g.mask_indices.iter().any(|&i| i as usize >= c.mask_bins() * 2) {
            return bad("mask_freq_indices");
        }
        Ok(())
    }
}

struct Reader<'a> {
    tensors: &'a SafeTensors<'a>,
    meta: &'a HashMap<String, String>,
}

impl Reader<'_> {
    fn meta<T: std::str::FromStr>(&self, key: &str) -> Result<T, StemsError> {
        self.meta
            .get(key)
            .and_then(|v| v.parse().ok())
            .ok_or_else(|| StemsError::Model(format!("missing metadata {key}")))
    }

    fn meta_or<T: std::str::FromStr>(&self, key: &str, default: T) -> Result<T, StemsError> {
        if self.meta.contains_key(key) {
            self.meta(key)
        } else {
            Ok(default)
        }
    }

    fn has(&self, name: &str) -> bool {
        self.tensors.tensor(name).is_ok()
    }

    fn optional_f32s(&self, name: &str) -> Result<Option<Vec<f32>>, StemsError> {
        if self.has(name) {
            self.f32s(name).map(Some)
        } else {
            Ok(None)
        }
    }

    /// A float tensor, widened to f32 when stored as f16.
    fn f32s(&self, name: &str) -> Result<Vec<f32>, StemsError> {
        let t = self.tensors.tensor(name).map_err(|_| missing(name))?;
        match t.dtype() {
            Dtype::F32 => {
                Ok(t.data().chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect())
            }
            Dtype::F16 => Ok(t
                .data()
                .chunks_exact(2)
                .map(|b| half::f16::from_le_bytes([b[0], b[1]]).to_f32())
                .collect()),
            other => Err(StemsError::Model(format!("{name} is {other:?}, not a float"))),
        }
    }

    /// A float tensor as stored.
    fn values(&self, name: &str) -> Result<Values, StemsError> {
        let t = self.tensors.tensor(name).map_err(|_| missing(name))?;
        match t.dtype() {
            Dtype::F16 => Ok(Values::F16(bytemuck::pod_collect_to_vec(t.data()))),
            _ => Ok(Values::F32(self.f32s(name)?)),
        }
    }

    fn u32s(&self, name: &str) -> Result<Vec<u32>, StemsError> {
        let t = self.tensors.tensor(name).map_err(|_| missing(name))?;
        if t.dtype() != Dtype::I32 {
            return Err(StemsError::Model(format!("{name} is not i32")));
        }
        t.data()
            .chunks_exact(4)
            .map(|b| {
                u32::try_from(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                    .map_err(|_| StemsError::Model(format!("{name} has a negative value")))
            })
            .collect()
    }

    fn linear(&self, prefix: &str) -> Result<Linear, StemsError> {
        let name = format!("{prefix}.weight");
        let t = self.tensors.tensor(&name).map_err(|_| missing(&name))?;
        let &[out, inp] = t.shape() else {
            return Err(StemsError::Model(format!("{name} is not a matrix")));
        };
        let bias_name = format!("{prefix}.bias");
        let bias = if self.has(&bias_name) { Some(self.f32s(&bias_name)?) } else { None };
        if bias.as_ref().is_some_and(|b| b.len() != out) {
            return Err(StemsError::Model(format!("{bias_name} does not match its weight")));
        }
        let weight = self.values(&name)?;
        if weight.len() != out * inp {
            return Err(StemsError::Model(format!("{name} does not match its shape")));
        }
        Ok(Linear { weight, bias, out, inp })
    }

    fn feed_forward(&self, prefix: &str) -> Result<FeedForward, StemsError> {
        Ok(FeedForward {
            ff1: self.linear(&format!("{prefix}.ff1"))?,
            ff2: self.linear(&format!("{prefix}.ff2"))?,
        })
    }

    fn pool(&self, prefix: &str) -> Result<Pool, StemsError> {
        Ok(Pool {
            queries: self.f32s(&format!("{prefix}.queries"))?,
            kv: self.linear(&format!("{prefix}.kv"))?,
            out: self.linear(&format!("{prefix}.out"))?,
            ff: self.feed_forward(prefix)?,
        })
    }
}

fn missing(name: &str) -> StemsError {
    StemsError::Model(format!("missing tensor {name}"))
}
