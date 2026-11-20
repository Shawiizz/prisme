//! The application as the actions see it at a moment (`prisme_actions::view::View`), taken
//! under short locks, each let go before the next: the engine's state first, copied out of the
//! session's lock.

use crate::dto::{CrossfaderCurveDto, CrossfaderSideDto, FxDto, LiveSourceDto};
use crate::state::{as_laid_out, live_preferences, lock, AppState, LoadedTrack};
use prisme_actions::perform::PerformState;
use prisme_actions::view::{DeckView, FxChain, FxSlot, Live, MixerView, Strip, TrackView, View};
use prisme_control::HOT_CUES;
use prisme_library::CueSlot;
use std::sync::Mutex;

/// `state` as the actions see it at `now_s` on their clock (the clip lights are timed by it),
/// the Auto DJ playing or not.
pub fn view_of(state: &AppState, auto_dj: bool, perform: &Mutex<PerformState>, now_s: f64) -> View {
    let (status, layout) = lock(&state.session)
        .as_mut()
        .map(|s| (s.controller.status(), Some((s.info.decks, s.info.input_channels))))
        .unwrap_or_default();
    let (decks_in_use, quantize, ranges, mixer) = state.preferences.read(|p| {
        let ranges: [f64; prisme_engine::MAX_DECKS] = std::array::from_fn(|d| p.tempo_range(d));
        (p.dj.decks(), p.dj.quantize, ranges, p.mixer)
    });
    let perform = lock(perform).view(&status, now_s);
    let settings = as_laid_out(*lock(&state.decks), layout);
    let tracks = lock(&state.tracks).each_ref().map(|t| t.as_ref().map(track_view));
    let samples = lock(&state.samplers).each_ref().map(|slots| {
        let loaded = slots.iter().enumerate().filter(|(_, s)| s.is_some());
        loaded.fold(0u16, |bits, (slot, _)| bits | 1 << slot)
    });
    let switches = *lock(&state.live);
    let crossfader = *lock(&state.crossfader);
    let live = |source: LiveSourceDto| {
        let on = switches[source as usize];
        Live { level: live_preferences(&mixer, source).level, on: on.on, cue: on.cue }
    };
    let decks = std::array::from_fn(|deck| {
        let d = &settings[deck];
        DeckView {
            pitch: d.pitch,
            tempo_range: ranges[deck],
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
            samples: samples[deck],
            size_beats: d.size_beats,
            track: tracks[deck],
        }
    });
    View {
        decks_in_use,
        status,
        decks,
        mixer: MixerView {
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
        },
        quantize,
        auto_dj,
        perform,
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
