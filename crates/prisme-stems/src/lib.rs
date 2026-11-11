//! Stem separation.
//!
//! A Mel-Band RoFormer network ([Wang et al., 2023]) estimates a complex mask of each of its
//! stems (the vocals, or the drums and the bass) on the track's STFT; what no stem holds is
//! the track minus the stems. The network runs on the GPU when one is usable, else on the
//! CPU ([`cpu`]).
//!
//! [Wang et al., 2023]: https://arxiv.org/abs/2310.01809

pub mod cpu;
pub mod gpu;
pub mod model;
pub mod network;
pub mod separate;
pub mod stft;
pub mod track;

pub use cpu::CpuNetwork;
pub use gpu::{Gpu, GpuNetwork, GpuOptions};
pub use model::{Config, Geometry, Weights};
pub use network::MaskNetwork;
pub use separate::{Separation, Separator};

#[derive(Debug, thiserror::Error)]
pub enum StemsError {
    #[error("invalid model file: {0}")]
    Model(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("invalid model file: {0}")]
    Tensors(#[from] safetensors::SafeTensorError),
    #[error("no usable GPU: {0}")]
    NoGpu(String),
    #[error("GPU: {0}")]
    Gpu(String),
    #[error("{0}")]
    Track(String),
    #[error("cancelled")]
    Cancelled,
}
