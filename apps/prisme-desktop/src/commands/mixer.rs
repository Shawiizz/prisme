use crate::dto::{
    CrossfaderSideDto, DeckSettingsDto, HeadphonesOutputDto, LiveSourceDto, LiveStateDto, MixModeDto,
    MixerStateDto,
};
use crate::error::AppResult;
use crate::sampler;
use crate::state::{free_inputs, live_preferences, lock, AppState};
use prisme_engine::Command;
use tauri::State;

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

/// External or software mixer, kept for the next start: the mode in use follows (an external
/// mixer needs an output pair per deck).
#[tauri::command]
pub fn set_mix_mode(state: State<'_, AppState>, mode: MixModeDto) -> AppResult<()> {
    state.update_preferences(|p| p.mixer.mode = mode);
    route(&state)
}

/// Where the headphones go, kept for the next start.
#[tauri::command]
pub fn set_headphones_output(state: State<'_, AppState>, output: HeadphonesOutputDto) -> AppResult<()> {
    state.update_preferences(|p| p.mixer.headphones = output);
    route(&state)
}

/// The booth monitors' outputs (the first of the pair), kept for the next start.
#[tauri::command]
pub fn set_booth_output(state: State<'_, AppState>, first: Option<usize>) -> AppResult<()> {
    state.update_preferences(|p| p.mixer.booth = first);
    route(&state)
}

/// Applies the routing the preferences give to the running audio, if any.
fn route(state: &AppState) -> AppResult<()> {
    let Some(info) = lock(&state.session).as_ref().map(|s| s.info.clone()) else { return Ok(()) };
    state.send(Command::SetRouting { routing: state.routing(&info) })
}

/// Master limiter; kept for the next start.
#[tauri::command]
pub fn set_limiter(state: State<'_, AppState>, enabled: bool) -> AppResult<()> {
    state.update_preferences(|p| p.mixer.limiter = enabled);
    state.send_if_running(Command::SetLimiter { enabled })
}
