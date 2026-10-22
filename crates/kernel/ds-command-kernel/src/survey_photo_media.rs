//! Bounded pixel work for a survey photo: its thumbnail, a rendered view and
//! its displayed dimensions — never a rewrite of the camera original.
//!
//! The camera original is kept exactly as captured (full resolution, EXIF and
//! all). A rotation is METADATA: the net clockwise quarter-turns an entry
//! records for the original (`survey_media_rotation`), applied after the EXIF
//! orientation whenever pixels are shown — here when the thumbnail or a
//! rendered view is made, and in a host's own transform when it draws the
//! original. That makes every rotation lossless and reversible.
//!
//! The host owns capture, storage and upload. This module owns the
//! deterministic decode, orientation, resize, rotation and encoding of the
//! derived images. It decodes JPEG and PNG only. No photo bytes cross this API
//! as JSON or base64.

use serde::Serialize;
use std::io::Cursor;

use image::{
    ColorType, DynamicImage, ImageBuffer, ImageDecoder, ImageEncoder, ImageFormat, Limits, Rgb,
    codecs::{jpeg::JpegDecoder, png::PngDecoder},
    metadata::Orientation,
};

/// The one admission limit for a survey media object, shared by this module,
/// the media outbox and ds-brain's upload grant (50 MiB declared size).
pub const MAX_INPUT_BYTES: usize = 50 * 1024 * 1024;
pub const MAX_SOURCE_EDGE: u32 = 50_000;
pub const MAX_SOURCE_PIXELS: u64 = 50_000_000;
pub const MAX_DECODED_BYTES: u64 = 256 * 1024 * 1024;
/// The longest edge of a rendered view (hosts that cannot transform pixels).
pub const MAX_RENDER_EDGE: u32 = 2_048;
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

/// Classify only the leading format signature: the one image classification
/// every host shares. JPEG and PNG are processed; GIF, WebP, AVIF, BMP, TIFF
/// and HEIC/HEIF are images kept as their exact bytes without a thumbnail;
/// anything else is an ordinary attachment.
pub fn detect_media(input: &[u8]) -> MediaDetection {
    if is_heif(input) {
        return MediaDetection::UnsupportedImage;
    }
    match image::guess_format(input) {
        Ok(ImageFormat::Jpeg) => MediaDetection::ProcessableJpeg,
        Ok(ImageFormat::Png) => MediaDetection::ProcessablePng,
        Ok(
            ImageFormat::Gif
            | ImageFormat::WebP
            | ImageFormat::Avif
            | ImageFormat::Bmp
            | ImageFormat::Tiff,
        ) => MediaDetection::UnsupportedImage,
        _ => MediaDetection::NotImage,
    }
}

/// An ISO-BMFF `ftyp` box naming a HEIF/HEIC brand (phones' default capture
/// format), which the image decoder does not recognise.
fn is_heif(input: &[u8]) -> bool {
    input.len() >= 12
        && &input[4..8] == b"ftyp"
        && matches!(
            &input[8..12],
            b"heic" | b"heix" | b"hevc" | b"hevx" | b"heim" | b"heis" | b"mif1" | b"msf1"
        )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Rotation {
    Deg0,
    Deg90,
    Deg180,
    Deg270,
}

impl Rotation {
    /// Net clockwise quarter-turns, 0..=3.
    pub const fn quarter_turns(self) -> u8 {
        match self {
            Self::Deg0 => 0,
            Self::Deg90 => 1,
            Self::Deg180 => 2,
            Self::Deg270 => 3,
        }
    }

    pub fn from_quarter_turns(turns: u8) -> Result<Self, String> {
        match turns {
            0 => Ok(Self::Deg0),
            1 => Ok(Self::Deg90),
            2 => Ok(Self::Deg180),
            3 => Ok(Self::Deg270),
            _ => Err("Survey-photo quarter-turns must be 0, 1, 2 or 3.".into()),
        }
    }
}

/// The same quarter-turn normalization for every host, including local drafts.
/// Half-quarter ties round clockwise, preserving the existing editor behavior.
pub fn normalize_rotation(degrees: f64) -> Result<u16, String> {
    if !degrees.is_finite() {
        return Err("Survey-photo rotation must be finite.".into());
    }
    Ok((((degrees.rem_euclid(360.0) / 90.0) + 0.5).floor() as u16 % 4) * 90)
}

pub fn accumulate_rotation(current: f64, direction: f64) -> Result<u16, String> {
    exact_rotation(current)?;
    if !matches!(direction, -90.0 | 90.0) {
        return Err("Survey-photo rotation direction must be -90 or 90 degrees.".into());
    }
    normalize_rotation(current + direction)
}

fn exact_rotation(degrees: f64) -> Result<Rotation, String> {
    match degrees {
        0.0 => Ok(Rotation::Deg0),
        90.0 => Ok(Rotation::Deg90),
        180.0 => Ok(Rotation::Deg180),
        270.0 => Ok(Rotation::Deg270),
        _ => Err("Survey-photo rotation must be 0, 90, 180, or 270 degrees clockwise.".into()),
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MediaPlan {
    pub detection: &'static str,
    pub action: &'static str,
    pub rotation: u16,
}

/// Decide thumbnailing versus exact-byte preservation from bytes, never a
/// host MIME label. `process` makes a thumbnail; the original stays its exact
/// bytes in every case. A rotation of an image this module cannot decode is
/// refused here in native and WASM alike.
pub fn plan_media(input: &[u8], degrees: f64) -> Result<MediaPlan, String> {
    let rotation = exact_rotation(degrees)?;
    let (detection, action) = match detect_media(input) {
        MediaDetection::ProcessableJpeg => ("jpeg", "process"),
        MediaDetection::ProcessablePng => ("png", "process"),
        MediaDetection::UnsupportedImage => ("unsupported_image", "preserve"),
        MediaDetection::NotImage => ("not_image", "preserve"),
    };
    if action == "preserve" && rotation != Rotation::Deg0 {
        return Err("Survey photo rotation is available only for JPEG and PNG images.".into());
    }
    Ok(MediaPlan {
        detection,
        action,
        rotation: degrees as u16,
    })
}

#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = surveyPhotoNormalizeRotation)]
pub fn normalize_rotation_wasm(degrees: f64) -> Result<u16, wasm_bindgen::JsValue> {
    normalize_rotation(degrees).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}

#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = surveyPhotoAccumulateRotation)]
pub fn accumulate_rotation_wasm(
    current: f64,
    direction: f64,
) -> Result<u16, wasm_bindgen::JsValue> {
    accumulate_rotation(current, direction).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}

#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = surveyPhotoMediaPlan)]
pub fn plan_media_wasm(input: &[u8], degrees: f64) -> Result<String, wasm_bindgen::JsValue> {
    plan_media(input, degrees)
        .and_then(|plan| serde_json::to_string(&plan).map_err(|error| error.to_string()))
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error))
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

/// One derived image: encoded bytes and the pixel size they show.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Encoded {
    pub media_type: &'static str,
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// The capture bundle: the camera original unchanged, its displayed size and
/// its thumbnail. `original_bytes` IS the input — never re-encoded.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PhotoBundle {
    pub media_type: &'static str,
    pub thumbnail_media_type: &'static str,
    pub original_bytes: Vec<u8>,
    pub thumbnail_bytes: Vec<u8>,
    /// The original as displayed: EXIF orientation then the rotation.
    pub original_width: u32,
    pub original_height: u32,
    pub thumbnail_width: u32,
    pub thumbnail_height: u32,
}

/// Capture/import: keep the original bytes exactly and make the thumbnail
/// showing `rotation` (applied after EXIF orientation).
pub fn process(input: &[u8], rotation: Rotation) -> Result<PhotoBundle, String> {
    let (format, decoder_orientation, (width, height)) = header(input)?;
    let thumbnail = thumbnail(input, rotation)?;
    let (original_width, original_height) =
        displayed((width, height), decoder_orientation, rotation);
    Ok(PhotoBundle {
        media_type: format.media_type(),
        thumbnail_media_type: thumbnail.media_type,
        original_bytes: input.to_vec(),
        thumbnail_bytes: thumbnail.bytes,
        original_width,
        original_height,
        thumbnail_width: thumbnail.width,
        thumbnail_height: thumbnail.height,
    })
}

/// The thumbnail alone: a white-composited JPEG at most 320 px on its longest
/// edge, showing the EXIF orientation and then `rotation`. No larger image is
/// encoded on the way.
pub fn thumbnail(input: &[u8], rotation: Rotation) -> Result<Encoded, String> {
    let image = derive(input, rotation, MAX_THUMBNAIL_EDGE)?.1;
    Ok(Encoded {
        media_type: "image/jpeg",
        width: image.width(),
        height: image.height(),
        bytes: encode_thumbnail(&image)?,
    })
}

/// A rendered view for a host that cannot transform pixels itself (a report,
/// a local file): the same format, at most `max_edge` px, orientation and
/// rotation applied. It is a derived copy, never stored over the original.
pub fn render(input: &[u8], rotation: Rotation, max_edge: u32) -> Result<Encoded, String> {
    let max_edge = max_edge.clamp(1, MAX_RENDER_EDGE);
    let (format, image) = derive(input, rotation, max_edge)?;
    Ok(Encoded {
        media_type: format.media_type(),
        width: image.width(),
        height: image.height(),
        bytes: encode(&image, format)?,
    })
}

/// The original's displayed size after EXIF orientation and the requested net
/// turn. Read headers only: do not decode pixels, resize, encode or change bytes.
pub fn display_dimensions(input: &[u8], rotation: Rotation) -> Result<(u32, u32), String> {
    if input.is_empty() || input.len() > MAX_INPUT_BYTES {
        return Err(format!(
            "{MALFORMED_IMAGE_CODE}: survey-photo image must contain at most {MAX_INPUT_BYTES} bytes."
        ));
    }
    let (size, orientation) = match detect_media(input) {
        MediaDetection::ProcessableJpeg => {
            let mut decoder = JpegDecoder::new(Cursor::new(input)).map_err(|error| {
                format!("{MALFORMED_IMAGE_CODE}: JPEG header cannot be read: {error}")
            })?;
            let orientation = decoder.orientation().map_err(|error| {
                format!("{MALFORMED_IMAGE_CODE}: JPEG orientation cannot be read: {error}")
            })?;
            (decoder.dimensions(), orientation)
        }
        MediaDetection::ProcessablePng => {
            let mut decoder =
                PngDecoder::with_limits(Cursor::new(input), limits()).map_err(|error| {
                    format!("{MALFORMED_IMAGE_CODE}: PNG header cannot be read: {error}")
                })?;
            let orientation = decoder.orientation().map_err(|error| {
                format!("{MALFORMED_IMAGE_CODE}: PNG orientation cannot be read: {error}")
            })?;
            (decoder.dimensions(), orientation)
        }
        MediaDetection::UnsupportedImage => {
            return Err(format!(
                "{UNSUPPORTED_IMAGE_CODE}: preserve the original bytes without a thumbnail."
            ));
        }
        MediaDetection::NotImage => {
            return Err(format!(
                "{MALFORMED_IMAGE_CODE}: bytes are not a readable JPEG or PNG."
            ));
        }
    };
    let exif_swaps = matches!(
        orientation,
        Orientation::Rotate90
            | Orientation::Rotate270
            | Orientation::Rotate90FlipH
            | Orientation::Rotate270FlipH
    );
    let turn_swaps = matches!(rotation, Rotation::Deg90 | Rotation::Deg270);
    if exif_swaps != turn_swaps {
        Ok((size.1, size.0))
    } else {
        Ok(size)
    }
}

fn displayed(
    (width, height): (u32, u32),
    orientation: Orientation,
    rotation: Rotation,
) -> (u32, u32) {
    let exif_swaps = matches!(
        orientation,
        Orientation::Rotate90
            | Orientation::Rotate270
            | Orientation::Rotate90FlipH
            | Orientation::Rotate270FlipH
    );
    let turn_swaps = matches!(rotation, Rotation::Deg90 | Rotation::Deg270);
    if exif_swaps != turn_swaps {
        (height, width)
    } else {
        (width, height)
    }
}

fn limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SOURCE_EDGE);
    limits.max_image_height = Some(MAX_SOURCE_EDGE);
    limits.max_alloc = Some(MAX_DECODED_BYTES);
    limits
}

fn admitted(input: &[u8]) -> Result<Format, String> {
    if input.is_empty() || input.len() > MAX_INPUT_BYTES {
        return Err(format!(
            "{MALFORMED_IMAGE_CODE}: survey-photo image must contain at most {MAX_INPUT_BYTES} bytes."
        ));
    }
    match detect_media(input) {
        MediaDetection::ProcessableJpeg => Ok(Format::Jpeg),
        MediaDetection::ProcessablePng => Ok(Format::Png),
        MediaDetection::UnsupportedImage => Err(format!(
            "{UNSUPPORTED_IMAGE_CODE}: preserve the original bytes without a thumbnail."
        )),
        MediaDetection::NotImage => Err(format!(
            "{MALFORMED_IMAGE_CODE}: bytes are not a readable JPEG or PNG."
        )),
    }
}

/// MIME type admitted by the shared byte-format owner, independent of host labels.
pub fn source_media_type(input: &[u8]) -> Result<&'static str, String> {
    admitted(input).map(Format::media_type)
}

fn malformed(kind: &str, what: &str, error: impl std::fmt::Display) -> String {
    format!("{MALFORMED_IMAGE_CODE}: {kind} {what}: {error}")
}

/// Format, orientation and stored size from the header; no pixels.
fn header(input: &[u8]) -> Result<(Format, Orientation, (u32, u32)), String> {
    match admitted(input)? {
        Format::Jpeg => {
            let mut decoder = JpegDecoder::new(Cursor::new(input))
                .map_err(|e| malformed("JPEG", "cannot be decoded", e))?;
            let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
            Ok((Format::Jpeg, orientation, decoder.dimensions()))
        }
        Format::Png => {
            let mut decoder = PngDecoder::new(Cursor::new(input))
                .map_err(|e| malformed("PNG", "cannot be decoded", e))?;
            let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
            Ok((Format::Png, orientation, decoder.dimensions()))
        }
    }
}

/// Decode once, shrink to `edge` FIRST, then orient and turn the small image:
/// orientation never allocates a second full-resolution buffer.
fn derive(input: &[u8], rotation: Rotation, edge: u32) -> Result<(Format, DynamicImage), String> {
    let (format, image, orientation) = decode(input)?;
    let mut image = shrink(image, edge);
    image.apply_orientation(orientation);
    Ok((format, rotate(image, rotation)))
}

fn decode(input: &[u8]) -> Result<(Format, DynamicImage, Orientation), String> {
    match admitted(input)? {
        Format::Jpeg => {
            let mut decoder = JpegDecoder::new(Cursor::new(input))
                .map_err(|e| malformed("JPEG", "cannot be decoded", e))?;
            decoder
                .set_limits(limits())
                .map_err(|e| malformed("JPEG", "exceeds decode bounds", e))?;
            check_source_pixels(decoder.dimensions())?;
            let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
            let image = DynamicImage::from_decoder(decoder)
                .map_err(|e| malformed("JPEG", "cannot be decoded", e))?;
            Ok((Format::Jpeg, image, orientation))
        }
        Format::Png => {
            let mut decoder = PngDecoder::new(Cursor::new(input))
                .map_err(|e| malformed("PNG", "cannot be decoded", e))?;
            decoder
                .set_limits(limits())
                .map_err(|e| malformed("PNG", "exceeds decode bounds", e))?;
            check_source_pixels(decoder.dimensions())?;
            let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
            let image = DynamicImage::from_decoder(decoder)
                .map_err(|e| malformed("PNG", "cannot be decoded", e))?;
            Ok((Format::Png, image, orientation))
        }
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

/// Area-averaging downscale: no full-width floating-point intermediate.
fn shrink(image: DynamicImage, edge: u32) -> DynamicImage {
    if image.width().max(image.height()) <= edge {
        image
    } else {
        image.thumbnail(edge, edge)
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
pub fn process_wasm(input: &[u8], rotation_degrees: u16) -> Result<SurveyPhotoBundleWasm, String> {
    Ok(SurveyPhotoBundleWasm {
        bundle: process(input, Rotation::try_from(rotation_degrees)?)?,
    })
}

/// One derived image for a browser host: bytes plus the size they show.
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub struct SurveyPhotoImageWasm {
    image: Encoded,
}

#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen]
impl SurveyPhotoImageWasm {
    #[wasm_bindgen::prelude::wasm_bindgen(getter, js_name = mediaType)]
    pub fn media_type(&self) -> String {
        self.image.media_type.into()
    }
    #[wasm_bindgen::prelude::wasm_bindgen(getter)]
    pub fn width(&self) -> u32 {
        self.image.width
    }
    #[wasm_bindgen::prelude::wasm_bindgen(getter)]
    pub fn height(&self) -> u32 {
        self.image.height
    }
    #[wasm_bindgen::prelude::wasm_bindgen]
    pub fn bytes(&self) -> Vec<u8> {
        self.image.bytes.clone()
    }
}

/// `surveyPhotoThumbnail(bytes, quarterTurns)`: the thumbnail-only entry
/// point (attachments of Project Work, messaging and notes included).
#[cfg(feature = "wasm")]
pub fn thumbnail_wasm(input: &[u8], quarter_turns: u8) -> Result<SurveyPhotoImageWasm, String> {
    Ok(SurveyPhotoImageWasm {
        image: thumbnail(input, Rotation::from_quarter_turns(quarter_turns)?)?,
    })
}

/// `surveyPhotoRender(bytes, quarterTurns, maxEdge)`: a rendered view.
#[cfg(feature = "wasm")]
pub fn render_wasm(
    input: &[u8],
    quarter_turns: u8,
    max_edge: u32,
) -> Result<SurveyPhotoImageWasm, String> {
    Ok(SurveyPhotoImageWasm {
        image: render(
            input,
            Rotation::from_quarter_turns(quarter_turns)?,
            max_edge,
        )?,
    })
}

#[cfg(test)]
#[path = "survey_photo_media_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "survey_photo_media_dimensions_tests.rs"]
mod dimensions_tests;
