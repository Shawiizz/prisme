//! The engine and its controller.
//!
//! [`create`] returns two halves: the [`Engine`], which lives on the audio thread and is
//! called from the sound card callback, and the [`EngineController`], which lives on the
//! control side (UI, MIDI). They only talk through lock-free queues:
//!
//! * commands: controller -> engine;
//! * status: engine -> controller, through a triple buffer (the controller always reads
//!   the most recent snapshot, however rarely it looks);
//! * garbage (replaced tracks and decoders): engine -> controller, freed there, so the
//!   audio thread never frees memory.

use crate::command::{Command, DeckId, Garbage, RecordSource};
use crate::deck::Deck;
use crate::mixer::{DeckMix, MixMode, Mixer};
use crate::recording::{input_pair, Recording};
use crate::scope::{self, ScopeTap};
use crate::status::{EngineStatus, RecordingStatus, MAX_DECKS};
use crate::sync::LeaderClock;
use prisme_dsp::DenormalGuard;
use prisme_timecode::{DecoderConfig, TimecodeDecoder, TimecodeFormat};

const COMMAND_QUEUE: usize = 256;
const GARBAGE_QUEUE: usize = 64;

/// Engine settings, fixed for the lifetime of an engine.
#[derive(Debug, Clone, Copy)]
pub struct EngineConfig {
    pub sample_rate: u32,
    /// Largest block the engine processes at once (longer callbacks are split).
    pub max_block_frames: usize,
    pub decks: usize,
    pub timecode: &'static TimecodeFormat,
    /// First input channel of each deck's turntable (stereo pair), if any.
    pub deck_inputs: [Option<usize>; MAX_DECKS],
    /// Status snapshots per second (at most one per callback).
    pub status_rate_hz: f64,
}

impl EngineConfig {
    /// Sensible defaults: deck N reads input channels 2N and 2N+1.
    pub fn new(sample_rate: u32, decks: usize, timecode: &'static TimecodeFormat) -> Self {
        Self {
            sample_rate,
            max_block_frames: 1024,
            decks: decks.clamp(1, MAX_DECKS),
            timecode,
            deck_inputs: [Some(0), Some(2), Some(4), Some(6)],
            status_rate_hz: 100.0,
        }
    }

    /// Builds a decoder for this configuration (allocates: control side only).
    pub fn decoder(&self, swap_channels: bool) -> Box<TimecodeDecoder> {
        Box::new(TimecodeDecoder::new(DecoderConfig {
            format: self.timecode,
            sample_rate: self.sample_rate,
            swap_channels,
        }))
    }
}

/// Creates an engine and its controller.
pub fn create(config: EngineConfig) -> (Engine, EngineController) {
    let (cmd_tx, cmd_rx) = rtrb::RingBuffer::new(COMMAND_QUEUE);
    let initial = EngineStatus { deck_count: config.decks, ..Default::default() };
    let (status_tx, status_rx) = triple_buffer::triple_buffer(&initial);
    let (garbage_tx, garbage_rx) = rtrb::RingBuffer::new(GARBAGE_QUEUE);
    let rate = config.sample_rate as f32;
    let decks = (0..config.decks)
        .map(|_| Deck::new(config.decoder(false), config.sample_rate, config.max_block_frames))
        .collect();
    let (scope_taps, scopes) = (0..config.decks).map(|_| scope::scope()).unzip();
    let engine = Engine {
        config,
        decks,
        mixer: Mixer::new(config.max_block_frames, rate),
        commands: cmd_rx,
        status: status_tx,
        garbage: garbage_tx,
        deck_in: vec![0.0; config.max_block_frames * 2],
        scope_taps,
        deck_out: vec![0.0; config.max_block_frames * 2],
        peak: 0.0,
        quantize: true,
        chosen_leader: None,
        leader: None,
        leader_clock: LeaderClock::default(),
        frames_since_status: 0,
        status_interval: (config.sample_rate as f64 / config.status_rate_hz) as usize,
        recording: None,
        record_buffer: vec![0.0; config.max_block_frames * 2],
    };
    let controller = EngineController { commands: cmd_tx, status: status_rx, garbage: garbage_rx, scopes };
    (engine, controller)
}

/// The audio-thread half. Real-time safe: no allocation, no lock, no free.
pub struct Engine {
    config: EngineConfig,
    decks: Vec<Deck>,
    mixer: Mixer,
    commands: rtrb::Consumer<Command>,
    status: triple_buffer::Input<EngineStatus>,
    garbage: rtrb::Producer<Garbage>,
    deck_in: Vec<f32>,
    scope_taps: Vec<ScopeTap>,
    deck_out: Vec<f32>,
    peak: f32,
    quantize: bool,
    /// Leader picked by the user, if any.
    chosen_leader: Option<DeckId>,
    leader: Option<DeckId>,
    leader_clock: LeaderClock,
    frames_since_status: usize,
    status_interval: usize,
    recording: Option<Recording>,
    /// Scratch space for what is recorded.
    record_buffer: Vec<f32>,
}

impl Engine {
    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    /// Processes one audio callback. `input` and `output` are interleaved with
    /// `input_channels` / `output_channels` channels. If the input has fewer frames than
    /// the output (input stream late), the missing frames are silence.
    pub fn process(
        &mut self,
        input: &[f32],
        input_channels: usize,
        output: &mut [f32],
        output_channels: usize,
    ) {
        let _denormals = DenormalGuard::new();
        self.apply_commands();
        let out_channels = output_channels.max(1);
        let frames = output.len() / out_channels;
        let mut done = 0;
        while done < frames {
            let n = (frames - done).min(self.config.max_block_frames);
            let out = &mut output[done * out_channels..(done + n) * out_channels];
            self.process_block(input, input_channels, done, out, out_channels);
            done += n;
        }
        self.publish_status(frames);
    }

    fn process_block(&mut self, input: &[f32], in_ch: usize, offset: usize, out: &mut [f32], out_ch: usize) {
        let frames = out.len() / out_ch;
        out.fill(0.0);
        let rate = self.config.sample_rate as f64;
        self.leader = self.resolve_leader();
        // every follower is compared with the leader as it was at the start of the block
        let seconds = frames as f64 / rate;
        let leader_beat = self.leader.and_then(|l| {
            let leader = &self.decks[l];
            let beat = leader.leader_beat()?;
            Some(self.leader_clock.beat(l, beat, leader.is_looping(), seconds))
        });
        self.mixer.begin(frames);
        for d in 0..self.decks.len() {
            let target = match leader_beat {
                Some(beat) if self.leader != Some(d) && self.decks[d].follows() => {
                    self.decks[d].sync_target(beat)
                }
                _ => None,
            };
            let deck_in = &mut self.deck_in[..frames * 2];
            fill_deck_input(deck_in, input, in_ch, offset, self.config.deck_inputs[d]);
            self.scope_taps[d].push(deck_in);
            let deck_out = &mut self.deck_out[..frames * 2];
            let deck = &mut self.decks[d];
            let beat = deck.effect_beat();
            deck.process(deck_in, deck_out, rate, target);
            deck.sampler.render(deck_out, rate);
            if self.mixer.routing.mode == MixMode::Internal {
                deck.strip.process(deck_out);
            }
            deck.fx.process(deck_out, beat);
            deck.peak = deck_out.iter().fold(deck.peak, |p, s| p.max(s.abs()));
            let mix = DeckMix { audio: deck_out, volume: deck.volume, side: deck.side, cue: deck.cue };
            self.mixer.add_deck(d, mix, out, out_ch);
        }
        self.mixer.finish(frames, out, out_ch);
        if let Some(recording) = self.recording.as_mut() {
            let buffer = &mut self.record_buffer[..frames * 2];
            match recording.source {
                RecordSource::Master => buffer.copy_from_slice(self.mixer.master(frames)),
                RecordSource::Input(first) => input_pair(input, in_ch, offset, first, buffer),
            }
            recording.push(buffer);
        }
        for s in out.iter() {
            self.peak = self.peak.max(s.abs());
        }
    }

    /// The chosen leader if it can lead, else the current one if it still can, else the
    /// first deck that can.
    fn resolve_leader(&self) -> Option<DeckId> {
        let can_lead = |d: &DeckId| self.decks.get(*d).is_some_and(Deck::can_lead);
        self.chosen_leader
            .filter(can_lead)
            .or(self.leader.filter(can_lead))
            .or_else(|| (0..self.decks.len()).find(can_lead))
    }

    fn apply_commands(&mut self) {
        while let Ok(command) = self.commands.pop() {
            self.apply(command);
        }
    }

    fn apply(&mut self, command: Command) {
        use Command::*;
        let deck_count = self.decks.len();
        let valid = |deck: usize| deck < deck_count;
        match command {
            LoadTrack { deck, track } => {
                if valid(deck) {
                    let d = &mut self.decks[deck];
                    d.reset_track_state();
                    if let Some(old) = d.playhead.load(track) {
                        self.discard(Garbage::Track(old));
                    }
                } else {
                    self.discard(Garbage::Track(track));
                }
            }
            Eject { deck } if valid(deck) => {
                let d = &mut self.decks[deck];
                d.playing = false;
                d.reset_track_state();
                if let Some(old) = d.playhead.eject() {
                    self.discard(Garbage::Track(old));
                }
            }
            SetPlaying { deck, playing } if valid(deck) => self.decks[deck].playing = playing,
            SetPitch { deck, ratio } if valid(deck) => self.decks[deck].pitch = ratio,
            SetPitchBend { deck, amount } if valid(deck) => self.decks[deck].bend = amount,
            Seek { deck, seconds } if valid(deck) => self.decks[deck].playhead.seek_s(seconds),
            Jump { deck, seconds } if valid(deck) => self.decks[deck].jump(seconds, self.quantize),
            BeatJump { deck, beats } if valid(deck) => self.decks[deck].beat_jump(beats),
            SetQuantize { enabled } => self.quantize = enabled,
            SetLoop { deck, region, active } if valid(deck) => self.decks[deck].set_loop(region, active),
            BeatLoop { deck, beats } if valid(deck) => self.decks[deck].beat_loop(beats, self.quantize),
            LoopIn { deck } if valid(deck) => self.decks[deck].loop_in(self.quantize),
            LoopOut { deck } if valid(deck) => self.decks[deck].loop_out(self.quantize),
            LoopRoll { deck, beats } if valid(deck) => self.decks[deck].loop_roll(beats, self.quantize),
            ExitLoop { deck } if valid(deck) => self.decks[deck].exit_loop(),
            Reloop { deck } if valid(deck) => self.decks[deck].reloop(),
            ScaleLoop { deck, factor } if valid(deck) => self.decks[deck].scale_loop(factor),
            SetSlip { deck, enabled } if valid(deck) => self.decks[deck].set_slip(enabled),
            Censor { deck, active } if valid(deck) => self.decks[deck].censor(active),
            SetVinylEnabled { deck, enabled } if valid(deck) => {
                self.decks[deck].vinyl.enabled = enabled;
                self.decks[deck].vinyl.take_back();
                self.decks[deck].sync.reset();
            }
            SetVinylMode { deck, mode } if valid(deck) => {
                self.decks[deck].vinyl.mode = mode;
                self.decks[deck].vinyl.take_back();
                self.decks[deck].sync.reset();
            }
            SetBeatGrid { deck, grid } if valid(deck) => self.decks[deck].grid = grid,
            SetSyncLocked { deck, locked: true } if valid(deck) => self.decks[deck].engage_sync(true),
            SetSyncLocked { deck, locked: false } if valid(deck) => self.decks[deck].sync.reset(),
            SyncOnce { deck } if valid(deck) => self.decks[deck].engage_sync(false),
            SetSyncLeader { deck } => self.chosen_leader = deck.filter(|&d| valid(d)),
            SetLeadIn { deck, seconds } if valid(deck) => self.decks[deck].vinyl.lead_in_s = seconds,
            ReplaceDecoder { deck, decoder } => {
                if valid(deck) {
                    let old = self.decks[deck].vinyl.replace_decoder(decoder);
                    self.discard(Garbage::Decoder(old));
                } else {
                    self.discard(Garbage::Decoder(decoder));
                }
            }
            SetVolume { deck, gain } if valid(deck) => self.decks[deck].volume = gain,
            SetCrossfaderSide { deck, side } if valid(deck) => self.decks[deck].side = side,
            SetCrossfader { position } => self.mixer.crossfader = position,
            SetMasterGain { gain } => self.mixer.master_gain = gain,
            SetRouting { routing } => self.mixer.routing = routing,
            SetDeckCue { deck, enabled } if valid(deck) => self.decks[deck].cue = enabled,
            SetCueMix { mix } => self.mixer.cue_mix = mix,
            SetHeadphoneGain { gain } => self.mixer.headphone_gain = gain,
            SetTrim { deck, gain } if valid(deck) => self.decks[deck].strip.set_trim(gain),
            SetEq { deck, band, gain } if valid(deck) => self.decks[deck].strip.set_eq(band, gain),
            SetEqKill { deck, band, kill } if valid(deck) => self.decks[deck].strip.set_kill(band, kill),
            SetFilter { deck, position } if valid(deck) => self.decks[deck].strip.set_filter(position),
            SetKeyLock { deck, enabled } if valid(deck) => self.decks[deck].key_lock_enabled = enabled,
            SetKeyShift { deck, semitones } if valid(deck) => self.decks[deck].set_key_shift(semitones),
            SetSample { deck, slot, sample } => {
                let old = match self.decks.get_mut(deck) {
                    Some(d) => d.sampler.set(slot, sample),
                    None => sample,
                };
                if let Some(old) = old {
                    self.discard(Garbage::Track(old));
                }
            }
            TriggerSample { deck, slot } if valid(deck) => {
                let rate = self.config.sample_rate as f64;
                let d = &mut self.decks[deck];
                let wait = if self.quantize { d.frames_to_next_beat(rate).unwrap_or(0) } else { 0 };
                d.sampler.trigger(slot, wait);
            }
            StopSample { deck, slot } if valid(deck) => self.decks[deck].sampler.stop(slot),
            SetSamplerGain { deck, gain } if valid(deck) => self.decks[deck].sampler.gain = gain,
            StartRecording { source, sink } => {
                let started = Recording { source, sink, frames: 0, dropped: 0 };
                if let Some(old) = self.recording.replace(started) {
                    self.discard(Garbage::Recording(old.sink));
                }
            }
            StopRecording => {
                if let Some(old) = self.recording.take() {
                    self.discard(Garbage::Recording(old.sink));
                }
            }
            SetFx { deck, kind } if valid(deck) => self.decks[deck].fx.set_kind(kind),
            SetFxEnabled { deck, enabled } if valid(deck) => self.decks[deck].fx.set_enabled(enabled),
            SetFxMix { deck, mix } if valid(deck) => self.decks[deck].fx.set_mix(mix),
            SetFxAmount { deck, amount } if valid(deck) => self.decks[deck].fx.set_amount(amount),
            SetFxBeats { deck, beats } if valid(deck) => self.decks[deck].fx.set_beats(beats),
            SetCrossfaderCurve { curve } => self.mixer.curve = curve,
            SetLimiter { enabled } => self.mixer.limiter.set_enabled(enabled),
            _ => {} // command for a deck that does not exist
        }
    }

    /// Hands an object back to the control side. If the queue is full (the controller
    /// stopped collecting), it is leaked rather than freed on the audio thread.
    fn discard(&mut self, garbage: Garbage) {
        if let Err(rtrb::PushError::Full(garbage)) = self.garbage.push(garbage) {
            std::mem::forget(garbage);
        }
    }

    fn publish_status(&mut self, frames: usize) {
        self.frames_since_status += frames;
        if self.frames_since_status < self.status_interval {
            return;
        }
        self.frames_since_status = 0;
        let recording =
            self.recording.as_ref().map(|r| RecordingStatus { frames: r.frames, dropped: r.dropped });
        let mut status = EngineStatus {
            deck_count: self.decks.len(),
            main_peak: self.peak,
            recording,
            ..Default::default()
        };
        for (d, (slot, deck)) in status.decks.iter_mut().zip(&mut self.decks).enumerate() {
            *slot = deck.status(self.leader == Some(d));
            deck.peak = 0.0;
        }
        self.peak = 0.0;
        self.status.write(status);
    }
}

/// Copies one deck's stereo pair out of the interleaved input (silence if absent).
fn fill_deck_input(deck_in: &mut [f32], input: &[f32], in_ch: usize, offset: usize, first: Option<usize>) {
    match first {
        Some(c) if in_ch >= c + 2 => {
            for (i, frame) in deck_in.chunks_exact_mut(2).enumerate() {
                let k = (offset + i) * in_ch + c;
                if k + 1 < input.len() {
                    frame[0] = input[k];
                    frame[1] = input[k + 1];
                } else {
                    frame[0] = 0.0;
                    frame[1] = 0.0;
                }
            }
        }
        _ => deck_in.fill(0.0),
    }
}

/// The control half: send commands, read the state, free what the engine returns.
pub struct EngineController {
    commands: rtrb::Producer<Command>,
    status: triple_buffer::Output<EngineStatus>,
    garbage: rtrb::Consumer<Garbage>,
    scopes: Vec<rtrb::Consumer<[f32; 2]>>,
}

impl EngineController {
    /// Sends a command. Fails (returning it) only if the engine is not running and the
    /// queue is full.
    pub fn send(&mut self, command: Command) -> Result<(), Command> {
        self.commands.push(command).map_err(|rtrb::PushError::Full(c)| c)
    }

    /// Latest engine state.
    pub fn status(&mut self) -> EngineStatus {
        *self.status.read()
    }

    /// Frees what the engine handed back. Call it regularly (e.g. with every UI update).
    /// Appends the turntable signal points of `deck` received since the last call (one
    /// frame in four, left and right).
    pub fn read_scope(&mut self, deck: usize, points: &mut Vec<[f32; 2]>) {
        if let Some(scope) = self.scopes.get_mut(deck) {
            points.extend(std::iter::from_fn(|| scope.pop().ok()));
        }
    }

    pub fn collect_garbage(&mut self) -> usize {
        let mut freed = 0;
        while let Ok(item) = self.garbage.pop() {
            drop(item);
            freed += 1;
        }
        freed
    }
}
