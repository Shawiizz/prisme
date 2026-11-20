use crate::actions::general;
use crate::audio_host::{deck_pair, layer_partner};
use crate::choices::{self, OutputNote};
use crate::dto::{
    AudioInfo, DeckSettingsDto, HeadphonesOutputDto, LiveStateDto, MixModeDto, MixerStateDto, RoutingStateDto,
};
use crate::error::AppResult;
use crate::sampler;
use crate::state::{free_inputs, live_channels, live_preferences, lock, AppState, EXTERNAL_MIXER_OUTPUTS};
use prisme_actions::knobs;
use prisme_actions::switch::LiveSource;
use prisme_actions::view::View;
use prisme_engine::{Command, Rpm, MAX_DECKS};
use tauri::State;

/// The mixer of `state`, its knobs as `view` shows them.
pub fn mixer_state_of(state: &AppState, view: &View) -> MixerStateDto {
    let mixer = state.preferences.read(|p| p.mixer);
    MixerStateDto {
        knobs: knobs::mixer(view),
        split_cue: mixer.split_cue,
        crossfader_curve: mixer.crossfader_curve,
        crossfader_reverse: mixer.crossfader_reverse,
        limiter: mixer.limiter,
        talkover: mixer.talkover,
        talkover_depth_db: mixer.talkover_depth_db,
        record_source: mixer.record_source,
        last_recording: lock(&state.recorded).clone(),
    }
}

/// Where the mixer of `state` is heard on the running device (laid out for `decks_in_use`
/// without one), and the live inputs it takes.
pub fn routing_of(state: &AppState, decks_in_use: usize) -> RoutingStateDto {
    let mixer = state.preferences.read(|p| p.mixer);
    let info = lock(&state.session).as_ref().map(|s| s.info.clone());
    let routing = info.as_ref().map(|info| state.routing(info));
    let switches = *lock(&state.live);
    let free = info.as_ref().map(free_inputs).unwrap_or_default();
    let live = |source: LiveSource| {
        let chosen = live_preferences(&mixer, source).input;
        let other = live_preferences(&mixer, source.other()).input;
        let switches = switches[source as usize];
        let in_use = live_channels(chosen, &free).is_some();
        LiveStateDto {
            input: chosen,
            in_use,
            choices: choices::inputs(&free, other, chosen, source == LiveSource::Mic),
            note: info.as_ref().and_then(|_| choices::live_note(chosen, in_use, &free)),
            on: switches.on,
            cue: switches.cue,
        }
    };
    let outputs = info.as_ref().map_or(0, |info| info.output_channels);
    let internal = routing.map_or(mixer.mode, |r| r.mode.into()) == MixModeDto::Internal;
    let headphones_in_use = routing.and_then(|r| r.headphones);
    let booth_in_use = routing.and_then(|r| r.booth);
    let laid_out = info.as_ref().map_or(decks_in_use, |info| info.decks);
    RoutingStateDto {
        mode: mixer.mode,
        internal,
        fallback: (mixer.mode == MixModeDto::External && internal && info.is_some())
            .then_some(EXTERNAL_MIXER_OUTPUTS),
        deck_outputs: (0..laid_out).map(|deck| 2 * deck_pair(deck, laid_out, outputs)).collect(),
        headphones: mixer.headphones,
        headphones_in_use,
        headphones_choices: choices::headphones(outputs, mixer.headphones),
        headphones_note: info.as_ref().map(|_| match headphones_in_use {
            Some(first) => OutputNote::On { first },
            None => OutputNote::NotHeard,
        }),
        booth: mixer.booth,
        booth_in_use,
        booth_choices: choices::booth(outputs, headphones_in_use.filter(|_| internal), mixer.booth),
        booth_note: info
            .as_ref()
            .filter(|_| internal)
            .and_then(|_| choices::booth_note(booth_in_use, mixer.booth, headphones_in_use)),
        mic: live(LiveSource::Mic),
        aux: live(LiveSource::Aux),
        record_options: general::record_options(internal, info.as_ref()),
    }
}

/// The decks' settings as the interface shows them, `view` showing their knobs.
pub fn decks_of(state: &AppState, view: &View) -> Vec<DeckSettingsDto> {
    let ranges = state.preferences.read(|p| std::array::from_fn::<_, MAX_DECKS, _>(|d| p.tempo_range(d)));
    let info = lock(&state.session).as_ref().map(|s| s.info.clone());
    let samplers = lock(&state.samplers);
    let laid_out = info.as_ref().map_or(view.decks_in_use, |info| info.decks);
    let inputs = info.as_ref().map(|info| info.input_channels);
    let turntables = info.as_ref().is_some_and(|info| info.simulated || info.input_channels >= 2);
    state
        .decks_laid_out(info.as_ref().map(AudioInfo::layout))
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

/// External or software mixer, kept for the next start: the mode in use follows (an external
/// mixer needs an output pair per deck). Applied with the audio settings.
pub fn set_mix_mode(state: &AppState, mode: MixModeDto) -> AppResult<()> {
    state.update_preferences(|p| p.mixer.mode = mode);
    route(state)
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
