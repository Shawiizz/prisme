//! A deck pushed faster or held back by a key, at the pace of Serato's − and + keys: held, the
//! speed leaves the tempo a little at once, then more and more, slower down to a stop in ten
//! seconds and then backwards, up to the tempo backwards, faster as much; let go, the deck is
//! back at its tempo at once.

/// The speed change as soon as a key is pressed (0.02 = 2 %)...
const START: f64 = 0.02;
/// ...growing with the square of the time it is held, so that held back the deck stops after
/// this long, as in Serato.
const STOP_S: f64 = 10.0;
const GROWTH: f64 = (1.0 - START) / (STOP_S * STOP_S);
/// The furthest it goes: four times the tempo forwards (Serato has no end, this one is far),
/// the tempo backwards.
const FASTEST: f64 = 3.0;
const SLOWEST: f64 = -2.0;

#[derive(Debug, Default)]
pub(crate) struct Push {
    /// Faster (1), slower (-1), or not pushed (0).
    way: f64,
    /// How long the key has been held (s).
    held_s: f64,
}

impl Push {
    /// Pushed `way`: faster (above 0), slower (below), or let go (0).
    pub(crate) fn hold(&mut self, way: f64) {
        self.way = if way == 0.0 { 0.0 } else { way.signum() };
        self.held_s = 0.0;
    }

    pub(crate) fn step(&mut self, seconds: f64) {
        if self.way != 0.0 {
            self.held_s += seconds;
        }
    }

    /// The speed added to the deck's (0.5 = +50 %, -2 = the tempo backwards).
    pub(crate) fn speed(&self) -> f64 {
        if self.way == 0.0 {
            return 0.0;
        }
        (self.way * (START + GROWTH * self.held_s * self.held_s)).clamp(SLOWEST, FASTEST)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn held_slower_it_stops_then_goes_back_up_to_the_tempo() {
        let mut push = Push::default();
        push.hold(-1.0);
        assert!((push.speed() + START).abs() < 1e-12, "a little at once");
        push.step(1.0);
        assert!(push.speed() > -0.03, "a nudge, after a second");
        push.step(9.0);
        assert!((push.speed() + 1.0).abs() < 1e-9, "stopped in ten seconds");
        push.step(10.0);
        assert_eq!(push.speed(), SLOWEST, "backwards at the tempo");
        push.hold(0.0);
        assert_eq!(push.speed(), 0.0, "let go: at its tempo at once");
    }

    #[test]
    fn held_faster_it_goes_as_fast_as_slower_then_on() {
        let mut push = Push::default();
        push.hold(1.0);
        push.step(10.0);
        assert!((push.speed() - 1.0).abs() < 1e-9, "twice the tempo in ten seconds");
        push.step(60.0);
        assert_eq!(push.speed(), FASTEST);
    }
}
