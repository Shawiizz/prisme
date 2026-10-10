//! What the library keeps from a file's tags.

use crate::covers;
use prisme_media::TrackTags;
use std::path::Path;

pub struct Metadata {
    pub tags: TrackTags,
    /// JPEG thumbnail of the embedded cover.
    pub cover: Option<Vec<u8>>,
}

/// Tags and cover thumbnail of `path`; unreadable tags count as none (the track still
/// plays).
pub fn read_metadata(path: &Path) -> Metadata {
    match prisme_media::read_tags(path) {
        Ok((tags, art)) => Metadata { tags, cover: art.and_then(|a| covers::thumbnail(&a.data)) },
        Err(_) => Metadata { tags: TrackTags::default(), cover: None },
    }
}
