//! Published artifact declarations and canonical names shared by every client.
//! The host transports responses; it never invents formats or drops layout metadata.
use crate::printing::{ArtifactPresentation, validate_artifact_presentation};
use crate::report_export::exact_lower_sha256;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Declaration {
    pub output_id: String,
    /// Exact native report receipt name; older report declarations require migration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
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

/// The run that produced one output of a head (ds-brain
/// `ComputeArtifactOutputOrigin`). A Network Reporter head keeps outputs of
/// several runs, so the head-level fingerprint and engine are the latest run's.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct OutputOrigin {
    pub work_id: String,
    pub input_base_fingerprint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine_build_manifest_sha256: Option<String>,
}

/// The published project head, independent of machine-local rooms or a shell.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Head {
    pub work_id: String,
    pub engine: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine_build_manifest_sha256: Option<String>,
    pub operation: String,
    pub variant: String,
    pub input_base_fingerprint: String,
    pub head_revision: u64,
    pub updated_at: String,
    pub outputs: Vec<Declaration>,
    /// Absent on single-run and older heads, which record no output's origin.
    #[serde(
        default,
        deserialize_with = "unique_origins",
        skip_serializing_if = "Option::is_none"
    )]
    pub output_origins: Option<BTreeMap<String, OutputOrigin>>,
}

impl Head {
    /// The recorded origin of one output. `None` when the head records none:
    /// the head-level fingerprint is never read as every output's origin.
    pub fn output_origin(&self, output_id: &str) -> Option<&OutputOrigin> {
        self.output_origins.as_ref()?.get(output_id)
    }
}

/// serde keeps the last of two equal JSON keys; a second origin for one
/// output is refused rather than silently chosen.
fn unique_origins<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<BTreeMap<String, OutputOrigin>>, D::Error> {
    struct Unique;
    impl<'de> serde::de::Visitor<'de> for Unique {
        type Value = Option<BTreeMap<String, OutputOrigin>>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("output origins keyed by output_id")
        }
        fn visit_none<E>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_some<D: serde::Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
            d.deserialize_map(self)
        }
        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut map: A,
        ) -> Result<Self::Value, A::Error> {
            let mut origins = BTreeMap::new();
            while let Some((output_id, origin)) = map.next_entry::<String, OutputOrigin>()? {
                if origins.insert(output_id, origin).is_some() {
                    return Err(serde::de::Error::custom("duplicate output origin"));
                }
            }
            Ok(Some(origins))
        }
    }
    deserializer.deserialize_option(Unique)
}

/// ds-brain records an engine build as a pair: both halves or neither.
fn engine_identity(version: Option<&str>, manifest: Option<&str>) -> bool {
    match (version, manifest) {
        (None, None) => true,
        (Some(version), Some(manifest)) => !version.is_empty() && exact_lower_sha256(manifest),
        _ => false,
    }
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
/// is allowed, but malformed identities, output declarations and output
/// origins are refused.
pub fn evaluate_head(input: &[u8]) -> Result<String, String> {
    serde_json::to_string(&decode_head(input)?).map_err(|e| e.to_string())
}

/// The validated typed head `evaluate_head` projects.
pub fn decode_head(input: &[u8]) -> Result<Head, String> {
    if input.is_empty() || input.len() > 512 * 1024 {
        return Err("Artifact head exceeds its byte bound.".into());
    }
    let head: Head = serde_json::from_slice(input)
        .map_err(|_| "Compute artifact response has an invalid head.".to_string())?;
    if !matches!(
        head.engine.as_str(),
        "solar" | "print" | "vector_tiler" | "network_reporter"
    ) || !id(&head.work_id)
        || !engine_identity(
            head.engine_version.as_deref(),
            head.engine_build_manifest_sha256.as_deref(),
        )
        || !valid_operation(&head.engine, &head.operation)
        || !id(&head.variant)
        || !exact_lower_sha256(&head.input_base_fingerprint)
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
    if let Some(origins) = &head.output_origins {
        // Every output names its producing run, and one run has one identity:
        // the head's own run included.
        let mut runs = BTreeMap::from([(
            head.work_id.as_str(),
            (
                head.input_base_fingerprint.as_str(),
                head.engine_version.as_deref(),
                head.engine_build_manifest_sha256.as_deref(),
            ),
        )]);
        let complete = origins.len() == head.outputs.len()
            && head
                .outputs
                .iter()
                .all(|output| origins.contains_key(&output.output_id));
        if !complete
            || !origins.values().all(|origin| {
                let identity = (
                    origin.input_base_fingerprint.as_str(),
                    origin.engine_version.as_deref(),
                    origin.engine_build_manifest_sha256.as_deref(),
                );
                id(&origin.work_id)
                    && exact_lower_sha256(identity.0)
                    && engine_identity(identity.1, identity.2)
                    && *runs.entry(origin.work_id.as_str()).or_insert(identity) == identity
            })
        {
            return Err("Compute artifact response has invalid output provenance.".into());
        }
    }
    Ok(head)
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
    let content_type = if engine == "network_reporter" {
        crate::report_formats::output(&output.format)
            .ok()
            .map(|value| value.content_type)
    } else {
        mime(&output.format)
    };
    if (engine == "network_reporter"
        && !output.filename.as_deref().is_some_and(|name| {
            !name.is_empty()
                && name.len() <= 255
                && !name.contains(['/', '\\'])
                && !name.chars().any(char::is_control)
                && name != "."
                && name != ".."
        }))
        || !id(&output.output_id)
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
    if engine == "network_reporter" {
        return crate::report_formats::report_filename(stem, &output.format)
            .expect("validated report format");
    }
    format!("{stem}.{}", output.format)
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
        let canonical = filename(&request.engine, &request.operation, output);
        if request.engine == "network_reporter" && output.filename.as_deref() != Some(&canonical) {
            return Err("Native report filename differs from the kernel naming contract.".into());
        }
        file_names.push(canonical);
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
            "engine_version":"solar-py-1", "engine_build_manifest_sha256":"c".repeat(64), "later_field":"additive server metadata",
            "outputs":[{"output_id":"summary", "format":"json", "content_type":"application/json", "sha256":"b".repeat(64), "size_bytes":1}]});
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
            ("engine_build_manifest_sha256", json!("C".repeat(64))),
            ("engine_version", json!("")),
        ] {
            let mut malformed = head.clone();
            malformed[field] = invalid;
            assert!(
                evaluate_head(&serde_json::to_vec(&malformed).unwrap()).is_err(),
                "{field}"
            );
        }
        // An engine build is recorded as a pair, as ds-brain serves it.
        for half in ["engine_version", "engine_build_manifest_sha256"] {
            let mut malformed = head.clone();
            malformed.as_object_mut().unwrap().remove(half);
            assert!(evaluate_head(&serde_json::to_vec(&malformed).unwrap()).is_err());
        }
    }

    fn reporter_head() -> serde_json::Value {
        let output = |id: &str, mime: &str| json!({"filename":crate::report_formats::report_filename("agasharu",id).unwrap(), "output_id":id, "format":id, "content_type":mime, "sha256":"b".repeat(64), "size_bytes":7});
        json!({"work_id":"work_next", "engine":"network_reporter", "engine_version":"ds-network-reporter@0.2.0",
            "engine_build_manifest_sha256":"e".repeat(64), "operation":"export-agasharu", "variant":"default",
            "input_base_fingerprint":"a".repeat(64), "head_revision":2, "updated_at":"2026-09-18T01:00:00Z",
            "outputs":[output("xlsx", "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"), output("gpkg", "application/geopackage+sqlite3")],
            "output_origins":{
                "xlsx":{"work_id":"work_prior", "input_base_fingerprint":"f".repeat(64), "engine_version":"ds-network-reporter@0.1.0", "engine_build_manifest_sha256":"d".repeat(64)},
                "gpkg":{"work_id":"work_next", "input_base_fingerprint":"a".repeat(64), "engine_version":"ds-network-reporter@0.2.0", "engine_build_manifest_sha256":"e".repeat(64)}}})
    }

    #[test]
    fn merged_heads_keep_each_output_origin_through_the_projection() {
        let bytes = serde_json::to_vec(&reporter_head()).unwrap();
        let head = decode_head(&bytes).unwrap();
        // The retained xlsx came from an earlier run: the head-level
        // fingerprint and engine are the latest run's, not the xlsx's.
        let xlsx = head.output_origin("xlsx").unwrap();
        assert_eq!(
            (xlsx.work_id.as_str(), xlsx.input_base_fingerprint.as_str()),
            ("work_prior", "f".repeat(64).as_str())
        );
        assert_ne!(xlsx.input_base_fingerprint, head.input_base_fingerprint);
        assert_eq!(
            xlsx.engine_build_manifest_sha256.as_deref(),
            Some("d".repeat(64).as_str())
        );
        assert_eq!(head.output_origin("gpkg").unwrap().work_id, "work_next");
        assert!(head.output_origin("pdf__huye").is_none());
        // The wire projection round-trips without losing an origin.
        let projected = evaluate_head(&bytes).unwrap();
        assert_eq!(decode_head(projected.as_bytes()).unwrap(), head);
        // A run recorded before engine builds were admitted has no build to match.
        let mut prior = reporter_head();
        prior["output_origins"]["xlsx"] =
            json!({"work_id":"work_prior", "input_base_fingerprint":"f".repeat(64)});
        let head = decode_head(&serde_json::to_vec(&prior).unwrap()).unwrap();
        assert!(
            head.output_origin("xlsx")
                .unwrap()
                .engine_build_manifest_sha256
                .is_none()
        );
    }

    #[test]
    fn older_heads_decode_without_inventing_an_origin() {
        let mut legacy = reporter_head();
        let object = legacy.as_object_mut().unwrap();
        for field in [
            "output_origins",
            "engine_version",
            "engine_build_manifest_sha256",
        ] {
            object.remove(field);
        }
        let projected = evaluate_head(&serde_json::to_vec(&legacy).unwrap()).unwrap();
        let wire: serde_json::Value = serde_json::from_str(&projected).unwrap();
        assert_eq!(wire, legacy, "an older head projects exactly as before");
        let head = decode_head(projected.as_bytes()).unwrap();
        assert!(head.output_origins.is_none());
        assert!(
            ["xlsx", "gpkg"]
                .iter()
                .all(|id| head.output_origin(id).is_none())
        );
        legacy["output_origins"] = json!(null);
        assert!(
            decode_head(&serde_json::to_vec(&legacy).unwrap())
                .unwrap()
                .output_origin("xlsx")
                .is_none()
        );
    }

    #[test]
    fn malformed_duplicate_or_contradicting_origins_refuse_the_head() {
        type Corrupt = fn(&mut serde_json::Value);
        let cases: [(&str, Corrupt); 11] = [
            ("empty", |h| h["output_origins"] = json!({})),
            ("missing output", |h| {
                h["output_origins"].as_object_mut().unwrap().remove("gpkg");
            }),
            ("undeclared output", |h| {
                h["output_origins"]["kmz"] = h["output_origins"]["gpkg"].clone()
            }),
            ("fingerprint", |h| {
                h["output_origins"]["xlsx"]["input_base_fingerprint"] = json!("F".repeat(64))
            }),
            ("work id", |h| {
                h["output_origins"]["xlsx"]["work_id"] = json!("../work")
            }),
            ("half engine", |h| {
                h["output_origins"]["xlsx"]
                    .as_object_mut()
                    .unwrap()
                    .remove("engine_build_manifest_sha256");
            }),
            ("manifest only", |h| {
                h["output_origins"]["xlsx"]
                    .as_object_mut()
                    .unwrap()
                    .remove("engine_version");
            }),
            ("manifest", |h| {
                h["output_origins"]["xlsx"]["engine_build_manifest_sha256"] = json!("bad")
            }),
            ("head run fingerprint", |h| {
                h["output_origins"]["gpkg"]["input_base_fingerprint"] = json!("f".repeat(64))
            }),
            ("head run engine", |h| {
                h["output_origins"]["gpkg"]["engine_build_manifest_sha256"] = json!("d".repeat(64))
            }),
            ("one run two bases", |h| {
                h["output_origins"]["gpkg"] = h["output_origins"]["xlsx"].clone();
                h["output_origins"]["gpkg"]["input_base_fingerprint"] = json!("0".repeat(64));
            }),
        ];
        for (name, corrupt) in cases {
            let mut head = reporter_head();
            corrupt(&mut head);
            assert!(
                decode_head(&serde_json::to_vec(&head).unwrap()).is_err(),
                "{name}"
            );
        }
        // Negative control: one earlier run that produced both outputs agrees with itself.
        let mut head = reporter_head();
        head["output_origins"]["gpkg"] = head["output_origins"]["xlsx"].clone();
        assert!(decode_head(&serde_json::to_vec(&head).unwrap()).is_ok());
        // Two origins for one output: neither is chosen.
        let wire = serde_json::to_string(&reporter_head()).unwrap();
        let gpkg = serde_json::to_string(&reporter_head()["output_origins"]["gpkg"]).unwrap();
        let duplicated = wire.replacen("\"gpkg\":{", &format!("\"xlsx\":{gpkg},\"gpkg\":{{"), 1);
        assert_ne!(duplicated, wire);
        assert!(decode_head(duplicated.as_bytes()).is_err());
        assert!(evaluate_head(duplicated.as_bytes()).is_err());
    }
    #[test]
    fn named_prints_keep_metadata_and_canonical_names() {
        for (format, mime, _suffix) in [
            ("pdf", "application/pdf", "pdf"),
            ("png", "image/png", "png"),
            ("jpeg", "image/jpeg", "jpg"),
        ] {
            let output = json!({"filename":crate::report_formats::report_filename("agasharu", &format!("{format}__huye")).unwrap(), "output_id":format!("{format}__huye"),"format":format!("{format}__huye"),"content_type":mime,"sha256":"b".repeat(64),"size_bytes":123,
                "paper_size":"A0","presentation":{"layout_id":"huye","layout_name":"Huye","paper_name":"A0","orientation":"landscape","page_mm":[1189,841]}});
            let mut input = json!({"engine":"network_reporter","operation":"export-agasharu","outputs":[output.clone()],"download":{"output":output,"file_name":crate::report_formats::report_filename("agasharu", &format!("{format}__huye")).unwrap()}});
            let result: serde_json::Value =
                serde_json::from_str(&evaluate(&serde_json::to_vec(&input).unwrap()).unwrap())
                    .unwrap();
            assert_eq!(
                result["outputs"][0]["presentation"]["orientation"],
                "landscape"
            );
            input["download"]["output"]["presentation"]["layout_name"] = json!("Changed");
            assert!(
                evaluate(&serde_json::to_vec(&input).unwrap())
                    .unwrap_err()
                    .contains("download projection")
            );
        }
    }
    #[test]
    fn refuses_duplicate_identity_invalid_layout_and_filename() {
        let output = json!({"filename":"Agasharu.huye.pdf","output_id":"pdf__huye","format":"pdf__huye","content_type":"application/pdf","sha256":"b".repeat(64),"size_bytes":123});
        let mut input = json!({"engine":"network_reporter","operation":"export-agasharu","outputs":[output.clone(),output.clone()]});
        assert!(evaluate(&serde_json::to_vec(&input).unwrap()).is_err());
        input["outputs"] = json!([output.clone()]);
        input["download"] = json!({"output":output,"file_name":"../agasharu.pdf"});
        assert!(
            evaluate(&serde_json::to_vec(&input).unwrap())
                .unwrap_err()
                .contains("file_name")
        );
        input["outputs"][0]["paper_size"] = json!("A0");
        input["outputs"][0]["presentation"] = json!({"layout_id":"other","layout_name":"Wrong","paper_name":"A0","orientation":"landscape","page_mm":[1189,841]});
        assert!(evaluate(&serde_json::to_vec(&input).unwrap()).is_err());
    }
}
