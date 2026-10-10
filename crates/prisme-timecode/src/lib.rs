//! Timecode vinyl (DVS) decoding.
//!
//! A control record carries a quadrature carrier with one bit of a known sequence per
//! cycle. [`TimecodeDecoder`] turns the turntable signal into a speed (pitch and
//! direction) and an absolute position on the record.
//!
//! ```no_run
//! use prisme_timecode::{DecoderConfig, TimecodeDecoder, SERATO_CV02_A};
//!
//! let mut decoder = TimecodeDecoder::new(DecoderConfig {
//!     format: &SERATO_CV02_A,
//!     sample_rate: 48_000,
//!     swap_channels: false,
//! });
//! let input: Vec<f32> = vec![0.0; 2 * 256]; // interleaved stereo from the sound card
//! decoder.process(&input);
//! let status = decoder.status();
//! println!("speed {:.3}, position {:?} ms", status.speed, status.position_ms);
//! ```
//!
//! Modules, from the signal to the result:
//! * `conditioning`: DC removal, envelopes, click rejection;
//! * `level`: needle bounces and the level / speed model of the cartridge;
//! * `crossings`: zero crossings and their validation;
//! * `speed`: speed from the crossings;
//! * `position`: bits and absolute position;
//! * `decoder`: puts it all together.

mod conditioning;
mod crossings;
mod decoder;
mod format;
mod level;
mod position;
mod speed;
pub mod testing;

pub use decoder::{DecoderConfig, DecoderStatus, SignalState, TimecodeDecoder};
pub use format::{Channel, SequenceTable, TimecodeFormat, FORMATS, SERATO_CD, SERATO_CV02_A, SERATO_CV02_B};
