use super::*;
use image::{ImageBuffer, Rgba};

#[test]
fn quarter_turn_drafts_wrap_and_preserve_clockwise_ties() {
    for (input, expected) in [
        (-450.0, 270),
        (-90.0, 270),
        (-45.0, 0),
        (0.0, 0),
        (44.9, 0),
        (45.0, 90),
        (315.0, 0),
        (450.0, 90),
    ] {
        assert_eq!(normalize_rotation(input).unwrap(), expected);
    }
    assert_eq!(accumulate_rotation(270.0, 90.0).unwrap(), 0);
    assert_eq!(accumulate_rotation(0.0, -90.0).unwrap(), 270);
    for degrees in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(normalize_rotation(degrees).is_err());
    }
    assert!(accumulate_rotation(45.0, 90.0).is_err());
    assert!(accumulate_rotation(0.0, 180.0).is_err());
    for turns in 0..4 {
        assert_eq!(
            Rotation::from_quarter_turns(turns).unwrap().quarter_turns(),
            turns
        );
    }
    assert!(Rotation::from_quarter_turns(4).is_err());
}

#[test]
fn attachment_preservation_and_rotation_refusal_are_kernel_decisions() {
    for (bytes, detection) in [
        (b"GIF89a".as_slice(), "unsupported_image"),
        (b"document".as_slice(), "not_image"),
    ] {
        assert_eq!(
            plan_media(bytes, 0.0).unwrap(),
            MediaPlan {
                detection,
                action: "preserve",
                rotation: 0
            }
        );
        assert!(
            plan_media(bytes, 90.0)
                .unwrap_err()
                .contains("only for JPEG and PNG")
        );
    }
    for degrees in [-90.0, 45.0, 360.0, 65536.0, f64::NAN, f64::INFINITY] {
        assert!(plan_media(b"GIF89a", degrees).is_err());
    }
    let png_signature = b"\x89PNG\r\n\x1a\n";
    assert_eq!(
        plan_media(png_signature, 90.0).unwrap(),
        MediaPlan {
            detection: "png",
            action: "process",
            rotation: 90
        }
    );
}

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

fn six_pixels() -> ImageBuffer<Rgba<u8>, Vec<u8>> {
    ImageBuffer::from_fn(3, 2, |x, y| Rgba([(1 + x + 3 * y) as u8, 0, 0, 255]))
}

/// A JPEG whose EXIF orientation tag is `tag` (6 = turn 90° clockwise).
fn exif_jpeg(width: u32, height: u32, tag: u8) -> Vec<u8> {
    let mut jpeg = Vec::new();
    let pixels = ImageBuffer::from_pixel(width, height, image::Rgb([20, 40, 60]));
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 100)
        .write_image(&pixels, width, height, ColorType::Rgb8.into())
        .unwrap();
    let mut tiff = b"II*\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\0\0\0\0\0\0\0\0".to_vec();
    tiff[18] = tag;
    let mut exif = b"Exif\0\0".to_vec();
    exif.append(&mut tiff);
    let mut oriented = vec![0xff, 0xd8, 0xff, 0xe1];
    oriented.extend_from_slice(&u16::try_from(exif.len() + 2).unwrap().to_be_bytes());
    oriented.extend_from_slice(&exif);
    oriented.extend_from_slice(&jpeg[2..]);
    oriented
}

#[test]
fn capture_keeps_the_camera_original_byte_identical() {
    let source = exif_jpeg(30, 20, 6);
    for rotation in [
        Rotation::Deg0,
        Rotation::Deg90,
        Rotation::Deg180,
        Rotation::Deg270,
    ] {
        let bundle = process(&source, rotation).unwrap();
        assert_eq!(
            bundle.original_bytes, source,
            "the original is never re-encoded"
        );
        assert_eq!(bundle.media_type, "image/jpeg");
    }
    // EXIF 6 shows 20x30; a further quarter-turn shows 30x20.
    assert_eq!(
        (
            process(&source, Rotation::Deg0).unwrap().original_width,
            process(&source, Rotation::Deg0).unwrap().original_height
        ),
        (20, 30)
    );
    let turned = process(&source, Rotation::Deg90).unwrap();
    assert_eq!((turned.original_width, turned.original_height), (30, 20));
    assert_eq!((turned.thumbnail_width, turned.thumbnail_height), (30, 20));
}

#[test]
fn rotation_moves_thumbnail_pixels_and_four_turns_return_to_the_start() {
    let source = png(six_pixels());
    let render_of = |turns: u8| {
        decode_png(
            &render(
                &source,
                Rotation::from_quarter_turns(turns % 4).unwrap(),
                MAX_RENDER_EDGE,
            )
            .unwrap()
            .bytes,
        )
    };
    let turned = render_of(1);
    assert_eq!(turned.dimensions(), (2, 3));
    assert_eq!(turned.get_pixel(1, 0), &Rgba([1, 0, 0, 255]));
    assert_eq!(turned.get_pixel(1, 2), &Rgba([3, 0, 0, 255]));
    assert_eq!(turned.get_pixel(0, 0), &Rgba([4, 0, 0, 255]));
    assert_eq!(
        render_of(4),
        six_pixels(),
        "four quarter-turns are identity"
    );
    let thumb = thumbnail(&source, Rotation::Deg90).unwrap();
    assert_eq!((thumb.width, thumb.height), (2, 3));
    assert_eq!(thumb.media_type, "image/jpeg");
}

#[test]
fn png_thumbnail_is_white_composited_and_bounded() {
    let source = ImageBuffer::from_fn(640, 320, |x, y| Rgba([x as u8, y as u8, 30, 17]));
    let result = process(&png(source), Rotation::Deg0).unwrap();
    assert_eq!(result.media_type, "image/png");
    assert_eq!(result.thumbnail_media_type, "image/jpeg");
    assert_eq!((result.original_width, result.original_height), (640, 320));
    assert_eq!(
        (result.thumbnail_width, result.thumbnail_height),
        (320, 160)
    );
    let thumbnail = image::load_from_memory_with_format(&result.thumbnail_bytes, ImageFormat::Jpeg)
        .unwrap()
        .to_rgb8();
    assert!(
        thumbnail.get_pixel(0, 10)[0] > 230,
        "transparent pixel is composited over white"
    );
}

#[test]
fn derived_edges_are_capped_and_the_thumbnail_encodes_nothing_larger() {
    let source = png(ImageBuffer::from_pixel(4096, 2048, Rgba([8, 9, 10, 255])));
    let thumb = thumbnail(&source, Rotation::Deg90).unwrap();
    assert_eq!((thumb.width, thumb.height), (160, 320));
    let view = render(&source, Rotation::Deg90, 4096).unwrap();
    assert_eq!((view.width, view.height), (1024, 2048));
    assert_eq!(
        display_dimensions(&source, Rotation::Deg90).unwrap(),
        (2048, 4096)
    );
}

#[test]
fn jpeg_exif_orientation_is_applied_before_the_requested_turn() {
    let oriented = exif_jpeg(3, 2, 6);
    assert_eq!(
        display_dimensions(&oriented, Rotation::Deg0).unwrap(),
        (2, 3)
    );
    let view = render(&oriented, Rotation::Deg0, MAX_RENDER_EDGE).unwrap();
    assert_eq!((view.width, view.height), (2, 3));
    let view = render(&oriented, Rotation::Deg270, MAX_RENDER_EDGE).unwrap();
    assert_eq!((view.width, view.height), (3, 2));
}

#[test]
fn unsupported_malformed_and_oversize_inputs_are_refused() {
    assert!(
        thumbnail(b"not an image", Rotation::Deg0)
            .unwrap_err()
            .contains(MALFORMED_IMAGE_CODE)
    );
    assert!(thumbnail(b"\x89PNG\r\n\x1a\n", Rotation::Deg0).is_err());
    assert!(
        thumbnail(&vec![0; MAX_INPUT_BYTES + 1], Rotation::Deg0)
            .unwrap_err()
            .contains("at most")
    );
    assert!(
        thumbnail(b"GIF89a\x01\0\x01\0", Rotation::Deg0)
            .unwrap_err()
            .contains(UNSUPPORTED_IMAGE_CODE)
    );
    assert!(Rotation::try_from(45).is_err());
}

#[test]
fn one_classification_for_heic_bmp_tiff_and_web_formats() {
    assert_eq!(
        detect_media(&png(ImageBuffer::from_pixel(1, 1, Rgba([1, 2, 3, 4])))),
        MediaDetection::ProcessablePng
    );
    for unsupported in [
        b"GIF89a\x01\0\x01\0".as_slice(),
        b"RIFF\x00\x00\x00\x00WEBP",
        b"BM\x3a\0\0\0\0\0\0\0\x36\0\0\0",
        b"II*\0\x08\0\0\0",
        b"MM\0*\0\0\0\x08",
        b"\0\0\0\x18ftypheic\0\0\0\0",
        b"\0\0\0\x18ftypmif1\0\0\0\0",
    ] {
        assert_eq!(
            detect_media(unsupported),
            MediaDetection::UnsupportedImage,
            "{unsupported:?}"
        );
    }
    assert_eq!(detect_media(b"a field-note.pdf"), MediaDetection::NotImage);
    assert_eq!(
        detect_media(b"\0\0\0\x18ftypisom\0\0\0\0"),
        MediaDetection::NotImage,
        "an MP4 is not an image"
    );
}
