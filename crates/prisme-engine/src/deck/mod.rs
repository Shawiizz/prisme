//! A deck: a track, a playhead and what drives it (internal play or the record).

mod transport;

use crate::command::{CrossfaderSide, VinylMode};
use crate::grid::BeatGrid;
use crate::keylock::KeyLock;
use crate::playhead::Playhead;
use crate::sampler::Sampler;
use crate::status::{DeckStatus, LoopStatus, SyncStatus};
use crate::strip::ChannelStrip;
use crate::sync::{self, DeckSync, LeaderBeat, SyncTarget};
use crate::vinyl::{VinylControl, VinylDrive};
use prisme_dsp::{Beat, FxSlot};
use prisme_timecode::TimecodeDecoder;
use transport::Transport;

/// In absolute mode, a gap larger than this (s) is a needle drop: jump.
const JUMP_THRESHOLD_S: f64 = 0.1;
/// Smaller gaps are closed smoothly: speed correction per second of gap, and its limit.
const PULL_PER_S: f64 = 5.0;
const MAX_CORRECTION: f64 = 0.05;

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
    pub(crate) fx: FxSlot,
    pub(crate) sampler: Sampler,
    pub(crate) sync: DeckSync,
    transport: Transport,
    correction: f64,
    speed: f64,
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
            fx: FxSlot::new(sample_rate as f32, max_block_frames),
            sampler: Sampler::new(),
            sync: DeckSync::new(),
            transport: Transport::default(),
            correction: 0.0,
            speed: 0.0,
        }
    }

    /// Renders one block from the turntable signal `input`; `target` is given when the
    /// deck follows a leader.
    pub(crate) fn process(
        &mut self,
        input: &[f32],
        out: &mut [f32],
        out_rate: f64,
        target: Option<SyncTarget>,
    ) {
        // decoded even with vinyl control off, for the signal indicator
        let drive = self.vinyl.process(input);
        let target = target.map(|t| self.engage(t));
        let speed = self.speed_for(drive, target);
        let speed = if self.transport.censoring { -speed } else { speed };
        if (self.key_lock_enabled || self.key_lock.key_shift() != 0) && KeyLock::applies(speed) {
            self.playhead.render_key_locked(&mut self.key_lock, out, out_rate, speed);
        } else {
            self.key_lock.disengage();
            self.playhead.render(out, out_rate, speed);
        }
        self.speed = speed;

        let seconds = (out.len() / 2) as f64 / out_rate;
        let platter = self.vinyl.enabled.then_some(drive.speed);
        self.sync.observe(speed, platter, seconds);
        self.advance_shadow(seconds);
        self.follow_record_position(drive);
    }

    /// Transposes the track; this goes through the key lock's stretcher, on or off.
    pub(crate) fn set_key_shift(&mut self, semitones: i8) {
        self.key_lock.set_key_shift(semitones);
    }

    /// Whether the deck follows a leader: locked, or about to match it once.
    pub(crate) fn follows(&self) -> bool {
        self.sync.locked || self.sync.once_pending || self.sync.snap_pending
    }

    pub(crate) fn can_lead(&self) -> bool {
        self.grid.is_some()
            && self.playhead.is_loaded()
            && !self.sync.locked
            && self.sync.smoothed_speed.abs() > sync::LEADER_MIN_SPEED
    }

    pub(crate) fn leader_beat(&self) -> Option<LeaderBeat> {
        let grid = self.grid?;
        Some(LeaderBeat {
            bpm: grid.bpm * self.tempo_rate(),
            beats: grid.beats_at(self.playhead.position_s()),
        })
    }

    pub(crate) fn sync_target(&self, leader: LeaderBeat) -> Option<SyncTarget> {
        let grid = self.grid?;
        Some(sync::sync_target(&grid, self.playhead.position_s(), leader))
    }

    /// Internal play is always locked; a relative record is locked or matched once.
    pub(crate) fn engage_sync(&mut self, locked: bool) {
        match (self.drive_mode(), locked) {
            (DriveMode::Absolute, _) => return,
            (DriveMode::Relative, false) => self.sync.once_pending = true,
            _ => self.sync.locked = true,
        }
        self.sync.snap_pending = true;
    }

    /// Applies what engaging sync asks for (phase snap, rate offset) and returns the
    /// target as it stands afterwards.
    fn engage(&mut self, mut target: SyncTarget) -> SyncTarget {
        let resnap = self.drive_mode() == DriveMode::Internal
            && self.sync.locked
            && self.playing
            && sync::needs_resnap(target.phase_error);
        if let (Some(grid), true) = (self.grid, self.sync.snap_pending || resnap) {
            let position = self.playhead.position_s() + sync::snap_offset_s(&grid, target.phase_error);
            self.playhead.seek_s(position);
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
                t.rate * (1.0 + sync::phase_correction(t.phase_error))
            }
            None => self.sync.held_rate.unwrap_or(self.pitch),
        };
        rate * (1.0 + self.bend)
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
                self.correction = (gap * PULL_PER_S).clamp(-MAX_CORRECTION, MAX_CORRECTION);
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
            DriveMode::Relative => self.sync.platter_reference * self.sync.rate_offset,
            DriveMode::Absolute => self.sync.platter_reference,
        }
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
        DeckStatus {
            loaded: self.playhead.is_loaded(),
            playing: self.playing,
            position_s,
            duration_s: self.playhead.duration_s(),
            speed: self.speed,
            bpm: self.grid.map(|g| g.bpm * self.tempo_rate()),
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
            slip_position_s: self.transport.shadow_s,
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
