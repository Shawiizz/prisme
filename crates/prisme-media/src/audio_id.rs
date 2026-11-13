//! What a file sounds like, whatever it is called and whatever its tags say: an identity of
//! its audio, to know a file again once it was renamed, moved or tagged.

use crate::{open_format, MediaError};
use std::path::Path;
use symphonia::core::errors::Error as SymphoniaError;

/// Packets the identity is made of: about half a minute of a compressed file. Read without
/// decoding, in a few milliseconds.
const PACKETS: usize = 1024;

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// An identity of the audio of `path`, made of its first packets as they are stored (FNV-1a):
/// the same once the file is renamed, moved or tagged again, none of which touches them.
pub fn audio_id(path: &Path) -> Result<u64, MediaError> {
    let mut format = open_format(path)?.0;
    let track = format.default_track().ok_or(MediaError::NoAudio)?.id;
    let mut id = FNV_OFFSET;
    let mut packets = 0;
    while packets < PACKETS {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            // the end of the file, or of what can be read of a damaged one
            Err(SymphoniaError::IoError(_) | SymphoniaError::ResetRequired) => break,
            Err(e) => return Err(e.into()),
        };
        if packet.track_id() != track {
            continue;
        }
        for &byte in packet.buf() {
            id = (id ^ u64::from(byte)).wrapping_mul(FNV_PRIME);
        }
        packets += 1;
    }
    if packets == 0 {
        return Err(MediaError::NoAudio);
    }
    Ok(id)
}
