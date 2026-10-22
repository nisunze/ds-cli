//! Closed, solver-independent result-query inputs shared by every Profile host.
use serde::{Deserialize, Serialize};

/// Independent admitted result sources; reading never changes the display case.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuerySource {
    #[default]
    Profile,
    ModelAnalysis,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryKind {
    Usage,
    Failures,
    NegativeLoading,
    ViolatingSections,
    Qualification,
    Blockers,
    Findings,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryCursor<H> {
    pub result_id: String,
    pub scope: String,
    pub model_id: String,
    pub history: H,
    pub kind: QueryKind,
    #[serde(default)]
    pub source: QuerySource,
    pub entity_ids: Vec<String>,
    pub offset: usize,
}

/// Shared CLI/MCP input DTO. The transport supplies the captured native
/// scope/model/history/root; engineering predicates remain in the retained runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, bound(deserialize = "H: Deserialize<'de>"))]
pub struct QueryArguments<H> {
    pub kind: QueryKind,
    #[serde(default)]
    pub source: QuerySource,
    #[serde(default)]
    pub entity_ids: Vec<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub cursor: Option<QueryCursor<H>>,
}

pub const DEFAULT_QUERY_LIMIT: usize = 50;
fn default_limit() -> usize {
    DEFAULT_QUERY_LIMIT
}
