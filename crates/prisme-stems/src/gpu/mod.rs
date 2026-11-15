//! The network on the GPU, through wgpu (Direct3D 12 or Vulkan): any recent GPU, no runtime
//! to install.
//!
//! Where wgpu reaches the GPU's matrix units (the tensor cores, through Vulkan's cooperative
//! matrices), the matrix products and the attention run on them: f16 operands, f32 sums. The
//! activations they read are then stored as f16, as the units round them anyway; the residual
//! stream between the layers stays f32. The weights are f16 either way, as the models store
//! them: half the memory of f32 ones, which lets the networks fit on 4 GB cards.
//!
//! The work for a chunk is planned once (buffers, bind groups, dispatches) per frame count,
//! then replayed: only the input bins change between chunks. Networks opened on one [`Gpu`]
//! share its queue: their chunks take turns instead of contending, and so share the buffers
//! holding the activations of the one running.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Arc, Mutex};

use wgpu::util::DeviceExt;

use crate::model::{Config, Geometry, Linear, Pool, Values, Weights};
use crate::network::{Axis, MaskNetwork};
use crate::StemsError;

const MAX_GROUPS: u32 = 65_535;

/// How a [`Gpu`] is opened.
#[derive(Debug, Clone, Copy, Default)]
pub struct GpuOptions {
    /// Accepts a software rasteriser: for tests on machines without a GPU.
    pub software: bool,
    /// Keeps to f32 arithmetic on the shader cores even where there are matrix units.
    pub without_matrix_units: bool,
    /// Binds no buffer larger than this (bytes), below the GPU's own limit: to try on a large
    /// GPU how a small one cuts a track.
    pub max_binding: Option<u64>,
    /// Times the work by kind ([`GpuNetwork::profile`]), where the GPU tells the time: each
    /// step then waits for the one before, which makes the whole slower.
    pub profile: bool,
    /// Leaves the GPU to others (a phone's screen, drawn by the same GPU): the work goes in
    /// steps of a few milliseconds, the screen's drawing slipping in between, and takes at
    /// most this share of the GPU's time.
    pub share: Option<f64>,
    /// The attention on the shader cores, else the one they run best for the network's heads
    /// (the matrix units have their own).
    pub attention: Option<Attention>,
}

/// How the shader cores work out the attention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attention {
    /// In one pass over the keys, a query per thread (`attention.wgsl`).
    OnePass,
    /// As products: the scores of a slice of the sequences and heads, their softmax, then the
    /// values (`attention_scores.wgsl` and the others). Heads of 8, 16, 32 or 64 values, a
    /// token's projections in whole groups of 4.
    Products,
}

/// The GPU the networks run on.
pub struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    /// The GPU and graphics API.
    name: String,
    errors: Arc<Mutex<Option<String>>>,
    /// The shaders for the matrix units run on it.
    matrix_units: bool,
    /// NVIDIA's GPUs take the f32 products in large tiles; others would spill their
    /// registers, and take them in small ones.
    large_tiles: bool,
    /// A software renderer, accepted for tests on machines without a GPU.
    software: bool,
    /// The largest buffer a shader binds (bytes).
    max_binding: u64,
    /// Each step is timed ([`GpuOptions::profile`]).
    profile: bool,
    /// The share of the GPU's time the work takes at most ([`GpuOptions::share`]).
    share: Option<f64>,
    attention: Option<Attention>,
    scratch: Mutex<Scratch>,
}

/// The activations of the networks on a GPU, one buffer per [`Slot`], as large as the
/// largest network needs.
#[derive(Default)]
struct Scratch {
    /// Bumped when a buffer gives way to a larger one: the plans binding the old one are
    /// built again (until then, they keep it alive).
    generation: u64,
    buffers: Vec<wgpu::Buffer>,
}

/// What each scratch buffer holds during a chunk.
#[derive(Clone, Copy)]
enum Slot {
    /// The residual stream, f32.
    X,
    /// The stream before the last time transformer (networks with skip connections).
    Earlier,
    Normed,
    Qkvg,
    Attended,
    Hidden,
    BandIn,
    Mlp0,
    Mlp1,
    /// The last layer of a mask estimator, f32.
    MlpOut,
    /// The band split's frames grouped into tokens, then the frames the tokens give back, f32
    /// ([`crate::model::Grouping`]).
    Frames,
    /// The scores of the attention computed as products, a slice of the sequences and heads
    /// at a time, f32.
    Scores,
}

const SLOTS: usize = Slot::Scores as usize + 1;

pub struct GpuNetwork {
    gpu: Arc<Gpu>,
    geometry: Geometry,
    adapter: String,
    matrix: bool,
    pipelines: Pipelines,
    params: Params,
    plans: HashMap<usize, Plan>,
    /// The frames of the chunk started, and its last submission.
    started: Option<(usize, wgpu::SubmissionIndex)>,
    /// Seconds and steps of each kind of work so far, when profiling.
    profiled: HashMap<&'static str, (f64, usize)>,
}

/// The weights, on the GPU.
struct Params {
    rotary_time: Vec<f32>,
    rotary_freq: Vec<f32>,
    band_proj: Vec<GpuLinear>,
    band_dim: usize,
    grouping: Option<GpuGrouping>,
    /// Each transformer's layers, and the gain of its output norm if it has one.
    transformers: Vec<(Vec<GpuLayer>, Option<wgpu::Buffer>)>,
    final_norm: Option<wgpu::Buffer>,
    registers: Vec<f32>,
    /// Over the bands, then the tokens.
    pools: Option<[GpuPool; 2]>,
    /// Per stem (one for all, with stem queries), per band.
    masks: Vec<Vec<Vec<GpuLinear>>>,
    no_bias: wgpu::Buffer,
    /// The widest hidden features of the transformers' feed-forwards, as stored: the other
    /// feed-forwards take turns in their buffer, a slice of their rows at a time.
    ff_hidden: usize,
    mask_hidden: usize,
    /// On the shader cores, the first product of each gated feed-forward gives its hidden
    /// features itself ([`Act::Gated`]).
    gated_products: bool,
}

struct GpuGrouping {
    input: [GpuLinear; 2],
    output: GpuLinear,
    /// The second product giving the frames back, cut frame by frame.
    frames: Vec<GpuLinear>,
}

struct GpuPool {
    /// f32, pre-scaled.
    queries: wgpu::Buffer,
    /// How many queries.
    count: usize,
    kv: GpuLinear,
    out: GpuLinear,
    ff: [GpuLinear; 2],
}

#[derive(Clone)]
struct GpuLinear {
    /// f16 rows: of `inp` values, or for the matrix units padded to `ld` groups of 4.
    weight: wgpu::Buffer,
    bias: Option<wgpu::Buffer>,
    out: usize,
    inp: usize,
    ld: usize,
}

struct GpuLayer {
    qkvg: GpuLinear,
    out: GpuLinear,
    ff1: GpuLinear,
    ff2: GpuLinear,
}

/// How an activation is stored: f32, or as the matrix units read it (f16 where they are
/// used, else f32).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Store {
    F32,
    Act,
}

struct Pipelines {
    /// By input then output storage: F32 → F32, F32 → Act, Act → F32, Act → Act.
    matmul: [wgpu::ComputePipeline; 4],
    /// The side of the square of outputs a workgroup of `matmul` computes.
    matmul_tile: usize,
    /// On the shader cores, the products whose rows are read four values at a time: from
    /// registers alone, several times faster (`matmul_blocked.wgsl`).
    blocked: Option<wgpu::ComputePipeline>,
    norm: wgpu::ComputePipeline,
    norm_in_place: wgpu::ComputePipeline,
    rotary: wgpu::ComputePipeline,
    attention: wgpu::ComputePipeline,
    /// The attention runs on the matrix units, rotating the queries and keys itself.
    matrix_attention: bool,
    /// On the shader cores, where chosen ([`attention_as_products`]): the attention as products
    /// (scores, softmax, values), several times faster where `attention` spills its registers.
    attention_products: Option<[wgpu::ComputePipeline; 3]>,
    glu: wgpu::ComputePipeline,
    gated: wgpu::ComputePipeline,
    add: wgpu::ComputePipeline,
    /// With stem queries.
    pool: Option<wgpu::ComputePipeline>,
}

/// The dispatches for one frame count, in submissions short enough not to trip the
/// operating system's GPU watchdog on a slow GPU.
struct Plan {
    /// Of the scratch buffers it binds.
    generation: u64,
    bins: wgpu::Buffer,
    /// Zeroed before the work: where the band split leaves the frames padding the last token,
    /// and the registers' columns.
    cleared: Option<wgpu::Buffer>,
    mask: wgpu::Buffer,
    readback: wgpu::Buffer,
    submissions: Vec<Vec<Step>>,
}

impl Plan {
    /// Writes the bins of a chunk, and clears what has to be.
    fn load(&self, gpu: &Gpu, bins: &[f32]) {
        gpu.queue.write_buffer(&self.bins, 0, bytemuck::cast_slice(bins));
        if let Some(buffer) = &self.cleared {
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            encoder.clear_buffer(buffer, 0, None);
            gpu.queue.submit([encoder.finish()]);
        }
    }
}

struct Step {
    pipeline: wgpu::ComputePipeline,
    bind_group: wgpu::BindGroup,
    groups: (u32, u32),
    /// What the step does, for [`GpuNetwork::profile`].
    kind: &'static str,
    /// Its arithmetic, where it weighs (products, attention), for pacing the work.
    flops: usize,
}

#[derive(Clone, Copy)]
enum Act {
    None = 0,
    Gelu = 1,
    Tanh = 2,
    /// `gelu(gate) * value` of the columns in groups of 4 gates then their 4 values
    /// ([`gates_beside_values`]), half as many: on the shader cores' blocked products only.
    Gated = 3,
    /// As [`Act::Gated`], through SiLU.
    GatedSilu = 4,
}

impl Gpu {
    /// The most capable GPU, with its matrix units if it has some; fails without a GPU
    /// worth it. Software rasterisers are refused unless `options` accept them, and so are
    /// integrated GPUs without matrix units unless picked by name (`WGPU_ADAPTER_NAME`): the
    /// CPU separates faster (on an Intel UHD of 2023, 26 times faster).
    pub fn open(options: GpuOptions) -> Result<Arc<Self>, StemsError> {
        let (adapter, matrix_units) = worthy_adapter(options)?;
        let info = adapter.get_info();
        let profile = if options.profile {
            adapter.features() & wgpu::Features::TIMESTAMP_QUERY
        } else {
            wgpu::Features::empty()
        };
        let large_tiles = info.vendor == NVIDIA;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("prisme-stems"),
            required_features: if matrix_units { MATRIX_FEATURES } else { wgpu::Features::empty() } | profile,
            required_limits: adapter.limits(),
            // SAFETY: the cooperative matrices are the only experimental feature in use; the
            // tests hold the networks on them to the f32 ones.
            experimental_features: if matrix_units {
                unsafe { wgpu::ExperimentalFeatures::enabled() }
            } else {
                wgpu::ExperimentalFeatures::disabled()
            },
            memory_hints: wgpu::MemoryHints::Performance,
            ..Default::default()
        }))
        .map_err(|e| StemsError::Gpu(e.to_string()))?;
        let max_binding = options.max_binding.unwrap_or(u64::MAX).min(binding_limit(&device.limits()));
        let errors = Arc::new(Mutex::new(None));
        let sink = errors.clone();
        device.on_uncaptured_error(Arc::new(move |e: wgpu::Error| {
            let message = match &e {
                wgpu::Error::Validation { description, .. } => description.clone(),
                other => other.to_string(),
            };
            sink.lock().expect("error sink").get_or_insert(message);
        }));
        let name = format!("{} ({:?})", info.name, info.backend);
        Ok(Arc::new(Self {
            device,
            queue,
            name,
            errors,
            matrix_units,
            large_tiles,
            software: info.device_type == wgpu::DeviceType::Cpu,
            max_binding,
            profile: !profile.is_empty(),
            share: options.share.map(|s| s.clamp(0.05, 1.0)),
            attention: options.attention,
            scratch: Mutex::default(),
        }))
    }

    /// Whether there is a GPU [`Gpu::open`] would take, without opening it.
    pub fn available() -> bool {
        worthy_adapter(GpuOptions::default()).is_ok()
    }

    /// The GPU and graphics API.
    pub fn name(&self) -> &str {
        &self.name
    }

    fn check_errors(&self) -> Result<(), StemsError> {
        match self.errors.lock().expect("error sink").take() {
            Some(e) => Err(StemsError::Gpu(e)),
            None => Ok(()),
        }
    }

    /// The scratch buffers, each of at least `bytes`, and their generation.
    fn scratch(&self, bytes: [u64; SLOTS]) -> (u64, Vec<wgpu::Buffer>) {
        let mut scratch = self.scratch.lock().expect("scratch buffers");
        let scratch = &mut *scratch;
        if scratch.buffers.len() != SLOTS {
            scratch.buffers = (0..SLOTS).map(|_| self.storage_buffer(0)).collect();
        }
        for (buffer, &needed) in scratch.buffers.iter_mut().zip(&bytes) {
            if buffer.size() < needed {
                *buffer = self.storage_buffer(needed);
                scratch.generation += 1;
            }
        }
        (scratch.generation, scratch.buffers.clone())
    }

    fn scratch_generation(&self) -> u64 {
        self.scratch.lock().expect("scratch buffers").generation
    }

    fn storage_buffer(&self, bytes: u64) -> wgpu::Buffer {
        self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: bytes.max(16),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }
}

impl GpuNetwork {
    /// On the most capable GPU ([`Gpu::open`]), alone.
    pub fn new(weights: Weights) -> Result<Self, StemsError> {
        Self::with_options(weights, GpuOptions::default())
    }

    /// Accepts a software rasteriser too: for tests on machines without a GPU.
    pub fn new_allowing_software(weights: Weights) -> Result<Self, StemsError> {
        Self::with_options(weights, GpuOptions { software: true, ..Default::default() })
    }

    pub fn with_options(weights: Weights, options: GpuOptions) -> Result<Self, StemsError> {
        Self::on(&Gpu::open(options)?, &weights)
    }

    /// On `gpu`, which other networks may share: the products on its matrix units if the
    /// network suits them.
    pub fn on(gpu: &Arc<Gpu>, weights: &Weights) -> Result<Self, StemsError> {
        let c = &weights.geometry.config;
        if c.dim_head > 64 || !c.dim_head.is_multiple_of(4) {
            return Err(StemsError::Model(
                "the GPU attention takes heads of at most 64 values, in groups of 4".into(),
            ));
        }
        let matrix = gpu.matrix_units && fits_matrix_units(weights);
        if !matrix && gpu.attention == Some(Attention::Products) && !products_take(c) {
            return Err(StemsError::Model(
                "the attention as products takes heads of 8, 16, 32 or 64 values, projections in groups of 4"
                    .into(),
            ));
        }
        let pipelines = Pipelines::new(gpu, &weights.geometry, matrix);
        let params = Params::upload(&gpu.device, weights, matrix);
        let units = if matrix { ", tensor cores" } else { "" };
        let mut geometry = weights.geometry.clone();
        let fit = frames_that_fit(gpu.max_binding, &geometry, &params, matrix);
        let chunks = &mut geometry.config.chunk_frames;
        let shortened = fit < *chunks;
        if shortened {
            if fit < *chunks / SHORTEST_CHUNK {
                return Err(StemsError::Gpu(format!(
                    "{} binds buffers of {} bytes at most, too few for chunks of {} frames",
                    gpu.name,
                    gpu.max_binding,
                    *chunks / SHORTEST_CHUNK
                )));
            }
            *chunks = fit;
        }
        let c = &geometry.config;
        let cut = if shortened {
            format!(", chunks of {:.1} s", c.chunk_samples() as f64 / f64::from(c.sample_rate))
        } else {
            String::new()
        };
        let mut network = Self {
            gpu: gpu.clone(),
            geometry,
            adapter: format!("{}{units}{cut}", gpu.name),
            matrix,
            pipelines,
            params,
            plans: HashMap::new(),
            started: None,
            profiled: HashMap::new(),
        };
        gpu.check_errors()?;
        network.prepare(network.geometry.config.chunk_frames)?;
        Ok(network)
    }

    /// Seconds and steps of each kind of work so far, the longest first: empty unless the
    /// GPU was opened to profile ([`GpuOptions::profile`]) and can.
    pub fn profile(&self) -> Vec<(&'static str, f64, usize)> {
        let mut kinds: Vec<_> = self.profiled.iter().map(|(&kind, &(s, n))| (kind, s, n)).collect();
        kinds.sort_by(|a, b| b.1.total_cmp(&a.1));
        kinds
    }

    /// Runs `steps` one by one, timing each.
    fn profile_steps_of(&mut self, steps: &[&Step]) -> Result<(), StemsError> {
        let gpu = &self.gpu;
        let device = &gpu.device;
        let set = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: None,
            ty: wgpu::QueryType::Timestamp,
            count: 2,
        });
        let resolved = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 16,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 16,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let period = f64::from(gpu.queue.get_timestamp_period()) * 1e-9;
        let failed = |e: &dyn std::fmt::Display| StemsError::Gpu(e.to_string());
        for step in steps {
            let mut encoder = device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: None,
                    timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
                        query_set: &set,
                        beginning_of_pass_write_index: Some(0),
                        end_of_pass_write_index: Some(1),
                    }),
                });
                pass.set_pipeline(&step.pipeline);
                pass.set_bind_group(0, &step.bind_group, &[]);
                pass.dispatch_workgroups(step.groups.0, step.groups.1, 1);
            }
            encoder.resolve_query_set(&set, 0..2, &resolved, 0);
            encoder.copy_buffer_to_buffer(&resolved, 0, &read, 0, 16);
            gpu.queue.submit([encoder.finish()]);
            read.slice(..).map_async(wgpu::MapMode::Read, |_| ());
            device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| failed(&e))?;
            let ticks: Vec<u64> =
                bytemuck::cast_slice(&read.slice(..).get_mapped_range().map_err(|e| failed(&e))?).to_vec();
            read.unmap();
            let entry = self.profiled.entry(step.kind).or_default();
            entry.0 += ticks[1].saturating_sub(ticks[0]) as f64 * period;
            entry.1 += 1;
        }
        Ok(())
    }

    /// The GPU and graphics API in use, and its matrix units if the network runs on them.
    pub fn adapter(&self) -> &str {
        &self.adapter
    }

    /// Whether the products run on matrix units.
    pub fn uses_matrix_units(&self) -> bool {
        self.matrix
    }

    /// Whether the network runs on a software renderer (see [`GpuOptions::software`]).
    pub fn on_software_renderer(&self) -> bool {
        self.gpu.software
    }

    fn check_errors(&self) -> Result<(), StemsError> {
        self.gpu.check_errors()
    }

    /// Whether the largest activation fits in one buffer binding.
    fn check_limits(&self, frames: usize) -> Result<(), StemsError> {
        let seq_frames = self.geometry.config.seq_frames(frames) as u64;
        let bytes = seq_frames * frame_bytes(&self.geometry, &self.params, self.matrix);
        if bytes > self.gpu.max_binding {
            return Err(StemsError::Gpu(format!("{bytes} byte buffers exceed this GPU's limits")));
        }
        Ok(())
    }

    fn act_bytes(&self) -> usize {
        if self.matrix {
            2
        } else {
            4
        }
    }

    /// Plans the work for `frames`, again if a network sharing the GPU grew the scratch
    /// buffers since.
    fn prepare(&mut self, frames: usize) -> Result<(), StemsError> {
        let generation = self.gpu.scratch_generation();
        if self.plans.get(&frames).is_some_and(|p| p.generation == generation) {
            return Ok(());
        }
        // the plans binding replaced buffers keep them alive: they go first
        self.plans.retain(|_, p| p.generation == generation);
        self.check_limits(frames)?;
        let plan = Planner { net: self, frames }.build();
        self.check_errors()?;
        let built = plan.generation;
        self.plans.retain(|_, p| p.generation == built);
        self.plans.insert(frames, plan);
        Ok(())
    }
}

impl MaskNetwork for GpuNetwork {
    fn geometry(&self) -> &Geometry {
        &self.geometry
    }

    fn start(&mut self, bins: &[f32], frames: usize) -> Result<(), StemsError> {
        assert!(bins.len() == frames * self.geometry.features(), "bins hold `frames` rows of features");
        self.prepare(frames)?;
        if self.gpu.profile {
            let plan = self.plans.remove(&frames).expect("prepared");
            plan.load(&self.gpu, bins);
            let steps: Vec<&Step> = plan.submissions.iter().flatten().collect();
            let result = self.profile_steps_of(&steps);
            self.plans.insert(frames, plan);
            result?;
        }
        let plan = &self.plans[&frames];
        let gpu = &self.gpu;
        plan.load(gpu, bins);
        if let Some(share) = gpu.share {
            // packets of steps of about PACED_FLOPS, each waited for, then a pause; without
            // pauses, one packet waits queued behind the one running, so that the GPU is never
            // idle while the CPU hears of a packet's end and sends the next one
            let mut packet: Vec<&Step> = Vec::new();
            let mut flops = 0;
            let mut queued = None;
            let wait = |submission| {
                let done = wgpu::PollType::Wait { submission_index: Some(submission), timeout: None };
                gpu.device.poll(done).map(drop).map_err(|e| StemsError::Gpu(e.to_string()))
            };
            let steps: Vec<&Step> = plan.submissions.iter().flatten().collect();
            for (i, step) in steps.iter().enumerate() {
                packet.push(step);
                flops += step.flops;
                if flops < PACED_FLOPS && i + 1 < steps.len() {
                    continue;
                }
                let mut encoder = gpu.device.create_command_encoder(&Default::default());
                {
                    let mut pass = encoder.begin_compute_pass(&Default::default());
                    for step in packet.drain(..) {
                        pass.set_pipeline(&step.pipeline);
                        pass.set_bind_group(0, &step.bind_group, &[]);
                        pass.dispatch_workgroups(step.groups.0, step.groups.1, 1);
                    }
                }
                flops = 0;
                let began = std::time::Instant::now();
                let submitted = gpu.queue.submit([encoder.finish()]);
                if share < 1.0 {
                    wait(submitted)?;
                    std::thread::sleep(began.elapsed().mul_f64((1.0 - share) / share));
                } else if let Some(previous) = queued.replace(submitted) {
                    wait(previous)?;
                }
            }
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            encoder.copy_buffer_to_buffer(&plan.mask, 0, &plan.readback, 0, plan.mask.size());
            self.started = Some((frames, gpu.queue.submit([encoder.finish()])));
            return self.check_errors();
        }
        let mut last = None;
        for (i, steps) in plan.submissions.iter().enumerate() {
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                for step in steps {
                    pass.set_pipeline(&step.pipeline);
                    pass.set_bind_group(0, &step.bind_group, &[]);
                    pass.dispatch_workgroups(step.groups.0, step.groups.1, 1);
                }
            }
            if i + 1 == plan.submissions.len() {
                encoder.copy_buffer_to_buffer(&plan.mask, 0, &plan.readback, 0, plan.mask.size());
            }
            last = Some(gpu.queue.submit([encoder.finish()]));
        }
        self.started = Some((frames, last.expect("a plan has submissions")));
        self.check_errors()
    }

    fn finish(&mut self, out: &mut [f32]) -> Result<(), StemsError> {
        let (frames, submission) = self.started.take().expect("a chunk was started");
        assert!(out.len() == self.geometry.mask_len(frames), "masks hold `frames` rows per stem");
        let plan = &self.plans[&frames];
        let slice = plan.readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let done = wgpu::PollType::Wait { submission_index: Some(submission), timeout: None };
        self.gpu.device.poll(done).map_err(|e| StemsError::Gpu(e.to_string()))?;
        rx.recv().map_err(|e| StemsError::Gpu(e.to_string()))?.map_err(|e| StemsError::Gpu(e.to_string()))?;
        {
            let view = slice.get_mapped_range().map_err(|e| StemsError::Gpu(e.to_string()))?;
            out.copy_from_slice(&bytemuck::cast_slice(&view)[..out.len()]);
        }
        plan.readback.unmap();
        self.check_errors()
    }
}

/// The most capable GPU, through Vulkan when it reaches a hardware one (its compiler makes
/// the matrix products faster than Direct3D 12's, and it reaches the matrix units), else
/// whatever API reaches one, and whether it was picked by name. `WGPU_BACKEND` restricts the APIs,
/// `WGPU_ADAPTER_NAME` picks a GPU by (part of) its name.
/// The adapter worth separating on and whether its matrix units are used: see [`Gpu::open`].
fn worthy_adapter(options: GpuOptions) -> Result<(wgpu::Adapter, bool), StemsError> {
    let (adapter, picked) = adapter()?;
    let info = adapter.get_info();
    if info.device_type == wgpu::DeviceType::Cpu && !options.software {
        return Err(StemsError::NoGpu(format!("{} is a software renderer", info.name)));
    }
    let matrix_units = !options.without_matrix_units && has_matrix_units(&adapter);
    // Apple's own GPUs are integrated, and anything but slow; so is a phone's, beside its CPU
    let slow = info.device_type == wgpu::DeviceType::IntegratedGpu
        && info.vendor != APPLE
        && !cfg!(target_os = "android");
    if slow && !matrix_units && !picked {
        return Err(StemsError::NoGpu(format!("{} is an integrated GPU, slower than the CPU", info.name)));
    }
    Ok((adapter, matrix_units))
}

fn adapter() -> Result<(wgpu::Adapter, bool), StemsError> {
    let instance = |backends: wgpu::Backends| {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = backends;
        wgpu::Instance::new(desc.with_env())
    };
    if let Some(wanted) = std::env::var_os("WGPU_ADAPTER_NAME") {
        let wanted = wanted.to_string_lossy().to_lowercase();
        let mut adapters =
            pollster::block_on(instance(wgpu::Backends::PRIMARY).enumerate_adapters(wgpu::Backends::PRIMARY));
        adapters.sort_by_key(|a| a.get_info().backend != wgpu::Backend::Vulkan);
        return adapters
            .into_iter()
            .find(|a| a.get_info().name.to_lowercase().contains(&wanted))
            .map(|a| (a, true))
            .ok_or_else(|| StemsError::NoGpu(format!("no GPU named like {wanted}")));
    }
    let request = |backends: wgpu::Backends| {
        pollster::block_on(instance(backends).request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
    };
    let hardware = |a: &wgpu::Adapter| a.get_info().device_type != wgpu::DeviceType::Cpu;
    [wgpu::Backends::VULKAN, wgpu::Backends::DX12]
        .into_iter()
        .find_map(|backends| request(backends).ok().filter(hardware))
        .map_or_else(|| request(wgpu::Backends::PRIMARY), Ok)
        .map(|a| (a, false))
        .map_err(|e| StemsError::NoGpu(e.to_string()))
}

/// The scores of the attention as products are worked out a slice of the sequences and heads
/// at a time, in this much memory at most (one sequence and head at least).
const SCORES_BYTES: u64 = 32 << 20;

/// When the GPU is shared ([`GpuOptions::share`]): a piece of a product, about 3 ms on an
/// Adreno 740, and the scores of the attention worked out at once.
const PACED_FLOPS: usize = 1 << 30;
const PACED_SCORES_BYTES: u64 = 8 << 20;

/// The side of the square of outputs a workgroup of `matmul_blocked.wgsl` computes.
const BLOCKED_TILE: usize = 64;

/// A GPU binding too little for a chunk the length the model was made for takes shorter
/// ones, down to this fraction of it: shorter, a chunk would hear too little of the music.
const SHORTEST_CHUNK: usize = 4;

/// The largest buffer the device lets a shader bind.
fn binding_limit(limits: &wgpu::Limits) -> u64 {
    limits.max_storage_buffer_binding_size.min(limits.max_buffer_size)
}

/// The bytes of the largest activation, for each token along the time of the transformers'
/// sequences.
fn frame_bytes(geometry: &Geometry, params: &Params, matrix: bool) -> u64 {
    let c = &geometry.config;
    let act = if matrix { 2 } else { 4 };
    let widest = c.qkvg_width().max(params.ff_hidden);
    let frames = if params.grouping.is_some() { c.group * params.band_dim * 4 } else { 0 };
    (geometry.seq_bands() * (widest * act).max(c.dim * 4).max(frames)) as u64
}

/// The most frames a chunk can take on a GPU binding `max_binding` bytes, at most the model's.
fn frames_that_fit(max_binding: u64, geometry: &Geometry, params: &Params, matrix: bool) -> usize {
    let fit = max_binding / frame_bytes(geometry, params, matrix);
    let c = &geometry.config;
    let tokens = usize::try_from(fit).unwrap_or(usize::MAX).saturating_sub(c.registers);
    tokens.saturating_mul(c.group).min(c.chunk_frames)
}

/// PCI vendor ids.
const NVIDIA: u32 = 0x10de;
const APPLE: u32 = 0x106b;

const MATRIX_FEATURES: wgpu::Features = wgpu::Features::SHADER_F16
    .union(wgpu::Features::SUBGROUP)
    .union(wgpu::Features::EXPERIMENTAL_COOPERATIVE_MATRIX);

/// Shared memory the attention on the matrix units takes.
const MATRIX_ATTENTION_SHARED: u32 = 23_056;

/// Whether the shaders written for the matrix units run here: 16 x 16 x 16 products of f16
/// summed in f32, by subgroups of 32 invocations.
fn has_matrix_units(adapter: &wgpu::Adapter) -> bool {
    let info = adapter.get_info();
    adapter.features().contains(MATRIX_FEATURES)
        && info.subgroup_min_size == 32
        && info.subgroup_max_size == 32
        && adapter.limits().max_compute_invocations_per_workgroup >= 256
        && adapter.cooperative_matrix_properties().iter().any(|p| {
            (p.m_size, p.n_size, p.k_size) == (16, 16, 16)
                && p.ab_type == wgpu::CooperativeScalarType::F16
                && p.cr_type == wgpu::CooperativeScalarType::F32
        })
}

/// Whether the products suit the network: they read their inputs 4 values at a time.
fn fits_matrix_units(weights: &Weights) -> bool {
    weights.geometry.config.dim.is_multiple_of(4) && weights.linears().all(|l| l.inp.is_multiple_of(4))
}

/// Whether the attention runs on the matrix units: heads of 64 or 32 values, and the queries,
/// keys and values of a token read 4 values at a time.
fn matrix_attention(device: &wgpu::Device, geometry: &Geometry) -> bool {
    let c = &geometry.config;
    matches!(c.dim_head, 32 | 64)
        && c.qkvg_width().is_multiple_of(4)
        && device.limits().max_compute_workgroup_storage_size >= MATRIX_ATTENTION_SHARED
}

/// Whether the attention on the shader cores runs as products, unless chosen
/// ([`GpuOptions::attention`]): for gated heads of 64 values, each with its own keys and
/// values, if it takes them. Smaller heads hold less in their registers, and their scores would weigh more beside
/// their products (on an RTX 4060, the products take 1.5 times as long as `attention.wgsl` for
/// Mini-BS-RoFormer-18M's heads of 32 values).
fn attention_as_products(c: &Config) -> bool {
    c.dim_head == 64 && c.kv_heads == c.heads && c.gates && products_take(c)
}

/// Whether the attention as products takes the network's heads: each thread of
/// `attention_values.wgsl` takes 8 of a head's values, a workgroup 64 threads.
fn products_take(c: &Config) -> bool {
    matches!(c.dim_head, 8 | 16 | 32 | 64) && c.qkvg_width().is_multiple_of(4)
}

/// The bytes of the scores of a sequence and head of `len` tokens, each query's padded to whole
/// vectors of 4.
fn scores_bytes(len: usize) -> u64 {
    (len * len.next_multiple_of(4) * 4) as u64
}

/// The queries a workgroup of `attention_values.wgsl` takes, for heads of `dim_head` values.
fn value_queries(dim_head: usize) -> usize {
    8 * 64 / (dim_head / 8)
}

/// The heads the attentions are compiled for, defined before their sources: constants rather
/// than overrides or parameters, so that the gated heads of 64 values compile as they did
/// before the others came.
fn heads_shape(c: &Config) -> String {
    let (dh, gated) = (c.dim_head, c.gates);
    format!("const DH: u32 = {dh}u;\nconst DH4: u32 = {}u;\nconst GATED: bool = {gated};\n", dh / 4)
}

impl Pipelines {
    fn new(gpu: &Gpu, geometry: &Geometry, matrix: bool) -> Self {
        let device = &gpu.device;
        let header = if matrix {
            "enable f16;\nenable wgpu_cooperative_matrix;\nalias Act = f16;\n"
        } else {
            "alias Act = f32;\n"
        };
        let make = |source: &str, entry: &str| {
            let descriptor = wgpu::ShaderModuleDescriptor {
                label: Some(entry),
                source: wgpu::ShaderSource::Wgsl(format!("{header}{source}").into()),
            };
            // SAFETY: the shaders bound every index themselves (each global access is guarded
            // by the sizes in its parameters, and the plan sizes every buffer for them); the
            // checks wgpu would add make the matrix products 20 to 30 times slower on Vulkan.
            let module = unsafe {
                device.create_shader_module_trusted(descriptor, wgpu::ShaderRuntimeChecks::unchecked())
            };
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: None,
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let matmul = if matrix {
            [("f32", "f32"), ("f32", "f16"), ("f16", "f32"), ("f16", "f16")].map(|(input, output)| {
                let types = format!("alias In = {input};\nalias Out = {output};\n");
                make(&format!("{types}{}", include_str!("matmul_coop.wgsl")), "main")
            })
        } else {
            let source =
                if gpu.large_tiles { include_str!("matmul.wgsl") } else { include_str!("matmul_small.wgsl") };
            let f32 = make(source, "main");
            [f32.clone(), f32.clone(), f32.clone(), f32]
        };
        let matmul_tile = if matrix || gpu.large_tiles { 128 } else { 64 };
        let blocked = (!matrix).then(|| make(include_str!("matmul_blocked.wgsl"), "main"));
        let matrix_attention = matrix && matrix_attention(device, geometry);
        let c = &geometry.config;
        let shape = heads_shape(c);
        let products = gpu.attention.map_or(attention_as_products(c), |a| a == Attention::Products);
        let attention_products = (!matrix && products).then(|| {
            [
                make(&format!("{shape}{}", include_str!("attention_scores.wgsl")), "main"),
                make(include_str!("attention_softmax.wgsl"), "main"),
                make(&format!("{shape}{}", include_str!("attention_values.wgsl")), "main"),
            ]
        });
        let source = if matrix_attention {
            include_str!("attention_coop.wgsl")
        } else {
            include_str!("attention.wgsl")
        };
        let attention = make(&format!("{shape}{source}"), "main");
        let pool = geometry
            .config
            .stem_queries
            .then(|| make(&format!("{shape}{}", include_str!("pool.wgsl")), "main"));
        Self {
            matmul,
            matmul_tile,
            blocked,
            norm: make(include_str!("norm.wgsl"), "normalize"),
            norm_in_place: make(include_str!("norm_in_place.wgsl"), "normalize_in_place"),
            rotary: make(include_str!("rotary.wgsl"), "main"),
            attention,
            matrix_attention,
            attention_products,
            glu: make(include_str!("glu.wgsl"), "main"),
            gated: make(include_str!("gated.wgsl"), "main"),
            add: make(include_str!("add.wgsl"), "main"),
            pool,
        }
    }

    fn matmul(&self, input: Store, output: Store) -> &wgpu::ComputePipeline {
        &self.matmul[2 * usize::from(input == Store::Act) + usize::from(output == Store::Act)]
    }

    /// What `pipeline` does, for [`GpuNetwork::profile`].
    fn kind(&self, pipeline: &wgpu::ComputePipeline) -> &'static str {
        let named = [
            (Some(&self.norm), "norm"),
            (Some(&self.norm_in_place), "norm"),
            (Some(&self.rotary), "rotary"),
            (Some(&self.attention), "attention"),
            (Some(&self.glu), "glu"),
            (Some(&self.gated), "glu"),
            (Some(&self.add), "add"),
            (self.pool.as_ref(), "pool"),
            (self.blocked.as_ref(), "products (blocked)"),
            (self.attention_products.as_ref().map(|p| &p[0]), "attention scores"),
            (self.attention_products.as_ref().map(|p| &p[1]), "attention softmax"),
            (self.attention_products.as_ref().map(|p| &p[2]), "attention values"),
        ];
        named.into_iter().find(|(p, _)| *p == Some(pipeline)).map_or("products (tiles)", |(_, kind)| kind)
    }
}

impl Params {
    fn upload(device: &wgpu::Device, w: &Weights, matrix: bool) -> Self {
        let c = &w.geometry.config;
        let layers = || w.transformers.iter().flat_map(|t| &t.layers);
        // the blocked products read the norms of the stream four values at a time
        let gated_products =
            !matrix && c.ff_glu && c.dim.is_multiple_of(4) && layers().all(|l| l.ff1.out.is_multiple_of(8));
        let linear = |l: &Linear| {
            // the matrix units read rows of whole groups of 4, which the networks they run
            // have (`fits_matrix_units`); the shader cores read pairs of values,
            // the last one padded by `storage`
            let ld = if matrix { l.inp / 4 } else { l.inp };
            GpuLinear {
                weight: storage(device, bytemuck::cast_slice(&l.weight.to_f16())),
                bias: l.bias.as_ref().map(|b| storage(device, bytemuck::cast_slice(b))),
                out: l.out,
                inp: l.inp,
                ld,
            }
        };
        let transformers = w
            .transformers
            .iter()
            .map(|tr| {
                let layers = tr
                    .layers
                    .iter()
                    .map(|l| GpuLayer {
                        qkvg: linear(&l.qkvg),
                        out: linear(&l.out),
                        ff1: if gated_products {
                            linear(&gates_beside_values(&l.ff1))
                        } else {
                            linear(&l.ff1)
                        },
                        ff2: linear(&l.ff2),
                    })
                    .collect();
                (layers, tr.norm.as_ref().map(|n| storage(device, bytemuck::cast_slice(n))))
            })
            .collect();
        let ff_hidden = layers().map(|l| l.ff1.out / if gated_products { 2 } else { 1 }).max().unwrap_or(0);
        let band_dim = w.band_dim();
        let grouping = w.grouping.as_ref().map(|g| GpuGrouping {
            input: [linear(&g.input.ff1), linear(&g.input.ff2)],
            output: linear(&g.output.ff1),
            frames: (0..w.geometry.config.group)
                .map(|f| linear(&g.output.ff2.rows(f * band_dim, band_dim)))
                .collect(),
        });
        let pool = |p: &Pool| GpuPool {
            queries: storage(device, bytemuck::cast_slice(&p.queries)),
            count: p.queries.len() / w.geometry.config.inner_dim(),
            kv: linear(&p.kv),
            out: linear(&p.out),
            ff: [linear(&p.ff.ff1), linear(&p.ff.ff2)],
        };
        let mask_hidden = w
            .masks
            .iter()
            .flatten()
            .flat_map(|m| m.split_last().map_or(&[][..], |(_, hidden)| hidden))
            .map(|l| l.out)
            .max()
            .unwrap_or(0);
        Self {
            rotary_time: w.rotary_time.clone(),
            rotary_freq: w.rotary_freq.clone(),
            band_proj: w.band_proj.iter().map(linear).collect(),
            band_dim,
            grouping,
            transformers,
            final_norm: w.final_norm.as_ref().map(|n| storage(device, bytemuck::cast_slice(n))),
            registers: w.registers.clone(),
            pools: w.stem_queries.as_ref().map(|q| [pool(&q.bands), pool(&q.tokens)]),
            masks: w
                .masks
                .iter()
                .map(|stem| stem.iter().map(|m| m.iter().map(linear).collect()).collect())
                .collect(),
            no_bias: storage(device, bytemuck::cast_slice(&[0.0f32; 4])),
            ff_hidden,
            mask_hidden,
            gated_products,
        }
    }
}

/// The first product of a gated feed-forward, its `out / 2` gates then their values, with its
/// rows in groups of 4 gates then their 4 values: the columns of a thread of
/// `matmul_blocked.wgsl`, which then gives `gelu(gate) * value` itself.
fn gates_beside_values(l: &Linear) -> Linear {
    let half = l.out / 2;
    let rows: Vec<usize> =
        (0..half).step_by(4).flat_map(|g| (g..g + 4).chain(half + g..half + g + 4)).collect();
    let weight = l.weight.to_f16();
    Linear {
        weight: Values::F16(rows.iter().flat_map(|&r| &weight[r * l.inp..][..l.inp]).copied().collect()),
        bias: l.bias.as_ref().map(|b| rows.iter().map(|&r| b[r]).collect()),
        out: l.out,
        inp: l.inp,
    }
}

struct Planner<'a> {
    net: &'a GpuNetwork,
    frames: usize,
}

impl Planner<'_> {
    fn build(self) -> Plan {
        let g = &self.net.geometry;
        let c = &g.config;
        let p = &self.net.params;
        let frames = self.frames;
        let token_frames = c.token_frames(frames);
        let (seq_frames, seq_bands) = self.seq();
        let tokens = seq_frames * seq_bands;
        // the tokens of the frames (the registers' columns among them), and their frames' values
        let frame_tokens = token_frames * seq_bands;
        let grouped = frame_tokens * c.group * p.band_dim;
        let features = g.features();
        let mask_features = g.mask_features();
        // the rows of the mask estimators: every frame of the tokens
        let mask_rows = token_frames * c.group;
        let closing = c.mask_glu || c.pair_gates || p.grouping.is_some();
        let widest_band = g.bands.iter().map(|b| b.width).max().unwrap_or(0);
        let mask_out = p.masks.iter().flatten().filter_map(|m| m.last()).map(|l| l.out).max().unwrap_or(0);
        let f32s = |n: usize| (n.max(1).next_multiple_of(4) * 4) as u64;
        let acts = |n: usize| (n.max(1).next_multiple_of(4) * self.net.act_bytes()) as u64;
        let mut bytes = [0; SLOTS];
        bytes[Slot::X as usize] = f32s(tokens * c.dim);
        bytes[Slot::Earlier as usize] = if c.skip { f32s(tokens * c.dim) } else { 0 };
        bytes[Slot::Normed as usize] = acts((tokens * c.dim).max(grouped));
        bytes[Slot::Qkvg as usize] = acts(tokens * c.qkvg_width());
        bytes[Slot::Attended as usize] = acts(tokens * c.inner_dim());
        let sliced = p.grouping.iter().flat_map(|g| [&g.input[0], &g.output]);
        let sliced = sliced.chain(p.pools.iter().flatten().map(|pool| &pool.ff[0]));
        let row = sliced.map(|l| l.out).max().unwrap_or(0);
        bytes[Slot::Hidden as usize] = acts((tokens * p.ff_hidden).max(row));
        bytes[Slot::BandIn as usize] = acts(frames * widest_band);
        bytes[Slot::Mlp0 as usize] = acts(mask_rows * p.mask_hidden);
        bytes[Slot::Mlp1 as usize] = acts(mask_rows * p.mask_hidden);
        bytes[Slot::MlpOut as usize] = if closing { f32s(mask_rows * mask_out) } else { 0 };
        bytes[Slot::Frames as usize] = if p.grouping.is_some() { f32s(grouped) } else { 0 };
        bytes[Slot::Scores as usize] = if self.net.pipelines.attention_products.is_some() {
            [Axis::Time, Axis::Freq]
                .map(|axis| {
                    let len = axis.len(seq_frames, seq_bands);
                    let pairs = axis.groups(seq_frames, seq_bands) * c.heads;
                    self.scores_slice(len, pairs) as u64 * scores_bytes(len)
                })
                .into_iter()
                .max()
                .unwrap_or(0)
        } else {
            0
        };
        let (generation, scratch) = self.net.gpu.scratch(bytes);
        let slot = |s: Slot| scratch[s as usize].clone();

        let bins = self.buffer(Store::F32, frames * features, wgpu::BufferUsages::COPY_DST);
        let mask = self.buffer(Store::F32, g.mask_len(frames), wgpu::BufferUsages::COPY_SRC);
        let readback = self.net.gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: mask.size(),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let x = slot(Slot::X);
        let earlier = c.skip.then(|| slot(Slot::Earlier));
        let normed = slot(Slot::Normed);
        let qkvg = slot(Slot::Qkvg);
        let attended = slot(Slot::Attended);
        let hidden = slot(Slot::Hidden);
        let band_in = slot(Slot::BandIn);
        let mlp = [slot(Slot::Mlp0), slot(Slot::Mlp1)];
        let mlp_out = slot(Slot::MlpOut);
        let split = slot(Slot::Frames);
        let scores = slot(Slot::Scores);
        let device = &self.net.gpu.device;
        let bands = g.bands.len();
        let angles_time =
            storage(device, bytemuck::cast_slice(&angles(&p.rotary_time, token_frames, seq_frames)));
        let angles_freq = storage(device, bytemuck::cast_slice(&angles(&p.rotary_freq, bands, seq_bands)));
        let x_rows = Input { buffer: &x, store: Store::F32, ld: c.dim, off: 0 };

        let mut submissions = Vec::new();
        let mut steps = Vec::new();
        if c.registers > 0 {
            // the registers in their corner, zeros elsewhere: the band split fills the rest
            let mut start = vec![0.0; tokens * c.dim];
            for (r, register) in p.registers.chunks_exact(c.dim).enumerate() {
                start[g.register_token(frames, r) * c.dim..][..c.dim].copy_from_slice(register);
            }
            steps.push(self.add(&storage(device, bytemuck::cast_slice(&start)), &x, start.len(), false));
        }
        for (b, (band, proj)) in g.bands.iter().zip(&p.band_proj).enumerate() {
            steps.push(self.norm(
                &bins,
                &band_in,
                frames,
                band.width,
                (features, band.offset),
                (band.width, 0),
            ));
            if p.grouping.is_none() {
                let input = Input { buffer: &band_in, store: Store::Act, ld: band.width, off: 0 };
                let into = Input { buffer: &x, store: Store::F32, ld: seq_bands * c.dim, off: b * c.dim };
                steps.extend(self.matmul(proj, input, frames, into, Act::None, false));
                continue;
            }
            // frame f of the band to frame f % group of token f / group
            for f in 0..c.group.min(frames) {
                let (ld, off) = (c.group * band.width, f * band.width);
                let input = Input { buffer: &band_in, store: Store::Act, ld, off };
                let (ld, off) = (seq_bands * c.group * p.band_dim, (b * c.group + f) * p.band_dim);
                let into = Input { buffer: &split, store: Store::F32, ld, off };
                let rows = (frames - f).div_ceil(c.group);
                steps.extend(self.matmul(proj, input, rows, into, Act::None, false));
            }
        }
        if let Some(grouping) = &p.grouping {
            let rows = (p.band_dim, 0);
            steps.push(self.norm(&split, &normed, frame_tokens * c.group, p.band_dim, rows, rows));
            let input = Input { buffer: &normed, store: Store::Act, ld: c.group * p.band_dim, off: 0 };
            steps.extend(self.feed_forward(&grouping.input, input, frame_tokens, &hidden, x_rows, false));
        }
        submissions.push(std::mem::take(&mut steps));

        let normed_in = Input { buffer: &normed, store: Store::Act, ld: c.dim, off: 0 };
        for (i, (layers, gain)) in p.transformers.iter().enumerate() {
            let axis = if i % 2 == 0 { Axis::Time } else { Axis::Freq };
            let angles = if axis == Axis::Time { &angles_time } else { &angles_freq };
            if let Some(earlier) = earlier.as_ref().filter(|_| axis == Axis::Time && i > 0) {
                steps.push(self.add(earlier, &x, tokens * c.dim, true));
            }
            for l in layers {
                steps.push(self.norm(&x, &normed, tokens, c.dim, (c.dim, 0), (c.dim, 0)));
                let into = Input { buffer: &qkvg, store: Store::Act, ld: c.qkvg_width(), off: 0 };
                steps.extend(self.matmul(&l.qkvg, normed_in, tokens, into, Act::None, false));
                if !self.net.pipelines.matrix_attention {
                    steps.push(self.rotary(&qkvg, angles, axis, tokens));
                }
                steps.extend(self.attention(&qkvg, angles, &attended, &scores, axis));
                let input = Input { buffer: &attended, store: Store::Act, ld: c.inner_dim(), off: 0 };
                steps.extend(self.matmul(&l.out, input, tokens, x_rows, Act::None, true));
                steps.push(self.norm(&x, &normed, tokens, c.dim, (c.dim, 0), (c.dim, 0)));
                let width = if p.gated_products { l.ff1.out / 2 } else { l.ff1.out };
                let into = Input { buffer: &hidden, store: Store::Act, ld: width, off: 0 };
                if p.gated_products {
                    let act = if c.silu { Act::GatedSilu } else { Act::Gated };
                    steps.extend(self.matmul(&l.ff1, normed_in, tokens, into, act, false));
                } else if c.ff_glu {
                    steps.extend(self.matmul(&l.ff1, normed_in, tokens, into, Act::None, false));
                    steps.push(self.gated(&hidden, tokens, l.ff1.out / 2));
                } else {
                    steps.extend(self.matmul(&l.ff1, normed_in, tokens, into, Act::Gelu, false));
                }
                steps.extend(self.matmul(&l.ff2, into, tokens, x_rows, Act::None, true));
            }
            if let Some(gain) = gain {
                steps.push(self.norm_in_place(&x, gain, tokens, c.dim));
            }
            if let Some(earlier) = earlier.as_ref().filter(|_| axis == Axis::Freq) {
                steps.push(self.add(&x, earlier, tokens * c.dim, i > 1));
            }
            submissions.push(std::mem::take(&mut steps));
        }
        if let Some(gain) = &p.final_norm {
            steps.push(self.norm_in_place(&x, gain, tokens, c.dim));
        }

        // with stem queries, each stem's row as the bias of the first product of the frames back
        let mut frames_back: Vec<Option<GpuLinear>> = vec![None; g.stems.len()];
        if let (Some([bands_pool, tokens_pool]), Some(grouping)) = (&p.pools, &p.grouping) {
            let work = PoolWork { keys: &qkvg, attended: &attended, normed: &normed, hidden: &hidden };
            let pooled = self.buffer(Store::F32, token_frames * c.dim, wgpu::BufferUsages::empty());
            let stems = self.buffer(Store::F32, g.stems.len() * c.dim, wgpu::BufferUsages::empty());
            steps.extend(self.pool(bands_pool, x_rows, (token_frames, bands, seq_bands), &pooled, &work));
            let pooled_rows = Input { buffer: &pooled, store: Store::F32, ld: c.dim, off: 0 };
            steps.extend(self.pool(tokens_pool, pooled_rows, (1, token_frames, 0), &stems, &work));
            for (s, back) in frames_back.iter_mut().enumerate() {
                let out = grouping.output.out;
                let bias = self.buffer(Store::F32, out, wgpu::BufferUsages::empty());
                let row = Input { buffer: &stems, store: Store::F32, ld: c.dim, off: s * c.dim };
                let into = Input { buffer: &bias, store: Store::F32, ld: out, off: 0 };
                steps.extend(self.matmul(&grouping.output, row, 1, into, Act::None, false));
                *back = Some(GpuLinear { bias: Some(bias), ..grouping.output.clone() });
            }
        }

        for (s, back) in frames_back.iter().enumerate() {
            let stem_off = s * frames * mask_features;
            let estimators = &p.masks[if c.stem_queries { 0 } else { s }];
            let (rows, width, base) = match &p.grouping {
                None => {
                    (frames, c.dim, Input { buffer: &x, store: Store::F32, ld: seq_bands * c.dim, off: 0 })
                }
                Some(grouping) => {
                    let ff1 = back.as_ref().unwrap_or(&grouping.output);
                    let into = Input { buffer: &hidden, store: Store::Act, ld: ff1.out, off: 0 };
                    for tokens in self.hidden_slices(frame_tokens, ff1.out, &hidden) {
                        let n = tokens.len();
                        let input = Input { off: tokens.start * c.dim, ..x_rows };
                        steps.extend(self.matmul(ff1, input, n, into, Act::None, false));
                        steps.push(self.gated(&hidden, n, ff1.out / 2));
                        // frame f of every token in the f-th block of rows: the estimators read
                        // the frames of a band in one stride, the glu step puts them in order
                        for (f, l) in grouping.frames.iter().enumerate() {
                            let off = (f * frame_tokens + tokens.start) * p.band_dim;
                            let out = Input { buffer: &split, store: Store::F32, ld: p.band_dim, off };
                            steps.extend(self.matmul(l, into, n, out, Act::None, false));
                        }
                    }
                    let rows = (p.band_dim, 0);
                    steps.push(self.norm(&split, &normed, frame_tokens * c.group, p.band_dim, rows, rows));
                    let ld = seq_bands * p.band_dim;
                    (mask_rows, p.band_dim, Input { buffer: &normed, store: Store::Act, ld, off: 0 })
                }
            };
            for (b, (band, mlp_layers)) in g.mask_bands.iter().zip(estimators).enumerate() {
                let mut input = Input { off: b * width, ..base };
                for (k, l) in mlp_layers.iter().enumerate() {
                    let last = k + 1 == mlp_layers.len();
                    let into = if !last {
                        Input { buffer: &mlp[k % 2], store: Store::Act, ld: l.out, off: 0 }
                    } else if closing {
                        Input { buffer: &mlp_out, store: Store::F32, ld: l.out, off: 0 }
                    } else {
                        Input {
                            buffer: &mask,
                            store: Store::F32,
                            ld: mask_features,
                            off: stem_off + band.offset,
                        }
                    };
                    let act = if last { Act::None } else { Act::Tanh };
                    steps.extend(self.matmul(l, input, rows, into, act, false));
                    input = into;
                }
                if closing {
                    steps.push(self.glu(
                        &mlp_out,
                        &mask,
                        frames,
                        band.width,
                        mask_features,
                        stem_off + band.offset,
                    ));
                }
            }
            submissions.push(std::mem::take(&mut steps));
        }

        let cleared = p.grouping.is_some().then_some(split);
        Plan { generation, bins, cleared, mask, readback, submissions }
    }

    /// Frames and bands of the transformers' sequences.
    fn seq(&self) -> (usize, usize) {
        (self.net.geometry.config.seq_frames(self.frames), self.net.geometry.seq_bands())
    }

    /// Room for `count` values stored as `store`, in whole groups of 4.
    fn buffer(&self, store: Store, count: usize, usage: wgpu::BufferUsages) -> wgpu::Buffer {
        let bytes = if store == Store::Act { self.net.act_bytes() } else { 4 };
        self.net.gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (count.max(1).next_multiple_of(4) * bytes) as u64,
            usage: wgpu::BufferUsages::STORAGE | usage,
            mapped_at_creation: false,
        })
    }

    fn step(
        &self,
        pipeline: &wgpu::ComputePipeline,
        groups: (u32, u32),
        buffers: &[&wgpu::Buffer],
        uniform: &[u32],
    ) -> Step {
        let device = &self.net.gpu.device;
        let mut words = uniform.to_vec();
        words.resize(uniform.len().next_multiple_of(4), 0);
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&words),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let entries: Vec<wgpu::BindGroupEntry> = buffers
            .iter()
            .chain([&&uniform])
            .enumerate()
            .map(|(i, b)| wgpu::BindGroupEntry { binding: i as u32, resource: b.as_entire_binding() })
            .collect();
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &entries,
        });
        Step {
            pipeline: pipeline.clone(),
            bind_group,
            groups,
            kind: self.net.pipelines.kind(pipeline),
            flops: 0,
        }
    }

    /// `out` (+)= act(`a` · `l`) over `rows` rows: in pieces of [`PACED_FLOPS`] when the GPU is
    /// shared ([`GpuOptions::share`]), else at once.
    fn matmul(
        &self,
        l: &GpuLinear,
        a: Input,
        rows: usize,
        out: Input,
        act: Act,
        accumulate: bool,
    ) -> Vec<Step> {
        let per_row = 2 * l.out * l.inp;
        let piece = match self.net.gpu.share {
            Some(_) => (PACED_FLOPS / per_row.max(1))
                .max(1)
                .next_multiple_of(BLOCKED_TILE.max(self.net.pipelines.matmul_tile)),
            None => rows,
        };
        (0..rows)
            .step_by(piece.max(1))
            .map(|first| {
                let a = Input { off: a.off + first * a.ld, ..a };
                let out = Input { off: out.off + first * out.ld, ..out };
                self.matmul_rows(l, a, piece.min(rows - first), out, act, accumulate)
            })
            .collect()
    }

    fn matmul_rows(
        &self,
        l: &GpuLinear,
        a: Input,
        rows: usize,
        out: Input,
        act: Act,
        accumulate: bool,
    ) -> Step {
        let bias = l.bias.as_ref().unwrap_or(&self.net.params.no_bias);
        let pipelines = &self.net.pipelines;
        let aligned = l.inp % 4 == 0 && a.ld % 4 == 0 && a.off % 4 == 0;
        let (pipeline, tile) = match pipelines.blocked.as_ref().filter(|_| aligned) {
            Some(blocked) => (blocked, BLOCKED_TILE),
            None => {
                assert!(!matches!(act, Act::Gated | Act::GatedSilu), "the gated products are blocked");
                (pipelines.matmul(a.store, out.store), pipelines.matmul_tile)
            }
        };
        let groups = (l.out.div_ceil(tile) as u32, rows.div_ceil(tile) as u32);
        let uniform = [
            rows,
            l.out,
            l.inp,
            a.ld,
            a.off,
            out.ld,
            out.off,
            act as usize,
            usize::from(accumulate),
            usize::from(l.bias.is_some()),
            l.ld,
        ];
        let step = self.step(pipeline, groups, &[a.buffer, &l.weight, bias, out.buffer], &words(&uniform));
        Step { flops: 2 * rows * l.out * l.inp, ..step }
    }

    fn norm(
        &self,
        src: &wgpu::Buffer,
        dst: &wgpu::Buffer,
        rows: usize,
        width: usize,
        (src_ld, src_off): (usize, usize),
        (dst_ld, dst_off): (usize, usize),
    ) -> Step {
        let (groups, groups_x) = grid(rows);
        let uniform = [rows, width, src_ld, src_off, dst_ld, dst_off, 0, groups_x, self.norm_eps(width)];
        let buffers = [src, dst, &self.net.params.no_bias];
        self.step(&self.net.pipelines.norm, groups, &buffers, &words(&uniform))
    }

    fn norm_in_place(&self, x: &wgpu::Buffer, gain: &wgpu::Buffer, rows: usize, width: usize) -> Step {
        let (groups, groups_x) = grid(rows);
        let uniform = [rows, width, width, 0, width, 0, 1, groups_x, self.norm_eps(width)];
        self.step(&self.net.pipelines.norm_in_place, groups, &[x, gain], &words(&uniform))
    }

    /// What the norms of rows of `width` add to their sums of squares, as the bits of an f32.
    fn norm_eps(&self, width: usize) -> usize {
        (self.net.geometry.config.norm_eps * width as f32).to_bits() as usize
    }

    fn rotary(&self, qkvg: &wgpu::Buffer, angles: &wgpu::Buffer, axis: Axis, tokens: usize) -> Step {
        let c = &self.net.geometry.config;
        let (seq_frames, seq_bands) = self.seq();
        let pairs = c.dim_head / 2;
        // the queries' heads then the keys', one after the other
        let heads = c.heads + c.kv_heads;
        let (groups, groups_x) = grid((tokens * heads * pairs).div_ceil(256));
        let positions = axis.len(seq_frames, seq_bands);
        let uniform = [tokens, heads, pairs, c.qkvg_width(), seq_bands, axis_id(axis), positions, groups_x];
        self.step(&self.net.pipelines.rotary, groups, &[qkvg, angles], &words(&uniform))
    }

    /// How many sequences and heads of `len` tokens, of `pairs`, have their scores at once:
    /// as many as [`SCORES_BYTES`] and the GPU's binding hold, one at least.
    fn scores_slice(&self, len: usize, pairs: usize) -> usize {
        let bytes = if self.net.gpu.share.is_some() { PACED_SCORES_BYTES } else { SCORES_BYTES };
        let budget = bytes.min(self.net.gpu.max_binding);
        usize::try_from(budget / scores_bytes(len)).unwrap_or(usize::MAX).clamp(1, pairs.max(1))
    }

    fn attention(
        &self,
        qkvg: &wgpu::Buffer,
        angles: &wgpu::Buffer,
        out: &wgpu::Buffer,
        scores: &wgpu::Buffer,
        axis: Axis,
    ) -> Vec<Step> {
        let c = &self.net.geometry.config;
        let (frames, bands) = self.seq();
        let len = axis.len(frames, bands);
        if let Some([products, softmax, values]) = &self.net.pipelines.attention_products {
            let pairs = axis.groups(frames, bands) * c.heads;
            // as many as the scores buffer holds, and the GPU binds
            let held = scores.size().min(self.net.gpu.max_binding) / scores_bytes(len);
            assert!(held > 0, "the scores of a sequence and head fit in their buffer");
            let slice = usize::try_from(held).unwrap_or(usize::MAX).min(pairs);
            let (query_groups, value_groups) = (len.div_ceil(64), len.div_ceil(value_queries(c.dim_head)));
            let mut steps = Vec::new();
            for first in (0..pairs).step_by(slice) {
                let count = slice.min(pairs - first);
                let (rows, rows_x) = grid(count * len);
                let uniform = |query_groups: usize, groups_x: usize| {
                    words(&[
                        len,
                        len.div_ceil(4),
                        bands,
                        axis_id(axis),
                        c.qkvg_width() / 4,
                        c.inner_dim() / 4,
                        c.kv_dim() / 4,
                        c.heads,
                        c.heads / c.kv_heads,
                        first,
                        count,
                        query_groups,
                        groups_x,
                    ])
                };
                let flops = 2 * count * len * len * c.dim_head;
                let groups = (len.div_ceil(64) as u32, (query_groups * count) as u32);
                let scored = self.step(products, groups, &[qkvg, scores], &uniform(query_groups, 0));
                steps.push(Step { flops, ..scored });
                steps.push(self.step(softmax, rows, &[scores], &uniform(query_groups, rows_x)));
                let groups = (1, (value_groups * count) as u32);
                let valued = self.step(values, groups, &[qkvg, scores, out], &uniform(value_groups, 0));
                steps.push(Step { flops, ..valued });
            }
            return steps;
        }
        let query_blocks = len.div_ceil(64);
        let blocks = axis.groups(frames, bands) * c.heads * query_blocks;
        let (groups, groups_x) = grid(blocks);
        let uniform = [
            len,
            bands,
            axis_id(axis),
            c.qkvg_width(),
            c.inner_dim(),
            c.heads,
            query_blocks,
            blocks,
            groups_x,
            c.heads / c.kv_heads,
            c.kv_dim(),
            usize::from(c.gates),
        ];
        let pipelines = &self.net.pipelines;
        let buffers: &[&wgpu::Buffer] =
            if pipelines.matrix_attention { &[qkvg, out, qkvg, angles] } else { &[qkvg, out] };
        vec![self.step(&pipelines.attention, groups, buffers, &words(&uniform))]
    }

    fn add(&self, src: &wgpu::Buffer, dst: &wgpu::Buffer, len: usize, accumulate: bool) -> Step {
        let (groups, groups_x) = grid(len.div_ceil(256));
        let uniform = [len, usize::from(accumulate), groups_x];
        self.step(&self.net.pipelines.add, groups, &[src, dst], &words(&uniform))
    }

    fn glu(
        &self,
        y: &wgpu::Buffer,
        mask: &wgpu::Buffer,
        rows: usize,
        width: usize,
        ld: usize,
        off: usize,
    ) -> Step {
        let c = &self.net.geometry.config;
        let gates = if c.mask_glu {
            1
        } else if c.pair_gates {
            2
        } else {
            0
        };
        let (groups, groups_x) = grid((rows * width).div_ceil(256));
        let uniform = [rows, width, ld, off, groups_x, gates, c.group, c.token_frames(rows)];
        self.step(&self.net.pipelines.glu, groups, &[y, mask], &words(&uniform))
    }

    /// `act(gate) * value` in place of the gates, each row `width` gates then `width` values.
    fn gated(&self, hidden: &wgpu::Buffer, rows: usize, width: usize) -> Step {
        let (groups, groups_x) = grid((rows * width).div_ceil(256));
        let silu = usize::from(self.net.geometry.config.silu);
        self.step(&self.net.pipelines.gated, groups, &[hidden], &words(&[rows, width, groups_x, silu]))
    }

    /// The gated feed-forward `ff` of `rows` rows of `a`, through `hidden`, into `out`.
    fn feed_forward(
        &self,
        [ff1, ff2]: &[GpuLinear; 2],
        a: Input,
        rows: usize,
        hidden: &wgpu::Buffer,
        out: Input,
        accumulate: bool,
    ) -> Vec<Step> {
        let into = Input { buffer: hidden, store: Store::Act, ld: ff1.out, off: 0 };
        let mut steps = Vec::new();
        for slice in self.hidden_slices(rows, ff1.out, hidden) {
            let a = Input { off: a.off + slice.start * a.ld, ..a };
            let out = Input { off: out.off + slice.start * out.ld, ..out };
            steps.extend(self.matmul(ff1, a, slice.len(), into, Act::None, false));
            steps.push(self.gated(hidden, slice.len(), ff1.out / 2));
            steps.extend(self.matmul(ff2, into, slice.len(), out, Act::None, accumulate));
        }
        steps
    }

    /// The rows of a feed-forward whose first product gives `width` values a row, in slices
    /// that `hidden` holds.
    fn hidden_slices(
        &self,
        rows: usize,
        width: usize,
        hidden: &wgpu::Buffer,
    ) -> impl Iterator<Item = Range<usize>> {
        let held = (hidden.size() as usize / self.net.act_bytes() / width).max(1);
        (0..rows).step_by(held).map(move |first| first..(first + held).min(rows))
    }

    /// The attention of `pool`'s queries over `sequences` sequences of `len` tokens of `a`, the
    /// first tokens of two `stride` apart, then its feed-forward: a row of `dim` per sequence
    /// and query into `out`, f32.
    fn pool(
        &self,
        pool: &GpuPool,
        a: Input,
        (sequences, len, stride): (usize, usize, usize),
        out: &wgpu::Buffer,
        work: &PoolWork,
    ) -> Vec<Step> {
        let c = &self.net.geometry.config;
        let width = 2 * c.kv_dim();
        let keys = Input { buffer: work.keys, store: Store::Act, ld: width, off: 0 };
        let mut steps = self.matmul(&pool.kv, a, (sequences - 1) * stride + len, keys, Act::None, false);
        let blocks = sequences * pool.count * c.heads;
        let (groups, groups_x) = grid(blocks);
        let uniform = [
            len,
            stride,
            width,
            c.kv_dim(),
            pool.count,
            c.heads,
            c.heads / c.kv_heads,
            c.inner_dim(),
            blocks,
            groups_x,
        ];
        let pipeline = self.net.pipelines.pool.as_ref().expect("compiled for the stem queries");
        steps.push(self.step(pipeline, groups, &[&pool.queries, work.keys, work.attended], &words(&uniform)));
        let rows = sequences * pool.count;
        let attended = Input { buffer: work.attended, store: Store::Act, ld: c.inner_dim(), off: 0 };
        let out_rows = Input { buffer: out, store: Store::F32, ld: c.dim, off: 0 };
        steps.extend(self.matmul(&pool.out, attended, rows, out_rows, Act::None, false));
        steps.push(self.norm(out, work.normed, rows, c.dim, (c.dim, 0), (c.dim, 0)));
        let normed = Input { buffer: work.normed, store: Store::Act, ld: c.dim, off: 0 };
        steps.extend(self.feed_forward(&pool.ff, normed, rows, work.hidden, out_rows, true));
        steps
    }
}

/// The scratch buffers a pooling works in.
struct PoolWork<'a> {
    keys: &'a wgpu::Buffer,
    attended: &'a wgpu::Buffer,
    normed: &'a wgpu::Buffer,
    hidden: &'a wgpu::Buffer,
}

/// Rows of a matrix in a buffer: `ld` values apart, from `off`.
#[derive(Clone, Copy)]
struct Input<'a> {
    buffer: &'a wgpu::Buffer,
    store: Store,
    ld: usize,
    off: usize,
}

fn axis_id(axis: Axis) -> usize {
    match axis {
        Axis::Time => 0,
        Axis::Freq => 1,
    }
}

/// Workgroups for `count` items as `(x, y)` within the per-dimension limit, and the `x` the
/// shaders use to linearise them.
fn grid(count: usize) -> ((u32, u32), usize) {
    let x = count.clamp(1, MAX_GROUPS as usize);
    let y = count.div_ceil(x).max(1);
    ((x as u32, y as u32), x)
}

fn words(values: &[usize]) -> Vec<u32> {
    values.iter().map(|&v| u32::try_from(v).expect("GPU sizes fit in 32 bits")).collect()
}

/// `cos` of every (position, pair) angle, then `sin`: at `positions` positions, the registers'
/// past the `real` ones left as they are.
fn angles(freqs: &[f32], real: usize, positions: usize) -> Vec<f32> {
    let all: Vec<f32> = (0..positions)
        .flat_map(|p| freqs.iter().map(move |f| if p < real { p as f32 * f } else { 0.0 }))
        .collect();
    all.iter().map(|a| a.cos()).chain(all.iter().map(|a| a.sin())).collect()
}

/// A storage buffer holding `bytes`, zero-padded to whole words.
fn storage(device: &wgpu::Device, bytes: &[u8]) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytes,
        usage: wgpu::BufferUsages::STORAGE,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Band, Config, Layer, Transformer, Values};

    /// The heads of the attentions under test: `(dim_head, kv_heads, gates)` of 4 heads. Of 64
    /// values, gated, each with its own keys and values, as the Mel-Band RoFormers'; of 32,
    /// without gates, pairs of them sharing their keys and values, as the Mini-BS-RoFormer-18M's;
    /// of 64 so shared, without gates, as the V2.5's.
    const HEADS: [(usize, usize, bool); 3] = [(64, 4, true), (32, 2, false), (64, 2, false)];

    /// A network of 4 heads shaped as `heads` on `bands` bands (its weights play no part
    /// here), its attention on the matrix units (`matrix`) or on the shader cores, as `attention`
    /// if given; `None` where this machine cannot.
    fn network(
        bands: usize,
        matrix: bool,
        (dim_head, kv_heads, gates): (usize, usize, bool),
        attention: Option<Attention>,
    ) -> Option<GpuNetwork> {
        let config = Config {
            dim: 64,
            heads: 4,
            kv_heads,
            dim_head,
            gates,
            depth: 1,
            n_fft: 2 * bands,
            n_fft_out: 2 * bands,
            hop: 4,
            sample_rate: 44_100,
            chunk_frames: 9,
            skip: false,
            overlap: 1.0,
            norm_eps: 0.0,
            ff_glu: false,
            silu: false,
            mask_glu: true,
            pair_gates: false,
            registers: 0,
            group: 1,
            stem_queries: false,
        };
        let zero = |out: usize, inp: usize| Linear {
            weight: Values::F32(vec![0.0; out * inp]),
            bias: None,
            out,
            inp,
        };
        let layer = || Layer {
            qkvg: zero(config.qkvg_width(), 64),
            out: zero(64, config.inner_dim()),
            ff1: zero(128, 64),
            ff2: zero(64, 128),
        };
        let pairs = dim_head / 2;
        let freqs: Vec<f32> = (0..pairs).map(|i| 10_000f32.powf(-(i as f32) / pairs as f32)).collect();
        let weights = Weights {
            geometry: Geometry {
                config: config.clone(),
                stems: vec!["vocals".into()],
                freq_indices: (0..2 * bands as u32).collect(),
                mask_indices: (0..2 * bands as u32).collect(),
                bands_per_freq: vec![1; bands + 1],
                bands: (0..bands).map(|b| Band { offset: 4 * b, width: 4 }).collect(),
                mask_bands: (0..bands).map(|b| Band { offset: 4 * b, width: 4 }).collect(),
            },
            rotary_time: freqs.clone(),
            rotary_freq: freqs,
            band_proj: (0..bands).map(|_| zero(64, 4)).collect(),
            grouping: None,
            transformers: (0..2)
                .map(|_| Transformer { layers: vec![layer()], norm: Some(vec![1.0; 64]) })
                .collect(),
            masks: vec![(0..bands).map(|_| vec![zero(8, 64)]).collect()],
            final_norm: None,
            registers: Vec::new(),
            stem_queries: None,
            extra: Default::default(),
        };
        let options =
            GpuOptions { software: true, without_matrix_units: !matrix, attention, ..Default::default() };
        let net = match GpuNetwork::with_options(weights, options) {
            Ok(net) => net,
            Err(StemsError::NoGpu(e)) => {
                eprintln!("skipped: {e}");
                return None;
            }
            Err(e) => panic!("{e}"),
        };
        if matrix && !net.pipelines.matrix_attention {
            eprintln!("skipped: {} has no matrix units", net.adapter());
            return None;
        }
        Some(net)
    }

    /// Projections whose keys grow along the sequences: the scores rise block after block.
    fn projections(c: &Config, axis: Axis, frames: usize, bands: usize) -> Vec<f32> {
        let (width, inner, kv) = (c.qkvg_width(), c.inner_dim(), c.kv_dim());
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        (0..frames * bands * width)
            .map(|i| {
                state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
                let r = (state >> 40) as f32 / (1u64 << 24) as f32 - 0.5;
                let (token, at) = (i / width, i % width);
                let growth = 1.0 + axis.position(token, bands) as f32 / 16.0;
                let v = if at < inner {
                    4.0 * r
                } else if at < inner + kv {
                    r * growth
                } else {
                    r
                };
                // as exact in f16 as in f32, for both networks to start from the same values
                half::f16::from_f32(v).to_f32()
            })
            .collect()
    }

    /// Runs `steps` and reads `out` back.
    fn run(net: &GpuNetwork, steps: &[Step], out: &wgpu::Buffer) -> Vec<u8> {
        let read = net.gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: out.size(),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = net.gpu.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            for step in steps {
                pass.set_pipeline(&step.pipeline);
                pass.set_bind_group(0, &step.bind_group, &[]);
                pass.dispatch_workgroups(step.groups.0, step.groups.1, 1);
            }
        }
        encoder.copy_buffer_to_buffer(out, 0, &read, 0, out.size());
        net.gpu.queue.submit([encoder.finish()]);
        read.slice(..).map_async(wgpu::MapMode::Read, |r| r.expect("mapped"));
        net.gpu.device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU done");
        let bytes = read.slice(..).get_mapped_range().expect("mapped").to_vec();
        bytes
    }

    fn relative_error(got: impl Iterator<Item = f32>, expected: &[f32]) -> f64 {
        let num: f64 = got.zip(expected).map(|(a, b)| f64::from(a - b).powi(2)).sum();
        let den: f64 = expected.iter().map(|b| f64::from(*b).powi(2)).sum();
        (num / den).sqrt()
    }

    /// The attention of rotated queries and keys, gated if the network is, in f64.
    fn reference(
        qkvg: &[f32],
        c: &Config,
        freqs: &[f32],
        axis: Axis,
        frames: usize,
        bands: usize,
    ) -> Vec<f32> {
        let (width, inner, kv, dh) = (c.qkvg_width(), c.inner_dim(), c.kv_dim(), c.dim_head);
        let len = axis.len(frames, bands);
        let rotated = |token: usize, at: usize| -> Vec<f64> {
            let pos = axis.position(token, bands) as f64;
            let v = &qkvg[token * width + at..][..dh];
            let mut out = vec![0.0; dh];
            for (p, &f) in freqs.iter().enumerate() {
                let (s, co) = (pos * f64::from(f)).sin_cos();
                let (a, b) = (f64::from(v[2 * p]), f64::from(v[2 * p + 1]));
                out[2 * p] = a * co - b * s;
                out[2 * p + 1] = b * co + a * s;
            }
            out
        };
        let mut out = vec![0.0; frames * bands * inner];
        for group in 0..axis.groups(frames, bands) {
            let token = |i| axis.token(group, i, bands);
            for head in 0..c.heads {
                let shared = head / (c.heads / c.kv_heads) * dh;
                let keys: Vec<Vec<f64>> = (0..len).map(|j| rotated(token(j), inner + shared)).collect();
                for i in 0..len {
                    let q = rotated(token(i), head * dh);
                    let scores: Vec<f64> =
                        keys.iter().map(|k| q.iter().zip(k).map(|(a, b)| a * b).sum()).collect();
                    let max = scores.iter().copied().fold(f64::MIN, f64::max);
                    let weights: Vec<f64> = scores.iter().map(|s| (s - max).exp()).collect();
                    let total: f64 = weights.iter().sum();
                    let gate = if c.gates {
                        1.0 / (1.0 + (-f64::from(qkvg[token(i) * width + inner + 2 * kv + head])).exp())
                    } else {
                        1.0
                    };
                    for d in 0..dh {
                        let v: f64 = (0..len)
                            .map(|j| weights[j] * f64::from(qkvg[token(j) * width + inner + kv + shared + d]))
                            .sum();
                        out[token(i) * inner + head * dh + d] = (v / total * gate) as f32;
                    }
                }
            }
        }
        out
    }

    /// `(axis, frames, bands)` of the sequences under test.
    const SEQUENCES: [(Axis, usize, usize); 2] = [(Axis::Time, 150, 3), (Axis::Freq, 5, 70)];

    /// Along both axes, over sequences that end in partial blocks of queries and keys, with
    /// keys growing along them: the scores rise block after block, and the attention rescales
    /// what it summed again and again.
    #[test]
    fn the_attention_on_the_matrix_units_matches_a_plain_one() {
        for ((axis, frames, bands), heads) in SEQUENCES.into_iter().flat_map(|s| HEADS.map(|h| (s, h))) {
            let Some(net) = network(bands, true, heads, None) else { return };
            let c = &net.geometry.config;
            let (tokens, len) = (frames * bands, axis.len(frames, bands));
            let qkvg = projections(c, axis, frames, bands);
            let freqs = if axis == Axis::Time { &net.params.rotary_time } else { &net.params.rotary_freq };
            let expected = reference(&qkvg, c, freqs, axis, frames, bands);

            let planner = Planner { net: &net, frames };
            let halves: Vec<half::f16> = qkvg.iter().map(|&v| half::f16::from_f32(v)).collect();
            let input = storage(&net.gpu.device, bytemuck::cast_slice(&halves));
            let angles = storage(&net.gpu.device, bytemuck::cast_slice(&angles(freqs, len, len)));
            let out = planner.buffer(Store::Act, tokens * c.inner_dim(), wgpu::BufferUsages::COPY_SRC);
            let scores = planner.buffer(Store::F32, 1, wgpu::BufferUsages::empty());
            let steps = planner.attention(&input, &angles, &out, &scores, axis);
            let bytes = run(&net, &steps, &out);
            let got = &bytemuck::cast_slice::<u8, half::f16>(&bytes)[..tokens * c.inner_dim()];
            let err = relative_error(got.iter().map(|v| v.to_f32()), &expected);
            assert!(err < 2e-3, "{axis:?}, {heads:?}: relative error {err:e}");
        }
    }

    /// The gated heads of 64 values take the products unless told otherwise, the others the
    /// attention in one pass.
    #[test]
    fn the_shader_cores_choose_the_attention_by_the_heads() {
        for heads in HEADS {
            for (attention, products) in [
                (None, heads == HEADS[0]),
                (Some(Attention::Products), true),
                (Some(Attention::OnePass), false),
            ] {
                let Some(net) = network(3, false, heads, attention) else { return };
                assert_eq!(net.pipelines.attention_products.is_some(), products, "{heads:?}, {attention:?}");
            }
        }
    }

    /// Both attentions, over sequences that end in partial blocks of queries and keys; for the
    /// attention as products, several slices of sequences and heads among them where the scores
    /// do not fit at once.
    #[test]
    fn the_attention_on_the_shader_cores_matches_a_plain_one() {
        let cases = SEQUENCES.into_iter().flat_map(|s| HEADS.map(|h| (s, h)));
        let cases = cases.flat_map(|c| [Attention::OnePass, Attention::Products].map(|a| (c, a)));
        for (((axis, frames, bands), heads), attention) in cases {
            let Some(net) = network(bands, false, heads, Some(attention)) else { return };
            let c = &net.geometry.config;
            let (tokens, len) = (frames * bands, axis.len(frames, bands));
            let qkvg = projections(c, axis, frames, bands);
            let freqs = if axis == Axis::Time { &net.params.rotary_time } else { &net.params.rotary_freq };
            let expected = reference(&qkvg, c, freqs, axis, frames, bands);

            let planner = Planner { net: &net, frames };
            let input = storage(&net.gpu.device, bytemuck::cast_slice(&qkvg));
            let angles = storage(&net.gpu.device, bytemuck::cast_slice(&angles(freqs, len, len)));
            let out = planner.buffer(Store::F32, tokens * c.inner_dim(), wgpu::BufferUsages::COPY_SRC);
            // two sequences and heads a slice: the slices take turns in the same scores
            let scores =
                planner.buffer(Store::F32, 2 * len * len.next_multiple_of(4), wgpu::BufferUsages::empty());
            let mut steps = vec![planner.rotary(&input, &angles, axis, tokens)];
            steps.extend(planner.attention(&input, &angles, &out, &scores, axis));
            let bytes = run(&net, &steps, &out);
            let got = &bytemuck::cast_slice::<u8, f32>(&bytes)[..tokens * c.inner_dim()];
            let err = relative_error(got.iter().copied(), &expected);
            assert!(err < 1e-4, "{axis:?}, {heads:?}, {attention:?}: relative error {err:e}");
        }
    }
}
