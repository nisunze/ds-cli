//! Dry-run-first backfills. Plans are pure; the authorized database/object host
//! supplies captured revisions and applies conditional operations after review.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub project: String,
    pub revision: String,
    pub config: Value,
    pub locale_if_missing: String,
    pub artifacts: Vec<Artifact>,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub stem: String,
    pub format: String,
    pub stored_name: String,
    pub sha256: String,
    pub generation: String,
}
pub fn plan(snapshot: &Snapshot) -> Result<Value, String> {
    if snapshot.project.is_empty() || snapshot.revision.is_empty() {
        return Err("report_migration_invalid: captured project/revision required".into());
    }
    crate::report_messages::Messages::new(
        Some(&snapshot.locale_if_missing),
        crate::report_messages::project_catalogue(&snapshot.config)?.as_ref(),
    )?;
    let mut config = snapshot.config.clone();
    let locale_change = match crate::report_messages::project_locale(&config) {
        Ok(locale) => {
            crate::report_messages::Messages::new(
                Some(&locale),
                crate::report_messages::project_catalogue(&config)?.as_ref(),
            )?;
            false
        }
        Err(error) if error.starts_with("report_locale_missing:") => {
            let path = if config.get("project_settings").is_some() {
                "/project_settings"
            } else {
                "/parameters/project_settings"
            };
            let rows = config
                .pointer_mut(path)
                .and_then(Value::as_array_mut)
                .ok_or("report_migration_invalid: existing project_settings sheet required")?;
            if rows.iter().any(|row| {
                crate::report_formats::setting_parameter_key(row).as_deref()
                    == Some(crate::report_messages::LOCALE_SETTING)
            }) {
                return Err("report_migration_conflict: invalid or duplicate existing locale; review without overwriting".into());
            }
            rows.push(json!({"parameter":crate::report_messages::LOCALE_SETTING,"value":snapshot.locale_if_missing}));
            true
        }
        Err(error) => return Err(error),
    };
    let mut targets = BTreeSet::new();
    let mut renames = Vec::new();
    for artifact in &snapshot.artifacts {
        if artifact.sha256.len() != 64
            || !artifact.sha256.bytes().all(|b| b.is_ascii_hexdigit())
            || artifact.generation.is_empty()
        {
            return Err(
                "report_migration_invalid: immutable artifact digest/generation required".into(),
            );
        }
        let target = crate::report_formats::report_filename(&artifact.stem, &artifact.format)?;
        if !targets.insert(target.to_lowercase()) {
            return Err("report_migration_collision: canonical target is duplicated".into());
        }
        if target != artifact.stored_name {
            renames.push(json!({"from":artifact.stored_name,"to":target,"sha256":artifact.sha256,"if_source_generation":artifact.generation,"if_target_absent":true,"verify_before_metadata":true}));
        }
    }
    Ok(
        json!({"schema":"ds.report-backfill-plan/v1","dry_run":true,"project":snapshot.project,"if_revision":snapshot.revision,"locale_change":locale_change,"config_after":config,"renames":renames,"retire_sources_after_verified_metadata":true}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dry_run_preserves_inputs_and_existing_locale_and_rejects_collisions() {
        let mut s = Snapshot {
            project: "fixture".into(),
            revision: "pinned".into(),
            config: json!({"project_settings":[]}),
            locale_if_missing: "fr".into(),
            artifacts: vec![Artifact {
                stem: "run_ruhuha".into(),
                format: "pdf_a0".into(),
                stored_name: "run_ruhuha.a0.pdf".into(),
                sha256: "a".repeat(64),
                generation: "1".into(),
            }],
        };
        let output = plan(&s).unwrap();
        assert_eq!(output["dry_run"], true);
        assert_eq!(output["renames"][0]["to"], "Run_Ruhuha.a0.pdf");
        assert_eq!(s.config, json!({"project_settings":[]}));
        s.config = output["config_after"].clone();
        s.locale_if_missing = "en".into();
        assert_eq!(plan(&s).unwrap()["locale_change"], false);
        s.artifacts.push(Artifact {
            stem: "RUN_RUHUHA".into(),
            format: "pdf_a0".into(),
            stored_name: "RUN_RUHUHA.a0.pdf".into(),
            sha256: "b".repeat(64),
            generation: "2".into(),
        });
        assert!(plan(&s)
            .unwrap_err()
            .starts_with("report_migration_collision:"));
    }
}
