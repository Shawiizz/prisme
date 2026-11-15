//! Stem separation.
//!
//! A RoFormer network estimates a complex mask of each of its stems on the track's STFT: a
//! Mel-Band RoFormer ([Wang et al., 2023]) or a BS-RoFormer ([Lu et al., 2023]), the small
//! Mini-BS-RoFormers among them. What no stem holds is the track minus the stems. The network
//! runs on the GPU when one is usable, else on the CPU ([`cpu`]).
//!
//! [Wang et al., 2023]: https://arxiv.org/abs/2310.01809
//! [Lu et al., 2023]: https://arxiv.org/abs/2309.02612

pub mod cpu;
pub mod gpu;
pub mod model;
pub mod network;
pub mod separate;
pub mod stft;
pub mod track;

pub use cpu::CpuNetwork;
pub use gpu::{Gpu, GpuNetwork, GpuOptions};
pub use model::{Config, Geometry, Values, Weights};
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
