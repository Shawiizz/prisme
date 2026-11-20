use crate::actions::mixer;
use crate::auto_dj::AutoDj;
use crate::dto::{
    BandDto, CrossfaderCurveDto, CrossfaderSideDto, DeckSettingsDto, HeadphonesOutputDto, LiveSourceDto,
    LiveStateDto, MixModeDto, MixerStateDto,
};
use crate::error::AppResult;
use crate::sampler;
use crate::state::{free_inputs, live_preferences, lock, AppState};
use prisme_engine::{Command, OutputRouting};
use tauri::State;

/// Deck and mixer settings as the backend knows them (to rebuild the UI).
#[tauri::command]
pub fn mixer_state(state: State<'_, AppState>) -> MixerStateDto {
    mixer_state_of(&state)
}

/// The settings of the decks and the mixer of `state`.
pub fn mixer_state_of(state: &AppState) -> MixerStateDto {
    let mixer = state.preferences.read(|p| p.mixer);
    let info = lock(&state.session).as_ref().map(|s| s.info.clone());
    let routing = info.as_ref().map(|info| state.routing(info));
    let samplers = lock(&state.samplers);
    let switches = *lock(&state.live);
    let live = |source: LiveSourceDto| {
        let chosen = live_preferences(&mixer, source);
        let switches = switches[source as usize];
        LiveStateDto {
            input: chosen.input,
            in_use: state.live_input(source, info.as_ref()).is_some(),
            level: chosen.level,
            on: switches.on,
            cue: switches.cue,
        }
    };
    MixerStateDto {
        decks: state
            .decks_in_use(info.as_ref())
            .iter()
            .zip(samplers.iter())
            .enumerate()
            .map(|(deck, (d, samples))| DeckSettingsDto {
                pitch: d.pitch,
                volume: d.volume,
                cue: d.cue,
                vinyl_enabled: d.vinyl_enabled,
                vinyl_mode: d.vinyl_mode,
                rpm: d.rpm,
                lead_in_s: d.lead_in_s,
                swap_channels: d.swap_channels,
                timecode: d.timecode.id,
                key_lock: d.key_lock,
                fader_start: d.fader_start,
                pitch_range: d.pitch_range,
                strip: d.strip,
                fx: d.fx,
                samples: samples.iter().map(|s| s.as_ref().map(|s| sampler::name(&s.path))).collect(),
                sampler_gain: d.sampler_gain,
                size_beats: d.size_beats,
                passthrough: d.passthrough,
                side: d.side.unwrap_or(CrossfaderSideDto::of_deck(deck)),
                jog_scratch: d.jog_scratch,
                repeat: d.repeat,
                mute: d.mute,
            })
            .collect(),
        crossfader: *lock(&state.crossfader),
        mode: mixer.mode,
        mode_in_use: routing.map(|r| r.mode.into()),
        headphones: mixer.headphones,
        headphones_in_use: routing.and_then(|r| r.headphones),
        output_channels: info.as_ref().map(|info| info.output_channels),
        cue_mix: mixer.cue_mix,
        split_cue: mixer.split_cue,
        headphone_gain: mixer.headphone_gain,
        crossfader_curve: mixer.crossfader_curve,
        crossfader_reverse: mixer.crossfader_reverse,
        limiter: mixer.limiter,
        main_gain: mixer.main_gain,
        balance: mixer.balance,
        booth: mixer.booth,
        booth_in_use: routing.and_then(|r| r.booth),
        booth_gain: mixer.booth_gain,
        mic: live(LiveSourceDto::Mic),
        aux: live(LiveSourceDto::Aux),
        talkover: mixer.talkover,
        talkover_depth_db: mixer.talkover_depth_db,
        free_inputs: info.as_ref().map(free_inputs).unwrap_or_default(),
        record_source: mixer.record_source,
    }
}

/// External or software mixer, kept for the next start; returns the mode in use.
#[tauri::command]
pub fn set_mix_mode(state: State<'_, AppState>, mode: MixModeDto) -> AppResult<Option<MixModeDto>> {
    state.update_preferences(|p| p.mixer.mode = mode);
    Ok(route(&state)?.map(|routing| routing.mode.into()))
}

/// Where the headphones go, kept for the next start; returns their first output in use.
#[tauri::command]
pub fn set_headphones_output(
    state: State<'_, AppState>,
    output: HeadphonesOutputDto,
) -> AppResult<Option<usize>> {
    state.update_preferences(|p| p.mixer.headphones = output);
    Ok(route(&state)?.and_then(|routing| routing.headphones))
}

/// The booth monitors' outputs (the first of the pair), kept for the next start; returns the
/// first in use.
#[tauri::command]
pub fn set_booth_output(state: State<'_, AppState>, first: Option<usize>) -> AppResult<Option<usize>> {
    state.update_preferences(|p| p.mixer.booth = first);
    Ok(route(&state)?.and_then(|routing| routing.booth))
}

/// Kept for the next start.
#[tauri::command]
pub fn set_booth_gain(state: State<'_, AppState>, gain: f32) -> AppResult<()> {
    mixer::set_booth_gain(&state, gain)
}

/// The master's level (linear); kept for the next start.
#[tauri::command]
pub fn set_main_gain(state: State<'_, AppState>, gain: f32) -> AppResult<()> {
    mixer::set_main_gain(&state, gain)
}

/// The side of the crossfader `deck` is on.
#[tauri::command]
pub fn set_crossfader_side(
    state: State<'_, AppState>,
    deck: usize,
    side: CrossfaderSideDto,
) -> AppResult<()> {
    AppState::check_deck(deck)?;
    lock(&state.decks)[deck].side = Some(side);
    state.send_if_running(Command::SetCrossfaderSide { deck, side: side.into() })
}

/// -1 = the master on the left only, 1 = on the right only; kept for the next start.
#[tauri::command]
pub fn set_balance(state: State<'_, AppState>, balance: f32) -> AppResult<()> {
    mixer::set_balance(&state, balance)
}

/// Applies the routing the preferences give to the running audio, if any.
fn route(state: &AppState) -> AppResult<Option<OutputRouting>> {
    let Some(info) = lock(&state.session).as_ref().map(|s| s.info.clone()) else { return Ok(None) };
    let routing = state.routing(&info);
    state.send(Command::SetRouting { routing })?;
    Ok(Some(routing))
}

/// With fader start on the software mixer, a fader raised from closed plays its deck, and
/// closed stops it on its cue point.
#[tauri::command]
pub fn set_volume(state: State<'_, AppState>, deck: usize, gain: f32) -> AppResult<()> {
    mixer::set_volume(&state, deck, gain)
}

/// Software mixer: the channel fader starts and stops the deck; kept for the next start.
#[tauri::command]
pub fn set_fader_start(state: State<'_, AppState>, deck: usize, enabled: bool) -> AppResult<()> {
    AppState::check_deck(deck)?;
    lock(&state.decks)[deck].fader_start = enabled;
    state.update_preferences(|p| p.decks[deck].fader_start = enabled);
    Ok(())
}

#[tauri::command]
pub fn set_crossfader(
    state: State<'_, AppState>,
    auto_dj: State<'_, AutoDj>,
    position: f32,
) -> AppResult<()> {
    mixer::set_crossfader(&state, Some(&auto_dj), position)
}

/// Sends the deck to the headphones (software mixer).
#[tauri::command]
pub fn set_deck_cue(state: State<'_, AppState>, deck: usize, enabled: bool) -> AppResult<()> {
    AppState::check_deck(deck)?;
    lock(&state.decks)[deck].cue = enabled;
    state.send(Command::SetDeckCue { deck, enabled })
}

/// The crossfader's sides swapped, or back. Kept for the next start.
#[tauri::command]
pub fn set_crossfader_reverse(state: State<'_, AppState>, enabled: bool) -> AppResult<()> {
    state.update_preferences(|p| p.mixer.crossfader_reverse = enabled);
    state.send_if_running(Command::SetCrossfaderReverse { enabled })
}

/// The deck's channel silenced in the master, after its fader, or heard again.
#[tauri::command]
pub fn set_mute(state: State<'_, AppState>, deck: usize, enabled: bool) -> AppResult<()> {
    AppState::check_deck(deck)?;
    lock(&state.decks)[deck].mute = enabled;
    state.send_if_running(Command::SetMute { deck, muted: enabled })
}

/// Headphones: 0 = cued decks only, 1 = master only. Kept for the next start.
#[tauri::command]
pub fn set_cue_mix(state: State<'_, AppState>, mix: f32) -> AppResult<()> {
    mixer::set_cue_mix(&state, mix)
}

/// Headphones: the cue on the left and the master on the right, each in mono, or blended.
/// Kept for the next start.
#[tauri::command]
pub fn set_split_cue(state: State<'_, AppState>, enabled: bool) -> AppResult<()> {
    state.update_preferences(|p| p.mixer.split_cue = enabled);
    state.send_if_running(Command::SetSplitCue { enabled })
}

#[tauri::command]
pub fn set_headphone_gain(state: State<'_, AppState>, gain: f32) -> AppResult<()> {
    mixer::set_headphone_gain(&state, gain)
}

/// Software mixer: gain before the EQ (linear).
#[tauri::command]
pub fn set_trim(state: State<'_, AppState>, deck: usize, gain: f32) -> AppResult<()> {
    mixer::set_trim(&state, deck, gain)
}

/// Software mixer: EQ band gain (linear, 1 = flat).
#[tauri::command]
pub fn set_eq(state: State<'_, AppState>, deck: usize, band: BandDto, gain: f32) -> AppResult<()> {
    mixer::set_eq(&state, deck, band, gain)
}

#[tauri::command]
pub fn set_eq_kill(state: State<'_, AppState>, deck: usize, band: BandDto, kill: bool) -> AppResult<()> {
    mixer::set_eq_kill(&state, deck, band, kill)
}

/// Software mixer: -1 = low-pass closed, 0 = off, 1 = high-pass open.
#[tauri::command]
pub fn set_filter(state: State<'_, AppState>, deck: usize, position: f32) -> AppResult<()> {
    mixer::set_filter(&state, deck, position)
}

/// Kept for the next start.
#[tauri::command]
pub fn set_crossfader_curve(state: State<'_, AppState>, curve: CrossfaderCurveDto) -> AppResult<()> {
    mixer::set_crossfader_curve(&state, curve)
}

/// Master limiter; kept for the next start.
#[tauri::command]
pub fn set_limiter(state: State<'_, AppState>, enabled: bool) -> AppResult<()> {
    state.update_preferences(|p| p.mixer.limiter = enabled);
    state.send_if_running(Command::SetLimiter { enabled })
}
