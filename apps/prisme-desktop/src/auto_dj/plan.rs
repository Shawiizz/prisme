//! How the Auto DJ mixes one track into the next: where the mix starts on the track playing,
//! where the next one starts, at what speed, and over how long.

use prisme_engine::BeatGrid;

/// Beats of a phrase, counted from the first beat of the grid: a mix starts on one.
pub const PHRASE_BEATS: f64 = 32.0;
/// A tempo further than this from the next track's own is not matched: it plays at its own.
pub const MAX_TEMPO_CHANGE: f64 = 0.1;
/// Quieter than this on the waveform's scale (255: full scale; this is about -36 dBFS),
/// a slice of a track is silence.
const SILENCE: u8 = 4;
/// Slower than this, a deck is taken to play at its own speed (it is about to start).
pub const MIN_SPEED: f64 = 0.05;

/// How long a mix lasts: in beats when both tracks have a beat grid, else in seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FadeLength {
    pub beats: u32,
    pub seconds: f64,
}

/// Where a track is heard: its silent start and tail left out (seconds of the track).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Audible {
    pub start_s: f64,
    pub end_s: f64,
}

/// Where a track is heard, from the peaks of its waveform; `None` for a silent track.
pub fn audible(peaks: &[u8], slices_per_second: u32) -> Option<Audible> {
    let first = peaks.iter().position(|&p| p > SILENCE)?;
    let last = peaks.iter().rposition(|&p| p > SILENCE)?;
    let per_s = f64::from(slices_per_second.max(1));
    Some(Audible { start_s: first as f64 / per_s, end_s: (last + 1) as f64 / per_s })
}

/// The track playing, as the mix needs it.
#[derive(Debug, Clone, Copy)]
pub struct Outgoing {
    pub grid: Option<BeatGrid>,
    /// Its tempo now, the speed of the deck included.
    pub bpm: Option<f64>,
    /// Where it stops being heard.
    pub end_s: f64,
    pub speed: f64,
}

/// The track coming in.
#[derive(Debug, Clone, Copy)]
pub struct Incoming {
    pub grid: Option<BeatGrid>,
    /// Its main cue, where the DJ wants it to start.
    pub cue_s: Option<f64>,
    /// Where it starts being heard.
    pub audible_start_s: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MixPlan {
    /// Where on the track playing the mix starts.
    pub at_s: f64,
    /// Where the next track starts.
    pub from_s: f64,
    /// The speed the next track plays at: the tempo of the one playing, or its own.
    pub rate: f64,
    /// Both tracks play at the same tempo, their beats together.
    pub synced: bool,
    /// How long the crossfader takes to go over, in seconds of the next track.
    pub length_s: f64,
}

/// Where the next track starts: its main cue, else where it is heard, on the beat if it has a
/// grid.
pub fn start_of(incoming: &Incoming) -> f64 {
    incoming.cue_s.unwrap_or_else(|| match incoming.grid {
        Some(grid) => beat_from(&grid, incoming.audible_start_s),
        None => incoming.audible_start_s,
    })
}

/// The mix of `incoming` into `outgoing`. With a beat grid on both and tempos close enough, the
/// next track takes the tempo of the one playing and comes in on the last phrase that leaves
/// room for the whole fade before the end of the track playing; otherwise it plays at its own
/// tempo, the fade ending where the track playing goes silent.
pub fn plan(outgoing: &Outgoing, incoming: &Incoming, fade: FadeLength) -> MixPlan {
    let from_s = start_of(incoming);
    let matched = match (outgoing.grid, outgoing.bpm, incoming.grid) {
        (Some(out_grid), Some(bpm), Some(in_grid)) => {
            matched_rate(bpm, in_grid.bpm).map(|rate| (out_grid, bpm, rate))
        }
        _ => None,
    };
    if let Some((grid, bpm, rate)) = matched {
        let beats = f64::from(fade.beats);
        let latest_s = outgoing.end_s - beats * grid.beat_period_s();
        return MixPlan {
            at_s: phrase_before(&grid, latest_s).max(0.0),
            from_s,
            rate,
            synced: true,
            // the beats of the track playing, at the speed the next one plays
            length_s: beats * 60.0 / bpm * rate,
        };
    }
    let speed = if outgoing.speed.abs() > MIN_SPEED { outgoing.speed.abs() } else { 1.0 };
    MixPlan {
        at_s: (outgoing.end_s - fade.seconds * speed).max(0.0),
        from_s,
        rate: 1.0,
        synced: false,
        length_s: fade.seconds,
    }
}

/// The speed giving a track of `own_bpm` the tempo `bpm` (or the octave of it nearest, as sync
/// does: 70 goes with 140), if it is close enough to its own.
pub fn matched_rate(bpm: f64, own_bpm: f64) -> Option<f64> {
    if !(bpm > 0.0 && own_bpm > 0.0) {
        return None;
    }
    let octave = prisme_analysis::tempo::octave_ratio(own_bpm, bpm);
    let rate = octave * bpm / own_bpm;
    ((rate - 1.0).abs() <= MAX_TEMPO_CHANGE).then_some(rate)
}

/// The start of the phrase `position_s` is in.
fn phrase_before(grid: &BeatGrid, position_s: f64) -> f64 {
    let phrase = (grid.beats_at(position_s) / PHRASE_BEATS).floor() * PHRASE_BEATS;
    grid.first_beat_s + phrase * grid.beat_period_s()
}

/// The first beat at or after `position_s`, and not before the track.
fn beat_from(grid: &BeatGrid, position_s: f64) -> f64 {
    const EPSILON: f64 = 1e-6;
    let first = (grid.beats_at(0.0) - EPSILON).ceil();
    let beat = (grid.beats_at(position_s) - EPSILON).ceil().max(first);
    grid.first_beat_s + beat * grid.beat_period_s()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FADE: FadeLength = FadeLength { beats: 16, seconds: 10.0 };
    /// 120 BPM: a beat every half second, a phrase every 16 s.
    const GRID_120: BeatGrid = BeatGrid { bpm: 120.0, first_beat_s: 0.25, bars_sure: true };

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    fn outgoing(grid: Option<BeatGrid>, end_s: f64) -> Outgoing {
        Outgoing { grid, bpm: grid.map(|g| g.bpm), end_s, speed: 1.0 }
    }

    #[test]
    fn the_silence_around_a_track_is_left_out() {
        let peaks = [0, 2, 4, 30, 255, 80, 3, 0, 0, 0];
        assert_eq!(audible(&peaks, 10), Some(Audible { start_s: 0.3, end_s: 0.6 }));
        assert_eq!(audible(&[0, 1, 4], 10), None);
    }

    #[test]
    fn a_synced_mix_starts_on_the_last_phrase_that_holds_the_fade() {
        let incoming = Incoming {
            grid: Some(BeatGrid { bpm: 124.0, first_beat_s: 0.1, bars_sure: true }),
            cue_s: None,
            audible_start_s: 0.0,
        };
        // heard until 200 s: the 16 beats must start by 192 s; phrases start at 0.25 + 16 k
        let plan = plan(&outgoing(Some(GRID_120), 200.0), &incoming, FADE);
        assert!(plan.synced);
        assert!(near(plan.at_s, 176.25), "{}", plan.at_s);
        assert!(near(plan.rate, 120.0 / 124.0));
        // 16 beats at 120 BPM last 8 s, which the next track covers at its speed
        assert!(near(plan.length_s, 8.0 * 120.0 / 124.0));
        assert!(near(plan.from_s, 0.1), "on its first beat");
    }

    #[test]
    fn the_next_track_starts_on_its_cue_or_on_its_first_beat_heard() {
        let grid = Some(BeatGrid { bpm: 120.0, first_beat_s: 0.2, bars_sure: true });
        let at = |cue_s, audible_start_s| start_of(&Incoming { grid, cue_s, audible_start_s });
        assert!(near(at(Some(33.3), 5.0), 33.3), "the cue of the DJ");
        assert!(near(at(None, 5.0), 5.2), "the beat after the silence");
        assert!(near(at(None, 5.2), 5.2));
        assert!(near(at(None, 0.0), 0.2));
        let early = BeatGrid { bpm: 120.0, first_beat_s: 0.7, bars_sure: true };
        let start = start_of(&Incoming { grid: Some(early), cue_s: None, audible_start_s: 0.0 });
        assert!(near(start, 0.2), "the grid goes on before its first beat, not before the track");
        let free = Incoming { grid: None, cue_s: None, audible_start_s: 1.5 };
        assert!(near(start_of(&free), 1.5));
    }

    #[test]
    fn without_a_grid_the_fade_ends_where_the_track_goes_silent() {
        let incoming = Incoming { grid: None, cue_s: None, audible_start_s: 0.0 };
        let plan = plan(&outgoing(Some(GRID_120), 200.0), &incoming, FADE);
        assert!(!plan.synced);
        assert_eq!((plan.at_s, plan.rate, plan.length_s), (190.0, 1.0, 10.0));
        let faster = Outgoing { speed: 1.05, ..outgoing(None, 200.0) };
        assert!(near(super::plan(&faster, &incoming, FADE).at_s, 189.5), "10 s at its speed");
    }

    #[test]
    fn tempos_too_far_apart_are_not_matched() {
        assert!(near(matched_rate(128.0, 124.0).unwrap(), 128.0 / 124.0));
        assert!(near(matched_rate(140.0, 70.0).unwrap(), 1.0), "half time");
        assert!(near(matched_rate(87.0, 174.0).unwrap(), 1.0), "double time");
        assert_eq!(matched_rate(174.0, 124.0), None);
        assert_eq!(matched_rate(0.0, 124.0), None);
        let incoming = Incoming {
            grid: Some(BeatGrid { bpm: 174.0, first_beat_s: 0.0, bars_sure: true }),
            cue_s: None,
            audible_start_s: 0.0,
        };
        let plan = plan(&outgoing(Some(GRID_120), 200.0), &incoming, FADE);
        assert!(!plan.synced && plan.rate == 1.0, "plays at its own tempo");
    }

    #[test]
    fn a_track_shorter_than_the_fade_mixes_from_its_start() {
        let incoming = Incoming { grid: Some(GRID_120), cue_s: None, audible_start_s: 0.0 };
        assert_eq!(plan(&outgoing(Some(GRID_120), 5.0), &incoming, FADE).at_s, 0.0);
        let free = Incoming { grid: None, ..incoming };
        assert_eq!(plan(&outgoing(None, 5.0), &free, FADE).at_s, 0.0);
    }
}
