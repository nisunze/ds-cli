use super::*;
use image::{ImageBuffer, ImageEncoder};

fn oriented_image(format: Format, orientation: u8) -> Vec<u8> {
    let pixels = ImageBuffer::from_pixel(30, 20, Rgb([20u8, 40, 60]));
    let mut tiff = b"II*\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x01\0\0\0\0\0\0\0".to_vec();
    tiff[18] = orientation;
    let mut encoded = Vec::new();
    match format {
        Format::Jpeg => {
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, 90)
                .write_image(&pixels, 30, 20, ColorType::Rgb8.into())
                .unwrap();
            let mut exif = b"Exif\0\0".to_vec();
            exif.extend_from_slice(&tiff);
            let mut output = vec![0xff, 0xd8, 0xff, 0xe1];
            output.extend_from_slice(&u16::try_from(exif.len() + 2).unwrap().to_be_bytes());
            output.extend_from_slice(&exif);
            output.extend_from_slice(&encoded[2..]);
            output
        }
        Format::Png => {
            let mut encoder = image::codecs::png::PngEncoder::new(&mut encoded);
            encoder.set_exif_metadata(tiff).unwrap();
            encoder
                .write_image(&pixels, 30, 20, ColorType::Rgb8.into())
                .unwrap();
            encoded
        }
    }
}

#[test]
fn jpeg_and_png_dimensions_apply_all_exif_orientations_then_net_turn_without_rewriting() {
    for format in [Format::Jpeg, Format::Png] {
        for orientation in 1..=8 {
            let original = oriented_image(format, orientation);
            let unchanged = original.clone();
            for (rotation, expected) in [
                (
                    Rotation::Deg0,
                    if orientation >= 5 { (20, 30) } else { (30, 20) },
                ),
                (
                    Rotation::Deg90,
                    if orientation >= 5 { (30, 20) } else { (20, 30) },
                ),
                (
                    Rotation::Deg180,
                    if orientation >= 5 { (20, 30) } else { (30, 20) },
                ),
                (
                    Rotation::Deg270,
                    if orientation >= 5 { (30, 20) } else { (20, 30) },
                ),
            ] {
                assert_eq!(
                    display_dimensions(&original, rotation).unwrap(),
                    expected,
                    "{format:?} EXIF {orientation} {rotation:?}"
                );
            }
            assert_eq!(original, unchanged);
        }
    }
}

#[test]
fn dimensions_do_not_decode_pixel_data_or_resize_the_original() {
    let mut header_only = oriented_image(Format::Jpeg, 6);
    let scan = header_only
        .windows(2)
        .position(|bytes| bytes == [0xff, 0xda])
        .unwrap();
    let scan_header_length =
        u16::from_be_bytes([header_only[scan + 2], header_only[scan + 3]]) as usize;
    header_only.truncate(scan + 2 + scan_header_length);
    header_only.extend_from_slice(&[0xff, 0xd9]);
    // No entropy-coded pixels remain; the dimension read only needs headers.
    assert_eq!(
        display_dimensions(&header_only, Rotation::Deg0).unwrap(),
        (20, 30)
    );

    let pixels = ImageBuffer::from_pixel(2300, 1, Rgb([20u8, 40, 60]));
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(&pixels, 2300, 1, ColorType::Rgb8.into())
        .unwrap();
    assert_eq!(display_dimensions(&png, Rotation::Deg0).unwrap(), (2300, 1));
}

#[test]
fn unsupported_malformed_and_oversize_dimension_reads_refuse() {
    for input in [
        b"not an image".as_slice(),
        b"\x89PNG\r\n\x1a\n".as_slice(),
        b"\xff\xd8".as_slice(),
    ] {
        assert!(display_dimensions(input, Rotation::Deg0).is_err());
    }
    assert!(
        display_dimensions(b"GIF89a\x01\0\x01\0", Rotation::Deg0)
            .unwrap_err()
            .contains(UNSUPPORTED_IMAGE_CODE)
    );
    assert!(
        display_dimensions(&vec![0; MAX_INPUT_BYTES + 1], Rotation::Deg0)
            .unwrap_err()
            .contains("at most")
    );
}
