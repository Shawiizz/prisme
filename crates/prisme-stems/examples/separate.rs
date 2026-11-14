//! Separates a stereo WAV at the model's rate, on the GPU when there is one
//! (`PRISME_STEMS_CPU=1` forces the CPU, `PRISME_STEMS_FLOAT=1` f32 arithmetic on the GPU,
//! `PRISME_STEMS_MAX_BINDING=<bytes>` the buffers of a smaller GPU, `PRISME_STEMS_PROFILE=1`
//! the time of each kind of work, `PRISME_STEMS_SHARE=0.5` half the GPU's time at most,
//! `PRISME_STEMS_ATTENTION=products|one-pass` the attention on the shader cores):
//! `separate <model.safetensors> <in.wav> <out>` writes `<out>.<stem>.wav` for each stem of the
//! model.

use std::path::PathBuf;
use std::time::Instant;

use prisme_stems::gpu::Attention;
use prisme_stems::{CpuNetwork, GpuNetwork, GpuOptions, MaskNetwork, Separator, Weights};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let [model, input, output] = args.as_slice() else {
        return Err("usage: separate <model.safetensors> <in.wav> <out>".into());
    };
    let t = Instant::now();
    let weights = Weights::load(model)?;
    eprintln!("model loaded in {:.1} s", t.elapsed().as_secs_f64());

    let mut reader = hound::WavReader::open(input)?;
    let spec = reader.spec();
    let samples: Vec<f32> = reader.samples::<f32>().collect::<Result<_, _>>()?;
    let left: Vec<f32> = samples.iter().step_by(2).copied().collect();
    let right: Vec<f32> = samples.iter().skip(1).step_by(2).copied().collect();

    if std::env::var_os("PRISME_STEMS_CPU").is_some() {
        run(&mut Separator::new(CpuNetwork::new(weights)), &left, &right, spec, output)
    } else {
        let t = Instant::now();
        let attention = match std::env::var("PRISME_STEMS_ATTENTION").as_deref() {
            Ok("products") => Some(Attention::Products),
            Ok("one-pass") => Some(Attention::OnePass),
            Ok(other) => {
                return Err(format!("PRISME_STEMS_ATTENTION: products or one-pass, not {other}").into())
            }
            Err(_) => None,
        };
        let options = GpuOptions {
            without_matrix_units: std::env::var_os("PRISME_STEMS_FLOAT").is_some(),
            max_binding: std::env::var("PRISME_STEMS_MAX_BINDING").ok().and_then(|b| b.parse().ok()),
            profile: std::env::var_os("PRISME_STEMS_PROFILE").is_some(),
            share: std::env::var("PRISME_STEMS_SHARE").ok().and_then(|s| s.parse().ok()),
            attention,
            ..Default::default()
        };
        let gpu = GpuNetwork::with_options(weights, options)?;
        eprintln!("{} ready in {:.1} s", gpu.adapter(), t.elapsed().as_secs_f64());
        let mut separator = Separator::new(gpu);
        run(&mut separator, &left, &right, spec, output)?;
        let profile = separator.network().profile();
        let total: f64 = profile.iter().map(|(_, s, _)| s).sum();
        for (kind, seconds, steps) in profile {
            eprintln!("{kind:>20} {seconds:8.2} s {:5.1} %  {steps} steps", 100.0 * seconds / total);
        }
        Ok(())
    }
}

fn run(
    separator: &mut Separator<impl MaskNetwork>,
    left: &[f32],
    right: &[f32],
    spec: hound::WavSpec,
    output: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let t = Instant::now();
    let stems = separator.separate(left, right, |done, total| {
        eprintln!("chunk {done}/{total} at {:.1} s", t.elapsed().as_secs_f64());
        true
    })?;
    let seconds = left.len() as f64 / f64::from(spec.sample_rate);
    let took = t.elapsed().as_secs_f64();
    eprintln!("{seconds:.1} s of audio in {took:.1} s ({:.2}x)", seconds / took);

    for (name, [l, r]) in separator.stems().iter().zip(stems) {
        let mut path = output.as_os_str().to_owned();
        path.push(format!(".{name}.wav"));
        let mut writer = hound::WavWriter::create(path, spec)?;
        for (a, b) in l.iter().zip(&r) {
            writer.write_sample(*a)?;
            writer.write_sample(*b)?;
        }
        writer.finalize()?;
    }
    Ok(())
}
