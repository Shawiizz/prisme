//! Loudness of a track as broadcasters measure it (ITU-R BS.1770-4): the signal weighted by
//! the K filter (a high shelf, as the head hears, and a high pass), its mean square over
//! 400 ms blocks overlapping by 75 %, the quiet blocks gated out (absolutely below
//! -70 LUFS, then 10 LU below the loudness of the rest).

use prisme_media::TrackAudio;

const BLOCK_S: f64 = 0.4;
/// Blocks start every quarter block.
const STEPS_PER_BLOCK: usize = 4;
const ABSOLUTE_GATE: f64 = -70.0;
const RELATIVE_GATE: f64 = -10.0;

/// How loud a track is, and its loudest sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Loudness {
    /// Integrated loudness (LUFS).
    pub lufs: f64,
    /// Highest absolute sample value.
    pub peak: f32,
}

/// The loudness of a decoded track; `None` for silence.
pub fn loudness(track: &TrackAudio) -> Option<Loudness> {
    let rate = f64::from(track.sample_rate());
    let step = ((BLOCK_S * rate) as usize / STEPS_PER_BLOCK).max(1);
    let mut filters = [KWeighting::new(rate), KWeighting::new(rate)];
    let mut steps: Vec<f64> = Vec::new();
    let (mut sum, mut count, mut peak) = (0.0f64, 0usize, 0.0f32);
    track.for_each_block(1 << 15, |block| {
        for frame in block.chunks_exact(2) {
            for (filter, &x) in filters.iter_mut().zip(frame) {
                let y = filter.process(f64::from(x));
                sum += y * y;
                peak = peak.max(x.abs());
            }
            count += 1;
            if count == step {
                steps.push(sum / step as f64);
                (sum, count) = (0.0, 0);
            }
        }
    });
    let blocks: Vec<f64> =
        steps.windows(STEPS_PER_BLOCK).map(|w| w.iter().sum::<f64>() / w.len() as f64).collect();
    let loud = |power: f64| -0.691 + 10.0 * power.log10();
    let mean = |gate: f64| {
        let kept: Vec<f64> = blocks.iter().copied().filter(|&p| loud(p) > gate).collect();
        (!kept.is_empty()).then(|| kept.iter().sum::<f64>() / kept.len() as f64)
    };
    let relative = loud(mean(ABSOLUTE_GATE)?) + RELATIVE_GATE;
    Some(Loudness { lufs: loud(mean(relative.max(ABSOLUTE_GATE))?), peak })
}

/// The K weighting: two biquads, from their analog prototypes at any rate (as libebur128,
/// the reference implementation, derives them).
struct KWeighting {
    shelf: Biquad,
    high_pass: Biquad,
}

impl KWeighting {
    fn new(rate: f64) -> Self {
        let (f0, gain_db, q) = (1681.974450955533, 3.999843853973347, 0.7071752369554196);
        let k = (std::f64::consts::PI * f0 / rate).tan();
        let vh = 10f64.powf(gain_db / 20.0);
        let vb = vh.powf(0.4996667741545416);
        let a0 = 1.0 + k / q + k * k;
        let shelf = Biquad::new(
            [(vh + vb * k / q + k * k) / a0, 2.0 * (k * k - vh) / a0, (vh - vb * k / q + k * k) / a0],
            [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
        );
        let (f0, q) = (38.13547087602444, 0.5003270373238773);
        let k = (std::f64::consts::PI * f0 / rate).tan();
        let a0 = 1.0 + k / q + k * k;
        let high_pass = Biquad::new([1.0, -2.0, 1.0], [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0]);
        Self { shelf, high_pass }
    }

    #[inline]
    fn process(&mut self, x: f64) -> f64 {
        self.high_pass.process(self.shelf.process(x))
    }
}

/// A direct form I biquad (a0 = 1).
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    x: [f64; 2],
    y: [f64; 2],
}

impl Biquad {
    fn new(b: [f64; 3], a: [f64; 2]) -> Self {
        Self { b, a, x: [0.0; 2], y: [0.0; 2] }
    }

    #[inline]
    fn process(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.b[1] * self.x[0] + self.b[2] * self.x[1]
            - self.a[0] * self.y[0]
            - self.a[1] * self.y[1];
        self.x = [x, self.x[0]];
        self.y = [y, self.y[0]];
        y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rate: u32, seconds: f64, amplitude: f32, both: bool) -> TrackAudio {
        let frames = (seconds * f64::from(rate)) as usize;
        let samples = (0..frames)
            .flat_map(|i| {
                let v = amplitude * (2.0 * std::f32::consts::PI * 997.0 * i as f32 / rate as f32).sin();
                [v, if both { v } else { 0.0 }]
            })
            .collect();
        TrackAudio::from_interleaved(rate, samples)
    }

    /// BS.1770's calibration: a full-scale 997 Hz sine in one channel reads -3.01 LUFS.
    #[test]
    fn a_full_scale_tone_in_one_channel_reads_minus_three() {
        for rate in [44_100, 48_000] {
            let l = loudness(&tone(rate, 5.0, 1.0, false)).unwrap();
            assert!((l.lufs + 3.01).abs() < 0.05, "{rate} Hz: {}", l.lufs);
            assert!((l.peak - 1.0).abs() < 1e-3);
        }
    }

    #[test]
    fn half_the_amplitude_in_both_channels_is_six_db_below_full_scale() {
        let l = loudness(&tone(44_100, 5.0, 0.5, true)).unwrap();
        assert!((l.lufs + 6.02).abs() < 0.05, "{}", l.lufs);
    }

    #[test]
    fn silence_has_no_loudness() {
        assert!(loudness(&TrackAudio::from_interleaved(44_100, vec![0.0; 44_100 * 4])).is_none());
    }
}
