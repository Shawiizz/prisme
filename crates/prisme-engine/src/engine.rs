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
use crate::grid::BeatGrid;
use crate::mixer::{DeckMix, MixMode, Mixer};
use crate::preview::Preview;
use crate::recording::{input_pair, Recording};
use crate::scope::{self, ScopeTap};
use crate::status::{EngineStatus, RecordingStatus, MAX_DECKS};
use crate::sync::{LeaderClock, LEADER_RESUME_S};
use prisme_dsp::{db_to_gain, DenormalGuard, FxSlot};
use prisme_timecode::{DecoderConfig, TimecodeDecoder, TimecodeFormat};

/// Room for a whole session restored at once when the audio starts again, before the engine
/// takes any: some 300 commands with four decks and all their pads.
const COMMAND_QUEUE: usize = 1024;
const GARBAGE_QUEUE: usize = 64;

/// Engine settings, fixed for the lifetime of an engine.
#[derive(Debug, Clone, Copy)]
pub struct EngineConfig {
    pub sample_rate: u32,
    /// Largest block the engine processes at once (longer callbacks are split).
    pub max_block_frames: usize,
    pub decks: usize,
    /// The record on each deck's turntable, which its decoder reads from the start.
    pub timecodes: [&'static TimecodeFormat; MAX_DECKS],
    /// First input channel of each deck's turntable (stereo pair), if any.
    pub deck_inputs: [Option<usize>; MAX_DECKS],
    /// Status snapshots per second (at most one per callback).
    pub status_rate_hz: f64,
}

impl EngineConfig {
    /// Sensible defaults: deck N reads input channels 2N and 2N+1, every turntable `timecode`.
    pub fn new(sample_rate: u32, decks: usize, timecode: &'static TimecodeFormat) -> Self {
        Self {
            sample_rate,
            max_block_frames: 1024,
            decks: decks.clamp(1, MAX_DECKS),
            timecodes: [timecode; MAX_DECKS],
            deck_inputs: [Some(0), Some(2), Some(4), Some(6)],
            status_rate_hz: 100.0,
        }
    }

    /// Builds a decoder of `format` for this configuration (allocates: control side only).
    pub fn decoder(&self, format: &'static TimecodeFormat, swap_channels: bool) -> Box<TimecodeDecoder> {
        Box::new(TimecodeDecoder::new(DecoderConfig { format, sample_rate: self.sample_rate, swap_channels }))
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
        .map(|d| {
            Deck::new(config.decoder(config.timecodes[d], false), config.sample_rate, config.max_block_frames)
        })
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
        master_peaks: [0.0; 2],
        held: HeldPeaks::default(),
        quantize: true,
        chosen_leader: None,
        leader: None,
        resumed_leader_s: 0.0,
        leader_clock: LeaderClock::default(),
        frames_since_status: 0,
        status_interval: (config.sample_rate as f64 / config.status_rate_hz) as usize,
        applied: 0,
        recording: None,
        record_buffer: vec![0.0; config.max_block_frames * 2],
        preview: Preview::new(config.max_block_frames),
    };
    let controller =
        EngineController { commands: cmd_tx, sent: 0, status: status_rx, garbage: garbage_rx, scopes };
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
    master_peaks: [f32; 2],
    held: HeldPeaks,
    quantize: bool,
    /// Leader picked by the user, if any.
    chosen_leader: Option<DeckId>,
    leader: Option<DeckId>,
    /// Seconds the leader resumed after a restart is still waited for, though it cannot lead.
    resumed_leader_s: f64,
    leader_clock: LeaderClock,
    frames_since_status: usize,
    status_interval: usize,
    /// Commands applied so far.
    applied: u64,
    recording: Option<Recording>,
    /// Scratch space for what is recorded.
    record_buffer: Vec<f32>,
    preview: Preview,
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
        let seconds = frames as f64 / rate;
        // a resumed leader gives its beats as soon as it moves, before its averaged speed has risen
        let resumed = self.resumed_leader_s > 0.0;
        let leading = self.leader.filter(|&l| {
            let deck = &self.decks[l];
            deck.can_lead() || resumed && deck.could_lead()
        });
        self.resumed_leader_s = if self.leader.is_some_and(|l| self.decks[l].can_lead()) {
            0.0
        } else {
            (self.resumed_leader_s - seconds).max(0.0)
        };
        // every follower is compared with the leader as it was at the start of the block
        let leader_beat = leading.and_then(|l| {
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
            let fx_done = deck.render(deck_in, deck_out, rate, target, beat);
            deck.sampler.render(deck_out, rate);
            match self.mixer.routing.mode {
                MixMode::Internal => deck.strip.process(deck_out),
                MixMode::External => deck.strip.filter(deck_out),
            }
            if !fx_done {
                deck.fx.process(deck_out, beat);
            }
            deck.cut.gate(deck_out);
            deck.peak = deck_out.iter().fold(deck.peak, |p, s| p.max(s.abs()));
            let mix = DeckMix { audio: deck_out, volume: deck.volume, side: deck.side, cue: deck.cue };
            self.mixer.add_deck(d, mix, out, out_ch);
        }
        self.mixer.read_live(input, in_ch, offset, frames);
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
        for frame in self.mixer.master(frames).chunks_exact(2) {
            self.master_peaks[0] = self.master_peaks[0].max(frame[0].abs());
            self.master_peaks[1] = self.master_peaks[1].max(frame[1].abs());
        }
        // after the recording and the meter: the audience never hears it
        let (preview, ended) = self.preview.render(frames, rate);
        self.mixer.add_preview(preview, self.decks.len(), out, out_ch);
        if let Some(track) = ended {
            self.discard(Garbage::Track(track));
        }
    }

    /// The chosen leader if it can lead, else the current one if it still can, or, resumed
    /// after a restart, while it is waited for; else the first deck that can.
    fn resolve_leader(&self) -> Option<DeckId> {
        let can_lead = |d: &DeckId| self.decks.get(*d).is_some_and(Deck::can_lead);
        let awaited = |d: &DeckId| self.resumed_leader_s > 0.0 && self.decks[*d].playhead.is_loaded();
        self.chosen_leader
            .filter(can_lead)
            .or(self.leader.filter(|d| can_lead(d) || awaited(d)))
            .or_else(|| (0..self.decks.len()).find(can_lead))
    }

    fn apply_commands(&mut self) {
        while let Ok(command) = self.commands.pop() {
            self.apply(command);
            self.applied += 1;
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
                    let grid = d.reset_track_state();
                    let stems = d.playhead.take_stems();
                    let old = d.playhead.load(track);
                    self.discard_grid(grid);
                    for garbage in stems.into_iter().chain([old]).flatten() {
                        self.discard(Garbage::Track(garbage));
                    }
                } else {
                    self.discard(Garbage::Track(track));
                }
            }
            Eject { deck } if valid(deck) => {
                let d = &mut self.decks[deck];
                d.set_playing(false);
                let grid = d.reset_track_state();
                let stems = d.playhead.take_stems();
                let old = d.playhead.eject();
                self.discard_grid(grid);
                for garbage in stems.into_iter().chain([old]).flatten() {
                    self.discard(Garbage::Track(garbage));
                }
            }
            SetStems { deck, stems } => {
                let old = if valid(deck) { self.decks[deck].playhead.set_stems(stems) } else { stems };
                for garbage in old.into_iter().flatten() {
                    self.discard(Garbage::Track(garbage));
                }
            }
            SetStemMix { deck, mix, on_bar } if valid(deck) => self.decks[deck].set_stem_mix(mix, on_bar),
            SetPlaying { deck, playing } if valid(deck) => self.decks[deck].set_playing(playing),
            Brake { deck } if valid(deck) => self.decks[deck].brake(),
            SpinBack { deck, held } if valid(deck) => self.decks[deck].spin_back(held),
            SetRepeat { deck, enabled } if valid(deck) => self.decks[deck].playhead.set_repeat(enabled),
            SetPitch { deck, ratio } if valid(deck) && ratio.is_finite() => self.decks[deck].pitch = ratio,
            SetPitchBend { deck, amount } if valid(deck) && amount.is_finite() => {
                self.decks[deck].bend = amount
            }
            Seek { deck, seconds } if valid(deck) && seconds.is_finite() => {
                self.decks[deck].playhead.seek_s(seconds)
            }
            SearchBy { deck, seconds } if valid(deck) && seconds.is_finite() => {
                self.decks[deck].search_by(seconds)
            }
            Search { deck, speed } if valid(deck) && speed.is_finite() => self.decks[deck].search(speed),
            TakePlace { deck, from } if valid(deck) && valid(from) => {
                let place = self.decks[from].place();
                self.decks[deck].take_place(place);
            }
            Hand { deck, position_s, at_s }
                if valid(deck) && position_s.is_none_or(f64::is_finite) && at_s.is_finite() =>
            {
                self.decks[deck].hand(position_s, at_s)
            }
            JogTouch { deck, touched, at_s } if valid(deck) && at_s.is_finite() => {
                self.decks[deck].jog_touch(touched, at_s)
            }
            Jog { deck, turns, at_s } if valid(deck) && turns.is_finite() && at_s.is_finite() => {
                self.decks[deck].jog_turn(turns, at_s)
            }
            Jump { deck, seconds } if valid(deck) && seconds.is_finite() => {
                self.decks[deck].jump(seconds, self.quantize)
            }
            BeatJump { deck, beats } if valid(deck) && beats.is_finite() => self.decks[deck].beat_jump(beats),
            SetQuantize { enabled } => self.quantize = enabled,
            SetLoop { deck, region, active } if valid(deck) => self.decks[deck].set_loop(region, active),
            BeatLoop { deck, beats } if valid(deck) => self.decks[deck].beat_loop(beats, self.quantize),
            LoopIn { deck } if valid(deck) => self.decks[deck].loop_in(self.quantize),
            LoopOut { deck } if valid(deck) => self.decks[deck].loop_out(self.quantize),
            LoopRoll { deck, beats } if valid(deck) => self.decks[deck].loop_roll(beats, self.quantize),
            Slice { deck, region }
                if valid(deck) && region.is_none_or(|r| r.start_s.is_finite() && r.end_s.is_finite()) =>
            {
                self.decks[deck].slice(region, self.quantize)
            }
            ExitLoop { deck } if valid(deck) => self.decks[deck].exit_loop(),
            FaderCut { deck, beats } if valid(deck) => self.decks[deck].cut.set(beats),
            Reloop { deck } if valid(deck) => self.decks[deck].reloop(),
            ScaleLoop { deck, factor } if valid(deck) => self.decks[deck].scale_loop(factor),
            SetSlip { deck, enabled } if valid(deck) => self.decks[deck].set_slip(enabled),
            Censor { deck, active } if valid(deck) => self.decks[deck].censor(active),
            SetReverse { deck, enabled } if valid(deck) => self.decks[deck].set_reverse(enabled),
            HoldReverse { deck, held } if valid(deck) => self.decks[deck].hold_reverse(held),
            SetVinylEnabled { deck, enabled } if valid(deck) => self.decks[deck].set_vinyl_enabled(enabled),
            SetVinylMode { deck, mode } if valid(deck) => self.decks[deck].set_vinyl_mode(mode),
            // a grid without a finite tempo would put the beats at infinity
            SetBeatGrid { deck, grid } => {
                let (kept, refused) = match grid {
                    Some(grid) if valid(deck) && grid.is_valid() => (Some(grid), None),
                    grid => (None, grid),
                };
                if valid(deck) {
                    let old = std::mem::replace(&mut self.decks[deck].grid, kept);
                    self.discard_grid(old);
                }
                self.discard_grid(refused);
            }
            SetSyncLocked { deck, locked: true } if valid(deck) => self.decks[deck].engage_sync(true),
            SetSyncLocked { deck, locked: false } if valid(deck) => self.decks[deck].sync.reset(),
            SyncOnce { deck } if valid(deck) => self.decks[deck].engage_sync(false),
            SetSyncLeader { deck } => self.chosen_leader = deck.filter(|&d| valid(d)),
            ResumeSyncLeader { deck } if valid(deck) => {
                self.leader = Some(deck);
                self.resumed_leader_s = LEADER_RESUME_S;
            }
            SetLeadIn { deck, seconds } if valid(deck) => self.decks[deck].vinyl.lead_in_s = seconds,
            SetRpm { deck, rpm } if valid(deck) => self.decks[deck].set_rpm(rpm),
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
            SetMute { deck, muted } if valid(deck) => self.mixer.set_mute(deck, muted),
            SetCueMix { mix } => self.mixer.cue_mix = mix,
            SetHeadphoneGain { gain } => self.mixer.headphone_gain = gain,
            SetSplitCue { enabled } => self.mixer.split_cue = enabled,
            SetLevel { deck, gain } if valid(deck) => self.decks[deck].level.set(gain.max(0.0)),
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
            TriggerSample { deck, slot, gain } if valid(deck) && gain.is_finite() => {
                let rate = self.config.sample_rate as f64;
                let d = &mut self.decks[deck];
                let wait = if self.quantize { d.frames_to_next_beat(rate).unwrap_or(0) } else { 0 };
                d.sampler.trigger(slot, wait, gain.max(0.0));
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
            SetFx { deck, slot, kind } => self.fx_slot(deck, slot, |s| s.set_kind(kind)),
            SetFxEnabled { deck, slot, enabled } => self.fx_slot(deck, slot, |s| s.set_enabled(enabled)),
            SetFxMix { deck, slot, mix } => self.fx_slot(deck, slot, |s| s.set_mix(mix)),
            SetFxAmount { deck, slot, amount } => self.fx_slot(deck, slot, |s| s.set_amount(amount)),
            SetFxParam2 { deck, slot, param2 } => self.fx_slot(deck, slot, |s| s.set_param2(param2)),
            SetFxBeats { deck, slot, beats } => self.fx_slot(deck, slot, |s| s.set_beats(beats)),
            SetFxChainEnabled { deck, enabled } if valid(deck) => self.decks[deck].fx.set_enabled(enabled),
            SetFxParts { deck, parts } if valid(deck) => self.decks[deck].set_fx_parts(parts),
            SetCrossfaderCurve { curve } => self.mixer.curve = curve,
            SetCrossfaderReverse { enabled } => self.mixer.crossfader_reverse = enabled,
            SetLimiter { enabled } => self.mixer.set_limiter(enabled),
            SetBalance { balance } if balance.is_finite() => self.mixer.set_balance(balance),
            SetBoothGain { gain } if gain.is_finite() => self.mixer.set_booth_gain(gain),
            SetLiveInput { source, channels } => self.mixer.live(source).set_channels(channels),
            SetLiveLevel { source, gain } if gain.is_finite() => self.mixer.live(source).set_level(gain),
            SetLiveOn { source, on } => self.mixer.live(source).set_on(on),
            SetLiveCue { source, cue } => self.mixer.live(source).set_cue(cue),
            SetTalkover { enabled, depth_db } if depth_db.is_finite() => {
                self.mixer.talkover.enabled = enabled;
                self.mixer.talkover.depth = db_to_gain(depth_db.min(0.0));
            }
            SetPassthrough { deck, enabled } if valid(deck) => self.decks[deck].set_passthrough(enabled),
            Preview { id, track, start_s } => {
                let start_s = if start_s.is_finite() { start_s } else { 0.0 };
                if let Some(old) = self.preview.start(id, track, start_s) {
                    self.discard(Garbage::Track(old));
                }
            }
            SeekPreview { seconds } if seconds.is_finite() => self.preview.seek(seconds),
            SkipPreview { seconds } if seconds.is_finite() => self.preview.skip(seconds),
            StopPreview => {
                if let Some(old) = self.preview.stop() {
                    self.discard(Garbage::Track(old));
                }
            }
            _ => {} // command for a deck that does not exist
        }
    }

    /// Changes a slot of a deck's effect chain, if both exist.
    fn fx_slot(&mut self, deck: usize, slot: usize, change: impl FnOnce(&mut FxSlot)) {
        if let Some(slot) = self.decks.get_mut(deck).and_then(|d| d.fx.slot_mut(slot)) {
            change(slot);
        }
    }

    /// Hands an object back to the control side. If the queue is full (the controller
    /// stopped collecting), it is leaked rather than freed on the audio thread.
    /// A grid let go: one with anchors freed off the audio thread.
    fn discard_grid(&mut self, grid: Option<BeatGrid>) {
        if let Some(grid) = grid.filter(|g| !g.is_steady()) {
            self.discard(Garbage::Grid(grid));
        }
    }

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
        let held = &mut self.held;
        let master = std::mem::take(&mut self.master_peaks);
        let mut status = EngineStatus {
            deck_count: self.decks.len(),
            main_peak: held.main.hold(self.peak),
            master_peaks: [held.master[0].hold(master[0]), held.master[1].hold(master[1])],
            mic_peak: held.mic.hold(std::mem::take(&mut self.mixer.mic.peak)),
            aux_peak: held.aux.hold(std::mem::take(&mut self.mixer.aux.peak)),
            recording,
            preview: self.preview.status(),
            commands: self.applied,
            ..Default::default()
        };
        for (d, (slot, deck)) in status.decks.iter_mut().zip(&mut self.decks).enumerate() {
            *slot = deck.status(self.leader == Some(d));
            slot.peak = self.held.decks[d].hold(slot.peak);
            deck.peak = 0.0;
        }
        self.peak = 0.0;
        self.status.write(status);
    }
}

/// How many status periods a peak stays published: a reader sampling the status less often
/// than it is published (the interface about every 17 ms, the controllers every 20 ms, the
/// engine every 10 ms) misses none.
const PEAK_HELD: usize = 3;

/// A meter's peaks of the last [`PEAK_HELD`] status periods.
#[derive(Debug, Default, Clone, Copy)]
struct HeldPeak([f32; PEAK_HELD]);

impl HeldPeak {
    /// The highest of the periods held, `peak` this one's.
    fn hold(&mut self, peak: f32) -> f32 {
        self.0.rotate_right(1);
        self.0[0] = peak;
        self.0.iter().copied().fold(0.0, f32::max)
    }
}

#[derive(Debug, Default)]
struct HeldPeaks {
    decks: [HeldPeak; MAX_DECKS],
    main: HeldPeak,
    master: [HeldPeak; 2],
    mic: HeldPeak,
    aux: HeldPeak,
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

#[cfg(feature = "testing")]
impl Engine {
    /// The commands sent and not applied yet, taken unapplied: what a test sent an engine it
    /// does not run.
    pub fn take_commands(&mut self) -> Vec<Command> {
        std::iter::from_fn(|| self.commands.pop().ok()).collect()
    }
}

/// The control half: send commands, read the state, free what the engine returns.
pub struct EngineController {
    commands: rtrb::Producer<Command>,
    /// Commands sent so far.
    sent: u64,
    status: triple_buffer::Output<EngineStatus>,
    garbage: rtrb::Consumer<Garbage>,
    scopes: Vec<rtrb::Consumer<[f32; 2]>>,
}

impl EngineController {
    /// Sends a command. Fails (returning it) only if the engine is not running and the
    /// queue is full.
    pub fn send(&mut self, command: Command) -> Result<(), Command> {
        self.commands.push(command).map_err(|rtrb::PushError::Full(c)| c)?;
        self.sent += 1;
        Ok(())
    }

    /// Commands sent so far: the status shows the engine with every one of them applied once
    /// its `commands` reach this.
    pub fn sent(&self) -> u64 {
        self.sent
    }

    /// Latest engine state.
    pub fn status(&mut self) -> EngineStatus {
        *self.status.read()
    }

    /// Appends the turntable signal points of `deck` received since the last call (one
    /// frame in four, left and right).
    pub fn read_scope(&mut self, deck: usize, points: &mut Vec<[f32; 2]>) {
        if let Some(scope) = self.scopes.get_mut(deck) {
            points.extend(std::iter::from_fn(|| scope.pop().ok()));
        }
    }

    /// Frees what the engine handed back. Call it regularly (e.g. with every UI update).
    pub fn collect_garbage(&mut self) -> usize {
        self.take_garbage().len()
    }

    /// What the engine handed back, for the caller to free: a track of a few minutes and its
    /// stems take a while to free, better done out of any lock other threads wait on.
    pub fn take_garbage(&mut self) -> Vec<Garbage> {
        std::iter::from_fn(|| self.garbage.pop().ok()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_peak_stays_published_for_the_periods_held_then_goes() {
        let mut held = HeldPeak::default();
        let published: Vec<f32> = [0.8, 0.0, 0.2, 0.0, 0.0, 0.0].map(|p| held.hold(p)).into();
        assert_eq!(published, [0.8, 0.8, 0.8, 0.2, 0.2, 0.0]);
    }
}
