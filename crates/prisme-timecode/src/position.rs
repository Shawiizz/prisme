//! Bit reading and absolute position.
//!
//! One bit is read per carrier cycle, when the secondary channel crosses zero while the
//! primary is positive. Its value is the primary peak compared with the secondary peak
//! of the same cycle (their ratio does not depend on the signal level). The last `bits`
//! bits form a register whose index on the record is looked up in the
//! [`SequenceTable`]. Once locked, the position is predicted bit after bit, so a wrong
//! bit costs nothing; a new consistent position (needle drop) re-locks after a few bits.

use crate::format::{SequenceTable, TimecodeFormat};
use std::f64::consts::TAU;
use std::sync::Arc;

/// Peaks used to initialise the bit threshold.
const WARMUP_PEAKS: u32 = 8;
/// Averaging length of the bit threshold, in bits.
const THRESHOLD_LENGTH: f64 = 24.0;
/// Consecutive consistent lookups needed to lock (or re-lock elsewhere).
const LOCK_RUN: u32 = 3;
/// Bits without a single matching lookup before the lock is dropped.
const MAX_GARBAGE: u32 = 80;
/// A bit is read at most once per this fraction of a cycle.
const MIN_BIT_SPACING: f64 = 0.6;

pub(crate) struct PositionTracker {
    table: Arc<SequenceTable>,
    bits: u32,
    secondary_peak: f64,
    threshold: f64,
    peaks_seen: u32,
    last_bit_phase: Option<f64>,
    register: u32,
    bits_read: u32,
    locked: bool,
    index: u32,
    index_phase: f64,
    candidate: Option<u32>,
    candidate_run: u32,
    garbage: u32,
}

impl PositionTracker {
    pub(crate) fn new(format: &TimecodeFormat, table: Arc<SequenceTable>) -> Self {
        Self {
            table,
            bits: format.bits,
            secondary_peak: 0.0,
            threshold: 0.0,
            peaks_seen: 0,
            last_bit_phase: None,
            register: 0,
            bits_read: 0,
            locked: false,
            index: 0,
            index_phase: 0.0,
            candidate: None,
            candidate_run: 0,
            garbage: 0,
        }
    }

    /// The primary channel crossed zero: the secondary is at its peak.
    pub(crate) fn on_primary_crossing(&mut self, secondary_abs: f64) {
        self.secondary_peak = secondary_abs;
    }

    /// The secondary crossed zero with the primary positive: read a bit.
    /// `phase` is the unwrapped carrier phase (radians, forward positive).
    pub(crate) fn on_bit_crossing(&mut self, primary_peak: f64, phase: f64, forwards: bool) {
        if let Some(last) = self.last_bit_phase {
            if (phase - last).abs() < MIN_BIT_SPACING * TAU {
                return;
            }
        }
        self.last_bit_phase = Some(phase);
        if self.secondary_peak <= 1e-6 {
            return;
        }

        let ratio = primary_peak / self.secondary_peak;
        if self.peaks_seen < WARMUP_PEAKS {
            self.peaks_seen += 1;
            self.threshold += (ratio - self.threshold) / self.peaks_seen as f64;
        } else {
            self.threshold += (ratio - self.threshold) / THRESHOLD_LENGTH;
        }
        let bit = (ratio > self.threshold) as u32;
        let mask = (1u32 << self.bits) - 1;
        self.register = if forwards {
            (self.register >> 1) | (bit << (self.bits - 1))
        } else {
            ((self.register << 1) & mask) | bit
        };
        self.bits_read = (self.bits_read + 1).min(64);

        if self.locked {
            self.index = if forwards { self.index + 1 } else { self.index.saturating_sub(1) };
            self.index_phase = phase;
        }
        if self.bits_read < self.bits || self.peaks_seen < WARMUP_PEAKS {
            return;
        }

        // Going backwards the newest bit is the oldest cycle of the register.
        let lookup = self.table.index_of(self.register).and_then(|i| {
            if forwards {
                Some(i)
            } else {
                i.checked_sub(self.bits - 1)
            }
        });
        self.track_candidate(lookup, forwards);

        if self.locked {
            if lookup == Some(self.index) {
                self.garbage = 0;
            } else {
                // a wrong bit spoils the next `bits` lookups: keep the prediction meanwhile
                self.garbage += 1;
                if self.candidate_run >= LOCK_RUN && self.candidate != Some(self.index) {
                    self.lock_at(self.candidate);
                } else if self.garbage > MAX_GARBAGE {
                    self.locked = false;
                }
            }
        } else if self.candidate_run >= LOCK_RUN {
            self.lock_at(self.candidate);
        }
    }

    fn track_candidate(&mut self, lookup: Option<u32>, forwards: bool) {
        let Some(index) = lookup else {
            self.candidate = None;
            self.candidate_run = 0;
            return;
        };
        let expected = self.candidate.map(|c| if forwards { c.wrapping_add(1) } else { c.wrapping_sub(1) });
        self.candidate_run = if expected == Some(index) { self.candidate_run + 1 } else { 1 };
        self.candidate = Some(index);
    }

    fn lock_at(&mut self, index: Option<u32>) {
        if let (Some(index), Some(phase)) = (index, self.last_bit_phase) {
            self.locked = true;
            self.index = index;
            self.index_phase = phase;
            self.garbage = 0;
        }
    }

    pub(crate) fn is_locked(&self) -> bool {
        self.locked
    }

    /// Position in carrier cycles, interpolated with the phase since the last bit.
    pub(crate) fn position_cycles(&self, phase: f64) -> Option<f64> {
        // the bit is read a quarter of a cycle into its cycle
        self.locked.then(|| self.index as f64 + 0.25 + (phase - self.index_phase) / TAU)
    }
}
