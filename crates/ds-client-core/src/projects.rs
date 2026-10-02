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
    /// verdict is what lets a caller say so. `false` when the server did not
    /// say (an older ds-brain), never a guess.
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
pub(crate) struct ProjectEnvelope {
    #[serde(default)]
    pub error: Option<serde_json::Value>,
    #[serde(default)]
    pub data: Option<ProjectData>,
    #[serde(default)]
    pub projects: Option<Vec<WireProject>>,
}

#[derive(Deserialize)]
pub(crate) struct ProjectData {
    pub projects: Vec<WireProject>,
    /// `handlers_user.go` writes `"elevated": true|false` beside `projects`.
    /// Absent on an older ds-brain, and absence must not make the answer
    /// unreadable: it means "not said", which this crate reads as `false`.
    #[serde(default)]
    pub elevated: bool,
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum ProjectPayload {
    Array(Vec<WireProject>),
    Envelope(ProjectEnvelope),
}

impl ProjectPayload {
    pub(crate) fn bucket(self) -> Result<WireBucket, ClientError> {
        match self {
            // A bare array and a top-level `projects` are the older shapes;
            // neither carries a verdict.
            Self::Array(rows) => Ok(WireBucket {
                rows,
                elevated: false,
            }),
            Self::Envelope(envelope) => envelope.bucket(),
        }
    }
}

impl ProjectEnvelope {
    pub(crate) fn bucket(self) -> Result<WireBucket, ClientError> {
        if self.error.is_some() {
            return Err(unreadable());
        }
        match (self.data, self.projects) {
            (Some(data), None) => Ok(WireBucket {
                rows: data.projects,
                elevated: data.elevated,
            }),
            (None, Some(rows)) => Ok(WireBucket {
                rows,
                elevated: false,
            }),
            _ => Err(unreadable()),
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct WireProject {
    #[serde(default)]
    eds_project_id: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    project_id: Option<String>,
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
        let aliases = [
            self.eds_project_id.as_deref(),
            self.id.as_deref(),
            self.project_id.as_deref(),
        ];
        let mut id: Option<&str> = None;
        for value in aliases.into_iter().flatten() {
            if crate::validate_project_id(value).is_err() {
                return Err(unreadable());
            }
            if id.is_some_and(|current| current != value) {
                return Err(unreadable());
            }
            id = Some(value);
        }
        id.map(str::to_owned).ok_or_else(unreadable)
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
    fn the_elevation_verdict_is_read_and_absence_is_false_not_unreadable() {
        // ds-brain writes `elevated` beside `projects` inside `data`
        // (handlers_user.go). Read it; and read an older server that never
        // wrote it as "not said".
        assert!(
            payload(
                r#"{"data":{"count":1,"projects":[{"id":"p"}],"elevated":true,"status":"active"}}"#
            )
            .bucket()
            .unwrap()
            .elevated
        );
        assert!(
            !payload(r#"{"data":{"projects":[{"id":"p"}],"elevated":false}}"#)
                .bucket()
                .unwrap()
                .elevated
        );
        assert!(
            !payload(r#"{"data":{"projects":[{"id":"p"}]}}"#)
                .bucket()
                .unwrap()
                .elevated
        );
        assert!(
            !payload(r#"{"projects":[{"id":"p"}]}"#)
                .bucket()
                .unwrap()
                .elevated
        );
        assert!(!payload(r#"[{"id":"p"}]"#).bucket().unwrap().elevated);
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
