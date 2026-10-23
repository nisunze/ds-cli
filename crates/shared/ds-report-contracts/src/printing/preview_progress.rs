//! Closed page-ready vocabulary for a native MV preview producer. Paths are
//! host-private transport; consumers admit their captured authority separately.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const SCHEMA: &str = "ds.mv-preview-progress/v1";
pub const EVENT_SCHEMA: &str = "ds.mv-preview-progress-event/v1";
pub const EVENT_PREFIX: &str = "DS_PRINT_PREVIEW ";
pub const FAILURE_SCHEMA: &str = "ds.mv-preview-failure/v1";
pub const MAX_MANIFEST_BYTES: u64 = 2 * 1024 * 1024;
pub const MAX_PAGES: usize = 4096;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub directory: PathBuf,
    pub input_sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Preparing,
    Rendering,
    Complete,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Page {
    pub page_number: usize,
    pub source_sheet_number: usize,
    pub alignment_id: String,
    pub from_station_m: f64,
    pub to_station_m: f64,
    pub path: PathBuf,
    pub sha256: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: String,
    pub input_sha256: String,
    pub project: String,
    pub model_revision: String,
    pub setup_sha256: String,
    pub revision: u64,
    pub stage: Stage,
    pub completed: usize,
    pub total: usize,
    pub full_page_count: usize,
    pub pages: Vec<Page>,
    pub error: Option<String>,
}

pub fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl Target {
    pub fn validate(&self) -> Result<(), String> {
        if !self.directory.is_absolute() || !digest(&self.input_sha256) {
            return Err(
                "MV preview progress requires a private absolute directory and exact input digest"
                    .into(),
            );
        }
        Ok(())
    }
}

impl Manifest {
    pub fn validate(
        &self,
        target: &Target,
        project: &str,
        model_revision: &str,
        setup_sha256: &str,
    ) -> Result<(), String> {
        target.validate()?;
        if self.schema != SCHEMA
            || self.input_sha256 != target.input_sha256
            || self.project != project
            || self.model_revision != model_revision
            || self.setup_sha256 != setup_sha256
            || !super::mv::valid_digest(setup_sha256)
            || self.revision == 0
            || self.completed != self.pages.len()
            || self.completed > self.total
            || self.total > MAX_PAGES
            || self.full_page_count < self.total
            || self.full_page_count > MAX_PAGES
            || (self.stage == Stage::Preparing && (self.total != 0 || self.completed != 0))
            || (self.stage == Stage::Rendering && self.total == 0)
            || (self.stage == Stage::Complete && (self.total == 0 || self.completed != self.total))
            || (self.stage == Stage::Failed) != self.error.is_some()
            || self
                .error
                .as_ref()
                .is_some_and(|text| text.is_empty() || text.len() > 8192)
        {
            return Err(
                "MV preview progress differs from its exact native job or ordered count".into(),
            );
        }
        let mut previous_sheet = 0;
        for (index, page) in self.pages.iter().enumerate() {
            if page.page_number != index + 1
                || page.source_sheet_number <= previous_sheet
                || page.alignment_id.is_empty()
                || page.alignment_id.len() > 256
                || !page.from_station_m.is_finite()
                || !page.to_station_m.is_finite()
                || page.to_station_m < page.from_station_m
                || page.path != target.directory.join(format!("page-{:04}.svg", index + 1))
                || !digest(&page.sha256)
                || page.size_bytes == 0
                || page.size_bytes > 96 * 1024 * 1024
            {
                return Err("MV preview progress page is crossed, unordered or unbounded".into());
            }
            previous_sheet = page.source_sheet_number;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub schema: String,
    pub input_sha256: String,
    pub revision: u64,
}

/// An early native producer refusal can precede the planned-page manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Failure {
    pub schema: String,
    pub input_sha256: String,
    pub project: String,
    pub error: String,
}
impl Failure {
    pub fn validate(&self, target: &Target, project: &str) -> Result<(), String> {
        target.validate()?;
        if self.schema != FAILURE_SCHEMA
            || self.input_sha256 != target.input_sha256
            || self.project != project
            || self.error.is_empty()
            || self.error.len() > 8192
        {
            return Err("MV preview failure differs from its exact native job".into());
        }
        Ok(())
    }
}
