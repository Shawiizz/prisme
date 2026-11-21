//! What the light of an action shows, read from a [`View`]: on or off, or a state of its own
//! (a cue paused, a sample loaded or playing, the end of a track soon), and the colour of a
//! hot cue. A key whose action can do nothing stays dark (see `available`), and one a record
//! holding its deck in place refuses (see `record`).

use crate::available::available;
use crate::cue::at_cue;
use crate::pads::{plays_loop, slice_at, ROLL_BEATS};
use crate::record::refuses;
use crate::screen::{drive, Drive};
use crate::switch::Switch;
use crate::sync::{phase, Beat, Phase, IN_PHASE};
use crate::view::{band_index, turns, TrackView, View};
use prisme_control::{
    AutoDjAction, Beats, DeckAction, FxAction, GeneralAction, LibraryAction, MixerAction, SampleAction,
    Target, Turn,
};
use prisme_engine::DeckStatus;
use serde::Serialize;

/// The share of a beat its light shows, from its start.
const BEAT_LIT: f64 = 0.25;
/// The end of a track near (s), and soon (s).
const ENDING_S: f64 = 30.0;
const ENDING_SOON_S: f64 = 10.0;
/// A deck this close to its track's tempo (ratio) is at it.
const AT_TEMPO: f64 = 0.0005;
/// Two decks this close in tempo (BPM) are at the same.
const SAME_BPM: f64 = 0.01;

/// The state a light shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "LightState"))]
pub enum State {
    On,
    Off,
    /// A cue: the deck stopped away from its cue point.
    Paused,
    /// A sample slot holding a sound, a deck holding a track.
    Loaded,
    Playing,
    /// The end of a track within a few seconds.
    Soon,
}

impl State {
    /// As the profiles name it.
    pub fn name(self) -> &'static str {
        match self {
            State::On => prisme_control::ON,
            State::Off => prisme_control::OFF,
            State::Paused => "paused",
            State::Loaded => "loaded",
            State::Playing => "playing",
            State::Soon => "soon",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Light {
    pub state: State,
    /// What it shows the colour of (a hot cue's), `0xrrggbb`.
    pub colour: Option<u32>,
}

impl Light {
    pub const OFF: Light = Light::of(State::Off);

    const fn of(state: State) -> Self {
        Light { state, colour: None }
    }

    fn lit(on: bool) -> Self {
        Light::of(if on { State::On } else { State::Off })
    }
}

/// The light of `target` as `view` shows it, if it has one; off for one that can do nothing
/// (of a deck not in use, a stem not separated…) and for a key its record refuses.
pub fn light(target: &Target, view: &View) -> Option<Light> {
    let light = match (Switch::of(target), target) {
        (Some((switch, turn)), _) => Some(Light::lit(shows(turn, switch.is_on(view)))),
        (None, Target::Deck(deck, action)) => deck_light(*deck, *action, view),
        (None, Target::Mixer(MixerAction::MainClip)) => Some(Light::lit(view.perform.main_clipping)),
        (None, Target::Library(LibraryAction::Load(deck))) => {
            Some(Light::of(deck_state(view.deck_status(*deck))))
        }
        (None, Target::Library(LibraryAction::Preview)) => {
            Some(Light::lit(view.status.preview.is_some() || view.preview_opening))
        }
        (None, Target::General(GeneralAction::Record)) => Some(Light::lit(view.status.recording.is_some())),
        (None, Target::General(GeneralAction::Library)) => Some(Light::lit(view.perform.library_only)),
        (None, Target::AutoDj(AutoDjAction::Toggle)) => Some(Light::lit(view.auto_dj)),
        _ => None,
    }?;
    let dark = !available(view, target)
        || match *target {
            Target::Deck(deck, action) => dark_under_record(view, deck, action),
            _ => target.deck().is_some_and(|deck| !view.shows(deck)),
        };
    Some(if dark { Light::OFF } else { light })
}

/// Whether the key of `action` of `deck` stays dark, a record holding the deck in place: one
/// it refuses but PLAY, lit while the record turns; a hot cue, which it would jump to; the
/// light of a loop, which none plays then.
fn dark_under_record(view: &View, deck: usize, action: DeckAction) -> bool {
    action != DeckAction::Play
        && (action == DeckAction::LoopAny || refuses(action, |_| true))
        && drive(view, deck) == Drive::Absolute
}

/// Whether a switch's key turning it `turn` is lit, the switch `on` or not: its key turning it
/// off while it is off.
fn shows(turn: Turn, on: bool) -> bool {
    if turn == Turn::Off {
        !on
    } else {
        on
    }
}

fn deck_light(deck: usize, action: DeckAction, view: &View) -> Option<Light> {
    let status = view.deck_status(deck);
    let settings = &view.decks[deck];
    let track = settings.track.as_ref();
    let key_shift = track.map_or(0, |t| t.key_shift);
    let perform = &view.perform.decks[deck];
    let sample = |slot: usize| (status.samples_playing & 1 << slot != 0, settings.samples & 1 << slot != 0);
    let fx = |slot: usize| &settings.fx.slots[slot];
    // a deck on its record plays while the record turns, the deck's own play aside
    let playing = match drive(view, deck) {
        Drive::Relative | Drive::Absolute => turns(status),
        Drive::Internal | Drive::Thru => status.playing,
    };
    let lit = match action {
        DeckAction::Play => playing,
        DeckAction::Cue => return Some(Light::of(cue_state(status, track, playing))),
        DeckAction::HotCue(slot) => {
            return Some(match track.and_then(|t| t.hot_cues[slot]) {
                Some(colour) => Light { state: State::On, colour: Some(colour) },
                None => Light::OFF,
            })
        }
        DeckAction::Loop | DeckAction::LoopAny => status.looping.active,
        DeckAction::LoopIn => status.looping.loop_in_s.is_some(),
        DeckAction::LoopSize => plays_loop(&status.looping, settings.size_beats),
        DeckAction::LoopBeats(Beats(length)) => plays_loop(&status.looping, length),
        DeckAction::ReloopStop => status.looping.region.is_some(),
        DeckAction::KeyReset => key_shift != 0,
        DeckAction::KeyTo(semitones) => i32::from(key_shift) == semitones,
        DeckAction::ReverseHeld => status.speed < 0.0,
        DeckAction::SlipHeld => status.slip_mode,
        DeckAction::SyncLeader => status.sync.leader,
        DeckAction::Stem(part) => track.is_some_and(|t| t.stem_mix.playing(part)),
        DeckAction::Sample(slot, SampleAction::Trigger | SampleAction::Smart) => {
            return Some(Light::of(match sample(slot) {
                (true, _) => State::Playing,
                (false, true) => State::Loaded,
                (false, false) => State::Off,
            }))
        }
        DeckAction::Sample(slot, SampleAction::Held | SampleAction::Velocity) => sample(slot).0,
        DeckAction::Fx(slot, FxAction::Held) => fx(slot).enabled,
        DeckAction::Fx(slot, FxAction::Effect(effect)) => fx(slot).enabled && fx(slot).effect == effect,
        DeckAction::Fx(slot, FxAction::Focus) => perform.fx_focus == Some(slot),
        DeckAction::Roll(roll) => perform.roll == Some(ROLL_BEATS[roll]),
        DeckAction::RollBeats(Beats(beats)) => perform.roll == Some(beats),
        DeckAction::Slice(slice) => slice_playing(status, track, perform.slicer) == Some(slice),
        DeckAction::Cut(cut) => perform.cut == Some(cut),
        DeckAction::EqKill(band) | DeckAction::EqKillToggle(band) => settings.strip.kills[band_index(band)],
        DeckAction::FilterToggle => settings.strip.filter != 0.0,
        DeckAction::HeadphonesSolo => (0..view.decks_in_use).all(|d| view.decks[d].cue == (d == deck)),
        DeckAction::CrossfaderSide(side) => settings.side == side,
        DeckAction::Beat => playing && status.beat_phase.is_some_and(|p| p < BEAT_LIT),
        DeckAction::Clip => perform.clipping,
        DeckAction::Ending => return Some(Light::of(ending(status))),
        DeckAction::TempoZero => tempo(deck, view) == TempoLight::Zero,
        DeckAction::TempoSlower => tempo(deck, view) == TempoLight::Slower,
        DeckAction::TempoFaster => tempo(deck, view) == TempoLight::Faster,
        DeckAction::Phase => phase_of(deck, view) == Phase::In,
        DeckAction::PhaseAhead => phase_of(deck, view) == Phase::Ahead,
        DeckAction::PhaseBehind => phase_of(deck, view) == Phase::Behind,
        _ => return None,
    };
    Some(Light::lit(lit))
}

/// A CDJ's cue: on at the cue point, paused stopped elsewhere, off `playing` or empty.
fn cue_state(status: &DeckStatus, track: Option<&TrackView>, playing: bool) -> State {
    let cue_s = track.and_then(|t| t.main_cue_s).unwrap_or(0.0);
    if !status.loaded {
        State::Off
    } else if at_cue(status.position_s, cue_s) {
        State::On
    } else if playing {
        State::Off
    } else {
        State::Paused
    }
}

/// A deck as a load key shows it: empty, holding a track, playing it.
fn deck_state(status: &DeckStatus) -> State {
    match (status.loaded, status.playing) {
        (false, _) => State::Off,
        (true, true) => State::Playing,
        (true, false) => State::Loaded,
    }
}

/// The slice of the slicer starting at `slicer` a deck at `status` plays in, if it does.
pub(crate) fn slice_playing(
    status: &DeckStatus,
    track: Option<&TrackView>,
    slicer: Option<f64>,
) -> Option<usize> {
    let beat_s = 60.0 / track?.bpm?;
    slice_at(slicer?, status.position_s, beat_s).filter(|_| status.loaded)
}

/// The end of the track of a deck at `status` near, or soon, while it plays.
pub(crate) fn ending(status: &DeckStatus) -> State {
    let left = status.duration_s - status.position_s;
    if !status.playing || status.duration_s <= 0.0 || left >= ENDING_S {
        State::Off
    } else if left < ENDING_SOON_S {
        State::Soon
    } else {
        State::On
    }
}

/// A deck's tempo against the deck it mixes with.
#[derive(Debug, PartialEq, Eq)]
enum TempoLight {
    /// At its track's own.
    Zero,
    Slower,
    Faster,
    /// The same, or not known.
    Off,
}

fn tempo(deck: usize, view: &View) -> TempoLight {
    if (view.tempo_rate(deck) - 1.0).abs() < AT_TEMPO {
        return TempoLight::Zero;
    }
    match (view.deck_status(deck).bpm, view.deck_status(view.mixed_with(deck)).bpm) {
        (Some(own), Some(other)) if (own - other).abs() >= SAME_BPM => {
            if own < other {
                TempoLight::Slower
            } else {
                TempoLight::Faster
            }
        }
        _ => TempoLight::Off,
    }
}

/// Where a deck's beats are against the leader's, another deck in use leading, else against
/// the deck it mixes with.
fn phase_of(deck: usize, view: &View) -> Phase {
    let leads = |d: &usize| *d != deck && *d < view.status.deck_count && view.deck_status(*d).sync.leader;
    let reference = (0..view.decks_in_use).find(leads).unwrap_or_else(|| view.mixed_with(deck));
    phase(Beat::playing(view.deck_status(deck)), Beat::playing(view.deck_status(reference)), IN_PHASE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stems::Separation;
    use crate::view::fixtures::{loaded, view};
    use prisme_control::StemPart;
    use prisme_engine::{LoopRegion, VinylMode};

    fn target(id: &str) -> Target {
        id.parse().unwrap_or_else(|e| panic!("{e}"))
    }

    fn state(id: &str, view: &View) -> Option<State> {
        light(&target(id), view).map(|l| l.state)
    }

    const ON: Option<State> = Some(State::On);
    const OFF: Option<State> = Some(State::Off);

    #[test]
    fn play_is_lit_while_its_deck_plays_and_an_action_without_a_light_shows_none() {
        let playing = loaded(view(), 0, 10.0, true);
        assert_eq!(state("deck1.play", &playing), ON);
        assert_eq!(state("deck2.play", &playing), OFF);
        assert_eq!(state("deck1.loop.halve", &playing), None);
        assert_eq!(state("deck1.jog", &playing), None);
    }

    #[test]
    fn the_lights_of_a_deck_not_in_use_are_off() {
        let mut four = view();
        four.decks_in_use = 4;
        four.status.deck_count = 4;
        let playing = loaded(four, 2, 10.0, true);
        assert_eq!(state("deck3.play", &playing), ON);
        assert_eq!(state("library.load3", &playing), Some(State::Playing));
        let two = View { decks_in_use: 2, ..playing };
        assert_eq!(state("deck3.play", &two), OFF);
        assert_eq!(state("library.load3", &two), OFF);
        assert_eq!(state("deck3.loop.halve", &two), None, "no light, shown or not");
    }

    #[test]
    fn a_cue_is_on_at_its_point_paused_away_from_it_off_playing() {
        let mut at_cue = loaded(view(), 0, 12.0, false);
        if let Some(track) = &mut at_cue.decks[0].track {
            track.main_cue_s = Some(12.01);
        }
        assert_eq!(state("deck1.cue", &at_cue), ON);
        let mut away = at_cue;
        away.status.decks[0].position_s = 20.0;
        assert_eq!(state("deck1.cue", &away), Some(State::Paused));
        away.status.decks[0].playing = true;
        assert_eq!(state("deck1.cue", &away), OFF);
        assert_eq!(state("deck1.cue", &loaded(view(), 0, 0.0, true)), ON, "no cue: the start");
        assert_eq!(state("deck1.cue", &view()), OFF, "empty");
    }

    /// Deck A on its record, away from its cue point, with a hot cue, a loop, a roll, a slip,
    /// a cut and an effect on, the record still.
    fn on_a_record(mode: VinylMode) -> View {
        let mut v = loaded(view(), 0, 30.0, false);
        (v.decks[0].vinyl, v.decks[0].vinyl_mode, v.decks[0].key_lock) = (true, mode, true);
        v.decks[0].fx.slots[0].enabled = true;
        if let Some(track) = &mut v.decks[0].track {
            track.hot_cues[0] = Some(0xff8000);
        }
        let status = &mut v.status.decks[0];
        status.looping.active = true;
        status.looping.beats = Some(4.0);
        (status.slip_mode, status.beat_phase) = (true, Some(0.1));
        v.perform.decks[0].roll = Some(0.5);
        v.perform.decks[0].cut = Some(1);
        v
    }

    #[test]
    fn under_a_record_in_abs_play_shows_it_turning_and_the_keys_it_refuses_are_dark() {
        let mut v = on_a_record(VinylMode::Absolute);
        for id in ["deck1.play", "deck1.beat", "deck1.cue", "deck1.hotcue1", "deck1.loop.4", "deck1.loop.any"]
        {
            assert_eq!(state(id, &v), OFF, "{id}");
        }
        for id in ["deck1.roll4", "deck1.slip.held", "deck1.slip", "deck1.reloop.stop"] {
            assert_eq!(state(id, &v), OFF, "{id}");
        }
        for id in ["deck1.cut2", "deck1.fx", "deck1.keylock"] {
            assert_eq!(state(id, &v), ON, "{id}");
        }
        v.status.decks[0].speed = 1.0;
        assert_eq!(state("deck1.play", &v), ON, "the record turns");
        assert_eq!(state("deck1.beat", &v), ON);
        assert_eq!(state("deck1.cue", &v), OFF);
        v.status.decks[0].vinyl.run_out = true;
        assert_eq!(state("deck1.loop.4", &v), ON, "played on by itself past the record");
    }

    #[test]
    fn under_a_record_in_rel_the_lights_are_those_of_a_deck_played_as_the_record_turns() {
        let mut v = on_a_record(VinylMode::Relative);
        v.status.decks[0].playing = true;
        assert_eq!(state("deck1.play", &v), OFF, "the record still, whatever PLAY said");
        assert_eq!(state("deck1.cue", &v), Some(State::Paused));
        assert_eq!(light(&target("deck1.hotcue1"), &v).map(|l| l.colour), Some(Some(0xff8000)));
        for id in ["deck1.loop.4", "deck1.loop.any", "deck1.roll4", "deck1.slip", "deck1.cut2"] {
            assert_eq!(state(id, &v), ON, "{id}");
        }
        v.status.decks[0].speed = 1.0;
        assert_eq!(state("deck1.play", &v), ON, "the record turns");
        assert_eq!(state("deck1.cue", &v), OFF);
    }

    #[test]
    fn a_hot_cue_set_shows_its_colour() {
        let mut cued = loaded(view(), 1, 0.0, false);
        if let Some(track) = &mut cued.decks[1].track {
            track.hot_cues[2] = Some(0xff8000);
        }
        assert_eq!(
            light(&target("deck2.hotcue3"), &cued),
            Some(Light { state: State::On, colour: Some(0xff8000) })
        );
        assert_eq!(light(&target("deck2.hotcue4"), &cued), Some(Light::OFF));
        assert_eq!(state("deck2.hotcue3.delete", &cued), None);
    }

    #[test]
    fn a_switch_is_lit_on_and_its_off_key_while_it_is_off() {
        let mut v = view();
        v.decks[1].mute = true;
        v.decks[0].key_lock = true;
        v.mixer.talkover = true;
        v.quantize = true;
        assert_eq!(state("deck2.mute", &v), ON);
        assert_eq!(state("deck2.mute.on", &v), ON);
        assert_eq!(state("deck2.mute.off", &v), OFF);
        assert_eq!(state("deck1.mute.off", &v), ON);
        assert_eq!(state("deck1.keylock", &v), ON);
        assert_eq!(state("mixer.talkover.off", &v), OFF);
        assert_eq!(state("general.quantize", &v), ON);
        assert_eq!(state("general.quantize.off", &v), OFF);
        v.decks[0].fx.enabled = false;
        assert_eq!(state("deck1.fx.chain.off", &v), ON);
        v.status.decks[0].sync.locked = true;
        assert_eq!(state("deck1.sync", &v), ON);
    }

    #[test]
    fn a_microphone_key_is_lit_while_it_is_open_or_heard() {
        let mut v = view();
        v.mixer.mic.cue = true;
        v.mixer.aux.on = true;
        assert_eq!(state("mixer.mic.cue", &v), ON);
        assert_eq!(state("mixer.mic.on", &v), OFF);
        assert_eq!(state("mixer.aux.on", &v), ON);
    }

    #[test]
    fn a_sample_slot_shows_empty_loaded_or_playing() {
        let mut v = view();
        assert_eq!(state("deck2.sample3.smart", &v), OFF);
        v.decks[1].samples = 1 << 2;
        assert_eq!(state("deck2.sample3.smart", &v), Some(State::Loaded));
        assert_eq!(state("deck2.sample3", &v), Some(State::Loaded));
        assert_eq!(state("deck2.sample3.held", &v), OFF);
        v.status.decks[1].samples_playing = 1 << 2;
        assert_eq!(state("deck2.sample3", &v), Some(State::Playing));
        assert_eq!(state("deck2.sample3.velocity", &v), ON);
        assert_eq!(state("deck2.sample3.stop", &v), None);
    }

    #[test]
    fn an_effect_is_lit_on_its_kind_on_and_its_slot_focused() {
        let mut v = view();
        v.decks[0].fx.slots[2].enabled = true;
        v.decks[0].fx.slots[2].effect = prisme_control::Effect::Gate;
        assert_eq!(state("deck1.fx3", &v), ON);
        assert_eq!(state("deck1.fx3.held", &v), ON);
        assert_eq!(state("deck1.fx3.gate", &v), ON);
        assert_eq!(state("deck1.fx3.echo", &v), OFF);
        assert_eq!(state("deck1.fx.off", &v), ON);
        v.perform.decks[0].fx_focus = Some(1);
        assert_eq!(state("deck1.fx2.focus", &v), ON);
        assert_eq!(state("deck1.fx.focus", &v), OFF);
    }

    #[test]
    fn a_loop_is_lit_by_its_length_in_beats() {
        let mut v = loaded(view(), 0, 10.0, true);
        // two seconds at 120 BPM: four beats
        v.status.decks[0].looping.region = Some(LoopRegion { start_s: 8.0, end_s: 10.0 });
        v.status.decks[0].looping.beats = Some(4.0);
        assert_eq!(state("deck1.loop.size", &v), OFF, "stored, not playing");
        assert_eq!(state("deck1.reloop.stop", &v), ON);
        v.status.decks[0].looping.active = true;
        assert_eq!(state("deck1.loop", &v), ON);
        assert_eq!(state("deck1.loop.any", &v), ON);
        assert_eq!(state("deck1.loop.size", &v), ON);
        assert_eq!(state("deck1.loop.4", &v), ON);
        assert_eq!(state("deck1.loop.8", &v), OFF);
        v.decks[0].size_beats = 8.0;
        assert_eq!(state("deck1.loop.size", &v), OFF);
        assert_eq!(state("deck1.loop.in", &v), OFF);
        v.status.decks[0].looping.loop_in_s = Some(3.0);
        assert_eq!(state("deck1.loop.in", &v), ON);
    }

    #[test]
    fn a_key_light_shows_the_key_moved_or_moved_by_so_many_semitones() {
        let mut v = loaded(view(), 0, 0.0, false);
        assert_eq!(state("deck1.key.reset", &v), OFF);
        assert_eq!(state("deck1.key.0", &v), ON);
        if let Some(track) = &mut v.decks[0].track {
            track.key_shift = -2;
        }
        assert_eq!(state("deck1.key.reset", &v), ON);
        assert_eq!(state("deck1.key.-2", &v), ON);
        assert_eq!(state("deck1.key.+2", &v), OFF);
    }

    #[test]
    fn a_stem_is_lit_while_its_part_plays_dark_until_it_is_separated_where_the_deck_plays() {
        let mut v = loaded(view(), 0, 0.0, false);
        assert_eq!(state("deck1.vocals", &v), OFF, "not separated");
        if let Some(track) = &mut v.decks[0].track {
            track.separation = Separation::Vocals;
            track.stem_mix = track.stem_mix.toggled(StemPart::Instrumental);
        }
        assert_eq!(state("deck1.vocals", &v), OFF, "not separated there yet");
        v.status.decks[0].stems_here = 0b1001;
        assert_eq!(state("deck1.vocals", &v), ON);
        assert_eq!(state("deck1.instrumental", &v), OFF);
        assert_eq!(state("deck1.drums", &v), OFF, "not a part of this separation");
    }

    #[test]
    fn the_keys_of_a_key_are_dark_without_a_track() {
        assert_eq!(state("deck1.key.0", &view()), OFF);
        assert_eq!(state("deck1.key.0", &loaded(view(), 0, 0.0, false)), ON);
    }

    #[test]
    fn the_channel_lights_show_the_kills_the_filter_the_side_and_the_headphones() {
        let mut v = view();
        v.decks[0].strip.kills[2] = true;
        v.decks[1].strip.filter = -0.3;
        v.decks[1].side = prisme_control::Side::Center;
        v.decks[1].cue = true;
        assert_eq!(state("deck1.eq.high.kill", &v), ON);
        assert_eq!(state("deck1.eq.low.kill", &v), OFF);
        assert_eq!(state("deck2.filter.toggle", &v), ON);
        assert_eq!(state("deck1.filter.toggle", &v), OFF);
        assert_eq!(state("deck2.crossfader.center", &v), ON);
        assert_eq!(state("deck2.crossfader.right", &v), OFF);
        assert_eq!(state("deck2.headphones.solo", &v), ON);
        v.decks[0].cue = true;
        assert_eq!(state("deck2.headphones.solo", &v), OFF);
        v.decks[0].cue = false;
        v.decks[3].cue = true;
        assert_eq!(state("deck2.headphones.solo", &v), ON, "deck D not in use");
    }

    #[test]
    fn the_beat_is_lit_on_its_first_quarter() {
        let mut v = loaded(view(), 0, 10.0, true);
        v.status.decks[0].beat_phase = Some(0.1);
        assert_eq!(state("deck1.beat", &v), ON);
        v.status.decks[0].beat_phase = Some(0.3);
        assert_eq!(state("deck1.beat", &v), OFF);
    }

    #[test]
    fn the_end_of_a_track_playing_is_near_then_soon() {
        let at = |position_s, playing| state("deck1.ending", &loaded(view(), 0, position_s, playing));
        assert_eq!(at(100.0, true), OFF);
        assert_eq!(at(280.0, true), ON);
        assert_eq!(at(295.0, true), Some(State::Soon));
        assert_eq!(at(295.0, false), OFF);
    }

    #[test]
    fn a_tempo_light_compares_with_the_deck_mixed_with_the_engine_rate_telling_zero() {
        let mut v = loaded(loaded(view(), 1, 0.0, true), 0, 0.0, false);
        assert_eq!(state("deck1.tempo.zero", &v), ON);
        // synced to a faster deck: its fader at zero, its tempo not
        v.status.decks[0].rate = 1.02;
        v.status.decks[0].bpm = Some(122.4);
        v.status.decks[1].bpm = Some(124.0);
        assert_eq!(state("deck1.tempo.zero", &v), OFF);
        assert_eq!(state("deck1.tempo.slower", &v), ON);
        assert_eq!(state("deck1.tempo.faster", &v), OFF);
        v.status.decks[1].bpm = Some(122.405);
        assert_eq!(state("deck1.tempo.slower", &v), OFF, "the same tempo");
        let mut empty = view();
        empty.decks[0].pitch = 1.03;
        assert_eq!(state("deck1.tempo.zero", &empty), OFF, "no track: its fader");
    }

    #[test]
    fn a_phase_light_compares_with_the_leader_else_the_deck_mixed_with() {
        let mut v = loaded(loaded(view(), 1, 0.0, true), 0, 0.0, true);
        v.status.decks[0].beat_phase = Some(0.6);
        v.status.decks[1].beat_phase = Some(0.5);
        assert_eq!(state("deck1.phase.ahead", &v), ON);
        assert_eq!(state("deck1.phase", &v), OFF);
        assert_eq!(state("deck2.phase.behind", &v), ON);
        v.status.decks[1].beat_phase = Some(0.59);
        assert_eq!(state("deck1.phase", &v), ON);
        v.status.decks[1].playing = false;
        assert_eq!(state("deck1.phase", &v), OFF, "the other stopped");
    }

    #[test]
    fn the_library_record_auto_dj_and_preview_lights() {
        let mut v = loaded(view(), 1, 0.0, false);
        assert_eq!(state("library.load2", &v), Some(State::Loaded));
        assert_eq!(state("library.load1", &v), OFF);
        assert_eq!(state("library.load2.play", &v), None);
        assert_eq!(state("general.record", &v), OFF);
        assert_eq!(state("library.preview", &v), OFF);
        v.preview_opening = true;
        assert_eq!(state("library.preview", &v), ON, "as soon as its file opens");
        v.preview_opening = false;
        v.status.recording = Some(prisme_engine::RecordingStatus::default());
        v.auto_dj = true;
        v.status.preview =
            Some(prisme_engine::PreviewStatus { id: 1, position_s: 0.0, duration_s: 1.0, waiting: false });
        assert_eq!(state("general.record", &v), ON);
        assert_eq!(state("autodj.toggle", &v), ON);
        assert_eq!(state("library.preview", &v), ON);
    }

    #[test]
    fn the_clip_lights_hold_a_level_past_full_scale() {
        let mut v = view();
        v.perform.decks[1].clipping = true;
        v.perform.main_clipping = true;
        assert_eq!(state("deck2.clip", &v), ON);
        assert_eq!(state("deck1.clip", &v), OFF);
        assert_eq!(state("main.clip", &v), ON);
    }

    #[test]
    fn the_library_key_is_lit_while_the_library_takes_the_whole_window() {
        let mut v = view();
        assert_eq!(state("general.library", &v), OFF);
        v.perform.library_only = true;
        assert_eq!(state("general.library", &v), ON);
    }

    #[test]
    fn a_roll_is_lit_while_it_plays_named_by_its_length_or_its_place() {
        let mut v = view();
        v.perform.decks[0].roll = Some(0.5);
        assert_eq!(state("deck1.roll.0.5", &v), ON);
        assert_eq!(state("deck1.roll4", &v), ON, "the fourth roll: half a beat");
        assert_eq!(state("deck1.roll.1", &v), OFF);
        assert_eq!(state("deck1.roll5", &v), OFF);
        assert_eq!(state("deck2.roll4", &v), OFF);
    }

    #[test]
    fn a_slice_is_lit_while_the_track_plays_in_it_and_a_cut_while_it_cuts() {
        // 120 BPM: a slice of half a second from 4 s
        let mut v = loaded(view(), 1, 5.2, true);
        assert_eq!(state("deck2.slice3", &v), OFF, "no slicer");
        v.perform.decks[1].slicer = Some(4.0);
        assert_eq!(state("deck2.slice3", &v), ON);
        assert_eq!(state("deck2.slice2", &v), OFF);
        v.status.decks[1].position_s = 9.0;
        assert_eq!(state("deck2.slice3", &v), OFF, "out of the slicer");
        v.perform.decks[1].cut = Some(2);
        assert_eq!(state("deck2.cut3", &v), ON);
        assert_eq!(state("deck2.cut1", &v), OFF);
    }
}
