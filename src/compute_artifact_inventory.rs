//! Published artifact declarations and canonical names shared by every client.
//! The host transports responses; it never invents formats or drops layout metadata.
use crate::printing::{validate_artifact_presentation, ArtifactPresentation};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Declaration {
    pub output_id: String,
    pub format: String,
    pub content_type: String,
    pub sha256: String,
    pub size_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paper_size: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation: Option<ArtifactPresentation>,
}
#[derive(Deserialize)]
struct Download {
    output: Declaration,
    file_name: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    engine: String,
    operation: String,
    outputs: Vec<Declaration>,
    #[serde(default)]
    download: Option<Download>,
}
#[derive(Serialize)]
struct Inventory {
    outputs: Vec<Declaration>,
    file_names: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    download_output: Option<Declaration>,
}
fn id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// The published project head, independent of machine-local rooms or a shell.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Head {
    pub work_id: String,
    pub engine: String,
    pub operation: String,
    pub variant: String,
    pub input_base_fingerprint: String,
    pub head_revision: u64,
    pub updated_at: String,
    pub outputs: Vec<Declaration>,
}

pub fn valid_operation(engine: &str, operation: &str) -> bool {
    if engine == "solar" {
        ["calculate-", "report-", "portfolio-"]
            .iter()
            .any(|prefix| operation.strip_prefix(prefix).is_some_and(id))
    } else {
        id(operation)
    }
}

/// Validate the safe discovery projection; unknown additive server metadata
/// is allowed, but malformed identities and output declarations are refused.
pub fn evaluate_head(input: &[u8]) -> Result<String, String> {
    if input.is_empty() || input.len() > 512 * 1024 {
        return Err("Artifact head exceeds its byte bound.".into());
    }
    let head: Head = serde_json::from_slice(input)
        .map_err(|_| "Compute artifact response has an invalid head.".to_string())?;
    if !matches!(
        head.engine.as_str(),
        "solar" | "print" | "vector_tiler" | "network_reporter"
    ) || !id(&head.work_id)
        || !valid_operation(&head.engine, &head.operation)
        || !id(&head.variant)
        || head.input_base_fingerprint.len() != 64
        || !head
            .input_base_fingerprint
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || head.head_revision == 0
        || head.head_revision > 9_007_199_254_740_991
        || time::OffsetDateTime::parse(
            &head.updated_at,
            &time::format_description::well_known::Rfc3339,
        )
        .is_err()
    {
        return Err("Compute artifact response has an invalid head.".into());
    }
    // The same output owner validates inventory, head and downloads.
    evaluate(
        &serde_json::to_vec(&serde_json::json!({
            "engine": head.engine, "operation": head.operation, "outputs": head.outputs
        }))
        .map_err(|e| e.to_string())?,
    )?;
    serde_json::to_string(&head).map_err(|e| e.to_string())
}

fn mime(format: &str) -> Option<&'static str> {
    Some(match format {
        "csv" => "text/csv",
        "geojson" => "application/geo+json",
        "geojsonseq" | "geojsonzip" | "shp" | "zip" => "application/zip",
        "gpkg" => "application/geopackage+sqlite3",
        "json" => "application/json",
        "kmz" => "application/vnd.google-earth.kmz",
        "md" => "text/markdown",
        "pdf" | "pdf_a0" | "pdf_a3" => "application/pdf",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "pmtiles" => "application/vnd.pmtiles",
        "png" => "image/png",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        _ => return None,
    })
}
pub fn validate(engine: &str, output: &Declaration) -> Result<(), String> {
    let content_type = mime(&output.format).or_else(|| {
        (engine == "network_reporter")
            .then(|| crate::report_formats::output(&output.format).ok())
            .flatten()
            .map(|value| value.content_type)
    });
    if !id(&output.output_id)
        || content_type != Some(output.content_type.as_str())
        || output.sha256.len() != 64
        || !output
            .sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || output.size_bytes == 0
        || output.size_bytes > 9_007_199_254_740_991
        || (engine != "network_reporter"
            && (output.paper_size.is_some() || output.presentation.is_some()))
    {
        return Err("Compute artifact response has an invalid output declaration.".into());
    }
    validate_artifact_presentation(
        &output.format,
        output.paper_size.as_deref(),
        output.presentation.as_ref(),
    )
    .map_err(|_| "Compute artifact response has an invalid output declaration.".into())
}
pub fn filename(engine: &str, operation: &str, output: &Declaration) -> String {
    let stem = if engine == "network_reporter" {
        match operation {
            "export_combined_transformer_report" | "export-combined_transformer" => {
                "combined_transformer"
            }
            "export-compounded_report" => "compounded_reports",
            _ => operation
                .strip_prefix("export-")
                .filter(|v| id(v))
                .unwrap_or(&output.output_id),
        }
    } else {
        &output.output_id
    };
    if engine == "network_reporter"
        && crate::report_formats::named_print_output(&output.format).is_some()
    {
        // The print-format owner already owns the layout suffix and media type.
        return format!(
            "{stem}{}",
            crate::report_formats::output(&output.format)
                .expect("validated format")
                .suffix
        );
    }
    if engine == "network_reporter" && output.format == "geojsonseq" {
        return format!("{stem}_geojsonl.zip");
    }
    let extension = match output.format.as_str() {
        "pdf_a0" => "a0.pdf",
        "pdf_a3" => "a3.pdf",
        "shp" => "shp.zip",
        "geojsonzip" => "geojson.zip",
        "geojsonseq" => "geojsonl.zip",
        value => value,
    };
    format!("{stem}.{extension}")
}
pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.is_empty() || input.len() > 512 * 1024 {
        return Err("Artifact inventory exceeds its byte bound.".into());
    }
    let request: Request =
        serde_json::from_slice(input).map_err(|_| "Invalid artifact inventory response.")?;
    if !matches!(
        request.engine.as_str(),
        "solar" | "print" | "vector_tiler" | "network_reporter"
    ) || request.outputs.is_empty()
        || request.outputs.len() > 100
    {
        return Err("Invalid artifact inventory response.".into());
    }
    let mut ids = BTreeSet::new();
    let mut file_names = Vec::new();
    for output in &request.outputs {
        validate(&request.engine, output)?;
        if !ids.insert(&output.output_id) {
            return Err("Duplicate artifact output identity.".into());
        }
        file_names.push(filename(&request.engine, &request.operation, output));
    }
    if let Some(download) = &request.download {
        validate(&request.engine, &download.output)?;
        if !request.outputs.contains(&download.output) {
            return Err("Compute artifact response has an invalid download projection.".into());
        }
        if download.file_name.len() > 255
            || download.file_name != filename(&request.engine, &request.operation, &download.output)
        {
            return Err("Compute artifact response has an invalid file_name.".into());
        }
    }
    serde_json::to_string(&Inventory {
        outputs: request.outputs,
        file_names,
        download_output: request.download.map(|v| v.output),
    })
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn published_heads_accept_city_and_portfolio_families_without_local_rooms() {
        let mut head = json!({"work_id":"work_1", "engine":"solar", "operation":"portfolio-gisagara", "variant":"default",
            "input_base_fingerprint":"a".repeat(64), "head_revision":1, "updated_at":"2026-09-18T01:00:00Z",
            "engine_version":"additive server metadata", "outputs":[{"output_id":"summary", "format":"json", "content_type":"application/json", "sha256":"b".repeat(64), "size_bytes":1}]});
        for family in ["calculate", "report", "portfolio"] {
            head["operation"] = json!(format!("{family}-c{}", "x".repeat(127)));
            assert!(evaluate_head(&serde_json::to_vec(&head).unwrap()).is_ok());
            head["operation"] = json!(format!("{family}-c{}", "x".repeat(128)));
            assert!(evaluate_head(&serde_json::to_vec(&head).unwrap()).is_err());
        }
        head["operation"] = json!("portfolio-gisagara");
        for (field, invalid) in [
            ("head_revision", json!(0)),
            ("input_base_fingerprint", json!("bad")),
            ("updated_at", json!("yesterday")),
            ("outputs", json!([])),
            ("engine", json!("unknown")),
        ] {
            let mut malformed = head.clone();
            malformed[field] = invalid;
            assert!(
                evaluate_head(&serde_json::to_vec(&malformed).unwrap()).is_err(),
                "{field}"
            );
        }
    }
    #[test]
    fn named_prints_keep_metadata_and_canonical_names() {
        for (format, mime, suffix) in [
            ("pdf", "application/pdf", "pdf"),
            ("png", "image/png", "png"),
            ("jpeg", "image/jpeg", "jpg"),
        ] {
            let output = json!({"output_id":format!("{format}__huye"),"format":format!("{format}__huye"),"content_type":mime,"sha256":"b".repeat(64),"size_bytes":123,
                "paper_size":"A0","presentation":{"layout_id":"huye","layout_name":"Huye","paper_name":"A0","orientation":"landscape","page_mm":[1189,841]}});
            let mut input = json!({"engine":"network_reporter","operation":"export-agasharu","outputs":[output.clone()],"download":{"output":output,"file_name":format!("agasharu.huye.{suffix}")}});
            let result: serde_json::Value =
                serde_json::from_str(&evaluate(&serde_json::to_vec(&input).unwrap()).unwrap())
                    .unwrap();
            assert_eq!(
                result["outputs"][0]["presentation"]["orientation"],
                "landscape"
            );
            input["download"]["output"]["presentation"]["layout_name"] = json!("Changed");
            assert!(evaluate(&serde_json::to_vec(&input).unwrap())
                .unwrap_err()
                .contains("download projection"));
        }
    }
    #[test]
    fn refuses_duplicate_identity_invalid_layout_and_filename() {
        let output = json!({"output_id":"pdf__huye","format":"pdf__huye","content_type":"application/pdf","sha256":"b".repeat(64),"size_bytes":123});
        let mut input = json!({"engine":"network_reporter","operation":"export-agasharu","outputs":[output.clone(),output.clone()]});
        assert!(evaluate(&serde_json::to_vec(&input).unwrap()).is_err());
        input["outputs"] = json!([output.clone()]);
        input["download"] = json!({"output":output,"file_name":"../agasharu.pdf"});
        assert!(evaluate(&serde_json::to_vec(&input).unwrap())
            .unwrap_err()
            .contains("file_name"));
        input["outputs"][0]["paper_size"] = json!("A0");
        input["outputs"][0]["presentation"] = json!({"layout_id":"other","layout_name":"Wrong","paper_name":"A0","orientation":"landscape","page_mm":[1189,841]});
        assert!(evaluate(&serde_json::to_vec(&input).unwrap()).is_err());
    }
}
