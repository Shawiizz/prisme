//! Times the products of a chunk of a model on this GPU's shader cores, kernel by kernel:
//! `matmul_bench <model.safetensors> [kernel.wgsl]...`. Timed are the engine's kernels (the
//! blocked one, the small tiles, and the large ones with `MATMUL_LARGE=1`), then any WGSL file
//! with their bindings and parameters, bounding its indices itself as they do, whose header
//! says how much of the output a workgroup takes, on a line of its own among the comments it
//! opens with: `// bench: rows=64 cols=64` (a generated kernel gets it from its generator).
//! Each result is checked against the first kernel's.

use std::path::Path;
use std::time::Instant;

use prisme_stems::Weights;
use wgpu::util::DeviceExt;

const BLOCKED: &str = include_str!("../src/gpu/matmul_blocked.wgsl");
const SMALL: &str = include_str!("../src/gpu/matmul_small.wgsl");
const LARGE: &str = include_str!("../src/gpu/matmul.wgsl");
const RUNS: u32 = 4;
/// Products timed by the GPU in one submission: a phone's GPU stalls on long ones.
const TIMED: u32 = 3;
/// The GPU is kept busy this long before timing, for its clock to come up.
const WARM_UP_S: f64 = 1.5;

struct Kernel {
    name: String,
    source: String,
    rows: usize,
    cols: usize,
    /// It reads its inputs four values at a time: `k` a multiple of 4.
    by_four: bool,
}

/// The rows and columns of the output a workgroup takes, from the `// bench:` line among the
/// comments `source` opens with.
fn header_tile(source: &str) -> Option<(usize, usize)> {
    let header = source.lines().take_while(|l| l.starts_with("//"));
    let line = header.filter_map(|l| l.strip_prefix("// bench:")).next()?;
    let value = |key: &str| line.split_whitespace().find_map(|w| w.strip_prefix(key)?.parse().ok());
    Some((value("rows=")?, value("cols=")?))
}

/// The products of a chunk: of the first layer of the first transformer, as `(name, m, n, k)`.
fn shapes(weights: &Weights) -> Vec<(&'static str, usize, usize, usize)> {
    let g = &weights.geometry;
    let tokens = g.config.seq_frames(g.config.chunk_frames) * g.seq_bands();
    let layer = &weights.transformers.first().and_then(|t| t.layers.first()).expect("a transformer layer");
    [("qkvg", &layer.qkvg), ("out", &layer.out), ("ff1", &layer.ff1), ("ff2", &layer.ff2)]
        .map(|(name, l)| (name, tokens, l.out, l.inp))
        .into()
}

/// The time of one product as the GPU counts it, over [`TIMED`] of them, if it can.
fn gpu_time(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipeline: &wgpu::ComputePipeline,
    bind: &wgpu::BindGroup,
    groups: (u32, u32),
) -> Option<f64> {
    if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
        return None;
    }
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
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, bind, &[]);
        for _ in 0..TIMED {
            pass.dispatch_workgroups(groups.0, groups.1, 1);
        }
    }
    encoder.resolve_query_set(&set, 0..2, &resolved, 0);
    encoder.copy_buffer_to_buffer(&resolved, 0, &read, 0, 16);
    queue.submit([encoder.finish()]);
    read.slice(..).map_async(wgpu::MapMode::Read, |_| ());
    device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
    let ticks: Vec<u64> = bytemuck::cast_slice(&read.slice(..).get_mapped_range().ok()?).to_vec();
    let elapsed = ticks[1].checked_sub(ticks[0])? as f64 * f64::from(queue.get_timestamp_period()) * 1e-9;
    (elapsed > 0.0).then(|| elapsed / f64::from(TIMED))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let model = args.next().ok_or("usage: matmul_bench <model.safetensors> [kernel.wgsl]...")?;
    let weights = Weights::load(Path::new(&model))?;

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle().with_env());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    }))?;
    let info = adapter.get_info();
    let features = adapter.features() & (wgpu::Features::SHADER_F16 | wgpu::Features::TIMESTAMP_QUERY);
    let limits = adapter.limits();
    eprintln!(
        "{} ({:?}) · timestamps {} · f16 {} · subgroups {}-{} · workgroup storage {} · binding {}",
        info.name,
        info.backend,
        features.contains(wgpu::Features::TIMESTAMP_QUERY),
        features.contains(wgpu::Features::SHADER_F16),
        info.subgroup_min_size,
        info.subgroup_max_size,
        limits.max_compute_workgroup_storage_size,
        limits.max_storage_buffer_binding_size,
    );
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: features,
        required_limits: limits,
        ..Default::default()
    }))?;

    let engine = |name: &str, source: &str, tile: usize, by_four: bool| Kernel {
        name: name.into(),
        source: source.into(),
        rows: tile,
        cols: tile,
        by_four,
    };
    let mut kernels =
        vec![engine("engine blocked", BLOCKED, 64, true), engine("engine small tiles", SMALL, 64, false)];
    // the large tiles spill on a GPU with fewer registers than NVIDIA's, and take minutes there
    if std::env::var_os("MATMUL_LARGE").is_some() {
        kernels.push(engine("engine large tiles", LARGE, 128, false));
    }
    for path in args {
        let source = std::fs::read_to_string(&path)?;
        let (rows, cols) = header_tile(&source).ok_or(format!("{path}: no `// bench: rows= cols=` line"))?;
        kernels.push(Kernel { name: path, source, rows, cols, by_four: false });
    }

    for (shape, m, n, k) in shapes(&weights) {
        let a: Vec<f32> = (0..m * k).map(|i| ((i * 7919 % 1000) as f32 / 1000.0) - 0.5).collect();
        let w: Vec<half::f16> = (0..n * k)
            .map(|i| half::f16::from_f32(((i * 104_729 % 1000) as f32 / 1000.0 - 0.5) * 0.1))
            .collect();
        let bias: Vec<f32> = (0..n).map(|i| (i % 17) as f32 * 0.01).collect();
        let storage = |bytes: &[u8]| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytes,
                usage: wgpu::BufferUsages::STORAGE,
            })
        };
        let (a_buf, w_buf, b_buf) = (
            storage(bytemuck::cast_slice(&a)),
            storage(bytemuck::cast_slice(&w)),
            storage(bytemuck::cast_slice(&bias)),
        );
        // m, n, k, lda, a_off, ldc, c_off, no activation, no accumulation, a bias
        let params: [u32; 12] = [m as u32, n as u32, k as u32, k as u32, 0, n as u32, 0, 0, 0, 1, 0, 0];
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let out_bytes = (m * n * 4) as u64;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: out_bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut reference: Option<Vec<f32>> = None;
        for kernel in kernels.iter().filter(|kernel| !kernel.by_four || k.is_multiple_of(4)) {
            let descriptor = wgpu::ShaderModuleDescriptor {
                label: Some(&kernel.name),
                source: wgpu::ShaderSource::Wgsl(kernel.source.as_str().into()),
            };
            // SAFETY: the kernels bound every index themselves, as the engine's do; created as
            // the engine creates them, without the checks that would slow them many times over
            let module = unsafe {
                device.create_shader_module_trusted(descriptor, wgpu::ShaderRuntimeChecks::unchecked())
            };
            let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: None,
                layout: None,
                module: &module,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            });
            let c_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: out_bytes,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[&a_buf, &w_buf, &b_buf, &c_buf, &uniform]
                    .iter()
                    .enumerate()
                    .map(|(i, b)| wgpu::BindGroupEntry { binding: i as u32, resource: b.as_entire_binding() })
                    .collect::<Vec<_>>(),
            });
            let groups = (n.div_ceil(kernel.cols) as u32, m.div_ceil(kernel.rows) as u32);
            let run = |times: u32| {
                let mut encoder = device.create_command_encoder(&Default::default());
                for _ in 0..times {
                    let mut pass = encoder.begin_compute_pass(&Default::default());
                    pass.set_pipeline(&pipeline);
                    pass.set_bind_group(0, &bind, &[]);
                    pass.dispatch_workgroups(groups.0, groups.1, 1);
                }
                queue.submit([encoder.finish()]);
                device.poll(wgpu::PollType::wait_indefinitely()).expect("the GPU answers");
            };
            let warm = Instant::now();
            while warm.elapsed().as_secs_f64() < WARM_UP_S {
                run(2);
            }
            let start = Instant::now();
            run(RUNS);
            let wall = start.elapsed().as_secs_f64() / f64::from(RUNS);
            let seconds = gpu_time(&device, &queue, &pipeline, &bind, groups).unwrap_or(wall);
            let gflops = 2.0 * (m * n * k) as f64 / seconds / 1e9;

            let mut encoder = device.create_command_encoder(&Default::default());
            encoder.copy_buffer_to_buffer(&c_buf, 0, &readback, 0, out_bytes);
            queue.submit([encoder.finish()]);
            readback.slice(..).map_async(wgpu::MapMode::Read, |_| ());
            device.poll(wgpu::PollType::wait_indefinitely()).expect("the GPU answers");
            let out: Vec<f32> = bytemuck::cast_slice(&readback.slice(..).get_mapped_range()?).to_vec();
            readback.unmap();
            let error = reference.as_ref().map_or(0.0, |r| {
                let worst = r.iter().zip(&out).map(|(x, y)| (x - y).abs()).fold(0.0f32, f32::max);
                worst / r.iter().map(|x| x.abs()).fold(1e-6f32, f32::max)
            });
            reference.get_or_insert(out);
            println!(
                "{shape:>5} {m}x{n}x{k} · {:<28} {:>8.2} ms {:>8.1} GFLOP/s  error {error:.1e}",
                kernel.name,
                seconds * 1e3,
                gflops
            );
        }
    }
    Ok(())
}
