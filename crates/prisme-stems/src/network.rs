//! What the separation needs from an implementation of the network.

use crate::model::Geometry;
use crate::StemsError;

/// The network between the STFT and the iSTFT: gathered bins in, a complex mask per bin out.
pub trait MaskNetwork: Send {
    fn geometry(&self) -> &Geometry;

    /// `bins` and `out` hold `frames` rows of [`Geometry::features`] values: for each gathered
    /// bin, its real then imaginary part.
    fn masks(&mut self, bins: &[f32], frames: usize, out: &mut [f32]) -> Result<(), StemsError>;
}

/// The axis a transformer attends along. Tokens are stored frame by frame, then band by band.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Axis {
    /// Along the frames, within each band.
    Time,
    /// Along the bands, within each frame.
    Freq,
}

impl Axis {
    /// Length of a sequence.
    pub(crate) fn len(self, frames: usize, bands: usize) -> usize {
        match self {
            Axis::Time => frames,
            Axis::Freq => bands,
        }
    }

    /// Number of independent sequences.
    pub(crate) fn groups(self, frames: usize, bands: usize) -> usize {
        match self {
            Axis::Time => bands,
            Axis::Freq => frames,
        }
    }

    /// Token of the `i`-th element of sequence `group`.
    pub(crate) fn token(self, group: usize, i: usize, bands: usize) -> usize {
        match self {
            Axis::Time => i * bands + group,
            Axis::Freq => group * bands + i,
        }
    }

    /// Tokens between two consecutive elements of a sequence.
    pub(crate) fn stride(self, bands: usize) -> usize {
        match self {
            Axis::Time => bands,
            Axis::Freq => 1,
        }
    }

    /// Position of a token in its sequence.
    pub(crate) fn position(self, token: usize, bands: usize) -> usize {
        match self {
            Axis::Time => token / bands,
            Axis::Freq => token % bands,
        }
    }
}
