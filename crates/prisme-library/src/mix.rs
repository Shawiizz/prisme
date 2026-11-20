//! How the tracks of a list go with the deck they are compared with: whether their key mixes
//! with the one heard there, the pitch that brings them to its tempo, and how well they would
//! follow it ([`Rank`]).

use crate::catalogue::Track;
use prisme_analysis::{octave_ratio, Key};
use std::cmp::Ordering;

/// The deck the tracks are compared with, as it sounds now: its key as heard, its tempo, and
/// the reach of the tempo fader of the deck the next track would play on.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Compare {
    pub key: Option<Key>,
    pub bpm: Option<f64>,
    pub range: f64,
}

/// The pitch that brings a track to the deck's tempo.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pitch {
    /// In percent, to a tenth.
    pub percent: f64,
    /// Whether the tempo fader reaches it.
    pub reachable: bool,
}

impl Compare {
    /// Whether a track in `key` mixes with the deck's.
    pub fn fits(&self, key: Option<Key>) -> bool {
        self.key.zip(key).is_some_and(|(deck, track)| deck.mixes_with(track))
    }

    /// The pitch that brings a track at `bpm` to the deck's tempo, half and double tempos
    /// counting as the same.
    pub fn pitch(&self, bpm: Option<f64>) -> Option<Pitch> {
        let change = change(bpm?, self.bpm?);
        let percent = (change * 1000.0).round() / 10.0;
        Some(Pitch { percent: percent + 0.0, reachable: change.abs() <= self.range })
    }
}

/// The share the speed of a track at `bpm` changes by to play at `reference` BPM.
fn change(bpm: f64, reference: f64) -> f64 {
    octave_ratio(bpm, reference) * reference / bpm - 1.0
}

/// The deck the tracks are ranked against, as it sounded when the ranking was asked for (its
/// tempo and key move with the platter, the order should not), and what the deck the next
/// track goes on does to it.
#[derive(Debug, Clone, PartialEq)]
pub struct MixReference {
    pub bpm: Option<f64>,
    /// Its key as heard.
    pub key: Option<Key>,
    /// The track on it: not a candidate.
    pub path: Option<String>,
    /// The reach of the next deck's tempo fader (0.08: ±8 %).
    pub range: f64,
    /// Whether the next deck holds its key when its tempo changes.
    pub key_lock: bool,
}

/// How well a track follows the reference, compared term by term, the best first:
/// 1. not played tonight (nor on the reference deck);
/// 2. the harmony of its key as it will be heard (moved by the pitch that matches the tempo,
///    unless the key lock holds it): it mixes, then it lifts the energy (two steps up the
///    wheel), then the others, then unknown keys;
/// 3. a tempo the fader reaches;
/// 4. the smallest pitch change.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rank {
    played: bool,
    harmony: u8,
    unreachable: bool,
    pitch: f64,
}

impl Rank {
    /// The rank of `track` against `reference`, `played` tonight or not.
    pub fn of(track: &Track, reference: &MixReference, played: bool) -> Self {
        let played = played || reference.path.as_deref() == Some(track.path.as_str());
        let change = track.bpm.zip(reference.bpm).map(|(bpm, to)| change(bpm, to));
        let heard = match change {
            Some(change) => track.key.map(|key| key.heard(reference.key_lock, 0, 1.0 + change)),
            None => track.key,
        };
        let harmony = match heard.zip(reference.key) {
            None => 3,
            Some((heard, to)) if heard.mixes_with(to) => 0,
            Some((heard, to)) if to.lifts_to(heard) => 1,
            Some(_) => 2,
        };
        let pitch = change.map_or(f64::MAX, f64::abs);
        Self { played, harmony, unreachable: pitch > reference.range, pitch }
    }
}

impl Eq for Rank {}

impl PartialOrd for Rank {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Rank {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.played, self.harmony, self.unreachable)
            .cmp(&(other.played, other.harmony, other.unreachable))
            .then(self.pitch.total_cmp(&other.pitch))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalogue::tests::record;
    use crate::catalogue::Catalogue;
    use prisme_media::TrackTags;

    fn camelot(code: &str) -> Key {
        Key::from_camelot(code).unwrap()
    }

    /// The ranks of tracks at `bpm` in `key`, ranked against 8A at 125 BPM, ±8 %.
    fn ranks(tracks: &[(&str, Option<f64>, Option<&str>)], key_lock: bool, played: &[&str]) -> Vec<Rank> {
        let mut catalogue = Catalogue::default();
        let reference = MixReference {
            bpm: Some(125.0),
            key: Some(camelot("8A")),
            path: Some("on-deck".into()),
            range: 0.08,
            key_lock,
        };
        tracks
            .iter()
            .map(|&(path, bpm, key)| {
                let id = catalogue.put(record(path, TrackTags::default(), bpm, key));
                Rank::of(catalogue.get(id), &reference, played.contains(&path))
            })
            .collect()
    }

    #[test]
    fn a_key_that_mixes_comes_first_then_one_that_lifts_then_the_others_then_none() {
        let r = ranks(
            &[
                ("mixes", Some(125.0), Some("9A")),
                ("lifts", Some(125.0), Some("10A")),
                ("other", Some(125.0), Some("2B")),
                ("unknown", Some(125.0), None),
            ],
            false,
            &[],
        );
        assert!(r[0] < r[1] && r[1] < r[2] && r[2] < r[3], "{r:?}");
    }

    #[test]
    fn the_key_is_ranked_as_it_will_be_heard_unless_the_key_lock_holds_it() {
        // a semitone slower brings 3A down to 8A
        let semitone_up = 125.0 * 2f64.powf(1.0 / 12.0);
        let free =
            ranks(&[("a", Some(semitone_up), Some("3A")), ("b", Some(semitone_up), Some("8A"))], false, &[]);
        assert!(free[0] < free[1], "3A heard as 8A, 8A as 1A");
        let locked =
            ranks(&[("a", Some(semitone_up), Some("3A")), ("b", Some(semitone_up), Some("8A"))], true, &[]);
        assert!(locked[1] < locked[0]);
    }

    #[test]
    fn played_tracks_go_last_then_unreachable_tempos_then_by_pitch() {
        let r = ranks(
            &[
                ("near", Some(126.0), Some("8A")),
                ("far", Some(128.0), Some("8A")),
                ("half", Some(62.0), Some("8A")),
                ("too far", Some(140.0), Some("8A")),
                ("played", Some(125.0), Some("8A")),
                ("on-deck", Some(125.0), Some("8A")),
            ],
            true,
            &["played"],
        );
        assert!(r[0] < r[1], "the smaller change");
        assert!(r[2] < r[1], "a half tempo is a small change");
        assert!(r[1] < r[3], "within reach first");
        assert!(r[3] < r[4] && r[4] == r[5], "played tonight or on the deck: last");
    }

    #[test]
    fn a_track_fits_the_key_heard_and_is_pitched_to_its_tempo() {
        let compare = Compare { key: Some(camelot("8A")), bpm: Some(128.0), range: 0.08 };
        assert!(compare.fits(Some(camelot("8B"))) && !compare.fits(Some(camelot("3A"))));
        assert!(!compare.fits(None) && !Compare::default().fits(Some(camelot("8A"))));
        assert_eq!(compare.pitch(Some(125.0)), Some(Pitch { percent: 2.4, reachable: true }));
        assert_eq!(compare.pitch(Some(64.0)), Some(Pitch { percent: 0.0, reachable: true }), "half tempo");
        assert_eq!(compare.pitch(Some(140.0)), Some(Pitch { percent: -8.6, reachable: false }));
        assert_eq!(compare.pitch(Some(128.06)).map(|p| p.percent.to_string()), Some("0".into()), "no -0");
        assert_eq!(compare.pitch(None), None);
    }
}
