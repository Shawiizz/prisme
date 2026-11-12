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
use std::sync::{Arc, Mutex};

use wgpu::util::DeviceExt;

use crate::model::{Geometry, Linear, Weights};
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
}

const SLOTS: usize = Slot::MlpOut as usize + 1;

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
}

/// The weights, on the GPU.
struct Params {
    rotary_time: Vec<f32>,
    rotary_freq: Vec<f32>,
    band_proj: Vec<GpuLinear>,
    transformers: Vec<(Vec<GpuLayer>, wgpu::Buffer)>,
    /// Per stem, per band.
    masks: Vec<Vec<Vec<GpuLinear>>>,
    no_bias: wgpu::Buffer,
    ff_hidden: usize,
    mask_hidden: usize,
}

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
    norm: wgpu::ComputePipeline,
    norm_in_place: wgpu::ComputePipeline,
    rotary: wgpu::ComputePipeline,
    attention: wgpu::ComputePipeline,
    /// The attention runs on the matrix units, rotating the queries and keys itself.
    matrix_attention: bool,
    glu: wgpu::ComputePipeline,
    add: wgpu::ComputePipeline,
}

/// The dispatches for one frame count, in submissions short enough not to trip the
/// operating system's GPU watchdog on a slow GPU.
struct Plan {
    /// Of the scratch buffers it binds.
    generation: u64,
    bins: wgpu::Buffer,
    mask: wgpu::Buffer,
    readback: wgpu::Buffer,
    submissions: Vec<Vec<Step>>,
}

struct Step {
    pipeline: wgpu::ComputePipeline,
    bind_group: wgpu::BindGroup,
    groups: (u32, u32),
}

#[derive(Clone, Copy)]
enum Act {
    None = 0,
    Gelu = 1,
    Tanh = 2,
}

impl Gpu {
    /// The most capable GPU, with its matrix units if it has some; fails without a GPU
    /// worth it. Software rasterisers are refused unless `options` accept them, and so are
    /// integrated GPUs without matrix units unless picked by name (`WGPU_ADAPTER_NAME`): the
    /// CPU separates faster (on an Intel UHD of 2023, 26 times faster).
    pub fn open(options: GpuOptions) -> Result<Arc<Self>, StemsError> {
        let (adapter, matrix_units) = worthy_adapter(options)?;
        let info = adapter.get_info();
        let large_tiles = info.vendor == NVIDIA;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("prisme-stems"),
            required_features: if matrix_units { MATRIX_FEATURES } else { wgpu::Features::empty() },
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
            usage: wgpu::BufferUsages::STORAGE,
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
        let pipelines = Pipelines::new(gpu, &weights.geometry, matrix);
        let params = Params::upload(&gpu.device, weights, matrix);
        let units = if matrix { ", tensor cores" } else { "" };
        let mut network = Self {
            gpu: gpu.clone(),
            geometry: weights.geometry.clone(),
            adapter: format!("{}{units}", gpu.name),
            matrix,
            pipelines,
            params,
            plans: HashMap::new(),
            started: None,
        };
        gpu.check_errors()?;
        network.prepare(network.geometry.config.chunk_frames)?;
        Ok(network)
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
        let c = &self.geometry.config;
        let tokens = frames * self.geometry.bands.len();
        let widest = c.qkvg_width().max(self.params.ff_hidden);
        let bytes = (tokens * widest * self.act_bytes()).max(tokens * c.dim * 4) as u64;
        let limits = self.gpu.device.limits();
        if bytes > limits.max_storage_buffer_binding_size || bytes > limits.max_buffer_size {
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
        let plan = &self.plans[&frames];
        let gpu = &self.gpu;
        gpu.queue.write_buffer(&plan.bins, 0, bytemuck::cast_slice(bins));
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
    if info.device_type == wgpu::DeviceType::IntegratedGpu && !matrix_units && !picked {
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

/// NVIDIA's PCI vendor id.
const NVIDIA: u32 = 0x10de;

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

/// Whether the attention runs on the matrix units: heads of 64 values, and the queries, keys
/// and values of a token read 4 values at a time.
fn matrix_attention(device: &wgpu::Device, geometry: &Geometry) -> bool {
    let c = &geometry.config;
    c.dim_head == 64
        && c.heads.is_multiple_of(4)
        && device.limits().max_compute_workgroup_storage_size >= MATRIX_ATTENTION_SHARED
}

impl Pipelines {
    fn new(gpu: &Gpu, geometry: &Geometry, matrix: bool) -> Self {
        let device = &gpu.device;
        let header = if matrix {
            "enable f16;\nenable wgpu_cooperative_matrix;\nalias Act = f16;\n"
        } else {
            "alias Act = f32;\n"
        };
        let make = |source: &str, entry: &str, constants: &[(&str, f64)]| {
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
                compilation_options: wgpu::PipelineCompilationOptions { constants, ..Default::default() },
                cache: None,
            })
        };
        let matmul = if matrix {
            [("f32", "f32"), ("f32", "f16"), ("f16", "f32"), ("f16", "f16")].map(|(input, output)| {
                let types = format!("alias In = {input};\nalias Out = {output};\n");
                make(&format!("{types}{}", include_str!("matmul_coop.wgsl")), "main", &[])
            })
        } else {
            let source =
                if gpu.large_tiles { include_str!("matmul.wgsl") } else { include_str!("matmul_small.wgsl") };
            let f32 = make(source, "main", &[]);
            [f32.clone(), f32.clone(), f32.clone(), f32]
        };
        let matmul_tile = if matrix || gpu.large_tiles { 128 } else { 64 };
        let matrix_attention = matrix && matrix_attention(device, geometry);
        let attention = if matrix_attention {
            make(include_str!("attention_coop.wgsl"), "main", &[])
        } else {
            make(include_str!("attention.wgsl"), "main", &[("DH", geometry.config.dim_head as f64)])
        };
        Self {
            matmul,
            matmul_tile,
            norm: make(include_str!("norm.wgsl"), "normalize", &[]),
            norm_in_place: make(include_str!("norm_in_place.wgsl"), "normalize_in_place", &[]),
            rotary: make(include_str!("rotary.wgsl"), "main", &[]),
            attention,
            matrix_attention,
            glu: make(include_str!("glu.wgsl"), "main", &[]),
            add: make(include_str!("add.wgsl"), "main", &[]),
        }
    }

    fn matmul(&self, input: Store, output: Store) -> &wgpu::ComputePipeline {
        &self.matmul[2 * usize::from(input == Store::Act) + usize::from(output == Store::Act)]
    }
}

impl Params {
    fn upload(device: &wgpu::Device, w: &Weights, matrix: bool) -> Self {
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
                        ff1: linear(&l.ff1),
                        ff2: linear(&l.ff2),
                    })
                    .collect();
                (layers, storage(device, bytemuck::cast_slice(&tr.norm)))
            })
            .collect();
        let ff_hidden = w.transformers.iter().flat_map(|t| &t.layers).map(|l| l.ff1.out).max().unwrap_or(0);
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
            transformers,
            masks: w
                .masks
                .iter()
                .map(|stem| stem.iter().map(|m| m.iter().map(linear).collect()).collect())
                .collect(),
            no_bias: storage(device, bytemuck::cast_slice(&[0.0f32; 4])),
            ff_hidden,
            mask_hidden,
        }
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
        let bands = g.bands.len();
        let tokens = frames * bands;
        let features = g.features();
        let widest_band = g.bands.iter().map(|b| b.width).max().unwrap_or(0);
        let mask_out = p.masks.iter().flatten().filter_map(|m| m.last()).map(|l| l.out).max().unwrap_or(0);
        let f32s = |n: usize| (n.max(1).next_multiple_of(4) * 4) as u64;
        let acts = |n: usize| (n.max(1).next_multiple_of(4) * self.net.act_bytes()) as u64;
        let mut bytes = [0; SLOTS];
        bytes[Slot::X as usize] = f32s(tokens * c.dim);
        bytes[Slot::Earlier as usize] = if c.skip { f32s(tokens * c.dim) } else { 0 };
        bytes[Slot::Normed as usize] = acts(tokens * c.dim);
        bytes[Slot::Qkvg as usize] = acts(tokens * c.qkvg_width());
        bytes[Slot::Attended as usize] = acts(tokens * c.inner_dim());
        bytes[Slot::Hidden as usize] = acts(tokens * p.ff_hidden);
        bytes[Slot::BandIn as usize] = acts(frames * widest_band);
        bytes[Slot::Mlp0 as usize] = acts(frames * p.mask_hidden);
        bytes[Slot::Mlp1 as usize] = acts(frames * p.mask_hidden);
        bytes[Slot::MlpOut as usize] = f32s(frames * mask_out);
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
        let angles_time =
            storage(&self.net.gpu.device, bytemuck::cast_slice(&angles(&p.rotary_time, frames)));
        let angles_freq = storage(&self.net.gpu.device, bytemuck::cast_slice(&angles(&p.rotary_freq, bands)));

        let mut submissions = Vec::new();
        let mut steps = Vec::new();
        for (b, (band, proj)) in g.bands.iter().zip(&p.band_proj).enumerate() {
            steps.push(self.norm(
                &bins,
                &band_in,
                frames,
                band.width,
                (features, band.offset),
                (band.width, 0),
            ));
            let input = Input { buffer: &band_in, store: Store::Act, ld: band.width, off: 0 };
            let into = Input { buffer: &x, store: Store::F32, ld: bands * c.dim, off: b * c.dim };
            steps.push(self.matmul(proj, input, frames, into, Act::None, false));
        }
        submissions.push(std::mem::take(&mut steps));

        let normed_in = Input { buffer: &normed, store: Store::Act, ld: c.dim, off: 0 };
        let x_rows = Input { buffer: &x, store: Store::F32, ld: c.dim, off: 0 };
        for (i, (layers, gain)) in p.transformers.iter().enumerate() {
            let axis = if i % 2 == 0 { Axis::Time } else { Axis::Freq };
            let angles = if axis == Axis::Time { &angles_time } else { &angles_freq };
            if let Some(earlier) = earlier.as_ref().filter(|_| axis == Axis::Time && i > 0) {
                steps.push(self.add(earlier, &x, tokens * c.dim, true));
            }
            for l in layers {
                steps.push(self.norm(&x, &normed, tokens, c.dim, (c.dim, 0), (c.dim, 0)));
                let into = Input { buffer: &qkvg, store: Store::Act, ld: c.qkvg_width(), off: 0 };
                steps.push(self.matmul(&l.qkvg, normed_in, tokens, into, Act::None, false));
                if !self.net.pipelines.matrix_attention {
                    steps.push(self.rotary(&qkvg, angles, axis, tokens));
                }
                steps.push(self.attention(&qkvg, angles, &attended, axis));
                let input = Input { buffer: &attended, store: Store::Act, ld: c.inner_dim(), off: 0 };
                steps.push(self.matmul(&l.out, input, tokens, x_rows, Act::None, true));
                steps.push(self.norm(&x, &normed, tokens, c.dim, (c.dim, 0), (c.dim, 0)));
                let into = Input { buffer: &hidden, store: Store::Act, ld: l.ff1.out, off: 0 };
                steps.push(self.matmul(&l.ff1, normed_in, tokens, into, Act::Gelu, false));
                steps.push(self.matmul(&l.ff2, into, tokens, x_rows, Act::None, true));
            }
            steps.push(self.norm_in_place(&x, gain, tokens, c.dim));
            if let Some(earlier) = earlier.as_ref().filter(|_| axis == Axis::Freq) {
                steps.push(self.add(&x, earlier, tokens * c.dim, i > 1));
            }
            submissions.push(std::mem::take(&mut steps));
        }

        for (s, stem) in p.masks.iter().enumerate() {
            let stem_off = s * frames * features;
            for (b, (band, mlp_layers)) in g.bands.iter().zip(stem).enumerate() {
                let mut input = Input { buffer: &x, store: Store::F32, ld: bands * c.dim, off: b * c.dim };
                for (k, l) in mlp_layers.iter().enumerate() {
                    let last = k + 1 == mlp_layers.len();
                    let (buffer, store) =
                        if last { (&mlp_out, Store::F32) } else { (&mlp[k % 2], Store::Act) };
                    let into = Input { buffer, store, ld: l.out, off: 0 };
                    let act = if last { Act::None } else { Act::Tanh };
                    steps.push(self.matmul(l, input, frames, into, act, false));
                    input = into;
                }
                steps.push(self.glu(&mlp_out, &mask, frames, band.width, features, stem_off + band.offset));
            }
            submissions.push(std::mem::take(&mut steps));
        }

        Plan { generation, bins, mask, readback, submissions }
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
        Step { pipeline: pipeline.clone(), bind_group, groups }
    }

    /// `out` (+)= act(`a` · `l`) over `rows` rows.
    fn matmul(&self, l: &GpuLinear, a: Input, rows: usize, out: Input, act: Act, accumulate: bool) -> Step {
        let bias = l.bias.as_ref().unwrap_or(&self.net.params.no_bias);
        let tile = self.net.pipelines.matmul_tile;
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
        let pipeline = self.net.pipelines.matmul(a.store, out.store);
        self.step(pipeline, groups, &[a.buffer, &l.weight, bias, out.buffer], &words(&uniform))
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
        let uniform = [rows, width, src_ld, src_off, dst_ld, dst_off, 0, groups_x];
        let buffers = [src, dst, &self.net.params.no_bias];
        self.step(&self.net.pipelines.norm, groups, &buffers, &words(&uniform))
    }

    fn norm_in_place(&self, x: &wgpu::Buffer, gain: &wgpu::Buffer, rows: usize, width: usize) -> Step {
        let (groups, groups_x) = grid(rows);
        let uniform = [rows, width, width, 0, width, 0, 1, groups_x];
        self.step(&self.net.pipelines.norm_in_place, groups, &[x, gain], &words(&uniform))
    }

    fn rotary(&self, qkvg: &wgpu::Buffer, angles: &wgpu::Buffer, axis: Axis, tokens: usize) -> Step {
        let g = &self.net.geometry;
        let c = &g.config;
        let bands = g.bands.len();
        let pairs = c.dim_head / 2;
        let (groups, groups_x) = grid((tokens * 2 * c.heads * pairs).div_ceil(256));
        let positions = axis.len(self.frames, bands);
        let uniform = [tokens, 2 * c.heads, pairs, c.qkvg_width(), bands, axis_id(axis), positions, groups_x];
        self.step(&self.net.pipelines.rotary, groups, &[qkvg, angles], &words(&uniform))
    }

    fn attention(&self, qkvg: &wgpu::Buffer, angles: &wgpu::Buffer, out: &wgpu::Buffer, axis: Axis) -> Step {
        let g = &self.net.geometry;
        let c = &g.config;
        let bands = g.bands.len();
        let len = axis.len(self.frames, bands);
        let query_blocks = len.div_ceil(64);
        let blocks = axis.groups(self.frames, bands) * c.heads * query_blocks;
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
        ];
        let pipelines = &self.net.pipelines;
        let buffers: &[&wgpu::Buffer] =
            if pipelines.matrix_attention { &[qkvg, out, qkvg, angles] } else { &[qkvg, out] };
        self.step(&pipelines.attention, groups, buffers, &words(&uniform))
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
        let (groups, groups_x) = grid((rows * width).div_ceil(256));
        self.step(&self.net.pipelines.glu, groups, &[y, mask], &words(&[rows, width, ld, off, groups_x]))
    }
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

/// `cos` of every (position, pair) angle, then `sin`.
fn angles(freqs: &[f32], positions: usize) -> Vec<f32> {
    let all: Vec<f32> = (0..positions).flat_map(|p| freqs.iter().map(move |f| p as f32 * f)).collect();
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

    /// A network of 4 heads of 64 values on `bands` bands, as the attention on the matrix
    /// units takes them (its weights play no part here); `None` without such units.
    fn matrix_network(bands: usize) -> Option<GpuNetwork> {
        let config = Config {
            dim: 64,
            heads: 4,
            dim_head: 64,
            depth: 1,
            n_fft: 2 * bands,
            hop: 4,
            sample_rate: 44_100,
            chunk_frames: 9,
            skip: false,
            overlap: 1,
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
        let freqs: Vec<f32> = (0..32).map(|i| 10_000f32.powf(-(i as f32) / 32.0)).collect();
        let weights = Weights {
            geometry: Geometry {
                config: config.clone(),
                stems: vec!["vocals".into()],
                freq_indices: (0..2 * bands as u32).collect(),
                bands_per_freq: vec![1; bands + 1],
                bands: (0..bands).map(|b| Band { offset: 4 * b, width: 4 }).collect(),
            },
            rotary_time: freqs.clone(),
            rotary_freq: freqs,
            band_proj: (0..bands).map(|_| zero(64, 4)).collect(),
            transformers: (0..2)
                .map(|_| Transformer { layers: vec![layer()], norm: vec![1.0; 64] })
                .collect(),
            masks: vec![(0..bands).map(|_| vec![zero(8, 64)]).collect()],
            extra: Default::default(),
        };
        let net = match GpuNetwork::with_options(weights, GpuOptions { software: true, ..Default::default() })
        {
            Ok(net) => net,
            Err(StemsError::NoGpu(e)) => {
                eprintln!("skipped: {e}");
                return None;
            }
            Err(e) => panic!("{e}"),
        };
        if !net.pipelines.matrix_attention {
            eprintln!("skipped: {} has no matrix units", net.adapter());
            return None;
        }
        Some(net)
    }

    /// The gated attention of rotated queries and keys, in f64.
    fn reference(
        qkvg: &[f32],
        c: &Config,
        freqs: &[f32],
        axis: Axis,
        frames: usize,
        bands: usize,
    ) -> Vec<f32> {
        let (width, inner, dh) = (c.qkvg_width(), c.inner_dim(), c.dim_head);
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
                let keys: Vec<Vec<f64>> = (0..len).map(|j| rotated(token(j), inner + head * dh)).collect();
                for i in 0..len {
                    let q = rotated(token(i), head * dh);
                    let scores: Vec<f64> =
                        keys.iter().map(|k| q.iter().zip(k).map(|(a, b)| a * b).sum()).collect();
                    let max = scores.iter().copied().fold(f64::MIN, f64::max);
                    let weights: Vec<f64> = scores.iter().map(|s| (s - max).exp()).collect();
                    let total: f64 = weights.iter().sum();
                    let gate = 1.0 / (1.0 + (-f64::from(qkvg[token(i) * width + 3 * inner + head])).exp());
                    for d in 0..dh {
                        let v: f64 = (0..len)
                            .map(|j| {
                                weights[j] * f64::from(qkvg[token(j) * width + 2 * inner + head * dh + d])
                            })
                            .sum();
                        out[token(i) * inner + head * dh + d] = (v / total * gate) as f32;
                    }
                }
            }
        }
        out
    }

    /// Along both axes, over sequences that end in partial blocks of queries and keys, with
    /// keys growing along them: the scores rise block after block, and the attention rescales
    /// what it summed again and again.
    #[test]
    fn the_attention_on_the_matrix_units_matches_a_plain_one() {
        for (axis, frames, bands) in [(Axis::Time, 150, 3), (Axis::Freq, 5, 70)] {
            let Some(net) = matrix_network(bands) else { return };
            let c = &net.geometry.config;
            let (width, inner) = (c.qkvg_width(), c.inner_dim());
            let tokens = frames * bands;
            let mut state = 0x9e37_79b9_7f4a_7c15u64;
            let qkvg: Vec<half::f16> = (0..tokens * width)
                .map(|i| {
                    state =
                        state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
                    let r = (state >> 40) as f32 / (1u64 << 24) as f32 - 0.5;
                    let (token, at) = (i / width, i % width);
                    let growth = 1.0 + axis.position(token, bands) as f32 / 16.0;
                    let v = if at < inner {
                        4.0 * r
                    } else if at < 2 * inner {
                        r * growth
                    } else {
                        r
                    };
                    half::f16::from_f32(v)
                })
                .collect();
            let wide: Vec<f32> = qkvg.iter().map(|v| v.to_f32()).collect();
            let freqs = if axis == Axis::Time { &net.params.rotary_time } else { &net.params.rotary_freq };
            let expected = reference(&wide, c, freqs, axis, frames, bands);

            let planner = Planner { net: &net, frames };
            let input = storage(&net.gpu.device, bytemuck::cast_slice(&qkvg));
            let angles =
                storage(&net.gpu.device, bytemuck::cast_slice(&angles(freqs, axis.len(frames, bands))));
            let out = planner.buffer(Store::Act, tokens * inner, wgpu::BufferUsages::COPY_SRC);
            let step = planner.attention(&input, &angles, &out, axis);
            let read = net.gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: out.size(),
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let mut encoder = net.gpu.device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&step.pipeline);
                pass.set_bind_group(0, &step.bind_group, &[]);
                pass.dispatch_workgroups(step.groups.0, step.groups.1, 1);
            }
            encoder.copy_buffer_to_buffer(&out, 0, &read, 0, out.size());
            net.gpu.queue.submit([encoder.finish()]);
            read.slice(..).map_async(wgpu::MapMode::Read, |r| r.expect("mapped"));
            net.gpu.device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU done");
            let view = read.slice(..).get_mapped_range().expect("mapped");
            let got = &bytemuck::cast_slice::<u8, half::f16>(&view)[..tokens * inner];

            let num: f64 = got.iter().zip(&expected).map(|(a, b)| f64::from(a.to_f32() - b).powi(2)).sum();
            let den: f64 = expected.iter().map(|b| f64::from(*b).powi(2)).sum();
            let err = (num / den).sqrt();
            assert!(err < 2e-3, "{axis:?}: relative error {err:e}");
        }
    }
}
