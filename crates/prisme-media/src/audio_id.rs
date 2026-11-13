//! What a file sounds like, whatever it is called and whatever its tags say: an identity of
//! its audio, to know a file again once it was renamed, moved or tagged.

use crate::{open_format, MediaError};
use std::io::ErrorKind;
use std::path::Path;
use symphonia::core::errors::Error as SymphoniaError;

/// Packets the identity is made of: about half a minute of a compressed file. Read without
/// decoding, in a few milliseconds.
const PACKETS: usize = 1024;

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Files that state their length exactly: it tells apart two edits that start alike, sample
/// for sample. (The length of an MP3 may be reckoned from the size of the file, tags
/// included.)
const EXACT_LENGTH: [&str; 4] = ["wav", "aif", "aiff", "flac"];

/// An identity of the audio of `path`, made of its first packets as they are stored (FNV-1a):
/// the same once the file is renamed, moved or tagged again, none of which touches them.
pub fn audio_id(path: &Path) -> Result<u64, MediaError> {
    let mut format = open_format(path)?.0;
    let (track, frames) = {
        let track = format.default_track().ok_or(MediaError::NoAudio)?;
        (track.id, track.codec_params.n_frames)
    };
    let exact = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXACT_LENGTH.contains(&e.to_ascii_lowercase().as_str()));
    let mut id = FNV_OFFSET;
    let mut hash = |bytes: &[u8]| {
        for &byte in bytes {
            id = (id ^ u64::from(byte)).wrapping_mul(FNV_PRIME);
        }
    };
    if let Some(frames) = frames.filter(|_| exact) {
        hash(&frames.to_le_bytes());
    }
    let mut packets = 0;
    while packets < PACKETS {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(e)) if e.kind() == ErrorKind::UnexpectedEof => break,
            Err(SymphoniaError::ResetRequired) => break,
            // a file read in part would be another file the next time
            Err(e) => return Err(e.into()),
        };
        if packet.track_id() != track {
            continue;
        }
        hash(packet.buf());
        packets += 1;
    }
    if packets == 0 {
        return Err(MediaError::NoAudio);
    }
    Ok(id)
}
