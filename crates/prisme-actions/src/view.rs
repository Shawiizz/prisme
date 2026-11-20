//! What the actions see of the application at a moment: the engine's state, the decks'
//! settings and tracks, the mixer and what the actions keep between presses. An owned
//! snapshot, taken once for all the lights and values a tick shows; or, for one move of a
//! control, the part of it the move reads ([`Sees`]).

use crate::input::Faders;
use crate::perform::PerformView;
use crate::stems::{Separation, StemMix};
use prisme_control::{Band, Effect, Side, FX_SLOTS, HOT_CUES, MAX_DECKS};
use prisme_engine::{DeckStatus, EngineStatus};

/// What the actions see of the application, part by part: a [`View`] taken whole for a tick,
/// or the application itself, read only as far as a move asks.
pub trait Sees {
    fn decks_in_use(&self) -> usize;
    /// The settings of `deck` (its track: [`Sees::track`]).
    fn deck(&self, deck: usize) -> DeckView;
    /// What the actions read of the track on `deck`.
    fn track(&self, deck: usize) -> Option<TrackView>;
    /// What the engine shows of `deck`.
    fn status(&self, deck: usize) -> DeckStatus;
    fn mixer(&self) -> MixerView;
    /// Where the faders over a few steps or silent at an end were last given.
    fn faders(&self) -> Faders;
    /// The slot of the effect chain of `deck` the effect knobs turn.
    fn fx_focus(&self, deck: usize) -> Option<usize>;
}

impl Sees for View {
    fn decks_in_use(&self) -> usize {
        self.decks_in_use
    }

    fn deck(&self, deck: usize) -> DeckView {
        self.decks[deck]
    }

    fn track(&self, deck: usize) -> Option<TrackView> {
        self.decks[deck].track
    }

    fn status(&self, deck: usize) -> DeckStatus {
        self.status.decks[deck]
    }

    fn mixer(&self) -> MixerView {
        self.mixer
    }

    fn faders(&self) -> Faders {
        self.perform.faders
    }

    fn fx_focus(&self, deck: usize) -> Option<usize> {
        self.perform.decks[deck].fx_focus
    }
}

#[derive(Debug, Clone, Copy)]
pub struct View {
    /// The decks in use: two, or four.
    pub decks_in_use: usize,
    /// The engine's state; a stopped engine's while the audio is off.
    pub status: EngineStatus,
    pub decks: [DeckView; MAX_DECKS],
    pub mixer: MixerView,
    pub quantize: bool,
    /// The Auto DJ plays or mixes.
    pub auto_dj: bool,
    /// The file of a track to preview opens (the engine previews it once it is open).
    pub preview_opening: bool,
    pub perform: PerformView,
}

/// A deck's settings, and its track.
#[derive(Debug, Clone, Copy)]
pub struct DeckView {
    /// Its tempo fader's ratio.
    pub pitch: f64,
    /// The range of its tempo fader: its own, else the general one.
    pub tempo_range: f64,
    pub volume: f32,
    /// Heard in the headphones.
    pub cue: bool,
    pub key_lock: bool,
    pub fader_start: bool,
    pub vinyl: bool,
    pub jog_scratch: bool,
    pub repeat: bool,
    pub mute: bool,
    pub passthrough: bool,
    pub side: Side,
    pub strip: Strip,
    pub fx: FxChain,
    pub sampler_gain: f32,
    /// The sampler slots holding a sound, a bit each.
    pub samples: u16,
    /// What its jumps go by and its sized loop lasts, in beats.
    pub size_beats: f64,
    pub track: Option<TrackView>,
}

/// A channel of the software mixer before its fader; gains are linear.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Strip {
    pub trim: f32,
    /// Low, mid, high.
    pub eq: [f32; 3],
    pub kills: [bool; 3],
    /// -1 the low-pass closed, 0 open, 1 the high-pass closed.
    pub filter: f32,
}

/// The place of `band` among a strip's.
pub fn band_index(band: Band) -> usize {
    match band {
        Band::Low => 0,
        Band::Mid => 1,
        Band::High => 2,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FxChain {
    /// The whole chain on, each slot then as its own switch says.
    pub enabled: bool,
    pub slots: [FxSlot; FX_SLOTS],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FxSlot {
    pub effect: Effect,
    pub enabled: bool,
    pub mix: f32,
    pub amount: f32,
    pub param2: f32,
    /// Its period in beats.
    pub beats: f32,
}

/// What the actions read of a deck's track.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackView {
    /// Of its beat grid.
    pub bpm: Option<f64>,
    pub key_shift: i8,
    pub separation: Separation,
    pub stem_mix: StemMix,
    pub main_cue_s: Option<f64>,
    /// The colour of each hot cue set (`0xrrggbb`).
    pub hot_cues: [Option<u32>; HOT_CUES],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MixerView {
    /// -1 full left, 1 full right.
    pub crossfader: f32,
    pub cue_mix: f32,
    pub split_cue: bool,
    pub headphone_gain: f32,
    /// The curve that cuts only at the ends (else the smooth one).
    pub sharp_curve: bool,
    pub crossfader_reverse: bool,
    pub main_gain: f32,
    pub balance: f32,
    pub booth_gain: f32,
    pub mic: Live,
    pub aux: Live,
    pub talkover: bool,
}

/// A live source of the software mixer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Live {
    pub level: f32,
    pub on: bool,
    pub cue: bool,
}

impl View {
    /// What the engine shows of `deck`.
    pub fn status(&self, deck: usize) -> &DeckStatus {
        &self.status.decks[deck]
    }

    /// Whether `deck` is one of the decks in use.
    pub fn shows(&self, deck: usize) -> bool {
        deck < self.decks_in_use
    }

    /// The deck `deck` is mixed with (see [`mixed_with`]).
    pub fn mixed_with(&self, deck: usize) -> usize {
        mixed_with(&self.status, deck)
    }

    /// The rate `deck` plays at, gestures aside: the engine's for a track, which a record or
    /// the sync may give it, else its tempo fader's.
    pub fn tempo_rate(&self, deck: usize) -> f64 {
        let status = self.status(deck);
        if status.loaded {
            status.rate
        } else {
            self.decks[deck].pitch
        }
    }
}

/// The deck `deck` is mixed with, the engine at `status`: another one playing, else the one
/// beside it (A and B, C and D).
pub fn mixed_with(status: &EngineStatus, deck: usize) -> usize {
    let running = &status.decks[..status.deck_count];
    running.iter().enumerate().position(|(d, s)| d != deck && s.playing).unwrap_or(deck ^ 1)
}

#[cfg(test)]
pub(crate) mod fixtures {
    use super::*;
    use crate::perform::PerformState;

    pub fn deck() -> DeckView {
        let slot =
            FxSlot { effect: Effect::Echo, enabled: false, mix: 0.5, amount: 0.5, param2: 0.5, beats: 1.0 };
        DeckView {
            pitch: 1.0,
            tempo_range: 0.08,
            volume: 1.0,
            cue: false,
            key_lock: false,
            fader_start: false,
            vinyl: false,
            jog_scratch: true,
            repeat: false,
            mute: false,
            passthrough: false,
            side: Side::Left,
            strip: Strip { trim: 1.0, eq: [1.0; 3], kills: [false; 3], filter: 0.0 },
            fx: FxChain { enabled: true, slots: [slot; FX_SLOTS] },
            sampler_gain: 1.0,
            samples: 0,
            size_beats: 4.0,
            track: None,
        }
    }

    pub fn track() -> TrackView {
        TrackView {
            bpm: Some(120.0),
            key_shift: 0,
            separation: Separation::None,
            stem_mix: StemMix::default(),
            main_cue_s: None,
            hot_cues: [None; HOT_CUES],
        }
    }

    /// Two decks in use, the audio running, nothing loaded.
    pub fn view() -> View {
        let live = Live { level: 1.0, on: false, cue: false };
        let status = EngineStatus { deck_count: 2, ..EngineStatus::default() };
        View {
            decks_in_use: 2,
            status,
            decks: [deck(); MAX_DECKS],
            mixer: MixerView {
                crossfader: 0.0,
                cue_mix: 0.0,
                split_cue: false,
                headphone_gain: 1.0,
                sharp_curve: false,
                crossfader_reverse: false,
                main_gain: 1.0,
                balance: 0.0,
                booth_gain: 1.0,
                mic: live,
                aux: live,
                talkover: false,
            },
            quantize: false,
            auto_dj: false,
            preview_opening: false,
            perform: PerformState::default().view(Default::default()),
        }
    }

    /// `view` with a track loaded on `deck`, at `position_s` of `duration_s`, playing or not.
    pub fn loaded(mut view: View, deck: usize, position_s: f64, playing: bool) -> View {
        view.decks[deck].track = Some(track());
        let status = &mut view.status.decks[deck];
        (status.loaded, status.position_s, status.duration_s, status.playing) =
            (true, position_s, 300.0, playing);
        status.bpm = Some(120.0);
        status.rate = 1.0;
        view
    }
}
