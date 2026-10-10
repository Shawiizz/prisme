//! Cover art, kept as small JPEG thumbnails: enough for lists and deck headers, and a
//! few kilobytes per track in the database.

use image::imageops::FilterType;
use image::ImageFormat;
use std::io::Cursor;

pub const THUMBNAIL_SIZE: u32 = 128;
const JPEG_QUALITY: u8 = 82;

/// A JPEG thumbnail of an embedded picture, or `None` if it cannot be decoded.
pub fn thumbnail(picture: &[u8]) -> Option<Vec<u8>> {
    let image = image::load_from_memory(picture).ok()?;
    let small = image.resize(THUMBNAIL_SIZE, THUMBNAIL_SIZE, FilterType::Triangle).to_rgb8();
    let mut jpeg = Vec::new();
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(Cursor::new(&mut jpeg), JPEG_QUALITY);
    small.write_with_encoder(encoder).ok()?;
    Some(jpeg)
}

/// Whether `data` is a picture format thumbnails can be made from.
pub fn is_supported(data: &[u8]) -> bool {
    matches!(image::guess_format(data), Ok(ImageFormat::Jpeg | ImageFormat::Png))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_large_picture_becomes_a_small_jpeg() {
        let picture =
            image::RgbImage::from_fn(600, 400, |x, y| image::Rgb([(x % 256) as u8, (y % 256) as u8, 90]));
        let mut png = Vec::new();
        picture.write_to(&mut Cursor::new(&mut png), ImageFormat::Png).unwrap();
        assert!(is_supported(&png));

        let jpeg = thumbnail(&png).expect("a thumbnail");
        let small = image::load_from_memory(&jpeg).unwrap();
        assert_eq!((small.width(), small.height()), (128, 85));
        assert!(jpeg.len() < png.len());
    }

    #[test]
    fn garbage_gives_no_thumbnail() {
        assert!(!is_supported(b"not a picture"));
        assert_eq!(thumbnail(b"not a picture"), None);
    }
}
