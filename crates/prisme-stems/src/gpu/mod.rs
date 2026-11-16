//! The network on the GPU, through wgpu (Vulkan, Direct3D 12 or Metal): any recent GPU, no
//! runtime to install.
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
//!
//! A GPU that draws the screen too gets the work in packets of a few milliseconds
//! ([`Pacing`]), the screen's drawing slipping in between: the plan cuts its steps small
//! enough, and the packets hold as many as the GPU's throughput, measured as they go, lets.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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
    /// step then goes alone and is waited for, which makes the whole slower.
    pub profile: bool,
    /// How the work goes to the GPU, else as it suits the GPU: in packets where it draws the
    /// screen too (an integrated GPU, a phone's), whole otherwise.
    pub pacing: Option<Pacing>,
    /// The attention on the shader cores, else the one they run best for the network's heads
    /// (the matrix units have their own).
    pub attention: Option<Attention>,
}

/// How the work of a chunk goes to the GPU.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pacing {
    /// In a submission per transformer, each as long as the GPU takes: for a GPU nothing else
    /// waits on.
    Whole,
    /// In packets of a few milliseconds, the screen's drawing slipping in between, one queued
    /// behind the one running: the GPU never idles while the CPU hears of a packet's end
    /// (pausing would let it lower its clock, and the whole with it).
    Packets,
    /// In packets, each waited for then followed by a pause, for the work to take at most this
    /// share of the GPU's time (from 0.05 to 1).
    Share(f64),
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
    /// registers, and take them in small ones (wgpu tells nothing of a GPU's registers).
    large_tiles: bool,
    /// It draws the screen too, beside the processor (an integrated GPU, a phone's).
    draws_the_screen: bool,
    /// A software renderer, accepted for tests on machines without a GPU.
    software: bool,
    /// The largest buffer a shader binds (bytes).
    max_binding: u64,
    /// Each step is timed ([`GpuOptions::profile`]).
    profile: bool,
    pacing: Pacing,
    /// The most arithmetic a step takes when the work goes in packets: [`PIECE_FLOPS`].
    piece_flops: usize,
    /// The arithmetic it does in a second, as the packets so far took it.
    throughput: Mutex<f64>,
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
    /// Copied to their corner of the stream before each chunk.
    registers: Option<wgpu::Buffer>,
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
    /// `out` rows of `inp` values, f16.
    weight: wgpu::Buffer,
    bias: Option<wgpu::Buffer>,
    out: usize,
    inp: usize,
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
    /// Zeroed by the first submission, each its first bytes: the stream where the registers'
    /// rows and columns cross the frames and the bands, and where the band split leaves the
    /// frames padding the last token.
    cleared: Vec<(wgpu::Buffer, u64)>,
    /// Then copied in place: each row of the registers' square into the stream.
    copies: Vec<Copied>,
    mask: wgpu::Buffer,
    readback: wgpu::Buffer,
    submissions: Vec<Vec<Step>>,
}

/// `bytes` from `from_offset` in `from` to `to_offset` in `to`.
struct Copied {
    from: wgpu::Buffer,
    from_offset: u64,
    to: wgpu::Buffer,
    to_offset: u64,
    bytes: u64,
}

impl Plan {
    /// Writes the bins of a chunk.
    fn load(&self, gpu: &Gpu, bins: &[f32]) {
        gpu.queue.write_buffer(&self.bins, 0, bytemuck::cast_slice(bins));
    }

    /// Every step, in order.
    fn steps(&self) -> impl Iterator<Item = &Step> {
        self.submissions.iter().flatten()
    }

    /// The encoder of a submission of the chunk: the first one clears what has to be and puts
    /// the registers in place.
    fn encoder(&self, gpu: &Gpu, first: bool) -> wgpu::CommandEncoder {
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        if first {
            for (buffer, bytes) in &self.cleared {
                encoder.clear_buffer(buffer, 0, Some(*bytes));
            }
            for c in &self.copies {
                encoder.copy_buffer_to_buffer(&c.from, c.from_offset, &c.to, c.to_offset, c.bytes);
            }
        }
        encoder
    }

    /// Submits `encoder`: the last submission of the chunk copies its masks out to be read.
    fn submit(&self, gpu: &Gpu, mut encoder: wgpu::CommandEncoder, last: bool) -> wgpu::SubmissionIndex {
        if last {
            encoder.copy_buffer_to_buffer(&self.mask, 0, &self.readback, 0, self.mask.size());
        }
        gpu.queue.submit([encoder.finish()])
    }
}

struct Step {
    pipeline: wgpu::ComputePipeline,
    bind_group: wgpu::BindGroup,
    groups: (u32, u32),
    /// What the step does, for [`GpuNetwork::profile`].
    kind: &'static str,
    /// Its arithmetic, where it weighs (products, attention, pooling), for pacing the work.
    flops: usize,
}

impl Step {
    fn record(&self, pass: &mut wgpu::ComputePass<'_>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.dispatch_workgroups(self.groups.0, self.groups.1, 1);
    }
}

/// Records `steps` in one compute pass of `encoder`.
fn record<'a>(encoder: &mut wgpu::CommandEncoder, steps: impl IntoIterator<Item = &'a Step>) {
    let mut pass = encoder.begin_compute_pass(&Default::default());
    steps.into_iter().for_each(|step| step.record(&mut pass));
}

/// `steps` in packets of at least `budget` flops each but the last, with their flops.
fn packets<'a>(steps: impl IntoIterator<Item = &'a Step>, budget: usize) -> Vec<(Vec<&'a Step>, usize)> {
    let mut packets = Vec::new();
    let (mut packet, mut flops) = (Vec::new(), 0);
    for step in steps {
        packet.push(step);
        flops += step.flops;
        if flops >= budget {
            packets.push((std::mem::take(&mut packet), std::mem::take(&mut flops)));
        }
    }
    if !packet.is_empty() {
        packets.push((packet, flops));
    }
    packets
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
        let draws_the_screen = draws_the_screen(&info);
        let pacing = match options.pacing {
            Some(Pacing::Share(share)) => Pacing::Share(share.clamp(0.05, 1.0)),
            Some(pacing) => pacing,
            None if draws_the_screen => Pacing::Packets,
            None => Pacing::Whole,
        };
        Ok(Arc::new(Self {
            device,
            queue,
            name,
            errors,
            matrix_units,
            large_tiles,
            draws_the_screen,
            software: info.device_type == wgpu::DeviceType::Cpu,
            max_binding,
            profile: !profile.is_empty(),
            pacing,
            piece_flops: PIECE_FLOPS,
            // a piece a packet, until the first packets tell more
            throughput: Mutex::new(PIECE_FLOPS as f64 / PACKET.as_secs_f64()),
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

    /// Whether it is a software renderer (see [`GpuOptions::software`]).
    pub fn is_software_renderer(&self) -> bool {
        self.software
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

    /// How many pieces of work of `flops` each a step takes when the work goes in packets: as
    /// many as [`Gpu::piece_flops`] hold, one at least. `None` when it goes whole.
    fn per_step(&self, flops: usize) -> Option<usize> {
        (self.pacing != Pacing::Whole).then(|| (self.piece_flops / flops.max(1)).max(1))
    }

    /// Submits the work of `plan`, its bins loaded, as the GPU is paced.
    fn submit(&self, plan: &Plan) -> Result<wgpu::SubmissionIndex, StemsError> {
        match self.pacing {
            Pacing::Whole => Ok(self.submit_whole(plan)),
            Pacing::Packets => self.submit_paced(plan, None),
            Pacing::Share(share) => self.submit_paced(plan, Some(share)),
        }
    }

    fn submit_whole(&self, plan: &Plan) -> wgpu::SubmissionIndex {
        let count = plan.submissions.len();
        let mut submitted = None;
        for (i, steps) in plan.submissions.iter().enumerate() {
            let mut encoder = plan.encoder(self, i == 0);
            record(&mut encoder, steps);
            submitted = Some(plan.submit(self, encoder, i + 1 == count));
        }
        submitted.expect("a plan has submissions")
    }

    /// Submits the work in packets of about [`PACKET`] at the throughput measured so far, and
    /// measures them: each waited for, then a pause leaving the GPU its `share`; without one,
    /// a packet queued behind the one running.
    fn submit_paced(&self, plan: &Plan, share: Option<f64>) -> Result<wgpu::SubmissionIndex, StemsError> {
        let budget = (*self.throughput.lock().expect("throughput") * PACKET.as_secs_f64()) as usize;
        let packets = packets(plan.steps(), budget.max(1));
        // the packet running, when it began at the latest, and its flops if it is a whole one
        // (the last one's time is mostly the waiting)
        let mut running: Option<(wgpu::SubmissionIndex, Instant, Option<usize>)> = None;
        let mut submitted = None;
        for (i, (steps, flops)) in packets.iter().enumerate() {
            let mut encoder = plan.encoder(self, i == 0);
            record(&mut encoder, steps.iter().copied());
            let submission = plan.submit(self, encoder, i + 1 == packets.len());
            let began = Instant::now();
            let whole = (*flops >= budget).then_some(*flops);
            if let Some(share) = share {
                self.wait(submission.clone())?;
                let took = began.elapsed();
                if let Some(flops) = whole {
                    self.measured(flops, took);
                }
                std::thread::sleep(took.mul_f64((1.0 - share) / share));
            } else if let Some((previous, began, flops)) = running.replace((submission.clone(), began, whole))
            {
                let waiting = Instant::now();
                self.wait(previous)?;
                // one done before it was waited for ended at some unknown moment: not timed
                if let Some(flops) = flops.filter(|_| waiting.elapsed() > SURE_WAIT) {
                    self.measured(flops, began.elapsed());
                }
                // the packet queued starts as the one before ends
                if let Some(next) = running.as_mut() {
                    next.1 = Instant::now();
                }
            }
            submitted = Some(submission);
        }
        Ok(submitted.expect("a plan has steps"))
    }

    /// Takes in that a packet of `flops` took `took`: the throughput moves a quarter of the
    /// way to what it did, taken as at most twice or half what it was (one odd packet, a
    /// thread woken late, does not throw it off).
    fn measured(&self, flops: usize, took: Duration) {
        let mut throughput = self.throughput.lock().expect("throughput");
        let did = (flops as f64 / took.as_secs_f64().max(1e-6)).clamp(*throughput / 2.0, *throughput * 2.0);
        *throughput += (did - *throughput) / 4.0;
    }

    /// Submits the work of `plan` a step at a time, each waited for and timed: `timed` gets
    /// the kind and the seconds of each step. The masks come as from the whole.
    fn submit_profiled(
        &self,
        plan: &Plan,
        mut timed: impl FnMut(&'static str, f64),
    ) -> Result<wgpu::SubmissionIndex, StemsError> {
        let device = &self.device;
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
        let period = f64::from(self.queue.get_timestamp_period()) * 1e-9;
        let failed = |e: &dyn std::fmt::Display| StemsError::Gpu(e.to_string());
        let steps: Vec<&Step> = plan.steps().collect();
        let mut submitted = None;
        for (i, step) in steps.iter().enumerate() {
            let mut encoder = plan.encoder(self, i == 0);
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: None,
                    timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
                        query_set: &set,
                        beginning_of_pass_write_index: Some(0),
                        end_of_pass_write_index: Some(1),
                    }),
                });
                step.record(&mut pass);
            }
            encoder.resolve_query_set(&set, 0..2, &resolved, 0);
            encoder.copy_buffer_to_buffer(&resolved, 0, &read, 0, 16);
            submitted = Some(plan.submit(self, encoder, i + 1 == steps.len()));
            read.slice(..).map_async(wgpu::MapMode::Read, |_| ());
            device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| failed(&e))?;
            let ticks: Vec<u64> =
                bytemuck::cast_slice(&read.slice(..).get_mapped_range().map_err(|e| failed(&e))?).to_vec();
            read.unmap();
            timed(step.kind, ticks[1].saturating_sub(ticks[0]) as f64 * period);
        }
        Ok(submitted.expect("a plan has steps"))
    }

    fn wait(&self, submission: wgpu::SubmissionIndex) -> Result<(), StemsError> {
        let done = wgpu::PollType::Wait { submission_index: Some(submission), timeout: None };
        self.device.poll(done).map(drop).map_err(|e| StemsError::Gpu(e.to_string()))
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
        let mut network = Self {
            gpu: gpu.clone(),
            geometry: weights.geometry.clone(),
            adapter: String::new(),
            matrix,
            pipelines: Pipelines::new(gpu, &weights.geometry, matrix),
            params: Params::upload(&gpu.device, weights, matrix),
            plans: HashMap::new(),
            started: None,
            profiled: HashMap::new(),
        };
        let chunks = network.geometry.config.chunk_frames;
        let fit = network.frames_that_fit();
        if fit < chunks / SHORTEST_CHUNK {
            return Err(StemsError::Gpu(format!(
                "{} binds buffers of {} bytes at most, too few for chunks of {} frames",
                gpu.name,
                gpu.max_binding,
                chunks / SHORTEST_CHUNK
            )));
        }
        network.geometry.config.chunk_frames = fit;
        let c = &network.geometry.config;
        let units = if matrix { ", tensor cores" } else { "" };
        let cut = if fit < chunks {
            format!(", chunks of {:.1} s", c.chunk_samples() as f64 / f64::from(c.sample_rate))
        } else {
            String::new()
        };
        network.adapter = format!("{}{units}{cut}", gpu.name);
        gpu.check_errors()?;
        network.prepare(fit)?;
        Ok(network)
    }

    /// Seconds and steps of each kind of work so far, the longest first: empty unless the
    /// GPU was opened to profile ([`GpuOptions::profile`]) and can.
    pub fn profile(&self) -> Vec<(&'static str, f64, usize)> {
        let mut kinds: Vec<_> = self.profiled.iter().map(|(&kind, &(s, n))| (kind, s, n)).collect();
        kinds.sort_by(|a, b| b.1.total_cmp(&a.1));
        kinds
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

    /// Whether every buffer a chunk of `frames` frames binds fits in one binding.
    fn check_limits(&self, frames: usize) -> Result<(), StemsError> {
        let largest = self.sizes(frames).largest();
        if largest > self.gpu.max_binding {
            return Err(StemsError::Gpu(format!("{largest} byte buffers exceed this GPU's limits")));
        }
        Ok(())
    }

    /// The most frames a chunk can take, in whole tokens, for every buffer it binds to fit in
    /// one binding: at most the model's.
    fn frames_that_fit(&self) -> usize {
        let c = &self.geometry.config;
        let frames = |tokens: usize| (tokens * c.group).min(c.chunk_frames);
        let fits = |tokens: usize| self.sizes(frames(tokens)).largest() <= self.gpu.max_binding;
        // the buffers grow with the tokens: the most that fit lie between `fit` and `over`
        let (mut fit, mut over) = (0, c.token_frames(c.chunk_frames) + 1);
        while over - fit > 1 {
            let tokens = (fit + over) / 2;
            if fits(tokens) {
                fit = tokens;
            } else {
                over = tokens;
            }
        }
        frames(fit)
    }

    /// The bytes of every buffer a chunk of `frames` frames binds, but the small ones (the
    /// weights, the angles of the rotary embeddings, what the stem queries pool).
    fn sizes(&self, frames: usize) -> Sizes {
        let g = &self.geometry;
        let c = &g.config;
        let p = &self.params;
        let token_frames = c.token_frames(frames);
        let (seq_frames, seq_bands) = (c.seq_frames(frames), g.seq_bands());
        let tokens = seq_frames * seq_bands;
        let grouped = token_frames * seq_bands * c.group * p.band_dim;
        // the rows of the mask estimators: every frame of the tokens
        let mask_rows = token_frames * c.group;
        let widest_band = g.bands.iter().map(|b| b.width).max().unwrap_or(0);
        let mask_out = p.masks.iter().flatten().filter_map(|m| m.last()).map(|l| l.out).max().unwrap_or(0);
        let f32s = |n: usize| (n.max(1).next_multiple_of(4) * 4) as u64;
        let acts = |n: usize| (n.max(1).next_multiple_of(4) * self.act_bytes()) as u64;
        let sliced = p.grouping.iter().flat_map(|g| [&g.input[0], &g.output]);
        let sliced = sliced.chain(p.pools.iter().flatten().map(|pool| &pool.ff[0]));
        let row = sliced.map(|l| l.out).max().unwrap_or(0);
        let mut scratch = [0; SLOTS];
        scratch[Slot::X as usize] = f32s(tokens * c.dim);
        scratch[Slot::Earlier as usize] = if c.skip { f32s(tokens * c.dim) } else { 0 };
        scratch[Slot::Normed as usize] = acts((tokens * c.dim).max(grouped));
        scratch[Slot::Qkvg as usize] = acts(tokens * c.qkvg_width());
        scratch[Slot::Attended as usize] = acts(tokens * c.inner_dim());
        scratch[Slot::Hidden as usize] = acts((tokens * p.ff_hidden).max(row));
        scratch[Slot::BandIn as usize] = acts(frames * widest_band);
        scratch[Slot::Mlp0 as usize] = acts(mask_rows * p.mask_hidden);
        scratch[Slot::Mlp1 as usize] = acts(mask_rows * p.mask_hidden);
        scratch[Slot::MlpOut as usize] = if self.closing() { f32s(mask_rows * mask_out) } else { 0 };
        scratch[Slot::Frames as usize] = if p.grouping.is_some() { f32s(grouped) } else { 0 };
        scratch[Slot::Scores as usize] = if self.pipelines.attention_products.is_some() {
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
        Sizes { scratch, bins: f32s(frames * g.features()), mask: f32s(g.mask_len(frames)) }
    }

    /// How many sequences and heads of `len` tokens, of `pairs`, have their scores at once: as
    /// many as [`SCORES_BYTES`] and the GPU's binding hold, and a step takes when the work
    /// goes in packets ([`Gpu::per_step`]), one at least.
    fn scores_slice(&self, len: usize, pairs: usize) -> usize {
        let budget = SCORES_BYTES.min(self.gpu.max_binding);
        let held = usize::try_from(budget / scores_bytes(len)).unwrap_or(usize::MAX);
        let paced = self.gpu.per_step(2 * len * len * self.geometry.config.dim_head).unwrap_or(usize::MAX);
        held.min(paced).clamp(1, pairs.max(1))
    }

    /// Whether the last products of the mask estimators go through the glu step into the
    /// masks: gated, or frames given back out of order.
    fn closing(&self) -> bool {
        let c = &self.geometry.config;
        c.mask_glu || c.pair_gates || self.params.grouping.is_some()
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
        let gpu = &self.gpu;
        let plan = &self.plans[&frames];
        plan.load(gpu, bins);
        let submission = if gpu.profile {
            gpu.submit_profiled(plan, |kind, seconds| {
                let entry = self.profiled.entry(kind).or_default();
                entry.0 += seconds;
                entry.1 += 1;
            })?
        } else {
            gpu.submit(plan)?
        };
        self.started = Some((frames, submission));
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

/// The most capable GPU, through Vulkan when it reaches a hardware one (its compiler makes
/// the matrix products faster than Direct3D 12's, and it reaches the matrix units), else
/// whatever API reaches one, and whether it was picked by name. `WGPU_BACKEND` restricts the
/// APIs, `WGPU_ADAPTER_NAME` picks a GPU by (part of) its name.
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

/// When the work goes in packets ([`Pacing`]): the most arithmetic a step takes, under a
/// millisecond on a phone's GPU (an Adreno 740 does about 350 GFLOP/s), and how long a packet
/// of steps takes.
const PIECE_FLOPS: usize = 1 << 28;
const PACKET: Duration = Duration::from_millis(3);
/// A wait for a packet shorter than this found it done already: when it ended is not known.
const SURE_WAIT: Duration = Duration::from_micros(200);

/// The side of the square of outputs a workgroup of `matmul_blocked.wgsl` computes.
const BLOCKED_TILE: usize = 64;

/// A GPU binding too little for a chunk the length the model was made for takes shorter
/// ones, down to this fraction of it: shorter, a chunk would hear too little of the music.
const SHORTEST_CHUNK: usize = 4;

/// The largest buffer the device lets a shader bind.
fn binding_limit(limits: &wgpu::Limits) -> u64 {
    limits.max_storage_buffer_binding_size.min(limits.max_buffer_size)
}

/// The bytes of the buffers a chunk binds ([`GpuNetwork::sizes`]): the scratch buffers by
/// [`Slot`], its bins and its masks.
struct Sizes {
    scratch: [u64; SLOTS],
    bins: u64,
    mask: u64,
}

impl Sizes {
    fn largest(&self) -> u64 {
        self.scratch.iter().copied().chain([self.bins, self.mask]).max().unwrap_or(0)
    }
}

/// Whether the GPU draws the screen too, beside the processor: an integrated GPU, a phone's.
fn draws_the_screen(info: &wgpu::AdapterInfo) -> bool {
    info.device_type == wgpu::DeviceType::IntegratedGpu || cfg!(any(target_os = "android", target_os = "ios"))
}

/// PCI vendor ids.
const NVIDIA: u32 = 0x10de;
const APPLE: u32 = 0x106b;

const MATRIX_FEATURES: wgpu::Features = wgpu::Features::SHADER_F16
    .union(wgpu::Features::SUBGROUP)
    .union(wgpu::Features::EXPERIMENTAL_COOPERATIVE_MATRIX);

/// Shared memory the attention on the matrix units takes: the workgroup arrays of
/// `attention_coop.wgsl`.
const MATRIX_ATTENTION_SHARED: u32 = 23_056;

/// Whether the shaders written for the matrix units run here: 16 x 16 x 16 products of f16
/// summed in f32, by subgroups of 32 invocations. The shaders count on 32 (8 subgroups to a
/// workgroup of 256, 32 lanes over a tile's rows), and wgpu lets the driver choose the size
/// within the GPU's range: a GPU whose subgroups may be of 64 too (AMD's RDNA) is left out.
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
/// ([`GpuOptions::attention`]), where it takes the heads: on a GPU drawing the screen too,
/// whose shader cores hold fewer registers than a graphics card's (on an Adreno 740, a fifth
/// faster than in one pass for Mini-BS-RoFormer-V2.5); elsewhere, for gated heads of 64 values
/// each with its own keys and values. Smaller heads hold less in their registers, and their
/// scores would weigh more beside their products (on an RTX 4060, the products take 1.5 times
/// as long as `attention.wgsl` for Mini-BS-RoFormer-18M's heads of 32 values).
fn attention_as_products(c: &Config, draws_the_screen: bool) -> bool {
    products_take(c) && (draws_the_screen || c.dim_head == 64 && c.kv_heads == c.heads && c.gates)
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
/// than overrides or parameters, as they size the arrays a thread holds a head in
/// (`attention.wgsl`'s query and output), which only constants can outside workgroup memory.
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
        let products = gpu
            .attention
            .map_or(attention_as_products(c, gpu.draws_the_screen), |a| a == Attention::Products);
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
        // the shader cores read the weights in pairs, the last one padded by `storage`
        let linear = |l: &Linear| GpuLinear {
            weight: storage(device, bytemuck::cast_slice(&l.weight.to_f16())),
            bias: l.bias.as_ref().map(|b| storage(device, bytemuck::cast_slice(b))),
            out: l.out,
            inp: l.inp,
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
            registers: (!w.registers.is_empty()).then(|| {
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: None,
                    contents: bytemuck::cast_slice(&w.registers),
                    usage: wgpu::BufferUsages::COPY_SRC,
                })
            }),
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

/// The buffers the steps of a chunk bind.
struct Work {
    bins: wgpu::Buffer,
    mask: wgpu::Buffer,
    /// The residual stream ([`Slot::X`]).
    x: wgpu::Buffer,
    /// With skip connections, [`Slot::Earlier`].
    earlier: Option<wgpu::Buffer>,
    normed: wgpu::Buffer,
    qkvg: wgpu::Buffer,
    attended: wgpu::Buffer,
    hidden: wgpu::Buffer,
    band_in: wgpu::Buffer,
    mlp: [wgpu::Buffer; 2],
    mlp_out: wgpu::Buffer,
    /// [`Slot::Frames`].
    frames: wgpu::Buffer,
    scores: wgpu::Buffer,
    /// Of the rotary embeddings along the time, then along the bands.
    angles: [wgpu::Buffer; 2],
}

impl Work {
    /// The stream, a token a row of `dim`.
    fn stream(&self, dim: usize) -> Input<'_> {
        Input { buffer: &self.x, store: Store::F32, ld: dim, off: 0 }
    }
}

impl Planner<'_> {
    fn build(self) -> Plan {
        let net = self.net;
        let (c, p) = (&net.geometry.config, &net.params);
        let sizes = net.sizes(self.frames);
        let (generation, scratch) = net.gpu.scratch(sizes.scratch);
        let w = self.work(&sizes, &scratch);

        let mut submissions = vec![self.band_split(&w)];
        for (i, (layers, gain)) in p.transformers.iter().enumerate() {
            submissions.push(self.transformer(i, layers, gain.as_ref(), &w));
        }
        let mut steps = Vec::new();
        if let Some(gain) = &p.final_norm {
            steps.push(self.norm_in_place(&w.x, gain, self.tokens(), c.dim));
        }
        let (pooled, frames_back) = self.stem_queries(&w);
        steps.extend(pooled);
        for (s, back) in frames_back.iter().enumerate() {
            // without stem queries, the frames the tokens give back are the same for every stem
            if let Some(grouping) = p.grouping.as_ref().filter(|_| s == 0 || back.is_some()) {
                steps.extend(self.frames_back(back.as_ref().unwrap_or(&grouping.output), &w));
            }
            steps.extend(self.masks(s, &w));
            submissions.push(std::mem::take(&mut steps));
        }

        let mut cleared = Vec::new();
        if c.registers > 0 {
            cleared.push((w.x.clone(), (self.tokens() * c.dim * 4) as u64));
        }
        if p.grouping.is_some() {
            cleared.push((w.frames.clone(), sizes.scratch[Slot::Frames as usize]));
        }
        let readback = net.gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: w.mask.size(),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Plan {
            generation,
            copies: self.registers(&w.x),
            cleared,
            bins: w.bins,
            mask: w.mask,
            readback,
            submissions,
        }
    }

    fn work(&self, sizes: &Sizes, scratch: &[wgpu::Buffer]) -> Work {
        let g = &self.net.geometry;
        let p = &self.net.params;
        let slot = |s: Slot| scratch[s as usize].clone();
        let (seq_frames, seq_bands) = self.seq();
        let angles = |freqs: &[f32], real: usize, positions: usize| {
            storage(&self.net.gpu.device, bytemuck::cast_slice(&angles(freqs, real, positions)))
        };
        Work {
            bins: self.buffer_of(sizes.bins, wgpu::BufferUsages::COPY_DST),
            mask: self.buffer_of(sizes.mask, wgpu::BufferUsages::COPY_SRC),
            x: slot(Slot::X),
            earlier: g.config.skip.then(|| slot(Slot::Earlier)),
            normed: slot(Slot::Normed),
            qkvg: slot(Slot::Qkvg),
            attended: slot(Slot::Attended),
            hidden: slot(Slot::Hidden),
            band_in: slot(Slot::BandIn),
            mlp: [slot(Slot::Mlp0), slot(Slot::Mlp1)],
            mlp_out: slot(Slot::MlpOut),
            frames: slot(Slot::Frames),
            scores: slot(Slot::Scores),
            angles: [
                angles(&p.rotary_time, g.config.token_frames(self.frames), seq_frames),
                angles(&p.rotary_freq, g.bands.len(), seq_bands),
            ],
        }
    }

    /// The rows of the registers' square, each from the registers into its place in the stream
    /// `x`.
    fn registers(&self, x: &wgpu::Buffer) -> Vec<Copied> {
        let g = &self.net.geometry;
        let (side, dim) = (g.config.registers, g.config.dim);
        let row = (side * dim * 4) as u64;
        let Some(registers) = &self.net.params.registers else { return Vec::new() };
        (0..side)
            .map(|r| Copied {
                from: registers.clone(),
                from_offset: r as u64 * row,
                to: x.clone(),
                to_offset: (g.register_token(self.frames, r * side) * dim * 4) as u64,
                bytes: row,
            })
            .collect()
    }

    /// Each band's frames normed, then projected into the stream; with grouping, into the
    /// frames grouped in tokens, which a feed-forward then takes into the stream.
    fn band_split(&self, w: &Work) -> Vec<Step> {
        let g = &self.net.geometry;
        let c = &g.config;
        let p = &self.net.params;
        let (frames, features) = (self.frames, g.features());
        let (_, seq_bands) = self.seq();
        let mut steps = Vec::new();
        for (b, (band, proj)) in g.bands.iter().zip(&p.band_proj).enumerate() {
            steps.push(self.norm(
                &w.bins,
                &w.band_in,
                frames,
                band.width,
                (features, band.offset),
                (band.width, 0),
            ));
            if p.grouping.is_none() {
                let input = Input { buffer: &w.band_in, store: Store::Act, ld: band.width, off: 0 };
                let into = Input { buffer: &w.x, store: Store::F32, ld: seq_bands * c.dim, off: b * c.dim };
                steps.extend(self.matmul(proj, input, frames, into, Act::None, false));
                continue;
            }
            // frame f of the band to frame f % group of token f / group
            for f in 0..c.group.min(frames) {
                let (ld, off) = (c.group * band.width, f * band.width);
                let input = Input { buffer: &w.band_in, store: Store::Act, ld, off };
                let (ld, off) = (seq_bands * c.group * p.band_dim, (b * c.group + f) * p.band_dim);
                let into = Input { buffer: &w.frames, store: Store::F32, ld, off };
                let rows = (frames - f).div_ceil(c.group);
                steps.extend(self.matmul(proj, input, rows, into, Act::None, false));
            }
        }
        if let Some(grouping) = &p.grouping {
            let frame_tokens = self.frame_tokens();
            let rows = (p.band_dim, 0);
            steps.push(self.norm(&w.frames, &w.normed, frame_tokens * c.group, p.band_dim, rows, rows));
            let input = Input { buffer: &w.normed, store: Store::Act, ld: c.group * p.band_dim, off: 0 };
            steps.extend(self.feed_forward(
                &grouping.input,
                input,
                frame_tokens,
                &w.hidden,
                w.stream(c.dim),
                false,
            ));
        }
        steps
    }

    /// Transformer `i` over the stream, along the time if `i` is even, else along the bands;
    /// `gain` that of its output norm.
    fn transformer(&self, i: usize, layers: &[GpuLayer], gain: Option<&wgpu::Buffer>, w: &Work) -> Vec<Step> {
        let c = &self.net.geometry.config;
        let p = &self.net.params;
        let tokens = self.tokens();
        let axis = if i.is_multiple_of(2) { Axis::Time } else { Axis::Freq };
        let angles = &w.angles[axis_id(axis)];
        let x_rows = w.stream(c.dim);
        let normed_in = Input { buffer: &w.normed, store: Store::Act, ld: c.dim, off: 0 };
        let mut steps = Vec::new();
        if let Some(earlier) = w.earlier.as_ref().filter(|_| axis == Axis::Time && i > 0) {
            steps.push(self.add(earlier, &w.x, tokens * c.dim, true));
        }
        for l in layers {
            steps.push(self.norm(&w.x, &w.normed, tokens, c.dim, (c.dim, 0), (c.dim, 0)));
            let into = Input { buffer: &w.qkvg, store: Store::Act, ld: c.qkvg_width(), off: 0 };
            steps.extend(self.matmul(&l.qkvg, normed_in, tokens, into, Act::None, false));
            if !self.net.pipelines.matrix_attention {
                steps.push(self.rotary(&w.qkvg, angles, axis, tokens));
            }
            steps.extend(self.attention(&w.qkvg, angles, &w.attended, &w.scores, axis));
            let input = Input { buffer: &w.attended, store: Store::Act, ld: c.inner_dim(), off: 0 };
            steps.extend(self.matmul(&l.out, input, tokens, x_rows, Act::None, true));
            steps.push(self.norm(&w.x, &w.normed, tokens, c.dim, (c.dim, 0), (c.dim, 0)));
            let width = if p.gated_products { l.ff1.out / 2 } else { l.ff1.out };
            let into = Input { buffer: &w.hidden, store: Store::Act, ld: width, off: 0 };
            if p.gated_products {
                let act = if c.silu { Act::GatedSilu } else { Act::Gated };
                steps.extend(self.matmul(&l.ff1, normed_in, tokens, into, act, false));
            } else if c.ff_glu {
                steps.extend(self.matmul(&l.ff1, normed_in, tokens, into, Act::None, false));
                steps.push(self.gated(&w.hidden, tokens, l.ff1.out / 2));
            } else {
                steps.extend(self.matmul(&l.ff1, normed_in, tokens, into, Act::Gelu, false));
            }
            steps.extend(self.matmul(&l.ff2, into, tokens, x_rows, Act::None, true));
        }
        if let Some(gain) = gain {
            steps.push(self.norm_in_place(&w.x, gain, tokens, c.dim));
        }
        if let Some(earlier) = w.earlier.as_ref().filter(|_| axis == Axis::Freq) {
            steps.push(self.add(&w.x, earlier, tokens * c.dim, i > 1));
        }
        steps
    }

    /// With stem queries, their pooling of the stream, then for each stem the first product of
    /// the frames given back with the stem's row as its bias; otherwise nothing and none.
    fn stem_queries(&self, w: &Work) -> (Vec<Step>, Vec<Option<GpuLinear>>) {
        let g = &self.net.geometry;
        let c = &g.config;
        let stems = g.stems.len();
        let (Some([bands_pool, tokens_pool]), Some(grouping)) =
            (&self.net.params.pools, &self.net.params.grouping)
        else {
            return (Vec::new(), vec![None; stems]);
        };
        let token_frames = c.token_frames(self.frames);
        let (_, seq_bands) = self.seq();
        let work = PoolWork { keys: &w.qkvg, attended: &w.attended, normed: &w.normed, hidden: &w.hidden };
        let pooled = self.buffer(Store::F32, token_frames * c.dim, wgpu::BufferUsages::empty());
        let stem_rows = self.buffer(Store::F32, stems * c.dim, wgpu::BufferUsages::empty());
        let x_rows = w.stream(c.dim);
        let mut steps =
            self.pool(bands_pool, x_rows, (token_frames, g.bands.len(), seq_bands), &pooled, &work);
        let pooled_rows = Input { buffer: &pooled, store: Store::F32, ld: c.dim, off: 0 };
        steps.extend(self.pool(tokens_pool, pooled_rows, (1, token_frames, 0), &stem_rows, &work));
        let mut frames_back = Vec::with_capacity(stems);
        for s in 0..stems {
            let out = grouping.output.out;
            let bias = self.buffer(Store::F32, out, wgpu::BufferUsages::empty());
            let row = Input { buffer: &stem_rows, store: Store::F32, ld: c.dim, off: s * c.dim };
            let into = Input { buffer: &bias, store: Store::F32, ld: out, off: 0 };
            steps.extend(self.matmul(&grouping.output, row, 1, into, Act::None, false));
            frames_back.push(Some(GpuLinear { bias: Some(bias), ..grouping.output.clone() }));
        }
        (steps, frames_back)
    }

    /// The frames the tokens give back through the grouping's output feed-forward, its first
    /// product `ff1`, normed for the mask estimators.
    fn frames_back(&self, ff1: &GpuLinear, w: &Work) -> Vec<Step> {
        let c = &self.net.geometry.config;
        let p = &self.net.params;
        let grouping = p.grouping.as_ref().expect("the frames come back from tokens grouping them");
        let frame_tokens = self.frame_tokens();
        let into = Input { buffer: &w.hidden, store: Store::Act, ld: ff1.out, off: 0 };
        let mut steps = Vec::new();
        for tokens in self.hidden_slices(frame_tokens, ff1.out, &w.hidden) {
            let n = tokens.len();
            let input = Input { off: tokens.start * c.dim, ..w.stream(c.dim) };
            steps.extend(self.matmul(ff1, input, n, into, Act::None, false));
            steps.push(self.gated(&w.hidden, n, ff1.out / 2));
            // frame f of every token in the f-th block of rows: the estimators read the frames
            // of a band in one stride, the glu step puts them in order
            for (f, l) in grouping.frames.iter().enumerate() {
                let off = (f * frame_tokens + tokens.start) * p.band_dim;
                let out = Input { buffer: &w.frames, store: Store::F32, ld: p.band_dim, off };
                steps.extend(self.matmul(l, into, n, out, Act::None, false));
            }
        }
        let rows = (p.band_dim, 0);
        steps.push(self.norm(&w.frames, &w.normed, frame_tokens * c.group, p.band_dim, rows, rows));
        steps
    }

    /// The masks of stem `s`, each band's from its estimator over the stream, or over the
    /// frames given back.
    fn masks(&self, s: usize, w: &Work) -> Vec<Step> {
        let g = &self.net.geometry;
        let c = &g.config;
        let p = &self.net.params;
        let (frames, mask_features) = (self.frames, g.mask_features());
        let (_, seq_bands) = self.seq();
        let closing = self.net.closing();
        let stem_off = s * frames * mask_features;
        let (rows, width, base) = match &p.grouping {
            None => (frames, c.dim, Input { buffer: &w.x, store: Store::F32, ld: seq_bands * c.dim, off: 0 }),
            Some(_) => {
                let ld = seq_bands * p.band_dim;
                let normed = Input { buffer: &w.normed, store: Store::Act, ld, off: 0 };
                (c.token_frames(frames) * c.group, p.band_dim, normed)
            }
        };
        let estimators = &p.masks[if c.stem_queries { 0 } else { s }];
        let mut steps = Vec::new();
        for (b, (band, mlp_layers)) in g.mask_bands.iter().zip(estimators).enumerate() {
            let mut input = Input { off: b * width, ..base };
            for (k, l) in mlp_layers.iter().enumerate() {
                let last = k + 1 == mlp_layers.len();
                let into = if !last {
                    Input { buffer: &w.mlp[k % 2], store: Store::Act, ld: l.out, off: 0 }
                } else if closing {
                    Input { buffer: &w.mlp_out, store: Store::F32, ld: l.out, off: 0 }
                } else {
                    Input {
                        buffer: &w.mask,
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
                    &w.mlp_out,
                    &w.mask,
                    frames,
                    band.width,
                    mask_features,
                    stem_off + band.offset,
                ));
            }
        }
        steps
    }

    /// The tokens of the transformers' sequences.
    fn tokens(&self) -> usize {
        let (seq_frames, seq_bands) = self.seq();
        seq_frames * seq_bands
    }

    /// The tokens of the frames, the registers' columns among them.
    fn frame_tokens(&self) -> usize {
        self.net.geometry.config.token_frames(self.frames) * self.net.geometry.seq_bands()
    }

    /// Frames and bands of the transformers' sequences.
    fn seq(&self) -> (usize, usize) {
        (self.net.geometry.config.seq_frames(self.frames), self.net.geometry.seq_bands())
    }

    /// Room for `count` values stored as `store`, in whole groups of 4.
    fn buffer(&self, store: Store, count: usize, usage: wgpu::BufferUsages) -> wgpu::Buffer {
        let bytes = if store == Store::Act { self.net.act_bytes() } else { 4 };
        self.buffer_of((count.max(1).next_multiple_of(4) * bytes) as u64, usage)
    }

    fn buffer_of(&self, bytes: u64, usage: wgpu::BufferUsages) -> wgpu::Buffer {
        self.net.gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: bytes,
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

    /// `out` (+)= act(`a` · `l`) over `rows` rows: in pieces of about [`Gpu::piece_flops`]
    /// when the work goes in packets, else at once.
    fn matmul(
        &self,
        l: &GpuLinear,
        a: Input,
        rows: usize,
        out: Input,
        act: Act,
        accumulate: bool,
    ) -> Vec<Step> {
        let tile = BLOCKED_TILE.max(self.net.pipelines.matmul_tile);
        let piece = self.net.gpu.per_step(2 * l.out * l.inp).map_or(rows, |rows| rows.next_multiple_of(tile));
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
        let aligned = l.inp.is_multiple_of(4) && a.ld.is_multiple_of(4) && a.off.is_multiple_of(4);
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

    fn attention(
        &self,
        qkvg: &wgpu::Buffer,
        angles: &wgpu::Buffer,
        out: &wgpu::Buffer,
        scores: &wgpu::Buffer,
        axis: Axis,
    ) -> Vec<Step> {
        let c = &self.net.geometry.config;
        let gpu = &self.net.gpu;
        let (frames, bands) = self.seq();
        let len = axis.len(frames, bands);
        // the arithmetic of the scores of a sequence and head, and of its values
        let flops = 2 * len * len * c.dim_head;
        if let Some([products, softmax, values]) = &self.net.pipelines.attention_products {
            let pairs = axis.groups(frames, bands) * c.heads;
            let held = scores.size().min(gpu.max_binding) / scores_bytes(len);
            assert!(held > 0, "the scores of a sequence and head fit in their buffer");
            let (query_groups, value_groups) = (len.div_ceil(64), len.div_ceil(value_queries(c.dim_head)));
            // as many as the scores buffer holds and the GPU binds, a step takes when the work
            // goes in packets, and a dispatch has workgroups along its second dimension
            let slice = usize::try_from(held)
                .unwrap_or(usize::MAX)
                .min(gpu.per_step(flops).unwrap_or(usize::MAX))
                .min(MAX_GROUPS as usize / query_groups.max(value_groups))
                .min(pairs);
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
                let flops = count * flops;
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
        // the blocks of queries of a sequence's heads follow each other: a step takes whole
        // sequences, all of them unless the work goes in packets
        let sequences = axis.groups(frames, bands);
        let query_blocks = len.div_ceil(64);
        let blocks = c.heads * query_blocks;
        let slice = gpu.per_step(2 * flops * c.heads).unwrap_or(sequences);
        let pipelines = &self.net.pipelines;
        let buffers: &[&wgpu::Buffer] =
            if pipelines.matrix_attention { &[qkvg, out, qkvg, angles] } else { &[qkvg, out] };
        (0..sequences)
            .step_by(slice)
            .map(|first| {
                let count = slice.min(sequences - first);
                let (groups, groups_x) = grid(count * blocks);
                let uniform = [
                    len,
                    bands,
                    axis_id(axis),
                    c.qkvg_width(),
                    c.inner_dim(),
                    c.heads,
                    query_blocks,
                    (first + count) * blocks,
                    groups_x,
                    c.heads / c.kv_heads,
                    c.kv_dim(),
                    first * blocks,
                ];
                let step = self.step(&pipelines.attention, groups, buffers, &words(&uniform));
                Step { flops: 2 * count * c.heads * flops, ..step }
            })
            .collect()
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
        // the blocks of a sequence's queries and heads follow each other: a step takes whole
        // sequences, all of them unless the work goes in packets
        let blocks = pool.count * c.heads;
        let flops = 4 * blocks * len * c.dim_head;
        let slice = self.net.gpu.per_step(flops).unwrap_or(sequences);
        let pipeline = self.net.pipelines.pool.as_ref().expect("compiled for the stem queries");
        for first in (0..sequences).step_by(slice) {
            let count = slice.min(sequences - first);
            let (groups, groups_x) = grid(count * blocks);
            let uniform = [
                len,
                stride,
                width,
                c.kv_dim(),
                pool.count,
                c.heads,
                c.heads / c.kv_heads,
                c.inner_dim(),
                (first + count) * blocks,
                groups_x,
                first * blocks,
            ];
            let buffers = [&pool.queries, work.keys, work.attended];
            let step = self.step(pipeline, groups, &buffers, &words(&uniform));
            steps.push(Step { flops: count * flops, ..step });
        }
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
        record(&mut encoder, steps);
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
    /// do not fit at once. On a software renderer, the products alone: Windows' (WARP) takes
    /// minutes to compile `attention.wgsl` for each shape of heads (6 on 4 cores), and the
    /// separation tests run it there (`tests/reference.rs`).
    #[test]
    fn the_attention_on_the_shader_cores_matches_a_plain_one() {
        let cases = SEQUENCES.into_iter().flat_map(|s| HEADS.map(|h| (s, h)));
        let cases = cases.flat_map(|c| [Attention::OnePass, Attention::Products].map(|a| (c, a)));
        for (((axis, frames, bands), heads), attention) in cases {
            let Some(net) = network(bands, false, heads, Some(attention)) else { return };
            if net.gpu.software && attention == Attention::OnePass {
                eprintln!("skipped: {heads:?} in one pass, on a software renderer");
                continue;
            }
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

    /// The bytes of the workgroup arrays of a shader, each `var<workgroup> name: array<T, N>;`.
    fn workgroup_bytes(source: &str) -> u32 {
        source
            .lines()
            .filter_map(|line| line.trim().strip_prefix("var<workgroup>"))
            .map(|declared| {
                let array = declared.split_once("array<").expect("an array").1;
                let (element, count) = array.trim_end_matches(">;").split_once(", ").expect("sized");
                let bytes = match element {
                    "f16" => 2,
                    "f32" | "u32" => 4,
                    other => panic!("an element of {other}"),
                };
                bytes * count.parse::<u32>().expect("a literal count")
            })
            .sum()
    }

    #[test]
    fn the_shared_memory_of_the_attention_on_the_matrix_units_is_its_arrays() {
        assert_eq!(workgroup_bytes(include_str!("attention_coop.wgsl")), MATRIX_ATTENTION_SHARED);
    }

    fn fixture(name: &str) -> Weights {
        Weights::load(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data").join(name))
            .expect("fixture loads")
    }

    /// Cut in steps of a few thousand flops, in packets of a few of them, the work of the small
    /// networks of the fixtures: every product in pieces of rows, the attentions and the
    /// pooling sequence by sequence, they give the masks they give whole. On a software renderer,
    /// the attention as products alone (see
    /// [`the_attention_on_the_shader_cores_matches_a_plain_one`]).
    #[test]
    fn the_work_in_packets_gives_what_it_gives_whole() {
        let names = [
            "tiny_roformer.safetensors",
            "tiny_roformer_stems.safetensors",
            "tiny_bs_roformer.safetensors",
            "tiny_bs_roformer_v25.safetensors",
        ];
        let ways = [(true, Some(Attention::OnePass)), (true, Some(Attention::Products)), (false, None)];
        let software =
            Gpu::open(GpuOptions { software: true, ..Default::default() }).is_ok_and(|gpu| gpu.software);
        let ways = if software {
            eprintln!("skipped: the attention in one pass, on a software renderer");
            &ways[1..2]
        } else {
            &ways[..]
        };
        for (name, &(without_matrix_units, attention)) in
            names.into_iter().flat_map(|n| ways.iter().map(move |w| (n, w)))
        {
            let weights = fixture(name);
            if attention == Some(Attention::Products) && !products_take(&weights.geometry.config) {
                continue;
            }
            let frames = 150;
            let bins: Vec<f32> = (0..frames * weights.geometry.features())
                .map(|i| ((i * 7919 % 2003) as f32 / 1001.5 - 1.0) * 3.0)
                .collect();
            let masks = |pacing: Pacing, piece_flops: usize| {
                let options = GpuOptions {
                    software: true,
                    without_matrix_units,
                    pacing: Some(pacing),
                    attention,
                    ..Default::default()
                };
                let mut gpu = match Gpu::open(options) {
                    Ok(gpu) => gpu,
                    Err(StemsError::NoGpu(e)) => {
                        eprintln!("skipped: {e}");
                        return None;
                    }
                    Err(e) => panic!("{e}"),
                };
                let alone = Arc::get_mut(&mut gpu).expect("alone");
                alone.piece_flops = piece_flops;
                *alone.throughput.get_mut().expect("throughput") = piece_flops as f64 / PACKET.as_secs_f64();
                let mut net = GpuNetwork::on(&gpu, &weights).unwrap();
                let mut out = vec![0.0; weights.geometry.mask_len(frames)];
                net.masks(&bins, frames, &mut out).unwrap();
                Some((out, net.plans[&frames].steps().count()))
            };
            let Some((whole, whole_steps)) = masks(Pacing::Whole, PIECE_FLOPS) else { return };
            let (paced, paced_steps) = masks(Pacing::Packets, 1 << 16).expect("the same GPU");
            assert!(paced_steps > whole_steps, "{name}: {paced_steps} steps from {whole_steps}");
            assert_eq!(paced, whole, "{name}, {attention:?}");
        }
    }

    /// A packet closes on the step reaching its budget; the last one takes what is left.
    #[test]
    fn packets_hold_their_budget_of_work() {
        let Some(net) = network(3, false, HEADS[1], None) else { return };
        let plan = &net.plans[&net.geometry.config.chunk_frames];
        let step = plan.steps().next().expect("a step");
        let steps: Vec<Step> = [5, 0, 3, 4, 0, 0, 9, 1]
            .map(|flops| Step {
                pipeline: step.pipeline.clone(),
                bind_group: step.bind_group.clone(),
                flops,
                ..*step
            })
            .into();
        let flops: Vec<(usize, usize)> = packets(&steps, 6).iter().map(|(p, f)| (p.len(), *f)).collect();
        assert_eq!(flops, [(3, 8), (4, 13), (1, 1)]);
    }
}
