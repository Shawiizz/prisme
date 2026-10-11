//! The tempo of a record, from its platter speed. The speed read from the timecode
//! wavers at each turn (an off-centre record, the platter's wow: ±0.5 % on a real
//! turntable), so the tempo averages whole turns, which cancels that wavering exactly.
//! It holds still while the record keeps its speed, follows a pitch fader move as it
//! goes and settles about two seconds after it.

/// One turn of a record at 33⅓ rpm, at the nominal speed.
const TURN_S: f64 = 1.8;
const BUCKET_S: f64 = 0.025;
/// Two turns at 70 % of the nominal speed.
const BUCKETS: usize = 208;
/// Farther than this from the shown tempo, the last turn is a pitch fader move to follow.
const MOVE: f64 = 0.0015;
/// Farther than this from the shown tempo, the last two turns correct it.
const DRIFT: f64 = 0.0005;

#[derive(Clone, Copy, Default)]
struct Bucket {
    distance: f64,
    duration: f64,
}

pub(crate) struct PlatterTempo {
    buckets: [Bucket; BUCKETS],
    current: usize,
    shown: f64,
}

impl PlatterTempo {
    pub(crate) fn new() -> Self {
        Self { buckets: [Bucket::default(); BUCKETS], current: 0, shown: 1.0 }
    }

    /// The tempo as a playback rate (1.0 = the nominal speed).
    pub(crate) fn rate(&self) -> f64 {
        self.shown
    }

    /// `speed` held steadily for `seconds` (gestures are not tempo: they are left out).
    pub(crate) fn observe(&mut self, speed: f64, seconds: f64) {
        let bucket = &mut self.buckets[self.current];
        bucket.distance += speed * seconds;
        bucket.duration += seconds;
        if bucket.duration >= BUCKET_S {
            self.current = (self.current + 1) % BUCKETS;
            self.buckets[self.current] = Bucket::default();
        }

        let turn_s = TURN_S / self.shown.max(0.7);
        let (Some(last_turn), Some(two_turns)) = (self.average(turn_s), self.average(2.0 * turn_s)) else {
            return;
        };
        // while the last two turns disagree, the fader has just moved: the last turn is the tempo
        let moving = (last_turn - two_turns).abs() > MOVE * self.shown;
        let off = (last_turn - self.shown).abs();
        if off > MOVE * self.shown || (moving && off > DRIFT * self.shown) {
            self.shown = last_turn;
        } else if !moving && (two_turns - self.shown).abs() > DRIFT * self.shown {
            self.shown = two_turns;
        }
    }

    /// Mean speed over the last `window_s` (or as much as has been seen).
    fn average(&self, window_s: f64) -> Option<f64> {
        let (mut distance, mut duration) = (0.0, 0.0);
        for i in 0..BUCKETS {
            let bucket = self.buckets[(self.current + BUCKETS - i) % BUCKETS];
            distance += bucket.distance;
            duration += bucket.duration;
            if duration >= window_s {
                break;
            }
        }
        (duration > 0.0).then(|| distance / duration)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;

    const STEP_S: f64 = 128.0 / 48_000.0;

    /// A platter at `rate` wavering by ±0.5 % at each turn, for `seconds` from `t0`.
    fn play(tempo: &mut PlatterTempo, t0: f64, seconds: f64, rate: impl Fn(f64) -> f64) -> f64 {
        let mut t = t0;
        while t < t0 + seconds {
            let base = rate(t);
            tempo.observe(base * (1.0 + 0.005 * (TAU * t * base / TURN_S).sin()), STEP_S);
            t += STEP_S;
        }
        t
    }

    #[test]
    fn a_wavering_record_shows_a_still_tempo() {
        let mut tempo = PlatterTempo::new();
        let mut t = play(&mut tempo, 0.0, 26.0, |_| 1.0);
        let mut seen = (f64::MAX, f64::MIN);
        for _ in 0..2000 {
            t = play(&mut tempo, t, STEP_S, |_| 1.0);
            seen = (seen.0.min(tempo.rate()), seen.1.max(tempo.rate()));
        }
        assert!((tempo.rate() - 1.0).abs() < 0.0005, "tempo {}", tempo.rate());
        assert!(seen.1 - seen.0 < 1e-9, "the tempo moved by {}", seen.1 - seen.0);
    }

    #[test]
    fn a_pitch_fader_move_shows_as_it_goes_and_settles_within_two_seconds() {
        let mut tempo = PlatterTempo::new();
        let t = play(&mut tempo, 0.0, 6.0, |_| 1.0);
        // the fader from 0 to +6 % in half a second, then held
        let fader = |s: f64| 1.0 + 0.06 * ((s - 6.0) / 0.5).clamp(0.0, 1.0);
        let t = play(&mut tempo, t, 1.0, fader);
        assert!(tempo.rate() > 1.015, "while moving: {}", tempo.rate());
        let t = play(&mut tempo, t, 1.5, fader);
        assert!((tempo.rate() - 1.06).abs() < 0.002, "2 s after the move: {}", tempo.rate());
        play(&mut tempo, t, 2.0, fader);
        assert!((tempo.rate() - 1.06).abs() < 0.0008, "4 s after the move: {}", tempo.rate());
    }
}
