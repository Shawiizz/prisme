//! Sound card input and output.
//!
//! The rest of Prisme only sees [`AudioCallback`]: one call per audio period with the
//! interleaved input (turntables) and output (main mix) buffers. This crate opens the
//! devices and calls it. The current backend is `cpal` (WASAPI on Windows); an ASIO
//! backend can be added behind the same interface.

mod devices;
mod stream;

pub use devices::{list_devices, DeviceInfo, DeviceKind};
pub use stream::{start, RunningStream, StreamSettings, StreamStats};

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
    #[error("unsupported sample format {0:?} (32-bit float or 16-bit integer expected)")]
    UnsupportedFormat(cpal::SampleFormat),
    #[error("the input runs at {input} Hz but the output at {output} Hz; set both devices to the same rate")]
    SampleRateMismatch { input: u32, output: u32 },
    #[error(transparent)]
    Devices(#[from] cpal::DevicesError),
    #[error(transparent)]
    DefaultConfig(#[from] cpal::DefaultStreamConfigError),
    #[error(transparent)]
    Build(#[from] cpal::BuildStreamError),
    #[error(transparent)]
    Play(#[from] cpal::PlayStreamError),
    #[error(transparent)]
    Name(#[from] cpal::DeviceNameError),
}
