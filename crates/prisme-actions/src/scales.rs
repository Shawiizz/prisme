//! Where a knob or a fader stands (0 full left or down, 0.5 the centre, 1 full right or up) and
//! the setting it gives: each scale both ways, the setting read back as a position, and what a
//! knob reads under it.

use serde::Serialize;

/// Below this the EQ knob removes its band, like an isolator, and a live input is silent.
const SILENT_BELOW: f32 = 0.02;
const EQ_CUT_DB: f32 = 40.0;
const EQ_BOOST_DB: f32 = 6.0;
const TRIM_RANGE_DB: f32 = 12.0;
/// A live input's knob: 0 dB three quarters of the way, +12 dB at the end.
const LIVE_UNITY: f32 = 0.75;
const LIVE_BOOST_DB: f32 = 12.0;
/// The headphone volume knob goes up to +3.5 dB.
pub const MAX_HEADPHONE_GAIN: f32 = 1.5;

fn gain_of(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

fn db_of(gain: f32) -> f32 {
    20.0 * gain.log10()
}

/// EQ: full left removes the band, the centre is flat, full right +6 dB.
pub fn eq_gain(knob: f32) -> f32 {
    if knob <= SILENT_BELOW {
        return 0.0;
    }
    let range = if knob < 0.5 { EQ_CUT_DB } else { EQ_BOOST_DB };
    gain_of((knob - 0.5) / 0.5 * range)
}

pub fn eq_knob(gain: f32) -> f32 {
    if gain <= 0.0 {
        return 0.0;
    }
    let db = db_of(gain);
    let range = if db < 0.0 { EQ_CUT_DB } else { EQ_BOOST_DB };
    (0.5 + db / range * 0.5).clamp(SILENT_BELOW + 1e-6, 1.0)
}

/// Filter: full left closes the low-pass (-1), the centre is open (0), full right closes the
/// high-pass (1).
pub fn filter_amount(knob: f32) -> f32 {
    knob * 2.0 - 1.0
}

pub fn filter_knob(amount: f32) -> f32 {
    (amount + 1.0) / 2.0
}

/// Trim: -12 dB to +12 dB, flat at the centre.
pub fn trim_gain(knob: f32) -> f32 {
    gain_of((knob - 0.5) * 2.0 * TRIM_RANGE_DB)
}

pub fn trim_knob(gain: f32) -> f32 {
    (0.5 + db_of(gain) / (2.0 * TRIM_RANGE_DB)).clamp(0.0, 1.0)
}

/// A live input's level: silent full left, 0 dB at three quarters, +12 dB full right.
pub fn live_gain(knob: f32) -> f32 {
    if knob <= SILENT_BELOW {
        return 0.0;
    }
    gain_of((knob - LIVE_UNITY) / (1.0 - LIVE_UNITY) * LIVE_BOOST_DB)
}

pub fn live_knob(gain: f32) -> f32 {
    if gain <= 0.0 {
        return 0.0;
    }
    (LIVE_UNITY + db_of(gain) / LIVE_BOOST_DB * (1.0 - LIVE_UNITY)).clamp(SILENT_BELOW + 1e-6, 1.0)
}

/// Balance: full left plays the master on the left only (-1), full right on the right (1).
pub fn balance_amount(knob: f32) -> f32 {
    knob * 2.0 - 1.0
}

pub fn balance_knob(amount: f32) -> f32 {
    (amount + 1.0) / 2.0
}

pub fn headphone_gain(knob: f32) -> f32 {
    knob * MAX_HEADPHONE_GAIN
}

pub fn headphone_knob(gain: f32) -> f32 {
    gain / MAX_HEADPHONE_GAIN
}

/// What a knob reads under it: its setting as the DJ reads it, which the screen only words.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum Readout {
    /// A gain in dB, to a tenth; `None` silent.
    Db(Option<f32>),
    Percent(i32),
    /// The side the master leans to, as a percentage turned down on the other: negative on
    /// the left.
    Balance(i32),
    /// How closed the filter is, as a percentage: negative a low-pass, positive a high-pass,
    /// 0 open.
    Filter(i32),
}

impl Readout {
    /// A linear gain.
    pub fn db(gain: f32) -> Self {
        // + 0.0: a tenth of a dB under nothing reads 0, not -0
        Readout::Db((gain > 0.0).then(|| (db_of(gain) * 10.0).round() / 10.0 + 0.0))
    }

    /// A share from 0 to 1.
    pub fn percent(share: f32) -> Self {
        Readout::Percent((share * 100.0).round() as i32)
    }

    /// The master's balance from -1 to 1.
    pub fn balance(amount: f32) -> Self {
        Readout::Balance((amount * 100.0).round() as i32)
    }

    /// A filter from -1 to 1: open within 2 % of the centre.
    pub fn filter(amount: f32) -> Self {
        let closed = (amount.abs() * 100.0).round() as i32;
        Readout::Filter(if closed < 2 { 0 } else { closed * amount.signum() as i32 })
    }
}

/// What a deck's tempo fader acts on: the tempo now, as a playback rate, and the fader's
/// range (0.08 for ±8 %).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tempo {
    pub ratio: f64,
    pub range: f64,
}

impl Tempo {
    /// Where the fader stands at this tempo, the middle being the track's own.
    pub fn position(self) -> f32 {
        (0.5 + (self.ratio - 1.0) / (2.0 * self.range)).clamp(0.0, 1.0) as f32
    }

    /// The tempo the fader gives at `position`.
    pub fn at(self, position: f32) -> f64 {
        1.0 + (f64::from(position) * 2.0 - 1.0) * self.range
    }
}

/// The ranges a tempo fader goes through: ±8, ±16, ±50 %.
pub const TEMPO_RANGES: [f64; 3] = [0.08, 0.16, 0.5];

/// The range `step` places after `range` among [`TEMPO_RANGES`] (before it, negative), round
/// from the last to the first; the first for a range not among them.
pub fn next_tempo_range(range: f64, step: isize) -> f64 {
    let Some(at) = TEMPO_RANGES.iter().position(|r| *r == range) else { return TEMPO_RANGES[0] };
    TEMPO_RANGES[(at as isize + step).rem_euclid(TEMPO_RANGES.len() as isize) as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_knob_reads_its_setting_in_db_percent_or_the_side_it_leans_to() {
        assert_eq!(Readout::db(0.0), Readout::Db(None), "silent");
        assert_eq!(Readout::db(1.0), Readout::Db(Some(0.0)));
        assert_eq!(Readout::db(0.999), Readout::Db(Some(0.0)), "never -0");
        assert_eq!(Readout::db(eq_gain(1.0)), Readout::Db(Some(6.0)));
        assert_eq!(Readout::db(trim_gain(0.0)), Readout::Db(Some(-12.0)));
        assert_eq!(Readout::db(headphone_gain(1.0)), Readout::Db(Some(3.5)));
        assert_eq!(Readout::percent(0.424), Readout::Percent(42));
        assert_eq!(Readout::balance(balance_amount(0.3)), Readout::Balance(-40));
        assert_eq!(Readout::filter(filter_amount(0.505)), Readout::Filter(0), "open near the centre");
        assert_eq!(Readout::filter(filter_amount(0.2)), Readout::Filter(-60), "a low-pass");
        assert_eq!(Readout::filter(filter_amount(0.9)), Readout::Filter(80), "a high-pass");
    }

    #[test]
    fn a_tempo_range_goes_round_either_way() {
        assert_eq!(next_tempo_range(0.08, 1), 0.16);
        assert_eq!(next_tempo_range(0.5, 1), 0.08);
        assert_eq!(next_tempo_range(0.08, -1), 0.5);
        assert_eq!(next_tempo_range(0.1, 1), 0.08, "one of none of them");
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn eq_removes_the_band_full_left_is_flat_at_the_centre_and_boosts_right() {
        assert_eq!(eq_gain(0.0), 0.0);
        assert!(close(eq_gain(0.5), 1.0));
        assert!(close(eq_gain(1.0), gain_of(6.0)));
        assert!(close(eq_gain(0.25), gain_of(-20.0)));
    }

    #[test]
    fn eq_knob_and_gain_read_each_other_back() {
        for knob in [0.1, 0.3, 0.5, 0.75, 1.0] {
            assert!(close(eq_knob(eq_gain(knob)), knob), "{knob}");
        }
        assert_eq!(eq_knob(0.0), 0.0);
    }

    #[test]
    fn filter_low_pass_left_open_at_the_centre_high_pass_right() {
        assert_eq!([0.0, 0.5, 1.0].map(filter_amount), [-1.0, 0.0, 1.0]);
        assert!(close(filter_knob(filter_amount(0.3)), 0.3));
    }

    #[test]
    fn trim_is_twelve_decibels_around_flat() {
        assert!(close(trim_gain(0.5), 1.0));
        assert!(close(trim_gain(1.0), gain_of(12.0)));
        assert!(close(trim_knob(trim_gain(0.2)), 0.2));
    }

    #[test]
    fn a_live_input_is_silent_then_flat_at_three_quarters_then_twelve_decibels_up() {
        assert_eq!(live_gain(0.0), 0.0);
        assert!(close(live_gain(0.75), 1.0));
        assert!(close(live_gain(1.0), gain_of(12.0)));
        assert!(close(live_knob(live_gain(0.4)), 0.4));
        assert_eq!(live_knob(0.0), 0.0);
    }

    #[test]
    fn balance_left_only_both_sides_right_only() {
        assert_eq!([0.0, 0.5, 1.0].map(balance_amount), [-1.0, 0.0, 1.0]);
        assert!(close(balance_knob(balance_amount(0.3)), 0.3));
    }

    #[test]
    fn the_headphones_go_half_again_as_loud_at_the_end() {
        assert_eq!(headphone_gain(1.0), 1.5);
        assert!(close(headphone_knob(headphone_gain(0.4)), 0.4));
    }

    #[test]
    fn a_tempo_fader_stands_where_its_tempo_is_within_its_range() {
        let tempo = Tempo { ratio: 1.04, range: 0.08 };
        assert!(close(tempo.position(), 0.75));
        assert!((tempo.at(0.75) - 1.04).abs() < 1e-9);
        assert_eq!(Tempo { ratio: 1.2, ..tempo }.position(), 1.0, "past its range, at its end");
    }
}
