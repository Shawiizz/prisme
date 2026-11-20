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

/// The share the track's scale may move by before the heights already set against it are set
/// again: a waveform read meanwhile is not sent whole at each piece.
const RESCALE: f64 = 0.02;
/// The scale is looked at again once the slices known grew by this share: it settles as the
/// track is read, and looking at it costs a pass over them all.
const RESCALE_AFTER: usize = 50;

/// The height of the envelope at each slice of `w`, 0 to 255: its loudness (from the energy of
/// its bands) set against the track's, the median slice at `MEDIAN_HEIGHT` and the loudest
/// `TOP_SHARE` at the top. A dynamic track keeps its shape; a limited master, every peak at the
/// ceiling, unfolds into its kicks.
pub fn heights(w: &Waveform) -> Vec<u8> {
    let mut envelope = Envelope::default();
    envelope.grow(w);
    envelope.heights
}

/// The envelope of a waveform as it is read (see [`heights`]): what arrived of the track set
/// against itself, its heights set again only when its scale moved by more than [`RESCALE`].
#[derive(Debug, Clone, Default)]
pub struct Envelope {
    loudness: Vec<f64>,
    scale: Scale,
    /// The slices known when the scale was last looked at.
    scaled_at: usize,
    heights: Vec<u8>,
}

impl Envelope {
    /// The slices of `w` that arrived since, followed: the first slice whose height changed (the
    /// last ones smoothed with those that came after them; all of them when the scale moved).
    pub fn grow(&mut self, w: &Waveform) -> usize {
        let n = w.len();
        let from = self.loudness.len().saturating_sub(radius(w)).min(n);
        self.loudness.truncate(from);
        self.loudness.extend(loudness(w, from..n));
        if self.scaled_at == 0 || n >= self.scaled_at + self.scaled_at / RESCALE_AFTER {
            self.scaled_at = n;
            let scale = Scale::of(&self.loudness);
            if scale.moved_from(&self.scale) {
                self.scale = scale;
                self.heights.clear();
            }
        }
        let from = from.min(self.heights.len());
        self.heights.truncate(from);
        self.heights.extend(self.loudness[from..].iter().map(|&l| self.scale.height(l)));
        from
    }

    /// The height of each slice known.
    pub fn heights(&self) -> &[u8] {
        &self.heights
    }
}

/// The half width of the low band's smoothing, in slices.
fn radius(w: &Waveform) -> usize {
    (LOW_SMOOTHING_S * f64::from(w.slices_per_second) / 2.0) as usize
}

/// The loudness of the slices `range` of `w`: the energy of their bands, the low band's
/// smoothed over the slices around.
fn loudness(w: &Waveform, range: std::ops::Range<usize>) -> impl Iterator<Item = f64> + '_ {
    let (n, radius) = (w.len(), radius(w));
    let low = |k: usize| f64::from(w.low[k]).powi(2);
    // the low band's energy over the slices from i - radius to i + radius
    let behind = if range.start > radius { range.start - radius - 1 } else { 0 };
    let mut low_sum: f64 = (behind..(range.start + radius).min(n)).map(low).sum();
    range.map(move |i| {
        if i + radius < n {
            low_sum += low(i + radius);
        }
        if i > radius {
            low_sum -= low(i - radius - 1);
        }
        let count = ((i + radius).min(n - 1) - i.saturating_sub(radius) + 1) as f64;
        let energy =
            LOW_WEIGHT * low_sum / count + f64::from(w.mid[i]).powi(2) + f64::from(w.high[i]).powi(2);
        energy.powf(LOUDNESS_EXPONENT / 2.0)
    })
}

/// What a track's loudness is set against: its median slice with sound, and the loudest
/// `TOP_SHARE` of them; none for a silent track.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Scale {
    median: f64,
    top: f64,
}

impl Scale {
    fn of(loudness: &[f64]) -> Self {
        let mut sounding: Vec<f64> = loudness.iter().copied().filter(|&l| l > 0.0).collect();
        let Some(last) = sounding.len().checked_sub(1) else { return Self::default() };
        let (median, top) =
            (sounding.len() / 2, (((1.0 - TOP_SHARE) * sounding.len() as f64) as usize).min(last));
        let (below, &mut top, _) = sounding.select_nth_unstable_by(top, f64::total_cmp);
        let median =
            if median < below.len() { *below.select_nth_unstable_by(median, f64::total_cmp).1 } else { top };
        Self { median, top }
    }

    /// Whether the heights set against `before` are too far from those set against this.
    fn moved_from(&self, before: &Scale) -> bool {
        let moved = |now: f64, was: f64| (now - was).abs() > RESCALE * was;
        before.median == 0.0 || moved(self.median, before.median) || moved(self.top, before.top)
    }

    /// The height of a slice as loud as `l`, 0 to 255.
    fn height(&self, l: f64) -> u8 {
        let Scale { median, top } = *self;
        let height = if median == 0.0 {
            0.0
        } else if l <= median {
            l / median * MEDIAN_HEIGHT
        } else if top > median {
            (MEDIAN_HEIGHT + (l - median) / (top - median) * (1.0 - MEDIAN_HEIGHT)).min(1.0)
        } else {
            1.0
        };
        (height * 255.0).round() as u8
    }
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
    fn a_waveform_read_piece_by_piece_is_sent_from_where_it_changed() {
        let whole = track(60, groove(1.0));
        let mut arriving = Waveform::new(SPS);
        let mut envelope = Envelope::default();
        let piece = SPS as usize / 4;
        let mut froms = Vec::new();
        for start in (0..whole.len()).step_by(piece) {
            arriving.extend_from(&whole, start..(start + piece).min(whole.len()));
            froms.push(envelope.grow(&arriving));
        }
        let radius = radius(&whole);
        let resent = froms.iter().filter(|&&from| from == 0).count();
        assert!(resent < froms.len() / 10, "{resent} of {} pieces set it all again", froms.len());
        assert!(froms.iter().skip(1).all(|&from| from == 0 || from + radius >= piece), "{froms:?}");
        let h = heights(&whole);
        let close = envelope.heights().iter().zip(&h).all(|(a, b)| a.abs_diff(*b) <= 8);
        assert!(close && envelope.heights().len() == h.len(), "within the scale's leeway");
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
