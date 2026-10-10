//! Recording: the engine copies what it records into a lock-free queue that a writer on
//! the control side empties to a file. If the writer falls behind, the samples that do
//! not fit are counted, never waited for.

use crate::command::RecordSource;

pub(crate) struct Recording {
    pub(crate) source: RecordSource,
    pub(crate) sink: rtrb::Producer<f32>,
    pub(crate) frames: u64,
    pub(crate) dropped: u64,
}

impl Recording {
    /// Queues interleaved stereo.
    pub(crate) fn push(&mut self, stereo: &[f32]) {
        let frames = stereo.len() / 2;
        let fits = (self.sink.slots() / 2).min(frames);
        if let Ok(chunk) = self.sink.write_chunk_uninit(fits * 2) {
            chunk.fill_from_iter(stereo.iter().copied());
        }
        self.frames += frames as u64;
        self.dropped += (frames - fits) as u64;
    }
}

/// Copies the stereo pair starting at input channel `first` (silence if absent).
pub(crate) fn input_pair(input: &[f32], channels: usize, offset: usize, first: usize, out: &mut [f32]) {
    for (i, frame) in out.chunks_exact_mut(2).enumerate() {
        let k = (offset + i) * channels + first;
        let present = first + 1 < channels && k + 1 < input.len();
        frame[0] = if present { input[k] } else { 0.0 };
        frame[1] = if present { input[k + 1] } else { 0.0 };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_does_not_fit_is_counted_not_waited_for() {
        let (sink, mut written) = rtrb::RingBuffer::new(6);
        let mut r = Recording { source: RecordSource::Master, sink, frames: 0, dropped: 0 };
        r.push(&[0.1, 0.2, 0.3, 0.4]);
        r.push(&[0.5, 0.6, 0.7, 0.8]);
        assert_eq!((r.frames, r.dropped), (4, 1));
        let got: Vec<f32> = std::iter::from_fn(|| written.pop().ok()).collect();
        assert_eq!(got, [0.1, 0.2, 0.3, 0.4, 0.5, 0.6]);
    }

    #[test]
    fn an_input_pair_is_taken_out_of_the_interleaved_input() {
        let input = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let mut out = [0.0; 4];
        input_pair(&input, 4, 0, 2, &mut out);
        assert_eq!(out, [3.0, 4.0, 7.0, 8.0]);
        input_pair(&input, 4, 0, 3, &mut out);
        assert_eq!(out, [0.0; 4], "a pair cut by the channel count is silence");
    }
}
