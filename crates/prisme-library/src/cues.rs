//! Cue points of a track: the main cue and the hot cues, each a point or a loop.

/// Two pages of eight.
pub const HOT_CUES: u8 = 16;

/// One colour per hot cue slot of a page (0xRRGGBB), the usual DJ palette order: the tags of
/// the design system (frontend/src/styles/tokens.css). The second page takes them again.
pub const DEFAULT_COLORS: [u32; 8] =
    [0xFF5442, 0xFF9A3C, 0xF5D547, 0xA6E25C, 0x3FD3B5, 0x4AA8FF, 0x8F7BFF, 0xE86BD6];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CueSlot {
    Main,
    Hot(u8),
}

impl CueSlot {
    pub(crate) fn to_db(self) -> i64 {
        match self {
            CueSlot::Main => -1,
            CueSlot::Hot(n) => n as i64,
        }
    }

    pub(crate) fn from_db(value: i64) -> Option<Self> {
        match value {
            -1 => Some(CueSlot::Main),
            n if (0..HOT_CUES as i64).contains(&n) => Some(CueSlot::Hot(n as u8)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    pub slot: CueSlot,
    pub position_s: f64,
    /// A saved loop starting at `position_s`.
    pub loop_length_s: Option<f64>,
    pub color: u32,
    pub label: String,
}

impl Cue {
    pub fn point(slot: CueSlot, position_s: f64) -> Self {
        Self { slot, position_s, loop_length_s: None, color: default_color(slot), label: String::new() }
    }

    pub fn looped(slot: CueSlot, start_s: f64, length_s: f64) -> Self {
        Self { loop_length_s: Some(length_s), ..Self::point(slot, start_s) }
    }
}

fn default_color(slot: CueSlot) -> u32 {
    match slot {
        CueSlot::Main => 0xFFB02E,
        CueSlot::Hot(n) => DEFAULT_COLORS[n as usize % DEFAULT_COLORS.len()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_second_page_of_hot_cues_takes_the_colours_of_the_first() {
        assert_eq!(Cue::point(CueSlot::Hot(9), 1.0).color, Cue::point(CueSlot::Hot(1), 1.0).color);
        assert_eq!(CueSlot::from_db(15), Some(CueSlot::Hot(15)));
        assert_eq!(CueSlot::from_db(16), None);
    }
}
