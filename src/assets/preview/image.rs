//! Images (§4.2 jpg / png / webp / tif): dimensions via `imagesize`, bytes
//! passed through, no full decode.
//!
//! The kernel reads the header only. It never decodes pixels, never
//! re-encodes and never carries the image inside the `PreviewDoc`
//! (`bytes_b64` is always `None`): the host already holds the bytes it asked
//! about and renders them itself, downscaled to [`MAX_THUMBNAIL_PX`] on the
//! longest edge. Over [`MAX_IMAGE_BYTES`] the preview is refused with the
//! number in it, never a partial render.

use super::super::{check_format_cap, thousands, Block, Format, PreviewDoc, PreviewRequest};
use super::super::{MAX_IMAGE_BYTES, MAX_THUMBNAIL_PX};

pub fn image(bytes: &[u8], request: &PreviewRequest) -> Result<PreviewDoc, String> {
    let format = request.format;
    let mime = mime_of(format)?;
    check_format_cap(bytes.len() as u64, format)?;
    let size = imagesize::blob_size(bytes).map_err(|error| {
        format!(
            "the {format} header carries no readable image size ({error}); the metadata card is the honest alternative"
        )
    })?;
    let width = edge(size.width, format, "width")?;
    let height = edge(size.height, format, "height")?;

    let mut doc = PreviewDoc::new(format);
    doc.blocks.push(Block::Image {
        index: 0,
        mime: mime.to_string(),
        width,
        height,
        bytes_b64: None,
    });
    doc.blocks.push(Block::Meta {
        entries: vec![
            ("dimensions".into(), format!("{width} × {height} px")),
            (
                "bytes".into(),
                format!("{} bytes", thousands(bytes.len() as u64)),
            ),
            ("mime".into(), mime.to_string()),
        ],
    });
    doc.notes.push(
        "the kernel read the image header only; the host renders the bytes it already holds".into(),
    );
    if width.max(height) > MAX_THUMBNAIL_PX {
        doc.notes.push(format!(
            "the image is {width} × {height} px; a thumbnail is drawn at most {MAX_THUMBNAIL_PX} px on its longest edge"
        ));
    }
    Ok(doc)
}

/// The §4.2 image formats and their media types; anything else is refused by
/// name rather than guessed at.
fn mime_of(format: Format) -> Result<&'static str, String> {
    match format {
        Format::Jpg => Ok("image/jpeg"),
        Format::Png => Ok("image/png"),
        Format::Webp => Ok("image/webp"),
        Format::Tif => Ok("image/tiff"),
        other => Err(format!(
            "{other} is not an image; the image decoder reads jpg, png, webp and tif (the image bound is {} bytes)",
            thousands(MAX_IMAGE_BYTES)
        )),
    }
}

/// One header dimension as a `u32`; a zero or absurd edge is a refusal, not a
/// rendered zero-pixel image.
fn edge(value: usize, format: Format, what: &str) -> Result<u32, String> {
    let edge = u32::try_from(value).map_err(|_| {
        format!("the {format} header declares a {what} of {value} px, which is not a readable size")
    })?;
    if edge == 0 {
        return Err(format!(
            "the {format} header declares a {what} of 0 px; the bytes are not a readable {format}"
        ));
    }
    Ok(edge)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::REQUEST_SCHEMA;

    fn request(format: Format) -> PreviewRequest {
        PreviewRequest {
            schema: REQUEST_SCHEMA.to_string(),
            format,
            member: None,
            sheet: None,
            pages: None,
            rows: None,
            name: None,
        }
    }

    /// An 8-byte signature, the IHDR length and tag, then width and height —
    /// exactly the header bytes a dimension read touches.
    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR".to_vec();
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
        bytes
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_image_dimensions_without_decode() {
        let doc = image(&png(1920, 1080), &request(Format::Png)).expect("png header reads");
        assert_eq!(
            doc.blocks[0],
            Block::Image {
                index: 0,
                mime: "image/png".into(),
                width: 1920,
                height: 1080,
                bytes_b64: None,
            }
        );
        assert!(matches!(&doc.blocks[1], Block::Meta { entries } if entries
            .iter()
            .any(|(key, value)| key == "dimensions" && value == "1920 × 1080 px")));
        assert!(!doc.truncated);
        assert!(doc.bounded);

        // A jpeg SOF0 header: marker, length, precision, height, width, and
        // the end-of-image marker, since a size read needs 12 bytes to
        // recognise the format at all.
        let jpeg = b"\xff\xd8\xff\xc0\x00\x11\x08\x00\x40\x00\x60\xff\xd9";
        let doc = image(jpeg, &request(Format::Jpg)).expect("jpeg header reads");
        assert_eq!(
            doc.blocks[0],
            Block::Image {
                index: 0,
                mime: "image/jpeg".into(),
                width: 96,
                height: 64,
                bytes_b64: None,
            }
        );

        // Above the thumbnail edge the host is told what it must downscale.
        let doc = image(&png(4000, 3000), &request(Format::Png)).expect("png header reads");
        assert!(
            doc.notes
                .iter()
                .any(|note| note.contains("4000 × 3000 px") && note.contains("2048")),
            "{:?}",
            doc.notes
        );
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_image_malformed_input() {
        let refusal =
            image(b"not an image at all", &request(Format::Jpg)).expect_err("garbage is refused");
        assert!(
            refusal.contains("jpg") && refusal.contains("metadata card"),
            "{refusal}"
        );
        assert!(image(&[], &request(Format::Png)).is_err());
        assert!(image(&png(0, 480), &request(Format::Png))
            .expect_err("a zero edge is refused")
            .contains("0 px"));
        assert!(image(&png(16, 16), &request(Format::Zip))
            .expect_err("a non-image format is refused")
            .contains("is not an image"));

        let over = vec![0u8; (MAX_IMAGE_BYTES + 1) as usize];
        let refusal = image(&over, &request(Format::Png)).expect_err("over the image bound");
        assert!(
            refusal.contains("12,582,913") && refusal.contains("12 MiB"),
            "{refusal}"
        );
    }

    // ── adversarial review (stage 4) ────────────────────────────────────────

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_review_truncated_image_headers_never_panic() {
        // SOF0 with 0x20 rows × 0x40 columns.
        let jpeg: Vec<u8> = vec![
            0xff, 0xd8, 0xff, 0xc0, 0x00, 0x11, 0x08, 0x00, 0x20, 0x00, 0x40, 0x03, 0x01, 0x22,
            0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01,
        ];
        // VP8X: width − 1 and height − 1 as 24-bit little-endian at 0x18.
        let mut webp = b"RIFF".to_vec();
        webp.extend_from_slice(&22u32.to_le_bytes());
        webp.extend_from_slice(b"WEBPVP8X");
        webp.extend_from_slice(&10u32.to_le_bytes());
        webp.extend_from_slice(&[0, 0, 0, 0]);
        webp.extend_from_slice(&[63, 0, 0, 31, 0, 0]);
        // Little-endian TIFF with one IFD carrying ImageWidth and ImageLength.
        let mut tiff = b"II*\0".to_vec();
        tiff.extend_from_slice(&8u32.to_le_bytes());
        tiff.extend_from_slice(&2u16.to_le_bytes());
        for (tag, value) in [(0x0100u16, 64u16), (0x0101u16, 32u16)] {
            tiff.extend_from_slice(&tag.to_le_bytes());
            tiff.extend_from_slice(&3u16.to_le_bytes());
            tiff.extend_from_slice(&1u32.to_le_bytes());
            tiff.extend_from_slice(&value.to_le_bytes());
            tiff.extend_from_slice(&0u16.to_le_bytes());
        }
        tiff.extend_from_slice(&0u32.to_le_bytes());
        for (format, bytes) in [
            (Format::Png, png(64, 32)),
            (Format::Jpg, jpeg),
            (Format::Webp, webp),
            (Format::Tif, tiff),
        ] {
            let doc = image(&bytes, &request(format))
                .unwrap_or_else(|error| panic!("{format}: the full header decodes: {error}"));
            match &doc.blocks[0] {
                Block::Image { width, height, .. } => {
                    assert_eq!((*width, *height), (64, 32), "{format}")
                }
                other => panic!("{format}: {other:?}"),
            }
            for length in 0..bytes.len() {
                let cut = &bytes[..length];
                if let Err(error) = image(cut, &request(format)) {
                    assert!(
                        error.contains(format.as_str()),
                        "{format}@{length}: {error}"
                    );
                }
                // And declared as every other image format: still no panic.
                for wrong in [Format::Png, Format::Jpg, Format::Webp, Format::Tif] {
                    let _ = image(cut, &request(wrong));
                }
            }
            assert!(
                image(&[], &request(format)).is_err(),
                "{format}: empty is a refusal"
            );
        }
        // A TIFF whose IFD offset points past the end of the bytes.
        let mut wild = b"II*\0".to_vec();
        wild.extend_from_slice(&0xffff_fff0u32.to_le_bytes());
        wild.extend_from_slice(&[0; 16]);
        let error = image(&wild, &request(Format::Tif)).expect_err("a wild IFD offset");
        assert!(error.contains("tif"), "{error}");
    }
}
