//! `prisme`: command line front-end. Plays a track through the real engine, driven
//! internally, by a simulated turntable, or by a real one on the sound card input;
//! analyses tracks (tempo, key).

mod analyze;
mod latency;
mod simulation;
mod timecode;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use prisme_audio_io::{AudioCallback, Backend, DeviceKind, StreamSettings};
use prisme_engine::{Command, Engine, EngineConfig, EngineController, VinylMode};
use prisme_timecode::testing::SimulatedTurntable;
use prisme_timecode::TimecodeFormat;
use simulation::Pattern;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Parser)]
#[command(name = "prisme", version, about = "Prisme DJ engine, command line")]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Lists the sound devices.
    Devices,
    /// Plays a track on deck 1.
    Play {
        file: PathBuf,
        /// Tempo ratio (1.0 = original).
        #[arg(long, default_value_t = 1.0)]
        pitch: f64,
        #[arg(long, default_value_t = 10)]
        seconds: u64,
        #[command(flatten)]
        audio: AudioArgs,
    },
    /// Plays a track driven by a simulated turntable (no hardware needed).
    Simulate {
        file: PathBuf,
        #[arg(long, value_enum, default_value_t = Pattern::Scratch)]
        pattern: Pattern,
        #[arg(long, value_enum, default_value_t = Mode::Absolute)]
        mode: Mode,
        #[arg(long, default_value_t = 10)]
        seconds: u64,
        #[command(flatten)]
        audio: AudioArgs,
    },
    /// Plays a track driven by a real turntable on the sound card input.
    Vinyl {
        file: PathBuf,
        /// Part of the input device name (the DVS interface).
        #[arg(long)]
        input: String,
        #[arg(long, value_enum, default_value_t = Mode::Absolute)]
        mode: Mode,
        /// Timecode format id (serato_cv02_a, serato_cv02_b, serato_cd).
        #[arg(long, default_value = "serato_cv02_a")]
        format: String,
        /// Position of the track start on the record, in seconds.
        #[arg(long, default_value_t = 0.0)]
        lead_in: f64,
        #[arg(long, default_value_t = 60)]
        seconds: u64,
        #[command(flatten)]
        audio: AudioArgs,
    },
    /// Measures the round trip from an output to an input joined by a cable.
    Latency {
        /// Part of the input device name (ASIO: the same driver as the output).
        #[arg(long)]
        input: String,
        /// Output channel playing the clicks, from 1.
        #[arg(long, default_value_t = 1)]
        out_channel: usize,
        /// Input channel receiving them, from 1.
        #[arg(long, default_value_t = 1)]
        in_channel: usize,
        #[arg(long, default_value_t = 4)]
        seconds: u64,
        #[command(flatten)]
        audio: AudioArgs,
    },
    /// Decodes a recording of a turntable (WAV) and says how steadily the record reads.
    Timecode {
        file: PathBuf,
        /// Timecode format id (serato_cv02_a, serato_cv02_b, serato_cd).
        #[arg(long, default_value = "serato_cv02_a")]
        format: String,
        /// The left and right channels are crossed.
        #[arg(long)]
        swap: bool,
        /// Milliseconds between two printed readings.
        #[arg(long, default_value_t = 250.0)]
        every_ms: f64,
    },
    /// Detects the tempo and key of files or folders.
    Analyze {
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        /// Lower bound of the BPM range (the range spans one octave).
        #[arg(long, default_value_t = prisme_analysis::DEFAULT_MIN_BPM)]
        min_bpm: f64,
        /// Also writes the results to this CSV file.
        #[arg(long)]
        csv: Option<PathBuf>,
    },
}

#[derive(clap::Args, Clone)]
struct AudioArgs {
    #[arg(long, value_enum, default_value_t = BackendArg::Wasapi)]
    backend: BackendArg,
    /// Part of the output device name (default device if omitted).
    #[arg(long)]
    output: Option<String>,
    /// Requested buffer size in frames.
    #[arg(long)]
    buffer: Option<u32>,
    /// Master gain (1.0 = unity).
    #[arg(long, default_value_t = 1.0)]
    gain: f32,
}

#[derive(Clone, Copy, ValueEnum)]
enum BackendArg {
    Wasapi,
    Asio,
}

impl From<BackendArg> for Backend {
    fn from(b: BackendArg) -> Self {
        match b {
            BackendArg::Wasapi => Backend::Wasapi,
            BackendArg::Asio => Backend::Asio,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum Mode {
    Absolute,
    Relative,
}

impl From<Mode> for VinylMode {
    fn from(m: Mode) -> Self {
        match m {
            Mode::Absolute => VinylMode::Absolute,
            Mode::Relative => VinylMode::Relative,
        }
    }
}

/// Connects the engine (and optionally a simulated turntable) to the sound card.
struct EngineCallback {
    engine: Engine,
    turntable: Option<SimulatedTurntable>,
}

impl AudioCallback for EngineCallback {
    fn process(&mut self, input: &[f32], input_channels: usize, output: &mut [f32], output_channels: usize) {
        match self.turntable.as_mut() {
            Some(t) => {
                let frames = output.len() / output_channels;
                let signal = t.render(frames);
                self.engine.process(signal, 2, output, output_channels);
            }
            None => self.engine.process(input, input_channels, output, output_channels),
        }
    }
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Cmd::Devices => devices(),
        Cmd::Play { file, pitch, seconds, audio } => run(&file, &audio, None, None, seconds, |c| {
            c.send(Command::SetPitch { deck: 0, ratio: pitch }).ok();
            c.send(Command::SetPlaying { deck: 0, playing: true }).ok();
        }),
        Cmd::Simulate { file, pattern, mode, seconds, audio } => {
            let format = &prisme_timecode::SERATO_CV02_A;
            run(&file, &audio, None, Some((format, pattern)), seconds, |c| {
                c.send(Command::SetVinylMode { deck: 0, mode: mode.into() }).ok();
                c.send(Command::SetLeadIn { deck: 0, seconds: SimulatedTurntable::START_S }).ok();
                c.send(Command::SetVinylEnabled { deck: 0, enabled: true }).ok();
            })
        }
        Cmd::Vinyl { file, input, mode, format, lead_in, seconds, audio } => {
            let format = TimecodeFormat::by_id(&format)
                .with_context(|| format!("unknown timecode format {format}"))?;
            run(&file, &audio, Some((input, format)), None, seconds, |c| {
                c.send(Command::SetVinylMode { deck: 0, mode: mode.into() }).ok();
                c.send(Command::SetLeadIn { deck: 0, seconds: lead_in }).ok();
                c.send(Command::SetVinylEnabled { deck: 0, enabled: true }).ok();
            })
        }
        Cmd::Latency { input, out_channel, in_channel, seconds, audio } => {
            let settings = StreamSettings {
                backend: audio.backend.into(),
                output_device: audio.output.clone(),
                input_device: Some(input),
                buffer_frames: audio.buffer,
            };
            latency::run(&settings, out_channel, in_channel, seconds)
        }
        Cmd::Timecode { file, format, swap, every_ms } => {
            let format = TimecodeFormat::by_id(&format)
                .with_context(|| format!("unknown timecode format {format}"))?;
            timecode::run(&file, format, swap, every_ms)
        }
        Cmd::Analyze { paths, min_bpm, csv } => {
            analyze::run(&paths, prisme_analysis::AnalysisOptions { min_bpm }, csv.as_deref())
        }
    }
}

fn devices() -> Result<()> {
    for backend in Backend::available() {
        for d in prisme_audio_io::list_devices(backend)? {
            let kind = match d.kind {
                DeviceKind::Input => "in ",
                DeviceKind::Output => "out",
            };
            let default = if d.is_default { " (default)" } else { "" };
            println!("{backend:<6} {kind}  {:<60} {} ch, {} Hz{default}", d.name, d.channels, d.sample_rate);
        }
    }
    Ok(())
}

fn run(
    file: &Path,
    audio: &AudioArgs,
    input: Option<(String, &'static TimecodeFormat)>,
    simulate: Option<(&'static TimecodeFormat, Pattern)>,
    seconds: u64,
    setup: impl FnOnce(&mut EngineController),
) -> Result<()> {
    let started = Instant::now();
    let track = Arc::new(prisme_media::decode_file(file)?);
    println!(
        "{}: {:.1} s at {} Hz, decoded in {} ms",
        file.display(),
        track.duration_s(),
        track.sample_rate(),
        started.elapsed().as_millis()
    );

    let settings = StreamSettings {
        backend: audio.backend.into(),
        output_device: audio.output.clone(),
        input_device: input.as_ref().map(|(name, _)| name.clone()),
        buffer_frames: audio.buffer,
    };
    let format = input
        .as_ref()
        .map(|(_, f)| *f)
        .or(simulate.map(|(f, _)| f))
        .unwrap_or(&prisme_timecode::SERATO_CV02_A);
    let mut controller = None;
    let stream = prisme_audio_io::start(&settings, |f| {
        let (engine, c) = prisme_engine::create(EngineConfig::new(f.sample_rate, 2, format));
        controller = Some(c);
        let turntable =
            simulate.map(|(t, pattern)| SimulatedTurntable::new(t, f.sample_rate, pattern.into()));
        EngineCallback { engine, turntable }
    })?;
    let mut controller = controller.context("the engine was not created")?;
    let f = stream.format;
    println!(
        "audio: {} Hz, {} output / {} input channels",
        f.sample_rate, f.output_channels, f.input_channels
    );

    controller.send(Command::LoadTrack { deck: 0, track }).ok();
    controller.send(Command::SetMasterGain { gain: audio.gain }).ok();
    setup(&mut controller);

    let end = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < end {
        std::thread::sleep(Duration::from_millis(100));
        controller.collect_garbage();
        let s = controller.status();
        let d = s.decks[0];
        let record =
            d.vinyl.record_position_s.map(|p| format!("{p:7.2}s")).unwrap_or_else(|| "   --   ".into());
        print!(
            "\rpos {:7.2}s / {:6.1}s  speed {:+6.3}  record {}  signal {:<6} peak {:4.2}   ",
            d.position_s,
            d.duration_s,
            d.speed,
            record,
            format!("{:?}", d.vinyl.signal),
            s.main_peak
        );
        std::io::stdout().flush().ok();
    }
    println!();
    let st = &stream.stats;
    let cb = st.callbacks.load(Ordering::Relaxed).max(1);
    println!(
        "{} callbacks of ~{} frames, worst callback {:.0} us, over budget {}, input underruns {} / dropped {} frames",
        cb,
        st.frames.load(Ordering::Relaxed) / cb,
        st.max_callback_ns.load(Ordering::Relaxed) as f64 / 1000.0,
        st.over_budget.load(Ordering::Relaxed),
        st.input_underruns.load(Ordering::Relaxed),
        st.input_dropped.load(Ordering::Relaxed)
    );
    Ok(())
}
