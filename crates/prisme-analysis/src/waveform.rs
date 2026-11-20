//! Waveform summary: for each time slice, the peak amplitude and the energy of the low,
//! mid and high bands, from which the height and the colour drawn are made
//! ([`crate::envelope`]). Each slice depends on the audio before it only, so the waveform is
//! computed as the track is decoded and drawn as it comes ([`WaveformBuilder`]).

use prisme_media::TrackAudio;
use std::f64::consts::TAU;

/// Crossover frequencies between the bands (Hz).
const LOW_MID_HZ: f64 = 200.0;
const MID_HIGH_HZ: f64 = 2_000.0;
/// Waveform summary of a track, one byte per slice in each band: the peak, and the RMS of
/// each band, on the scale of the full-scale signal (255). A fixed scale, not the track's
/// loudest, so that each slice is known as soon as it is read.
#[derive(Debug, Clone, PartialEq)]
pub struct Waveform {
    pub slices_per_second: u32,
    pub peak: Vec<u8>,
    pub low: Vec<u8>,
    pub mid: Vec<u8>,
    pub high: Vec<u8>,
}

impl Waveform {
    pub fn new(slices_per_second: u32) -> Self {
        Self { slices_per_second, peak: Vec::new(), low: Vec::new(), mid: Vec::new(), high: Vec::new() }
    }

    pub fn len(&self) -> usize {
        self.peak.len()
    }

    pub fn is_empty(&self) -> bool {
        self.peak.is_empty()
    }

    /// Appends the slices `range` of `other`.
    pub fn extend_from(&mut self, other: &Waveform, range: std::ops::Range<usize>) {
        self.peak.extend_from_slice(&other.peak[range.clone()]);
        self.low.extend_from_slice(&other.low[range.clone()]);
        self.mid.extend_from_slice(&other.mid[range.clone()]);
        self.high.extend_from_slice(&other.high[range]);
    }
}

/// Two cascaded one-pole low-pass filters (12 dB/octave).
struct LowPass {
    a: f64,
    s1: f64,
    s2: f64,
}

impl LowPass {
    fn new(cutoff_hz: f64, sample_rate: f64) -> Self {
        Self { a: 1.0 - (-TAU * cutoff_hz / sample_rate).exp(), s1: 0.0, s2: 0.0 }
    }
    #[inline]
    fn process(&mut self, x: f64) -> f64 {
        self.s1 += self.a * (x - self.s1);
        self.s2 += self.a * (self.s1 - self.s2);
        self.s2
    }
}

/// Computes a waveform from the start of a track as it is decoded: [`Self::read`] takes the
/// frames decoded since, and the slices they complete are [`Self::waveform`] at once.
pub struct WaveformBuilder {
    frames_per_slice: f64,
    low_pass: LowPass,
    mid_pass: LowPass,
    /// Frames read so far.
    frames: usize,
    /// The slice being read: its peak, the energy of its bands, its frames.
    peak: f64,
    bands: [f64; 3],
    count: u32,
    waveform: Waveform,
}

impl WaveformBuilder {
    pub fn new(sample_rate: u32, slices_per_second: u32) -> Self {
        let sr = sample_rate as f64;
        Self {
            frames_per_slice: (sr / slices_per_second as f64).max(1.0),
            low_pass: LowPass::new(LOW_MID_HZ, sr),
            mid_pass: LowPass::new(MID_HIGH_HZ, sr),
            frames: 0,
            peak: 0.0,
            bands: [0.0; 3],
            count: 0,
            waveform: Waveform::new(slices_per_second),
        }
    }

    /// The slices of a track of `frames` frames.
    pub fn slices_of(&self, frames: usize) -> usize {
        (frames as f64 / self.frames_per_slice).ceil() as usize
    }

    /// Reads the frames of `track` decoded since the last call.
    pub fn read(&mut self, track: &TrackAudio) {
        let end = track.readable().end;
        // taken after the end: it holds every frame up to it
        let view = track.view();
        for i in self.frames..end {
            let (l, r) = view.frame(i as isize);
            self.push((l as f64 + r as f64) * 0.5);
        }
    }

    #[inline]
    fn push(&mut self, x: f64) {
        let slice = (self.frames as f64 / self.frames_per_slice) as usize;
        if slice > self.waveform.len() {
            self.close_slice();
        }
        let low = self.low_pass.process(x);
        let below_high = self.mid_pass.process(x);
        self.peak = self.peak.max(x.abs());
        self.bands[0] += low * low;
        self.bands[1] += (below_high - low) * (below_high - low);
        self.bands[2] += (x - below_high) * (x - below_high);
        self.count += 1;
        self.frames += 1;
    }

    fn close_slice(&mut self) {
        let n = self.count as f64;
        let [low, mid, high] = self.bands.map(|e| (e / n).sqrt());
        let level = |v: f64| (v.min(1.0) * 255.0).round() as u8;
        let w = &mut self.waveform;
        w.peak.push(level(self.peak));
        w.low.push(level(low));
        w.mid.push(level(mid));
        w.high.push(level(high));
        (self.peak, self.bands, self.count) = (0.0, [0.0; 3], 0);
    }

    /// The slices complete so far.
    pub fn waveform(&self) -> &Waveform {
        &self.waveform
    }

    /// The whole waveform, once the track is all read.
    pub fn finish(mut self) -> Waveform {
        if self.count > 0 {
            self.close_slice();
        }
        self.waveform
    }
}

/// Computes the waveform summary of `track` with `slices_per_second` slices per second.
pub fn compute_waveform(track: &TrackAudio, slices_per_second: u32) -> Waveform {
    let mut builder = WaveformBuilder::new(track.sample_rate(), slices_per_second);
    builder.read(track);
    builder.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use prisme_media::PENDING_BLOCK;

    fn sine_samples(freq: f64, seconds: f64) -> Vec<f32> {
        let sr = 44_100.0;
        let n = (sr * seconds) as usize;
        (0..n)
            .flat_map(|i| {
                let v = (TAU * freq * i as f64 / sr).sin() as f32 * 0.5;
                [v, v]
            })
            .collect()
    }

    fn sine(freq: f64, seconds: f64) -> TrackAudio {
        TrackAudio::from_interleaved(44_100, sine_samples(freq, seconds))
    }

    #[test]
    fn slice_count_follows_the_duration_and_the_peak_the_level() {
        let w = compute_waveform(&sine(440.0, 2.0), 100);
        assert_eq!(w.len(), 200);
        assert_eq!(w.peak.iter().max(), Some(&127), "half of full scale");
    }

    #[test]
    fn bands_reflect_the_frequency() {
        let dominant = |f: f64| {
            let w = compute_waveform(&sine(f, 1.0), 50);
            let i = w.len() / 2;
            let b = [w.low[i], w.mid[i], w.high[i]];
            (0..3).max_by_key(|&k| b[k]).unwrap()
        };
        assert_eq!(dominant(60.0), 0, "bass");
        assert_eq!(dominant(700.0), 1, "mids");
        assert_eq!(dominant(8_000.0), 2, "highs");
    }

    #[test]
    fn bands_keep_the_level_of_the_sound() {
        let level = |gain: f32| {
            let samples = sine_samples(700.0, 1.0).into_iter().map(|v| v * gain).collect();
            let w = compute_waveform(&TrackAudio::from_interleaved(44_100, samples), 50);
            w.mid[w.len() / 2]
        };
        let (full, half) = (level(1.0), level(0.5));
        assert!(full > 64, "a sine at half scale, most of it in its band: {full}");
        assert!(full.abs_diff(2 * half) <= 1, "6 dB lower, half as high: {full} {half}");
    }

    #[test]
    fn silence_has_no_level_and_no_colour() {
        let w = compute_waveform(&TrackAudio::from_interleaved(44_100, vec![0.0; 2 * 44_100]), 10);
        assert!(w.peak.iter().chain(&w.low).chain(&w.mid).chain(&w.high).all(|&v| v == 0));
    }

    #[test]
    fn read_as_it_is_decoded_it_is_the_waveform_of_the_whole() {
        let samples = sine_samples(700.0, 1.5);
        let frames = samples.len() / 2;
        let track = TrackAudio::pending(44_100, frames);
        let mut builder = WaveformBuilder::new(44_100, 150);
        let mut grown = Vec::new();
        for start in (0..frames).step_by(5 * PENDING_BLOCK) {
            let end = (start + 5 * PENDING_BLOCK).min(frames);
            track.publish(start, &samples[start * 2..end * 2]);
            builder.read(&track);
            grown.push(builder.waveform().len());
        }
        assert!(grown.windows(2).all(|w| w[0] < w[1]), "slices come as the track is read: {grown:?}");
        assert_eq!(builder.slices_of(frames), 225);
        assert_eq!(builder.finish(), compute_waveform(&sine(700.0, 1.5), 150));
    }
}
