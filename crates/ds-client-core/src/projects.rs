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
}

impl ProjectDirectory {
    pub fn projects(&self) -> &[Project] {
        &self.projects
    }

    pub fn exact(&self, ds_project: &str) -> Option<&Project> {
        self.projects
            .iter()
            .find(|project| project.ds_project == ds_project)
    }

    pub(crate) fn from_buckets(
        buckets: Vec<(ProjectStatus, Vec<WireProject>)>,
    ) -> Result<Self, ClientError> {
        let total: usize = buckets.iter().map(|(_, rows)| rows.len()).sum();
        if total > 10_000 {
            return Err(unreadable());
        }
        let mut seen = BTreeMap::<String, ProjectStatus>::new();
        let mut projects = Vec::with_capacity(total);
        for (status, rows) in buckets {
            for row in rows {
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
        Ok(Self { projects })
    }
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
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum ProjectPayload {
    Array(Vec<WireProject>),
    Envelope(ProjectEnvelope),
}

impl ProjectPayload {
    pub(crate) fn rows(self) -> Result<Vec<WireProject>, ClientError> {
        match self {
            Self::Array(projects) => Ok(projects),
            Self::Envelope(envelope) => envelope.rows(),
        }
    }
}

impl ProjectEnvelope {
    pub(crate) fn rows(self) -> Result<Vec<WireProject>, ClientError> {
        if self.error.is_some() {
            return Err(unreadable());
        }
        match (self.data, self.projects) {
            (Some(data), None) => Ok(data.projects),
            (None, Some(projects)) => Ok(projects),
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
            if value.is_empty()
                || value.trim() != value
                || value.chars().count() > 200
                || value.chars().any(char::is_control)
            {
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
