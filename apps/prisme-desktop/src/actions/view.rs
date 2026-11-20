//! The application as the actions see it at a moment (`prisme_actions::view::View`), taken
//! under short locks, each let go before the next: the engine's state first, copied out of the
//! session's lock. A move of a control reads only the part it needs ([`Sees`]).

use super::Performing;
use crate::dto::{CrossfaderCurveDto, CrossfaderSideDto, FxDto, LiveSourceDto};
use crate::preferences::MixerPreferences;
use crate::sampler::LoadedSample;
use crate::state::{as_laid_out, live_preferences, lock, AppState, DeckSettings, LiveSwitches, LoadedTrack};
use prisme_actions::input::Faders;
use prisme_actions::perform::Memory;
use prisme_actions::view::{DeckView, FxChain, FxSlot, Live, MixerView, Sees, Strip, TrackView, View};
use prisme_control::{HOT_CUES, SAMPLE_SLOTS};
use prisme_engine::DeckStatus;
use prisme_library::CueSlot;
use std::sync::Mutex;

/// `state` as the actions see it at `now_s` on their clock (the clip lights are timed by it),
/// with what they keep for themselves (`memory`), the Auto DJ playing or not.
pub fn view_of(state: &AppState, auto_dj: bool, memory: &Mutex<Memory>, now_s: f64) -> View {
    let (status, layout) = lock(&state.session)
        .as_mut()
        .map(|s| (s.controller.status(), Some((s.info.decks, s.info.input_channels))))
        .unwrap_or_default();
    let (decks_in_use, quantize, ranges, mixer) = state.preferences.read(|p| {
        let ranges: [f64; prisme_engine::MAX_DECKS] = std::array::from_fn(|d| p.tempo_range(d));
        (p.dj.decks(), p.dj.quantize, ranges, p.mixer)
    });
    let (clipping, faders) = {
        let mut memory = lock(memory);
        (memory.clips.heard(&status, now_s), memory.faders)
    };
    let perform = lock(&state.perform).view(clipping);
    let settings = as_laid_out(*lock(&state.decks), layout);
    let tracks = lock(&state.tracks).each_ref().map(|t| t.as_ref().map(track_view));
    let samples = lock(&state.samplers).each_ref().map(loaded);
    let mixer = mixer_view(&mixer, *lock(&state.live), *lock(&state.crossfader));
    let preview_opening = lock(&state.preview).opening.is_some();
    let decks = std::array::from_fn(|d| deck_view(d, &settings[d], ranges[d], samples[d], tracks[d]));
    View { decks_in_use, status, decks, mixer, quantize, auto_dj, preview_opening, perform, faders }
}

/// What a move reads, as it reads it: the settings of a deck as it shows them (a turntable
/// taken by the deck layered with it off), without its track and its sampler, its track only
/// when asked, the engine's state of a deck only.
impl Sees for Performing<'_> {
    fn decks_in_use(&self) -> usize {
        self.state.preferences.read(|p| p.dj.decks())
    }

    fn deck(&self, deck: usize) -> DeckView {
        let range = self.state.tempo_range(deck);
        let settings = self.state.decks_laid_out()[deck];
        deck_view(deck, &settings, range, 0, None)
    }

    fn track(&self, deck: usize) -> Option<TrackView> {
        lock(&self.state.tracks)[deck].as_ref().map(track_view)
    }

    fn status(&self, deck: usize) -> DeckStatus {
        self.state.deck_status(deck).unwrap_or_default()
    }

    fn mixer(&self) -> MixerView {
        let mixer = self.state.preferences.read(|p| p.mixer);
        mixer_view(&mixer, *lock(&self.state.live), *lock(&self.state.crossfader))
    }

    fn faders(&self) -> Faders {
        self.memory().faders
    }

    fn fx_focus(&self, deck: usize) -> Option<usize> {
        lock(&self.state.perform).decks[deck].fx_focus
    }

    fn quantize(&self) -> bool {
        self.state.preferences.read(|p| p.dj.quantize)
    }
}

/// The sampler slots holding a sound, a bit each.
fn loaded(slots: &[Option<LoadedSample>; SAMPLE_SLOTS]) -> u16 {
    let loaded = slots.iter().enumerate().filter(|(_, s)| s.is_some());
    loaded.fold(0, |bits, (slot, _)| bits | 1 << slot)
}

fn deck_view(
    deck: usize,
    d: &DeckSettings,
    tempo_range: f64,
    samples: u16,
    track: Option<TrackView>,
) -> DeckView {
    DeckView {
        pitch: d.pitch,
        tempo_range,
        volume: d.volume,
        cue: d.cue,
        key_lock: d.key_lock,
        fader_start: d.fader_start,
        vinyl: d.vinyl_enabled,
        jog_scratch: d.jog_scratch,
        repeat: d.repeat,
        mute: d.mute,
        passthrough: d.passthrough,
        side: d.side.unwrap_or(CrossfaderSideDto::of_deck(deck)).into(),
        strip: Strip { trim: d.strip.trim, eq: d.strip.eq, kills: d.strip.kills, filter: d.strip.filter },
        fx: FxChain { enabled: d.fx.enabled, slots: d.fx.slots.map(fx_slot) },
        sampler_gain: d.sampler_gain,
        samples,
        size_beats: d.size_beats,
        track,
    }
}

fn mixer_view(mixer: &MixerPreferences, switches: [LiveSwitches; 2], crossfader: f32) -> MixerView {
    let live = |source: LiveSourceDto| {
        let on = switches[source as usize];
        Live { level: live_preferences(mixer, source).level, on: on.on, cue: on.cue }
    };
    MixerView {
        crossfader,
        cue_mix: mixer.cue_mix,
        split_cue: mixer.split_cue,
        headphone_gain: mixer.headphone_gain,
        sharp_curve: mixer.crossfader_curve == CrossfaderCurveDto::Sharp,
        crossfader_reverse: mixer.crossfader_reverse,
        main_gain: mixer.main_gain,
        balance: mixer.balance,
        booth_gain: mixer.booth_gain,
        mic: live(LiveSourceDto::Mic),
        aux: live(LiveSourceDto::Aux),
        talkover: mixer.talkover,
    }
}

fn track_view(track: &LoadedTrack) -> TrackView {
    let mut hot_cues = [None; HOT_CUES];
    let mut main_cue_s = None;
    for cue in &track.cues {
        match cue.slot {
            CueSlot::Main => main_cue_s = Some(cue.position_s),
            CueSlot::Hot(slot) => {
                if let Some(colour) = hot_cues.get_mut(usize::from(slot)) {
                    *colour = Some(cue.color);
                }
            }
        }
    }
    TrackView {
        bpm: track.grid().map(|g| g.bpm),
        key_shift: track.key_shift,
        separation: track.separation(),
        stem_mix: track.stem_mix,
        main_cue_s,
        hot_cues,
    }
}

fn fx_slot(fx: FxDto) -> FxSlot {
    FxSlot {
        effect: fx.kind.into(),
        enabled: fx.enabled,
        mix: fx.mix,
        amount: fx.amount,
        param2: fx.param2,
        beats: fx.beats,
    }
}

#[cfg(test)]
mod tests {
    use crate::actions::bench::Bench;
    use crate::actions::Source;
    use prisme_actions::input::Input;
    use prisme_actions::position::position;
    use prisme_control::Target;

    #[test]
    fn a_move_reads_where_its_fader_stands_as_the_whole_view_shows_it() {
        let bench = Bench::new();
        for (id, value) in
            [("deck1.eq.low", 0.01), ("deck2.fx3.period", 0.2), ("mixer.mic", 0.3), ("deck2.volume", 0.4)]
        {
            bench.perform(id, Input::Value(value), Source::Interface { confirmed: false }).unwrap();
        }
        let view = bench.view();
        let doing = bench.performing(Source::Interface { confirmed: false });
        for id in [
            "deck1.eq.low",
            "deck2.fx3.period",
            "deck2.fx.knob3",
            "deck2.volume",
            "deck1.pitch",
            "deck1.position",
            "mixer.mic",
            "mixer.crossfaderCurve",
            "sampler.gain",
            "deck3.volume",
            "deck1.play",
        ] {
            let target: Target = id.parse().unwrap();
            assert_eq!(position(&target, &doing), position(&target, &view), "{id}");
        }
    }
}
