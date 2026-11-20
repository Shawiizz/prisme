//! What the screen draws of a waveform: an envelope, its height at each slice the loudness of
//! the slice set against the track's own, coloured slice by slice by the balance of its bands
//! (design/DESIGN.md, Waveform).

use crate::waveform::Waveform;

/// The low band's energy is averaged over this long (s): a slice is shorter than a period of a
/// kick's fundamental, and the energy within it swings with the phase.
const LOW_SMOOTHING_S: f64 = 0.047;
/// The low band counts twice in a slice's level: the kick and the bass carry the groove.
const LOW_WEIGHT: f64 = 2.0;
/// Loudness grows as the amplitude to this power (Stevens' power law).
const LOUDNESS_EXPONENT: f64 = 0.6;
/// The height of the track's median slice, among those with sound.
const MEDIAN_HEIGHT: f64 = 0.35;
/// The share of the track's slices with sound that reach the full height.
const TOP_SHARE: f64 = 0.005;
/// The green of a slice whose mid band is the strongest: a full green outshines the others.
const GREEN: f64 = 230.0;

/// The height of the envelope at each slice of `w`, 0 to 255: its loudness (from the energy of
/// its bands) set against the track's, the median slice at `MEDIAN_HEIGHT` and the loudest
/// `TOP_SHARE` at the top. A dynamic track keeps its shape; a limited master, every peak at the
/// ceiling, unfolds into its kicks. While the track is read, `w` holds what has arrived of it,
/// set against itself.
pub fn heights(w: &Waveform) -> Vec<u8> {
    let n = w.len();
    let radius = (LOW_SMOOTHING_S * f64::from(w.slices_per_second) / 2.0) as usize;
    let low = |k: usize| f64::from(w.low[k]).powi(2);
    let mut loudness = Vec::with_capacity(n);
    // the low band's energy over the slices from i - radius to i + radius
    let mut low_sum: f64 = (0..radius.min(n)).map(low).sum();
    for i in 0..n {
        if i + radius < n {
            low_sum += low(i + radius);
        }
        if i > radius {
            low_sum -= low(i - radius - 1);
        }
        let count = ((i + radius).min(n - 1) - i.saturating_sub(radius) + 1) as f64;
        let energy =
            LOW_WEIGHT * low_sum / count + f64::from(w.mid[i]).powi(2) + f64::from(w.high[i]).powi(2);
        loudness.push(energy.powf(LOUDNESS_EXPONENT / 2.0));
    }
    let mut sounding: Vec<f64> = loudness.iter().copied().filter(|&l| l > 0.0).collect();
    if sounding.is_empty() {
        return vec![0; n];
    }
    sounding.sort_by(f64::total_cmp);
    let median = sounding[sounding.len() / 2];
    let top = sounding[(((1.0 - TOP_SHARE) * sounding.len() as f64) as usize).min(sounding.len() - 1)];
    let height = |l: f64| {
        if l <= median {
            l / median * MEDIAN_HEIGHT
        } else if top > median {
            (MEDIAN_HEIGHT + (l - median) / (top - median) * (1.0 - MEDIAN_HEIGHT)).min(1.0)
        } else {
            1.0
        }
    };
    loudness.into_iter().map(|l| (height(l) * 255.0).round() as u8).collect()
}

/// The colour of slice `i` of `w`: the bands mixed like light, the low band lighting the red,
/// the mid the green, the high the blue, scaled to the strongest so that the colours stay vivid
/// whatever the level.
pub fn colour(w: &Waveform, i: usize) -> [u8; 3] {
    let (low, mid, high) = (f64::from(w.low[i]), f64::from(w.mid[i]), f64::from(w.high[i]));
    let max = low.max(mid).max(high).max(1.0);
    [low / max * 255.0, mid / max * GREEN, high / max * 255.0].map(|c| c.round() as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPS: u32 = 150;

    /// `seconds` of four-to-the-floor at 150 BPM: a kick of 60 ms on each beat over a steady mid
    /// and high band.
    fn track(seconds: usize, level: impl Fn(f64) -> [f64; 3]) -> Waveform {
        let mut w = Waveform::new(SPS);
        for i in 0..seconds * SPS as usize {
            let [low, mid, high] = level(i as f64 / f64::from(SPS)).map(|v| v.round() as u8);
            w.peak.push(250);
            w.low.push(low);
            w.mid.push(mid);
            w.high.push(high);
        }
        w
    }

    fn on_kick(s: f64) -> bool {
        s % 0.4 < 0.06
    }

    fn kick_slice(beat: usize) -> usize {
        ((beat as f64 * 0.4 + 0.03) * f64::from(SPS)).round() as usize
    }

    fn offbeat_slice(beat: usize) -> usize {
        ((beat as f64 * 0.4 + 0.2) * f64::from(SPS)).round() as usize
    }

    fn share(height: u8) -> f64 {
        f64::from(height) / 255.0
    }

    fn groove(gain: f64) -> impl Fn(f64) -> [f64; 3] {
        move |s| [if on_kick(s) { 200.0 } else { 60.0 }, 90.0, 40.0].map(|v| (v * gain).round())
    }

    #[test]
    fn a_limited_master_unfolds_into_its_kicks() {
        let h = heights(&track(60, |s| [if on_kick(s) { 240.0 } else { 150.0 }, 170.0, 110.0]));
        for beat in [10, 50, 120] {
            assert!(share(h[kick_slice(beat)]) > 0.95);
            assert!(share(h[offbeat_slice(beat)]) < 0.45);
        }
    }

    #[test]
    fn the_contrast_is_the_tracks_not_its_level() {
        let (loud, quiet) = (heights(&track(30, groove(1.0))), heights(&track(30, groove(0.5))));
        for i in [kick_slice(20), offbeat_slice(20)] {
            assert!((share(quiet[i]) - share(loud[i])).abs() < 0.05);
        }
    }

    #[test]
    fn a_breakdown_stays_below_the_body_and_silence_flat() {
        let h = heights(&track(60, |s| {
            if s < 1.0 {
                [0.0; 3]
            } else if s < 40.0 {
                [if on_kick(s) { 220.0 } else { 90.0 }, 120.0, 70.0]
            } else {
                [5.0, 40.0, 30.0]
            }
        }));
        assert_eq!(h[(0.5 * f64::from(SPS)) as usize], 0);
        assert!(share(h[50 * SPS as usize]) < 0.3);
        assert!(share(h[kick_slice(30)]) > 0.95);
        assert!(heights(&track(2, |_| [0.0; 3])).iter().all(|&v| v == 0));
    }

    #[test]
    fn while_the_track_is_read_what_arrived_is_set_against_itself() {
        let whole = heights(&track(30, groove(1.0)));
        let mut arriving = Waveform::new(SPS);
        arriving.extend_from(&track(30, groove(1.0)), 0..10 * SPS as usize);
        let h = heights(&arriving);
        for i in [kick_slice(5), offbeat_slice(5)] {
            assert!((share(h[i]) - share(whole[i])).abs() < 0.05);
        }
    }

    #[test]
    fn a_slice_is_coloured_by_its_strongest_band() {
        let mut w = Waveform::new(SPS);
        for [low, mid, high] in [[200, 100, 50], [0, 80, 0], [0, 0, 0]] {
            w.peak.push(0);
            w.low.push(low);
            w.mid.push(mid);
            w.high.push(high);
        }
        assert_eq!(colour(&w, 0), [255, 115, 64]);
        assert_eq!(colour(&w, 1), [0, 230, 0]);
        assert_eq!(colour(&w, 2), [0, 0, 0]);
    }
}
