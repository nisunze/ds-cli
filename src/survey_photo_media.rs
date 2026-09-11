//! Bounded pixel processing for a locally captured or imported survey photo.
//!
//! The host owns capture, Blob/IndexedDB persistence and upload. This module
//! owns the deterministic decode, orientation, resize, rotation and encoding
//! that happen before the host persists those bytes. It accepts JPEG and PNG
//! only; the output stays in that same format. No photo bytes cross this API as
//! JSON or base64.

use std::io::Cursor;

use image::{
    codecs::{jpeg::JpegDecoder, png::PngDecoder},
    imageops::FilterType,
    metadata::Orientation,
    ColorType, DynamicImage, ImageBuffer, ImageDecoder, ImageEncoder, ImageFormat, Limits, Rgb,
};

pub const MAX_INPUT_BYTES: usize = 50 * 1024 * 1024;
pub const MAX_SOURCE_EDGE: u32 = 50_000;
pub const MAX_SOURCE_PIXELS: u64 = 50_000_000;
pub const MAX_DECODED_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_ORIGINAL_EDGE: u32 = 2_048;
pub const MAX_THUMBNAIL_EDGE: u32 = 320;
pub const JPEG_QUALITY: u8 = 85;
pub const UNSUPPORTED_IMAGE_CODE: &str = "SURVEY_PHOTO_UNSUPPORTED_IMAGE";
pub const MALFORMED_IMAGE_CODE: &str = "SURVEY_PHOTO_MALFORMED_IMAGE";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MediaDetection {
    ProcessableJpeg,
    ProcessablePng,
    UnsupportedImage,
    NotImage,
}

/// Classify only the leading format signature. Hosts use this before calling
/// [`process`] so a GIF/WebP or a non-image attachment remains its exact
/// original bytes rather than being silently flattened or rejected.
pub fn detect_media(input: &[u8]) -> MediaDetection {
    match image::guess_format(input) {
        Ok(ImageFormat::Jpeg) => MediaDetection::ProcessableJpeg,
        Ok(ImageFormat::Png) => MediaDetection::ProcessablePng,
        Ok(ImageFormat::Gif | ImageFormat::WebP | ImageFormat::Avif) => {
            MediaDetection::UnsupportedImage
        }
        _ => MediaDetection::NotImage,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Rotation {
    Deg0,
    Deg90,
    Deg180,
    Deg270,
}

impl TryFrom<u16> for Rotation {
    type Error = String;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Deg0),
            90 => Ok(Self::Deg90),
            180 => Ok(Self::Deg180),
            270 => Ok(Self::Deg270),
            _ => Err("Survey-photo rotation must be 0, 90, 180, or 270 degrees clockwise.".into()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Format {
    Jpeg,
    Png,
}

impl Format {
    fn media_type(self) -> &'static str {
        match self {
            Self::Jpeg => "image/jpeg",
            Self::Png => "image/png",
        }
    }
}

/// Encoded bytes and their independently useful metadata. `original_bytes`
/// means the normalized upload original, bounded to 2048px; it is not the
/// unprocessed camera file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PhotoBundle {
    pub media_type: &'static str,
    pub thumbnail_media_type: &'static str,
    pub original_bytes: Vec<u8>,
    pub thumbnail_bytes: Vec<u8>,
    pub original_width: u32,
    pub original_height: u32,
    pub thumbnail_width: u32,
    pub thumbnail_height: u32,
}

/// Decode one supported image and produce the normalized upload original plus
/// its 320px thumbnail. EXIF orientation is applied before the caller's net
/// clockwise turn, and is not retained in the freshly encoded result.
pub fn process(input: &[u8], rotation: Rotation) -> Result<PhotoBundle, String> {
    if input.is_empty() || input.len() > MAX_INPUT_BYTES {
        return Err(format!(
            "{MALFORMED_IMAGE_CODE}: survey-photo image must contain at most {MAX_INPUT_BYTES} bytes."
        ));
    }

    let (format, mut image, orientation) = decode(input)?;
    image.apply_orientation(orientation);
    let original = resize_to_edge(image, MAX_ORIGINAL_EDGE);
    let original = rotate(original, rotation);
    let thumbnail = resize_to_edge(original.clone(), MAX_THUMBNAIL_EDGE);

    let (original_width, original_height) = (original.width(), original.height());
    let (thumbnail_width, thumbnail_height) = (thumbnail.width(), thumbnail.height());
    Ok(PhotoBundle {
        media_type: format.media_type(),
        thumbnail_media_type: "image/jpeg",
        original_bytes: encode(&original, format)?,
        thumbnail_bytes: encode_thumbnail(&thumbnail)?,
        original_width,
        original_height,
        thumbnail_width,
        thumbnail_height,
    })
}

fn limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SOURCE_EDGE);
    limits.max_image_height = Some(MAX_SOURCE_EDGE);
    limits.max_alloc = Some(MAX_DECODED_BYTES);
    limits
}

fn decode(input: &[u8]) -> Result<(Format, DynamicImage, Orientation), String> {
    let guessed = image::guess_format(input);
    if matches!(detect_media(input), MediaDetection::UnsupportedImage) {
        return Err(format!(
            "{UNSUPPORTED_IMAGE_CODE}: preserve the original bytes without a thumbnail."
        ));
    }
    let guessed = guessed
        .map_err(|_| format!("{MALFORMED_IMAGE_CODE}: bytes are not a readable JPEG or PNG."))?;
    match guessed {
        ImageFormat::Jpeg => {
            let mut decoder = JpegDecoder::new(Cursor::new(input)).map_err(|error| {
                format!("{MALFORMED_IMAGE_CODE}: JPEG cannot be decoded: {error}")
            })?;
            decoder.set_limits(limits()).map_err(|error| {
                format!("{MALFORMED_IMAGE_CODE}: JPEG exceeds decode bounds: {error}")
            })?;
            check_source_pixels(decoder.dimensions())?;
            let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
            let image = DynamicImage::from_decoder(decoder).map_err(|error| {
                format!("{MALFORMED_IMAGE_CODE}: JPEG cannot be decoded: {error}")
            })?;
            Ok((Format::Jpeg, image, orientation))
        }
        ImageFormat::Png => {
            let mut decoder = PngDecoder::new(Cursor::new(input)).map_err(|error| {
                format!("{MALFORMED_IMAGE_CODE}: PNG cannot be decoded: {error}")
            })?;
            decoder.set_limits(limits()).map_err(|error| {
                format!("{MALFORMED_IMAGE_CODE}: PNG exceeds decode bounds: {error}")
            })?;
            check_source_pixels(decoder.dimensions())?;
            let image = DynamicImage::from_decoder(decoder).map_err(|error| {
                format!("{MALFORMED_IMAGE_CODE}: PNG cannot be decoded: {error}")
            })?;
            Ok((Format::Png, image, Orientation::NoTransforms))
        }
        _ => Err(format!(
            "{UNSUPPORTED_IMAGE_CODE}: format must be JPEG or PNG."
        )),
    }
}

fn check_source_pixels((width, height): (u32, u32)) -> Result<(), String> {
    if u64::from(width).saturating_mul(u64::from(height)) > MAX_SOURCE_PIXELS {
        return Err(format!(
            "Survey-photo image exceeds the {MAX_SOURCE_PIXELS}-pixel decode bound."
        ));
    }
    Ok(())
}

fn resize_to_edge(image: DynamicImage, edge: u32) -> DynamicImage {
    if image.width().max(image.height()) <= edge {
        image
    } else {
        image.resize(edge, edge, FilterType::Triangle)
    }
}

fn rotate(image: DynamicImage, rotation: Rotation) -> DynamicImage {
    match rotation {
        Rotation::Deg0 => image,
        Rotation::Deg90 => image.rotate90(),
        Rotation::Deg180 => image.rotate180(),
        Rotation::Deg270 => image.rotate270(),
    }
}

fn encode(image: &DynamicImage, format: Format) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    match format {
        Format::Jpeg => {
            let pixels = image.to_rgb8();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut output, JPEG_QUALITY)
                .write_image(
                    &pixels,
                    pixels.width(),
                    pixels.height(),
                    ColorType::Rgb8.into(),
                )
                .map_err(|error| format!("Survey-photo JPEG cannot be encoded: {error}"))?;
        }
        Format::Png => {
            let pixels = image.to_rgba8();
            image::codecs::png::PngEncoder::new(&mut output)
                .write_image(
                    &pixels,
                    pixels.width(),
                    pixels.height(),
                    ColorType::Rgba8.into(),
                )
                .map_err(|error| format!("Survey-photo PNG cannot be encoded: {error}"))?;
        }
    }
    Ok(output)
}

fn encode_thumbnail(image: &DynamicImage) -> Result<Vec<u8>, String> {
    // The companion is always the canonical JPEG. PNG alpha is composited over
    // white so an otherwise transparent field photo does not turn black.
    let rgba = image.to_rgba8();
    let rgb = ImageBuffer::from_fn(rgba.width(), rgba.height(), |x, y| {
        let [red, green, blue, alpha] = rgba.get_pixel(x, y).0;
        let over_white = |channel: u8| {
            ((u16::from(channel) * u16::from(alpha) + 255 * u16::from(255 - alpha)) / 255) as u8
        };
        Rgb([over_white(red), over_white(green), over_white(blue)])
    });
    let mut output = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut output, JPEG_QUALITY)
        .write_image(&rgb, rgb.width(), rgb.height(), ColorType::Rgb8.into())
        .map_err(|error| format!("Survey-photo thumbnail cannot be encoded: {error}"))?;
    Ok(output)
}

#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub struct SurveyPhotoBundleWasm {
    bundle: PhotoBundle,
}

#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen]
impl SurveyPhotoBundleWasm {
    #[wasm_bindgen::prelude::wasm_bindgen(getter, js_name = mediaType)]
    pub fn media_type(&self) -> String {
        self.bundle.media_type.into()
    }
    #[wasm_bindgen::prelude::wasm_bindgen(getter, js_name = originalWidth)]
    pub fn original_width(&self) -> u32 {
        self.bundle.original_width
    }
    #[wasm_bindgen::prelude::wasm_bindgen(getter, js_name = originalHeight)]
    pub fn original_height(&self) -> u32 {
        self.bundle.original_height
    }
    #[wasm_bindgen::prelude::wasm_bindgen(getter, js_name = thumbnailWidth)]
    pub fn thumbnail_width(&self) -> u32 {
        self.bundle.thumbnail_width
    }
    #[wasm_bindgen::prelude::wasm_bindgen(getter, js_name = thumbnailHeight)]
    pub fn thumbnail_height(&self) -> u32 {
        self.bundle.thumbnail_height
    }
    #[wasm_bindgen::prelude::wasm_bindgen(getter, js_name = thumbnailMediaType)]
    pub fn thumbnail_media_type(&self) -> String {
        self.bundle.thumbnail_media_type.into()
    }
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = originalBytes)]
    pub fn original_bytes(&self) -> Vec<u8> {
        self.bundle.original_bytes.clone()
    }
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = thumbnailBytes)]
    pub fn thumbnail_bytes(&self) -> Vec<u8> {
        self.bundle.thumbnail_bytes.clone()
    }
}

#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = surveyPhotoMediaDetect)]
pub fn survey_photo_media_detect_wasm(input: &[u8]) -> String {
    match detect_media(input) {
        MediaDetection::ProcessableJpeg => "jpeg",
        MediaDetection::ProcessablePng => "png",
        MediaDetection::UnsupportedImage => "unsupported_image",
        MediaDetection::NotImage => "not_image",
    }
    .into()
}

#[cfg(feature = "wasm")]
pub fn process_wasm(input: &[u8], rotation_degrees: u16) -> Result<SurveyPhotoBundleWasm, String> {
    Ok(SurveyPhotoBundleWasm {
        bundle: process(input, Rotation::try_from(rotation_degrees)?)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba};

    fn png(image: ImageBuffer<Rgba<u8>, Vec<u8>>) -> Vec<u8> {
        let mut bytes = Vec::new();
        image::codecs::png::PngEncoder::new(&mut bytes)
            .write_image(
                &image,
                image.width(),
                image.height(),
                ColorType::Rgba8.into(),
            )
            .unwrap();
        bytes
    }

    fn decode_png(bytes: &[u8]) -> ImageBuffer<Rgba<u8>, Vec<u8>> {
        image::load_from_memory_with_format(bytes, ImageFormat::Png)
            .unwrap()
            .to_rgba8()
    }

    #[test]
    fn png_rotation_moves_actual_pixels_and_inverse_restores_them() {
        let source = ImageBuffer::from_fn(3, 2, |x, y| match (x, y) {
            (0, 0) => Rgba([1, 0, 0, 255]),
            (1, 0) => Rgba([2, 0, 0, 255]),
            (2, 0) => Rgba([3, 0, 0, 255]),
            (0, 1) => Rgba([4, 0, 0, 255]),
            (1, 1) => Rgba([5, 0, 0, 255]),
            _ => Rgba([6, 0, 0, 255]),
        });
        let forward = process(&png(source.clone()), Rotation::Deg90).unwrap();
        assert_eq!((forward.original_width, forward.original_height), (2, 3));
        let turned = decode_png(&forward.original_bytes);
        assert_eq!(turned.get_pixel(1, 0), &Rgba([1, 0, 0, 255]));
        assert_eq!(turned.get_pixel(1, 2), &Rgba([3, 0, 0, 255]));
        assert_eq!(turned.get_pixel(0, 0), &Rgba([4, 0, 0, 255]));

        let inverse = process(&forward.original_bytes, Rotation::Deg270).unwrap();
        assert_eq!(decode_png(&inverse.original_bytes), source);
    }

    #[test]
    fn png_keeps_alpha_and_thumbnail_is_bounded() {
        let source = ImageBuffer::from_fn(640, 320, |x, y| Rgba([x as u8, y as u8, 30, 17]));
        let result = process(&png(source), Rotation::Deg0).unwrap();
        assert_eq!(result.media_type, "image/png");
        assert_eq!(result.thumbnail_media_type, "image/jpeg");
        assert_eq!((result.original_width, result.original_height), (640, 320));
        assert_eq!(
            (result.thumbnail_width, result.thumbnail_height),
            (320, 160)
        );
        assert_eq!(decode_png(&result.original_bytes).get_pixel(0, 10)[3], 17);
        let thumbnail =
            image::load_from_memory_with_format(&result.thumbnail_bytes, ImageFormat::Jpeg)
                .unwrap()
                .to_rgb8();
        assert!(
            thumbnail.get_pixel(0, 10)[0] > 230,
            "transparent pixel is composited over white"
        );
    }

    #[test]
    fn original_and_thumbnail_edges_are_capped() {
        let source = ImageBuffer::from_pixel(4096, 2048, Rgba([8, 9, 10, 255]));
        let result = process(&png(source), Rotation::Deg90).unwrap();
        assert_eq!(
            (result.original_width, result.original_height),
            (1024, 2048)
        );
        assert_eq!(
            (result.thumbnail_width, result.thumbnail_height),
            (160, 320)
        );
    }

    #[test]
    fn jpeg_exif_orientation_is_applied_before_the_requested_turn() {
        let mut jpeg = Vec::new();
        let pixels = ImageBuffer::from_pixel(3, 2, image::Rgb([20, 40, 60]));
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 100)
            .write_image(&pixels, 3, 2, ColorType::Rgb8.into())
            .unwrap();
        let mut tiff = b"II*\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0".to_vec();
        let mut exif = b"Exif\0\0".to_vec();
        exif.append(&mut tiff);
        let mut oriented = vec![0xff, 0xd8, 0xff, 0xe1];
        oriented.extend_from_slice(&u16::try_from(exif.len() + 2).unwrap().to_be_bytes());
        oriented.extend_from_slice(&exif);
        oriented.extend_from_slice(&jpeg[2..]);
        let result = process(&oriented, Rotation::Deg0).unwrap();
        assert_eq!((result.original_width, result.original_height), (2, 3));
        let result = process(&oriented, Rotation::Deg270).unwrap();
        assert_eq!((result.original_width, result.original_height), (3, 2));
    }

    #[test]
    fn unsupported_malformed_and_oversize_inputs_are_refused() {
        assert!(process(b"not an image", Rotation::Deg0)
            .unwrap_err()
            .contains(MALFORMED_IMAGE_CODE));
        assert!(process(b"\x89PNG\r\n\x1a\n", Rotation::Deg0).is_err());
        assert!(process(&vec![0; MAX_INPUT_BYTES + 1], Rotation::Deg0)
            .unwrap_err()
            .contains("at most"));
        assert!(Rotation::try_from(45).is_err());
    }

    #[test]
    fn sniff_separates_processable_unsupported_images_and_other_attachments() {
        assert_eq!(
            detect_media(&png(ImageBuffer::from_pixel(1, 1, Rgba([1, 2, 3, 4])))),
            MediaDetection::ProcessablePng
        );
        assert_eq!(
            detect_media(b"GIF89a\x01\0\x01\0"),
            MediaDetection::UnsupportedImage
        );
        assert_eq!(
            detect_media(b"RIFF\x00\x00\x00\x00WEBP"),
            MediaDetection::UnsupportedImage
        );
        assert_eq!(detect_media(b"a field-note.pdf"), MediaDetection::NotImage);
    }
}
