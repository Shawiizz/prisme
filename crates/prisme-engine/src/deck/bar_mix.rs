//! Stem changes that wait for the next bar: the vocals cut as the bar turns, whenever the
//! button was pressed during the bar before.

use crate::stems::StemMix;

/// The mix a deck is asked to play, and the parts of it waiting for a bar.
#[derive(Default)]
pub(super) struct BarMix {
    asked: StemMix,
    /// What plays: the asked mix, but for the waiting parts.
    playing: StemMix,
    waiting: [bool; 4],
    /// Where the waiting parts change (s): the start of a bar.
    at_s: f64,
}

impl BarMix {
    /// Asks for `mix`: the parts it changes from `bar_s` if given, else at once (and those
    /// waiting already keep waiting). What to play now.
    pub(super) fn ask(&mut self, mix: StemMix, bar_s: Option<f64>) -> StemMix {
        if let Some(bar_s) = bar_s {
            let (new, old) = (mix.parts(), self.asked.parts());
            for (waiting, (new, old)) in self.waiting.iter_mut().zip(new.into_iter().zip(old)) {
                *waiting |= new != old;
            }
            self.at_s = bar_s;
        }
        self.asked = mix;
        self.settle()
    }

    pub(super) fn is_waiting(&self) -> bool {
        self.waiting.contains(&true)
    }

    /// The waiting parts, a bit each in the order of [`StemMix::parts`].
    pub(super) fn waiting(&self) -> u8 {
        self.waiting.iter().enumerate().map(|(i, &w)| u8::from(w) << i).sum()
    }

    /// The playhead at `position_s`, about to move `advance_s` forward, the next bar from
    /// there at `next_bar_s` (`None` without a beat grid): the mix to play if the waiting
    /// parts' bar has come (or the grid gone). Moved back, they wait for the first bar from
    /// there.
    pub(super) fn follow(
        &mut self,
        position_s: f64,
        advance_s: f64,
        next_bar_s: Option<f64>,
    ) -> Option<StemMix> {
        if !self.is_waiting() {
            return None;
        }
        match next_bar_s {
            Some(next) if position_s + advance_s / 2.0 < self.at_s => {
                self.at_s = self.at_s.min(next);
                None
            }
            _ => Some(self.release()),
        }
    }

    /// The waiting parts change now: what to play.
    pub(super) fn release(&mut self) -> StemMix {
        self.waiting = [false; 4];
        self.settle()
    }

    /// Nothing waits any more, and the track plays whole: a new track was loaded.
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    fn settle(&mut self) -> StemMix {
        let (asked, playing) = (self.asked.parts(), self.playing.parts());
        self.playing =
            StemMix::from_parts(std::array::from_fn(|i| if self.waiting[i] { playing[i] } else { asked[i] }));
        self.playing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MUTED_VOCALS: StemMix = StemMix { vocals: 0.0, drums: 1.0, bass: 1.0, other: 1.0 };

    #[test]
    fn a_change_waits_for_its_bar() {
        let mut mix = BarMix::default();
        assert_eq!(mix.ask(MUTED_VOCALS, Some(8.0)), StemMix::default());
        assert_eq!(mix.waiting(), 0b0001);
        assert_eq!(mix.follow(7.0, 0.01, Some(8.0)), None);
        assert_eq!(mix.follow(7.996, 0.01, Some(8.0)), Some(MUTED_VOCALS));
        assert!(!mix.is_waiting());
    }

    #[test]
    fn other_parts_change_at_once_while_one_waits() {
        let mut mix = BarMix::default();
        mix.ask(MUTED_VOCALS, Some(8.0));
        let lower_drums = StemMix { drums: 0.5, ..MUTED_VOCALS };
        let now = mix.ask(lower_drums, None);
        assert_eq!(now, StemMix { drums: 0.5, ..StemMix::default() });
        assert_eq!(mix.follow(8.0, 0.01, Some(10.0)), Some(lower_drums));
    }

    #[test]
    fn moved_back_the_change_waits_for_the_first_bar_from_there() {
        let mut mix = BarMix::default();
        mix.ask(MUTED_VOCALS, Some(8.0));
        assert_eq!(mix.follow(2.5, 0.01, Some(4.0)), None);
        assert_eq!(mix.follow(3.999, 0.01, Some(4.0)), Some(MUTED_VOCALS));
    }

    #[test]
    fn without_a_grid_nothing_waits() {
        let mut mix = BarMix::default();
        mix.ask(MUTED_VOCALS, Some(8.0));
        assert_eq!(mix.follow(1.0, 0.01, None), Some(MUTED_VOCALS));
    }
}
