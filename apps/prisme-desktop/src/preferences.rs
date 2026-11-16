//! What the user chooses once and finds again at the next start.

use crate::dto::{
    AudioSettings, CrossfaderCurveDto, HeadphonesOutputDto, InputChannelsDto, MixModeDto, RpmDto,
    VinylModeDto,
};
use prisme_engine::MAX_DECKS;
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Preferences {
    pub audio: AudioSettings,
    #[serde(deserialize_with = "decks_of")]
    pub decks: [DeckPreferences; MAX_DECKS],
    pub dj: DjPreferences,
    pub mixer: MixerPreferences,
    pub library: LibraryPreferences,
}

/// The decks a file holds, fewer than there are (two, before there could be four): the
/// others take the defaults.
fn decks_of<'de, D: Deserializer<'de>>(file: D) -> Result<[DeckPreferences; MAX_DECKS], D::Error> {
    let mut saved = Vec::<DeckPreferences>::deserialize(file)?.into_iter();
    Ok(std::array::from_fn(|_| saved.next().unwrap_or_default()))
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
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
}

/// What SYNC does on a record in relative mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VinylSync {
    #[default]
    Once,
    Locked,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DjPreferences {
    pub vinyl_sync: VinylSync,
    pub quantize: bool,
    /// Range of the tempo faders of the decks without one of their own (0.08 = ±8 %).
    pub pitch_range: f64,
    /// Every track played at the same loudness (see `LoadedTrack::level`).
    pub auto_gain: bool,
    pub deck_count: DeckCount,
    /// The knobs of the mixer hold at their middle when turned through it.
    pub knob_magnet: bool,
    /// On a phone, the decks play and pause from its lock screen and its notifications.
    pub lock_controls: bool,
}

impl Preferences {
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
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MixerPreferences {
    pub mode: MixModeDto,
    pub headphones: HeadphonesOutputDto,
    /// Headphones: 0 = cued decks only, 1 = master only.
    pub cue_mix: f32,
    pub headphone_gain: f32,
    pub crossfader_curve: CrossfaderCurveDto,
    /// Master limiter (adds 1 ms of latency to the software mixer).
    pub limiter: bool,
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
            headphones: HeadphonesOutputDto::Auto,
            cue_mix: 0.0,
            headphone_gain: 1.0,
            crossfader_curve: CrossfaderCurveDto::Smooth,
            limiter: true,
            balance: 0.0,
            booth: None,
            booth_gain: 1.0,
            mic: LivePreferences::default(),
            aux: LivePreferences::default(),
            talkover: false,
            talkover_depth_db: -18.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LibraryPreferences {
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
        assert!(p.library.pause_while_playing);
        assert_eq!(p.library.separate_on_load, None);
    }

    #[test]
    fn a_file_from_before_the_live_inputs_reads_them_off() {
        let p: Preferences = serde_json::from_str(r#"{ "mixer": { "limiter": false } }"#).unwrap();
        assert!(!p.mixer.limiter);
        assert_eq!((p.mixer.balance, p.mixer.booth, p.mixer.booth_gain), (0.0, None, 1.0));
        assert_eq!(p.mixer.mic, LivePreferences::default());
        assert!(!p.mixer.talkover);
        assert_eq!(p.mixer.talkover_depth_db, -18.0);
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
    fn a_file_from_before_four_decks_keeps_its_two() {
        let p: Preferences =
            serde_json::from_str(r#"{ "decks": [{ "keyLock": true }, { "rpm": "45" }] }"#).unwrap();
        assert!(p.decks[0].key_lock);
        assert_eq!(p.decks[1].rpm, RpmDto::Rpm45);
        assert_eq!(p.decks[2], DeckPreferences::default());
        assert!(!p.decks[0].fader_start, "fader start is off unless chosen");
        assert_eq!(p.decks[0].pitch_range, None);
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
    fn preferences_survive_a_round_trip() {
        let mut p = Preferences::default();
        p.decks[1] = DeckPreferences {
            vinyl_enabled: true,
            vinyl_mode: VinylModeDto::Relative,
            rpm: RpmDto::Rpm45,
            lead_in_s: Some(2.5),
            swap_channels: true,
            key_lock: true,
            fader_start: true,
            pitch_range: Some(0.5),
            samples: vec![Some("C:/Samples/horn.wav".into()), None],
        };
        p.library.last_folder = Some("C:/Music/House".into());
        p.mixer.booth = Some(4);
        p.mixer.mic =
            LivePreferences { input: Some(InputChannelsDto { first: 4, stereo: false }), level: 0.5 };
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(serde_json::from_str::<Preferences>(&json).unwrap(), p);
    }
}
