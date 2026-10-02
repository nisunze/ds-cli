use std::collections::BTreeMap;

use serde::Deserialize;

use crate::{ClientError, ErrorKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProjectStatus {
    Active,
    Archived,
    Testing,
}

impl ProjectStatus {
    pub const fn query_value(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Archived => "archived",
            Self::Testing => "testing",
        }
    }
}

/// Canonical order from `src/lib/api/projects.ts`, pinned for deterministic calls/results.
pub const PROJECT_STATUS_ORDER: [ProjectStatus; 3] = [
    ProjectStatus::Active,
    ProjectStatus::Archived,
    ProjectStatus::Testing,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    ds_project: String,
    project_name: String,
    display_name: Option<String>,
    role: Option<String>,
    status: ProjectStatus,
}

impl Project {
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
                let project_name = bounded_trimmed(row.project_name.unwrap_or_default(), 500)?;
                let display_name = optional_bounded(row.display_name, 500)?;
                let role = optional_bounded(row.role, 100)?;
                projects.push(Project {
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
}

impl WireProject {
    fn exact_id(&self) -> Result<String, ClientError> {
        let id = &self.eds_project_id;
        if id.is_empty()
            || id.trim() != id
            || id.chars().count() > 200
            || id.chars().any(char::is_control)
            || self.id.as_deref().is_some_and(|alias| alias != id)
        {
            return Err(unreadable());
        }
        Ok(id.clone())
    }

    fn validate_bucket(&self, status: ProjectStatus) -> Result<(), ClientError> {
        if self
            .lifecycle_state
            .as_deref()
            .is_some_and(|value| value != status.query_value())
            || self
                .is_archived
                .is_some_and(|value| value != matches!(status, ProjectStatus::Archived))
            || self
                .is_testing
                .is_some_and(|value| value != matches!(status, ProjectStatus::Testing))
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
}
