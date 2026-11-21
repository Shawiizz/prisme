//! The decks: their tracks, their settings, what the screen shows of them at each frame.

use super::{AudioInfo, ChannelStripDto, FxChainDto, RpmDto, SignalDto, VinylDto, VinylModeDto};
use crate::audio_host::{deck_pair, layer_partner};
use crate::sampler;
use crate::state::{lock, AppState};
use prisme_actions::knobs::{self, StripKnobs};
use prisme_actions::pads::Pads;
use prisme_actions::perform::DeckPerform;
use prisme_actions::stems::{Separation as SeparationDto, StemMix as StemMixDto};
use prisme_actions::view::View;
use prisme_actions::{light, screen};
use prisme_engine::{LoopStatus, Rpm, StemTracks, MAX_DECKS};
use prisme_library::{Cue, CueSlot};
use prisme_timecode::SignalState;
use serde::Serialize;

/// A waveform as the screen draws it, in binary (as JSON, a track's waveform is several hundred
/// kilobytes of numbers): the slices of the whole track, `slices_per_second` and the first slice
/// sent, `from` (u32 each, little-endian), then for each slice known from there the height of its
/// envelope, `heights`, then its red, green and blue (`prisme_analysis::envelope`), one byte
/// each. A waveform still read is sent from where it changed as it grows: the slices before
/// `from` stay as they were sent.
pub fn waveform_bytes(w: &prisme_analysis::Waveform, heights: &[u8], slices: usize, from: usize) -> Vec<u8> {
    let known = w.len();
    let mut bytes = Vec::with_capacity(12 + 4 * (known - from));
    for value in [slices.max(known), w.slices_per_second as usize, from] {
        bytes.extend((value as u32).to_le_bytes());
    }
    bytes.extend(&heights[from..]);
    let colours: Vec<[u8; 3]> = (from..known).map(|i| prisme_analysis::envelope::colour(w, i)).collect();
    for c in 0..3 {
        bytes.extend(colours.iter().map(|rgb| rgb[c]));
    }
    bytes
}

/// All of `w`, a waveform whose slices are all known, in binary (see [`waveform_bytes`]).
pub fn whole_waveform_bytes(w: &prisme_analysis::Waveform) -> Vec<u8> {
    waveform_bytes(w, &prisme_analysis::envelope::heights(w), w.len(), 0)
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "Tempo"))]
pub struct TempoDto {
    pub bpm: f64,
    pub first_beat_s: f64,
    /// The beats of a bar, the first on `first_beat_s`.
    pub beats_per_bar: u32,
}

impl From<&prisme_analysis::Tempo> for TempoDto {
    fn from(t: &prisme_analysis::Tempo) -> Self {
        Self { bpm: t.bpm, first_beat_s: t.first_beat_s, beats_per_bar: prisme_engine::BEATS_PER_BAR }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "MusicalKey"))]
pub struct KeyDto {
    /// "Am", "F#"...
    pub name: &'static str,
    /// Its tonic's letter, "C" to "B".
    pub note: &'static str,
    /// Its tonic's "♯" or "♭", or nothing.
    pub accidental: &'static str,
    /// "8A", "2B"...
    pub camelot: &'static str,
    /// Its place on the Camelot wheel, 1 to 12.
    pub number: u8,
    pub minor: bool,
}

impl From<prisme_analysis::Key> for KeyDto {
    fn from(key: prisme_analysis::Key) -> Self {
        let (note, accidental) = key.tonic_spelled();
        Self {
            name: key.name(),
            note,
            accidental,
            camelot: key.camelot(),
            number: key.camelot_number(),
            minor: key.mode() == prisme_analysis::Mode::Minor,
        }
    }
}

impl From<&prisme_analysis::KeyEstimate> for KeyDto {
    fn from(k: &prisme_analysis::KeyEstimate) -> Self {
        k.key.into()
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "TrackInfo"))]
pub struct TrackInfoDto {
    pub deck: usize,
    /// Tells this load from another of the same file.
    pub load_id: u64,
    pub path: String,
    pub title: String,
    pub artist: Option<String>,
    pub duration_s: f64,
    pub sample_rate: u32,
    pub tempo: Option<TempoDto>,
    pub key: Option<KeyDto>,
    pub grid_edited: bool,
    pub key_edited: bool,
    pub cues: Vec<CueDto>,
    /// Semitones the deck transposes the track by.
    pub key_shift: i8,
    #[serde(flatten)]
    pub stems: DeckStemsDto,
    pub stem_mix: StemMixDto,
}

/// The stems on a deck, which it can mute.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "DeckStems"))]
pub struct DeckStemsDto {
    pub separation: SeparationDto,
    /// Separated in parts only: being separated, or a separation stopped short.
    pub partial: bool,
}

impl DeckStemsDto {
    pub fn of(stems: &StemTracks) -> Self {
        Self {
            separation: SeparationDto::of(|stem| stems[stem as usize].is_some()),
            partial: stems.iter().flatten().any(|s| !s.is_complete()),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "Cue"))]
pub struct CueDto {
    /// Hot cue slot, `None` for the main cue.
    pub hot: Option<u8>,
    pub position_s: f64,
    pub loop_length_s: Option<f64>,
    /// 0xRRGGBB
    pub color: u32,
    pub label: String,
}

impl From<&Cue> for CueDto {
    fn from(c: &Cue) -> Self {
        let hot = match c.slot {
            CueSlot::Main => None,
            CueSlot::Hot(n) => Some(n),
        };
        Self {
            hot,
            position_s: c.position_s,
            loop_length_s: c.loop_length_s,
            color: c.color,
            label: c.label.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "DeckSettings"))]
pub struct DeckSettingsDto {
    pub pitch: f64,
    pub volume: f32,
    pub cue: bool,
    pub vinyl_enabled: bool,
    pub vinyl_mode: VinylModeDto,
    pub rpm: RpmDto,
    /// Chosen, else the record's own.
    pub lead_in_s: Option<f64>,
    /// The record's own: where its track starts at its speed (s of music).
    pub record_lead_in_s: f64,
    pub swap_channels: bool,
    /// The record on its turntable (a timecode format's id).
    pub timecode: &'static str,
    pub key_lock: bool,
    /// Software mixer: the channel fader starts and stops the deck.
    pub fader_start: bool,
    /// The range of its tempo fader: its own, else the general one.
    pub tempo_range: f64,
    pub strip: ChannelStripDto,
    /// Its channel's knobs, as the screen shows them.
    pub knobs: StripKnobs,
    pub fx: FxChainDto,
    /// Name of the sound in each sampler slot.
    pub samples: Vec<Option<String>>,
    pub sampler_gain: f32,
    /// Its size in beats: what its jumps go by, what its sized loop lasts.
    pub size_beats: f64,
    /// The deck plays its input instead of its track.
    pub passthrough: bool,
    /// Records may drive it: turntables can be read on the device, or it is handed to one.
    pub records: bool,
    /// The first of the input pair its turntable and its passthrough come in on (0-based).
    pub input: Option<usize>,
    /// The deck whose turntable it shares and reads the record of, on a device without a pair of
    /// inputs for each deck.
    pub layered_on: Option<usize>,
    /// Its side of the crossfader.
    pub side: prisme_control::Side,
    /// At the end, the track goes on from its start.
    pub repeat: bool,
    /// Silenced in the master, after its fader (still heard in the headphones).
    pub mute: bool,
}

impl DeckSettingsDto {
    /// The decks' settings as the interface shows them, `view` showing their knobs.
    pub fn all(state: &AppState, view: &View) -> Vec<Self> {
        let ranges = state.preferences.read(|p| std::array::from_fn::<_, MAX_DECKS, _>(|d| p.tempo_range(d)));
        let info = state.running_info();
        let samplers = lock(&state.samplers);
        let laid_out = info.as_ref().map_or(view.decks_in_use, |info| info.decks);
        let inputs = info.as_ref().map(|info| info.input_channels);
        let turntables = info.as_ref().is_some_and(|info| info.simulated || info.input_channels >= 2);
        state
            .decks_laid_out(info.as_ref().map(AudioInfo::layout))
            .iter()
            .zip(samplers.iter())
            .enumerate()
            .map(|(deck, (d, samples))| Self {
                pitch: d.pitch,
                volume: d.volume,
                cue: d.cue,
                vinyl_enabled: d.vinyl_enabled,
                vinyl_mode: d.vinyl_mode,
                rpm: d.rpm,
                lead_in_s: d.lead_in_s,
                record_lead_in_s: Rpm::from(d.rpm).music_s(d.timecode.lead_in_s),
                swap_channels: d.swap_channels,
                timecode: d.timecode.id,
                key_lock: d.key_lock,
                fader_start: d.fader_start,
                tempo_range: ranges[deck],
                strip: d.strip,
                knobs: knobs::strip(view, deck),
                fx: d.fx,
                samples: samples.iter().map(|s| s.as_ref().map(|s| sampler::name(&s.path))).collect(),
                sampler_gain: d.sampler_gain,
                size_beats: d.size_beats,
                passthrough: d.passthrough,
                records: turntables || d.vinyl_enabled || d.passthrough,
                input: inputs.map(|inputs| 2 * deck_pair(deck, laid_out, inputs)),
                layered_on: inputs
                    .and_then(|inputs| layer_partner(deck, laid_out, inputs))
                    .filter(|&partner| partner < deck),
                side: d.side.unwrap_or(prisme_control::Side::of_deck(deck)),
                repeat: d.repeat,
                mute: d.mute,
            })
            .collect()
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "SyncStatus"))]
pub struct SyncDto {
    pub state: screen::Sync,
    pub leader: bool,
    /// What its SYNC key does.
    pub key: screen::SyncKey,
}

/// A deck as the engine has it and the screen shows it at a frame.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "DeckStatus"))]
pub struct DeckStatusDto {
    pub loaded: bool,
    pub playing: bool,
    pub position_s: f64,
    pub duration_s: f64,
    pub speed: f64,
    /// The rate it plays at: its tempo, gestures excluded, or its fader's without a track.
    pub rate: f64,
    /// Its tempo, gestures excluded (needs a beat grid).
    pub bpm: Option<f64>,
    pub heard_key: Option<KeyDto>,
    /// The DJ hears or holds its track: a load would replace it.
    pub plays: bool,
    pub remaining_s: Option<f64>,
    /// The end of its track near (on), or soon.
    pub ending: light::State,
    /// Who drives it.
    pub mode: screen::Drive,
    pub tempo: screen::TempoFader,
    pub sync: SyncDto,
    /// Its loops, jumps and pads may move its track.
    pub can_move: bool,
    /// Its motor (brake, spin back) may move its track.
    pub can_motor: bool,
    /// Its grid may move onto the beats of the deck it is mixed with.
    pub can_align_grid: bool,
    /// Its key may be moved to mix with the one heard on the deck it is mixed with.
    pub can_sync_key: bool,
    pub lights: DeckLightsDto,
    pub looping: LoopDto,
    pub slip_mode: bool,
    /// The reverse is on: the track plays backwards.
    pub reverse: bool,
    pub vinyl: VinylDto,
    /// Sampler slots playing (bit per slot).
    pub samples_playing: u16,
    /// The parts of its separated track, a knob each.
    pub stems: Vec<screen::StemKnob>,
    /// How far its track's separation is, from 0 to 1: as it is separated, or as the downloads
    /// of the models it waits for are.
    pub separating: Option<f64>,
}

/// The carrier level of a healthy line-level timecode.
const FULL_LEVEL: f32 = 0.5;

/// The timecode as the light of a deck shows it: none while no record drives it (it plays by
/// itself or its input), none heard while its channels seem crossed.
pub(super) fn signal_light(on_record: bool, crossed: bool, signal: SignalState) -> Option<SignalDto> {
    on_record.then(|| if crossed { SignalDto::None } else { signal.into() })
}

/// What a deck's keys and pads light by the controllers' rules.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "DeckLights"))]
pub struct DeckLightsDto {
    /// Its CUE: on at the cue point, paused stopped elsewhere.
    pub cue: light::State,
    /// What its pads on the screen show.
    pub pads: Pads,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "LoopStatus"))]
pub struct LoopDto {
    pub start_s: Option<f64>,
    pub end_s: Option<f64>,
    pub active: bool,
    /// Start marked by LOOP IN, waiting for LOOP OUT.
    pub loop_in_s: Option<f64>,
}

impl From<LoopStatus> for LoopDto {
    fn from(l: LoopStatus) -> Self {
        Self {
            start_s: l.region.map(|r| r.start_s),
            end_s: l.region.map(|r| r.end_s),
            active: l.active,
            loop_in_s: l.loop_in_s,
        }
    }
}

impl DeckStatusDto {
    /// `deck` as `view` shows it.
    pub(super) fn of(view: &View, deck: usize) -> Self {
        let d = view.deck_status(deck);
        let shown = screen::deck(view, deck);
        Self {
            loaded: d.loaded,
            playing: d.playing,
            position_s: d.position_s,
            duration_s: d.duration_s,
            speed: d.speed,
            rate: shown.rate,
            bpm: d.bpm,
            heard_key: shown.heard_key.map(KeyDto::from),
            plays: shown.plays,
            remaining_s: shown.remaining_s,
            ending: shown.ending,
            mode: shown.drive,
            tempo: shown.tempo,
            sync: SyncDto { state: shown.sync, leader: d.sync.leader, key: shown.sync_key },
            can_move: shown.can_move,
            can_motor: shown.can_motor,
            can_align_grid: shown.can_align_grid,
            can_sync_key: shown.can_sync_key,
            lights: DeckLightsDto { cue: shown.cue, pads: shown.pads },
            looping: d.looping.into(),
            slip_mode: d.slip_mode,
            reverse: d.reverse,
            samples_playing: d.samples_playing,
            stems: screen::stem_knobs(view, deck).collect(),
            separating: None,
            vinyl: VinylDto {
                enabled: d.vinyl.enabled,
                mode: d.vinyl.mode.into(),
                run_out: d.vinyl.run_out,
                crossed: d.vinyl.crossed,
                signal: d.vinyl.signal.into(),
                light: signal_light(
                    view.decks[deck].vinyl && !view.decks[deck].passthrough,
                    d.vinyl.crossed,
                    d.vinyl.signal,
                ),
                level: (d.vinyl.level / FULL_LEVEL).min(1.0),
                record_speed: d.vinyl.record_speed,
                record_position_s: d.vinyl.record_position_s,
            },
        }
    }
}

/// Which slots of a deck's effect chain its pads and knobs act on.
#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "DeckPerform"))]
pub struct DeckPadsDto {
    /// The slot of its effect chain the effect knobs turn.
    pub fx_focus: Option<usize>,
    /// The slot of its effect chain the screen's pads and knobs act on.
    pub fx_shown: usize,
}

impl From<&DeckPerform> for DeckPadsDto {
    fn from(deck: &DeckPerform) -> Self {
        Self { fx_focus: deck.fx_focus, fx_shown: deck.fx_shown }
    }
}
