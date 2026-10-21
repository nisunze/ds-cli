use std::collections::BTreeMap;

use serde::Deserialize;

use crate::{ClientError, ErrorKind};

/// The project state model: `active | archived | testing | template`, one
/// listing bucket each (ds-brain `docs/contracts/project-lifecycle.md`). A
/// template is an ordinary project in the template state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProjectStatus {
    Active,
    Archived,
    Testing,
    Template,
}

impl ProjectStatus {
    pub const fn query_value(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Archived => "archived",
            Self::Testing => "testing",
            Self::Template => "template",
        }
    }

    /// The bucket ds-brain derives from three independent flags: archive
    /// dominates, then template, then testing.
    pub const fn from_flags(is_archived: bool, is_testing: bool, is_template: bool) -> Self {
        if is_archived {
            Self::Archived
        } else if is_template {
            Self::Template
        } else if is_testing {
            Self::Testing
        } else {
            Self::Active
        }
    }
}

/// Canonical order from `src/lib/api/projects.ts`, pinned for deterministic calls/results.
pub const PROJECT_STATUS_ORDER: [ProjectStatus; 4] = [
    ProjectStatus::Active,
    ProjectStatus::Archived,
    ProjectStatus::Testing,
    ProjectStatus::Template,
];

/// The settings a project carries beside its identity, exactly as the
/// authenticated directory listed them. They are read, never inferred: an
/// absent setting is the server's default, which this crate does not restate.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectSettings {
    pub country: Option<String>,
    pub client: Option<String>,
    pub network_template: Option<String>,
    pub styling_template_id: Option<String>,
    pub project_components: serde_json::Value,
    pub project_phases: serde_json::Value,
    pub processing_lanes: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    ds_project: String,
    project_name: String,
    display_name: Option<String>,
    role: Option<String>,
    status: ProjectStatus,
    lifecycle_facts: serde_json::Value,
    project_params: serde_json::Value,
    legacy_project_crs: serde_json::Value,
    settings: ProjectSettings,
}

impl Project {
    /// Exact server-normalized project properties, retained for project-fenced print CRS capture.
    pub fn project_params(&self) -> &serde_json::Value {
        &self.project_params
    }
    /// The settings a project created from this one as a template inherits.
    pub fn settings(&self) -> &ProjectSettings {
        &self.settings
    }
    /// Shapes only from the same authenticated directory; never inferred authority.
    pub fn legacy_project_crs_shape(&self) -> serde_json::Value {
        let crs = &self.legacy_project_crs;
        serde_json::json!({"present": !crs.is_null(), "is_object": crs.is_object(), "mode_is_string": crs.get("mode").is_some_and(serde_json::Value::is_string), "utm_zone_is_number": crs.get("utm_zone").is_some_and(serde_json::Value::is_number), "utm_hemisphere_is_string": crs.get("utm_hemisphere").is_some_and(serde_json::Value::is_string), "definition_is_string": crs.get("definition").is_some_and(serde_json::Value::is_string)})
    }
    pub fn lifecycle(&self, now_ms: i64) -> serde_json::Value {
        crate::project_properties::lifecycle_projection(&self.lifecycle_facts, now_ms)
    }
    pub fn ds_project(&self) -> &str {
        &self.ds_project
    }
    pub fn project_name(&self) -> &str {
        &self.project_name
    }
    pub fn display_name(&self) -> Option<&str> {
        self.display_name.as_deref()
    }
    pub fn role(&self) -> Option<&str> {
        self.role.as_deref()
    }
    pub const fn status(&self) -> ProjectStatus {
        self.status
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectDirectory {
    projects: Vec<Project>,
    elevated: bool,
}

impl ProjectDirectory {
    pub fn projects(&self) -> &[Project] {
        &self.projects
    }

    /// Whether ds-brain listed these projects by elevation rather than by
    /// membership.
    ///
    /// A `platform_admin` sees every project through the elevated path with no
    /// membership document behind any row, so every row's `role` is empty —
    /// which reads as "no role" when the truth is "seen by elevation". The
    /// verdict is what lets a caller say so.
    pub const fn elevated(&self) -> bool {
        self.elevated
    }

    pub fn exact(&self, ds_project: &str) -> Option<&Project> {
        self.projects
            .iter()
            .find(|project| project.ds_project == ds_project)
    }

    pub(crate) fn from_buckets(
        buckets: Vec<(ProjectStatus, WireBucket)>,
    ) -> Result<Self, ClientError> {
        let total: usize = buckets.iter().map(|(_, bucket)| bucket.rows.len()).sum();
        if total > 10_000 {
            return Err(unreadable());
        }
        // Elevation is a property of the caller, not of a lifecycle bucket:
        // ds-brain derives it from the principal's governance roles on every
        // list call, and the three buckets are one directory read by one
        // principal, so they must agree. If they do not, the server changed
        // its mind mid-read (its own elevation lookup fails soft to `false`),
        // and the rows are then a mix of the elevated and the membership view
        // — not a directory this crate can vouch for. Refuse rather than pick.
        let elevated = buckets.first().is_some_and(|(_, bucket)| bucket.elevated);
        if buckets
            .iter()
            .any(|(_, bucket)| bucket.elevated != elevated)
        {
            return Err(ClientError::new(
                ErrorKind::UnreadableResponse,
                "the lifecycle buckets disagreed about the caller's elevation",
            ));
        }
        let mut seen = BTreeMap::<String, ProjectStatus>::new();
        let mut projects = Vec::with_capacity(total);
        for (status, bucket) in buckets {
            for row in bucket.rows {
                let id = row.exact_id()?;
                row.validate_bucket(status)?;
                if let Some(previous) = seen.insert(id.clone(), status) {
                    let _ = previous;
                    return Err(ClientError::new(
                        ErrorKind::UnreadableResponse,
                        "one project identity appeared in more than one lifecycle bucket",
                    ));
                }
                for value in [
                    &row.expires_at,
                    &row.expiration_reason,
                    &row.expiration_support_email,
                    &row.expiration_support_phone,
                    &row.read_only_reason,
                ] {
                    if value
                        .as_ref()
                        .is_some_and(|v| v.chars().count() > 512 || v.chars().any(char::is_control))
                    {
                        return Err(unreadable());
                    }
                }
                let lifecycle_facts = serde_json::json!({
                    "ds_project":id,"display_name":row.display_name,"project_name":row.project_name,
                    "lifecycle_state":row.lifecycle_state,"is_archived":row.is_archived,
                    "is_expired":row.is_expired,"read_only_reason":row.read_only_reason,
                    "expires_at":row.expires_at,"expiration_reason":row.expiration_reason,
                    "expiration_support_email":row.expiration_support_email,"expiration_support_phone":row.expiration_support_phone,
                });
                let project_name = bounded_trimmed(row.project_name.unwrap_or_default(), 500)?;
                let display_name = optional_bounded(row.display_name, 500)?;
                let role = optional_bounded(row.role, 100)?;
                for metadata in [
                    &row.project_params,
                    &row.legacy_project_crs,
                    &row.project_components,
                    &row.project_phases,
                    &row.processing_lanes,
                ] {
                    if serde_json::to_vec(metadata)
                        .map_err(|_| unreadable())?
                        .len()
                        > 8192
                    {
                        return Err(unreadable());
                    }
                }
                let settings = ProjectSettings {
                    country: optional_setting(row.country)?,
                    client: optional_setting(row.client)?,
                    network_template: optional_setting(row.network_template)?,
                    styling_template_id: optional_setting(row.styling_template_id)?,
                    project_components: row.project_components,
                    project_phases: row.project_phases,
                    processing_lanes: row.processing_lanes,
                };
                projects.push(Project {
                    project_params: row.project_params,
                    legacy_project_crs: row.legacy_project_crs,
                    settings,
                    lifecycle_facts,
                    ds_project: id,
                    project_name,
                    display_name,
                    role,
                    status,
                });
            }
        }
        Ok(Self { projects, elevated })
    }
}

/// One lifecycle bucket as the wire carried it: its rows and the server's
/// elevation verdict for the caller that asked.
pub(crate) struct WireBucket {
    pub rows: Vec<WireProject>,
    pub elevated: bool,
}

#[derive(Deserialize)]
pub(crate) struct ProjectPayload {
    success: bool,
    data: ProjectData,
}

#[derive(Deserialize)]
pub(crate) struct ProjectData {
    pub projects: Vec<WireProject>,
    pub elevated: bool,
}

impl ProjectPayload {
    pub(crate) fn bucket(self) -> Result<WireBucket, ClientError> {
        if !self.success {
            return Err(unreadable());
        }
        Ok(WireBucket {
            rows: self.data.projects,
            elevated: self.data.elevated,
        })
    }
}

#[derive(Deserialize)]
pub(crate) struct WireProject {
    #[serde(default)]
    project_params: serde_json::Value,
    #[serde(default, rename = "crs")]
    legacy_project_crs: serde_json::Value,
    eds_project_id: String,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    project_name: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    lifecycle_state: Option<String>,
    #[serde(default)]
    is_archived: Option<bool>,
    #[serde(default)]
    is_testing: Option<bool>,
    #[serde(default)]
    is_template: Option<bool>,
    #[serde(default)]
    country: Option<String>,
    #[serde(default)]
    client: Option<String>,
    #[serde(default)]
    network_template: Option<String>,
    #[serde(default)]
    styling_template_id: Option<String>,
    #[serde(default)]
    project_components: serde_json::Value,
    #[serde(default)]
    project_phases: serde_json::Value,
    #[serde(default)]
    processing_lanes: serde_json::Value,
    #[serde(default)]
    expires_at: Option<String>,
    #[serde(default)]
    expiration_reason: Option<String>,
    #[serde(default)]
    expiration_support_email: Option<String>,
    #[serde(default)]
    expiration_support_phone: Option<String>,
    #[serde(default)]
    is_expired: Option<bool>,
    #[serde(default)]
    read_only_reason: Option<String>,
}

impl WireProject {
    fn exact_id(&self) -> Result<String, ClientError> {
        // One canonical project id, validated by the one owner; a stray `id`
        // alias must agree with it.
        let id = &self.eds_project_id;
        if crate::validate_project_id(id).is_err()
            || self.id.as_deref().is_some_and(|alias| alias != id)
        {
            return Err(unreadable());
        }
        Ok(id.clone())
    }

    /// A row's own facts must derive the bucket it was listed under. The
    /// flags are independent and the bucket is decided by precedence
    /// ([`ProjectStatus::from_flags`]), so a flag a row did not send is no
    /// evidence, and a flag the precedence overrides may be true: an archived
    /// testing fixture is listed under Archived, a testing project an admin
    /// marked template under Template. A server that predates the template
    /// bucket answers `?status=template` with its active bucket; its rows say
    /// `lifecycle_state: "active"` and are refused here, never relabelled.
    fn validate_bucket(&self, status: ProjectStatus) -> Result<(), ClientError> {
        use ProjectStatus::{Active, Archived, Template, Testing};
        let template_disagrees = self.is_template.is_some_and(|value| match status {
            Template => !value,
            Active | Testing => value,
            Archived => false,
        });
        let testing_disagrees = self.is_testing.is_some_and(|value| match status {
            Testing => !value,
            Active => value,
            Archived | Template => false,
        });
        if self
            .lifecycle_state
            .as_deref()
            .is_some_and(|value| value != status.query_value())
            || self
                .is_archived
                .is_some_and(|value| value != matches!(status, Archived))
            || template_disagrees
            || testing_disagrees
        {
            return Err(ClientError::new(
                ErrorKind::UnreadableResponse,
                "a project row disagreed with its requested lifecycle bucket",
            ));
        }
        Ok(())
    }
}

fn bounded_trimmed(value: String, max: usize) -> Result<String, ClientError> {
    if value.trim() != value || value.chars().count() > max || value.chars().any(char::is_control) {
        return Err(unreadable());
    }
    Ok(value)
}

fn optional_bounded(value: Option<String>, max: usize) -> Result<Option<String>, ClientError> {
    value.map(|value| bounded_trimmed(value, max)).transpose()
}

/// A project setting as an operator typed it: bounded and free of control
/// characters, but its outer whitespace is the author's and is not a reason
/// to refuse the whole directory. Whoever carries it trims it.
fn optional_setting(value: Option<String>) -> Result<Option<String>, ClientError> {
    match value {
        Some(value) if value.chars().count() > 500 || value.chars().any(char::is_control) => {
            Err(unreadable())
        }
        value => Ok(value),
    }
}

fn unreadable() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "the project response is outside the closed project-list contract",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(json: &str) -> ProjectPayload {
        serde_json::from_str(json).expect("project payload")
    }

    fn row(id: &str) -> WireProject {
        serde_json::from_value(serde_json::json!({ "eds_project_id": id, "role": "" }))
            .expect("wire row")
    }

    #[test]
    fn the_elevation_verdict_is_required_from_the_server() {
        for elevated in [true, false] {
            let body =
                serde_json::json!({"success":true,"data":{"projects":[],"elevated":elevated}});
            assert_eq!(
                payload(&body.to_string()).bucket().unwrap().elevated,
                elevated
            );
        }
        assert!(serde_json::from_str::<ProjectPayload>(
            r#"{"success":true,"data":{"projects":[]}}"#
        )
        .is_err());
    }

    #[test]
    fn only_the_current_project_envelope_is_read() {
        for body in [
            r#"[]"#,
            r#"{"projects":[]}"#,
            r#"{"data":{"projects":[],"elevated":false}}"#,
        ] {
            assert!(serde_json::from_str::<ProjectPayload>(body).is_err());
        }
        assert!(
            payload(r#"{"success":false,"data":{"projects":[],"elevated":false}}"#)
                .bucket()
                .is_err()
        );
    }

    #[test]
    fn the_project_identity_is_the_canonical_eds_project_id() {
        for body in [r#"{"id":"p"}"#, r#"{"project_id":"p"}"#] {
            assert!(serde_json::from_str::<WireProject>(body).is_err());
        }
        let row: WireProject =
            serde_json::from_str(r#"{"eds_project_id":"p","id":"other"}"#).unwrap();
        assert!(row.exact_id().is_err());
        let control: WireProject =
            serde_json::from_str(r#"{"eds_project_id":"bad\u0001id"}"#).unwrap();
        assert!(control.exact_id().is_err());
    }

    #[test]
    fn buckets_that_disagree_about_elevation_are_refused_not_averaged() {
        let directory = ProjectDirectory::from_buckets(vec![
            (
                ProjectStatus::Active,
                WireBucket {
                    rows: vec![row("a")],
                    elevated: true,
                },
            ),
            (
                ProjectStatus::Archived,
                WireBucket {
                    rows: vec![],
                    elevated: true,
                },
            ),
            (
                ProjectStatus::Testing,
                WireBucket {
                    rows: vec![row("t")],
                    elevated: true,
                },
            ),
        ])
        .unwrap();
        assert!(directory.elevated());
        // Elevation, not membership, is why these rows are visible: the empty
        // role is preserved as the server sent it, and the verdict says why.
        assert_eq!(directory.exact("a").unwrap().role(), Some(""));

        let error = ProjectDirectory::from_buckets(vec![
            (
                ProjectStatus::Active,
                WireBucket {
                    rows: vec![row("a")],
                    elevated: true,
                },
            ),
            (
                ProjectStatus::Archived,
                WireBucket {
                    rows: vec![],
                    elevated: false,
                },
            ),
        ])
        .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::UnreadableResponse);
        assert_eq!(
            error.to_string(),
            "the lifecycle buckets disagreed about the caller's elevation"
        );

        assert!(!ProjectDirectory::from_buckets(vec![]).unwrap().elevated());
    }

    fn wire(value: serde_json::Value) -> WireProject {
        serde_json::from_value(value).expect("wire row")
    }

    fn bucket(status: ProjectStatus, row: WireProject) -> Result<ProjectDirectory, ClientError> {
        ProjectDirectory::from_buckets(vec![(
            status,
            WireBucket {
                rows: vec![row],
                elevated: false,
            },
        )])
    }

    #[test]
    fn the_state_model_is_four_buckets_derived_by_precedence() {
        assert_eq!(
            PROJECT_STATUS_ORDER.map(ProjectStatus::query_value),
            ["active", "archived", "testing", "template"]
        );
        for (flags, status) in [
            ((false, false, false), ProjectStatus::Active),
            ((true, false, false), ProjectStatus::Archived),
            ((false, true, false), ProjectStatus::Testing),
            ((false, false, true), ProjectStatus::Template),
            ((true, true, true), ProjectStatus::Archived),
            ((false, true, true), ProjectStatus::Template),
        ] {
            assert_eq!(ProjectStatus::from_flags(flags.0, flags.1, flags.2), status);
        }
    }

    /// A row may carry a flag the precedence overrides. An archived testing
    /// fixture was refused here before the precedence was stated, which made
    /// one such project fail the whole directory read.
    #[test]
    fn rows_whose_flags_derive_their_bucket_are_admitted() {
        let template = bucket(
            ProjectStatus::Template,
            wire(serde_json::json!({"eds_project_id":"t","lifecycle_state":"template","is_archived":false,"is_testing":true,"is_template":true})),
        )
        .unwrap();
        assert_eq!(
            template.exact("t").unwrap().status(),
            ProjectStatus::Template
        );
        bucket(
            ProjectStatus::Archived,
            wire(serde_json::json!({"eds_project_id":"a","lifecycle_state":"archived","is_archived":true,"is_testing":true,"is_template":true})),
        )
        .expect("an archived testing template is Archived");
    }

    #[test]
    fn rows_whose_flags_contradict_their_bucket_are_refused() {
        for (status, row) in [
            // A template listed under Testing or Active.
            (
                ProjectStatus::Testing,
                serde_json::json!({"eds_project_id":"x","is_testing":true,"is_template":true}),
            ),
            (
                ProjectStatus::Active,
                serde_json::json!({"eds_project_id":"x","is_template":true}),
            ),
            // A template-bucket row that is no template.
            (
                ProjectStatus::Template,
                serde_json::json!({"eds_project_id":"x","is_template":false}),
            ),
            // A server that predates the template bucket answers it as active.
            (
                ProjectStatus::Template,
                serde_json::json!({"eds_project_id":"x","lifecycle_state":"active","is_archived":false,"is_testing":false}),
            ),
        ] {
            let error = bucket(status, wire(row)).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::UnreadableResponse);
            assert_eq!(
                error.to_string(),
                "a project row disagreed with its requested lifecycle bucket"
            );
        }
    }

    #[test]
    fn the_settings_a_template_carries_are_read_as_listed() {
        let directory = bucket(
            ProjectStatus::Template,
            wire(serde_json::json!({
                "eds_project_id":"czgmdwth_gisagara","lifecycle_state":"template","is_template":true,
                "country":"Rwanda","client":"EDCL ","network_template":"master","styling_template_id":"",
                "project_components":{"design":true,"solar":false},"project_phases":{"network":["I"]},
                "processing_lanes":{"fast":true}
            })),
        )
        .unwrap();
        let settings = directory.exact("czgmdwth_gisagara").unwrap().settings();
        assert_eq!(settings.country.as_deref(), Some("Rwanda"));
        // The author's whitespace is kept; whoever carries it trims it.
        assert_eq!(settings.client.as_deref(), Some("EDCL "));
        assert_eq!(settings.network_template.as_deref(), Some("master"));
        assert_eq!(settings.styling_template_id.as_deref(), Some(""));
        assert_eq!(settings.project_components["solar"], false);
        assert_eq!(settings.processing_lanes["fast"], true);
        // A control character is still outside the closed contract.
        let error = bucket(
            ProjectStatus::Active,
            wire(serde_json::json!({"eds_project_id":"p","client":"bad\u{0007}"})),
        )
        .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::UnreadableResponse);
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    #[test]
    fn project_directory_preserves_expiry_contacts_for_the_native_projection() {
        let wire = serde_json::from_value::<WireProject>(serde_json::json!({"eds_project_id":"p","expires_at":"2026-10-02T00:00:00Z","expiration_support_email":"help@example.com"})).unwrap();
        let directory = ProjectDirectory::from_buckets(vec![(
            ProjectStatus::Active,
            WireBucket {
                rows: vec![wire],
                elevated: false,
            },
        )])
        .unwrap();
        let result = directory.projects()[0].lifecycle(0);
        assert_eq!(result["supportEmail"], "help@example.com");
        assert_eq!(result["chip"]["tone"], "far");
    }
}

#[cfg(test)]
mod project_crs_tests {
    use super::*;
    #[test]
    fn normalized_server_project_params_are_preserved_without_crs_inference() {
        let params = serde_json::json!({"project_type":"new_build","crs":{"mode":"utm","utm_zone":35,"utm_hemisphere":"south","definition":""}});
        let wire = serde_json::from_value::<WireProject>(
            serde_json::json!({"eds_project_id":"p","project_params":params}),
        )
        .unwrap();
        let directory = ProjectDirectory::from_buckets(vec![(
            ProjectStatus::Active,
            WireBucket {
                rows: vec![wire],
                elevated: false,
            },
        )])
        .unwrap();
        assert_eq!(directory.projects()[0].project_params(), &params);
        let missing = serde_json::from_value::<WireProject>(
            serde_json::json!({"eds_project_id":"q","country":"Rwanda"}),
        )
        .unwrap();
        let directory = ProjectDirectory::from_buckets(vec![(
            ProjectStatus::Active,
            WireBucket {
                rows: vec![missing],
                elevated: false,
            },
        )])
        .unwrap();
        assert!(directory.projects()[0].project_params().is_null());
    }
    #[test]
    fn legacy_directory_crs_is_diagnosed_without_normalizing_or_guessing() {
        let wire = serde_json::from_value::<WireProject>(serde_json::json!({"eds_project_id":"p","crs":{"mode":"utm","utm_zone":35,"utm_hemisphere":"south"}})).unwrap();
        let directory = ProjectDirectory::from_buckets(vec![(
            ProjectStatus::Active,
            WireBucket {
                rows: vec![wire],
                elevated: false,
            },
        )])
        .unwrap();
        let project = directory.exact("p").unwrap();
        assert!(project.project_params().is_null());
        assert_eq!(project.legacy_project_crs_shape()["mode_is_string"], true);
        assert!(directory.exact("other").is_none());
        assert!(!project
            .legacy_project_crs_shape()
            .to_string()
            .contains("south"));
    }
}
