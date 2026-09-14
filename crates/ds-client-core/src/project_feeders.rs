//! Fresh project configuration and bounded feeder-setting updates.
use crate::{ClientError, ErrorKind, TransportResponse};
use serde_json::{json, Value};

pub const MINIMUM: &str = "minimum_feeder_cable_size";
pub const MAXIMUM: &str = "max_feeder_cable_size";

#[derive(Debug, Clone)]
pub enum ProjectConfigurationChange {
    /// Fetch Settings without requiring the unrelated feeder catalog.
    ReadSettings,
    SaveSheet {
        sheet: String,
        value: Value,
    },
    SetParameter {
        sheet: String,
        parameter: String,
        raw: Value,
    },
    DuplicateRuleSet {
        sheet: String,
        source: String,
        target: String,
    },
    FeederLimits {
        minimum: f64,
        maximum: f64,
    },
    EnsureMeterType {
        name: String,
    },
    CustomerCategoryAlias {
        alias: String,
        category: String,
    },
    /// The project's whole `project_settings` sheet, already carrying the
    /// design output selection its author chose. The rows are the caller's
    /// because the kernel writes them — `report_formats::apply_output_selection`
    /// resolves the export row through every alias and creates it when the
    /// sheet has none — and this variant is the only door from that answer to
    /// a settings save, so no caller can reach `settings: Some(rows)` with
    /// rows nothing validated.
    DesignOutputs {
        rows: Vec<Value>,
    },
}

pub(crate) fn customer_category_alias(
    document: &Value,
    alias: &str,
    category: &str,
) -> Result<Vec<Value>, ClientError> {
    for name in [alias, category] {
        if name.trim() != name
            || name.is_empty()
            || name.len() > 100
            || name.chars().any(char::is_control)
        {
            return Err(ClientError::new(
                ErrorKind::InvalidInput,
                "category names must be trimmed and between 1 and 100 bytes",
            ));
        }
    }
    let mut rows = document["sheets"]["cust_category"]
        .as_array()
        .filter(|rows| {
            rows.len() < 1000
                && rows.iter().all(|row| {
                    row.is_object()
                        && (row["clean_name"].is_string() || row["clean_name"].is_null())
                })
        })
        .ok_or_else(invalid_response)?
        .clone();
    let index = rows
        .iter()
        .position(|row| {
            row["clean_name"]
                .as_str()
                .is_some_and(|v| v.eq_ignore_ascii_case(category))
        })
        .ok_or_else(|| {
            ClientError::new(
                ErrorKind::InvalidInput,
                "target customer category does not exist",
            )
        })?;
    let mut already_bound = false;
    for (i, row) in rows.iter().enumerate() {
        let mut names = ds_command_kernel::design_config::alias_list(&row["misspelled_names"]);
        if let Some(clean) = row["clean_name"].as_str() {
            names.push(clean.to_owned());
        }
        if names.iter().any(|name| name.eq_ignore_ascii_case(alias)) {
            if i == index {
                already_bound = true;
                continue;
            }
            return Err(ClientError::new(
                ErrorKind::InvalidInput,
                "alias already belongs to another customer category",
            ));
        }
    }
    let mut aliases =
        ds_command_kernel::design_config::alias_list(&rows[index]["misspelled_names"]);
    if !already_bound {
        aliases.push(alias.to_owned());
    }
    rows[index]["misspelled_names"] = json!(aliases);
    Ok(rows)
}

pub(crate) fn ensure_meter_type(document: &Value, name: &str) -> Result<Vec<Value>, ClientError> {
    if name.trim() != name
        || name.is_empty()
        || name.len() > 100
        || name.chars().any(char::is_control)
    {
        return Err(ClientError::new(
            ErrorKind::InvalidInput,
            "meter type must be a nonblank trimmed name of at most 100 bytes",
        ));
    }
    let mut rows = document["sheets"]["cust_meter_type"]
        .as_array()
        .filter(|rows| rows.len() < 1000 && rows.iter().all(|row| row["clean_name"].is_string()))
        .ok_or_else(invalid_response)?
        .clone();
    if !rows.iter().any(|row| {
        row["clean_name"]
            .as_str()
            .is_some_and(|v| v.eq_ignore_ascii_case(name))
    }) {
        rows.push(json!({"clean_name":name,"misspelled_names":[name]}));
    }
    Ok(rows)
}

/// What this client will accept as a design-output settings sheet: the row
/// shape and bounds the planner reads with, and an export row that actually
/// holds a readable selection. A save that decides nothing is a save that
/// should not have been sent.
pub(crate) fn design_outputs(rows: &[Value]) -> Result<(), ClientError> {
    use ds_command_kernel::report_formats::{output_setting_index, stored_output_selection};
    ds_command_kernel::report_formats::settings_rows_for_patch(rows).map_err(|_| {
        ClientError::new(
            ErrorKind::InvalidInput,
            "design output settings must be at most 10,000 rows, each an object with a text parameter",
        )
    })?;
    let index = output_setting_index(rows).ok_or_else(|| {
        ClientError::new(
            ErrorKind::InvalidInput,
            "design output settings must carry the project's export selection row",
        )
    })?;
    stored_output_selection(&rows[index]["value"]).map_err(|_| {
        ClientError::new(
            ErrorKind::InvalidInput,
            "the design export row does not hold a readable output selection",
        )
    })?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct FeederConfiguration {
    pub document: Value,
    pub summary: Value,
}

pub(crate) fn decode(response: TransportResponse) -> Result<Value, ClientError> {
    use ds_command_kernel::project_config::{admit, Facts, ResponseStatus, Verdict};
    let decision = admit(&Facts {
        cache_present: false,
        cached_generation: 0,
        current_generation: 0,
        scope_matches: true,
        invalidation_pending: false,
        response_status: if response.status == 200 {
            ResponseStatus::Success
        } else {
            ResponseStatus::Failed
        },
        fetched_present: response.status == 200,
        wait_expired: false,
        online: true,
    });
    if decision.verdict != Verdict::ServeFetched {
        // The class alone was not actionable. `auth_transient` with no status
        // and no server sentence is what a 502 from the configuration route
        // looked like on 2026-09-10: indistinguishable from an expired
        // credential, and nothing to retry against. The status and ds-brain's
        // own bounded sentence travel with the refusal.
        let kind = match response.status {
            500..=599 | 429 => ErrorKind::Transient,
            _ => ErrorKind::AuthenticationRejected,
        };
        const MESSAGE: &str =
            "project configuration request was refused; no cached configuration is substituted";
        let envelope: Option<Value> = serde_json::from_slice(&response.body).ok();
        let envelope = envelope.unwrap_or(Value::Null);
        let refusal = crate::ServiceRefusal::new(
            response.status,
            envelope["error"]["code"].as_str(),
            envelope["error"]["message"]
                .as_str()
                .or_else(|| envelope["detail"].as_str()),
        );
        return Err(ClientError::with_service_refusal(kind, MESSAGE, refusal));
    }
    let mut value: Value =
        serde_json::from_slice(&response.body).map_err(|_| invalid_response())?;
    if value.get("success") == Some(&Value::Bool(false)) {
        return Err(invalid_response());
    }
    if value.get("data").is_some() {
        value = value["data"].take();
    }
    Ok(value)
}

pub(crate) fn rows(doc: &Value) -> Result<&Vec<Value>, ClientError> {
    doc["sheets"]["project_settings"]
        .as_array()
        .filter(|rows| {
            rows.len() <= 10_000
                && rows
                    .iter()
                    .all(|row| row.is_object() && row["parameter"].is_string())
        })
        .ok_or_else(invalid_response)
}

fn key(row: &Value) -> String {
    ds_command_kernel::design_config::canonical_parameter_name(
        row["parameter"].as_str().unwrap_or_default(),
    )
}

pub(crate) fn validate(minimum: f64, maximum: f64) -> Result<(), ClientError> {
    if !minimum.is_finite() || !maximum.is_finite() || minimum <= 0.0 || minimum > maximum {
        return Err(ClientError::new(
            ErrorKind::InvalidInput,
            "feeder limits must be positive finite mm2 values with minimum at most maximum",
        ));
    }
    Ok(())
}

pub(crate) fn updated(
    rows: &[Value],
    minimum: f64,
    maximum: f64,
) -> Result<Vec<Value>, ClientError> {
    validate(minimum, maximum)?;
    let mut out = Vec::new();
    let mut seen = [false; 2];
    for row in rows {
        if let Some(index) = [MINIMUM, MAXIMUM].iter().position(|name| *name == key(row)) {
            if !seen[index] {
                let mut row = row.clone();
                row["parameter"] = json!([MINIMUM, MAXIMUM][index]);
                row["value"] = json!([minimum, maximum][index]);
                out.push(row);
                seen[index] = true;
            }
        } else {
            out.push(row.clone());
        }
    }
    for (index, parameter) in [MINIMUM, MAXIMUM].into_iter().enumerate() {
        if !seen[index] {
            out.push(json!({"parameter":parameter,"value":([minimum,maximum][index])}));
        }
    }
    Ok(out)
}

pub(crate) fn snapshot(
    project: &str,
    document: Value,
    saved: bool,
) -> Result<FeederConfiguration, ClientError> {
    let rows = rows(&document)?;
    let value = |name| {
        rows.iter()
            .find(|row| key(row) == name)
            .map(|row| row["value"].clone())
            .unwrap_or(Value::Null)
    };
    let catalog = document["sheets"]["transfo_sizes"]
        .as_array()
        .ok_or_else(invalid_response)?;
    let summary = json!({"project":project,"minimum":value(MINIMUM),"maximum":value(MAXIMUM),
        "lv_minimum":value("minimum_cable_size"),"lv_maximum":value("max_cable_size"),"saved":saved,
        "catalog":catalog.iter().take(200).map(|row| json!({"transformer_kva":row["transfo_sizes"],"lv_cable":row["lv_cable_sizes_bundles"],"feeder_cable":row["standard_cables_size_transfo_to_db"]})).collect::<Vec<_>>(),
        "catalog_omitted":catalog.len().saturating_sub(200),
        "meter_types":document["sheets"]["cust_meter_type"].as_array().map(|rows| rows.iter().take(200).map(|row| row["clean_name"].clone()).collect::<Vec<_>>())});
    Ok(FeederConfiguration { document, summary })
}

fn invalid_response() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "project configuration did not return complete valid settings and transformer catalog rows",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn meter_and_customer_catalog_updates_preserve_existing_semantics() {
        let doc = json!({"sheets":{
            "cust_meter_type":[{"clean_name":"Single Phase"},{"clean_name":"Three Phase"}],
            "cust_category":[{"clean_name":"Residential","peak_power_w":250},{"clean_name":"Commercial","peak_power_w":600,"misspelled_names":["Bar"]},{"clean_name":null,"misspelled_names":[]}]
        }});
        let meters = ensure_meter_type(&doc, "Readyboard").unwrap();
        assert_eq!(meters.len(), 3);
        assert_eq!(meters[0], doc["sheets"]["cust_meter_type"][0]);
        assert_eq!(meters[2]["clean_name"], "Readyboard");
        let categories = customer_category_alias(&doc, "Productive", "Commercial").unwrap();
        assert_eq!(categories[0], doc["sheets"]["cust_category"][0]);
        assert_eq!(categories[1]["peak_power_w"], 600);
        assert_eq!(
            categories[1]["misspelled_names"],
            json!(["Bar", "Productive"])
        );
        assert_eq!(categories[2], doc["sheets"]["cust_category"][2]);
        assert!(customer_category_alias(&doc, "Residential", "Commercial").is_err());
        assert!(customer_category_alias(&doc, "Productive", "Missing").is_err());
        let mut saved = doc;
        saved["sheets"]["cust_meter_type"] = json!(meters);
        saved["sheets"]["cust_category"] = json!(categories);
        assert_eq!(ensure_meter_type(&saved, "Readyboard").unwrap(), meters);
        assert_eq!(
            customer_category_alias(&saved, "Productive", "Commercial").unwrap(),
            categories
        );
        saved["sheets"]["cust_category"][2] =
            json!({"clean_name":"Industry","misspelled_names":["Productive"]});
        assert!(customer_category_alias(&saved, "Productive", "Commercial").is_err());
    }

    /// The closed variant refuses a sheet that decides nothing: no export row,
    /// or one whose value no reader can turn into a selection. Both are saves
    /// that would leave the project exactly as unreadable as before.
    #[test]
    fn design_output_rows_must_carry_a_readable_export_selection() {
        let selection = json!({"schema":"ds.design-output-selection/v1","prints":[],
            "geospatial":["gpkg"],"tabular":["xlsx"],"execution":{"gpkg":["web"]}});
        for alias in [
            "design_export_format",
            "REPORT-FORMATS",
            "tr_export_formats",
        ] {
            assert!(design_outputs(&[
                json!({"parameter":"unrelated","value":1}),
                json!({"parameter":alias,"value":selection}),
            ])
            .is_ok());
        }
        // A legacy token list is still a selection this client may save.
        assert!(
            design_outputs(&[json!({"parameter":"report_formats","value":"gpkg,xlsx"})]).is_ok()
        );
        assert!(design_outputs(&[json!({"parameter":"unrelated","value":1})]).is_err());
        assert!(design_outputs(&[json!({"parameter":"design_export_format",
            "value":{"schema":"ds.design-output-selection/v1","prints":"no"}})])
        .is_err());
        assert!(design_outputs(&[json!(["not", "a", "row"])]).is_err());
    }

    /// The request a native settings save sends is the browser's request. The
    /// field names are the claim: `action`, `eds_project_id`, `sheet_name`,
    /// `data` and nothing else — `ds-web/src/lib/api/design.ts:2119-2125`.
    #[test]
    fn a_native_settings_save_sends_the_browsers_request() {
        let rows = vec![json!({"parameter":"design_export_format",
            "value":{"schema":"ds.design-output-selection/v1","geospatial":["gpkg"]}})];
        let body: Value = serde_json::from_slice(&crate::transport::save_config_body(
            "aderm_loc7",
            "project_settings",
            &rows,
        ))
        .expect("the save body is JSON");
        assert_eq!(
            body,
            json!({"action":"save_config","eds_project_id":"aderm_loc7",
                "sheet_name":"project_settings","data":rows})
        );
        assert_eq!(
            body.as_object().expect("object").keys().collect::<Vec<_>>(),
            ["action", "eds_project_id", "sheet_name", "data"],
            "the native save must send the browser's fields, in its order, and no other"
        );
    }

    #[test]
    fn bounds_preserve_other_rows_and_metadata() {
        let rows = vec![
            json!({"parameter":"max_cable_size","value":"3 x 70"}),
            json!({"parameter":"Max Feeder Cable Size","value":120,"note":"BOQ"}),
        ];
        let updated = updated(&rows, 25.0, 95.0).unwrap();
        assert_eq!(updated[0], rows[0]);
        assert_eq!(
            updated[1],
            json!({"parameter":MAXIMUM,"value":95.0,"note":"BOQ"})
        );
        assert_eq!(updated[2], json!({"parameter":MINIMUM,"value":25.0}));
        assert!(validate(96.0, 95.0).is_err());
        assert!(validate(0.0, 95.0).is_err());
        assert!(validate(25.0, f64::NAN).is_err());
    }

    /// `report.project.outputs.set` answered `auth_transient` with one static
    /// sentence and nothing else — no status, no server message — so a 502
    /// from the configuration route was indistinguishable from an expired
    /// credential, and an operator had nothing to act on. The class is
    /// unchanged; what travels with it is not.
    #[test]
    fn a_refused_configuration_read_carries_its_status_and_the_service_sentence() {
        let body = br#"{"success":false,"service":"data-solutions-backend","version":"1.0.0","error":{"code":"internal_error","message":"project configuration store unavailable","timestamp":1}}"#;
        let error = decode(crate::TransportResponse::new(502, body.to_vec()))
            .expect_err("a 502 is never served as configuration");
        assert_eq!(error.kind(), ErrorKind::Transient);
        let refusal = error.service_refusal().expect("the status travels");
        assert_eq!(refusal.status(), 502);
        assert_eq!(refusal.code(), Some("internal_error"));
        assert_eq!(
            refusal.message(),
            Some("project configuration store unavailable")
        );
        assert_eq!(
            error.to_string(),
            "project configuration request was refused; no cached configuration is substituted"
        );

        // 429 stays retryable; a 403 stays an identity refusal; and a body
        // this client cannot read still carries the exact status.
        assert_eq!(
            decode(crate::TransportResponse::new(429, b"{}".to_vec()))
                .expect_err("a 429 is never served")
                .kind(),
            ErrorKind::Transient
        );
        let rejected = decode(crate::TransportResponse::new(403, b"nope".to_vec()))
            .expect_err("a 403 is never served");
        assert_eq!(rejected.kind(), ErrorKind::AuthenticationRejected);
        assert_eq!(
            rejected
                .service_refusal()
                .map(crate::ServiceRefusal::status),
            Some(403)
        );
    }
}
