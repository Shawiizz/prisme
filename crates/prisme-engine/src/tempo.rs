//! The tempo of a record, from its platter speed. The speed read from the timecode
//! wavers at each turn (an off-centre record, the platter's wow: ±0.5 % on a real
//! turntable), so the tempo averages whole turns, which cancels that wavering exactly.
//! It holds still while the record keeps its speed. A pitch fader move shows against the
//! speed one turn earlier, at the same place of the record, where the wavering cancels
//! whatever its size: it is followed by a quick estimate within a third of a second, then
//! by the mean speed since the fader stopped, until the last turn is all after the move.

/// One turn of a record at 33⅓ rpm, at the nominal speed.
const TURN_S: f64 = 1.8;
const BUCKET_S: f64 = 0.025;
/// Two turns at 70 % of the nominal speed.
const BUCKETS: usize = 208;
/// Farther than this from the shown tempo, the last turn is a pitch fader move to follow.
const MOVE: f64 = 0.0015;
/// Farther than this from the shown tempo, the last two turns correct it.
const DRIFT: f64 = 0.0005;
/// Time constant of the quick estimate shown while the fader moves (s).
const QUICK_TAU_S: f64 = 0.25;
/// A change this large is the fader moving: of the speed over a turn, measured over
/// `FADER_BUCKETS`, or of the quick estimate while it moves.
const FADER_STEP: f64 = 0.012;
const FADER_BUCKETS: usize = 10;
/// The fader is taken as still after this long (s) without such a change.
const FADER_STILL_S: f64 = 0.4;

#[derive(Clone, Copy, Default)]
struct Bucket {
    distance: f64,
    duration: f64,
}

/// A pitch fader move, from the speed changing over a turn until the last turn is all
/// after it.
#[derive(Clone, Copy)]
struct FaderMove {
    quick: f64,
    /// The quick estimate at the last step.
    step_from: f64,
    since_step_s: f64,
    active: bool,
    /// The speed since the last step.
    settled: Bucket,
}

pub(crate) struct PlatterTempo {
    buckets: [Bucket; BUCKETS],
    current: usize,
    shown: f64,
    fader: FaderMove,
}

impl PlatterTempo {
    pub(crate) fn new() -> Self {
        let fader = FaderMove {
            quick: 1.0,
            step_from: 1.0,
            since_step_s: 0.0,
            active: false,
            settled: Bucket::default(),
        };
        Self { buckets: [Bucket::default(); BUCKETS], current: 0, shown: 1.0, fader }
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
        if self.follow_fader(speed, seconds, turn_s) {
            return;
        }
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

    /// Shows the fader's move while it lasts; false once the turn averages hold it.
    fn follow_fader(&mut self, speed: f64, seconds: f64, turn_s: f64) -> bool {
        let moved = self.moved_over_a_turn(turn_s);
        let fader = &mut self.fader;
        fader.quick += (speed - fader.quick) * (1.0 - (-seconds / QUICK_TAU_S).exp());
        let stepped = (fader.quick - fader.step_from).abs() > FADER_STEP * fader.step_from;
        if stepped || (moved && !fader.active) {
            fader.step_from = fader.quick;
            fader.since_step_s = 0.0;
            fader.settled = Bucket::default();
        } else {
            fader.since_step_s += seconds;
        }
        fader.settled.distance += speed * seconds;
        fader.settled.duration += seconds;
        fader.active = moved || (fader.active && fader.since_step_s < FADER_STILL_S);
        if !fader.active {
            return false;
        }
        self.shown = if fader.since_step_s < FADER_STILL_S {
            fader.quick
        } else {
            fader.settled.distance / fader.settled.duration
        };
        true
    }

    /// Whether the speed of the last buckets differs from the same buckets one turn
    /// earlier by a fader step (the wavering of the turn cancels between them).
    fn moved_over_a_turn(&self, turn_s: f64) -> bool {
        let back = (turn_s / BUCKET_S).round() as usize;
        match (self.mean_of(1, FADER_BUCKETS), self.mean_of(1 + back, FADER_BUCKETS)) {
            (Some(now), Some(turn_ago)) => (now - turn_ago).abs() > FADER_STEP * turn_ago,
            _ => false,
        }
    }

    /// Mean speed of `count` whole buckets, the newest of them `skip` buckets back.
    fn mean_of(&self, skip: usize, count: usize) -> Option<f64> {
        let (mut distance, mut duration) = (0.0, 0.0);
        for i in skip..skip + count {
            let bucket = self.buckets[(self.current + BUCKETS - i % BUCKETS) % BUCKETS];
            distance += bucket.distance;
            duration += bucket.duration;
        }
        (duration > 0.0).then(|| distance / duration)
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
        play_wavering(tempo, t0, seconds, 0.005, rate)
    }

    /// A platter at `rate` wavering by `waver` (relative) at each turn.
    fn play_wavering(
        tempo: &mut PlatterTempo,
        t0: f64,
        seconds: f64,
        waver: f64,
        rate: impl Fn(f64) -> f64,
    ) -> f64 {
        let mut t = t0;
        while t < t0 + seconds {
            let base = rate(t);
            tempo.observe(base * (1.0 + waver * (TAU * t * base / TURN_S).sin()), STEP_S);
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
    fn a_record_wavering_more_than_most_is_not_taken_for_fader_moves() {
        let mut tempo = PlatterTempo::new();
        let mut t = play_wavering(&mut tempo, 0.0, 10.0, 0.02, |_| 1.0);
        let mut seen = (f64::MAX, f64::MIN);
        for _ in 0..4000 {
            t = play_wavering(&mut tempo, t, STEP_S, 0.02, |_| 1.0);
            seen = (seen.0.min(tempo.rate()), seen.1.max(tempo.rate()));
        }
        assert!(seen.1 - seen.0 < 0.002, "the tempo wavered by {}", seen.1 - seen.0);
    }

    #[test]
    fn a_pitch_fader_move_shows_as_it_goes_and_settles_within_two_seconds() {
        let mut tempo = PlatterTempo::new();
        let t = play(&mut tempo, 0.0, 6.0, |_| 1.0);
        // the fader from 0 to +6 % in half a second, then held
        let fader = |s: f64| 1.0 + 0.06 * ((s - 6.0) / 0.5).clamp(0.0, 1.0);
        let t = play(&mut tempo, t, 0.5, fader);
        assert!(tempo.rate() > 1.03, "as the fader stops: {}", tempo.rate());
        let t = play(&mut tempo, t, 0.3, fader);
        assert!((tempo.rate() - 1.06).abs() < 0.012, "0.3 s after the move: {}", tempo.rate());
        let t = play(&mut tempo, t, 1.7, fader);
        assert!((tempo.rate() - 1.06).abs() < 0.002, "2 s after the move: {}", tempo.rate());
        play(&mut tempo, t, 2.0, fader);
        assert!((tempo.rate() - 1.06).abs() < 0.0008, "4 s after the move: {}", tempo.rate());
    }
}
