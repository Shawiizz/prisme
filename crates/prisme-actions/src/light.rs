//! What the light of an action shows, read from a [`View`]: on or off, or a state of its own
//! (a cue paused, a sample loaded or playing, the end of a track soon), and the colour of a
//! hot cue.

use crate::cue::at_cue;
use crate::pads::{slice_at, ROLL_BEATS};
use crate::sync::{phase, Beat, Phase};
use crate::view::{band_index, TrackView, View};
use prisme_control::{
    AutoDjAction, Beats, DeckAction, DeckSwitch, FxAction, GeneralAction, LibraryAction, MixerAction,
    MixerSwitch, SampleAction, Target, Turn,
};
use prisme_engine::DeckStatus;

/// The share of a beat its light shows, from its start.
const BEAT_LIT: f64 = 0.25;
/// The end of a track near (s), and soon (s).
const ENDING_S: f64 = 30.0;
const ENDING_SOON_S: f64 = 10.0;
/// A deck this close to its track's tempo (ratio) is at it.
const AT_TEMPO: f64 = 0.0005;
/// Two decks this close in tempo (BPM) are at the same.
const SAME_BPM: f64 = 0.01;
/// Two decks this close in their beat (of a beat) are in phase.
const IN_PHASE: f64 = 0.03;
/// A loop this close to a length (of a beat, or of the deck's size) has it.
const LOOP_LENGTH_WITHIN: f64 = 0.01;

/// The state a light shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// Whether the interface tells the light of `target`: what it shows is kept there yet (the
/// effect focus the interface performs, the library over the whole window).
pub fn told_by_interface(target: &Target) -> bool {
    matches!(
        target,
        Target::Deck(_, DeckAction::Fx(_, FxAction::Focus)) | Target::General(GeneralAction::Library)
    )
}

/// The light of `target` as `view` shows it, if it has one; off for one of a deck not in use.
pub fn light(target: &Target, view: &View) -> Option<Light> {
    let light = match target {
        Target::Deck(deck, action) => deck_light(*deck, *action, view),
        Target::Mixer(action) => mixer_light(*action, view).map(Light::lit),
        Target::Library(LibraryAction::Load(deck)) => Some(Light::of(deck_state(view.status(*deck)))),
        Target::Library(LibraryAction::Preview) => Some(Light::lit(view.status.preview.is_some())),
        Target::General(GeneralAction::Record) => Some(Light::lit(view.status.recording.is_some())),
        Target::General(GeneralAction::Quantize(turn)) => Some(Light::lit(shows(*turn, view.quantize))),
        Target::AutoDj(AutoDjAction::Toggle) => Some(Light::lit(view.auto_dj)),
        _ => None,
    }?;
    Some(if deck_of(target).is_some_and(|deck| !view.shows(deck)) { Light::OFF } else { light })
}

/// The deck an action is of, a load's among them.
fn deck_of(target: &Target) -> Option<usize> {
    match target {
        Target::Deck(deck, _)
        | Target::Library(
            LibraryAction::Load(deck) | LibraryAction::LoadPlay(deck) | LibraryAction::Double(deck),
        ) => Some(*deck),
        _ => None,
    }
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
    let status = view.status(deck);
    let settings = &view.decks[deck];
    let track = settings.track.as_ref();
    let key_shift = track.map_or(0, |t| t.key_shift);
    let perform = &view.perform.decks[deck];
    let sample = |slot: usize| (status.samples_playing & 1 << slot != 0, settings.samples & 1 << slot != 0);
    let fx = |slot: usize| &settings.fx.slots[slot];
    let lit = match action {
        DeckAction::Play => status.playing,
        DeckAction::Cue => return Some(Light::of(cue_state(status, track))),
        DeckAction::Switch(switch, turn) => {
            let on = match switch {
                DeckSwitch::Sync => status.sync.locked,
                DeckSwitch::Slip => status.slip_mode,
                DeckSwitch::Reverse => status.reverse,
                DeckSwitch::Repeat => settings.repeat,
                DeckSwitch::FaderStart => settings.fader_start,
                DeckSwitch::KeyLock => settings.key_lock,
                DeckSwitch::Headphones => settings.cue,
                DeckSwitch::Mute => settings.mute,
                DeckSwitch::Passthrough => settings.passthrough,
                DeckSwitch::Vinyl => settings.vinyl,
                DeckSwitch::JogScratch => settings.jog_scratch,
                DeckSwitch::FxChain => settings.fx.enabled,
            };
            shows(turn, on)
        }
        DeckAction::HotCue(slot) => {
            return Some(match track.and_then(|t| t.hot_cues[slot]) {
                Some(colour) => Light { state: State::On, colour: Some(colour) },
                None => Light::OFF,
            })
        }
        DeckAction::Loop | DeckAction::LoopAny => status.looping.active,
        DeckAction::LoopIn => status.looping.loop_in_s.is_some(),
        DeckAction::LoopSize => loop_beats(status, track)
            .is_some_and(|beats| (beats / settings.size_beats - 1.0).abs() < LOOP_LENGTH_WITHIN),
        DeckAction::LoopBeats(Beats(length)) => {
            loop_beats(status, track).is_some_and(|beats| (beats - length).abs() < LOOP_LENGTH_WITHIN)
        }
        DeckAction::ReloopStop => status.looping.region.is_some(),
        DeckAction::KeyReset => key_shift != 0,
        DeckAction::KeyTo(semitones) => i32::from(key_shift) == semitones,
        DeckAction::ReverseHeld => status.speed < 0.0,
        DeckAction::SlipHeld => status.slip_mode,
        DeckAction::SyncLeader => status.sync.leader,
        DeckAction::Stem(part) => track.is_some_and(|t| t.separation.has(part) && t.stem_mix.playing(part)),
        DeckAction::Sample(slot, SampleAction::Trigger | SampleAction::Smart) => {
            return Some(Light::of(match sample(slot) {
                (true, _) => State::Playing,
                (false, true) => State::Loaded,
                (false, false) => State::Off,
            }))
        }
        DeckAction::Sample(slot, SampleAction::Held | SampleAction::Velocity) => sample(slot).0,
        DeckAction::Fx(slot, FxAction::Enabled(turn)) => shows(turn, fx(slot).enabled),
        DeckAction::Fx(slot, FxAction::Held) => fx(slot).enabled,
        DeckAction::Fx(slot, FxAction::Effect(effect)) => fx(slot).enabled && fx(slot).effect == effect,
        DeckAction::Fx(slot, FxAction::Focus) => perform.fx_focus == Some(slot),
        DeckAction::Roll(roll) => perform.roll == Some(ROLL_BEATS[roll]),
        DeckAction::RollBeats(Beats(beats)) => perform.roll == Some(beats),
        DeckAction::Slice(slice) => slice_playing(status, track, perform.slicer) == Some(slice),
        DeckAction::Cut(cut) => perform.cut == Some(cut),
        DeckAction::EqKill(band) => settings.strip.kills[band_index(band)],
        DeckAction::FilterToggle => settings.strip.filter != 0.0,
        DeckAction::HeadphonesSolo => (0..view.decks_in_use).all(|d| view.decks[d].cue == (d == deck)),
        DeckAction::CrossfaderSide(side) => settings.side == side,
        DeckAction::Beat => status.playing && status.beat_phase.is_some_and(|p| p < BEAT_LIT),
        DeckAction::Clip => perform.clipping,
        DeckAction::Ending => return Some(Light::of(ending(status))),
        DeckAction::TempoZero => tempo(deck, view) == Tempo::Zero,
        DeckAction::TempoSlower => tempo(deck, view) == Tempo::Slower,
        DeckAction::TempoFaster => tempo(deck, view) == Tempo::Faster,
        DeckAction::Phase => phase_of(deck, view) == Phase::In,
        DeckAction::PhaseAhead => phase_of(deck, view) == Phase::Ahead,
        DeckAction::PhaseBehind => phase_of(deck, view) == Phase::Behind,
        _ => return None,
    };
    Some(Light::lit(lit))
}

fn mixer_light(action: MixerAction, view: &View) -> Option<bool> {
    let mixer = &view.mixer;
    Some(match action {
        MixerAction::Switch(switch, turn) => shows(
            turn,
            match switch {
                MixerSwitch::HeadphonesSplit => mixer.split_cue,
                MixerSwitch::Talkover => mixer.talkover,
                MixerSwitch::CrossfaderReverse => mixer.crossfader_reverse,
            },
        ),
        MixerAction::MicOn => mixer.mic.on,
        MixerAction::MicCue => mixer.mic.cue,
        MixerAction::AuxOn => mixer.aux.on,
        MixerAction::AuxCue => mixer.aux.cue,
        MixerAction::MainClip => view.perform.main_clipping,
        _ => return None,
    })
}

/// A CDJ's cue: on at the cue point, paused stopped elsewhere, off playing or empty.
fn cue_state(status: &DeckStatus, track: Option<&TrackView>) -> State {
    let cue_s = track.and_then(|t| t.main_cue_s).unwrap_or(0.0);
    if !status.loaded {
        State::Off
    } else if at_cue(status.position_s, cue_s) {
        State::On
    } else if status.playing {
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
fn slice_playing(status: &DeckStatus, track: Option<&TrackView>, slicer: Option<f64>) -> Option<usize> {
    let beat_s = 60.0 / track?.bpm?;
    slice_at(slicer?, status.position_s, beat_s).filter(|_| status.loaded)
}

/// The beats of the loop playing, if the track has a grid.
fn loop_beats(status: &DeckStatus, track: Option<&TrackView>) -> Option<f64> {
    let region = status.looping.region.filter(|_| status.looping.active)?;
    Some((region.end_s - region.start_s) * track?.bpm? / 60.0)
}

fn ending(status: &DeckStatus) -> State {
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
enum Tempo {
    /// At its track's own.
    Zero,
    Slower,
    Faster,
    /// The same, or not known.
    Off,
}

fn tempo(deck: usize, view: &View) -> Tempo {
    if (view.tempo_rate(deck) - 1.0).abs() < AT_TEMPO {
        return Tempo::Zero;
    }
    match (view.status(deck).bpm, view.status(view.mixed_with(deck)).bpm) {
        (Some(own), Some(other)) if (own - other).abs() >= SAME_BPM => {
            if own < other {
                Tempo::Slower
            } else {
                Tempo::Faster
            }
        }
        _ => Tempo::Off,
    }
}

/// Where a deck's beats are against the leader's, another deck in use leading, else against
/// the deck it mixes with.
fn phase_of(deck: usize, view: &View) -> Phase {
    let leads = |d: &usize| *d != deck && *d < view.status.deck_count && view.status(*d).sync.leader;
    let reference = (0..view.decks_in_use).find(leads).unwrap_or_else(|| view.mixed_with(deck));
    phase(Beat::playing(view.status(deck)), Beat::playing(view.status(reference)), IN_PHASE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stems::Separation;
    use crate::view::fixtures::{loaded, view};
    use prisme_control::StemPart;
    use prisme_engine::LoopRegion;

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
    fn a_stem_is_lit_while_its_part_plays() {
        let mut v = loaded(view(), 0, 0.0, false);
        assert_eq!(state("deck1.vocals", &v), OFF, "not separated");
        if let Some(track) = &mut v.decks[0].track {
            track.separation = Separation::Vocals;
            track.stem_mix = track.stem_mix.toggled(StemPart::Instrumental);
        }
        assert_eq!(state("deck1.vocals", &v), ON);
        assert_eq!(state("deck1.instrumental", &v), OFF);
        assert_eq!(state("deck1.drums", &v), OFF, "not a part of this separation");
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
    fn the_interface_tells_the_lights_of_what_it_keeps() {
        let told: Vec<&str> =
            ["deck1.roll.0.5", "deck2.slice3", "deck3.fx2.focus", "general.library", "deck1.play"]
                .into_iter()
                .filter(|id| told_by_interface(&target(id)))
                .collect();
        assert_eq!(told, ["deck3.fx2.focus", "general.library"]);
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
