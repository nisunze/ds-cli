//! Selected project survey geometry uses the same native hold as field reads.
//! Photos do not participate in map capture.

use ds_cli_contract::outcome::Failure;
use ds_command_kernel::printing::{PrintContextLayer, PrintContextSource};
use ds_command_kernel::project_dataset_cache::Scope;
use ds_command_kernel::survey::hold::{self, Filter, Refresh};
use ds_project_data::held::{HeldLayer, PreviewContextRefusal};
use ds_project_data::survey_hold::{self as store, HoldError};
use std::collections::BTreeSet;
use std::path::Path;

fn invalid(message: impl std::fmt::Display) -> Failure {
    Failure::failed("print_context_invalid", message.to_string())
        .remedy("Read the selected exact project's survey form through ds survey entries read, then repeat the export")
}

fn capture_failure(error: PreviewContextRefusal) -> Failure {
    match error {
        PreviewContextRefusal::Kernel(refusal) => {
            invalid(format!("{}: {}", refusal.code, refusal.message_key))
                .detail(serde_json::json!({"refusal":refusal}))
        }
        PreviewContextRefusal::Holdings(failure) => invalid(failure),
    }
}

/// Refresh each selected form once; capture all held rows, never a display limit.
pub(super) fn load(
    lane: &str,
    scope: &Scope,
    identity: &ds_cli_auth::ProviderIdentity,
    contexts: &[PrintContextLayer],
    refresh: &str,
    link: &mut super::hold::Link,
) -> Result<Vec<HeldLayer>, Failure> {
    if !contexts
        .iter()
        .any(|layer| matches!(layer.source, PrintContextSource::Survey { .. }))
    {
        return Ok(Vec::new());
    }
    // Survey entries and their media use the layer-store root. The geographic
    // asset root used by report context rooms is an independent setting.
    let root = ds_layer_store::default_root().map_err(invalid)?;
    load_at(&root, lane, scope, identity, contexts, refresh, link)
}

fn load_at(
    root: &Path,
    lane: &str,
    scope: &Scope,
    identity: &ds_cli_auth::ProviderIdentity,
    contexts: &[PrintContextLayer],
    refresh: &str,
    link: &mut super::hold::Link,
) -> Result<Vec<HeldLayer>, Failure> {
    let selected: Vec<PrintContextLayer> = contexts
        .iter()
        .filter(|layer| matches!(layer.source, PrintContextSource::Survey { .. }))
        .cloned()
        .collect();
    if selected.is_empty() {
        return Ok(Vec::new());
    }
    if scope.principal != identity.uid() {
        return Err(invalid("the report's survey scope names another account"));
    }
    if refresh == "local" {
        return store::printing_layers(root, scope, &selected).map_err(capture_failure);
    }
    let forms: BTreeSet<&str> = selected
        .iter()
        .filter_map(|layer| match &layer.source {
            PrintContextSource::Survey { form } => Some(form.as_str()),
            _ => None,
        })
        .collect();
    let report = link.read(|| {
        ds_cli_auth::survey_hold(lane, &scope.project, |uid, fetch| {
            if uid != scope.principal {
                return Err(invalid("account changed before survey context capture"));
            }
            for form in &forms {
                store::refresh(
                    root,
                    scope,
                    form,
                    &Filter::default(),
                    Refresh::Auto {
                        max_age_seconds: hold::DEFAULT_MAX_AGE_SECONDS,
                    },
                    ds_command_kernel::time::OffsetDateTime::now_utc(),
                    |body| fetch(body),
                )
                .map_err(|error| match error {
                    HoldError::Fetch(failure) => invalid(format!("survey {form}: {}", failure)),
                    HoldError::NotHeld => invalid(format!("survey {form} is not held")),
                    HoldError::Refused(message) | HoldError::Store(message) => {
                        invalid(format!("survey {form}: {message}"))
                    }
                })?;
            }
            store::printing_layers(root, scope, &selected).map_err(capture_failure)
        })
    })?;
    match report {
        Some(report) => {
            super::export::require_same_context(
                identity,
                &scope.project,
                report.identity(),
                report.project_id(),
            )
            .map_err(|error| invalid(error.message))?;
            Ok(report.into_result())
        }
        None => store::printing_layers(root, scope, &selected).map_err(capture_failure),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn selected_survey_geometry_uses_the_complete_hold_without_photo_authority() {
        let dir = tempfile::tempdir().unwrap();
        let scope = Scope {
            principal: "user-a".into(),
            project: "project-a".into(),
        };
        let identity =
            ds_cli_auth::ProviderIdentity::new("stable", &"a".repeat(64), "user-a").unwrap();
        let row = json!({"type":"Feature","geometry":{"type":"Point","coordinates":[29.5,-2.0]},
            "properties":{"id":"pole-1","photo":"unavailable-photo", "metadata":{"firestore_updated_at":"2026-09-30T00:00:00Z","is_deleted":false}}});
        store::refresh(
            dir.path(),
            &scope,
            "lv_poles_as_built",
            &Filter::default(),
            Refresh::Full,
            ds_command_kernel::time::OffsetDateTime::now_utc(),
            |_| {
                Ok::<_, String>(
                    format!(
                        "{row}\n{}\n",
                        json!({"__type":"summary","form_count":1,"errors":[],"total_features":1})
                    )
                    .into_bytes(),
                )
            },
        )
        .unwrap();
        let selected = [PrintContextLayer {
            id: "survey_existing_poles".into(),
            label: "Surveyed poles".into(),
            source: PrintContextSource::Survey {
                form: "lv_poles_as_built".into(),
            },
        }];
        let captured = load_at(
            dir.path(),
            "stable",
            &scope,
            &identity,
            &selected,
            "local",
            &mut super::super::hold::Link::default(),
        )
        .unwrap();
        assert_eq!(captured[0].collection["features"], json!([row]));
        assert_eq!(captured[0].source["source_project"], "project-a");
        let foreign = Scope {
            principal: "user-b".into(),
            ..scope.clone()
        };
        let refused = load_at(
            dir.path(),
            "stable",
            &foreign,
            &identity,
            &selected,
            "local",
            &mut super::super::hold::Link::default(),
        )
        .unwrap_err();
        assert_eq!(refused.code(), "print_context_invalid");
    }

    #[test]
    fn production_capture_uses_survey_layer_root_when_geographic_root_differs() {
        const PROBE: &str = "DS_SURVEY_PRINT_ROOT_PROBE";
        let scope = Scope {
            principal: "user-a".into(),
            project: "project-a".into(),
        };
        let identity =
            ds_cli_auth::ProviderIdentity::new("stable", &"a".repeat(64), "user-a").unwrap();
        let selected = [PrintContextLayer {
            id: "survey_existing_poles".into(),
            label: "Surveyed poles".into(),
            source: PrintContextSource::Survey {
                form: "lv_poles_as_built".into(),
            },
        }];
        if std::env::var_os(PROBE).is_some() {
            let actual_root = ds_layer_store::default_root().unwrap();
            let geographic_root = ds_report_host::shared_root().unwrap();
            assert_ne!(actual_root, geographic_root);
            assert!(
                store::printing_layers(&geographic_root, &scope, &selected).is_err(),
                "the geographic root must not accidentally contain the survey hold"
            );
            let captured = load(
                "stable",
                &scope,
                &identity,
                &selected,
                "local",
                &mut super::super::hold::Link::default(),
            )
            .unwrap();
            assert_eq!(captured[0].source["declared_count"], 1);
            assert_eq!(
                captured[0].collection["features"][0]["properties"]["id"],
                "pole-a"
            );
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let survey_root = dir.path().join("survey-layers");
        let geographic_data = dir.path().join("geographic-data");
        let row = json!({"type":"Feature","geometry":{"type":"Point","coordinates":[29.5,-2.0]},
            "properties":{"id":"pole-a","metadata":{"firestore_updated_at":"2026-09-30T00:00:00Z","is_deleted":false}}});
        store::refresh(
            &survey_root,
            &scope,
            "lv_poles_as_built",
            &Filter::default(),
            Refresh::Full,
            ds_command_kernel::time::OffsetDateTime::now_utc(),
            |_| {
                Ok::<_, String>(
                    format!(
                        "{row}\n{}\n",
                        json!({"__type":"summary","form_count":1,"errors":[],"total_features":1})
                    )
                    .into_bytes(),
                )
            },
        )
        .unwrap();
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "project::survey_context::tests::production_capture_uses_survey_layer_root_when_geographic_root_differs", "--nocapture"])
            .env(PROBE, "child")
            .env("DS_LAYER_HOME", &survey_root)
            .env("XDG_DATA_HOME", &geographic_data)
            .env("LOCALAPPDATA", &geographic_data)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }
}
