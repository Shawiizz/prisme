//! Times the matrix products of the separation on this GPU, kernel by kernel, at the sizes of
//! a chunk: `matmul_bench [kernel.wgsl]...`. Besides the engine's two kernels, any WGSL file
//! with the same bindings is timed; its first line says how much of the output a workgroup
//! takes: `// bench: rows=64 cols=64`. Its result is checked against the engine's kernel.

use std::time::Instant;

use wgpu::util::DeviceExt;

const SMALL: &str = include_str!("../src/gpu/matmul_small.wgsl");
const LARGE: &str = include_str!("../src/gpu/matmul.wgsl");
const RUNS: u32 = 10;

/// The products of a chunk of the small vocals model (258 frames x 60 bands), and of the band
/// split into its first features.
const SHAPES: [(&str, usize, usize, usize); 4] = [
    ("qkvg", 15_480, 1544, 192),
    ("out", 15_480, 192, 512),
    ("ff1", 15_480, 768, 192),
    ("ff2", 15_480, 192, 768),
];

struct Kernel {
    name: String,
    source: String,
    rows: usize,
    cols: usize,
}

fn header_tile(source: &str) -> Option<(usize, usize)> {
    let line = source.lines().next()?.strip_prefix("// bench:")?;
    let value = |key: &str| line.split_whitespace().find_map(|w| w.strip_prefix(key)?.parse().ok());
    Some((value("rows=")?, value("cols=")?))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle().with_env());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    }))?;
    let info = adapter.get_info();
    let features = adapter.features() & wgpu::Features::SHADER_F16;
    let limits = adapter.limits();
    eprintln!(
        "{} ({:?}) · f16 {} · subgroups {}-{} · workgroup storage {} · binding {}",
        info.name,
        info.backend,
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

    let mut kernels = vec![Kernel { name: "engine small tiles".into(), source: SMALL.into(), rows: 64, cols: 64 }];
    // the large tiles spill on a GPU with fewer registers than NVIDIA's, and take minutes there
    if std::env::var_os("MATMUL_LARGE").is_some() {
        kernels.push(Kernel { name: "engine large tiles".into(), source: LARGE.into(), rows: 128, cols: 128 });
    }
    for path in std::env::args().skip(1) {
        let source = std::fs::read_to_string(&path)?;
        let (rows, cols) = header_tile(&source).ok_or(format!("{path}: no `// bench: rows= cols=` line"))?;
        kernels.push(Kernel { name: path, source, rows, cols });
    }

    for (shape, m, n, k) in SHAPES {
        let a: Vec<f32> = (0..m * k).map(|i| ((i * 7919 % 1000) as f32 / 1000.0) - 0.5).collect();
        let w: Vec<half::f16> =
            (0..n * k).map(|i| half::f16::from_f32(((i * 104_729 % 1000) as f32 / 1000.0 - 0.5) * 0.1)).collect();
        let bias: Vec<f32> = (0..n).map(|i| (i % 17) as f32 * 0.01).collect();
        let storage = |bytes: &[u8]| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytes,
                usage: wgpu::BufferUsages::STORAGE,
            })
        };
        let (a_buf, w_buf, b_buf) =
            (storage(bytemuck::cast_slice(&a)), storage(bytemuck::cast_slice(&w)), storage(bytemuck::cast_slice(&bias)));
        let params: [u32; 12] = [m as u32, n as u32, k as u32, k as u32, 0, n as u32, 0, 0, 0, 1, k as u32, 0];
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
        for kernel in &kernels {
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(&kernel.name),
                source: wgpu::ShaderSource::Wgsl(kernel.source.as_str().into()),
            });
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
            run(2);
            let start = Instant::now();
            run(RUNS);
            let seconds = start.elapsed().as_secs_f64() / f64::from(RUNS);
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
