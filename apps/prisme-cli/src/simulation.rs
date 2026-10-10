//! Command line names for the simulated turntable patterns.

use clap::ValueEnum;
use prisme_timecode::testing::Pattern as TimecodePattern;

#[derive(Clone, Copy, ValueEnum)]
pub enum Pattern {
    /// Plays at +2% for ever.
    Steady,
    /// Plays, then scratches back and forth every few seconds.
    Scratch,
    /// Plays with small pitch nudges, like when beatmatching by hand.
    Nudge,
}

impl From<Pattern> for TimecodePattern {
    fn from(p: Pattern) -> Self {
        match p {
            Pattern::Steady => TimecodePattern::Steady,
            Pattern::Scratch => TimecodePattern::Scratch,
            Pattern::Nudge => TimecodePattern::Nudge,
        }
    }
}
