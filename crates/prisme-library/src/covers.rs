//! Cover art, kept as small JPEG thumbnails: enough for lists and deck headers, and a
//! few kilobytes per track in the database. A track whose file has none is given one drawn.

use image::imageops::FilterType;
use image::{ImageFormat, Rgb, RgbImage, Rgba, RgbaImage};
use prisme_media::TrackTags;
use std::io::Cursor;
use std::path::Path;

pub const THUMBNAIL_SIZE: u32 = 128;
const JPEG_QUALITY: u8 = 82;

/// A JPEG thumbnail of an embedded picture, or `None` if it cannot be decoded.
pub fn thumbnail(picture: &[u8]) -> Option<Vec<u8>> {
    let image = image::load_from_memory(picture).ok()?;
    jpeg(&image.resize(THUMBNAIL_SIZE, THUMBNAIL_SIZE, FilterType::Triangle).to_rgb8())
}

fn jpeg(image: &RgbImage) -> Option<Vec<u8>> {
    let mut jpeg = Vec::new();
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(Cursor::new(&mut jpeg), JPEG_QUALITY);
    image.write_with_encoder(encoder).ok()?;
    Some(jpeg)
}

/// A JPEG thumbnail drawn for the track at `path` whose file has no cover: the same for each
/// track of an album, else one per track. A gradient of a hue the track gives, under soft
/// shapes of the hues near it.
pub fn drawn(tags: &TrackTags, path: &Path) -> Vec<u8> {
    let artist = tags.artist.as_deref().unwrap_or_default();
    let work = tags.album.as_deref().filter(|album| !album.trim().is_empty());
    let work = work.or(tags.title.as_deref()).map(str::to_owned);
    let work = work.unwrap_or_else(|| path.file_stem().unwrap_or_default().to_string_lossy().into_owned());
    jpeg(&draw(&format!("{artist}\0{work}"))).expect("a JPEG is written to memory")
}

/// Drawn twice as large, then made smaller, for smooth edges.
const DRAWN: u32 = THUMBNAIL_SIZE * 2;

fn draw(key: &str) -> RgbImage {
    let mut random = Random::new(key);
    let size = DRAWN as f32;
    let hue = random.next();
    let mut shapes = RgbaImage::new(DRAWN, DRAWN);
    for _ in 0..random.between(3.0, 7.0) as usize {
        let [r, g, b] = hsv((hue + random.next() * 0.4).fract(), 0.5, 1.0);
        let alpha = random.between(0.24, 0.59);
        let (x, y) = (random.next() * size, random.next() * size);
        let reach = random.between(80.0, 320.0) / 600.0 * size;
        let round = random.next() < 0.5;
        for (px, py, pixel) in shapes.enumerate_pixels_mut() {
            let (px, py) = (px as f32 + 0.5, py as f32 + 0.5);
            let inside = if round {
                (px - x).hypot(py - y) <= reach
            } else {
                py <= y + reach && (px - x).abs() <= (py - (y - reach)) / 2.0
            };
            if inside {
                // over what is there, premultiplied so that the blur below keeps the colours
                let under = pixel.0.map(|c| f32::from(c) / 255.0);
                let over = [r * alpha, g * alpha, b * alpha, alpha];
                *pixel = Rgba(std::array::from_fn(|i| ((over[i] + under[i] * (1.0 - alpha)) * 255.0) as u8));
            }
        }
    }
    let softness = random.next() * 12.0 / 600.0 * size;
    let shapes = if softness >= 0.5 { image::imageops::blur(&shapes, softness) } else { shapes };
    let image = RgbImage::from_fn(DRAWN, DRAWN, |_, y| {
        let down = y as f32 / size;
        Rgb(hsv((hue + 0.18 * down).fract(), 0.65, 0.25 + 0.5 * down).map(|c| (c * 255.0) as u8))
    });
    let image = RgbImage::from_fn(DRAWN, DRAWN, |x, y| {
        let Rgba([r, g, b, a]) = *shapes.get_pixel(x, y);
        let under = image.get_pixel(x, y).0;
        let over = [r, g, b];
        Rgb(std::array::from_fn(|i| {
            over[i].saturating_add(((u16::from(under[i]) * u16::from(255 - a)) / 255) as u8)
        }))
    });
    image::imageops::resize(&image, THUMBNAIL_SIZE, THUMBNAIL_SIZE, FilterType::Triangle)
}

fn hsv(hue: f32, saturation: f32, value: f32) -> [f32; 3] {
    let channel = |n: f32| {
        let k = (n + hue * 6.0) % 6.0;
        value - value * saturation * k.min(4.0 - k).clamp(0.0, 1.0)
    };
    [channel(5.0), channel(3.0), channel(1.0)]
}

/// Numbers drawn from a text, always the same for it (xorshift seeded by FNV-1a).
struct Random(u64);

impl Random {
    fn new(text: &str) -> Self {
        let seed = text
            .bytes()
            .fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3));
        Self(seed | 1)
    }

    /// In [0, 1).
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }

    fn between(&mut self, low: f32, high: f32) -> f32 {
        low + self.next() * (high - low)
    }
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

    fn tags(title: &str, artist: &str, album: Option<&str>) -> TrackTags {
        TrackTags {
            title: Some(title.into()),
            artist: Some(artist.into()),
            album: album.map(Into::into),
            ..TrackTags::default()
        }
    }

    #[test]
    fn a_cover_drawn_is_a_thumbnail_the_same_for_an_album_its_own_for_a_single() {
        let path = Path::new("a.mp3");
        let drawn_for = |title, album| drawn(&tags(title, "Lumen Varga", album), path);
        let cover = image::load_from_memory(&drawn_for("Glass Meridian", Some("Meridian EP"))).unwrap();
        assert_eq!((cover.width(), cover.height()), (THUMBNAIL_SIZE, THUMBNAIL_SIZE));

        assert_eq!(
            drawn_for("Glass Meridian", Some("Meridian EP")),
            drawn_for("Afterglow Theory", Some("Meridian EP"))
        );
        assert_ne!(drawn_for("Glass Meridian", None), drawn_for("Afterglow Theory", None));
        assert_ne!(
            drawn_for("Glass Meridian", Some("Meridian EP")),
            drawn_for("Glass Meridian", Some("Other"))
        );
    }

    #[test]
    fn a_track_without_tags_has_a_cover_drawn_from_its_file_name() {
        let none = TrackTags::default();
        assert_eq!(
            drawn(&none, Path::new("x/Night Ferry.mp3")),
            drawn(&none, Path::new("y/Night Ferry.flac"))
        );
        assert_ne!(drawn(&none, Path::new("Night Ferry.mp3")), drawn(&none, Path::new("Copper Sun.mp3")));
    }

    #[test]
    fn garbage_gives_no_thumbnail() {
        assert!(!is_supported(b"not a picture"));
        assert_eq!(thumbnail(b"not a picture"), None);
    }
}
