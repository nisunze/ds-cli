//! The Kernel admits export requests into unit effects and checks the
//! unit's reply container; the writers are tested beside the Network unit.
use super::*;
use ds_network_contracts::grid_unit::{encode_reply, GridRefusal};
use serde_json::json;
use std::io::Write;

fn archive(members: &[&str]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for member in members {
        zip.start_file(*member, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"x").unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[test]
fn export_requests_become_unit_effects_after_admission() {
    let input =
        serde_json::to_vec(&json!({"format": "kmz", "layers": [{"name": "p", "geojson": {}}]}))
            .unwrap();
    let effect = request_effect(&input).unwrap();
    let sent = GridRequest::decode(&effect.payload).unwrap();
    assert_eq!(sent.operation, OPERATION);
    assert_eq!(sent.args, [GridArg::Bytes(input)]);
    let error = request_effect(br#"{"format":"pdf","layers":[]}"#).unwrap_err();
    assert!(error.to_string().starts_with("export_request_invalid: "));
}

#[test]
fn replies_pass_the_container_check_and_refusals_keep_their_message() {
    let xlsx = archive(&["[Content_Types].xml", "xl/workbook.xml"]);
    let reply = encode_reply(OPERATION, &Ok(GridResult::Bytes(xlsx.clone())));
    assert_eq!(
        decode_unit_reply(LayerExportFormat::Xlsx, &reply).unwrap(),
        xlsx
    );
    assert!(decode_unit_reply(LayerExportFormat::Dashboard, &reply).is_ok());
    for (format, bytes) in [
        (LayerExportFormat::Kmz, xlsx),
        (LayerExportFormat::Geojson, b"[1, 2]".to_vec()),
        (LayerExportFormat::ShpZip, archive(&["a.shp", "a.dbf"])),
    ] {
        let reply = encode_reply(OPERATION, &Ok(GridResult::Bytes(bytes)));
        let error = decode_unit_reply(format, &reply).unwrap_err();
        assert!(
            error.to_string().starts_with("export_response_invalid: "),
            "{error}"
        );
    }
    let refused = encode_reply(
        OPERATION,
        &Err(GridRefusal::new(
            "export_request_invalid",
            "export_request_invalid: no layers",
        )),
    );
    assert_eq!(
        decode_unit_reply(LayerExportFormat::Kmz, &refused)
            .unwrap_err()
            .to_string(),
        "export_request_invalid: no layers"
    );
    let text = encode_reply(OPERATION, &Ok(GridResult::Text("{}".into())));
    assert!(matches!(
        decode_unit_reply(LayerExportFormat::Geojson, &text),
        Err(LayerExportError::InvalidResponse(_))
    ));
}
