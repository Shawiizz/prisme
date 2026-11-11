//! How present each part of a separated track is along it: the level of each stem, and of
//! what the stems leave (the track minus them), slice by slice.

use prisme_media::TrackAudio;

/// Levels per second of track.
pub const ACTIVITY_SLICES_PER_S: f64 = 4.0;
/// The quietest level told apart from silence (dBFS); 0 dBFS maps to 255.
const FLOOR_DB: f64 = -48.0;

/// The level of each of `stems` along `track`, then of the rest: a row of levels from 0
/// (silent) to 255 (full scale) per part, [`ACTIVITY_SLICES_PER_S`] a second. `None` unless
/// every stem has the track's rate and length and is all there.
pub fn stem_activity(track: &TrackAudio, stems: &[&TrackAudio]) -> Option<Vec<Vec<u8>>> {
    let frames = track.frames();
    let fits = |s: &&TrackAudio| s.sample_rate() == track.sample_rate() && s.frames() == frames;
    if !track.is_complete() || !stems.iter().all(|s| fits(s) && s.is_complete()) {
        return None;
    }
    let slice = (f64::from(track.sample_rate()) / ACTIVITY_SLICES_PER_S).round() as usize;
    let slices = frames.div_ceil(slice);
    let mut rows = vec![Vec::with_capacity(slices); stems.len() + 1];
    let mut power = vec![0.0f64; stems.len() + 1];
    for start in (0..frames).step_by(slice) {
        power.fill(0.0);
        let end = (start + slice).min(frames);
        for i in start..end {
            let (mut rest_l, mut rest_r) = track.frame(i as isize);
            for (stem, sum) in stems.iter().zip(&mut power) {
                let (l, r) = stem.frame(i as isize);
                *sum += f64::from(l * l + r * r);
                rest_l -= l;
                rest_r -= r;
            }
            power[stems.len()] += f64::from(rest_l * rest_l + rest_r * rest_r);
        }
        for (row, sum) in rows.iter_mut().zip(&power) {
            row.push(level(sum / (2 * (end - start)) as f64));
        }
    }
    Some(rows)
}

/// A mean square as a level from 0 to 255, on a dB scale.
fn level(mean_square: f64) -> u8 {
    let db = 10.0 * mean_square.max(1e-12).log10();
    ((db - FLOOR_DB) / -FLOOR_DB * 255.0).clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(rate: u32, seconds: f64, amplitude: impl Fn(f64) -> f32) -> TrackAudio {
        let frames = (seconds * f64::from(rate)) as usize;
        let samples = (0..frames)
            .flat_map(|i| {
                let t = i as f64 / f64::from(rate);
                let v = amplitude(t) * (2.0 * std::f64::consts::PI * 440.0 * t).sin() as f32;
                [v, v]
            })
            .collect();
        TrackAudio::from_interleaved(rate, samples)
    }

    #[test]
    fn each_part_shows_where_it_plays() {
        let rate = 8_000;
        // vocals in the second half only, the rest all along
        let vocals = track(rate, 4.0, |t| if t >= 2.0 { 0.5 } else { 0.0 });
        let rest = track(rate, 4.0, |_| 0.25);
        let mix = TrackAudio::from_interleaved(
            rate,
            (0..vocals.frames())
                .flat_map(|i| {
                    let (v, r) = (vocals.frame(i as isize), rest.frame(i as isize));
                    [v.0 + r.0, v.1 + r.1]
                })
                .collect(),
        );
        let rows = stem_activity(&mix, &[&vocals]).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].len(), 16);
        assert!(rows[0][..8].iter().all(|&v| v == 0), "{:?}", rows[0]);
        assert!(rows[0][8..].iter().all(|&v| v > 150), "{:?}", rows[0]);
        assert!(rows[1].iter().all(|&v| v > 120), "{:?}", rows[1]);
    }

    #[test]
    fn a_stem_of_another_length_has_no_activity() {
        let mix = track(8_000, 2.0, |_| 0.5);
        let short = track(8_000, 1.0, |_| 0.5);
        assert!(stem_activity(&mix, &[&short]).is_none());
    }
}
