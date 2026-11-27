//! What the user chooses once and finds again at the next start.

use crate::dto::{
    AudioSettings, CrossfaderCurveDto, HeadphonesOutputDto, InputChannelsDto, MixModeDto, RecordSourceDto,
    RpmDto, VinylModeDto,
};
use prisme_engine::MAX_DECKS;
use prisme_timecode::{TimecodeFormat, SERATO_CV02_A};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;
use std::ops::RangeInclusive;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Preferences {
    pub audio: AudioSettings,
    #[serde(deserialize_with = "decks_of")]
    pub decks: [DeckPreferences; MAX_DECKS],
    pub dj: DjPreferences,
    pub mixer: MixerPreferences,
    pub library: LibraryPreferences,
    pub auto_dj: AutoDjPreferences,
    /// The keyboard keys the DJ changed, by action (the interface's control ids): the actions
    /// left out keep the keys Prisme gives them, an empty list takes them off.
    pub keys: BTreeMap<String, Vec<KeyPreference>>,
    /// Sound cards the DJ chose not to use when plugged in, as their system tells them apart
    /// ([`Card::id`](prisme_audio_io::Card)): not offered again.
    pub declined_sound_cards: Vec<String>,
    pub ui: UiPreferences,
    pub updates: UpdatePreferences,
}

/// How a computer brings Prisme's new versions.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct UpdatePreferences {
    /// Downloaded and put in place by themselves, for the next start; else when the DJ asks.
    pub automatic: bool,
}

impl Default for UpdatePreferences {
    fn default() -> Self {
        Self { automatic: true }
    }
}

/// How the interface shows itself, chosen in it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct UiPreferences {
    /// One of [`UiPreferences::LANGUAGES`], else the system's.
    pub language: Option<String>,
    /// On a phone, the decks' touch controls shown or not, else as what is plugged in says.
    pub phone_touch: Option<bool>,
    /// The main window full screen at the start, not only filling its screen.
    pub start_fullscreen: bool,
    /// The screen the decks are on in a window of their own (its id, see `list_screens`), else in the main
    /// window.
    pub decks_screen: Option<String>,
    /// How the decks are laid out in their own window.
    pub decks_layout: DecksLayout,
}

/// How the decks are laid out in their own window.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum DecksLayout {
    /// As the screen's shape asks: lanes on a screen far wider than high, else as a controller.
    #[default]
    Auto,
    /// As a controller: the waveforms across the screen, the pads and the mixer under them.
    Stage,
    /// A lane a deck, as in the main window.
    Lanes,
}

impl UiPreferences {
    /// The languages of the interface.
    pub const LANGUAGES: [&str; 2] = ["en", "fr"];
}

/// A key of the keyboard (`KeyboardEvent.code`, so the layout does not matter), with the
/// modifiers held: Control is Command on a Mac.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "Key"))]
pub struct KeyPreference {
    pub code: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ctrl: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub shift: bool,
}

/// The decks a file holds, fewer than there are (two, before there could be four): the
/// others take the defaults.
fn decks_of<'de, D: Deserializer<'de>>(file: D) -> Result<[DeckPreferences; MAX_DECKS], D::Error> {
    let mut saved = Vec::<DeckPreferences>::deserialize(file)?.into_iter();
    Ok(std::array::from_fn(|_| saved.next().unwrap_or_default()))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DeckPreferences {
    pub vinyl_enabled: bool,
    pub vinyl_mode: VinylModeDto,
    pub rpm: RpmDto,
    /// A lead-in chosen for the deck's record, else the record's own. Under a name of its
    /// own: before records had theirs, every file held a lead-in of 0.
    #[serde(rename = "chosenLeadInS")]
    pub lead_in_s: Option<f64>,
    /// The turntable's left and right channels are crossed.
    pub swap_channels: bool,
    /// The record on its turntable (a timecode format's id), once chosen.
    pub timecode: Option<String>,
    /// The pitch stays when the speed changes.
    pub key_lock: bool,
    /// Software mixer: the channel fader raised from closed plays the deck, closed it stops
    /// it on its cue point.
    pub fader_start: bool,
    /// The range of the deck's tempo fader, if it has one of its own; else the general one
    /// ([`DjPreferences::pitch_range`]), as every deck had before.
    pub pitch_range: Option<f64>,
    /// Sound file in each sampler slot.
    pub samples: Vec<Option<String>>,
    /// The level of the deck's sampler (linear).
    pub sampler_gain: f32,
    /// The deck's size in beats: what its jumps go by, what its sized loop lasts.
    pub size_beats: f64,
}

impl DeckPreferences {
    /// The sizes a deck can have, in beats.
    pub const SIZE_BEATS: RangeInclusive<f64> = 1.0 / 16.0..=64.0;
    const DEFAULT_SIZE_BEATS: f64 = 4.0;

    /// The deck's size, the default if the file holds one out of reach.
    pub fn size(&self) -> f64 {
        if Self::SIZE_BEATS.contains(&self.size_beats) {
            self.size_beats
        } else {
            Self::DEFAULT_SIZE_BEATS
        }
    }

    /// The record on its turntable: the first Prisme reads if none was chosen, or one it
    /// does not know.
    pub fn timecode_format(&self) -> &'static TimecodeFormat {
        self.timecode.as_deref().and_then(TimecodeFormat::by_id).unwrap_or(&SERATO_CV02_A)
    }

    /// The level of the deck's sampler, unity if the file holds none that can be.
    pub fn sampler_level(&self) -> f32 {
        if self.sampler_gain.is_finite() && self.sampler_gain >= 0.0 {
            self.sampler_gain
        } else {
            1.0
        }
    }
}

impl Default for DeckPreferences {
    fn default() -> Self {
        Self {
            vinyl_enabled: false,
            vinyl_mode: VinylModeDto::default(),
            rpm: RpmDto::default(),
            lead_in_s: None,
            swap_channels: false,
            timecode: None,
            key_lock: false,
            fader_start: false,
            pitch_range: None,
            samples: Vec::new(),
            sampler_gain: 1.0,
            size_beats: Self::DEFAULT_SIZE_BEATS,
        }
    }
}

/// What SYNC does on a record in relative mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "VinylSyncMode"))]
pub enum VinylSync {
    #[default]
    Once,
    Locked,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct DjPreferences {
    pub vinyl_sync: VinylSync,
    pub quantize: bool,
    /// Range of the tempo faders of the decks without one of their own (0.08 = ±8 %).
    pub pitch_range: f64,
    /// Every track played at the same loudness (see `LoadedTrack::level`).
    pub auto_gain: bool,
    pub deck_count: DeckCount,
    /// The knobs with a middle (a mark at their top) hold there when turned through it.
    pub knob_magnet: bool,
    /// On a phone, the decks play and pause from its lock screen and its notifications.
    pub lock_controls: bool,
    /// The headphones go to the deck a track is loaded on, alone.
    pub cue_on_load: bool,
    /// A load onto a deck that plays is refused, rather than asked about.
    pub protect_playing: bool,
}

impl Preferences {
    /// Takes over what a file from an older Prisme kept elsewhere: the record every turntable
    /// had goes to each without one of its own, the separation on load is settled (see
    /// [`LibraryPreferences::settle`]). `true` if something changed.
    pub fn settle(&mut self) -> bool {
        let library = self.library.settle();
        let timecode = self.audio.timecode_before.take();
        for deck in self.decks.iter_mut().filter(|d| d.timecode.is_none()) {
            deck.timecode.clone_from(&timecode);
        }
        library || timecode.is_some()
    }

    /// The range of the tempo fader of `deck`: its own, else the general one.
    pub fn tempo_range(&self, deck: usize) -> f64 {
        self.decks[deck].pitch_range.unwrap_or(self.dj.pitch_range)
    }
}

impl DjPreferences {
    /// The decks of the engine.
    pub fn decks(&self) -> usize {
        self.deck_count.into()
    }
}

/// Two decks, or four. Saved as the number, as the interface has it: any other than four
/// reads as two.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "usize", into = "usize")]
#[cfg_attr(test, derive(ts_rs::TS), ts(as = "usize"))]
pub enum DeckCount {
    #[default]
    Two,
    Four,
}

impl From<usize> for DeckCount {
    fn from(count: usize) -> Self {
        if count == 4 {
            DeckCount::Four
        } else {
            DeckCount::Two
        }
    }
}

impl From<DeckCount> for usize {
    fn from(count: DeckCount) -> Self {
        match count {
            DeckCount::Two => 2,
            DeckCount::Four => 4,
        }
    }
}

impl Default for DjPreferences {
    fn default() -> Self {
        Self {
            vinyl_sync: VinylSync::Once,
            quantize: true,
            pitch_range: 0.08,
            auto_gain: true,
            deck_count: DeckCount::Two,
            knob_magnet: true,
            lock_controls: true,
            cue_on_load: false,
            protect_playing: false,
        }
    }
}

/// How the Auto DJ mixes (it is off at each start).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AutoDjPreferences {
    /// Length of a mix between two tracks with a beat grid.
    pub fade_beats: u32,
    /// Length of a mix when one has none.
    pub fade_s: f64,
}

impl AutoDjPreferences {
    pub const MAX_BEATS: u32 = 128;
    pub const MAX_S: f64 = 60.0;
    /// The lengths offered, in beats and in seconds.
    pub const FADE_BEATS: [u32; 4] = [8, 16, 32, 64];
    pub const FADE_SECONDS: [f64; 4] = [5.0, 10.0, 15.0, 30.0];

    /// The lengths a mix can have: at least a beat or a second, at most a long blend.
    pub fn bounded(self) -> Self {
        let fade_s = if self.fade_s.is_finite() { self.fade_s.clamp(1.0, Self::MAX_S) } else { 10.0 };
        Self { fade_beats: self.fade_beats.clamp(1, Self::MAX_BEATS), fade_s }
    }
}

impl MixerPreferences {
    /// How far the music can duck under the microphone (dB).
    pub const TALKOVER_DEPTHS: [f32; 4] = [-6.0, -12.0, -18.0, -24.0];
}

impl Default for AutoDjPreferences {
    fn default() -> Self {
        Self { fade_beats: 16, fade_s: 10.0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MixerPreferences {
    pub mode: MixModeDto,
    /// On the sound card of a controller with no mixer of its own (a DDJ-SB2), Prisme mixes,
    /// whatever `mode` says.
    pub all_in_one_mixes: bool,
    pub headphones: HeadphonesOutputDto,
    /// Headphones: 0 = cued decks only, 1 = master only.
    pub cue_mix: f32,
    /// Headphones: the cue on the left and the master on the right, each in mono.
    pub split_cue: bool,
    pub headphone_gain: f32,
    pub crossfader_curve: CrossfaderCurveDto,
    /// The crossfader's sides swapped.
    pub crossfader_reverse: bool,
    /// Master limiter (adds 1 ms of latency to the software mixer).
    pub limiter: bool,
    /// The master's level (linear).
    pub main_gain: f32,
    /// -1 = the master on the left only, 1 = on the right only.
    pub balance: f32,
    /// The first output of the booth monitors, if they are plugged in.
    pub booth: Option<usize>,
    pub booth_gain: f32,
    pub mic: LivePreferences,
    pub aux: LivePreferences,
    /// While the microphone is on, the music ducks by [`MixerPreferences::talkover_depth_db`].
    pub talkover: bool,
    pub talkover_depth_db: f32,
    /// What was recorded last: what a recording started without asking takes again.
    pub record_source: Option<RecordSourceDto>,
}

/// Where a live source comes in, and how loud. Off again at each start.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LivePreferences {
    pub input: Option<InputChannelsDto>,
    pub level: f32,
}

impl Default for LivePreferences {
    fn default() -> Self {
        Self { input: None, level: 1.0 }
    }
}

impl Default for MixerPreferences {
    fn default() -> Self {
        Self {
            // a phone mixes itself: the controller plugged into it only sends what its
            // faders and knobs do
            mode: if cfg!(mobile) { MixModeDto::Internal } else { MixModeDto::External },
            all_in_one_mixes: true,
            headphones: HeadphonesOutputDto::Auto,
            cue_mix: 0.0,
            split_cue: false,
            headphone_gain: 1.0,
            crossfader_curve: CrossfaderCurveDto::Smooth,
            crossfader_reverse: false,
            limiter: true,
            main_gain: 1.0,
            balance: 0.0,
            booth: None,
            booth_gain: 1.0,
            mic: LivePreferences::default(),
            aux: LivePreferences::default(),
            talkover: false,
            talkover_depth_db: -18.0,
            record_source: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LibraryPreferences {
    /// The music folder chosen, else the system's (see `folders::music_root`).
    pub music_folder: Option<String>,
    /// The music folders added next to it (a USB key, a drive, a sets folder), in the order
    /// they were added. None on a phone.
    pub folders: Vec<String>,
    pub last_folder: Option<String>,
    /// Columns of the track list the DJ hid (frontend ids).
    pub hidden_columns: Vec<String>,
    /// Tempo and key written into the files' tags (see `tag_writer`).
    pub write_tags: bool,
    /// Those another program wrote (Mixed In Key, say) replaced as well.
    pub replace_tags: bool,
    /// The background analysis of the music folder waits while a deck plays.
    pub pause_while_playing: bool,
    /// A track loaded on a deck is separated right away, before the library's; one put in
    /// the prepare list in the background. Not chosen: with a graphics card only (see
    /// `stems_service::separates_on_load`).
    #[serde(rename = "stemsOnLoad")]
    pub separate_on_load: Option<bool>,
    /// The same as saved before it could be left open, when on was what nobody chose (see
    /// [`LibraryPreferences::settle`]).
    #[serde(rename = "separateOnLoad", skip_serializing)]
    #[cfg_attr(test, ts(skip))]
    separate_on_load_before: Option<bool>,
    /// The separation runs on the graphics chip when there is one worth it (else on the
    /// processor alone, minutes a track).
    pub stems_gpu: bool,
    /// The small separation models, faster, rather than the large ones (a phone's default).
    pub stems_fast: bool,
}

impl LibraryPreferences {
    /// Takes over the choice of a file saved before it could be left open: off was chosen
    /// and stays, on was the default and is left open. `true` if something changed.
    pub fn settle(&mut self) -> bool {
        match self.separate_on_load_before.take() {
            Some(false) if self.separate_on_load.is_none() => {
                self.separate_on_load = Some(false);
                true
            }
            before => before.is_some(),
        }
    }
}

impl Default for LibraryPreferences {
    fn default() -> Self {
        Self {
            music_folder: None,
            folders: Vec::new(),
            last_folder: None,
            hidden_columns: Vec::new(),
            // a phone lets an application read the music of the others, not rewrite it
            write_tags: cfg!(desktop),
            replace_tags: false,
            pause_while_playing: true,
            separate_on_load: None,
            separate_on_load_before: None,
            stems_gpu: true,
            stems_fast: cfg!(mobile),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_partial_file_keeps_the_other_defaults() {
        let p: Preferences = serde_json::from_str(r#"{ "dj": { "vinylSync": "locked" } }"#).unwrap();
        assert_eq!(p.dj.vinyl_sync, VinylSync::Locked);
        assert!(p.dj.quantize);
        assert!(p.dj.auto_gain);
        assert_eq!(p.dj.pitch_range, 0.08);
        assert_eq!(p.audio, AudioSettings::default());
        assert!(p.library.write_tags);
        assert!(!p.library.replace_tags);
        assert_eq!(p.ui, UiPreferences::default(), "a file from before the interface kept its own");
        assert!(p.library.pause_while_playing);
        assert_eq!(p.library.music_folder, None, "the system's music folder");
        assert!(p.library.folders.is_empty());
        assert_eq!(p.library.separate_on_load, None);
        assert_eq!(p.auto_dj, AutoDjPreferences { fade_beats: 16, fade_s: 10.0 });
        assert!(!p.dj.cue_on_load, "the headphones stay where they are at a load");
        assert!(!p.dj.protect_playing, "a load onto a deck that plays is asked about");
        assert!(p.keys.is_empty(), "every action keeps the keys Prisme gives it");
    }

    #[test]
    fn a_fade_length_out_of_reach_is_brought_back() {
        let bounded = AutoDjPreferences { fade_beats: 0, fade_s: f64::NAN }.bounded();
        assert_eq!(bounded, AutoDjPreferences { fade_beats: 1, fade_s: 10.0 });
        let long = AutoDjPreferences { fade_beats: 1000, fade_s: 600.0 }.bounded();
        assert_eq!(long, AutoDjPreferences { fade_beats: 128, fade_s: 60.0 });
    }

    #[test]
    fn a_file_from_before_the_live_inputs_reads_them_off() {
        let p: Preferences = serde_json::from_str(r#"{ "mixer": { "limiter": false } }"#).unwrap();
        assert!(!p.mixer.limiter);
        assert_eq!((p.mixer.balance, p.mixer.booth, p.mixer.booth_gain), (0.0, None, 1.0));
        assert_eq!(p.mixer.mic, LivePreferences::default());
        assert!(!p.mixer.talkover);
        assert!(!p.mixer.split_cue, "the headphones blend unless split");
        assert!(!p.mixer.crossfader_reverse);
        assert_eq!(p.mixer.talkover_depth_db, -18.0);
        assert_eq!(p.mixer.record_source, None, "nothing recorded yet");
    }

    #[test]
    fn separating_on_load_stays_off_where_it_was_chosen_and_open_where_it_was_the_default() {
        let saved = |json: &str| {
            let mut p: Preferences = serde_json::from_str(json).unwrap();
            p.library.settle();
            p.library.separate_on_load
        };
        assert_eq!(saved(r#"{ "library": { "separateOnLoad": true } }"#), None);
        assert_eq!(saved(r#"{ "library": { "separateOnLoad": false } }"#), Some(false));
        assert_eq!(saved(r#"{ "library": { "separateOnLoad": false, "stemsOnLoad": true } }"#), Some(true));
        assert_eq!(saved(r#"{ "library": { "stemsOnLoad": true } }"#), Some(true));
        // what is saved from now on holds the choice alone
        let mut p: Preferences =
            serde_json::from_str(r#"{ "library": { "separateOnLoad": false } }"#).unwrap();
        assert!(p.library.settle());
        assert!(!p.library.settle());
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains(r#""stemsOnLoad":false"#) && !json.contains("separateOnLoad"));
    }

    #[test]
    fn the_one_record_of_a_file_from_before_each_turntable_had_its_own_goes_to_every_turntable() {
        let mut p: Preferences = serde_json::from_str(
            r#"{ "audio": { "timecodeFormat": "serato_cd" }, "decks": [{}, { "timecode": "serato_cv02_b" }] }"#,
        )
        .unwrap();
        assert!(p.settle());
        assert_eq!(
            p.decks.each_ref().map(|d| d.timecode_format().id),
            ["serato_cd", "serato_cv02_b", "serato_cd", "serato_cd"]
        );
        assert!(!serde_json::to_string(&p).unwrap().contains("timecodeFormat"));
        assert!(!p.settle());
    }

    #[test]
    fn a_turntable_without_a_record_or_with_one_unknown_reads_the_first_one() {
        let mut p: Preferences =
            serde_json::from_str(r#"{ "decks": [{}, { "timecode": "a_record_of_later" }] }"#).unwrap();
        assert!(!p.settle());
        assert_eq!(p.decks[0].timecode, None);
        assert_eq!([0, 1].map(|d| p.decks[d].timecode_format().id), ["serato_cv02_a"; 2]);
    }

    #[test]
    fn a_file_from_before_four_decks_keeps_its_two() {
        let p: Preferences =
            serde_json::from_str(r#"{ "decks": [{ "keyLock": true }, { "rpm": "45" }] }"#).unwrap();
        assert!(p.decks[0].key_lock);
        assert_eq!(p.decks[1].rpm, RpmDto::Rpm45);
        assert_eq!(p.decks[2], DeckPreferences::default());
        assert!(!p.decks[0].fader_start, "fader start is off unless chosen");
        assert_eq!(p.decks[0].pitch_range, None);
        assert_eq!((p.decks[0].sampler_level(), p.decks[0].size()), (1.0, 4.0), "as before they were kept");
    }

    #[test]
    fn a_size_or_a_sampler_level_out_of_reach_reads_as_the_default() {
        let p: Preferences = serde_json::from_str(
            r#"{ "decks": [{ "sizeBeats": 0, "samplerGain": -1 }, { "sizeBeats": 0.0625, "samplerGain": 0 }] }"#,
        )
        .unwrap();
        assert_eq!((p.decks[0].size(), p.decks[0].sampler_level()), (4.0, 1.0));
        assert_eq!((p.decks[1].size(), p.decks[1].sampler_level()), (0.0625, 0.0));
    }

    #[test]
    fn a_file_from_before_the_ranges_of_the_decks_gives_them_all_the_general_one() {
        let p: Preferences =
            serde_json::from_str(r#"{ "dj": { "pitchRange": 0.16 }, "decks": [{ "pitchRange": 0.5 }] }"#)
                .unwrap();
        assert_eq!(p.tempo_range(0), 0.5);
        assert_eq!([1, 2, 3].map(|deck| p.tempo_range(deck)), [0.16; 3]);
        assert_eq!(p.dj.decks(), 2);
    }

    #[test]
    fn two_decks_or_four_are_saved_as_their_number() {
        let read = |json: &str| serde_json::from_str::<DjPreferences>(json).unwrap().deck_count;
        assert_eq!(read(r#"{ "deckCount": 4 }"#), DeckCount::Four);
        assert_eq!(read(r#"{ "deckCount": 2 }"#), DeckCount::Two);
        assert_eq!(read(r#"{ "deckCount": 3 }"#), DeckCount::Two);
        assert_eq!(read("{}"), DeckCount::Two);
        let four = DjPreferences { deck_count: DeckCount::Four, ..Default::default() };
        assert!(serde_json::to_string(&four).unwrap().contains(r#""deckCount":4"#));
        assert_eq!(four.decks(), 4);
    }

    #[test]
    fn the_keys_of_the_dj_read_with_the_modifiers_they_hold() {
        let p: Preferences = serde_json::from_str(
            r#"{ "keys": { "deck3.play": [{ "code": "KeyG", "shift": true }], "general.help": [] } }"#,
        )
        .unwrap();
        assert_eq!(p.keys["deck3.play"], [KeyPreference { code: "KeyG".into(), ctrl: false, shift: true }]);
        assert!(p.keys["general.help"].is_empty());
        let json = serde_json::to_string(&p.keys).unwrap();
        assert_eq!(json, r#"{"deck3.play":[{"code":"KeyG","shift":true}],"general.help":[]}"#);
    }

    #[test]
    fn preferences_survive_a_round_trip() {
        let mut p = Preferences::default();
        p.decks[1] = DeckPreferences {
            vinyl_enabled: true,
            vinyl_mode: VinylModeDto::Relative,
            rpm: RpmDto::Rpm45,
            lead_in_s: Some(2.5),
            swap_channels: true,
            timecode: Some("serato_cd".into()),
            key_lock: true,
            fader_start: true,
            pitch_range: Some(0.5),
            samples: vec![Some("C:/Samples/horn.wav".into()), None],
            sampler_gain: 0.5,
            size_beats: 0.25,
        };
        p.library.music_folder = Some("D:/Music".into());
        p.library.folders = vec!["E:/".into(), "D:/Sets".into()];
        p.library.last_folder = Some("C:/Music/House".into());
        p.mixer.booth = Some(4);
        p.auto_dj = AutoDjPreferences { fade_beats: 32, fade_s: 15.0 };
        p.mixer.split_cue = true;
        p.mixer.record_source = Some(RecordSourceDto::Input { first: 4 });
        p.mixer.crossfader_reverse = true;
        p.mixer.mic =
            LivePreferences { input: Some(InputChannelsDto { first: 4, stereo: false }), level: 0.5 };
        p.keys = BTreeMap::from([
            ("deck1.play".into(), vec![KeyPreference { code: "KeyG".into(), ctrl: true, shift: false }]),
            ("library.search".into(), Vec::new()),
        ]);
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(serde_json::from_str::<Preferences>(&json).unwrap(), p);
    }
}
