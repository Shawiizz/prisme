//! The network's weights, as written by `tools/export_roformer.py`.

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
    pub dim_head: usize,
    /// Pairs of (time, frequency) transformers.
    pub depth: usize,
    pub n_fft: usize,
    pub hop: usize,
    pub sample_rate: u32,
    /// STFT frames the network sees at once.
    pub chunk_frames: usize,
}

impl Config {
    /// Width of one token's query, key and value projections together, plus one gate per head.
    pub fn qkvg_width(&self) -> usize {
        3 * self.heads * self.dim_head + self.heads
    }

    pub fn inner_dim(&self) -> usize {
        self.heads * self.dim_head
    }

    /// Samples per chunk: the frames span `chunk_frames - 1` hops (the STFT is centred).
    pub fn chunk_samples(&self) -> usize {
        self.hop * (self.chunk_frames - 1)
    }

    pub fn freq_bins(&self) -> usize {
        self.n_fft / 2 + 1
    }
}

/// `y = x · weightᵀ + bias`, the weight stored `out` rows of `inp` values.
#[derive(Debug, Clone)]
pub struct Linear {
    pub weight: Vec<f32>,
    pub bias: Option<Vec<f32>>,
    pub out: usize,
    pub inp: usize,
}

/// One band of the band split: its slice of a frame of gathered STFT bins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Band {
    /// Its first feature.
    pub offset: usize,
    /// Its features: real and imaginary parts of its bins, both channels.
    pub width: usize,
}

/// What the separation needs to know around the network: its shape and how the STFT bins
/// map to its bands.
#[derive(Debug, Clone)]
pub struct Geometry {
    pub config: Config,
    /// For each gathered bin, its index in the stereo STFT (`bin * 2 + channel`).
    pub freq_indices: Vec<u32>,
    /// How many bands each STFT bin belongs to.
    pub bands_per_freq: Vec<u32>,
    pub bands: Vec<Band>,
}

impl Geometry {
    /// The features of a frame: every gathered bin, real and imaginary.
    pub fn features(&self) -> usize {
        self.freq_indices.len() * 2
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

#[derive(Debug, Clone)]
pub struct Transformer {
    pub layers: Vec<Layer>,
    /// Gain of the output RMS norm.
    pub norm: Vec<f32>,
}

#[derive(Debug, Clone)]
pub struct Weights {
    pub geometry: Geometry,
    pub rotary_time: Vec<f32>,
    pub rotary_freq: Vec<f32>,
    /// Each band's projection to `dim` features, its RMS norm gain folded in.
    pub band_proj: Vec<Linear>,
    /// Time then frequency transformer, `depth` times.
    pub transformers: Vec<Transformer>,
    /// For each band, its mask MLP (tanh between layers, a GLU after the last).
    pub masks: Vec<Vec<Linear>>,
    /// Tensors of the file the network does not use (test vectors of the fixtures).
    pub extra: HashMap<String, Vec<f32>>,
}

impl Weights {
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

        let config = Config {
            dim: file.meta("dim")?,
            heads: file.meta("heads")?,
            dim_head: file.meta("dim_head")?,
            depth: file.meta("depth")?,
            n_fft: file.meta("n_fft")?,
            hop: file.meta("hop")?,
            sample_rate: file.meta("sample_rate")?,
            chunk_frames: file.meta("chunk_frames")?,
        };
        let time_depth: usize = file.meta("time_depth")?;
        let freq_depth: usize = file.meta("freq_depth")?;
        let mask_layers: usize = file.meta("mask_layers")?;

        let freq_indices = file.u32s("freq_indices")?;
        let bands_per_freq = file.u32s("bands_per_freq")?;
        if bands_per_freq.len() != config.freq_bins() {
            return Err(StemsError::Model("bands_per_freq does not match n_fft".into()));
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
            transformers.push(Transformer { layers, norm: file.f32s(&format!("transformer.{i}.norm"))? });
        }

        let masks = (0..bands.len())
            .map(|b| (0..mask_layers).map(|k| file.linear(&format!("mask.{b}.{k}"))).collect())
            .collect::<Result<_, StemsError>>()?;

        let mut extra = HashMap::new();
        for (name, _) in tensors.tensors() {
            if name.starts_with("test.") {
                extra.insert(name.clone(), file.f32s(&name)?);
            }
        }

        let weights = Self {
            geometry: Geometry { config, freq_indices, bands_per_freq, bands },
            rotary_time: file.f32s("rotary.time")?,
            rotary_freq: file.f32s("rotary.freq")?,
            band_proj,
            transformers,
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
        for proj in &self.band_proj {
            if proj.out != c.dim {
                return bad("band split");
            }
        }
        for tr in &self.transformers {
            for l in &tr.layers {
                if l.qkvg.inp != c.dim
                    || l.qkvg.out != c.qkvg_width()
                    || l.out.inp != c.inner_dim()
                    || l.out.out != c.dim
                    || l.ff1.inp != c.dim
                    || l.ff2.inp != l.ff1.out
                    || l.ff2.out != c.dim
                {
                    return bad("transformer layer");
                }
            }
            if tr.norm.len() != c.dim {
                return bad("transformer norm");
            }
        }
        for (band, mlp) in g.bands.iter().zip(&self.masks) {
            let mut inp = c.dim;
            for l in mlp {
                if l.inp != inp {
                    return bad("mask estimator");
                }
                inp = l.out;
            }
            if inp != band.width * 2 {
                return bad("mask estimator output");
            }
        }
        if g.freq_indices.iter().any(|&i| i as usize >= c.freq_bins() * 2) {
            return bad("freq_indices");
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

    fn has(&self, name: &str) -> bool {
        self.tensors.tensor(name).is_ok()
    }

    fn f32s(&self, name: &str) -> Result<Vec<f32>, StemsError> {
        let t = self.tensors.tensor(name).map_err(|_| missing(name))?;
        if t.dtype() != Dtype::F32 {
            return Err(StemsError::Model(format!("{name} is not f32")));
        }
        Ok(t.data().chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect())
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
        Ok(Linear { weight: self.f32s(&name)?, bias, out, inp })
    }
}

fn missing(name: &str) -> StemsError {
    StemsError::Model(format!("missing tensor {name}"))
}
