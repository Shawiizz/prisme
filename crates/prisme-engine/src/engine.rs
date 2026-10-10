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

use crate::command::{Command, DeckId, Garbage};
use crate::deck::Deck;
use crate::mixer::Mixer;
use crate::status::{EngineStatus, MAX_DECKS};
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
    let decks = (0..config.decks).map(|_| Deck::new(config.decoder(false))).collect();
    let engine = Engine {
        config,
        decks,
        mixer: Mixer::new(),
        commands: cmd_rx,
        status: status_tx,
        garbage: garbage_tx,
        deck_in: vec![0.0; config.max_block_frames * 2],
        deck_out: vec![0.0; config.max_block_frames * 2],
        peak: 0.0,
        quantize: true,
        chosen_leader: None,
        leader: None,
        frames_since_status: 0,
        status_interval: (config.sample_rate as f64 / config.status_rate_hz) as usize,
    };
    let controller = EngineController { commands: cmd_tx, status: status_rx, garbage: garbage_rx };
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
    deck_out: Vec<f32>,
    peak: f32,
    quantize: bool,
    /// Leader picked by the user, if any.
    chosen_leader: Option<DeckId>,
    leader: Option<DeckId>,
    frames_since_status: usize,
    status_interval: usize,
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
        let leader_beat = self.leader.and_then(|l| self.decks[l].leader_beat());
        for d in 0..self.decks.len() {
            let target = match leader_beat {
                Some(beat) if self.leader != Some(d) && self.decks[d].follows() => {
                    self.decks[d].sync_target(beat)
                }
                _ => None,
            };
            let deck_in = &mut self.deck_in[..frames * 2];
            fill_deck_input(deck_in, input, in_ch, offset, self.config.deck_inputs[d]);
            let deck_out = &mut self.deck_out[..frames * 2];
            let deck = &mut self.decks[d];
            deck.process(deck_in, deck_out, rate, target);
            let gain = deck.volume * self.mixer.crossfader_gain(deck.side);
            self.mixer.add_deck(deck_out, gain, out, out_ch);
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
            LoopRoll { deck, beats } if valid(deck) => self.decks[deck].loop_roll(beats, self.quantize),
            ExitLoop { deck } if valid(deck) => self.decks[deck].exit_loop(),
            Reloop { deck } if valid(deck) => self.decks[deck].reloop(),
            ScaleLoop { deck, factor } if valid(deck) => self.decks[deck].scale_loop(factor),
            SetSlip { deck, enabled } if valid(deck) => self.decks[deck].set_slip(enabled),
            Censor { deck, active } if valid(deck) => self.decks[deck].censor(active),
            SetVinylEnabled { deck, enabled } if valid(deck) => {
                self.decks[deck].vinyl.enabled = enabled;
                self.decks[deck].sync.reset();
            }
            SetVinylMode { deck, mode } if valid(deck) => {
                self.decks[deck].vinyl.mode = mode;
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
        let mut status =
            EngineStatus { deck_count: self.decks.len(), main_peak: self.peak, ..Default::default() };
        for (d, (slot, deck)) in status.decks.iter_mut().zip(&self.decks).enumerate() {
            *slot = deck.status(self.leader == Some(d));
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
    pub fn collect_garbage(&mut self) -> usize {
        let mut freed = 0;
        while let Ok(item) = self.garbage.pop() {
            drop(item);
            freed += 1;
        }
        freed
    }
}
