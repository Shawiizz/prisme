//! Tags and cover art of audio files.

use crate::MediaError;
use lofty::file::TaggedFileExt;
use lofty::prelude::Accessor;
use lofty::tag::Tag;
use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrackTags {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub genre: Option<String>,
    pub year: Option<u16>,
    pub comment: Option<String>,
}

/// An embedded picture, as stored in the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverArt {
    pub data: Vec<u8>,
}

/// Tags of `path` (empty when the file has none) and its first picture.
pub fn read_tags(path: &Path) -> Result<(TrackTags, Option<CoverArt>), MediaError> {
    let file = lofty::read_from_path(path)
        .map_err(|e| MediaError::Tags { path: path.display().to_string(), message: e.to_string() })?;
    let Some(tag) = file.primary_tag().or_else(|| file.first_tag()) else {
        return Ok((TrackTags::default(), None));
    };
    Ok((tags_of(tag), tag.pictures().first().map(|p| CoverArt { data: p.data().to_vec() })))
}

fn tags_of(tag: &Tag) -> TrackTags {
    let text = |value: Option<std::borrow::Cow<'_, str>>| {
        value.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
    };
    TrackTags {
        title: text(tag.title()),
        artist: text(tag.artist()),
        album: text(tag.album()),
        genre: text(tag.genre()),
        year: tag.date().map(|d| d.year),
        comment: text(tag.comment()),
    }
}
