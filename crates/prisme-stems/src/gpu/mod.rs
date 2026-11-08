//! The network on the GPU, through wgpu (Direct3D 12 or Vulkan): any recent GPU, no runtime
//! to install.
//!
//! The work for a chunk is planned once (buffers, bind groups, dispatches) per frame count,
//! then replayed: only the input bins change between chunks.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use wgpu::util::DeviceExt;

use crate::model::{Geometry, Linear, Weights};
use crate::network::{Axis, MaskNetwork};
use crate::StemsError;

const MAX_GROUPS: u32 = 65_535;

pub struct GpuNetwork {
    geometry: Geometry,
    device: wgpu::Device,
    queue: wgpu::Queue,
    adapter: String,
    errors: Arc<Mutex<Option<String>>>,
    pipelines: Pipelines,
    params: Params,
    plans: HashMap<usize, Plan>,
}

/// The weights, on the GPU.
struct Params {
    rotary_time: Vec<f32>,
    rotary_freq: Vec<f32>,
    band_proj: Vec<GpuLinear>,
    transformers: Vec<(Vec<GpuLayer>, wgpu::Buffer)>,
    masks: Vec<Vec<GpuLinear>>,
    no_bias: wgpu::Buffer,
    ff_hidden: usize,
    mask_hidden: usize,
}

struct GpuLinear {
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

struct Pipelines {
    matmul: wgpu::ComputePipeline,
    norm: wgpu::ComputePipeline,
    norm_in_place: wgpu::ComputePipeline,
    rotary: wgpu::ComputePipeline,
    attention: wgpu::ComputePipeline,
    glu: wgpu::ComputePipeline,
}

/// The dispatches for one frame count, in submissions short enough not to trip the
/// operating system's GPU watchdog on a slow GPU.
struct Plan {
    bins: wgpu::Buffer,
    mask: wgpu::Buffer,
    readback: wgpu::Buffer,
    submissions: Vec<Vec<Step>>,
}

struct Step {
    pipeline: Kind,
    bind_group: wgpu::BindGroup,
    groups: (u32, u32),
}

#[derive(Clone, Copy)]
enum Kind {
    Matmul,
    Norm,
    NormInPlace,
    Rotary,
    Attention,
    Glu,
}

#[derive(Clone, Copy)]
enum Act {
    None = 0,
    Gelu = 1,
    Tanh = 2,
}

impl GpuNetwork {
    /// Takes the most capable GPU; fails without one (software rasterisers are refused: the
    /// CPU implementation is faster).
    pub fn new(weights: Weights) -> Result<Self, StemsError> {
        Self::open(weights, false)
    }

    /// Accepts a software rasteriser too: for tests on machines without a GPU.
    pub fn new_allowing_software(weights: Weights) -> Result<Self, StemsError> {
        Self::open(weights, true)
    }

    fn open(weights: Weights, software: bool) -> Result<Self, StemsError> {
        let c = &weights.geometry.config;
        if c.dim_head > 64 || !c.dim_head.is_multiple_of(2) {
            return Err(StemsError::Model("the GPU attention takes heads of at most 64 values".into()));
        }
        let adapter = adapter()?;
        let info = adapter.get_info();
        if info.device_type == wgpu::DeviceType::Cpu && !software {
            return Err(StemsError::NoGpu(format!("{} is a software renderer", info.name)));
        }
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("prisme-stems"),
            required_limits: adapter.limits(),
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

        let pipelines = Pipelines::new(&device, weights.geometry.config.dim_head);
        let params = Params::upload(&device, &weights);
        let network = Self {
            geometry: weights.geometry,
            device,
            queue,
            adapter: format!("{} ({:?})", info.name, info.backend),
            errors,
            pipelines,
            params,
            plans: HashMap::new(),
        };
        network.check_errors()?;
        let frames = network.geometry.config.chunk_frames;
        network.check_limits(frames)?;
        Ok(network)
    }

    /// The GPU and graphics API in use.
    pub fn adapter(&self) -> &str {
        &self.adapter
    }

    fn check_errors(&self) -> Result<(), StemsError> {
        match self.errors.lock().expect("error sink").take() {
            Some(e) => Err(StemsError::Gpu(e)),
            None => Ok(()),
        }
    }

    /// Whether the largest activation fits in one buffer binding.
    fn check_limits(&self, frames: usize) -> Result<(), StemsError> {
        let c = &self.geometry.config;
        let tokens = frames * self.geometry.bands.len();
        let widest = c.qkvg_width().max(self.params.ff_hidden);
        let bytes = (tokens * widest * 4) as u64;
        let limits = self.device.limits();
        if bytes > limits.max_storage_buffer_binding_size || bytes > limits.max_buffer_size {
            return Err(StemsError::Gpu(format!("{bytes} byte buffers exceed this GPU's limits")));
        }
        Ok(())
    }

    fn prepare(&mut self, frames: usize) -> Result<(), StemsError> {
        if !self.plans.contains_key(&frames) {
            self.check_limits(frames)?;
            let plan = Planner { net: self, frames }.build();
            self.check_errors()?;
            self.plans.insert(frames, plan);
        }
        Ok(())
    }
}

impl MaskNetwork for GpuNetwork {
    fn geometry(&self) -> &Geometry {
        &self.geometry
    }

    fn masks(&mut self, bins: &[f32], frames: usize, out: &mut [f32]) -> Result<(), StemsError> {
        let size = frames * self.geometry.features();
        assert!(bins.len() == size && out.len() == size, "bins and masks hold `frames` rows of features");
        self.prepare(frames)?;
        let plan = &self.plans[&frames];
        self.queue.write_buffer(&plan.bins, 0, bytemuck::cast_slice(bins));
        for (i, steps) in plan.submissions.iter().enumerate() {
            let mut encoder = self.device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                for step in steps {
                    pass.set_pipeline(self.pipelines.get(step.pipeline));
                    pass.set_bind_group(0, &step.bind_group, &[]);
                    pass.dispatch_workgroups(step.groups.0, step.groups.1, 1);
                }
            }
            if i + 1 == plan.submissions.len() {
                encoder.copy_buffer_to_buffer(&plan.mask, 0, &plan.readback, 0, plan.mask.size());
            }
            self.queue.submit([encoder.finish()]);
        }
        let slice = plan.readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| StemsError::Gpu(e.to_string()))?;
        rx.recv().map_err(|e| StemsError::Gpu(e.to_string()))?.map_err(|e| StemsError::Gpu(e.to_string()))?;
        {
            let view = slice.get_mapped_range().map_err(|e| StemsError::Gpu(e.to_string()))?;
            out.copy_from_slice(bytemuck::cast_slice(&view));
        }
        plan.readback.unmap();
        self.check_errors()
    }
}

/// The most capable GPU, through Vulkan when it can (its compiler makes the matrix products
/// faster than Direct3D 12's), else whatever API reaches one. `WGPU_BACKEND` overrides.
fn adapter() -> Result<wgpu::Adapter, StemsError> {
    let request = |backends: wgpu::Backends| {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = backends;
        let instance = wgpu::Instance::new(desc.with_env());
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
    };
    request(wgpu::Backends::VULKAN)
        .or_else(|_| request(wgpu::Backends::PRIMARY))
        .map_err(|e| StemsError::NoGpu(e.to_string()))
}

impl Pipelines {
    fn new(device: &wgpu::Device, dim_head: usize) -> Self {
        let make = |source: &str, entry: &str, constants: &[(&str, f64)]| {
            let descriptor = wgpu::ShaderModuleDescriptor {
                label: Some(entry),
                source: wgpu::ShaderSource::Wgsl(source.into()),
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
        Self {
            matmul: make(include_str!("matmul.wgsl"), "main", &[]),
            norm: make(include_str!("norm.wgsl"), "normalize", &[]),
            norm_in_place: make(include_str!("norm_in_place.wgsl"), "normalize_in_place", &[]),
            rotary: make(include_str!("rotary.wgsl"), "main", &[]),
            attention: make(include_str!("attention.wgsl"), "main", &[("DH", dim_head as f64)]),
            glu: make(include_str!("glu.wgsl"), "main", &[]),
        }
    }

    fn get(&self, kind: Kind) -> &wgpu::ComputePipeline {
        match kind {
            Kind::Matmul => &self.matmul,
            Kind::Norm => &self.norm,
            Kind::NormInPlace => &self.norm_in_place,
            Kind::Rotary => &self.rotary,
            Kind::Attention => &self.attention,
            Kind::Glu => &self.glu,
        }
    }
}

impl Params {
    fn upload(device: &wgpu::Device, w: &Weights) -> Self {
        let linear = |l: &Linear| GpuLinear {
            weight: storage(device, &l.weight),
            bias: l.bias.as_ref().map(|b| storage(device, b)),
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
                        ff1: linear(&l.ff1),
                        ff2: linear(&l.ff2),
                    })
                    .collect();
                (layers, storage(device, &tr.norm))
            })
            .collect();
        let ff_hidden = w.transformers.iter().flat_map(|t| &t.layers).map(|l| l.ff1.out).max().unwrap_or(0);
        let mask_hidden = w
            .masks
            .iter()
            .flat_map(|m| m.split_last().map_or(&[][..], |(_, hidden)| hidden))
            .map(|l| l.out)
            .max()
            .unwrap_or(0);
        Self {
            rotary_time: w.rotary_time.clone(),
            rotary_freq: w.rotary_freq.clone(),
            band_proj: w.band_proj.iter().map(linear).collect(),
            transformers,
            masks: w.masks.iter().map(|m| m.iter().map(linear).collect()).collect(),
            no_bias: storage(device, &[0.0; 4]),
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
        let mask_out = p.masks.iter().filter_map(|m| m.last()).map(|l| l.out).max().unwrap_or(0);

        let bins = self.buffer(frames * features, wgpu::BufferUsages::COPY_DST);
        let mask = self.buffer(frames * features, wgpu::BufferUsages::COPY_SRC);
        let readback = self.net.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (frames * features * 4) as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let x = self.buffer(tokens * c.dim, wgpu::BufferUsages::empty());
        let normed = self.buffer(tokens * c.dim, wgpu::BufferUsages::empty());
        let qkvg = self.buffer(tokens * c.qkvg_width(), wgpu::BufferUsages::empty());
        let attended = self.buffer(tokens * c.inner_dim(), wgpu::BufferUsages::empty());
        let hidden = self.buffer(tokens * p.ff_hidden, wgpu::BufferUsages::empty());
        let band_in = self.buffer(frames * widest_band, wgpu::BufferUsages::empty());
        let mlp = [
            self.buffer(frames * p.mask_hidden, wgpu::BufferUsages::empty()),
            self.buffer(frames * p.mask_hidden, wgpu::BufferUsages::empty()),
        ];
        let mlp_out = self.buffer(frames * mask_out, wgpu::BufferUsages::empty());
        let angles_time = storage(&self.net.device, &angles(&p.rotary_time, frames));
        let angles_freq = storage(&self.net.device, &angles(&p.rotary_freq, bands));

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
            let into = Out { buffer: &x, ld: bands * c.dim, off: b * c.dim };
            steps.push(self.matmul(proj, (&band_in, band.width, 0), frames, into, Act::None, false));
        }
        submissions.push(std::mem::take(&mut steps));

        for (i, (layers, gain)) in p.transformers.iter().enumerate() {
            let axis = if i % 2 == 0 { Axis::Time } else { Axis::Freq };
            let angles = if axis == Axis::Time { &angles_time } else { &angles_freq };
            for l in layers {
                steps.push(self.norm(&x, &normed, tokens, c.dim, (c.dim, 0), (c.dim, 0)));
                let into = Out { buffer: &qkvg, ld: c.qkvg_width(), off: 0 };
                steps.push(self.matmul(&l.qkvg, (&normed, c.dim, 0), tokens, into, Act::None, false));
                steps.push(self.rotary(&qkvg, angles, axis, tokens));
                steps.push(self.attention(&qkvg, &attended, axis));
                let into = Out { buffer: &x, ld: c.dim, off: 0 };
                steps.push(self.matmul(&l.out, (&attended, c.inner_dim(), 0), tokens, into, Act::None, true));
                steps.push(self.norm(&x, &normed, tokens, c.dim, (c.dim, 0), (c.dim, 0)));
                let into = Out { buffer: &hidden, ld: l.ff1.out, off: 0 };
                steps.push(self.matmul(&l.ff1, (&normed, c.dim, 0), tokens, into, Act::Gelu, false));
                let into = Out { buffer: &x, ld: c.dim, off: 0 };
                steps.push(self.matmul(&l.ff2, (&hidden, l.ff1.out, 0), tokens, into, Act::None, true));
            }
            steps.push(self.norm_in_place(&x, gain, tokens, c.dim));
            submissions.push(std::mem::take(&mut steps));
        }

        for (b, (band, mlp_layers)) in g.bands.iter().zip(&p.masks).enumerate() {
            let mut input = (&x, bands * c.dim, b * c.dim);
            for (k, l) in mlp_layers.iter().enumerate() {
                let last = k + 1 == mlp_layers.len();
                let target = if last { &mlp_out } else { &mlp[k % 2] };
                let act = if last { Act::None } else { Act::Tanh };
                steps.push(self.matmul(
                    l,
                    input,
                    frames,
                    Out { buffer: target, ld: l.out, off: 0 },
                    act,
                    false,
                ));
                input = (target, l.out, 0);
            }
            steps.push(self.glu(&mlp_out, &mask, frames, band.width, features, band.offset));
        }
        submissions.push(steps);

        Plan { bins, mask, readback, submissions }
    }

    fn buffer(&self, floats: usize, usage: wgpu::BufferUsages) -> wgpu::Buffer {
        self.net.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (floats.max(4) * 4) as u64,
            usage: wgpu::BufferUsages::STORAGE | usage,
            mapped_at_creation: false,
        })
    }

    fn step(&self, kind: Kind, groups: (u32, u32), buffers: &[&wgpu::Buffer], uniform: &[u32]) -> Step {
        let device = &self.net.device;
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
            layout: &self.net.pipelines.get(kind).get_bind_group_layout(0),
            entries: &entries,
        });
        Step { pipeline: kind, bind_group, groups }
    }

    fn matmul(
        &self,
        l: &GpuLinear,
        (a, lda, a_off): (&wgpu::Buffer, usize, usize),
        rows: usize,
        out: Out,
        act: Act,
        accumulate: bool,
    ) -> Step {
        let bias = l.bias.as_ref().unwrap_or(&self.net.params.no_bias);
        let groups = (l.out.div_ceil(128) as u32, rows.div_ceil(128) as u32);
        let uniform = [
            rows,
            l.out,
            l.inp,
            lda,
            a_off,
            out.ld,
            out.off,
            act as usize,
            accumulate as usize,
            l.bias.is_some() as usize,
        ];
        self.step(Kind::Matmul, groups, &[a, &l.weight, bias, out.buffer], &words(&uniform))
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
        self.step(Kind::Norm, groups, &[src, dst, &self.net.params.no_bias], &words(&uniform))
    }

    fn norm_in_place(&self, x: &wgpu::Buffer, gain: &wgpu::Buffer, rows: usize, width: usize) -> Step {
        let (groups, groups_x) = grid(rows);
        let uniform = [rows, width, width, 0, width, 0, 1, groups_x];
        self.step(Kind::NormInPlace, groups, &[x, gain], &words(&uniform))
    }

    fn rotary(&self, qkvg: &wgpu::Buffer, angles: &wgpu::Buffer, axis: Axis, tokens: usize) -> Step {
        let g = &self.net.geometry;
        let c = &g.config;
        let bands = g.bands.len();
        let pairs = c.dim_head / 2;
        let (groups, groups_x) = grid((tokens * 2 * c.heads * pairs).div_ceil(256));
        let positions = axis.len(self.frames, bands);
        let uniform = [tokens, 2 * c.heads, pairs, c.qkvg_width(), bands, axis_id(axis), positions, groups_x];
        self.step(Kind::Rotary, groups, &[qkvg, angles], &words(&uniform))
    }

    fn attention(&self, qkvg: &wgpu::Buffer, out: &wgpu::Buffer, axis: Axis) -> Step {
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
        self.step(Kind::Attention, groups, &[qkvg, out], &words(&uniform))
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
        self.step(Kind::Glu, groups, &[y, mask], &words(&[rows, width, ld, off, groups_x]))
    }
}

struct Out<'a> {
    buffer: &'a wgpu::Buffer,
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

fn storage(device: &wgpu::Device, data: &[f32]) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(data),
        usage: wgpu::BufferUsages::STORAGE,
    })
}
