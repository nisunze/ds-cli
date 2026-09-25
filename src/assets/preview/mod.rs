//! `PreviewDoc` decoding (§4): one bounded decoder per format, dispatched by
//! the declared format. Unsupported formats are refused by name with the
//! honest alternative.
//!
//! Member handling: `bytes` are what the host holds. When `request.member`
//! names a container member, the member is extracted here (bounded by its
//! format's cap) and the decoder receives the member's bytes — except for
//! `shp`, whose decoder receives the whole container because a shapefile is
//! its companions too (`geo.rs` resolves them from the central directory).
//! A decoder therefore never sees `member` as something to extract; it is
//! context for its notes and its `geo` block. A workbook tab is selected by
//! `request.sheet`, never by `member`, so the two never collide.
//!
//! `odt` and `ods` are refused here by name: slice 1 decodes OOXML only, and
//! `format::previewable` says the same before any bytes are read.

pub mod docx;
pub mod eml;
pub mod geo;
pub mod image;
pub mod tabular;
pub mod text;
pub mod xlsx;

use super::{check_format_cap, container, format, Format, PreviewDoc, PreviewRequest};

pub fn preview(bytes: &[u8], request: &PreviewRequest) -> Result<PreviewDoc, String> {
    let format = request.format;
    match request.member.as_deref() {
        Some(_) if format == Format::Shp => decode(bytes, request),
        Some(member) => {
            let extracted =
                container::extract_member(bytes, member, format::format_cap_bytes(format))?;
            check_format_cap(extracted.len() as u64, format)?;
            decode(&extracted, request)
        }
        None => {
            check_format_cap(bytes.len() as u64, format)?;
            decode(bytes, request)
        }
    }
}

fn decode(bytes: &[u8], request: &PreviewRequest) -> Result<PreviewDoc, String> {
    let format = request.format;
    match format {
        Format::Md => text::markdown(bytes, request),
        Format::Html => Err("HTML is rendered only in a sandboxed attachment viewer; the kernel does not execute or decode it".into()),
        Format::Txt => text::plain(bytes, request),
        Format::Csv | Format::Tsv => tabular::delimited(bytes, request),
        Format::Xlsx => xlsx::workbook(bytes, request),
        Format::Docx => docx::document(bytes, request),
        Format::Odt => Err(
            "odt is an OpenDocument text document and this kernel decodes docx (OOXML); convert it to docx or open it externally"
                .into(),
        ),
        Format::Ods => Err(
            "ods is an OpenDocument workbook and this kernel decodes xlsx (OOXML); convert it to xlsx or open it externally"
                .into(),
        ),
        Format::Eml => eml::mail(bytes, request),
        Format::Jpg | Format::Png | Format::Webp | Format::Tif => image::image(bytes, request),
        Format::Shp | Format::Kmz | Format::Kml | Format::Gpkg | Format::Geojson => {
            geo::geo(bytes, request)
        }
        Format::Pdf => Err(
            "pdf is rendered by pdf.js in the host (decision D8); the kernel recognises and bounds pdf but does not decode it"
                .into(),
        ),
        Format::Msg => Err(
            "msg is recognised as mail but its decoder is slice 2; open the message externally or import it as eml"
                .into(),
        ),
        Format::Doc | Format::Rtf | Format::Xls => Err(format!(
            "{format} has no kernel decoder; open it externally or convert it to {}",
            match format {
                Format::Xls => "xlsx",
                _ => "docx",
            }
        )),
        Format::Zip | Format::SevenZ | Format::TarGz => Err(format!(
            "{format} is a container, not a document; walk it with container_walk and preview one member"
        )),
        Format::Dsgrid => {
            Err("dsgrid is a DS Grid model; open it in DS Grid rather than as a preview".into())
        }
        Format::Unknown => {
            Err("unknown format has no preview; the metadata card is the honest alternative".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write};

    use serde_json::{json, Value};

    use super::super::{evaluate_with_bytes, MAX_TEXT_BYTES, REQUEST_SCHEMA};

    fn zip_of(parts: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = zip::write::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, bytes) in parts {
            writer.start_file(*name, options).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    fn preview(bytes: &[u8], body: Value) -> Result<Value, String> {
        let mut request = body;
        request["schema"] = json!(REQUEST_SCHEMA);
        request["action"] = json!("preview");
        let request = serde_json::to_vec(&request).unwrap();
        evaluate_with_bytes(bytes, &request)
            .map(|text| serde_json::from_str(&text).expect("preview result is JSON"))
    }

    /// A two-sheet OOXML workbook with inline strings and no shared strings.
    fn workbook() -> Vec<u8> {
        const WORKBOOK: &str = r#"<?xml version="1.0" encoding="UTF-8"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Summary" sheetId="1" r:id="rId1"/><sheet name="Data" sheetId="2" r:id="rId2"/></sheets></workbook>"#;
        const RELS: &str = r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/></Relationships>"#;
        const SUMMARY: &str = r#"<?xml version="1.0" encoding="UTF-8"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Total</t></is></c><c r="B1"><v>3</v></c></row></sheetData></worksheet>"#;
        const DATA: &str = r#"<?xml version="1.0" encoding="UTF-8"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Transformer</t></is></c></row><row r="2"><c r="A2"><v>7</v></c></row></sheetData></worksheet>"#;
        zip_of(&[
            ("[Content_Types].xml", b"<Types/>"),
            ("xl/workbook.xml", WORKBOOK.as_bytes()),
            ("xl/_rels/workbook.xml.rels", RELS.as_bytes()),
            ("xl/worksheets/sheet1.xml", SUMMARY.as_bytes()),
            ("xl/worksheets/sheet2.xml", DATA.as_bytes()),
        ])
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_preview_member_is_extracted_then_decoded_by_its_own_format() {
        let archive = zip_of(&[
            ("Lot3/gis/poles.csv", b"id,name\n1,A\n2,B\n"),
            ("Lot3/readme.md", b"# Lot 3\n\nSee the poles.\n"),
        ]);
        let doc = preview(
            &archive,
            json!({"format": "csv", "member": "Lot3/gis/poles.csv"}),
        )
        .unwrap();
        assert_eq!(doc["action"], "preview");
        assert_eq!(doc["schema"], "ds.assets.preview_doc/v1");
        assert_eq!(doc["format"], "csv");
        assert_eq!(doc["kind"], "sheet");
        assert_eq!(doc["blocks"][0]["type"], "grid");
        assert_eq!(doc["blocks"][0]["columns"], json!(["id", "name"]));
        assert_eq!(doc["blocks"][0]["rows_total"], 2);
        let doc = preview(
            &archive,
            json!({"format": "md", "member": "Lot3/readme.md"}),
        )
        .unwrap();
        assert_eq!(doc["blocks"][0]["type"], "heading");
        assert_eq!(doc["blocks"][0]["text"], "Lot 3");
        let error = preview(
            &archive,
            json!({"format": "md", "member": "Lot3/absent.md"}),
        )
        .unwrap_err();
        assert!(error.contains("Lot3/absent.md"), "{error}");
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_preview_member_cap_is_the_members_own_format_cap() {
        // A 1 MiB + 1 text member deflates to a few KiB, so the archive is
        // far under every input bound; the member's declared size is what
        // trips the txt cap, before any inflation.
        let oversized = vec![b'a'; MAX_TEXT_BYTES as usize + 1];
        let archive = zip_of(&[("notes.txt", &oversized)]);
        assert!(archive.len() < 64 * 1024);
        let error = preview(&archive, json!({"format": "txt", "member": "notes.txt"})).unwrap_err();
        assert!(
            error.contains("1,048,577 bytes") && error.contains("1 MiB"),
            "{error}"
        );
        // The same bytes asked for as a pack are walked, not decoded.
        let error = preview(&archive, json!({"format": "zip"})).unwrap_err();
        assert!(error.contains("container_walk"), "{error}");
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_preview_selects_a_sheet_of_a_workbook_inside_a_container() {
        let workbook = workbook();
        let doc = preview(&workbook, json!({"format": "xlsx"})).unwrap();
        assert_eq!(doc["blocks"][0]["title"], "Summary");
        let doc = preview(&workbook, json!({"format": "xlsx", "sheet": "Data"})).unwrap();
        assert_eq!(doc["blocks"][0]["title"], "Data");
        assert_eq!(doc["blocks"][0]["rows"][0][0], "Transformer");
        assert_eq!(doc["blocks"][0]["rows"][1][0], "7");

        let archive = zip_of(&[("Lot3/boq.xlsx", &workbook)]);
        let doc = preview(
            &archive,
            json!({"format": "xlsx", "member": "Lot3/boq.xlsx", "sheet": "Data"}),
        )
        .unwrap();
        assert_eq!(doc["blocks"][0]["title"], "Data");
        assert_eq!(doc["blocks"][1]["type"], "meta");
        assert_eq!(doc["blocks"][1]["entries"][1][1], "Data");
        let error = preview(
            &archive,
            json!({"format": "xlsx", "member": "Lot3/boq.xlsx", "sheet": "Ledger"}),
        )
        .unwrap_err();
        assert_eq!(
            error,
            "the workbook has no sheet named 'Ledger'; its sheets are 'Summary', 'Data'"
        );
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_preview_refuses_opendocument_by_name() {
        let package = zip_of(&[("mimetype", b"application/vnd.oasis.opendocument.text")]);
        let error = preview(&package, json!({"format": "odt"})).unwrap_err();
        assert!(
            error.starts_with("odt is an OpenDocument text document") && error.contains("docx"),
            "{error}"
        );
        let error = preview(&package, json!({"format": "ods"})).unwrap_err();
        assert!(
            error.starts_with("ods is an OpenDocument workbook") && error.contains("xlsx"),
            "{error}"
        );
        for format in ["odt", "ods"] {
            let recognised: Value = serde_json::from_str(
                &super::super::evaluate(
                    &serde_json::to_vec(&json!({
                        "schema": REQUEST_SCHEMA, "action": "recognise",
                        "name": format!("letter.{format}"), "bytes_len": package.len(),
                        "head_b64": base64::Engine::encode(
                            &base64::engine::general_purpose::STANDARD, &package)
                    }))
                    .unwrap(),
                )
                .unwrap(),
            )
            .unwrap();
            assert_eq!(recognised["previewable"], false, "{format}");
        }
    }

    // ── adversarial review (stage 4) ────────────────────────────────────────

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_review_every_format_survives_empty_short_and_wrong_bytes() {
        use crate::assets::Format;
        let probes: [&[u8]; 9] = [
            b"",
            b"x",
            b"PK",
            b"PK\x03\x04",
            b"PK\x03\x04\x14\x00\x00\x00\x08\x00",
            b"%PDF-",
            b"{",
            b"\xff\xd8\xff",
            b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1",
        ];
        for format in Format::ALL {
            for probe in probes {
                let outcome = preview(probe, json!({"format": format.as_str()}));
                if let Err(error) = &outcome {
                    assert!(!error.trim().is_empty(), "{format}: an empty refusal");
                }
                let outcome = preview(probe, json!({"format": format.as_str(), "member": "m"}));
                if let Err(error) = &outcome {
                    assert!(!error.trim().is_empty(), "{format}: an empty refusal");
                }
            }
        }
        // The two that read nothing are honest about it.
        let doc = preview(b"", json!({"format": "txt"})).unwrap();
        assert_eq!(doc["blocks"], json!([]));
        let error = preview(b"", json!({"format": "csv"})).unwrap_err();
        assert!(error.contains("empty"), "{error}");
        let error = preview(b"", json!({"format": "eml"})).unwrap_err();
        assert!(error.contains("empty"), "{error}");
    }
}
