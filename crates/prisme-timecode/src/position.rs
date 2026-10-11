//! Bit reading and absolute position.
//!
//! Each carrier cycle carries one bit, centred on the negative half of the primary
//! channel: the bit changes where the primary peaks. One bit is read per such lobe, from
//! the signal level averaged over it, against the average of the recent lobes. The last
//! `bits` bits form a register whose index on the record is looked up in the
//! [`SequenceTable`]. Once locked, the position follows the carrier phase, so wrong bits
//! and changes of direction cost nothing; a new position (needle drop) takes over once it
//! has been confirmed as long as the first lock.

use crate::format::{SequenceTable, TimecodeFormat};
use std::f64::consts::TAU;
use std::sync::Arc;

/// Lobes used to initialise the bit threshold.
const WARMUP_LOBES: u32 = 8;
/// Averaging length of the bit threshold, in bits.
const THRESHOLD_LENGTH: f64 = 16.0;
/// Consecutive consistent lookups needed to lock (or re-lock elsewhere). Any register
/// is a position, and a random one goes on following the sequence for one more bit half
/// the time: 24 bits leave one false lock in about 16 million.
const LOCK_RUN: u32 = 24;
/// Bits without a single matching lookup before the lock is dropped.
const MAX_GARBAGE: u32 = 80;
/// A bit is read at most once per this fraction of a cycle.
const MIN_BIT_SPACING: f64 = 0.6;

pub(crate) struct PositionTracker {
    table: Arc<SequenceTable>,
    bits: u32,
    threshold: f64,
    lobes_seen: u32,
    last_bit_phase: Option<f64>,
    register: u32,
    bits_read: u32,
    locked: bool,
    lock_cycle: u32,
    lock_phase: f64,
    candidate: Option<u32>,
    candidate_run: u32,
    garbage: u32,
}

impl PositionTracker {
    pub(crate) fn new(format: &TimecodeFormat, table: Arc<SequenceTable>) -> Self {
        Self {
            table,
            bits: format.bits,
            threshold: 0.0,
            lobes_seen: 0,
            last_bit_phase: None,
            register: 0,
            bits_read: 0,
            locked: false,
            lock_cycle: 0,
            lock_phase: 0.0,
            candidate: None,
            candidate_run: 0,
            garbage: 0,
        }
    }

    /// A negative lobe of the primary was crossed: read its bit. `level`: the signal
    /// level averaged over the lobe; `phase`: the unwrapped carrier phase at its centre
    /// (radians, forward positive); `forwards`: the direction the lobe was left in.
    pub(crate) fn on_lobe(&mut self, level: f64, phase: f64, forwards: bool) {
        if self.last_bit_phase.is_some_and(|last| (phase - last).abs() < MIN_BIT_SPACING * TAU) {
            return;
        }
        self.last_bit_phase = Some(phase);

        if self.lobes_seen < WARMUP_LOBES {
            self.lobes_seen += 1;
            self.threshold += (level - self.threshold) / self.lobes_seen as f64;
        } else {
            self.threshold += (level - self.threshold) / THRESHOLD_LENGTH;
        }
        let bit = (level > self.threshold) as u32;
        let mask = (1u32 << self.bits) - 1;
        self.register = if forwards {
            (self.register >> 1) | (bit << (self.bits - 1))
        } else {
            ((self.register << 1) & mask) | bit
        };
        self.bits_read = (self.bits_read + 1).min(64);
        if self.bits_read < self.bits || self.lobes_seen < WARMUP_LOBES {
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
            let expected = self.cycle_at(phase);
            if lookup == expected {
                self.garbage = 0;
            } else {
                // a wrong bit spoils the next `bits` lookups: the phase carries on meanwhile
                self.garbage += 1;
                if self.candidate_run >= LOCK_RUN && self.candidate != expected {
                    self.lock_at(self.candidate, phase);
                } else if self.garbage > MAX_GARBAGE {
                    self.locked = false;
                }
            }
        } else if self.candidate_run >= LOCK_RUN {
            self.lock_at(self.candidate, phase);
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

    fn lock_at(&mut self, cycle: Option<u32>, phase: f64) {
        if let Some(cycle) = cycle {
            self.locked = true;
            self.lock_cycle = cycle;
            self.lock_phase = phase;
            self.garbage = 0;
        }
    }

    /// The cycle whose lobe is centred at `phase`, by the phase travelled since the lock.
    fn cycle_at(&self, phase: f64) -> Option<u32> {
        let cycle = self.lock_cycle as f64 + ((phase - self.lock_phase) / TAU).round();
        (cycle >= 0.0).then_some(cycle as u32)
    }

    pub(crate) fn is_locked(&self) -> bool {
        self.locked
    }

    /// Position in carrier cycles: a cycle starts and ends where the primary peaks.
    pub(crate) fn position_cycles(&self, phase: f64) -> Option<f64> {
        self.locked.then(|| self.lock_cycle as f64 + 0.5 + (phase - self.lock_phase) / TAU)
    }
}
