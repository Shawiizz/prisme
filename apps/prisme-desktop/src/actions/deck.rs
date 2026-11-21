//! A deck's own actions: the jog wheels, played in time (a jog searching moves through the
//! track quickly, under shift say), the tempo fader, and the deck leading the others.

use super::edits::GridChange;
use super::{fx, mixer, take_over, transport, Performing};
use crate::auto_dj::Takeover;
use crate::dto::{RpmDto, StemMixDto, VinylChangeDto, VinylModeDto};
use crate::error::{AppError, AppResult};
use crate::preferences::{DeckPreferences, VinylSync};
use crate::state::{deck_commands, lock, AppState, DeckSettings, LoadedTrack};
use prisme_actions::input::Input;
use prisme_actions::screen::Drive;
use prisme_actions::sync::{kept_tempo, matching_tempo};
use prisme_actions::view::{mixed_with, tempo_rate};
use prisme_control::{DeckAction, StemPart};
use prisme_engine::Command;
use tracing::warn;

/// Seconds of track a turn of a jog wheel goes through in a search.
const SEARCH_SECONDS_PER_TURN: f64 = 10.0;
/// How far a key moves, either way.
pub const MAX_KEY_SHIFT: u8 = 12;

/// A jog wheel of `deck` turned, touched or let go (`action`) by `input`, its turns not
/// added up as an encoder's steps: they come as finely as the wheel tells them.
pub(super) fn turn(doing: &Performing, deck: usize, action: DeckAction, input: Input) -> AppResult<()> {
    let steps_per_turn = match doing.source() {
        super::Source::Controller(moved) => moved.jog_steps_per_turn,
        super::Source::Interface { .. } => prisme_control::JOG_STEPS_PER_TURN,
    };
    let jog_scratch = lock(&doing.state.decks)[deck].jog_scratch;
    let Some(command) = jog(deck, action, input, doing.at_s(), steps_per_turn, jog_scratch) else {
        return Ok(());
    };
    // a hand let go of the wheel takes nothing over
    if !matches!(command, Command::JogTouch { touched: false, .. }) {
        take_over(doing.auto_dj().as_deref(), Takeover::Transport(deck));
    }
    doing.state.send_if_running(command)
}

/// The command for a jog wheel of `deck` turned or touched by `input` at `at_s`, a turn of it
/// sending `steps_per_turn`, its top holding the record or not (`jog_scratch`). None for a
/// touch of a wheel that only nudges; a hand let go always is, the wheel maybe switched to
/// nudging while it held the record.
fn jog(
    deck: usize,
    action: DeckAction,
    input: Input,
    at_s: f64,
    steps_per_turn: f64,
    jog_scratch: bool,
) -> Option<Command> {
    let turns = f64::from(input.amount()) / steps_per_turn;
    match action {
        DeckAction::Jog => Some(Command::Jog { deck, turns, at_s }),
        // however lightly: some wheels tell how hard they are touched; a wheel that only
        // nudges: its top is no hand on the record
        DeckAction::JogTouch => {
            let touched = input.amount() > 0.0;
            (jog_scratch || !touched).then_some(Command::JogTouch { deck, touched, at_s })
        }
        DeckAction::JogSearch => Some(Command::SearchBy { deck, seconds: turns * SEARCH_SECONDS_PER_TURN }),
        _ => None,
    }
}

/// The tempo of `deck`'s fader, as a ratio to its track's; kept as the audio stops.
pub fn set_pitch(state: &AppState, deck: usize, ratio: f64) -> AppResult<()> {
    state.deck_in_use(deck)?;
    state.decks.change(|decks| decks[deck].pitch = ratio);
    state.send_if_running(Command::SetPitch { deck, ratio })
}

/// The key of the track on `deck` moved by `semitones`, until the next load.
pub fn set_key_shift(state: &AppState, deck: usize, semitones: i8) -> AppResult<()> {
    if semitones.unsigned_abs() > MAX_KEY_SHIFT {
        return Err(AppError::Other(format!("a key shift is ±{MAX_KEY_SHIFT} semitones at most")));
    }
    state.change_track(deck, |track| track.key_shift = semitones)?;
    state.send_if_running(Command::SetKeyShift { deck, semitones })
}

/// How the track on `deck` plays each part, until the next load: the parts it turns on or off
/// from the next bar if `on_bar` (see `Command::SetStemMix`).
pub fn set_stem_mix(state: &AppState, deck: usize, mix: StemMixDto, on_bar: bool) -> AppResult<()> {
    state.change_track(deck, |t| t.stem_mix = mix)?;
    state.send_if_running(Command::SetStemMix { deck, mix: mix.into(), on_bar })
}

/// What follows a track put on `deck`, by the DJ or the Auto DJ: the effects back on the whole
/// deck if they were on a part the track does not have, and the deck alone in the headphones if
/// the DJ asks for it. The track stays loaded whatever fails of it.
pub fn loaded(state: &AppState, deck: usize) {
    let follow = || {
        let separation = state.with_track(deck, |t| t.separation())?;
        if lock(&state.decks)[deck].fx.part.is_some_and(|part| !separation.has(part)) {
            fx::set_fx_part(state, deck, None)?;
        }
        if state.preferences.read(|p| p.dj.cue_on_load) {
            mixer::solo_cue(state, deck)?;
        }
        AppResult::Ok(())
    };
    if let Err(e) = follow() {
        warn!("what follows the load of deck {} not done: {e}", deck + 1);
    }
}

/// `part` of the track on `deck` muted, or played again: on the next bar with quantize; nothing
/// on a track without it.
pub fn toggle_stem(state: &AppState, deck: usize, part: StemPart) -> AppResult<()> {
    let mix = |t: &LoadedTrack| t.separation().has(part).then(|| t.stem_mix.toggled(part));
    match state.with_track(deck, mix) {
        Ok(Some(mix)) => set_stem_mix(state, deck, mix, state.preferences.read(|p| p.dj.quantize)),
        _ => Ok(()),
    }
}

/// How loud `part` of the track on `deck` plays, 0 to 1: nothing on a track without it.
pub fn set_stem_level(state: &AppState, deck: usize, part: StemPart, level: f32) -> AppResult<()> {
    let mix = |t: &LoadedTrack| t.separation().has(part).then(|| t.stem_mix.with_level(part, level));
    match state.with_track(deck, mix) {
        Ok(Some(mix)) => set_stem_mix(state, deck, mix, false),
        _ => Ok(()),
    }
}

/// The deck chosen to lead the others; `None`: the engine picks it.
pub fn set_sync_leader(state: &AppState, deck: Option<usize>) -> AppResult<()> {
    if let Some(d) = deck {
        state.deck_in_use(d)?;
    }
    *lock(&state.sync_leader) = deck;
    state.send(Command::SetSyncLeader { deck })
}

/// `deck` leads the others, or, leading, lets the engine pick the leader again.
pub fn toggle_leader(state: &AppState, deck: usize) -> AppResult<()> {
    let leads = state.deck_status(deck).is_ok_and(|status| status.sync.leader);
    set_sync_leader(state, (!leads).then_some(deck))
}

/// Whether a touch of the controller's jog wheel of `deck` holds the record (else the wheel
/// only nudges it).
pub fn set_jog_scratch(state: &AppState, deck: usize, on: bool) -> AppResult<()> {
    AppState::deck_exists(deck)?;
    lock(&state.decks)[deck].jog_scratch = on;
    Ok(())
}

/// One setting of the vinyl control of `deck` changed, the others as they are; kept for the
/// next start.
pub fn set_vinyl(state: &AppState, deck: usize, change: VinylChangeDto) -> AppResult<()> {
    AppState::deck_exists(deck)?;
    if let VinylChangeDto::LeadIn(Some(s)) = change {
        if !(s.is_finite() && s >= 0.0) {
            return Err(AppError::Other(format!("a lead-in of {s} s: it is 0 or more")));
        }
    }
    let settings = change_vinyl(state, deck, |(_, mode, rpm, lead_in_s)| match change {
        VinylChangeDto::Mode(m) => *mode = m,
        VinylChangeDto::Rpm(r) => *rpm = r,
        VinylChangeDto::LeadIn(s) => *lead_in_s = s,
    });
    send_deck(state, deck, &settings)
}

/// The vinyl control of `deck` put on or off, kept for the next start. Put on, it takes the
/// turntable from the deck layered with it there (see `audio_host::deck_pair`), which plays on
/// by itself; a deck a record lets go of plays on at the record's tempo.
pub fn set_vinyl_enabled(state: &AppState, deck: usize, on: bool) -> AppResult<()> {
    AppState::deck_exists(deck)?;
    if !on {
        state.let_go_of_record(deck);
    }
    let settings = change_vinyl(state, deck, |(enabled, ..)| *enabled = on);
    if on {
        state.take_turntable(deck);
    }
    send_deck(state, deck, &settings)
}

/// Where a deck's vinyl settings are kept: whether it is on, its mode, speed and lead-in.
type VinylSettings<'a> = (&'a mut bool, &'a mut VinylModeDto, &'a mut RpmDto, &'a mut Option<f64>);

/// The vinyl settings of `deck` changed by `change`, on the deck and in the preferences: the
/// settings of the deck now.
fn change_vinyl(state: &AppState, deck: usize, change: impl Fn(VinylSettings)) -> DeckSettings {
    let settings = {
        let mut decks = lock(&state.decks);
        let d = &mut decks[deck];
        change((&mut d.vinyl_enabled, &mut d.vinyl_mode, &mut d.rpm, &mut d.lead_in_s));
        *d
    };
    state.update_preferences(|p| {
        let d = &mut p.decks[deck];
        change((&mut d.vinyl_enabled, &mut d.vinyl_mode, &mut d.rpm, &mut d.lead_in_s));
    });
    settings
}

/// The engine told `deck` is set as `settings` say.
fn send_deck(state: &AppState, deck: usize, settings: &DeckSettings) -> AppResult<()> {
    deck_commands(deck, settings).into_iter().try_for_each(|c| state.send_if_running(c))
}

/// Who drives `deck`: itself, its record (relative or absolute) or its input as it comes. Its
/// input leaves the record's settings as they were, for when the deck plays its track again.
pub fn set_mode(state: &AppState, deck: usize, mode: Drive) -> AppResult<()> {
    AppState::deck_exists(deck)?;
    let thru = mode == Drive::Thru;
    if lock(&state.decks)[deck].passthrough != thru {
        set_passthrough(state, deck, thru)?;
    }
    let vinyl = match mode {
        Drive::Thru => return Ok(()),
        Drive::Internal => return set_vinyl_enabled(state, deck, false),
        Drive::Relative => VinylModeDto::Relative,
        Drive::Absolute => VinylModeDto::Absolute,
    };
    set_vinyl(state, deck, VinylChangeDto::Mode(vinyl))?;
    set_vinyl_enabled(state, deck, true)
}

/// The deck plays its input as it comes (a record, a CD player) instead of its track.
pub fn set_passthrough(state: &AppState, deck: usize, enabled: bool) -> AppResult<()> {
    state.set_deck(deck, |d| d.passthrough = enabled, Command::SetPassthrough { deck, enabled })
}

/// The range of the tempo fader of `deck` (0.08 = ±8 %), or of every deck (`None`), which
/// then all follow the general one; kept for the next start.
pub fn set_tempo_range(state: &AppState, deck: Option<usize>, range: f64) -> AppResult<()> {
    if !(range > 0.0 && range <= 1.0) {
        return Err(AppError::Other("a tempo range is within ±100 %".into()));
    }
    match deck {
        Some(deck) => {
            AppState::deck_exists(deck)?;
            lock(&state.decks)[deck].pitch_range = Some(range);
            state.update_preferences(|p| p.decks[deck].pitch_range = Some(range));
        }
        None => {
            lock(&state.decks).iter_mut().for_each(|d| d.pitch_range = None);
            state.update_preferences(|p| {
                p.dj.pitch_range = range;
                p.decks.iter_mut().for_each(|d| d.pitch_range = None);
            });
        }
    }
    Ok(())
}

/// The deck's size in beats, what its jumps go by and its sized loop lasts; kept for the next
/// start.
fn set_deck_size(state: &AppState, deck: usize, beats: f64) -> AppResult<()> {
    AppState::deck_exists(deck)?;
    if !DeckPreferences::SIZE_BEATS.contains(&beats) {
        return Err(AppError::Other("a deck's size is from 1/16 of a beat to 64 beats".into()));
    }
    lock(&state.decks)[deck].size_beats = beats;
    state.update_preferences(|p| p.decks[deck].size_beats = beats);
    Ok(())
}

/// At the end, the track goes on from its start.
pub fn set_repeat(state: &AppState, deck: usize, enabled: bool) -> AppResult<()> {
    state.set_deck(deck, |d| d.repeat = enabled, Command::SetRepeat { deck, enabled })
}

/// The pitch stays when the speed changes; kept for the next start.
pub fn set_key_lock(state: &AppState, deck: usize, enabled: bool) -> AppResult<()> {
    state.set_deck(deck, |d| d.key_lock = enabled, Command::SetKeyLock { deck, enabled })?;
    state.update_preferences(|p| p.decks[deck].key_lock = enabled);
    Ok(())
}

/// Locked to the leader, or free.
fn set_sync_locked(state: &AppState, deck: usize, locked: bool) -> AppResult<()> {
    state.deck_in_use(deck)?;
    state.send(Command::SetSyncLocked { deck, locked })
}

/// The leader's tempo and beats matched once.
fn sync_once(state: &AppState, deck: usize) -> AppResult<()> {
    state.deck_in_use(deck)?;
    state.send(Command::SyncOnce { deck })
}

/// SYNC on, locked to the leader (or matched once, on a record in one-shot mode), or off.
pub fn set_sync(state: &AppState, deck: usize, on: bool) -> AppResult<()> {
    if !on {
        return unlock(state, deck);
    }
    let record = lock(&state.decks)[deck].vinyl_enabled;
    if record && state.preferences.read(|p| p.dj.vinyl_sync == VinylSync::Once) {
        sync_once(state, deck)
    } else {
        set_sync_locked(state, deck, true)
    }
}

/// The leader's tempo and beats matched once, the deck left free (unlocked first). A deck
/// playing by itself takes the tempo on its fader, which then says what it plays.
pub fn match_once(state: &AppState, deck: usize) -> AppResult<()> {
    let status = state.engine_status()?;
    if status.decks[deck].sync.locked {
        unlock(state, deck)?;
    }
    if !lock(&state.decks)[deck].vinyl_enabled {
        if let Some(ratio) = matching_tempo(&status, deck) {
            set_pitch(state, deck, ratio)?;
        }
    }
    sync_once(state, deck)
}

/// SYNC off. A deck playing by itself keeps the tempo it was locked to: its fader is moved
/// there first (a locked deck does not follow it), and a controller's picks it up there.
fn unlock(state: &AppState, deck: usize) -> AppResult<()> {
    if let Some(ratio) = kept_tempo(&state.deck_status(deck)?) {
        set_pitch(state, deck, ratio)?;
    }
    set_sync_locked(state, deck, false)
}

/// The deck's size multiplied by `factor`, within 1/16 of a beat and 64 beats.
pub fn scale_size(state: &AppState, deck: usize, factor: f64) -> AppResult<()> {
    let beats = lock(&state.decks)[deck].size_beats * factor;
    let sizes = DeckPreferences::SIZE_BEATS;
    set_deck_size(state, deck, beats.clamp(*sizes.start(), *sizes.end()))
}

/// The track's key moved to `semitones`, within 12 either way: nothing without a track.
pub fn key_to(state: &AppState, deck: usize, semitones: i32) -> AppResult<()> {
    if state.with_track(deck, |_| ()).is_err() {
        return Ok(());
    }
    let most = i32::from(MAX_KEY_SHIFT);
    set_key_shift(state, deck, semitones.clamp(-most, most) as i8)
}

impl Performing<'_> {
    /// The track on `deck` run through while `held`, back (`way` -1) or forward (1).
    pub(super) fn search(&self, deck: usize, way: f64, held: bool) -> AppResult<()> {
        if held {
            self.take_over_transport(deck);
        }
        let speed = if held { way * transport::SEARCH_SPEED } else { 0.0 };
        transport::send(self.state, deck, Command::Search { deck, speed })
    }

    /// The filter of `deck` heard or not: one heard is set aside, whatever was set aside
    /// before; a silent one gets back the one set aside.
    pub(super) fn toggle_filter(&self, deck: usize) -> AppResult<()> {
        let filter = lock(&self.state.decks)[deck].strip.filter;
        let to = {
            let aside = &mut self.memory().decks[deck].filter_aside;
            if filter != 0.0 {
                *aside = Some(filter);
                0.0
            } else {
                match aside.take() {
                    Some(aside) => aside,
                    None => return Ok(()),
                }
            }
        };
        mixer::set_filter(self.state, deck, to)
    }

    /// The tempo of the track on `deck` tapped on its key: its beat grid at that tempo, once
    /// there are enough taps.
    pub(super) fn tap(&self, deck: usize) -> AppResult<()> {
        match self.actions.tapped(deck, self.at_s()) {
            Some(bpm) => self.change_grid(deck, GridChange::Bpm { bpm }),
            None => Ok(()),
        }
    }

    /// The key of the track on `deck` moved by the fewest semitones that make it mix with the
    /// key the deck it is mixed with is heard in: nothing unless both keys are known.
    pub(super) fn sync_key(&self, deck: usize) -> AppResult<()> {
        let state = self.state;
        let Ok(status) = state.engine_status() else { return Ok(()) };
        let other = mixed_with(&status, deck);
        let key = |d: usize| state.with_track(d, |t| (t.analysis().key.map(|k| k.key), t.key_shift)).ok();
        let (Some((Some(own), _)), Some((Some(theirs), shift))) = (key(deck), key(other)) else {
            return Ok(());
        };
        let d = lock(&state.decks)[other];
        let rate = tempo_rate(&status.decks[other], d.pitch);
        key_to(state, deck, own.semitones_to_mix(theirs.heard(d.key_lock, shift, rate)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::bench::Bench;
    use crate::actions::Source;
    use crate::dto::RpmDto;

    fn turned(deck: usize, action: DeckAction, steps: f32, at_s: f64, jog_scratch: bool) -> Option<Command> {
        let input = Input::of(steps, action != DeckAction::JogTouch, None);
        jog(deck, action, input, at_s, 720.0, jog_scratch)
    }

    #[test]
    fn a_jog_turns_by_its_steps_on_its_controllers_clock() {
        let turned_back = turned(1, DeckAction::Jog, -180.0, 2.5, true);
        assert!(
            matches!(turned_back, Some(Command::Jog { deck: 1, turns, at_s }) if turns == -0.25 && at_s == 2.5)
        );
        let touched = turned(0, DeckAction::JogTouch, 0.2, 3.0, true);
        assert!(matches!(touched, Some(Command::JogTouch { deck: 0, touched: true, .. })));
        let let_go = turned(0, DeckAction::JogTouch, 0.0, 3.5, true);
        assert!(matches!(let_go, Some(Command::JogTouch { deck: 0, touched: false, .. })));
        assert!(
            turned(0, DeckAction::JogTouch, 1.0, 4.0, false).is_none(),
            "a wheel that only nudges holds nothing"
        );
        let switched = turned(0, DeckAction::JogTouch, 0.0, 4.5, false);
        assert!(
            matches!(switched, Some(Command::JogTouch { deck: 0, touched: false, .. })),
            "let go after the wheel was switched to nudging"
        );
    }

    #[test]
    fn a_jog_searches_seconds_a_turn() {
        let searched = turned(0, DeckAction::JogSearch, 360.0, 0.0, true);
        assert!(matches!(searched, Some(Command::SearchBy { deck: 0, seconds }) if seconds == 5.0));
    }

    /// The ratio the tempo of deck A was set to by its fader moved to `value` by `device`.
    fn pitch(bench: &Bench, device: &str, value: f32) -> Option<f64> {
        bench.moved(device, "deck1.pitch", Input::Value(value)).unwrap();
        bench.sent().into_iter().find_map(|c| match c {
            Command::SetPitch { deck: 0, ratio } => Some(ratio),
            _ => None,
        })
    }

    #[test]
    fn a_tempo_fader_picks_the_tempo_up_before_moving_it() {
        let bench = Bench::new();
        // the screen left the tempo at +4 %: the fader at -8 % does not jump it there
        set_pitch(&bench.state, 0, 1.04).unwrap();
        bench.sent();
        assert_eq!(pitch(&bench, "DDJ", 0.0), None);
        assert_eq!(pitch(&bench, "DDJ", 0.5), None);
        let picked = pitch(&bench, "DDJ", 0.8).unwrap();
        assert!((picked - 1.048).abs() < 1e-6);
        assert!((pitch(&bench, "DDJ", 1.0).unwrap() - 1.08).abs() < 1e-6);
        assert!((lock(&bench.state.decks)[0].pitch - 1.08).abs() < 1e-6, "kept");
    }

    #[test]
    fn two_controllers_on_one_deck_each_pick_its_tempo_up_and_the_interface_sets_it() {
        let bench = Bench::new();
        assert!(pitch(&bench, "DDJ", 0.5).is_some());
        assert_eq!(pitch(&bench, "CDJ", 0.9), None, "its fader is elsewhere");
        assert!(pitch(&bench, "DDJ", 0.55).is_some());
        bench.perform("deck1.pitch", Input::Value(1.0), Source::Interface { confirmed: false }).unwrap();
        assert!((lock(&bench.state.decks)[0].pitch - 1.08).abs() < 1e-6);
    }

    #[test]
    fn a_vinyl_setting_changes_alone_whatever_else_changed_meanwhile() {
        let bench = Bench::new();
        set_vinyl(&bench.state, 1, VinylChangeDto::Mode(VinylModeDto::Relative)).unwrap();
        set_vinyl(&bench.state, 1, VinylChangeDto::Rpm(RpmDto::Rpm45)).unwrap();
        bench.press("deck2.vinyl.on", 1.0).unwrap();
        set_vinyl(&bench.state, 1, VinylChangeDto::LeadIn(Some(12.0))).unwrap();
        assert!(set_vinyl(&bench.state, 1, VinylChangeDto::LeadIn(Some(-1.0))).is_err(), "before the record");
        let d = lock(&bench.state.decks)[1];
        assert_eq!(
            (d.vinyl_enabled, d.vinyl_mode, d.rpm, d.lead_in_s),
            (true, VinylModeDto::Relative, RpmDto::Rpm45, Some(12.0))
        );
        let kept = bench.state.preferences.read(|p| p.decks[1].clone());
        assert_eq!(
            (kept.vinyl_enabled, kept.vinyl_mode, kept.rpm, kept.lead_in_s),
            (true, VinylModeDto::Relative, RpmDto::Rpm45, Some(12.0))
        );
    }

    #[test]
    fn a_deck_handed_to_its_input_keeps_its_record_for_later() {
        let bench = Bench::new();
        let mode = |bench: &Bench| {
            let d = lock(&bench.state.decks)[1];
            (d.passthrough, d.vinyl_enabled, d.vinyl_mode)
        };
        set_mode(&bench.state, 1, Drive::Relative).unwrap();
        assert_eq!(mode(&bench), (false, true, VinylModeDto::Relative));
        set_mode(&bench.state, 1, Drive::Thru).unwrap();
        assert_eq!(mode(&bench), (true, true, VinylModeDto::Relative));
        set_mode(&bench.state, 1, Drive::Absolute).unwrap();
        assert_eq!(mode(&bench), (false, true, VinylModeDto::Absolute));
        set_mode(&bench.state, 1, Drive::Internal).unwrap();
        assert_eq!(mode(&bench), (false, false, VinylModeDto::Absolute));
    }

    #[test]
    fn a_deck_taking_its_turntable_is_told_after_the_deck_it_takes_it_from() {
        let bench = Bench::new();
        // four decks on two turntables: C on A's
        lock(&bench.state.session).as_mut().unwrap().info.decks = 4;
        lock(&bench.state.decks)[2].vinyl_enabled = true;
        bench.press("deck1.vinyl.on", 1.0).unwrap();
        let turned: Vec<_> = bench
            .sent()
            .into_iter()
            .filter_map(|c| match c {
                Command::SetVinylEnabled { deck, enabled } => Some((deck, enabled)),
                _ => None,
            })
            .collect();
        assert_eq!(turned, [(2, false), (0, true)]);
    }

    #[test]
    fn a_deck_is_chosen_to_lead_the_others() {
        let bench = Bench::new();
        bench.press("deck2.sync.leader", 1.0).unwrap();
        bench.press("deck2.sync.leader", 0.0).unwrap();
        assert!(matches!(bench.sent()[..], [Command::SetSyncLeader { deck: Some(1) }]));
        assert_eq!(*lock(&bench.state.sync_leader), Some(1));
    }

    fn press(bench: &Bench, id: &str) {
        bench.press(id, 1.0).unwrap();
        bench.press(id, 0.0).unwrap();
    }

    #[test]
    fn a_search_runs_through_the_track_while_held() {
        let bench = Bench::new();
        bench.load(0, 60.0, None);
        bench.press("deck1.play", 1.0).unwrap();
        bench.press("deck1.search.forward", 1.0).unwrap();
        bench.run(1.0);
        assert!((bench.status(0).position_s - 9.0).abs() < 0.1);
        bench.press("deck1.search.forward", 0.0).unwrap();
        bench.run(1.0);
        assert!((bench.status(0).position_s - 10.0).abs() < 0.1, "let go: played on");
        bench.press("deck1.search.back", 1.0).unwrap();
        bench.run(1.0);
        assert!((bench.status(0).position_s - 3.0).abs() < 0.1);
    }

    #[test]
    fn the_filter_heard_is_set_aside_even_one_moved_since_the_last_was() {
        let bench = Bench::new();
        let filter = || lock(&bench.state.decks)[0].strip.filter;
        crate::actions::mixer::set_filter(&bench.state, 0, 0.6).unwrap();
        press(&bench, "deck1.filter.toggle");
        assert_eq!(filter(), 0.0);
        crate::actions::mixer::set_filter(&bench.state, 0, -0.3).unwrap();
        press(&bench, "deck1.filter.toggle");
        assert_eq!(filter(), 0.0);
        press(&bench, "deck1.filter.toggle");
        assert_eq!(filter(), -0.3);
        press(&bench, "deck1.filter.toggle");
        press(&bench, "deck1.filter.toggle");
        assert_eq!(filter(), -0.3, "the one set aside last");
    }

    #[test]
    fn the_grid_editor_s_taps_give_the_tempo_without_setting_it() {
        let bench = Bench::new();
        bench.load(0, 60.0, Some(120.0));
        let tap = |at_s| bench.actions.tapped(0, at_s);
        assert_eq!([0.0, 0.48, 0.96].map(tap), [None, None, None]);
        assert_eq!(tap(1.44), Some(125.0));
        assert_eq!(tap(9.0), None, "a pause starts the count again");
        let bpm = bench.state.with_track(0, |t| t.grid().map(|g| g.bpm)).unwrap();
        assert_eq!(bpm, Some(120.0), "for the DJ to apply");
    }

    /// A track on `deck` in `camelot` (a key of the wheel), playing or not.
    fn keyed(bench: &Bench, deck: usize, camelot: &str, playing: bool) {
        bench.load(deck, 60.0, Some(120.0));
        let key = prisme_analysis::Key::from_camelot(camelot).unwrap();
        lock(&bench.state.tracks)[deck].as_mut().unwrap().detected.key =
            Some(prisme_analysis::KeyEstimate { key, confidence: 1.0, tuning_cents: 0.0 });
        if playing {
            bench.press(&format!("deck{}.play", deck + 1), 1.0).unwrap();
            bench.run(0.01);
        }
    }

    #[test]
    fn the_key_is_brought_to_mix_with_the_deck_heard_beside_it() {
        let bench = Bench::new();
        keyed(&bench, 0, "8A", false);
        keyed(&bench, 1, "10A", true);
        press(&bench, "deck1.key.sync");
        let shift = bench.state.with_track(0, |t| t.key_shift).unwrap();
        let heard = prisme_analysis::Key::from_camelot("8A").unwrap().transposed(i32::from(shift));
        assert!(heard.mixes_with(prisme_analysis::Key::from_camelot("10A").unwrap()), "{shift}");
        assert_ne!(shift, 0);
    }

    #[test]
    fn a_stem_is_muted_and_played_again_on_a_separated_track_alone() {
        let bench = Bench::new();
        bench.load(0, 1.0, None);
        press(&bench, "deck1.vocals");
        assert!(
            bench.state.with_track(0, |t| t.stem_mix.playing(StemPart::Vocals)).unwrap(),
            "not separated"
        );
        let stem = std::sync::Arc::new(prisme_media::TrackAudio::from_interleaved(48_000, vec![0.0; 2]));
        lock(&bench.state.tracks)[0].as_mut().unwrap().stems[prisme_engine::Stem::Vocals as usize] =
            Some(stem);
        press(&bench, "deck1.vocals");
        assert!(!bench.state.with_track(0, |t| t.stem_mix.playing(StemPart::Vocals)).unwrap());
        press(&bench, "deck1.vocals");
        assert!(bench.state.with_track(0, |t| t.stem_mix.playing(StemPart::Vocals)).unwrap());
    }

    #[test]
    fn a_track_loaded_takes_the_effects_off_a_part_it_lacks_and_may_be_heard_alone() {
        let bench = Bench::new();
        crate::actions::fx::set_fx_part(&bench.state, 1, Some(prisme_control::StemPart::Vocals)).unwrap();
        crate::actions::mixer::set_deck_cue(&bench.state, 0, true).unwrap();
        bench.load(1, 1.0, None);
        loaded(&bench.state, 1);
        assert_eq!(lock(&bench.state.decks)[1].fx.part, None, "not separated");
        assert_eq!(
            lock(&bench.state.decks).map(|d| d.cue)[..2],
            [true, false],
            "the headphones as they were"
        );
        bench.state.update_preferences(|p| p.dj.cue_on_load = true);
        loaded(&bench.state, 1);
        assert_eq!(lock(&bench.state.decks).map(|d| d.cue)[..2], [false, true]);
    }
}
