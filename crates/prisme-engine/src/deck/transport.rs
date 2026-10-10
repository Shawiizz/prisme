//! Moving around the track: loops, loop rolls, jumps, slip and censor.
//!
//! A record in absolute mode decides where the track is, so none of this applies to it.

use super::{Deck, DriveMode};
use crate::looping::LoopRegion;

#[derive(Default)]
pub(crate) struct Transport {
    pub(super) loop_region: Option<LoopRegion>,
    pub(super) loop_active: bool,
    pub(super) slip_mode: bool,
    /// Where the track would be without the loop, roll or censor in progress.
    pub(super) shadow_s: Option<f64>,
    rolling: bool,
    pub(super) censoring: bool,
}

impl Deck {
    /// Forgets everything tied to the previous track.
    pub(crate) fn reset_track_state(&mut self) {
        self.grid = None;
        self.sync.reset();
        self.transport = Transport { slip_mode: self.transport.slip_mode, ..Transport::default() };
        self.playhead.set_loop(None);
    }

    pub(crate) fn set_loop(&mut self, region: Option<LoopRegion>, active: bool) {
        if !self.controls_position() {
            return;
        }
        self.transport.loop_region = region;
        self.activate_loop(active && region.is_some());
    }

    pub(crate) fn beat_loop(&mut self, beats: f64, quantize: bool) {
        let Some(grid) = self.grid else { return };
        if !self.controls_position() || beats <= 0.0 {
            return;
        }
        self.transport.loop_region = Some(LoopRegion::of_beats(&grid, self.playhead.position_s(), beats, quantize));
        self.activate_loop(true);
    }

    /// `Some(beats)`: loops while held, the track carrying on underneath; `None`: release.
    pub(crate) fn loop_roll(&mut self, beats: Option<f64>, quantize: bool) {
        match beats {
            Some(beats) if !self.transport.rolling => {
                self.start_slip();
                self.transport.rolling = true;
                self.beat_loop(beats, quantize);
            }
            None if self.transport.rolling => {
                self.transport.rolling = false;
                self.activate_loop(false);
                self.end_slip();
            }
            _ => {}
        }
    }

    pub(crate) fn exit_loop(&mut self) {
        self.activate_loop(false);
    }

    /// Toggles the stored loop; turning it on past its end goes back to its start.
    pub(crate) fn reloop(&mut self) {
        let Some(region) = self.transport.loop_region else { return };
        if self.transport.loop_active {
            self.activate_loop(false);
        } else if self.controls_position() {
            if self.playhead.position_s() >= region.end_s {
                self.playhead.seek_s(region.start_s);
            }
            self.activate_loop(true);
        }
    }

    pub(crate) fn scale_loop(&mut self, factor: f64) {
        if let Some(region) = self.transport.loop_region.filter(|_| factor > 0.0) {
            self.transport.loop_region = Some(region.scaled(factor));
            self.activate_loop(self.transport.loop_active);
        }
    }

    /// Moves by whole beats; an active loop around the playhead moves along.
    pub(crate) fn beat_jump(&mut self, beats: f64) {
        let Some(grid) = self.grid else { return };
        if !self.controls_position() {
            return;
        }
        let by_s = beats * grid.beat_period_s();
        let position = self.playhead.position_s();
        if let Some(region) = self.transport.loop_region.filter(|r| self.transport.loop_active && r.contains(position)) {
            self.transport.loop_region = Some(region.shifted(by_s));
            self.activate_loop(true);
        }
        self.playhead.seek_s(position + by_s);
    }

    /// Jumps to `target_s`; while playing with quantize on, keeps the beat phase.
    pub(crate) fn jump(&mut self, target_s: f64, quantize: bool) {
        if !self.controls_position() {
            return;
        }
        let position = self.playhead.position_s();
        let landing = match self.grid {
            Some(grid) if quantize && self.is_moving() => grid.phase_preserving_target(position, target_s),
            _ => target_s,
        };
        self.playhead.seek_s(landing);
    }

    pub(crate) fn set_slip(&mut self, enabled: bool) {
        self.transport.slip_mode = enabled;
        if !enabled && !self.transport.rolling && !self.transport.censoring {
            self.transport.shadow_s = None;
        }
    }

    /// Plays backwards while held, then lands where the track would have been.
    pub(crate) fn censor(&mut self, active: bool) {
        if active == self.transport.censoring || !self.controls_position() {
            return;
        }
        self.transport.censoring = active;
        if active {
            self.start_slip();
        } else {
            self.end_slip();
        }
    }

    pub(super) fn advance_shadow(&mut self, seconds: f64) {
        if !self.is_moving() {
            return;
        }
        let rate = self.tempo_rate();
        if let Some(shadow) = self.transport.shadow_s.as_mut() {
            *shadow += rate * seconds;
        }
    }

    fn activate_loop(&mut self, active: bool) {
        let was_active = self.transport.loop_active;
        self.transport.loop_active = active;
        self.playhead.set_loop(self.transport.loop_region.filter(|_| active));
        if self.transport.slip_mode && !self.transport.rolling {
            match (was_active, active) {
                (false, true) => self.start_slip(),
                (true, false) => self.end_slip(),
                _ => {}
            }
        }
    }

    fn start_slip(&mut self) {
        if self.transport.shadow_s.is_none() {
            self.transport.shadow_s = Some(self.playhead.position_s());
        }
    }

    fn end_slip(&mut self) {
        if let Some(shadow) = self.transport.shadow_s.take() {
            self.playhead.seek_s(shadow);
        }
    }

    fn controls_position(&self) -> bool {
        self.playhead.is_loaded() && self.drive_mode() != DriveMode::Absolute
    }
}
