//! Closed publication declarations, shared by native credentials and hosts.
//! The engine table selects declaration policy; every engine uses the same
//! project fence, output vocabulary and open/finalize sequence.
use super::engines::{engine, PublicationPolicy};
use crate::{compute_artifact_inventory as inventory, report_export::exact_lower_sha256};
use serde_json::{Map, Value};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Open,
    Finalize,
}

pub fn validate(project: &str, body: &Value) -> Result<Action, String> {
    let object = body.as_object().ok_or("publication must be an object")?;
    let text = |key| text(object, key);
    if !id(project) || text("project_id")? != project {
        return Err("publication project is not the selected project".into());
    }
    if !id(text("client_run_id")?) {
        return Err("invalid publication identity".into());
    }
    match text("action")? {
        "finalize" => {
            if object.len() != 4 || !id(text("work_id")?) {
                return Err("invalid finalize declaration".into());
            }
            Ok(Action::Finalize)
        }
        "open" => {
            const BASE: &[&str] = &[
                "action",
                "project_id",
                "client_run_id",
                "engine",
                "operation",
                "variant",
                "engine_version",
                "engine_build_manifest_sha256",
                "input_base_fingerprint",
                "outputs",
            ];
            if !BASE.iter().all(|key| object.contains_key(*key))
                || !object.keys().all(|key| {
                    BASE.contains(&key.as_str())
                        || [
                            "source_snapshot_sha256",
                            "snapshot_receipt_id",
                            "printing_preview",
                        ]
                        .contains(&key.as_str())
                })
            {
                return Err("publication has missing or undeclared fields".into());
            }
            let engine = engine(text("engine")?).ok_or("unregistered publication engine")?;
            let operation = text("operation")?;
            let variant = text("variant")?;
            let version = text("engine_version")?;
            let manifest = text("engine_build_manifest_sha256")?;
            if !id(operation)
                || !id(variant)
                || !bounded(version, 200)
                || (!manifest.is_empty() && !exact_lower_sha256(manifest))
                || !exact_lower_sha256(text("input_base_fingerprint")?)
            {
                return Err("invalid publication provenance".into());
            }
            let outputs = object
                .get("outputs")
                .and_then(Value::as_array)
                .filter(|outputs| (1..=100).contains(&outputs.len()))
                .ok_or("publication must declare 1..100 outputs")?;
            let mut ids = BTreeSet::new();
            let mut declarations = Vec::new();
            for output in outputs {
                let fields = output.as_object().ok_or("output must be an object")?;
                const KEYS: &[&str] = &[
                    "output_id",
                    "format",
                    "content_type",
                    "sha256",
                    "size_bytes",
                    "filename",
                    "paper_size",
                    "presentation",
                ];
                if !fields.keys().all(|key| KEYS.contains(&key.as_str()))
                    || ["filename", "paper_size"]
                        .iter()
                        .any(|key| fields.get(*key).is_some_and(|value| !value.is_string()))
                    || fields
                        .get("presentation")
                        .is_some_and(|value| !value.is_object())
                {
                    return Err("output has undeclared or invalid metadata".into());
                }
                let output: inventory::Declaration = serde_json::from_value(output.clone())
                    .map_err(|_| "invalid output declaration")?;
                inventory::validate(engine.name, &output)?;
                if output.size_bytes > 64 * 1024 * 1024
                    || output
                        .filename
                        .as_deref()
                        .is_some_and(|name| !bounded(name, 255))
                {
                    return Err("output exceeds the publication boundary".into());
                }
                if !ids.insert(output.output_id.clone()) {
                    return Err("duplicate output identity".into());
                }
                declarations.push(output);
            }
            match engine.publication {
                PublicationPolicy::Report => {
                    if operation
                        .strip_prefix("export-")
                        .is_none_or(|resource| !id(resource))
                        || object.contains_key("source_snapshot_sha256")
                        || object.contains_key("snapshot_receipt_id")
                    {
                        return Err("invalid report publication".into());
                    }
                    if engine.retained_preview_variant == Some(variant) {
                        let binding: crate::printing::retained_preview::Binding =
                            serde_json::from_value(
                                object
                                    .get("printing_preview")
                                    .ok_or("preview binding missing")?
                                    .clone(),
                            )
                            .map_err(|_| "invalid preview binding")?;
                        if binding.input_base_fingerprint(project, &operation[7..])?
                            != text("input_base_fingerprint")?
                            || declarations.len() != 1
                            || declarations[0].output_id != "voltage_drop_pdf"
                            || declarations[0].format != "voltage_drop_pdf"
                            || declarations[0].presentation.is_some()
                        {
                            return Err("preview differs from its sealed binding".into());
                        }
                    } else if variant != "default" || object.contains_key("printing_preview") {
                        return Err("invalid report variant".into());
                    }
                }
                PublicationPolicy::Solar => {
                    if object.contains_key("printing_preview")
                        || !inventory::valid_operation(engine.name, operation)
                    {
                        return Err("invalid Solar publication".into());
                    }
                    if operation.starts_with("calculate-") {
                        if variant != "default"
                            || text("client_run_id")?.len() > 80
                            || !version.starts_with("ds-solar-engine@")
                            || !exact_lower_sha256(manifest)
                            || !exact_lower_sha256(text("source_snapshot_sha256")?)
                            || !snapshot_receipt(text("snapshot_receipt_id")?)
                            || declarations.len() != 1
                            || declarations[0].output_id != "report-input"
                            || declarations[0].format != "json"
                            || declarations[0].size_bytes
                                > super::solar_declaration::REPORT_INPUT_MAX_BYTES
                        {
                            return Err(
                                "calculation differs from its sealed snapshot and report-input"
                                    .into(),
                            );
                        }
                    } else if object.contains_key("source_snapshot_sha256")
                        || object.contains_key("snapshot_receipt_id")
                        || (operation.starts_with("report-") && variant == "default")
                        || (operation.starts_with("portfolio-")
                            && !matches!(variant, "aggregate-v2" | "aggregate-v3"))
                    {
                        return Err("invalid derived artifact provenance or variant".into());
                    }
                    if !operation.starts_with("calculate-") {
                        super::solar_declaration::derived(operation, variant, &declarations)?;
                    }
                }
            }
            Ok(Action::Open)
        }
        _ => Err("publication action must be open or finalize".into()),
    }
}

fn text<'a>(object: &'a Map<String, Value>, key: &str) -> Result<&'a str, String> {
    object
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("publication field {key} must be text"))
}
fn bounded(value: &str, limit: usize) -> bool {
    !value.is_empty()
        && value.len() <= limit
        && value.trim() == value
        && !value.chars().any(char::is_control)
}
fn id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}
fn snapshot_receipt(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => *byte == b'-',
            14 => matches!(*byte, b'1'..=b'5'),
            19 => matches!(*byte, b'8' | b'9' | b'a' | b'b'),
            _ => byte.is_ascii_digit() || matches!(*byte, b'a'..=b'f'),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn open(policy: PublicationPolicy) -> Value {
        let engine = match policy {
            PublicationPolicy::Report => super::super::engines::report_engine(),
            PublicationPolicy::Solar => super::super::engines::solar_engine(),
        };
        let mut body = json!({
            "action":"open", "project_id":"project-a", "client_run_id":"job-a",
            "engine":engine.name, "operation":"export-city-a", "variant":"default",
            "engine_version":"native-engine@1.2.3", "engine_build_manifest_sha256":"a".repeat(64),
            "input_base_fingerprint":"b".repeat(64),
            "outputs":[{"output_id":"xlsx", "format":"xlsx", "filename":"city-a.xlsx",
                "content_type":"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
                "sha256":"c".repeat(64), "size_bytes":42}],
        });
        if matches!(policy, PublicationPolicy::Solar) {
            body["operation"] = json!("calculate-city-a");
            body["engine_version"] = json!("ds-solar-engine@1.2.3");
            body["source_snapshot_sha256"] = json!("d".repeat(64));
            body["snapshot_receipt_id"] = json!("21c655a1-1037-52ce-a601-e9c7e3734d53");
            body["outputs"] = json!([{"output_id":"report-input", "format":"json",
                "content_type":"application/json", "sha256":"c".repeat(64), "size_bytes":42}]);
        }
        body
    }

    #[test]
    fn every_engine_uses_the_same_project_identity_and_output_fences() {
        for policy in [PublicationPolicy::Report, PublicationPolicy::Solar] {
            let body = open(policy);
            assert_eq!(validate("project-a", &body), Ok(Action::Open));
            assert!(validate("project-b", &body).is_err());
            for (field, invalid) in [
                ("input_base_fingerprint", json!("")),
                ("engine", json!("unregistered")),
                ("client_run_id", json!("../job")),
                ("authority_grant_id", json!("grant-a")),
                ("outputs", json!([])),
            ] {
                let mut changed = body.clone();
                changed[field] = invalid;
                assert!(
                    validate("project-a", &changed).is_err(),
                    "{field}: {changed}"
                );
            }
            for (field, invalid) in [
                ("sha256", json!("c".repeat(63))),
                ("size_bytes", json!(0)),
                ("size_bytes", json!(64 * 1024 * 1024 + 1_u64)),
                ("content_type", json!("text/plain")),
                ("session_uri", json!("https://example.invalid")),
            ] {
                let mut changed = body.clone();
                changed["outputs"][0][field] = invalid;
                assert!(
                    validate("project-a", &changed).is_err(),
                    "{field}: {changed}"
                );
            }
            let mut duplicates = body.clone();
            duplicates["outputs"]
                .as_array_mut()
                .unwrap()
                .push(body["outputs"][0].clone());
            assert!(validate("project-a", &duplicates).is_err());
        }
    }

    #[test]
    fn snapshot_facts_are_mandatory_only_on_the_calculation_declaration() {
        let body = open(PublicationPolicy::Solar);
        for field in ["source_snapshot_sha256", "snapshot_receipt_id"] {
            let mut changed = body.clone();
            changed.as_object_mut().unwrap().remove(field);
            assert!(validate("project-a", &changed).is_err());
            changed[field] = json!("invalid");
            assert!(validate("project-a", &changed).is_err());
        }
        let mut report = open(PublicationPolicy::Report);
        report["source_snapshot_sha256"] = body["source_snapshot_sha256"].clone();
        assert!(validate("project-a", &report).is_err());
        let mut derived = body.clone();
        derived["operation"] = json!("report-city-a");
        derived["variant"] = json!("apd-draft-en");
        derived["outputs"] = json!([{"output_id":"apd-draft-en","format":"md","content_type":"text/markdown","sha256":"c".repeat(64),"size_bytes":42}]);
        assert!(validate("project-a", &derived).is_err());
        derived
            .as_object_mut()
            .unwrap()
            .remove("source_snapshot_sha256");
        derived
            .as_object_mut()
            .unwrap()
            .remove("snapshot_receipt_id");
        assert_eq!(validate("project-a", &derived), Ok(Action::Open));
    }

    #[test]
    fn finalize_is_one_closed_identity_for_every_engine() {
        let body = json!({"action":"finalize", "project_id":"project-a", "work_id":"work-a", "client_run_id":"job-a"});
        assert_eq!(validate("project-a", &body), Ok(Action::Finalize));
        assert!(validate("project-b", &body).is_err());
        for field in ["work_id", "client_run_id"] {
            let mut changed = body.clone();
            changed[field] = json!("../work");
            assert!(validate("project-a", &changed).is_err());
        }
        let mut changed = body;
        changed["engine"] = json!("unregistered");
        assert!(validate("project-a", &changed).is_err());
    }
    #[test]
    fn portfolio_inventory_uses_the_same_hundred_output_bound_as_brain() {
        let mut body = open(PublicationPolicy::Solar);
        body["operation"] = json!("portfolio-a");
        body["variant"] = json!("aggregate-v3");
        body.as_object_mut()
            .unwrap()
            .remove("source_snapshot_sha256");
        body.as_object_mut().unwrap().remove("snapshot_receipt_id");
        body["outputs"] =
            json!((0..100).map(|index| json!({
            "output_id":format!("plot-chart-{index}"),"format":"png","content_type":"image/png",
            "sha256":"c".repeat(64),"size_bytes":42
        })).collect::<Vec<_>>());
        body["outputs"][0] = json!({"output_id":"portfolio-result","format":"json","content_type":"application/json","sha256":"c".repeat(64),"size_bytes":42});
        assert_eq!(validate("project-a", &body).unwrap(), Action::Open);
        let extra = json!({"output_id":"plot-extra","format":"png","content_type":"image/png","sha256":"c".repeat(64),"size_bytes":42});
        body["outputs"].as_array_mut().unwrap().push(extra);
        assert!(validate("project-a", &body).is_err());
    }
    #[test]
    fn solar_report_and_portfolio_outputs_match_the_brain_admission_vocabulary() {
        let mut body = open(PublicationPolicy::Solar);
        body.as_object_mut()
            .unwrap()
            .remove("source_snapshot_sha256");
        body.as_object_mut().unwrap().remove("snapshot_receipt_id");
        body["operation"] = json!("report-city-a");
        body["variant"] = json!("network-draft-en");
        let draft = json!({"output_id":"network-draft-en","format":"md","content_type":"text/markdown","sha256":"c".repeat(64),"size_bytes":42});
        let plot = json!({"output_id":"plot-site-load","format":"png","content_type":"image/png","sha256":"c".repeat(64),"size_bytes":42});
        body["outputs"] = json!([draft, plot]);
        assert_eq!(validate("project-a", &body), Ok(Action::Open));
        body["variant"] = json!("plant-draft-en");
        assert!(validate("project-a", &body).is_err());
        body["variant"] = json!("apd-final-en");
        body["outputs"][0]["output_id"] = json!("apd-final-en");
        assert!(validate("project-a", &body).is_err());
        body["outputs"].as_array_mut().unwrap().pop();
        assert_eq!(validate("project-a", &body), Ok(Action::Open));
        for variant in [
            "apd-final-fr",
            "apd-final-en",
            "network-final-fr",
            "network-final-en",
            "plant-final-fr",
            "plant-final-en",
            "financial-final-fr",
            "financial-final-en",
        ] {
            body["variant"] = json!(variant);
            body["outputs"][0]["output_id"] = json!(variant);
            assert_eq!(validate("project-a", &body), Ok(Action::Open));
            body["outputs"][0]["output_id"] = json!("apd-draft-fr");
            assert!(validate("project-a", &body).is_err());
        }
        body["operation"] = json!("portfolio-a");
        body["variant"] = json!("aggregate-v3");
        assert!(validate("project-a", &body).is_err());
        body["outputs"] = json!([{"output_id":"portfolio-result","format":"json","content_type":"application/json","sha256":"c".repeat(64),"size_bytes":8<<20}]);
        assert_eq!(validate("project-a", &body), Ok(Action::Open));
        body["outputs"][0]["size_bytes"] = json!((8 << 20) + 1);
        assert!(validate("project-a", &body).is_err());
    }
    #[test]
    fn calculation_bulk_reserves_the_same_ingress_bytes_as_brain() {
        let mut body = open(PublicationPolicy::Solar);
        body["outputs"][0]["size_bytes"] = json!((8 << 20) - (1 << 10));
        assert_eq!(validate("project-a", &body), Ok(Action::Open));
        body["outputs"][0]["size_bytes"] = json!((8 << 20) - (1 << 10) + 1);
        assert!(validate("project-a", &body).is_err());
    }
}
