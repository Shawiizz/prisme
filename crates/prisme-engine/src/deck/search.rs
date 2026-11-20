//! A search key held: the track runs through faster than it plays, by jumps a few times a
//! second, a snatch of it heard after each, as on a CD player.

/// How often a search jumps (s).
const EVERY_S: f64 = 0.05;

#[derive(Default)]
pub(super) struct Search {
    /// Seconds of track a second it runs through beside its play (back below zero); 0: none.
    speed: f64,
    /// Play left before the next jump (s).
    wait_s: f64,
}

impl Search {
    /// Runs through at `speed` from now on, its first jump a little later; 0 stops.
    pub(super) fn set(&mut self, speed: f64) {
        *self = Self { speed, wait_s: EVERY_S };
    }

    /// `seconds` of play gone by: how far the track jumps now (s), if it does.
    pub(super) fn step(&mut self, seconds: f64) -> Option<f64> {
        if self.speed == 0.0 {
            return None;
        }
        self.wait_s -= seconds;
        if self.wait_s > 0.0 {
            return None;
        }
        let jumps = (-self.wait_s / EVERY_S).floor() + 1.0;
        self.wait_s += jumps * EVERY_S;
        Some(self.speed * EVERY_S * jumps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_search_jumps_by_its_speed_every_twentieth_of_a_second() {
        let mut search = Search::default();
        assert_eq!(search.step(1.0), None, "none held");
        search.set(8.0);
        let jumps: Vec<Option<f64>> = (0..10).map(|_| search.step(0.01)).collect();
        let jumped: f64 = jumps.iter().flatten().sum();
        assert_eq!(jumps.iter().flatten().count(), 2);
        assert!((jumped - 0.8).abs() < 1e-9);
        assert!((search.step(0.12).unwrap() - 0.8).abs() < 1e-9, "a long period: two jumps");
        search.set(0.0);
        assert_eq!(search.step(1.0), None);
    }
}
