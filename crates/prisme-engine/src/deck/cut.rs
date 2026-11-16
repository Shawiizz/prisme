//! Fader cuts: while a pad is held, the deck chopped in time with its beats, as a hand
//! flicking the channel fader would, on the grid to the sample.

use prisme_dsp::Smoothed;

/// The edges of a cut: tight, without a click (s).
const EDGE_S: f32 = 0.001;

pub(crate) struct Cut {
    /// The period of the cut in beats while held: open its first half, closed the second.
    beats: Option<f64>,
    gain: Smoothed,
    /// The beat at the first frame of the block, and how far a frame moves it; `None`
    /// without a beat grid, when nothing is cut.
    grid: Option<(f64, f64)>,
}

impl Cut {
    pub(crate) fn new(sample_rate: f32) -> Self {
        Self { beats: None, gain: Smoothed::new(1.0, EDGE_S, sample_rate), grid: None }
    }

    /// Cuts on every `beats` while `Some`, or lets the deck through.
    pub(crate) fn set(&mut self, beats: Option<f64>) {
        self.beats = beats.filter(|b| b.is_finite() && *b > 0.0);
    }

    /// Where the beats are over the block to be gated: from `from`, `per_frame` a frame.
    pub(crate) fn follow(&mut self, grid: Option<(f64, f64)>) {
        self.grid = grid;
    }

    /// Gates one block, interleaved stereo.
    pub(crate) fn gate(&mut self, out: &mut [f32]) {
        let cut = self.beats.zip(self.grid);
        if cut.is_none() && self.gain.is_settled() && self.gain.target() == 1.0 {
            return;
        }
        for (n, frame) in out.chunks_exact_mut(2).enumerate() {
            let open = cut.is_none_or(|(period, (from, per_frame))| {
                ((from + n as f64 * per_frame) / period).rem_euclid(1.0) < 0.5
            });
            self.gain.set(if open { 1.0 } else { 0.0 });
            let gain = self.gain.tick();
            frame[0] *= gain;
            frame[1] *= gain;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 48_000.0;

    /// A block of `frames` at 1, gated with beats `per_frame` from `from`.
    fn gated(cut: &mut Cut, from: f64, per_frame: f64, frames: usize) -> Vec<f32> {
        let mut out = vec![1.0; frames * 2];
        cut.follow(Some((from, per_frame)));
        cut.gate(&mut out);
        out.iter().step_by(2).copied().collect()
    }

    #[test]
    fn half_a_beat_cut_opens_on_the_beat_and_closes_on_its_middle() {
        let mut cut = Cut::new(RATE);
        cut.set(Some(0.5));
        // a beat every 480 frames: open 120 frames, closed 120
        let out = gated(&mut cut, 0.0, 1.0 / 480.0, 480);
        assert_eq!(out[100], 1.0);
        assert!(out[121] > 0.5 && out[130] < 0.5, "an edge, no click: {}", out[121]);
        assert!(out[200] < 1e-3);
        assert!(out[250] > 0.5 && out[350] > 0.999, "open again on the half beat");
        assert!(out[460] < 1e-3);
    }

    #[test]
    fn the_beat_goes_on_from_block_to_block() {
        let mut cut = Cut::new(RATE);
        cut.set(Some(0.25));
        // a beat every 4800 frames: a quarter of a beat is 1200
        let first = gated(&mut cut, 0.0, 1.0 / 4800.0, 64);
        let next = gated(&mut cut, 0.2, 1.0 / 4800.0, 200);
        assert_eq!(first[30], 1.0);
        assert!(next[150] < 1e-3, "0.2 of a beat in: in the closed half of the first quarter");
    }

    #[test]
    fn released_or_without_a_grid_the_deck_goes_through() {
        let mut cut = Cut::new(RATE);
        cut.set(Some(0.5));
        gated(&mut cut, 0.3, 0.0, 256);
        cut.set(None);
        let out = gated(&mut cut, 0.3, 0.0, 512);
        assert!(out[511] > 0.999, "back up after its edge");
        cut.set(Some(0.5));
        let mut out = vec![1.0; 512];
        cut.follow(None);
        cut.gate(&mut out);
        assert!(out.iter().all(|&s| s == 1.0));
    }
}
