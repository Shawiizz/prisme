//! A row of keys of which one acts at a time (the slices of a slicer, the fader cuts): of
//! those held, the last pressed.

/// The keys of a row held, the last pressed last.
#[derive(Debug, Clone, Default)]
pub struct Held {
    keys: Vec<usize>,
}

impl Held {
    /// `key` pressed (`held`), or let go: the key that acted before, and the one that acts now.
    pub fn press(&mut self, key: usize, held: bool) -> (Option<usize>, Option<usize>) {
        let before = self.acting();
        self.keys.retain(|&k| k != key);
        if held {
            self.keys.push(key);
        }
        (before, self.acting())
    }

    /// The key acting, if one is held.
    pub fn acting(&self) -> Option<usize> {
        self.keys.last().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn of_the_keys_held_the_last_pressed_acts_and_the_one_before_once_it_is_let_go() {
        let mut slices = Held::default();
        assert_eq!(slices.press(2, true), (None, Some(2)));
        assert_eq!(slices.press(5, true), (Some(2), Some(5)));
        assert_eq!(slices.press(2, false), (Some(5), Some(5)), "one held before let go: no change");
        assert_eq!(slices.press(2, true), (Some(5), Some(2)));
        assert_eq!(slices.press(2, false), (Some(2), Some(5)));
        assert_eq!(slices.press(5, false), (Some(5), None));
        assert_eq!(slices.press(5, false), (None, None), "a release unheard of");
    }
}
