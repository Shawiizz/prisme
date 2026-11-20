//! The key a deck is heard in.

use prisme_analysis::Key;

/// The key heard of a track in `key` played at `rate` (1 its own tempo): moved by the speed,
/// unless the key lock holds it (`key_lock`) or a shift of `key_shift` semitones, which goes
/// through the same stretcher, does.
pub fn heard(key: Key, key_lock: bool, key_shift: i8, rate: f64) -> Key {
    let held = key_lock || key_shift != 0;
    let semitones = if held { i32::from(key_shift) } else { (12.0 * rate.log2()).round() as i32 };
    key.transposed(semitones)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camelot(code: &str) -> Key {
        Key::from_camelot(code).unwrap()
    }

    #[test]
    fn a_deck_sped_up_a_semitone_is_heard_a_semitone_higher() {
        let up = 2f64.powf(1.0 / 12.0);
        assert_eq!(heard(camelot("8A"), false, 0, up), camelot("3A"));
        assert_eq!(heard(camelot("8A"), false, 0, 1.02), camelot("8A"), "nearer its own");
    }

    #[test]
    fn the_key_lock_and_a_key_shift_hold_the_key_whatever_the_speed() {
        assert_eq!(heard(camelot("8A"), true, 0, 1.1), camelot("8A"));
        assert_eq!(heard(camelot("8A"), false, -1, 1.1), camelot("8A").transposed(-1));
    }
}
