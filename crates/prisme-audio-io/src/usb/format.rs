//! The samples of a USB audio stream, and how many frames each packet of an asynchronous
//! output carries.

/// Samples of `subslot` bytes (little endian, the sound in their high bits) to floats.
pub(crate) fn decode(bytes: &[u8], subslot: usize, out: &mut [f32]) {
    match subslot {
        2 => {
            for (o, b) in out.iter_mut().zip(bytes.chunks_exact(2)) {
                *o = f32::from(i16::from_le_bytes([b[0], b[1]])) / 32_768.0;
            }
        }
        3 => {
            for (o, b) in out.iter_mut().zip(bytes.chunks_exact(3)) {
                *o = i32::from_le_bytes([0, b[0], b[1], b[2]]) as f32 / 2_147_483_648.0;
            }
        }
        _ => {
            for (o, b) in out.iter_mut().zip(bytes.chunks_exact(4)) {
                *o = i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32 / 2_147_483_648.0;
            }
        }
    }
}

/// Floats to samples of `subslot` bytes, clipped to the full scale.
pub(crate) fn encode(samples: &[f32], subslot: usize, out: &mut [u8]) {
    for (s, b) in samples.iter().zip(out.chunks_exact_mut(subslot)) {
        let full = (f64::from(s.clamp(-1.0, 1.0)) * 2_147_483_648.0)
            .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32;
        let bytes = full.to_le_bytes();
        b.copy_from_slice(&bytes[4 - subslot..]);
    }
}

/// How many frames each packet of an output carries: the rate over the packets a second,
/// as the card asks through its feedback endpoint when it has one (its own clock paces it).
pub(crate) struct PacketSizer {
    nominal: f64,
    per_packet: f64,
    /// Packets of the data endpoint per second.
    packets_per_second: f64,
    /// The fraction of a frame carried over from one packet to the next.
    remainder: f64,
    max_frames: usize,
}

impl PacketSizer {
    pub fn new(sample_rate: u32, packets_per_second: u32, max_frames: usize) -> Self {
        let nominal = f64::from(sample_rate) / f64::from(packets_per_second);
        Self {
            nominal,
            per_packet: nominal,
            packets_per_second: f64::from(packets_per_second),
            remainder: 0.0,
            max_frames,
        }
    }

    /// What the card asked: frames per microframe in 16.16 at high speed (four bytes), per
    /// millisecond in 10.14 at full speed (three). Cards that count otherwise are read in the
    /// unit that brings them within a quarter of the rate; a value far from it is left aside.
    pub fn feedback(&mut self, bytes: &[u8]) {
        let (raw, readings): (u32, &[(f64, f64)]) = match *bytes {
            [a, b, c, d] => (
                u32::from_le_bytes([a, b, c, d]),
                &[(65_536.0, 8_000.0), (8_192.0, 8_000.0), (16_384.0, 1_000.0)],
            ),
            [a, b, c] => (u32::from_le_bytes([a, b, c, 0]), &[(16_384.0, 1_000.0)]),
            _ => return,
        };
        let asked = readings
            .iter()
            .map(|&(one, per_second)| f64::from(raw) / one * per_second / self.packets_per_second);
        if let Some(frames) = asked.into_iter().find(|f| (f / self.nominal - 1.0).abs() < 0.25) {
            self.per_packet = frames;
        }
    }

    /// The frames of the next packet.
    pub fn next(&mut self) -> usize {
        let exact = self.per_packet + self.remainder;
        let frames = (exact.floor() as usize).min(self.max_frames);
        self.remainder = (exact - frames as f64).clamp(0.0, 1.0);
        frames
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_come_back_as_they_went() {
        let samples = [0.0, 0.5, -0.5, 0.999, -1.0, 0.25];
        for subslot in [2, 3, 4] {
            let mut bytes = vec![0; samples.len() * subslot];
            encode(&samples, subslot, &mut bytes);
            let mut back = [0.0; 6];
            decode(&bytes, subslot, &mut back);
            for (a, b) in samples.iter().zip(back) {
                assert!((a - b).abs() < 1e-4, "{subslot} bytes: {a} came back {b}");
            }
        }
    }

    #[test]
    fn full_scale_is_clipped() {
        let mut bytes = [0; 8];
        encode(&[1.5, -1.5], 4, &mut bytes);
        assert_eq!(i32::from_le_bytes(bytes[..4].try_into().unwrap()), i32::MAX);
        assert_eq!(i32::from_le_bytes(bytes[4..].try_into().unwrap()), i32::MIN);
    }

    #[test]
    fn twenty_four_bits_in_four_bytes_are_the_high_ones() {
        let mut bytes = [0; 4];
        encode(&[0.5], 4, &mut bytes);
        assert_eq!(bytes, [0, 0, 0, 0x40]);
    }

    #[test]
    fn packets_carry_the_rate_on_average() {
        let mut sizer = PacketSizer::new(44_100, 8_000, 25);
        let frames: usize = (0..8_000).map(|_| sizer.next()).sum();
        assert_eq!(frames, 44_100);
    }

    #[test]
    fn the_card_paces_the_packets() {
        let mut sizer = PacketSizer::new(48_000, 8_000, 25);
        // 6.01 frames a microframe in 16.16
        sizer.feedback(&((6.01 * 65_536.0) as u32).to_le_bytes());
        let frames: usize = (0..1_000).map(|_| sizer.next()).sum();
        assert!((6_009..=6_010).contains(&frames), "{frames}");
    }

    #[test]
    fn feedback_far_from_the_rate_is_left_aside() {
        let mut sizer = PacketSizer::new(48_000, 8_000, 25);
        sizer.feedback(&(20 * 65_536u32).to_le_bytes());
        assert_eq!((0..8).map(|_| sizer.next()).sum::<usize>(), 48);
    }

    #[test]
    fn full_speed_feedback_is_per_millisecond() {
        let mut sizer = PacketSizer::new(48_000, 1_000, 60);
        // 48.5 frames a millisecond in 10.14
        sizer.feedback(&((48.5 * 16_384.0) as u32).to_le_bytes()[..3]);
        assert_eq!((0..2).map(|_| sizer.next()).sum::<usize>(), 97);
    }
}
