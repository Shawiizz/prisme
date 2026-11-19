//! A deck: a track, a playhead and what drives it (internal play, the record, or a hand
//! on the screen).

mod bar_mix;
mod cut;
mod hand;
mod jog;
mod motor;
mod transport;

use crate::command::{CrossfaderSide, Rpm, VinylMode};
use crate::grid::BeatGrid;
use crate::keylock::KeyLock;
use crate::playhead::Playhead;
use crate::sampler::Sampler;
use crate::status::{DeckStatus, LoopStatus, SyncStatus};
use crate::stems::{Parts, StemMix};
use crate::strip::ChannelStrip;
use crate::sync::{self, DeckSync, LeaderBeat, SyncTarget};
use crate::tempo::PlatterTempo;
use crate::vinyl::{self, VinylControl, VinylDrive};
use bar_mix::BarMix;
use cut::Cut;
use hand::{Hand, Step};
use jog::{Jog, Turn};
use motor::Motor;
use prisme_dsp::{Beat, FxChain, Smoothed};
use prisme_timecode::TimecodeDecoder;
use transport::Transport;

/// In absolute mode, a gap larger than this (s) is a needle drop: jump.
const JUMP_THRESHOLD_S: f64 = 0.1;
/// Smaller gaps are closed within about 20 ms: speed correction per second of gap, and
/// its limit, wider when the record moves fast (a scratch leaves larger gaps to close).
const PULL_PER_S: f64 = 50.0;
const MAX_CORRECTION: f64 = 0.2;
const MAX_CORRECTION_PER_SPEED: f64 = 0.5;
/// Below this speed the track stands still (stem changes on the bar wait for it to move).
const STILL_SPEED: f64 = 0.02;
/// Below this speed the track is as loud as it moves, as a cartridge plays a groove: a record
/// held by a finger over the turning platter barely sounds (a stopped playhead holds its last
/// sample, which a level jumping at each jolt of the finger turns into thumps), a record
/// dragged slowly to find an attack sounds softer, a scratch at full level.
const FULL_LEVEL_SPEED: f64 = 0.25;
/// How fast the level of a track glides to its auto gain (s): no click when it comes.
const LEVEL_GLIDE_S: f32 = 0.05;
/// Stem changes on the bar wait for the next group of this many beats.
const BEATS_PER_BAR: f64 = 4.0;
/// A playhead this far (s) from where play took it went round a loop or jumped.
const LEAP_S: f64 = 1e-3;
/// How fast the track fades with the motion (s).
const MOTION_FADE_S: f32 = 0.005;
/// The crossfade between the track and the input passed through (s).
const THROUGH_FADE_S: f32 = 0.01;

pub(crate) struct Deck {
    pub(crate) playhead: Playhead,
    pub(crate) playing: bool,
    pub(crate) pitch: f64,
    /// Temporary speed change while a bend key is held (0.04 = +4 %).
    pub(crate) bend: f64,
    pub(crate) vinyl: VinylControl,
    pub(crate) volume: f32,
    pub(crate) side: CrossfaderSide,
    /// Internal mixer: heard in the headphones.
    pub(crate) cue: bool,
    /// Loudest sample since the last status (after the channel strip in internal mode).
    pub(crate) peak: f32,
    pub(crate) grid: Option<BeatGrid>,
    pub(crate) strip: ChannelStrip,
    /// The pitch stays when the speed changes (within [`KeyLock::applies`]).
    pub(crate) key_lock_enabled: bool,
    key_lock: KeyLock,
    pub(crate) fx: FxChain,
    /// The parts of the track the effects are on (none: the whole deck), and room for them.
    fx_parts: Parts,
    apart: Vec<f32>,
    pub(crate) sampler: Sampler,
    pub(crate) sync: DeckSync,
    transport: Transport,
    correction: f64,
    speed: f64,
    /// The track dragged on the screen: it drives the deck while there.
    hand: Option<Hand>,
    /// The track held by the jog wheel: it drives the deck while there, unless the screen does.
    jog_hand: Option<Hand>,
    jog: Jog,
    /// Internal play: the record's speed as a turntable's motor brings it up or down.
    motor: Motor,
    /// Gain of the track, following its motion.
    motion: Smoothed,
    /// Gain bringing the track to the level of the others (1 without auto gain).
    pub(crate) level: Smoothed,
    /// The stem mix, and its changes waiting for a bar.
    bar_mix: BarMix,
    /// Share of the input passed through in place of the track: 1 in passthrough.
    through: Smoothed,
    /// The deck chopped in time with its beats while a pad is held.
    pub(crate) cut: Cut,
}

impl Deck {
    pub(crate) fn new(decoder: Box<TimecodeDecoder>, sample_rate: u32, max_block_frames: usize) -> Self {
        Self {
            playhead: Playhead::new(),
            playing: false,
            pitch: 1.0,
            bend: 0.0,
            vinyl: VinylControl::new(decoder, sample_rate),
            volume: 1.0,
            side: CrossfaderSide::Center,
            cue: false,
            peak: 0.0,
            grid: None,
            strip: ChannelStrip::new(sample_rate as f32),
            key_lock_enabled: false,
            key_lock: KeyLock::new(sample_rate, max_block_frames),
            fx: FxChain::new(sample_rate as f32, max_block_frames),
            fx_parts: Parts::NONE,
            apart: vec![0.0; max_block_frames * 2],
            sampler: Sampler::new(),
            sync: DeckSync::new(),
            transport: Transport::default(),
            correction: 0.0,
            speed: 0.0,
            hand: None,
            jog_hand: None,
            jog: Jog::default(),
            motor: Motor::default(),
            motion: Smoothed::new(0.0, MOTION_FADE_S, sample_rate as f32),
            level: Smoothed::new(1.0, LEVEL_GLIDE_S, sample_rate as f32),
            bar_mix: BarMix::default(),
            through: Smoothed::new(0.0, THROUGH_FADE_S, sample_rate as f32),
            cut: Cut::new(sample_rate as f32),
        }
    }

    /// Plays the deck's input as it comes (a record, a CD player) instead of its track, which
    /// waits where it is; or the track again.
    pub(crate) fn set_passthrough(&mut self, enabled: bool) {
        self.through.set(if enabled { 1.0 } else { 0.0 });
    }

    fn passes_through(&self) -> bool {
        self.through.target() == 1.0
    }

    /// Renders one block from the input (see [`Deck::process`]) with the effect on the parts
    /// of the track it is on; in passthrough, the input itself. Returns whether the effect is
    /// done, else it goes on the whole deck after its channel strip.
    pub(crate) fn render(
        &mut self,
        input: &[f32],
        out: &mut [f32],
        out_rate: f64,
        target: Option<SyncTarget>,
        beat: Beat,
    ) -> bool {
        let settled = |s: &Smoothed, value: f32| s.is_settled() && s.target() == value;
        if settled(&self.through, 1.0) {
            out.copy_from_slice(input);
            // the track stands meanwhile, as its averaged speed tells once a record lets go
            self.speed = 0.0;
            self.sync.observe(0.0, None, (out.len() / 2) as f64 / out_rate);
            return false;
        }
        self.process(input, out, out_rate, target);
        let fx_done = self.fx_on_parts(out, beat);
        if !settled(&self.through, 0.0) {
            for (o, i) in out.chunks_exact_mut(2).zip(input.chunks_exact(2)) {
                let through = self.through.tick();
                o[0] += (i[0] - o[0]) * through;
                o[1] += (i[1] - o[1]) * through;
            }
        }
        fx_done
    }

    /// Plays the parts of the track as `mix` says: the parts it changes from the next bar if
    /// `on_bar` and the deck moves forward with a beat grid, else at once.
    pub(crate) fn set_stem_mix(&mut self, mix: StemMix, on_bar: bool) {
        let bar_s = (on_bar && self.speed > STILL_SPEED)
            .then(|| self.next_bar_s())
            .flatten()
            .filter(|&bar_s| bar_s < self.playhead.duration_s());
        let playing = self.bar_mix.ask(mix, bar_s);
        self.playhead.set_stem_mix(playing);
    }

    /// Stem changes waiting for a bar come when play forward leaps instead of reaching it:
    /// a loop going round (a loop shorter than a bar never reaches the next) or a jump.
    fn leap_stems_to_their_bar(&mut self, from_s: f64, played_s: f64) {
        let leapt = (self.playhead.position_s() - from_s - played_s).abs() > LEAP_S;
        if self.bar_mix.is_waiting() && played_s > 0.0 && leapt {
            let mix = self.bar_mix.release();
            self.playhead.set_stem_mix(mix);
        }
    }

    /// The start of the next bar after the playhead, by the beat grid.
    fn next_bar_s(&self) -> Option<f64> {
        let grid = self.grid?;
        let bar_s = BEATS_PER_BAR * grid.beat_period_s();
        let bars = ((self.playhead.position_s() - grid.first_beat_s) / bar_s).floor() + 1.0;
        Some(grid.first_beat_s + bars * bar_s)
    }

    /// Renders one block from the turntable signal `input`; `target` is given when the
    /// deck follows a leader.
    fn process(&mut self, input: &[f32], out: &mut [f32], out_rate: f64, target: Option<SyncTarget>) {
        // decoded even with vinyl control off, for the signal indicator
        let drive = self.vinyl.process(input);
        // a record in absolute mode decides where the track is: no loop or repeat takes it round
        self.playhead.set_wrapping(self.drive_mode() != DriveMode::Absolute);
        let target = target.map(|t| self.engage(t));
        let seconds = (out.len() / 2) as f64 / out_rate;
        let by_hand = self.drive_mode() != DriveMode::Absolute;
        let playhead_s = self.playhead.position_s();
        // both hands keep their clocks going, the screen's leading
        let [screen, jog] = [&mut self.hand, &mut self.jog_hand]
            .map(|hand| hand.as_mut().filter(|_| by_hand).map(|h| h.step(playhead_s, seconds)));
        // a hand moves the track as it goes; the censor and the reverse turn the rest round
        let turned = screen.is_none() && jog.is_none() && self.backwards();
        let (speed, by_motor) = match screen.or(jog) {
            Some(Step::Play(speed)) => (speed, false),
            Some(Step::Jump(to)) => {
                self.playhead.seek_s(to);
                (0.0, false)
            }
            None => {
                let speed = self.speed_for(drive, target);
                (if turned { -speed } else { speed }, self.drive_mode() == DriveMode::Internal)
            }
        };
        self.jog.fade(seconds);
        let ramp = by_motor && !self.motor.is_steady();
        let moved = if ramp { speed * self.motor.factor() } else { speed };
        if let Some(mix) = self.bar_mix.follow(playhead_s, seconds * moved.max(0.0), self.next_bar_s()) {
            self.playhead.set_stem_mix(mix);
        }
        let apart = (!self.fx_parts.is_empty()).then(|| &mut self.apart[..out.len()]);
        let key_locked = self.key_lock_enabled || self.key_lock.key_shift() != 0;
        if ramp {
            // the key lock lets go while the motor moves the record, as it does under a hand
            self.key_lock.disengage();
            let (motor, frame_s) = (&mut self.motor, 1.0 / out_rate);
            self.playhead.render_with(out, apart, out_rate, || speed * motor.tick(frame_s));
        } else if key_locked && KeyLock::applies(if turned { -speed } else { speed }) {
            self.playhead.render_key_locked(&mut self.key_lock, out, apart, out_rate, speed);
        } else {
            self.key_lock.disengage();
            self.playhead.render(out, apart, out_rate, speed);
        }
        let speed = if ramp { speed * self.motor.factor() } else { speed };
        // the beats over the block, for the cuts: from where it started, at the mean speed
        let beats_per_frame = (moved + speed) / 2.0 / out_rate / 60.0;
        self.cut.follow(self.grid.map(|g| (g.beats_at(playhead_s), beats_per_frame * g.bpm)));
        if self.motor.has_stopped() {
            self.playing = false;
            self.motor = Motor::default();
        }
        self.speed = speed;
        self.leap_stems_to_their_bar(playhead_s, seconds * speed);
        self.fade_with_motion(out);
        self.stop_at_the_end();

        let platter = self.vinyl.enabled.then_some(drive.speed);
        self.sync.observe(speed, platter, seconds);
        self.advance_shadow(seconds);
        self.follow_record_position(drive);
    }

    /// A hand on the track: at `position_s` at `at_s` on the pointer's clock, or taken off
    /// it (`None`) and play goes on as before. A record in absolute mode keeps the track
    /// where it is.
    pub(crate) fn hand(&mut self, position_s: Option<f64>, at_s: f64) {
        Hand::put(&mut self.hand, position_s, at_s);
    }

    /// The jog wheel touched (at `at_s` on the controller's clock) or let go. A record in
    /// vinyl control drives the deck: the jog does nothing then.
    pub(crate) fn jog_touch(&mut self, touched: bool, at_s: f64) {
        if self.vinyl.enabled || touched == self.jog.touched() {
            return;
        }
        let position = touched.then(|| self.playhead.position_s());
        self.jog.touch(position);
        Hand::put(&mut self.jog_hand, position, at_s);
    }

    /// The jog wheel turned by `turns` (positive forwards) at `at_s`.
    pub(crate) fn jog_turn(&mut self, turns: f64, at_s: f64) {
        if self.vinyl.enabled || !self.playhead.is_loaded() {
            return;
        }
        match self.jog.turn(turns, self.playing) {
            Turn::Hand(position_s) => Hand::put(&mut self.jog_hand, Some(position_s), at_s),
            Turn::Move(seconds) => {
                let position = (self.playhead.position_s() + seconds).clamp(0.0, self.playhead.duration_s());
                self.playhead.seek_s(position);
            }
            Turn::Nothing => {}
        }
    }

    /// Internal play: starts or stops at once.
    pub(crate) fn set_playing(&mut self, playing: bool) {
        self.playing = playing;
        self.motor = Motor::default();
    }

    /// Vinyl control on or off. A record letting go of the deck leaves it playing by itself as
    /// the record played it: at its tempo and in its direction, or else stopped.
    pub(crate) fn set_vinyl_enabled(&mut self, enabled: bool) {
        // a jog held meanwhile is let go: its release would no longer reach the deck
        self.jog_touch(false, 0.0);
        let let_go = self.vinyl.enabled && !enabled;
        let (rate, speed) = (self.tempo_rate(), self.sync.smoothed_speed);
        self.vinyl.enabled = enabled;
        self.vinyl.take_back();
        self.sync.reset();
        if let_go {
            let moving = self.playhead.is_loaded() && speed.abs() >= vinyl::PLAYING_SPEED;
            self.pitch = rate;
            self.set_playing(moving);
            if moving && (speed < 0.0) != self.backwards() {
                self.set_reverse(!self.transport.reversed);
            }
        }
    }

    /// Internal play: a playing deck slows to a stop as a turntable switched off; a stopped
    /// one, or one braking, gets up to speed as one switched on.
    pub(crate) fn brake(&mut self) {
        if self.drive_mode() != DriveMode::Internal || !self.playhead.is_loaded() {
            return;
        }
        if self.playing && !self.motor.is_braking() {
            self.motor.brake();
        } else {
            self.motor.start(!self.playing);
            self.playing = true;
        }
    }

    /// Internal play: the record spun back fast while `held`; let go, it plays on from where
    /// it is.
    pub(crate) fn spin_back(&mut self, held: bool) {
        if self.drive_mode() != DriveMode::Internal || !self.playhead.is_loaded() {
            return;
        }
        self.motor.spin_back(held, !self.playing);
        self.playing |= held;
    }

    /// Internal play stops at the end of the track, or at its start in reverse, and so does a
    /// deck playing on past the end of its record (a record, on the other hand, turns on); a
    /// loop going on past it takes the track back instead.
    fn stop_at_the_end(&mut self) {
        if self.playhead.goes_round() {
            return;
        }
        let internal = self.drive_mode() == DriveMode::Internal;
        if internal && self.playing && self.transport.reversing() && self.playhead.position_s() <= 0.0 {
            self.set_playing(false);
            self.playhead.seek_s(0.0);
            return;
        }
        let end_s = self.playhead.duration_s();
        if self.playhead.position_s() < end_s {
            return;
        }
        if internal && self.playing {
            self.set_playing(false);
            self.playhead.seek_s(end_s);
        } else if self.vinyl.plays_on() {
            self.vinyl.stand();
            self.playhead.seek_s(end_s);
        }
    }

    /// Applies the track's gain, to the parts apart too: its motion's (a fade as it stops
    /// and moves again) and its level's.
    fn fade_with_motion(&mut self, out: &mut [f32]) {
        self.motion.set((self.speed.abs() / FULL_LEVEL_SPEED).min(1.0) as f32);
        let at_unity = |s: &Smoothed| s.is_settled() && s.target() == 1.0;
        if at_unity(&self.motion) && at_unity(&self.level) {
            return;
        }
        let apart = &mut self.apart[..if self.fx_parts.is_empty() { 0 } else { out.len() }];
        let mut apart = apart.chunks_exact_mut(2);
        for frame in out.chunks_exact_mut(2) {
            let gain = self.motion.tick() * self.level.tick();
            frame[0] *= gain;
            frame[1] *= gain;
            if let Some(part) = apart.next() {
                part[0] *= gain;
                part[1] *= gain;
            }
        }
    }

    /// Puts the effects on `parts` of the track, or on the whole deck (none).
    pub(crate) fn set_fx_parts(&mut self, parts: Parts) {
        self.fx_parts = parts;
        self.playhead.set_apart(parts);
    }

    /// The effects on the parts they are on, mixed back into `out` (before the sampler and the
    /// channel strip); false when they are on the whole deck, to be applied after them.
    fn fx_on_parts(&mut self, out: &mut [f32], beat: Beat) -> bool {
        if self.fx_parts.is_empty() {
            return false;
        }
        let apart = &mut self.apart[..out.len()];
        self.fx.process(apart, beat);
        for (sample, part) in out.iter_mut().zip(apart.iter()) {
            *sample += part;
        }
        true
    }

    /// Transposes the track; this goes through the key lock's stretcher, on or off.
    pub(crate) fn set_key_shift(&mut self, semitones: i8) {
        self.key_lock.set_key_shift(semitones);
    }

    /// Whether the deck follows a leader: locked, or about to match it once.
    pub(crate) fn follows(&self) -> bool {
        self.sync.locked || self.sync.once_pending || self.sync.snap_pending
    }

    /// A deck stopped no longer leads at once (paused, its beats stand still: followers
    /// would chase them); a record, as soon as its averaged speed says it slowed; nor does a
    /// deck passing its input through.
    pub(crate) fn can_lead(&self) -> bool {
        !self.passes_through()
            && self.grid.is_some()
            && self.playhead.is_loaded()
            && !self.sync.locked
            && !self.backwards()
            && self.is_moving()
            && self.sync.smoothed_speed.abs() > sync::LEADER_MIN_SPEED
    }

    pub(crate) fn leader_beat(&self) -> Option<LeaderBeat> {
        let grid = self.grid?;
        Some(LeaderBeat {
            bpm: grid.bpm * self.tempo_rate(),
            beats: grid.beats_at(self.playhead.position_s()),
        })
    }

    /// Whether the deck plays a loop.
    pub(crate) fn is_looping(&self) -> bool {
        self.transport.loop_active
    }

    /// The record drives the deck in `mode`: from a change on, it is followed afresh.
    pub(crate) fn set_vinyl_mode(&mut self, mode: VinylMode) {
        if self.vinyl.mode != mode {
            self.vinyl.mode = mode;
            self.vinyl.take_back();
            self.sync.reset();
        }
    }

    /// The record is meant to turn at `rpm`: its tempo is learnt again.
    pub(crate) fn set_rpm(&mut self, rpm: Rpm) {
        if self.vinyl.rpm != rpm {
            self.vinyl.rpm = rpm;
            self.vinyl.take_back();
            self.sync.reset();
            self.sync.tempo = PlatterTempo::new(rpm.turn_s());
        }
    }

    pub(crate) fn sync_target(&self, leader: LeaderBeat) -> Option<SyncTarget> {
        let grid = self.grid?;
        Some(sync::sync_target(&grid, self.playhead.position_s(), leader))
    }

    /// Locks the deck to the leader, or matches it once: a relative record takes its rate,
    /// internal play only its beats (the pitch is the caller's to set, as the fader shows it).
    pub(crate) fn engage_sync(&mut self, locked: bool) {
        match (self.drive_mode(), locked) {
            (DriveMode::Absolute, _) => return,
            (DriveMode::Relative, false) => self.sync.once_pending = true,
            (DriveMode::Internal, false) => {}
            (_, true) => self.sync.locked = true,
        }
        self.sync.snap_pending = true;
    }

    /// Applies what engaging sync asks for (phase snap, rate offset) and returns the
    /// target as it stands afterwards.
    fn engage(&mut self, mut target: SyncTarget) -> SyncTarget {
        let resnap = self.drive_mode() == DriveMode::Internal
            && self.sync.locked
            && self.playing
            && self.chases_beats()
            && sync::needs_resnap(target.phase_error);
        if let (Some(grid), true) = (self.grid, self.sync.snap_pending || resnap) {
            let position = self.playhead.position_s() + sync::snap_offset_s(&grid, target.phase_error);
            self.playhead.seek_round_s(position);
            target.phase_error = 0.0;
            if self.sync.locked {
                // start from the platter as it turns now rather than from a stale average
                if let Some(platter) = self.sync.platter_now() {
                    self.sync.platter_reference = platter;
                }
            }
            self.sync.snap_pending = false;
        }
        if self.sync.once_pending {
            // a stopped platter is assumed to restart at its nominal speed
            self.sync.rate_offset = target.rate / self.sync.platter_now().unwrap_or(1.0);
            self.sync.once_pending = false;
        }
        target
    }

    fn speed_for(&mut self, drive: VinylDrive, target: Option<SyncTarget>) -> f64 {
        match self.drive_mode() {
            DriveMode::Internal => self.internal_speed(target),
            DriveMode::Relative => self.relative_speed(drive.speed, target),
            DriveMode::Absolute => drive.speed + self.correction,
        }
    }

    fn internal_speed(&mut self, target: Option<SyncTarget>) -> f64 {
        if !self.playing {
            return 0.0;
        }
        let rate = match target {
            _ if !self.sync.locked => self.pitch,
            Some(t) => {
                self.sync.held_rate = Some(t.rate);
                let correction =
                    if self.chases_beats() { sync::phase_correction(t.phase_error) } else { 0.0 };
                t.rate * (1.0 + correction)
            }
            None => self.sync.held_rate.unwrap_or(self.pitch),
        };
        rate * (1.0 + self.bend + self.jog.nudge)
    }

    /// Locked: the platter's gestures around its steady speed, at the leader's tempo.
    fn relative_speed(&mut self, platter: f64, target: Option<SyncTarget>) -> f64 {
        if !self.sync.locked {
            return platter * self.sync.rate_offset;
        }
        let rate = match target {
            Some(t) => *self.sync.held_rate.insert(t.rate),
            None => self.sync.held_rate.unwrap_or(self.sync.platter_reference),
        };
        platter / self.sync.platter_reference * rate
    }

    /// Absolute mode: jumps on needle drops, closes small gaps smoothly.
    fn follow_record_position(&mut self, drive: VinylDrive) {
        self.correction = 0.0;
        if self.drive_mode() != DriveMode::Absolute || !self.playhead.is_loaded() {
            return;
        }
        if let Some(target) = drive.target_position_s {
            let gap = target - self.playhead.position_s();
            if gap.abs() > JUMP_THRESHOLD_S {
                self.playhead.seek_s(target);
            } else {
                // held still, a record's position reads only the jolts of the finger
                let trusted = (drive.speed.abs() / FULL_LEVEL_SPEED).min(1.0);
                let cap = (MAX_CORRECTION + MAX_CORRECTION_PER_SPEED * drive.speed.abs()) * trusted;
                self.correction = (gap * PULL_PER_S).clamp(-cap, cap);
            }
        }
    }

    /// Output frames until the next beat, when the deck plays on its grid.
    pub(crate) fn frames_to_next_beat(&self, out_rate: f64) -> Option<usize> {
        // slower than this, the next beat is too far to wait for
        const MIN_SPEED: f64 = 0.1;
        let grid = self.grid?;
        if self.speed < MIN_SPEED {
            return None;
        }
        let beats = grid.beats_at(self.playhead.position_s());
        let to_next = 1.0 - beats.rem_euclid(1.0);
        Some((to_next * 60.0 / grid.bpm / self.speed * out_rate) as usize)
    }

    /// The beat the effects follow, at the current position.
    pub(crate) fn effect_beat(&self) -> Beat {
        // stopped or crawling, effects keep a usable tempo
        const SLOWEST_RATE: f64 = 0.25;
        match self.grid {
            Some(g) => Beat {
                seconds: (60.0 / (g.bpm * self.tempo_rate().max(SLOWEST_RATE))) as f32,
                phase: Some(g.beats_at(self.playhead.position_s()).rem_euclid(1.0) as f32),
            },
            None => Beat::DEFAULT,
        }
    }

    /// Playback rate without the gestures: what a follower matches.
    fn tempo_rate(&self) -> f64 {
        match self.drive_mode() {
            DriveMode::Internal if self.sync.locked => self.sync.held_rate.unwrap_or(self.pitch),
            DriveMode::Internal => self.pitch,
            DriveMode::Relative if self.sync.locked => {
                self.sync.held_rate.unwrap_or(self.sync.platter_reference)
            }
            DriveMode::Relative => self.sync.tempo.rate() * self.sync.rate_offset,
            DriveMode::Absolute => self.sync.tempo.rate(),
        }
    }

    /// The censor or the reverse plays the track backwards (a record in absolute mode keeps
    /// it where it is).
    fn backwards(&self) -> bool {
        (self.transport.censoring || self.transport.reversing()) && self.drive_mode() != DriveMode::Absolute
    }

    /// Whether a follower keeps to the leader's beats, not its tempo alone: not backwards,
    /// its beats running against the leader's, nor in a loop, its beats jumping back at each
    /// turn.
    fn chases_beats(&self) -> bool {
        !self.backwards() && !self.transport.loop_active
    }

    /// Whether the track advances at its tempo (gestures aside).
    fn is_moving(&self) -> bool {
        match self.drive_mode() {
            DriveMode::Internal => self.playing,
            DriveMode::Relative | DriveMode::Absolute => self.sync.platter_now().is_some(),
        }
    }

    fn drive_mode(&self) -> DriveMode {
        match (self.vinyl.enabled, self.vinyl.mode) {
            (false, _) => DriveMode::Internal,
            (true, VinylMode::Relative) => DriveMode::Relative,
            (true, VinylMode::Absolute) => DriveMode::Absolute,
        }
    }

    pub(crate) fn status(&self, leader: bool) -> DeckStatus {
        let position_s = self.playhead.position_s();
        let rate = self.tempo_rate();
        DeckStatus {
            loaded: self.playhead.is_loaded(),
            playing: self.playing,
            position_s,
            duration_s: self.playhead.duration_s(),
            speed: self.speed,
            rate,
            bpm: self.grid.map(|g| g.bpm * rate),
            beat_phase: self.grid.map(|g| g.beats_at(position_s).rem_euclid(1.0)),
            sync: SyncStatus { locked: self.sync.locked, leader, rate_offset: self.sync.rate_offset },
            looping: LoopStatus {
                region: self.transport.loop_region,
                active: self.transport.loop_active,
                loop_in_s: self.transport.loop_in_s,
            },
            peak: self.peak,
            samples_playing: self.sampler.playing(),
            cue: self.cue,
            slip_mode: self.transport.slip_mode,
            reverse: self.transport.reversed,
            slip_position_s: self.transport.shadow_s,
            stems_waiting: self.bar_mix.waiting(),
            stems_here: self.playhead.stems_here(),
            vinyl: self.vinyl.status(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DriveMode {
    Internal,
    Relative,
    Absolute,
}
