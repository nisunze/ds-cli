use super::*;
use crate::assets::REQUEST_SCHEMA;
use image::{ColorType, ImageBuffer, ImageEncoder};

/// A 30x20 JPEG whose EXIF orientation 6 shows it 20x30.
fn turned_jpeg() -> Vec<u8> {
    let mut jpeg = Vec::new();
    let pixels = ImageBuffer::from_pixel(30, 20, image::Rgb([20u8, 40, 60]));
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 90)
        .write_image(&pixels, 30, 20, ColorType::Rgb8.into())
        .unwrap();
    let mut exif = b"Exif\0\0II*\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0".to_vec();
    let mut out = vec![0xff, 0xd8, 0xff, 0xe1];
    out.extend_from_slice(&u16::try_from(exif.len() + 2).unwrap().to_be_bytes());
    out.append(&mut exif);
    out.extend_from_slice(&jpeg[2..]);
    out
}

#[test]
fn a_jpeg_reports_the_dimensions_its_exif_orientation_shows() {
    let request = PreviewRequest {
        schema: REQUEST_SCHEMA.to_string(),
        format: Format::Jpg,
        member: None,
        sheet: None,
        pages: None,
        rows: None,
        name: None,
    };
    let doc = image(&turned_jpeg(), &request).expect("jpeg header reads");
    assert!(
        matches!(&doc.blocks[0], Block::Image { width: 20, height: 30, .. }),
        "{:?}",
        doc.blocks[0]
    );
}
