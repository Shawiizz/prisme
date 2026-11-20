//! The parts of a separated track a deck mutes or turns down: a stem, or the instrumental (all
//! but the vocals), and how the deck plays each.

use prisme_control::StemPart;
use prisme_engine::Stem;
use serde::{Deserialize, Serialize};

/// How far a track is separated, the further the greater.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Separation {
    #[default]
    None,
    /// Its vocals, the rest being the instrumental.
    Vocals,
    /// Its vocals, drums and bass, the rest being the other instruments.
    Full,
}

impl Separation {
    /// From which stems there are; the drums and bass count only with the vocals.
    pub fn of(has: impl Fn(Stem) -> bool) -> Self {
        if !has(Stem::Vocals) {
            Self::None
        } else if has(Stem::Drums) && has(Stem::Bass) {
            Self::Full
        } else {
            Self::Vocals
        }
    }

    /// The parts a track separated this far is shown in, one knob each.
    pub fn parts(self) -> &'static [StemPart] {
        match self {
            Separation::None => &[],
            Separation::Vocals => &[StemPart::Vocals, StemPart::Instrumental],
            Separation::Full => &[StemPart::Vocals, StemPart::Drums, StemPart::Bass, StemPart::Melody],
        }
    }

    /// Whether a track separated this far has `part` to mute.
    pub fn has(self, part: StemPart) -> bool {
        match self {
            Separation::None => false,
            Separation::Vocals => matches!(part, StemPart::Vocals | StemPart::Instrumental),
            Separation::Full => true,
        }
    }
}

/// A part's level: where its knob stands (0 to 1), kept while it is muted.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StemLevel {
    pub level: f32,
    pub on: bool,
}

impl Default for StemLevel {
    fn default() -> Self {
        Self { level: 1.0, on: true }
    }
}

impl StemLevel {
    /// The gain of the knob's position along a volume taper (the square: -12 dB half way),
    /// so that the whole travel is heard, as on a fader.
    fn gain(self) -> f32 {
        if self.on {
            self.level.clamp(0.0, 1.0).powi(2)
        } else {
            0.0
        }
    }
}

/// How a deck plays each part of the track.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StemMix {
    pub vocals: StemLevel,
    pub drums: StemLevel,
    pub bass: StemLevel,
    /// What no stem holds: the instrumental of a track separated into its vocals alone.
    pub other: StemLevel,
}

/// The stems `part` stands for, by their place in the engine's bits (vocals, drums, bass,
/// other).
fn stems_of(part: StemPart) -> &'static [usize] {
    match part {
        StemPart::Vocals => &[0],
        StemPart::Drums => &[1],
        StemPart::Bass => &[2],
        StemPart::Melody => &[3],
        StemPart::Instrumental => &[1, 2, 3],
    }
}

/// Whether any stem of `part` is among `bits`, a bit each by the engine's order.
pub fn part_in(bits: u8, part: StemPart) -> bool {
    stems_of(part).iter().any(|&s| bits & 1 << s != 0)
}

impl StemMix {
    fn levels(&self) -> [&StemLevel; 4] {
        [&self.vocals, &self.drums, &self.bass, &self.other]
    }

    fn levels_mut(&mut self) -> [&mut StemLevel; 4] {
        [&mut self.vocals, &mut self.drums, &mut self.bass, &mut self.other]
    }

    /// Whether `part` plays: the instrumental while any of its instruments does.
    pub fn playing(&self, part: StemPart) -> bool {
        let levels = self.levels();
        stems_of(part).iter().any(|&s| levels[s].on)
    }

    /// With `part` muted, or played again: the instrumental mutes all its instruments, or
    /// plays them all. The levels stay.
    pub fn toggled(mut self, part: StemPart) -> Self {
        let on = !self.playing(part);
        let levels = self.levels_mut();
        for &s in stems_of(part) {
            levels[s].on = on;
        }
        self
    }

    /// The level of `part`: the instrumental's loudest instrument.
    pub fn level(&self, part: StemPart) -> f32 {
        let levels = self.levels();
        stems_of(part).iter().map(|&s| levels[s].level).fold(0.0, f32::max)
    }

    /// With the level of `part` at `level` (the instrumental's instruments together).
    pub fn with_level(mut self, part: StemPart, level: f32) -> Self {
        let levels = self.levels_mut();
        for &s in stems_of(part) {
            levels[s].level = level.clamp(0.0, 1.0);
        }
        self
    }
}

impl From<StemMix> for prisme_engine::StemMix {
    fn from(m: StemMix) -> Self {
        Self { vocals: m.vocals.gain(), drums: m.drums.gain(), bass: m.bass.gain(), other: m.other.gain() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OFF: StemLevel = StemLevel { level: 1.0, on: false };

    fn all() -> StemMix {
        StemMix::default()
    }

    #[test]
    fn a_separation_offers_its_parts() {
        assert!(!Separation::None.has(StemPart::Vocals));
        assert!(Separation::Vocals.has(StemPart::Vocals));
        assert!(Separation::Vocals.has(StemPart::Instrumental));
        assert!(!Separation::Vocals.has(StemPart::Drums));
        assert!(Separation::Full.has(StemPart::Bass));
        assert!(Separation::Full.has(StemPart::Instrumental));
    }

    #[test]
    fn the_engines_bits_hold_a_part_the_instrumental_by_any_of_its_instruments() {
        assert!(part_in(0b0001, StemPart::Vocals));
        assert!(!part_in(0b0001, StemPart::Drums));
        assert!(part_in(0b0100, StemPart::Bass));
        assert!(part_in(0b1000, StemPart::Instrumental));
        assert!(part_in(0b1000, StemPart::Melody));
        assert!(!part_in(0b0001, StemPart::Instrumental));
    }

    #[test]
    fn one_stem_toggles_alone() {
        assert_eq!(all().toggled(StemPart::Drums), StemMix { drums: OFF, ..all() });
        assert_eq!(StemMix { other: OFF, ..all() }.toggled(StemPart::Melody), all());
    }

    #[test]
    fn the_whole_instrumental_mutes_then_plays_again() {
        let muted = all().toggled(StemPart::Instrumental);
        assert_eq!(muted, StemMix { vocals: all().vocals, drums: OFF, bass: OFF, other: OFF });
        assert!(!muted.playing(StemPart::Instrumental));
        assert_eq!(muted.toggled(StemPart::Instrumental), all());
    }

    #[test]
    fn the_instrumental_plays_while_any_instrument_does() {
        let drums_only = StemMix { vocals: OFF, drums: all().drums, bass: OFF, other: OFF };
        assert!(drums_only.playing(StemPart::Instrumental));
        assert_eq!(drums_only.toggled(StemPart::Instrumental), StemMix { drums: OFF, ..drums_only });
    }

    #[test]
    fn a_level_is_kept_through_a_mute() {
        let muted = all().with_level(StemPart::Bass, 0.4).toggled(StemPart::Bass);
        assert_eq!(muted.bass, StemLevel { level: 0.4, on: false });
        assert_eq!(muted.toggled(StemPart::Bass).bass, StemLevel { level: 0.4, on: true });
    }

    #[test]
    fn the_instrumental_level_is_set_on_all_its_instruments_within_0_and_1() {
        assert_eq!(all().with_level(StemPart::Instrumental, 1.5).level(StemPart::Instrumental), 1.0);
        let low = all().with_level(StemPart::Instrumental, 0.25);
        assert_eq!(
            [low.drums.level, low.bass.level, low.other.level, low.vocals.level],
            [0.25, 0.25, 0.25, 1.0]
        );
    }

    #[test]
    fn a_stem_knob_follows_a_volume_taper_and_a_mute_keeps_its_level() {
        let at = |level, on| StemLevel { level, on };
        let mix = StemMix { vocals: at(0.5, true), drums: at(1.0, false), ..Default::default() };
        let engine = prisme_engine::StemMix::from(mix);
        assert_eq!((engine.vocals, engine.drums, engine.bass), (0.25, 0.0, 1.0));
    }
}
