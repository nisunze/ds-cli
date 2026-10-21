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
    // A held-first print that holds every selected form reads them without the
    // service, not even a session refresh; a form it does not hold is read once.
    if refresh == "held"
        && let Ok(layers) = store::printing_layers(root, scope, &selected)
    {
        return Ok(layers);
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
                        max_age_seconds: if refresh == "held" {
                            super::export::SURVEY_HELD_FIRST_AGE_SECONDS
                        } else {
                            hold::DEFAULT_MAX_AGE_SECONDS
                        },
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
#[path = "survey_context_tests.rs"]
mod tests;
