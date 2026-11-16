//! Timecode formats: the bit sequence pressed on a control record, and the table used
//! to find where a sequence of bits sits on the record.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

/// Stereo channel of the turntable signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Left,
    Right,
}

/// Description of a control record.
///
/// The signal is a quadrature carrier (`resolution` cycles per second at 33 1/3 rpm).
/// Every carrier cycle carries one bit of a linear feedback shift register (LFSR)
/// sequence, encoded in the amplitude of the `primary` channel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimecodeFormat {
    /// Stable identifier, used in settings.
    pub id: &'static str,
    /// Who makes the record.
    pub maker: &'static str,
    /// The record (or CD), as its maker names it.
    pub record: &'static str,
    /// The side of the record, when its sides carry different timecodes.
    pub side: Option<&'static str>,
    /// Size of the LFSR register, in bits.
    pub bits: u32,
    /// Register value at the very start of the record.
    pub seed: u32,
    /// LFSR taps (excluding the end taps).
    pub taps: u32,
    /// Number of carrier cycles on the record.
    pub length: u32,
    /// Last cycle before the run-out / label area.
    pub safe: u32,
    /// Carrier cycles per second at nominal speed.
    pub resolution: f64,
    /// Where a track starts on the record, in seconds at 33 1/3 rpm: the grooves before
    /// are where the needle is put down (Mixxx's values).
    pub lead_in_s: f64,
    /// Channel carrying the bits.
    pub primary: Channel,
    /// Sign of the carrier phase rotation (atan2(primary, secondary)) for forward play.
    pub phase_sign: f64,
}

/// Serato Control Vinyl, 2nd edition, side A.
pub const SERATO_CV02_A: TimecodeFormat = TimecodeFormat {
    id: "serato_cv02_a",
    maker: "Serato",
    record: "Control vinyl CV02",
    side: Some("A"),
    bits: 20,
    seed: 0x59017,
    taps: 0x361e4,
    length: 712_000,
    safe: 625_000,
    resolution: 1000.0,
    lead_in_s: 28.0,
    primary: Channel::Right,
    phase_sign: -1.0,
};

/// Serato Control Vinyl, 2nd edition, side B.
pub const SERATO_CV02_B: TimecodeFormat = TimecodeFormat {
    id: "serato_cv02_b",
    maker: "Serato",
    record: "Control vinyl CV02",
    side: Some("B"),
    bits: 20,
    seed: 0x8f3c6,
    taps: 0x4f0d8,
    length: 922_000,
    safe: 908_000,
    resolution: 1000.0,
    lead_in_s: 28.0,
    primary: Channel::Right,
    phase_sign: -1.0,
};

/// Serato control CD.
pub const SERATO_CD: TimecodeFormat = TimecodeFormat {
    id: "serato_cd",
    maker: "Serato",
    record: "Control CD",
    side: None,
    bits: 20,
    seed: 0xd8b40,
    taps: 0x34d54,
    length: 950_000,
    safe: 890_000,
    resolution: 1000.0,
    lead_in_s: 0.0,
    primary: Channel::Right,
    phase_sign: -1.0,
};

/// Every record Prisme reads, by maker, then record, then side: the settings offer them in
/// this order. Another record is another entry here.
pub const FORMATS: &[TimecodeFormat] = &[SERATO_CV02_A, SERATO_CV02_B, SERATO_CD];

impl TimecodeFormat {
    /// Finds a format by its identifier.
    pub fn by_id(id: &str) -> Option<&'static TimecodeFormat> {
        FORMATS.iter().find(|f| f.id == id)
    }

    /// Next register value when the record moves forward by one cycle.
    pub fn next_state(&self, state: u32) -> u32 {
        let feedback = (state & (self.taps | 1)).count_ones() & 1;
        (state >> 1) | (feedback << (self.bits - 1))
    }

    /// Duration of one carrier cycle at nominal speed, in milliseconds.
    pub fn cycle_ms(&self) -> f64 {
        1000.0 / self.resolution
    }
}

/// Maps every register value of a format to its cycle index on the record.
///
/// This is a direct table (2^bits entries, 4 MB for 20 bits), built once per format and
/// shared between decoders. Build it off the audio thread.
pub struct SequenceTable {
    lut: Vec<u32>,
}

const NONE: u32 = u32::MAX;

impl SequenceTable {
    fn build(format: &TimecodeFormat) -> Self {
        let mut lut = vec![NONE; 1usize << format.bits];
        let mut state = format.seed;
        for index in 0..format.length {
            lut[state as usize] = index;
            state = format.next_state(state);
        }
        Self { lut }
    }

    /// Returns the shared table for `format`, building it on first use.
    pub fn for_format(format: &TimecodeFormat) -> Arc<SequenceTable> {
        static CACHE: OnceLock<Mutex<HashMap<&'static str, Arc<SequenceTable>>>> = OnceLock::new();
        let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
        let mut cache = cache.lock().unwrap_or_else(|e| e.into_inner());
        cache.entry(format.id).or_insert_with(|| Arc::new(SequenceTable::build(format))).clone()
    }

    /// Cycle index of the register value whose newest bit is the most significant one.
    pub fn index_of(&self, state: u32) -> Option<u32> {
        match self.lut.get(state as usize) {
            Some(&i) if i != NONE => Some(i),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_maps_the_whole_sequence() {
        let f = &SERATO_CV02_A;
        let table = SequenceTable::for_format(f);
        let mut state = f.seed;
        for i in 0..1000 {
            assert_eq!(table.index_of(state), Some(i));
            state = f.next_state(state);
        }
        assert_eq!(table.index_of(0), None, "the all-zero register never appears in an LFSR");
    }

    /// Whether equal keys all sit next to each other.
    fn grouped<T: PartialEq>(keys: &[T]) -> bool {
        (1..keys.len()).all(|i| keys[i - 1] == keys[i] || !keys[..i].contains(&keys[i]))
    }

    fn unique<T: PartialEq>(keys: &[T]) -> bool {
        (0..keys.len()).all(|i| !keys[..i].contains(&keys[i]))
    }

    #[test]
    fn the_records_of_a_maker_and_the_sides_of_a_record_follow_each_other() {
        let makers: Vec<_> = FORMATS.iter().map(|f| f.maker).collect();
        let records: Vec<_> = FORMATS.iter().map(|f| (f.maker, f.record)).collect();
        assert!(grouped(&makers) && grouped(&records));
        assert!(unique(&FORMATS.iter().map(|f| (f.maker, f.record, f.side)).collect::<Vec<_>>()));
        assert!(unique(&FORMATS.iter().map(|f| f.id).collect::<Vec<_>>()));
        // a record without sides is alone
        for f in FORMATS.iter().filter(|f| f.side.is_none()) {
            assert_eq!(records.iter().filter(|&&r| r == (f.maker, f.record)).count(), 1, "{}", f.record);
        }
    }

    #[test]
    fn formats_are_found_by_id() {
        assert_eq!(TimecodeFormat::by_id("serato_cv02_b").map(|f| f.seed), Some(0x8f3c6));
        assert!(TimecodeFormat::by_id("unknown").is_none());
    }
}
