//! Owns the running audio: the sound card stream lives on its own thread (it cannot move
//! between threads), the engine runs inside its callback, and the controller is handed
//! back to the application.

use crate::dto::{AudioInfo, AudioSettings};
use crate::error::{AppError, AppResult};
use prisme_audio_io::{AudioCallback, StreamSettings};
use prisme_engine::{Engine, EngineConfig, EngineController};
use prisme_timecode::testing::{Pattern, SimulatedTurntable};
use prisme_timecode::TimecodeFormat;
use std::sync::mpsc;
use std::thread::JoinHandle;

/// Number of decks of the desktop application.
pub const DECKS: usize = 2;

/// Connects the engine to the sound card; optionally replaces deck 1's turntable input
/// with a simulated one.
struct EngineCallback {
    engine: Engine,
    turntable: Option<SimulatedTurntable>,
}

impl AudioCallback for EngineCallback {
    fn process(&mut self, input: &[f32], input_channels: usize, output: &mut [f32], output_channels: usize) {
        match self.turntable.as_mut() {
            Some(t) => {
                let frames = output.len() / output_channels.max(1);
                let signal = t.render(frames);
                self.engine.process(signal, 2, output, output_channels);
            }
            None => self.engine.process(input, input_channels, output, output_channels),
        }
    }
}

/// A running audio session. Dropping it stops the audio.
pub struct AudioSession {
    pub controller: EngineController,
    /// What the engine was built with (to build matching decoders).
    pub engine_config: EngineConfig,
    pub info: AudioInfo,
    stop: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for AudioSession {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Starts the audio with `settings`.
pub fn start(settings: &AudioSettings) -> AppResult<AudioSession> {
    let format = TimecodeFormat::by_id(&settings.timecode_format)
        .ok_or_else(|| AppError::UnknownFormat(settings.timecode_format.clone()))?;
    let stream_settings = StreamSettings {
        backend: settings.backend.into(),
        output_device: settings.output_device.clone(),
        input_device: if settings.simulate { None } else { settings.input_device.clone() },
        buffer_frames: settings.buffer_frames,
    };
    let simulate = settings.simulate;

    let (ready_tx, ready_rx) = mpsc::channel::<AppResult<(EngineController, EngineConfig, AudioInfo)>>();
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let thread = std::thread::Builder::new().name("prisme-audio".into()).spawn(move || {
        let mut created = None;
        let started = prisme_audio_io::start(&stream_settings, |f| {
            let config = EngineConfig::new(f.sample_rate, DECKS, format);
            let (engine, controller) = prisme_engine::create(config);
            created = Some((controller, config));
            let turntable =
                simulate.then(|| SimulatedTurntable::new(format, f.sample_rate, Pattern::Scratch));
            EngineCallback { engine, turntable }
        });
        match (started, created) {
            (Ok(stream), Some((controller, config))) => {
                let info = AudioInfo {
                    sample_rate: stream.format.sample_rate,
                    output_channels: stream.format.output_channels,
                    input_channels: stream.format.input_channels,
                    simulated: simulate,
                };
                if ready_tx.send(Ok((controller, config, info))).is_ok() {
                    let _ = stop_rx.recv(); // keep the stream alive until asked to stop
                }
                drop(stream);
            }
            (Err(e), _) => {
                let _ = ready_tx.send(Err(e.into()));
            }
            (Ok(_), None) => unreachable!("the callback is built before the stream starts"),
        }
    })?;
    let (controller, engine_config, info) =
        ready_rx.recv().map_err(|_| AppError::Other("the audio thread stopped".into()))??;
    Ok(AudioSession { controller, engine_config, info, stop: Some(stop_tx), thread: Some(thread) })
}
