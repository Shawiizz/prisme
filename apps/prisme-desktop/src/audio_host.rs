//! Owns the running audio: the sound card stream lives on its own thread (it cannot move
//! between threads), the engine runs inside its callback, and the controller is handed
//! back to the application.

use crate::dto::{AudioInfo, AudioSettings};
use crate::error::{AppError, AppResult};
use prisme_audio_io::{is_available, AudioCallback, DeviceKind, StreamSettings, StreamStats};
use prisme_engine::{Engine, EngineConfig, EngineController};
use prisme_timecode::testing::{Pattern, SimulatedTurntable};
use prisme_timecode::TimecodeFormat;
use std::sync::atomic::Ordering;
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;

/// The stereo pair of deck `deck` on a device with `channels` (the inputs, for its
/// turntable; the outputs, to an external mixer): its own when each of the `decks` has one,
/// else that of the deck it is layered over, C over A and D over B, as the layers of a
/// controller with two jogs.
pub fn deck_pair(deck: usize, decks: usize, channels: usize) -> usize {
    if channels >= 2 * decks {
        deck
    } else {
        deck % 2
    }
}

/// The deck layered with `deck` on a device with `channels`, if the decks share their pairs.
pub fn layer_partner(deck: usize, decks: usize, channels: usize) -> Option<usize> {
    let partner = deck ^ 2;
    (channels < 2 * decks && partner < decks).then_some(partner)
}

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
    pub stats: Arc<StreamStats>,
    /// What the engine was built with (to build matching decoders).
    pub engine_config: EngineConfig,
    pub info: AudioInfo,
    /// What it was started with, to start it again.
    pub settings: AudioSettings,
    stop: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
    /// The computer stays awake while the audio runs.
    _awake: Option<crate::keep_awake::KeepAwake>,
}

impl AudioSession {
    /// The device went away (unplugged): the session must be restarted.
    pub fn device_lost(&self) -> bool {
        self.stats.device_lost.load(Ordering::Relaxed)
    }

    /// The engine failed: the audio is silent until it is restarted.
    pub fn engine_failed(&self) -> bool {
        self.stats.callback_failed.load(Ordering::Relaxed)
    }

    /// Periods processed so far.
    pub fn periods(&self) -> u64 {
        self.stats.callbacks.load(Ordering::Relaxed)
    }
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

/// Whether the devices `settings` asks for are there.
pub fn devices_available(settings: &AudioSettings) -> bool {
    let backend = settings.backend.into();
    let input = settings.input_device.as_deref().filter(|_| !settings.simulate);
    is_available(backend, DeviceKind::Output, settings.output_device.as_deref())
        && input.is_none_or(|name| is_available(backend, DeviceKind::Input, Some(name)))
}

/// Starts the audio with `settings`, for `decks` decks.
pub fn start(settings: &AudioSettings, decks: usize) -> AppResult<AudioSession> {
    let format = TimecodeFormat::by_id(&settings.timecode_format)
        .ok_or_else(|| AppError::UnknownFormat(settings.timecode_format.clone()))?;
    let stream_settings = StreamSettings {
        backend: settings.backend.into(),
        output_device: settings.output_device.clone(),
        input_device: if settings.simulate { None } else { settings.input_device.clone() },
        buffer_frames: settings.buffer_frames,
    };
    let simulate = settings.simulate;
    let backend = settings.backend;

    type Ready = (EngineController, EngineConfig, AudioInfo, Arc<StreamStats>);
    let (ready_tx, ready_rx) = mpsc::channel::<AppResult<Ready>>();
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let thread = std::thread::Builder::new().name("prisme-audio".into()).spawn(move || {
        let mut created = None;
        let started = prisme_audio_io::start(&stream_settings, |f| {
            let mut config = EngineConfig::new(f.sample_rate, decks, format);
            config.deck_inputs =
                std::array::from_fn(|deck| Some(2 * deck_pair(deck, config.decks, f.input_channels)));
            let (engine, controller) = prisme_engine::create(config);
            created = Some((controller, config));
            let turntable =
                simulate.then(|| SimulatedTurntable::new(format, f.sample_rate, Pattern::Scratch));
            EngineCallback { engine, turntable }
        });
        match (started, created) {
            (Ok(stream), Some((controller, config))) => {
                let info = AudioInfo {
                    backend,
                    device: stream.output_device.clone(),
                    sample_rate: stream.format.sample_rate,
                    output_channels: stream.format.output_channels,
                    input_channels: stream.format.input_channels,
                    decks: config.decks,
                    simulated: simulate,
                };
                if ready_tx.send(Ok((controller, config, info, stream.stats.clone()))).is_ok() {
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
    let (controller, engine_config, info, stats) =
        ready_rx.recv().map_err(|_| AppError::Other("the audio thread stopped".into()))??;
    Ok(AudioSession {
        controller,
        stats,
        engine_config,
        info,
        settings: settings.clone(),
        stop: Some(stop_tx),
        thread: Some(thread),
        _awake: crate::keep_awake::KeepAwake::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decks_without_a_pair_of_their_own_are_layered() {
        // an interface with four channels: two decks each on their pair
        assert_eq!([0, 1].map(|d| deck_pair(d, 2, 4)), [0, 1]);
        assert_eq!(layer_partner(0, 2, 4), None);
        // four decks on it: C over A, D over B
        assert_eq!([0, 1, 2, 3].map(|d| deck_pair(d, 4, 4)), [0, 1, 0, 1]);
        assert_eq!([0, 1, 2, 3].map(|d| layer_partner(d, 4, 4)), [Some(2), Some(3), Some(0), Some(1)]);
        // and on one with eight: each its own
        assert_eq!([0, 1, 2, 3].map(|d| deck_pair(d, 4, 8)), [0, 1, 2, 3]);
        assert_eq!(layer_partner(2, 4, 8), None);
    }
}
