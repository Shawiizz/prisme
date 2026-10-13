//! The track moved by hand on the screen, the scrolling waveform dragged like a record.
//! The pointer gives positions with the time it was there; they reach the engine in
//! bunches, one audio buffer at a time, so the deck replays the hand's path a little
//! behind it, from those times: as smooth as the hand, whatever the buffer. Where the
//! hand holds still the track stops, with nothing pulled back; where it goes faster than
//! a track can sensibly be played (dragged along the whole track's overview), the track
//! jumps after it.

/// How far behind the hand the track follows (s): more than the audio buffers and the
/// pointer's own jitter, so that the path ahead is known.
const BEHIND_S: f64 = 0.03;
/// Positions kept: the path over more than `BEHIND_S` at the pointer's rate.
const KEPT: usize = 32;
/// The fastest a hand plays the track; faster, it jumps.
const MAX_SPEED: f64 = 32.0;

#[derive(Clone, Copy, Default)]
struct Point {
    /// When the hand was there, on the pointer's clock (s), and where (track s).
    at_s: f64,
    position_s: f64,
}

/// What a hand does to the track for a block.
pub(crate) enum Step {
    Play(f64),
    Jump(f64),
}

pub(crate) struct Hand {
    path: [Point; KEPT],
    /// Index of the newest point, and how many there are.
    newest: usize,
    len: usize,
    /// The engine's time since the hand came (s), and the pointer's clock minus it.
    clock_s: f64,
    offset_s: Option<f64>,
}

impl Hand {
    /// A hand put on the track where it plays.
    pub(crate) fn new() -> Self {
        Self { path: [Point::default(); KEPT], newest: 0, len: 0, clock_s: 0.0, offset_s: None }
    }

    /// The hand was at `position_s` at `at_s` on the pointer's clock.
    pub(crate) fn move_to(&mut self, position_s: f64, at_s: f64) {
        // the earliest a point arrives sets how the clocks line up
        let offset = at_s - self.clock_s;
        self.offset_s = Some(self.offset_s.map_or(offset, |o| o.min(offset)));
        if self.len > 0 && at_s <= self.path[self.newest].at_s {
            return;
        }
        self.newest = (self.newest + 1) % KEPT;
        self.path[self.newest] = Point { at_s, position_s };
        self.len = (self.len + 1).min(KEPT);
    }

    /// The next `seconds`, the playhead being at `playhead_s`: to where the hand was
    /// `BEHIND_S` before their end.
    pub(crate) fn step(&mut self, playhead_s: f64, seconds: f64) -> Step {
        self.clock_s += seconds;
        let Some(offset) = self.offset_s else { return Step::Play(0.0) };
        let target = self.position_at(self.clock_s + offset - BEHIND_S);
        let speed = (target - playhead_s) / seconds;
        if speed.abs() > MAX_SPEED {
            Step::Jump(target)
        } else {
            Step::Play(speed)
        }
    }

    /// Where the hand was at `at_s`: between the points around, the first or the last
    /// outside them.
    fn position_at(&self, at_s: f64) -> f64 {
        let point = |age: usize| self.path[(self.newest + KEPT - age) % KEPT];
        let mut later = point(0);
        if later.at_s <= at_s {
            return later.position_s;
        }
        for age in 1..self.len {
            let earlier = point(age);
            if earlier.at_s <= at_s {
                let share = (at_s - earlier.at_s) / (later.at_s - earlier.at_s);
                return earlier.position_s + share * (later.position_s - earlier.position_s);
            }
            later = earlier;
        }
        later.position_s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WASAPI's 10 ms buffers: the pointer's points arrive in bunches.
    const BUFFER_S: f64 = 0.01;
    /// A point every 17 ms, as a pointer moving at the screen's rate.
    const POINT_S: f64 = 0.017;

    /// The hand moving the track by `velocity` from 10 s for `seconds`, then holding still
    /// for `still_s`; returns the playhead and the speed of each buffer.
    fn drag(velocity: f64, seconds: f64, still_s: f64) -> (f64, Vec<f64>) {
        let mut hand = Hand::new();
        let mut playhead = 10.0;
        let mut speeds = Vec::new();
        let mut next_point = 0.0;
        let mut t = 0.0;
        while t < seconds + still_s {
            t += BUFFER_S;
            // the points of the buffer just gone arrive now
            while next_point <= t.min(seconds) {
                hand.move_to(10.0 + velocity * next_point, 100.0 + next_point);
                next_point += POINT_S;
            }
            match hand.step(playhead, BUFFER_S) {
                Step::Play(speed) => {
                    playhead += speed * BUFFER_S;
                    speeds.push(speed);
                }
                Step::Jump(to) => panic!("jumped to {to}"),
            }
        }
        (playhead, speeds)
    }

    #[test]
    fn a_hand_moving_steadily_plays_the_track_smoothly_at_its_speed() {
        let (_, speeds) = drag(-0.5, 1.0, 0.0);
        let settled = &speeds[10..speeds.len() - 5];
        assert!(settled.iter().all(|s| (s + 0.5).abs() < 0.02), "{settled:?}");
    }

    #[test]
    fn a_hand_far_ahead_of_the_track_makes_it_jump() {
        let mut hand = Hand::new();
        hand.move_to(10.0, 100.0);
        hand.move_to(70.0, 100.02);
        hand.step(10.0, 0.04);
        assert!(matches!(hand.step(10.0, 0.01), Step::Jump(to) if (to - 70.0).abs() < 1e-9));
    }

    #[test]
    fn a_hand_holding_still_stops_the_track_where_it_is() {
        let (playhead, speeds) = drag(1.0, 0.3, 0.3);
        assert!(speeds.iter().all(|&s| s >= 0.0), "pulled back: {speeds:?}");
        assert!(speeds.last().unwrap().abs() < 1e-9, "{:?}", speeds.last());
        let last_point = 10.0 + (0.3 / POINT_S).floor() * POINT_S;
        assert!((playhead - last_point).abs() < 1e-6, "{playhead}");
    }
}
