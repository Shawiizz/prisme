//! A decimated copy of each turntable's signal, for drawing it (the XY "circle" that
//! shows how clean the timecode is). The audio thread only pushes into a lock-free ring;
//! when nobody reads, the ring is full and the samples are simply not kept.

/// One point every this many input frames: plenty for a picture, cheap to send.
const DECIMATION: usize = 4;
const CAPACITY: usize = 8192;

pub(crate) struct ScopeTap {
    points: rtrb::Producer<[f32; 2]>,
    phase: usize,
}

impl ScopeTap {
    /// Keeps every [`DECIMATION`]-th frame of interleaved stereo `signal`.
    pub(crate) fn push(&mut self, signal: &[f32]) {
        for frame in signal.chunks_exact(2) {
            if self.phase == 0 {
                let _ = self.points.push([frame[0], frame[1]]);
            }
            self.phase = (self.phase + 1) % DECIMATION;
        }
    }
}

pub(crate) fn scope() -> (ScopeTap, rtrb::Consumer<[f32; 2]>) {
    let (points, reader) = rtrb::RingBuffer::new(CAPACITY);
    (ScopeTap { points, phase: 0 }, reader)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_one_frame_in_four() {
        let (mut tap, mut reader) = scope();
        let signal: Vec<f32> = (0..16).map(|i| i as f32).collect();
        tap.push(&signal);
        let points: Vec<[f32; 2]> = std::iter::from_fn(|| reader.pop().ok()).collect();
        assert_eq!(points, vec![[0.0, 1.0], [8.0, 9.0]]);
    }
}
