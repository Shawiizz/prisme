//! Sound card input and output.
//!
//! The rest of Prisme only sees [`AudioCallback`]: one call per audio period with the
//! interleaved input (turntables) and output buffers. This crate opens the devices of
//! the chosen [`Backend`] (WASAPI, or ASIO for the lowest latency) and calls it.

mod devices;
mod formats;
pub mod latency;
mod stream;

pub use devices::{is_available, list_devices, Backend, DeviceInfo, DeviceKind};
pub use stream::{start, RunningStream, StreamFormat, StreamSettings, StreamStats};

/// Called on the audio thread once per period. Must be real-time safe.
pub trait AudioCallback: Send + 'static {
    /// `input`: interleaved samples from the input device (`input_channels` per frame),
    /// possibly shorter than the output if the input is late (pad with silence).
    /// `output`: interleaved buffer to fill (`output_channels` per frame).
    fn process(&mut self, input: &[f32], input_channels: usize, output: &mut [f32], output_channels: usize);
}

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("no audio output device found")]
    NoOutputDevice,
    #[error("audio device not found: {0}")]
    DeviceNotFound(String),
    #[error("{0} is not available on this computer")]
    BackendUnavailable(Backend),
    #[error("unsupported sample format {0:?}")]
    UnsupportedFormat(cpal::SampleFormat),
    #[error("the input runs at {input} Hz but the output at {output} Hz; set both devices to the same rate")]
    SampleRateMismatch { input: u32, output: u32 },
    #[error(transparent)]
    Backend(#[from] cpal::Error),
}
