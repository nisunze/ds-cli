//! Geo (§4.2, D20): plan and bounds only — geojson counted here, shp / kml / kmz / gpkg handed to the host's ds-io decoder.
//!
//! ## What this decoder does and does not do
//!
//! D20 keeps one decoder per format where it already lives. `ds-io`
//! (ds-network) decodes shp-zip, kml, kmz and gpkg geometry for the map,
//! natively and as pooled `ds-network-wasm` ops, and the kernel does not link
//! it. So for those formats this module emits a **plan**: a `geo` block naming
//! `decoder: "ds-io"`, the member the host should hand it, and the shapefile
//! companions resolved from the container's central directory. GeoJSON is
//! different — it is JSON, the kernel already parses JSON, and counting its
//! features and computing its bounding box here costs one parse and no new
//! dependency, so its block says `decoder: "kernel"` and carries the answers.
//!
//! Nothing here fetches anything. A `geo` block never contains a URL, and no
//! remote sidecar, tile, style or CRS lookup is referenced as fetchable: a
//! shapefile's companions are paths *inside the bytes the host already holds*.
//!
//! ## Refusal shape
//!
//! Malformed bytes are an `Err` with a sentence. A **bound** — the
//! [`MAX_GEO_FEATURES`] feature cap, the `shp` byte cap, or the missing
//! SQLite reader behind gpkg — is a `geo` block whose `refused` names the
//! number or the missing reader and offers the honest alternative, so the
//! deck shows a card that says why instead of an error with no document.

use serde_json::Value;

use super::super::{
    check_format_cap, container, thousands, Block, Format, PreviewDoc, PreviewRequest,
    MAX_GEO_FEATURES,
};

/// `decoder` values: who reads the geometry.
const KERNEL: &str = "kernel";
const DS_IO: &str = "ds-io";

/// How deep coordinate arrays are followed. `serde_json` refuses deeper
/// nesting than this while parsing, so it is a belt beside its braces.
const MAX_COORDINATE_DEPTH: u32 = 128;

pub fn geo(bytes: &[u8], request: &PreviewRequest) -> Result<PreviewDoc, String> {
    match request.format {
        Format::Geojson => geojson(bytes, request),
        Format::Shp => shapefile(bytes, request),
        Format::Kml | Format::Kmz => handed_to_ds_io(request),
        Format::Gpkg => refused_until_a_reader_exists(request),
        other => Err(format!(
            "{other} is not a geo format; the geo previews are geojson, shp, kml, kmz and gpkg"
        )),
    }
}

/// GeoJSON is counted and bounded in the kernel (D20): one parse, a feature
/// count, and a bounding box walked out of the coordinates.
fn geojson(bytes: &[u8], request: &PreviewRequest) -> Result<PreviewDoc, String> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|error| format!("the geojson is not valid JSON: {error}"))?;
    let features = feature_count(&value)?;
    let mut doc = PreviewDoc::new(Format::Geojson);

    if features > MAX_GEO_FEATURES {
        doc.blocks.push(Block::Geo {
            decoder: KERNEL.to_string(),
            member: request.member.clone(),
            companions: Vec::new(),
            feature_count: Some(features),
            bbox: None,
            refused: Some(format!(
                "the geojson has {} features; the preview bound is {} features — promote it as a layer to work with all of them",
                thousands(features),
                thousands(MAX_GEO_FEATURES)
            )),
        });
        return Ok(doc);
    }

    let bbox = bounding_box(&value);
    if bbox.is_none() {
        doc.notes
            .push("the geojson carries no coordinates, so it has no bounding box".to_string());
    }
    doc.blocks.push(Block::Geo {
        decoder: KERNEL.to_string(),
        member: request.member.clone(),
        companions: Vec::new(),
        feature_count: Some(features),
        bbox,
        refused: None,
    });
    Ok(doc)
}

/// A shapefile is its companions too, so the plan is built from the
/// container's central directory: which member, and which sidecars beside it.
/// `preview::preview` hands this decoder the whole container for exactly that
/// reason (a `.shp` member alone would be half a shapefile).
fn shapefile(bytes: &[u8], request: &PreviewRequest) -> Result<PreviewDoc, String> {
    let mut doc = PreviewDoc::new(Format::Shp);
    let mut member = request.member.clone();
    let mut companions = Vec::new();
    let mut refused = None;

    if looks_like_zip(bytes) {
        let listing = container::walk(bytes)?;
        let row = match &member {
            Some(path) => listing
                .members
                .iter()
                .find(|member| member.path == *path)
                .ok_or_else(|| format!("the container has no member named {path}"))?,
            None => listing
                .members
                .iter()
                .find(|member| member.format == Format::Shp)
                .ok_or_else(|| "the container holds no .shp member to preview".to_string())?,
        };
        // The container as a whole is bounded by the caller; what the geo
        // bound applies to is the shapefile itself.
        if let Err(message) = check_format_cap(row.bytes, Format::Shp) {
            refused = Some(message);
        } else if row.companions.is_empty() {
            // An empty `companions` list is `container.rs`'s wire form of an
            // incomplete shapefile: neither a `.dbf` nor a `.shx` beside it.
            refused = Some(format!(
                "{} has no .dbf or .shx companion in the container; an incomplete shapefile is reported, not half-previewed",
                row.path
            ));
        }
        companions = row.companions.clone();
        member = Some(row.path.clone());
    } else if let Some(path) = &member {
        return Err(format!(
            "the preview asks for member {path} but the bytes are not a zip archive"
        ));
    } else {
        doc.notes.push(
            "these are bare .shp bytes: any .dbf / .shx / .prj companion is the host's to supply, because the kernel can see none beside them"
                .to_string(),
        );
    }

    if refused.is_none() {
        doc.notes.push(handoff_note(Format::Shp));
    }
    doc.blocks.push(Block::Geo {
        decoder: DS_IO.to_string(),
        member,
        companions,
        feature_count: None,
        bbox: None,
        refused,
    });
    Ok(doc)
}

/// kml and kmz: the plan is the whole answer — the host runs the decoder it
/// already ships and holds the result to the feature bound.
fn handed_to_ds_io(request: &PreviewRequest) -> Result<PreviewDoc, String> {
    let mut doc = PreviewDoc::new(request.format);
    doc.notes.push(handoff_note(request.format));
    doc.blocks.push(Block::Geo {
        decoder: DS_IO.to_string(),
        member: request.member.clone(),
        companions: Vec::new(),
        feature_count: None,
        bbox: None,
        refused: None,
    });
    Ok(doc)
}

/// gpkg is refused by name in slice 1 (§4.1): there is no SQLite reader in
/// the WASM engine, and inventing one is not a preview's job.
fn refused_until_a_reader_exists(request: &PreviewRequest) -> Result<PreviewDoc, String> {
    let mut doc = PreviewDoc::new(Format::Gpkg);
    doc.blocks.push(Block::Geo {
        decoder: DS_IO.to_string(),
        member: request.member.clone(),
        companions: Vec::new(),
        feature_count: None,
        bbox: None,
        refused: Some(
            "no SQLite reader in WASM: a gpkg is not previewed in the browser — open it on the native desktop lane, or promote it as a layer"
                .to_string(),
        ),
    });
    Ok(doc)
}

fn handoff_note(format: Format) -> String {
    format!(
        "{format} is decoded by the host's existing ds-io decoder (D20); the kernel fetches nothing and the host holds the result to {} features",
        thousands(MAX_GEO_FEATURES)
    )
}

/// The GeoJSON object's feature count: a collection's `features`, or one for
/// a single `Feature` or bare geometry. Anything else is not GeoJSON.
fn feature_count(value: &Value) -> Result<u64, String> {
    let Some(kind) = value.get("type").and_then(Value::as_str) else {
        return Err(
            "the bytes are not GeoJSON: no \"type\" member names what this document is".to_string(),
        );
    };
    match kind {
        "FeatureCollection" => {
            let Some(features) = value.get("features").and_then(Value::as_array) else {
                return Err(
                    "the geojson FeatureCollection has no \"features\" array to count".to_string(),
                );
            };
            Ok(features.len() as u64)
        }
        "Feature" | "Point" | "MultiPoint" | "LineString" | "MultiLineString" | "Polygon"
        | "MultiPolygon" | "GeometryCollection" => Ok(1),
        other => Err(format!(
            "the bytes are not GeoJSON: \"{other}\" is not a FeatureCollection, a Feature or a geometry"
        )),
    }
}

/// `[min_x, min_y, max_x, max_y]` over every position in the document, at any
/// nesting depth. Only `coordinates` are read: a number in `properties` is
/// somebody's meter reading, not a place.
fn bounding_box(value: &Value) -> Option<[f64; 4]> {
    let mut bounds = None;
    scan(value, 0, &mut bounds);
    bounds
}

fn scan(value: &Value, depth: u32, bounds: &mut Option<[f64; 4]>) {
    if depth > MAX_COORDINATE_DEPTH {
        return;
    }
    match value {
        Value::Object(map) => {
            if let Some(coordinates) = map.get("coordinates") {
                positions(coordinates, depth + 1, bounds);
            }
            for key in ["features", "geometries", "geometry"] {
                if let Some(child) = map.get(key) {
                    scan(child, depth + 1, bounds);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                scan(item, depth + 1, bounds);
            }
        }
        _ => {}
    }
}

/// One position is `[x, y]` or `[x, y, z]`; anything else is a nest of them.
fn positions(value: &Value, depth: u32, bounds: &mut Option<[f64; 4]>) {
    if depth > MAX_COORDINATE_DEPTH {
        return;
    }
    let Some(items) = value.as_array() else {
        return;
    };
    let x = items.first().and_then(Value::as_f64);
    let y = items.get(1).and_then(Value::as_f64);
    if let (Some(x), Some(y)) = (x, y) {
        extend(bounds, x, y);
        return;
    }
    for item in items {
        positions(item, depth + 1, bounds);
    }
}

fn extend(bounds: &mut Option<[f64; 4]>, x: f64, y: f64) {
    if !x.is_finite() || !y.is_finite() {
        return;
    }
    match bounds {
        Some(box_) => {
            box_[0] = box_[0].min(x);
            box_[1] = box_[1].min(y);
            box_[2] = box_[2].max(x);
            box_[3] = box_[3].max(y);
        }
        None => *bounds = Some([x, y, x, y]),
    }
}

fn looks_like_zip(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::{Kind, PREVIEW_DOC_SCHEMA, REQUEST_SCHEMA};
    use std::io::{Cursor, Write};
    use zip::write::{SimpleFileOptions, ZipWriter};

    fn ask(format: Format, member: Option<&str>) -> PreviewRequest {
        PreviewRequest {
            schema: REQUEST_SCHEMA.to_string(),
            format,
            member: member.map(str::to_string),
            sheet: None,
            pages: None,
            rows: None,
            name: None,
        }
    }

    #[allow(clippy::type_complexity)]
    fn geo_block(
        doc: &PreviewDoc,
    ) -> (
        String,
        Option<String>,
        Vec<String>,
        Option<u64>,
        Option<[f64; 4]>,
        Option<String>,
    ) {
        assert_eq!(doc.schema, PREVIEW_DOC_SCHEMA);
        assert_eq!(doc.kind, Kind::Geo);
        assert!(doc.bounded, "a geo preview is always a bounded one");
        assert_eq!(doc.blocks.len(), 1, "a geo preview is one geo block");
        match doc.blocks.first() {
            Some(Block::Geo {
                decoder,
                member,
                companions,
                feature_count,
                bbox,
                refused,
            }) => (
                decoder.clone(),
                member.clone(),
                companions.clone(),
                *feature_count,
                *bbox,
                refused.clone(),
            ),
            other => panic!("expected one geo block, found {other:?}"),
        }
    }

    fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (path, bytes) in entries {
            writer.start_file(*path, options).expect("start member");
            writer.write_all(bytes).expect("write member");
        }
        writer.finish().expect("finish archive").into_inner()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_geojson_feature_count_and_bbox() {
        let text = br#"{
          "type": "FeatureCollection",
          "features": [
            { "type": "Feature", "properties": { "load_kva": 999 },
              "geometry": { "type": "Point", "coordinates": [30.05, -1.95, 1420] } },
            { "type": "Feature", "properties": {},
              "geometry": { "type": "LineString",
                            "coordinates": [[30.10, -1.90], [29.98, -2.02]] } },
            { "type": "Feature", "properties": {},
              "geometry": { "type": "Polygon",
                            "coordinates": [[[30.00, -1.99], [30.02, -1.99],
                                             [30.02, -1.97], [30.00, -1.99]]] } }
          ]
        }"#;
        let doc = geo(text, &ask(Format::Geojson, None)).expect("geojson previews in the kernel");
        let (decoder, member, companions, features, bbox, refused) = geo_block(&doc);
        assert_eq!(decoder, "kernel", "geojson is counted here, not by ds-io");
        assert_eq!(member, None);
        assert!(companions.is_empty());
        assert_eq!(features, Some(3));
        assert_eq!(refused, None);
        let bbox = bbox.expect("coordinates give a bounding box");
        assert!((bbox[0] - 29.98).abs() < 1e-9, "{bbox:?}");
        assert!((bbox[1] - (-2.02)).abs() < 1e-9, "{bbox:?}");
        assert!((bbox[2] - 30.10).abs() < 1e-9, "{bbox:?}");
        assert!((bbox[3] - (-1.90)).abs() < 1e-9, "{bbox:?}");

        // A single Feature, a bare geometry and an empty collection all count
        // honestly, and a document with no coordinates says it has no box.
        let one = geo(
            br#"{"type":"Feature","geometry":{"type":"Point","coordinates":[1.5,2.5]},"properties":{}}"#,
            &ask(Format::Geojson, Some("gis/one.geojson")),
        )
        .expect("a single feature");
        let (_, member, _, features, bbox, _) = geo_block(&one);
        assert_eq!(member.as_deref(), Some("gis/one.geojson"));
        assert_eq!(features, Some(1));
        assert_eq!(bbox, Some([1.5, 2.5, 1.5, 2.5]));

        let empty = geo(
            br#"{"type":"FeatureCollection","features":[]}"#,
            &ask(Format::Geojson, None),
        )
        .expect("an empty collection");
        let (_, _, _, features, bbox, _) = geo_block(&empty);
        assert_eq!(features, Some(0));
        assert_eq!(bbox, None);
        assert!(empty
            .notes
            .iter()
            .any(|note| note.contains("no bounding box")));
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_geojson_refuses_above_feature_cap() {
        let mut text = String::from("{\"type\":\"FeatureCollection\",\"features\":[");
        for index in 0..=MAX_GEO_FEATURES {
            if index > 0 {
                text.push(',');
            }
            text.push_str(
                "{\"type\":\"Feature\",\"properties\":{},\"geometry\":{\"type\":\"Point\",\"coordinates\":[30.1,-1.9]}}",
            );
        }
        text.push_str("]}");

        let doc = geo(text.as_bytes(), &ask(Format::Geojson, None)).expect("a refusal is a card");
        let (decoder, _, _, features, bbox, refused) = geo_block(&doc);
        assert_eq!(decoder, "kernel");
        assert_eq!(features, Some(MAX_GEO_FEATURES + 1));
        assert_eq!(bbox, None, "a refused preview computes nothing further");
        let refused = refused.expect("past the feature bound the preview is refused");
        assert!(refused.contains("20,001"), "{refused}");
        assert!(refused.contains("20,000"), "{refused}");
        assert!(
            refused.contains("promote it as a layer"),
            "a refusal offers the honest alternative: {refused}"
        );
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_geojson_refuses_malformed_bytes() {
        let malformed: [(&[u8], &str); 6] = [
            (b"", "not valid JSON"),
            (b"{\"type\": \"FeatureCollection\"", "not valid JSON"),
            (b"\x00\x01\x02\x03", "not valid JSON"),
            (b"[1,2,3]", "not GeoJSON"),
            (b"{\"features\":[]}", "not GeoJSON"),
            (b"{\"type\":\"FeatureCollection\"}", "features"),
        ];
        for (bytes, expected) in malformed {
            let refusal = geo(bytes, &ask(Format::Geojson, None))
                .expect_err("malformed bytes are refused, never a panic");
            assert!(
                refusal.contains(expected),
                "{refusal} should mention {expected}"
            );
        }

        // A feature count is never guessed from a shape that is not GeoJSON,
        // and a nest of arrays neither panics nor overflows the stack.
        let deep = format!("{}[1,2]{}", "[".repeat(60), "]".repeat(60));
        let text = format!(
            "{{\"type\":\"Feature\",\"geometry\":{{\"type\":\"MultiPolygon\",\"coordinates\":{deep}}}}}"
        );
        let doc = geo(text.as_bytes(), &ask(Format::Geojson, None)).expect("deep nesting");
        let (_, _, _, features, bbox, _) = geo_block(&doc);
        assert_eq!(features, Some(1));
        assert_eq!(bbox, Some([1.0, 2.0, 1.0, 2.0]));
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_geo_plans_the_existing_ds_io_decoder() {
        for format in [Format::Kml, Format::Kmz] {
            let doc = geo(b"<kml/>", &ask(format, Some("gis/lines.kml"))).expect("a plan");
            let (decoder, member, companions, features, bbox, refused) = geo_block(&doc);
            assert_eq!(decoder, "ds-io", "{format} keeps its existing decoder");
            assert_eq!(member.as_deref(), Some("gis/lines.kml"));
            assert!(companions.is_empty());
            assert_eq!((features, bbox, refused), (None, None, None));
            assert!(doc.notes.iter().any(|note| note.contains("ds-io")));
        }

        let doc = geo(b"SQLite format 3\0", &ask(Format::Gpkg, None)).expect("a named refusal");
        let (_, _, _, _, _, refused) = geo_block(&doc);
        let refused = refused.expect("gpkg is refused by name in slice 1");
        assert!(refused.contains("no SQLite reader in WASM"), "{refused}");
        assert!(refused.contains("promote it as a layer"), "{refused}");

        let wrong = geo(b"%PDF-1.7", &ask(Format::Pdf, None)).expect_err("not a geo format");
        assert!(wrong.contains("not a geo format"), "{wrong}");

        // No block a host could mistake for something to fetch.
        for doc in [
            geo(b"<kml/>", &ask(Format::Kmz, None)).expect("a plan"),
            geo(
                b"{\"type\":\"Feature\",\"geometry\":null}",
                &ask(Format::Geojson, None),
            )
            .expect("a plan"),
        ] {
            let rendered = serde_json::to_string(&doc).expect("serialises");
            assert!(!rendered.contains("http://"), "{rendered}");
            assert!(!rendered.contains("https://"), "{rendered}");
        }
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_geo_previews_through_the_two_slice_request() {
        let request = serde_json::to_vec(&serde_json::json!({
            "schema": REQUEST_SCHEMA,
            "action": "preview",
            "format": "geojson",
        }))
        .expect("request");
        let answer = crate::assets::evaluate_with_bytes(
            br#"{"type":"Feature","geometry":{"type":"Point","coordinates":[30.06,-1.94]},"properties":{}}"#,
            &request,
        )
        .expect("the two-slice preview");
        let answer: serde_json::Value = serde_json::from_str(&answer).expect("json");
        assert_eq!(answer["action"], "preview");
        assert_eq!(answer["schema"], PREVIEW_DOC_SCHEMA);
        assert_eq!(answer["kind"], "geo");
        assert_eq!(answer["bounded"], true);
        assert_eq!(answer["blocks"][0]["type"], "geo");
        assert_eq!(answer["blocks"][0]["decoder"], "kernel");
        assert_eq!(answer["blocks"][0]["feature_count"], 1);
        assert_eq!(answer["blocks"][0]["bbox"][0], 30.06);
        assert_eq!(answer["blocks"][0]["refused"], serde_json::Value::Null);
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_shapefile_companions_reach_the_geo_block() {
        let bytes = zip_of(&[
            ("Lot3/gis/poles.shp", b"shp"),
            ("Lot3/gis/poles.dbf", b"dbf"),
            ("Lot3/gis/poles.shx", b"shx"),
            ("Lot3/gis/poles.prj", b"prj"),
            ("Lot3/drawings/site.pdf", b"%PDF"),
        ]);

        // Named member, and the same answer when the container holds one
        // shapefile and the caller names no member.
        for member in [Some("Lot3/gis/poles.shp"), None] {
            let doc = geo(&bytes, &ask(Format::Shp, member)).expect("a shapefile plan");
            let (decoder, resolved, companions, features, bbox, refused) = geo_block(&doc);
            assert_eq!(decoder, "ds-io");
            assert_eq!(resolved.as_deref(), Some("Lot3/gis/poles.shp"));
            assert_eq!(
                companions,
                vec![
                    "Lot3/gis/poles.dbf".to_string(),
                    "Lot3/gis/poles.shx".to_string(),
                    "Lot3/gis/poles.prj".to_string(),
                ]
            );
            assert_eq!((features, bbox, refused), (None, None, None));
        }

        // An incomplete shapefile is reported, not half-previewed.
        let lone = zip_of(&[("gis/lone.shp", b"shp"), ("gis/lone.prj", b"prj")]);
        let doc = geo(&lone, &ask(Format::Shp, None)).expect("a plan with a refusal");
        let (_, _, companions, _, _, refused) = geo_block(&doc);
        assert!(companions.is_empty());
        let refused = refused.expect("an incomplete shapefile is named");
        assert!(refused.contains("gis/lone.shp"), "{refused}");
        assert!(refused.contains("no .dbf or .shx companion"), "{refused}");

        // Bare .shp bytes say who owns the missing companions; a member asked
        // for in bytes that are not an archive is refused by name.
        let doc = geo(b"\x00\x00\x27\x0a bare shp", &ask(Format::Shp, None)).expect("a plan");
        assert!(doc
            .notes
            .iter()
            .any(|note| note.contains("host's to supply")));
        let refusal = geo(
            b"\x00\x00\x27\x0a bare shp",
            &ask(Format::Shp, Some("gis/x.shp")),
        )
        .expect_err("a member needs an archive");
        assert!(refusal.contains("not a zip archive"), "{refusal}");

        let missing =
            geo(&bytes, &ask(Format::Shp, Some("gis/absent.shp"))).expect_err("no such member");
        assert!(missing.contains("gis/absent.shp"), "{missing}");
        let none = geo(&zip_of(&[("read.me", b"hi")]), &ask(Format::Shp, None))
            .expect_err("no shapefile in the archive");
        assert!(none.contains("no .shp member"), "{none}");
    }

    // ── adversarial review (stage 4) ────────────────────────────────────────

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_review_geojson_huge_coordinate_lists_and_non_finite_numbers() {
        // 200,000 positions in one LineString: one feature, one bbox, no
        // feature-bound refusal (the bound counts features, not vertices).
        let mut text = String::from(
            r#"{"type":"Feature","properties":{},"geometry":{"type":"LineString","coordinates":["#,
        );
        for index in 0..200_000u32 {
            if index > 0 {
                text.push(',');
            }
            text.push_str(&format!("[{}.5,{}.25]", index % 360, index % 180));
        }
        text.push_str("]}}");
        let doc = geo(text.as_bytes(), &ask(Format::Geojson, None)).expect("counted");
        let (_, _, _, features, bbox, refused) = geo_block(&doc);
        assert_eq!(features, Some(1));
        assert_eq!(refused, None);
        assert_eq!(bbox, Some([0.5, 0.25, 359.5, 179.25]));

        // NaN and Infinity are not JSON: a named refusal, never a bbox.
        for body in [
            r#"{"type":"Point","coordinates":[NaN, 1]}"#,
            r#"{"type":"Point","coordinates":[Infinity, 1]}"#,
            r#"{"type":"Point","coordinates":[-Infinity, 1]}"#,
        ] {
            let error = geo(body.as_bytes(), &ask(Format::Geojson, None)).expect_err(body);
            assert!(error.contains("not valid JSON"), "{error}");
        }
        // An overflowing literal is either refused or excluded from the bbox;
        // the bbox that survives is finite.
        for body in [
            r#"{"type":"LineString","coordinates":[[1e999, 1],[2, 3]]}"#,
            r#"{"type":"LineString","coordinates":[[-1e999, 1],[2, 3]]}"#,
        ] {
            if let Ok(doc) = geo(body.as_bytes(), &ask(Format::Geojson, None)) {
                let (_, _, _, _, bbox, _) = geo_block(&doc);
                let bbox = bbox.expect("the finite position gives a bbox");
                assert!(bbox.iter().all(|edge| edge.is_finite()), "{bbox:?}");
                assert_eq!(bbox, [2.0, 1.0, 2.0, 3.0]);
            }
        }
        // Positions that are not numbers, and a geometry with no coordinates.
        let body = r#"{"type":"FeatureCollection","features":[{"type":"Feature","geometry":{"type":"Point","coordinates":["x","y"]}},{"type":"Feature","geometry":null}]}"#;
        let doc = geo(body.as_bytes(), &ask(Format::Geojson, None)).expect("counted");
        let (_, _, _, features, bbox, _) = geo_block(&doc);
        assert_eq!(features, Some(2));
        assert_eq!(bbox, None);
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_review_geojson_deep_nesting_and_junk_are_named_refusals() {
        let mut deep = String::from(r#"{"type":"Point","coordinates":"#);
        deep.push_str(&"[".repeat(5_000));
        deep.push_str("1,2");
        deep.push_str(&"]".repeat(5_000));
        deep.push('}');
        let error = geo(deep.as_bytes(), &ask(Format::Geojson, None)).expect_err("too deep");
        assert!(error.contains("not valid JSON"), "{error}");
        for junk in [
            "",
            "{",
            "[]",
            "null",
            r#"{"type":"Feature"}"#,
            r#"{"type":"FeatureCollection","features":3}"#,
            r#"{"type":"Nonsense"}"#,
            "\u{feff}{}",
        ] {
            let outcome = geo(junk.as_bytes(), &ask(Format::Geojson, None));
            match outcome {
                Ok(doc) => {
                    let (_, _, _, features, _, _) = geo_block(&doc);
                    assert_eq!(features, Some(1), "{junk:?}");
                }
                Err(error) => assert!(!error.is_empty(), "{junk:?}"),
            }
        }
        // shp with a member on bytes that are not a zip, and kml/kmz with
        // anything at all: no panic, a plan or a named refusal.
        let error = geo(b"not a zip", &ask(Format::Shp, Some("x.shp"))).expect_err("not a zip");
        assert!(error.contains("not a zip archive"), "{error}");
        assert!(geo(b"", &ask(Format::Kml, None)).is_ok());
        assert!(geo(b"PK", &ask(Format::Kmz, Some("doc.kml"))).is_ok());
    }
}
