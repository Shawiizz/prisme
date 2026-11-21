//! What the screen shows of each deck at a frame, read from a [`View`] by the rules the
//! controllers' lights and values follow: the rate it plays at and the key heard, where its
//! tempo fader stands, the time left, who drives it, what its keys may do and light, its stems;
//! and the strip comparing two decks' beats.

use crate::light::{cue_state, ending, State};
use crate::pads::Pads;
use crate::scales::Tempo;
use crate::stems::part_in;
use crate::sync::{drift_time, grid_shift_to, matching_tempo, phase, phase_offset, Beat, Phase, IN_PHASE};
use crate::view::{plays, View};
use prisme_analysis::Key;
use prisme_control::StemPart;
use prisme_engine::VinylMode;
use serde::{Deserialize, Serialize};

/// The offset heard as two kicks rather than one: a sixteenth note (of a beat).
const HEARD_BEATS: f64 = 1.0 / 16.0;
/// How far ahead the strip tells the drift (s).
const FORECAST_S: f64 = 60.0;

/// A deck as the screen shows it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Deck {
    /// The rate it plays at (see [`View::tempo_rate`]).
    pub rate: f64,
    /// The key its track is heard in, if the track's is known.
    pub heard_key: Option<Key>,
    /// The deck it is mixed with (see [`View::mixed_with`]).
    pub mixed_with: usize,
    /// The DJ hears or holds its track (see [`plays`]).
    pub plays: bool,
    /// The time left of its track, if it has one (s).
    pub remaining_s: Option<f64>,
    /// The end of its track near, and soon.
    pub ending: State,
    pub drive: Drive,
    pub tempo: TempoFader,
    pub sync: SyncState,
    pub sync_key: SyncKey,
    /// Its loops, jumps and pads may move its track: it has one, which no record holds in place.
    pub can_move: bool,
    /// Its motor (brake, spin back) may move its track: it has one, which it plays by itself.
    pub can_motor: bool,
    /// Its grid may move onto the beats of the deck it is mixed with: both play on a grid.
    pub can_align_grid: bool,
    /// Its key may be moved to mix with the one heard on the deck it is mixed with: both known.
    pub can_sync_key: bool,
    /// Its CUE key: on at the cue point, paused stopped elsewhere.
    pub cue: State,
    /// What its pads on the screen show.
    pub pads: Pads,
}

/// A part of a deck's separated track, as its knob shows it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct StemKnob {
    pub part: StemPart,
    pub level: f32,
    pub on: bool,
    /// Its change waits for the next bar.
    pub waiting: bool,
    /// Separated where the deck plays.
    pub ready: bool,
}

/// The strip comparing two decks' beats.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PhaseStrip {
    /// The decks compared: the first two playing, else A and B.
    pub decks: [usize; 2],
    #[serde(flatten)]
    pub reading: Reading,
}

/// What the strip tells of the second deck's beats against the first's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "PhaseReading"))]
pub enum Reading {
    /// Not both playing on a grid.
    Off,
    /// At their tempos, they will be heard off the beat within a minute (0: they are).
    Drifting {
        seconds: u32,
    },
    Matched,
    Ahead {
        ms: u32,
    },
    Behind {
        ms: u32,
    },
}

/// Who drives a deck.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "DeckMode"))]
pub enum Drive {
    /// It plays by itself.
    #[serde(rename = "int")]
    Internal,
    /// Its record gives the speed.
    #[serde(rename = "relative")]
    Relative,
    /// Its record gives the speed and the place.
    #[serde(rename = "absolute")]
    Absolute,
    /// No one: it plays its input as it comes.
    #[serde(rename = "thru")]
    Thru,
}

/// A deck's tempo fader as shown.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct TempoFader {
    /// It does not set the tempo: a record does, or the sync.
    pub held: bool,
    /// The tempo it shows, as a rate: the one played while held, else its own.
    pub ratio: f64,
    /// Where it stands, 0 to 1.
    pub position: f32,
    /// On a record the sync does not lock: the rate matching the deck it follows.
    pub matching: Option<f64>,
    /// Where that rate is along the fader, within its range.
    pub matching_at: Option<f32>,
}

/// A deck's sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum SyncState {
    Off,
    /// Matched once, on a record: its speed is moved by the match.
    Matched,
    Locked,
}

/// What a deck's SYNC key does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum SyncKey {
    /// Locks it to the leader, or held, matches it once.
    Lock,
    /// Matches the leader once.
    Once,
    /// Nothing: its track has no beat grid, or it has no track.
    NoGrid,
    /// Nothing: its record holds the place.
    Absolute,
}

/// `deck` as `view` shows it.
pub fn deck(view: &View, deck: usize) -> Deck {
    let status = view.deck_status(deck);
    let settings = &view.decks[deck];
    let rate = view.tempo_rate(deck);
    let mixed_with = view.mixed_with(deck);
    let drive = drive(view, deck);
    let sync = match (status.sync.locked, status.sync.rate_offset != 1.0) {
        (true, _) => SyncState::Locked,
        (false, true) => SyncState::Matched,
        (false, false) => SyncState::Off,
    };
    let sync_key = if drive == Drive::Absolute {
        SyncKey::Absolute
    } else if !status.loaded || status.bpm.is_none() {
        SyncKey::NoGrid
    } else if settings.vinyl && view.sync_once_on_records {
        SyncKey::Once
    } else {
        SyncKey::Lock
    };
    let other = view.deck_status(mixed_with);
    let bpm = settings.track.and_then(|t| t.bpm);
    Deck {
        rate,
        heard_key: heard_key(view, deck),
        mixed_with,
        plays: plays(status),
        remaining_s: status.loaded.then(|| (status.duration_s - status.position_s).max(0.0)),
        ending: ending(status),
        drive,
        tempo: tempo_fader(view, deck),
        sync,
        sync_key,
        can_move: status.loaded && drive != Drive::Absolute,
        can_motor: status.loaded && drive == Drive::Internal,
        can_align_grid: bpm.and_then(|bpm| grid_shift_to(status, other, bpm)).is_some(),
        can_sync_key: settings.track.and_then(|t| t.key).is_some() && heard_key(view, mixed_with).is_some(),
        cue: cue_state(status, settings.track.as_ref()),
        pads: Pads::of(view, deck),
    }
}

/// The knobs of the parts of the track on `deck`, as far as it is separated.
pub fn stem_knobs(view: &View, deck: usize) -> impl Iterator<Item = StemKnob> + '_ {
    let status = view.deck_status(deck);
    view.decks[deck].track.into_iter().flat_map(move |track| {
        track.separation.parts().iter().map(move |&part| StemKnob {
            part,
            level: track.stem_mix.level(part),
            on: track.stem_mix.playing(part),
            waiting: part_in(status.stems_waiting, part),
            ready: part_in(status.stems_here, part),
        })
    })
}

/// The strip as `view` shows it.
pub fn phase_strip(view: &View) -> PhaseStrip {
    let mut playing = (0..view.decks_in_use).filter(|&d| view.deck_status(d).playing);
    let decks = match (playing.next(), playing.next()) {
        (Some(a), Some(b)) => [a, b],
        _ => [0, 1],
    };
    let [first, second] = decks.map(|d| Beat::playing(view.deck_status(d)));
    PhaseStrip { decks, reading: second.zip(first).map_or(Reading::Off, |(b, a)| reading(b, a)) }
}

/// What the strip tells of `deck`'s beats against `other`'s, both playing.
fn reading(deck: Beat, other: Beat) -> Reading {
    let drifting = drift_time(deck, other, HEARD_BEATS).map(f64::round).filter(|s| *s <= FORECAST_S);
    if let Some(seconds) = drifting {
        return Reading::Drifting { seconds: seconds as u32 };
    }
    let ms = (phase_offset(deck, other).abs() * 60_000.0 / deck.bpm).round() as u32;
    match phase(Some(deck), Some(other), IN_PHASE) {
        Phase::In => Reading::Matched,
        Phase::Ahead => Reading::Ahead { ms },
        Phase::Behind => Reading::Behind { ms },
        Phase::Off => Reading::Off,
    }
}

/// The key the track on `deck` is heard in, if its key is known.
pub fn heard_key(view: &View, deck: usize) -> Option<Key> {
    let settings = &view.decks[deck];
    let track = settings.track?;
    Some(track.key?.heard(settings.key_lock, track.key_shift, view.tempo_rate(deck)))
}

/// Who drives `deck`: its record, unless the needle ran out and the deck plays on by itself.
fn drive(view: &View, deck: usize) -> Drive {
    let settings = &view.decks[deck];
    if settings.passthrough {
        Drive::Thru
    } else if !settings.vinyl || view.deck_status(deck).vinyl.run_out {
        Drive::Internal
    } else {
        match settings.vinyl_mode {
            VinylMode::Relative => Drive::Relative,
            VinylMode::Absolute => Drive::Absolute,
        }
    }
}

/// Whether something else than its fader holds the tempo of `deck`: its record, or the sync.
pub fn tempo_held(view: &View, deck: usize) -> bool {
    view.decks[deck].vinyl || view.deck_status(deck).sync.locked
}

fn tempo_fader(view: &View, deck: usize) -> TempoFader {
    let status = view.deck_status(deck);
    let settings = &view.decks[deck];
    let held = tempo_held(view, deck);
    let ratio = if held { view.tempo_rate(deck) } else { settings.pitch };
    let range = settings.tempo_range;
    let matching = matching_tempo(&view.status, deck).filter(|_| settings.vinyl && !status.sync.locked);
    TempoFader {
        held,
        ratio,
        position: Tempo { ratio, range }.position(),
        matching,
        matching_at: matching
            .filter(|m| (m - 1.0).abs() <= range)
            .map(|ratio| Tempo { ratio, range }.position()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stems::{Separation, StemMix};
    use crate::view::fixtures::{loaded, view};

    fn camelot(code: &str) -> Key {
        Key::from_camelot(code).unwrap()
    }

    #[test]
    fn a_deck_plays_at_the_engines_rate_with_a_track_else_at_its_faders() {
        let mut v = loaded(view(), 0, 10.0, true);
        v.status.decks[0].rate = 1.04;
        v.decks[0].pitch = 1.02;
        v.decks[1].pitch = 0.97;
        assert_eq!(deck(&v, 0).rate, 1.04, "a record or the sync gives it");
        assert_eq!(deck(&v, 1).rate, 0.97, "no track");
    }

    #[test]
    fn the_key_is_heard_moved_by_the_rate_unless_the_key_lock_or_a_shift_holds_it() {
        let mut v = loaded(view(), 0, 10.0, true);
        assert_eq!(deck(&v, 0).heard_key, None, "no key known");
        if let Some(track) = &mut v.decks[0].track {
            track.key = Some(camelot("8A"));
        }
        v.status.decks[0].rate = 2f64.powf(1.0 / 12.0);
        assert_eq!(deck(&v, 0).heard_key, Some(camelot("3A")), "a semitone up");
        v.decks[0].key_lock = true;
        assert_eq!(deck(&v, 0).heard_key, Some(camelot("8A")));
    }

    #[test]
    fn a_deck_is_mixed_with_one_playing_else_the_one_beside_it() {
        let mut v = view();
        v.decks_in_use = 4;
        v.status.deck_count = 4;
        let v = loaded(v, 2, 0.0, true);
        assert_eq!(deck(&v, 0).mixed_with, 2);
        assert_eq!(deck(&v, 2).mixed_with, 3);
    }

    #[test]
    fn a_deck_plays_for_the_dj_while_its_track_plays_or_moves_under_its_record() {
        let mut v = loaded(view(), 0, 10.0, false);
        assert!(!deck(&v, 0).plays);
        v.status.decks[0].speed = 0.1;
        assert!(deck(&v, 0).plays, "a hand moving the record");
        v.status.decks[0].speed = 0.01;
        assert!(!deck(&v, 0).plays, "held still");
        v.status.decks[1].playing = true;
        assert!(!deck(&v, 1).plays, "no track");
    }

    #[test]
    fn the_time_left_and_the_end_near_while_it_plays() {
        let v = loaded(view(), 0, 285.0, true);
        assert_eq!(deck(&v, 0).remaining_s, Some(15.0));
        assert_eq!(deck(&v, 0).ending, State::On);
        assert_eq!(deck(&v, 1).remaining_s, None);
        let mut past = loaded(view(), 0, 301.0, false);
        past.status.decks[0].position_s = 301.0;
        assert_eq!(deck(&past, 0).remaining_s, Some(0.0));
        assert_eq!(deck(&past, 0).ending, State::Off, "stopped");
    }

    #[test]
    fn a_record_drives_its_deck_until_the_needle_runs_out_and_a_deck_on_its_input_is_thru() {
        let mut v = loaded(view(), 0, 10.0, true);
        assert_eq!(deck(&v, 0).drive, Drive::Internal);
        assert!(deck(&v, 0).can_motor);
        v.decks[0].vinyl = true;
        assert_eq!(deck(&v, 0).drive, Drive::Absolute);
        assert!(!deck(&v, 0).can_move, "the record holds the place");
        v.decks[0].vinyl_mode = VinylMode::Relative;
        assert_eq!(deck(&v, 0).drive, Drive::Relative);
        assert!(deck(&v, 0).can_move);
        assert!(!deck(&v, 0).can_motor, "the record turns it");
        v.status.decks[0].vinyl.run_out = true;
        assert_eq!(deck(&v, 0).drive, Drive::Internal);
        assert!(deck(&v, 0).can_motor, "played by itself");
        v.decks[0].passthrough = true;
        assert_eq!(deck(&v, 0).drive, Drive::Thru);
        assert!(!deck(&view(), 0).can_move, "no track");
    }

    #[test]
    fn the_tempo_fader_stands_at_its_tempo_or_at_the_rate_played_while_held() {
        let mut v = loaded(view(), 0, 10.0, true);
        v.decks[0].pitch = 1.04;
        v.status.decks[0].rate = 1.04;
        let free = deck(&v, 0).tempo;
        assert!(!free.held);
        assert_eq!((free.ratio, free.position), (1.04, 0.75), "half way up a range of 8 %");
        v.status.decks[0].sync.locked = true;
        v.status.decks[0].rate = 1.02;
        let locked = deck(&v, 0).tempo;
        assert!(locked.held);
        assert_eq!(locked.ratio, 1.02);
        assert_eq!(locked.matching, None, "the sync keeps it");
    }

    #[test]
    fn a_record_not_locked_is_shown_the_rate_matching_the_deck_it_follows() {
        let mut v = loaded(loaded(view(), 1, 0.0, true), 0, 0.0, true);
        v.decks[0].vinyl = true;
        v.status.decks[1].bpm = Some(123.0);
        let tempo = deck(&v, 0).tempo;
        let matching = tempo.matching.unwrap();
        assert!((matching - 1.025).abs() < 1e-9, "{matching}");
        assert!((tempo.matching_at.unwrap() - 0.65625).abs() < 1e-6);
        v.status.decks[1].bpm = Some(140.0);
        assert!(deck(&v, 0).tempo.matching.is_some());
        assert_eq!(deck(&v, 0).tempo.matching_at, None, "beyond its range");
        v.decks[0].vinyl = false;
        assert_eq!(deck(&v, 0).tempo.matching, None, "its fader is its own");
    }

    #[test]
    fn the_sync_key_locks_matches_once_on_a_record_or_waits_for_a_grid() {
        let mut v = loaded(view(), 0, 10.0, false);
        assert_eq!(deck(&v, 0).sync_key, SyncKey::Lock);
        assert_eq!(deck(&v, 1).sync_key, SyncKey::NoGrid, "no track");
        v.decks[0].vinyl = true;
        v.decks[0].vinyl_mode = VinylMode::Relative;
        assert_eq!(deck(&v, 0).sync_key, SyncKey::Lock);
        v.sync_once_on_records = true;
        assert_eq!(deck(&v, 0).sync_key, SyncKey::Once);
        v.decks[0].vinyl_mode = VinylMode::Absolute;
        assert_eq!(deck(&v, 0).sync_key, SyncKey::Absolute);
        v.status.decks[0].sync.rate_offset = 1.01;
        assert_eq!(deck(&v, 0).sync, SyncState::Matched);
        v.status.decks[0].sync.locked = true;
        assert_eq!(deck(&v, 0).sync, SyncState::Locked);
    }

    #[test]
    fn the_cue_key_is_lit_at_the_cue_point_and_paused_away_from_it() {
        let mut v = loaded(view(), 0, 0.0, false);
        assert_eq!(deck(&v, 0).cue, State::On, "no cue: the start");
        v.status.decks[0].position_s = 30.0;
        assert_eq!(deck(&v, 0).cue, State::Paused);
        v.status.decks[0].playing = true;
        assert_eq!(deck(&v, 0).cue, State::Off);
        assert_eq!(deck(&v, 1).cue, State::Off, "no track");
    }

    #[test]
    fn the_slice_played_is_the_one_the_playhead_is_in() {
        // 120 BPM: a slice of half a second from 4 s
        let mut v = loaded(view(), 0, 5.2, true);
        assert_eq!(deck(&v, 0).pads.slice.lit, 0, "no slicer");
        v.perform.decks[0].slicer = Some(4.0);
        assert_eq!(deck(&v, 0).pads.slice.lit, 1 << 2);
        v.status.decks[0].position_s = 9.0;
        assert_eq!(deck(&v, 0).pads.slice.lit, 0, "past its eight beats");
    }

    #[test]
    fn a_separated_track_shows_a_knob_a_part_the_instrumental_by_its_loudest_instrument() {
        let mut v = loaded(view(), 0, 0.0, true);
        assert_eq!(stem_knobs(&v, 0).count(), 0, "not separated");
        if let Some(track) = &mut v.decks[0].track {
            track.separation = Separation::Vocals;
            track.stem_mix = StemMix::default()
                .with_level(StemPart::Drums, 0.25)
                .with_level(StemPart::Bass, 0.5)
                .with_level(StemPart::Melody, 0.1)
                .toggled(StemPart::Vocals);
        }
        v.status.decks[0].stems_waiting = 0b0100;
        v.status.decks[0].stems_here = 0b0001;
        let knobs: Vec<StemKnob> = stem_knobs(&v, 0).collect();
        let knob = |part, level, on, waiting, ready| StemKnob { part, level, on, waiting, ready };
        assert_eq!(
            knobs,
            [
                knob(StemPart::Vocals, 1.0, false, false, true),
                knob(StemPart::Instrumental, 0.5, true, true, false)
            ]
        );
        if let Some(track) = &mut v.decks[0].track {
            track.separation = Separation::Full;
        }
        let parts: Vec<StemPart> = stem_knobs(&v, 0).map(|k| k.part).collect();
        assert_eq!(parts, [StemPart::Vocals, StemPart::Drums, StemPart::Bass, StemPart::Melody]);
    }

    /// Decks A and B playing on grids of `bpm` at `phase`.
    fn strip(a: (f64, f64), b: (f64, f64)) -> PhaseStrip {
        let mut v = loaded(loaded(view(), 1, 30.0, true), 0, 30.0, true);
        for (d, (bpm, phase)) in [a, b].into_iter().enumerate() {
            (v.status.decks[d].bpm, v.status.decks[d].beat_phase) = (Some(bpm), Some(phase));
        }
        phase_strip(&v)
    }

    #[test]
    fn decks_at_different_tempos_are_told_when_they_will_be_heard_off_the_beat() {
        assert_eq!(strip((120.0, 0.5), (150.0, 0.5)).reading, Reading::Drifting { seconds: 0 });
        assert_eq!(strip((128.0, 0.5), (128.1, 0.5)).reading, Reading::Drifting { seconds: 38 });
    }

    #[test]
    fn decks_at_one_tempo_are_told_whether_their_beats_meet() {
        assert_eq!(strip((128.0, 0.5), (128.0, 0.5)).reading, Reading::Matched);
        assert_eq!(strip((128.0, 0.5), (128.0, 0.6)).reading, Reading::Ahead { ms: 47 });
        assert_eq!(strip((128.0, 0.5), (128.0, 0.4)).reading, Reading::Behind { ms: 47 });
    }

    #[test]
    fn the_strip_compares_the_first_two_decks_playing_else_a_and_b_both_playing() {
        let mut v = view();
        v.decks_in_use = 4;
        v.status.deck_count = 4;
        let v = loaded(loaded(v, 3, 30.0, true), 1, 30.0, true);
        assert_eq!(phase_strip(&v).decks, [1, 3]);
        let one = loaded(view(), 1, 30.0, true);
        assert_eq!(phase_strip(&one), PhaseStrip { decks: [0, 1], reading: Reading::Off });
    }

    #[test]
    fn a_grid_aligns_and_a_key_matches_with_the_deck_mixed_with_once_both_are_known() {
        let mut v = loaded(loaded(view(), 1, 0.0, true), 0, 0.0, true);
        v.status.decks[0].beat_phase = Some(0.3);
        assert!(!deck(&v, 0).can_align_grid, "the other's beat not known");
        v.status.decks[1].beat_phase = Some(0.1);
        assert!(deck(&v, 0).can_align_grid);
        v.status.decks[1].playing = false;
        assert!(!deck(&v, 0).can_align_grid, "the other stopped");
        assert!(!deck(&v, 0).can_sync_key);
        for d in [0, 1] {
            if let Some(track) = &mut v.decks[d].track {
                track.key = Some(camelot("5B"));
            }
        }
        assert!(deck(&v, 0).can_sync_key);
    }
}
