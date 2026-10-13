//! Closed shared Project Notes requests. Project permissions, authorship,
//! aliases, applicability, conflicts and recoverable lifecycle are server-owned.
use crate::design_versions_headless::ObjectRef;
use serde::{Deserialize, Serialize};

pub const MEMORY_SCHEMA: &str = "ds.project-note-memory/v1";
pub const MAX_BODY_BYTES: usize = 128 * 1024;
pub const MAX_REQUEST_BYTES: usize = 256 * 1024;
pub const MAX_QUERY_ROWS: u32 = 50;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoteType {
    SessionSummary,
    Decision,
    Constraint,
    Correction,
    RejectedAssumption,
    Observation,
    Proposal,
    MutationOutcome,
    ContextConsolidation,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lifecycle {
    Active,
    Superseded,
    Withdrawn,
    Discarded,
    Irrelevant,
    Archived,
}
/// Authenticated author is separate. These labels never prove actor authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Basis {
    UserInstruction,
    UserDecision,
    Observation,
    Proposal,
    Inference,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub kind: Basis,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_author: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeKind {
    Project,
    Design,
    Asset,
    Workstream,
    Structures,
    Conditions,
    Preference,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub kind: ScopeKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub asset_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workstream: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub object_refs: Vec<ObjectRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub structure_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_ref: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VersionRef {
    pub id: String,
    pub version: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimRef {
    pub note_id: String,
    pub version: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim_id: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TagMatch {
    Any,
    All,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sort {
    Created,
    Updated,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Asc,
    Desc,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    #[serde(default)]
    pub terms: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub tag_match: TagMatch,
    #[serde(default)]
    pub note_types: Vec<NoteType>,
    #[serde(default)]
    pub lifecycle: Vec<Lifecycle>,
    pub sort: Sort,
    pub direction: Direction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    pub limit: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    #[serde(flatten)]
    pub query: Query,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<Scope>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Read {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim_id: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Versions {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    pub limit: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Backup {
    pub operation_id: String,
    pub sources: Vec<VersionRef>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Retire {
    pub operation_id: String,
    pub sources: Vec<VersionRef>,
    pub backup_id: String,
    pub backup_sha256: String,
    pub lifecycle: Lifecycle,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replacement: Option<VersionRef>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Restore {
    pub operation_id: String,
    pub backup_id: String,
    pub backup_sha256: String,
    pub sources: Vec<VersionRef>,
    pub expected_versions: Vec<VersionRef>,
}
fn meaningful(s: &str, max: usize) -> bool {
    !s.is_empty() && s.trim() == s && s.len() <= max && !s.chars().any(char::is_control)
}
fn id(s: &str) -> bool {
    meaningful(s, 256)
        && s != "."
        && s != ".."
        && !s
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '/' | '\\'))
}
fn instant(s: &str) -> bool {
    time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339)
        .is_ok_and(|t| t.unix_timestamp() > 0)
}
fn sha(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
fn require(valid: bool, why: &str) -> Result<(), String> {
    if valid { Ok(()) } else { Err(why.into()) }
}
fn ids(items: &[String], max: usize) -> bool {
    items.len() <= max
        && items.iter().all(|s| id(s))
        && items
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            == items.len()
}
fn refs(items: &[VersionRef]) -> Result<(), String> {
    require(
        !items.is_empty()
            && items.len() <= 50
            && items
                .iter()
                .all(|r| id(&r.id) && r.version > 0 && r.version < i64::MAX as u64)
            && items
                .iter()
                .map(|r| &r.id)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == items.len(),
        "Need1..50 unique exact note/version references",
    )
}
impl Scope {
    pub fn validate(&self) -> Result<(), String> {
        require(
            self.object_refs.len() <= 128
                && self
                    .object_refs
                    .iter()
                    .all(|r| matches!(r.kind.as_str(), "lv_transformer" | "mv_model") && id(&r.id))
                && self
                    .object_refs
                    .iter()
                    .map(|r| (&r.kind, &r.id))
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
                    == self.object_refs.len(),
            "Scope needs unique exact governed object references",
        )?;
        require(
            ids(&self.asset_ids, 128)
                && ids(&self.structure_ids, 128)
                && self.conditions.len() <= 32
                && self.conditions.iter().all(|s| meaningful(s, 512)),
            "Scope structure/condition bounds are invalid",
        )?;
        require(
            self.workstream.as_ref().is_none_or(|s| meaningful(s, 160))
                && self.user_ref.as_ref().is_none_or(|s| meaningful(s, 256)),
            "Scope attribution bounds are invalid",
        )?;
        require(
            match self.kind {
                ScopeKind::Project => true,
                ScopeKind::Design => !self.object_refs.is_empty(),
                ScopeKind::Asset => !self.asset_ids.is_empty(),
                ScopeKind::Workstream => self.workstream.is_some(),
                ScopeKind::Structures => !self.structure_ids.is_empty(),
                ScopeKind::Conditions => !self.conditions.is_empty(),
                ScopeKind::Preference => self.user_ref.is_some(),
            },
            "A local scope needs its actual affected objects, conditions or user",
        )
    }
}
impl Query {
    pub fn validate(&self) -> Result<(), String> {
        require(
            self.terms.len() <= 32
                && self.terms.iter().all(|s| meaningful(s, 160))
                && self.limit > 0
                && self.limit <= MAX_QUERY_ROWS
                && self.tags.len() <= 32
                && self.tags.iter().all(|s| meaningful(s, 160))
                && self.note_types.len() <= 9
                && self.lifecycle.len() <= 6
                && self.cursor.as_ref().is_none_or(|s| meaningful(s, 2048)),
            "Query bounds are invalid; aliases and tag identity remain project-owned",
        )
    }
}
impl Context {
    pub fn validate(&self) -> Result<(), String> {
        self.query.validate()?;
        if let Some(scope) = &self.scope {
            scope.validate()?;
        }
        require(
            self.query.lifecycle.is_empty() || self.query.lifecycle == vec![Lifecycle::Active],
            "Routine context accepts active notes only; trace retired evidence by read/query",
        )
    }
}
impl Read {
    pub fn validate(&self) -> Result<(), String> {
        require(
            id(&self.id)
                && self.version.is_none_or(|v| v > 0 && v < i64::MAX as u64)
                && self.claim_id.as_ref().is_none_or(|s| id(s)),
            "Read needs an exact note, optional positive version and claim",
        )
    }
}
impl Versions {
    pub fn validate(&self) -> Result<(), String> {
        require(
            id(&self.id)
                && self.limit > 0
                && self.limit <= 50
                && self.cursor.as_ref().is_none_or(|s| meaningful(s, 2048)),
            "Version page is invalid",
        )
    }
}
impl Backup {
    pub fn validate(&self) -> Result<(), String> {
        require(
            id(&self.operation_id),
            "Backup needs a stable operation identity",
        )?;
        refs(&self.sources)
    }
}
impl Retire {
    pub fn validate(&self) -> Result<(), String> {
        require(
            id(&self.operation_id)
                && id(&self.backup_id)
                && sha(&self.backup_sha256)
                && self.lifecycle != Lifecycle::Active
                && meaningful(&self.reason, 512),
            "Retirement needs an exact verified backup and reason; active is not retirement",
        )?;
        refs(&self.sources)?;
        if let Some(r) = &self.replacement {
            refs(std::slice::from_ref(r))?;
        }
        Ok(())
    }
}
impl Restore {
    pub fn validate(&self) -> Result<(), String> {
        require(
            id(&self.operation_id) && id(&self.backup_id) && sha(&self.backup_sha256),
            "Restore needs an exact verified backup",
        )?;
        refs(&self.sources)?;
        refs(&self.expected_versions)?;
        require(
            self.sources
                .iter()
                .map(|r| &r.id)
                .collect::<std::collections::BTreeSet<_>>()
                == self
                    .expected_versions
                    .iter()
                    .map(|r| &r.id)
                    .collect::<std::collections::BTreeSet<_>>(),
            "Restore fences must cover exactly the restored note identities",
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    Note,
    Record,
    Asset,
    External,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub kind: EvidenceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_version: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locator: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimKind {
    Instruction,
    Decision,
    Constraint,
    RejectedAssumption,
    Correction,
    Observation,
    Proposal,
    Inference,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Claim {
    pub id: String,
    pub kind: ClaimKind,
    pub issue_key: String,
    pub text: String,
    pub provenance: Provenance,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<Scope>,
    #[serde(default)]
    pub evidence: Vec<Evidence>,
    #[serde(default)]
    pub supersedes: Vec<ClaimRef>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Memory {
    pub schema: String,
    pub note_type: NoteType,
    pub lifecycle: Lifecycle,
    pub provenance: Provenance,
    pub scope: Scope,
    pub claims: Vec<Claim>,
    #[serde(default)]
    pub evidence: Vec<Evidence>,
    #[serde(default)]
    pub supersedes: Vec<ClaimRef>,
    #[serde(default)]
    pub unresolved: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Draft {
    pub subject: String,
    pub body: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub memory: Memory,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Write {
    pub operation_id: String,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_version: Option<u64>,
    pub reason: String,
    pub draft: Draft,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Consolidate {
    pub operation_id: String,
    pub id: String,
    pub reason: String,
    pub draft: Draft,
    pub sources: Vec<VersionRef>,
}
impl Provenance {
    pub fn validate(&self) -> Result<(), String> {
        require(
            [&self.agent_label, &self.session_ref, &self.source_author]
                .into_iter()
                .all(|s| s.as_ref().is_none_or(|x| meaningful(x, 256))),
            "Attribution must be bounded and truthful; absent attribution stays absent",
        )
    }
}
impl ClaimRef {
    pub fn validate(&self) -> Result<(), String> {
        require(
            id(&self.note_id)
                && self.version > 0
                && self.version < i64::MAX as u64
                && self.claim_id.as_ref().is_none_or(|s| id(s)),
            "Supersession needs an exact note/version and optional claim identity",
        )
    }
}
impl Evidence {
    pub fn validate(&self) -> Result<(), String> {
        let positive = |v: Option<u64>| v.is_some_and(|v| v > 0 && v < i64::MAX as u64);
        require(
            self.id.as_ref().is_none_or(|s| id(s))
                && self.claim_id.as_ref().is_none_or(|s| id(s))
                && self.revision_id.as_ref().is_none_or(|s| id(s))
                && self.sha256.as_ref().is_none_or(|s| sha(s))
                && self.locator.as_ref().is_none_or(|s| meaningful(s, 2048))
                && self.label.as_ref().is_none_or(|s| meaningful(s, 300)),
            "Evidence reference bounds are invalid",
        )?;
        require(
            match self.kind {
                EvidenceKind::Note | EvidenceKind::Record => {
                    self.id.is_some() && positive(self.version) && self.asset_version.is_none()
                }
                EvidenceKind::Asset => {
                    self.id.is_some()
                        && positive(self.asset_version)
                        && self.sha256.is_some()
                        && self.version.is_none()
                }
                EvidenceKind::External => {
                    self.locator.is_some() && self.version.is_none() && self.asset_version.is_none()
                }
            },
            "Evidence requires actual source/version pins; external references are unverified",
        )
    }
}
fn evidence(items: &[Evidence]) -> Result<(), String> {
    require(items.len() <= 64, "Too many evidence references")?;
    for item in items {
        item.validate()?;
    }
    Ok(())
}
fn claim_refs(items: &[ClaimRef]) -> Result<(), String> {
    require(items.len() <= 64, "Too many supersession references")?;
    for item in items {
        item.validate()?;
    }
    Ok(())
}
impl Claim {
    pub fn validate(&self) -> Result<(), String> {
        require(
            id(&self.id) && meaningful(&self.issue_key, 160) && meaningful(&self.text, 8192),
            "Claim needs a stable identity, issue key and substantive text",
        )?;
        self.provenance.validate()?;
        if let Some(s) = &self.scope {
            s.validate()?;
        }
        evidence(&self.evidence)?;
        claim_refs(&self.supersedes)?;
        require(
            match self.kind {
                ClaimKind::Instruction => self.provenance.kind == Basis::UserInstruction,
                ClaimKind::Decision => self.provenance.kind == Basis::UserDecision,
                ClaimKind::Constraint | ClaimKind::RejectedAssumption | ClaimKind::Correction => {
                    matches!(
                        self.provenance.kind,
                        Basis::UserInstruction | Basis::UserDecision
                    )
                }
                ClaimKind::Observation => self.provenance.kind == Basis::Observation,
                ClaimKind::Proposal => self.provenance.kind == Basis::Proposal,
                ClaimKind::Inference => self.provenance.kind == Basis::Inference,
            },
            "A claim cannot promote an observation, proposal or inference into a user decision",
        )?;
        require(
            !matches!(
                self.provenance.kind,
                Basis::UserInstruction | Basis::UserDecision
            ) || !self.evidence.is_empty(),
            "Explicit user rulings need their actual source evidence",
        )
    }
}
impl Draft {
    pub fn validate(&self) -> Result<(), String> {
        require(
            meaningful(&self.subject, 300)
                && self.body.len() <= MAX_BODY_BYTES
                && self.tags.len() <= 32
                && self.tags.iter().all(|s| meaningful(s, 160)),
            "Note subject/body/tag bounds are invalid",
        )?;
        let m = &self.memory;
        require(
            m.schema == MEMORY_SCHEMA && m.lifecycle == Lifecycle::Active,
            "Write active memory only; retirement requires a verified recoverable backup",
        )?;
        m.provenance.validate()?;
        m.scope.validate()?;
        evidence(&m.evidence)?;
        claim_refs(&m.supersedes)?;
        require(
            !m.claims.is_empty()
                && m.claims.len() <= 128
                && m.claims
                    .iter()
                    .map(|c| &c.id)
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
                    == m.claims.len(),
            "Meaningful notes need1..128 individually retrievable unique claims",
        )?;
        for c in &m.claims {
            c.validate()?;
        }
        require(
            m.unresolved.len() <= 32 && m.unresolved.iter().all(|s| meaningful(s, 2048)),
            "Unresolved questions exceed their bound",
        )?;
        require(
            serde_json::to_vec(self).map_err(|e| e.to_string())?.len() <= MAX_REQUEST_BYTES,
            "The note exceeds its full serialized bound",
        )
    }
}
impl Write {
    pub fn validate(&self) -> Result<(), String> {
        require(
            id(&self.operation_id)
                && id(&self.id)
                && meaningful(&self.reason, 512)
                && self
                    .expected_version
                    .is_none_or(|v| v > 0 && v < i64::MAX as u64),
            "Write needs a stable operation/note identity, reason and optional reviewed version",
        )?;
        self.draft.validate()
    }
}
impl Consolidate {
    pub fn validate(&self) -> Result<(), String> {
        require(
            id(&self.operation_id)
                && id(&self.id)
                && meaningful(&self.reason, 512)
                && self.draft.memory.note_type == NoteType::ContextConsolidation,
            "Consolidation needs a stable identity, reason and explicit consolidation draft",
        )?;
        refs(&self.sources)?;
        self.draft.validate()
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Write(Write),
    Read(Read),
    Query(Query),
    Context(Context),
    Versions(Versions),
    Backup(Backup),
    Retire(Retire),
    Restore(Restore),
    Consolidate(Consolidate),
}
impl Command {
    pub fn action(&self) -> &'static str {
        match self {
            Self::Write(_) => "project_note_write",
            Self::Read(_) => "project_note_read",
            Self::Query(_) => "project_note_query",
            Self::Context(_) => "project_note_context",
            Self::Versions(_) => "project_note_versions",
            Self::Backup(_) => "project_note_backup",
            Self::Retire(_) => "project_note_retire",
            Self::Restore(_) => "project_note_restore",
            Self::Consolidate(_) => "project_note_consolidate",
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Write(x) => x.validate(),
            Self::Read(x) => x.validate(),
            Self::Query(x) => x.validate(),
            Self::Context(x) => x.validate(),
            Self::Versions(x) => x.validate(),
            Self::Backup(x) => x.validate(),
            Self::Retire(x) => x.validate(),
            Self::Restore(x) => x.validate(),
            Self::Consolidate(x) => x.validate(),
        }
    }
    pub fn payload(&self, project: &str) -> Result<serde_json::Value, String> {
        require(id(project), "Project Notes requires the exact project")?;
        self.validate()?;
        let value = match self {
            Self::Write(x) => serde_json::to_value(x),
            Self::Read(x) => serde_json::to_value(x),
            Self::Query(x) => serde_json::to_value(x),
            Self::Context(x) => serde_json::to_value(x),
            Self::Versions(x) => serde_json::to_value(x),
            Self::Backup(x) => serde_json::to_value(x),
            Self::Retire(x) => serde_json::to_value(x),
            Self::Restore(x) => serde_json::to_value(x),
            Self::Consolidate(x) => serde_json::to_value(x),
        }
        .map_err(|e| e.to_string())?;
        let out =
            serde_json::json!({"action":self.action(),"ds_project":project,"project_note":value});
        require(
            serde_json::to_vec(&out).map_err(|e| e.to_string())?.len() <= MAX_REQUEST_BYTES,
            "The note request exceeds its bound",
        )?;
        Ok(out)
    }
}

impl Command {
    /// Check the exact selected project/identity and declared completeness before
    /// a thin host can call a result saved, empty, backed up or restored.
    pub fn verify_response(&self, project: &str, v: &serde_json::Value) -> Result<(), String> {
        require(
            v["project_id"] == project,
            "Project Notes response belongs to another project",
        )?;
        match self {
            Self::Write(w) => {
                verify_note(project, &v["note"], Some(&w.id))?;
                require(
                    v["operation_id"] == w.operation_id
                        && v["replayed"].is_boolean()
                        && v["note"]["version"].as_u64()
                            == Some(w.expected_version.map_or(1, |n| n + 1))
                        && v["context_revision"].as_str().is_some_and(sha),
                    "Write lacks its exact durable operation receipt",
                )?;
            }
            Self::Read(r) => {
                verify_note(project, &v["note"], Some(&r.id))?;
                require(
                    r.version
                        .is_none_or(|version| v["note"]["version"] == version)
                        && r.claim_id.as_ref().is_none_or(|id| v["claim"]["id"] == *id),
                    "Read differs from its exact selected version/claim",
                )?;
            }
            Self::Query(q) => verify_page(project, v, q.limit)?,
            Self::Context(c) => {
                verify_page(project, v, c.query.limit)?;
                let claims = v["claims"]
                    .as_array()
                    .ok_or("Applicable claim rows are missing")?;
                require(
                    claims.len() <= 128
                        && claims.iter().all(|c| {
                            c["note_id"].as_str().is_some_and(id)
                                && c["version"].as_u64().is_some_and(|v| v > 0)
                                && c["claim"]["id"].as_str().is_some_and(id)
                        }),
                    "Applicable claim identities are invalid",
                )?;
                let conflicts = v["conflicts"]
                    .as_array()
                    .ok_or("Context conflict status is missing")?;
                require(
                    conflicts.len() <= 128
                        && conflicts.iter().all(|c| {
                            c["state"] == "potential_conflict"
                                && c["issue_key"].as_str().is_some_and(|s| meaningful(s, 160))
                                && c["sources"].as_array().is_some_and(|refs| {
                                    refs.len() >= 2
                                        && refs.len() <= 128
                                        && refs.iter().all(|r| {
                                            r["note_id"].as_str().is_some_and(id)
                                                && r["version"].as_u64().is_some_and(|v| v > 0)
                                                && r["claim_id"].as_str().is_some_and(id)
                                        })
                                })
                        }),
                    "Potential conflicts lack exact source provenance",
                )?;
            }
            Self::Versions(r) => {
                require(
                    v["id"] == r.id && v["truncated"].is_boolean(),
                    "Version history is not the selected note",
                )?;
                let rows = v["items"]
                    .as_array()
                    .ok_or("Version history rows are missing")?;
                require(
                    rows.len() <= r.limit as usize
                        && rows.iter().all(|r| {
                            r["version"].as_u64().is_some_and(|v| v > 0)
                                && r["sha256"].as_str().is_some_and(sha)
                                && r["changed_by"].as_str().is_some_and(|s| meaningful(s, 256))
                                && r["changed_at"].as_str().is_some_and(instant)
                                && r["reason"].as_str().is_some_and(|s| meaningful(s, 512))
                        }),
                    "Version history provenance is invalid",
                )?;
            }
            Self::Backup(r) => {
                let b = &v["backup"];
                require(
                    b["project_id"] == project
                        && b["id"].as_str().is_some_and(id)
                        && b["sha256"].as_str().is_some_and(sha)
                        && b["verified"] == true
                        && b["created_at"].as_str().is_some_and(instant)
                        && b["created_by"].as_str().is_some_and(|s| meaningful(s, 256))
                        && b["sources"]
                            == serde_json::to_value(&r.sources).map_err(|e| e.to_string())?
                        && b["history_count"]
                            .as_u64()
                            .is_some_and(|n| n >= r.sources.len() as u64)
                        && b["json_bytes"].as_u64().is_some_and(|n| n > 0)
                        && v["operation_id"] == r.operation_id
                        && v["replayed"].is_boolean(),
                    "Backup receipt is not a verified recoverable exact-source backup",
                )?;
            }
            Self::Retire(r) => {
                verify_lifecycle(
                    project,
                    v,
                    &r.sources,
                    &r.operation_id,
                    &r.backup_id,
                    &r.backup_sha256,
                )?;
                require(
                    v["notes"].as_array().unwrap().iter().all(|n| {
                        n["memory"]["lifecycle"] == serde_json::to_value(&r.lifecycle).unwrap()
                    }),
                    "Retirement did not commit the requested lifecycle",
                )?;
            }
            Self::Restore(r) => {
                verify_lifecycle(
                    project,
                    v,
                    &r.sources,
                    &r.operation_id,
                    &r.backup_id,
                    &r.backup_sha256,
                )?;
                require(
                    v["notes"].as_array().unwrap().iter().all(|n| {
                        n["memory"]["lifecycle"] == "active"
                            && r.expected_versions.iter().any(|r| {
                                n["id"] == r.id
                                    && n["version"].as_u64().is_some_and(|v| v > r.version)
                            })
                    }),
                    "Restore did not create a new active version under its current fences",
                )?;
            }
            Self::Consolidate(r) => {
                verify_note(project, &v["note"], Some(&r.id))?;
                require(
                    v["operation_id"] == r.operation_id
                        && v["replayed"].is_boolean()
                        && v["sources"]
                            == serde_json::to_value(&r.sources).map_err(|e| e.to_string())?,
                    "Consolidation must preserve its exact source/version refs",
                )?;
            }
        }
        Ok(())
    }
}
fn verify_note(project: &str, n: &serde_json::Value, wanted: Option<&str>) -> Result<(), String> {
    require(
        n["project_id"] == project
            && n["id"].as_str().is_some_and(id)
            && wanted.is_none_or(|id| n["id"] == id)
            && n["version"]
                .as_u64()
                .is_some_and(|v| v > 0 && v < i64::MAX as u64)
            && n["document"].is_object()
            && n["memory"].is_object()
            && n["legacy"].is_boolean(),
        "Note lacks its exact project/identity/version and preserved document",
    )
}
fn verify_page(project: &str, v: &serde_json::Value, limit: u32) -> Result<(), String> {
    let rows = v["items"].as_array().ok_or("Note page rows are missing")?;
    require(
        rows.len() <= limit as usize
            && v["scanned"].as_u64().is_some_and(|n| n <= 200)
            && v["complete"].is_boolean()
            && v["truncated"].is_boolean()
            && !(v["complete"] == true && v["truncated"] == true)
            && v["context_revision"].as_str().is_some_and(sha),
        "Note page completeness is invalid; an incomplete page is not no context",
    )?;
    for n in rows {
        verify_note(project, n, None)?;
    }
    Ok(())
}
fn verify_lifecycle(
    project: &str,
    v: &serde_json::Value,
    refs: &[VersionRef],
    op: &str,
    backup: &str,
    digest: &str,
) -> Result<(), String> {
    require(
        v["operation_id"] == op
            && v["backup_id"] == backup
            && v["backup_sha256"] == digest
            && v["replayed"].is_boolean()
            && v["context_revision"].as_str().is_some_and(sha),
        "Lifecycle receipt differs from its verified backup/operation",
    )?;
    let rows = v["notes"]
        .as_array()
        .ok_or("Lifecycle note receipts are missing")?;
    require(
        rows.len() == refs.len(),
        "Lifecycle receipt omits selected source notes",
    )?;
    let actual = rows
        .iter()
        .filter_map(|r| r["id"].as_str())
        .collect::<std::collections::BTreeSet<_>>();
    require(
        actual
            == refs
                .iter()
                .map(|r| r.id.as_str())
                .collect::<std::collections::BTreeSet<_>>(),
        "Lifecycle receipt names different source notes",
    )?;
    for n in rows {
        verify_note(project, n, None)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn scope() -> Scope {
        Scope {
            kind: ScopeKind::Workstream,
            asset_ids: vec![],
            workstream: Some("MV staking table".into()),
            object_refs: vec![],
            structure_ids: vec![],
            conditions: vec!["Gisagara fill-in replacement equipment".into()],
            user_ref: None,
        }
    }
    fn draft() -> Draft {
        serde_json::from_value(json!({"subject":"Fill-in transformer supply","body":"Replacement equipment stays; transformer supply is excluded.","tags":["MV staking"],"memory":{"schema":MEMORY_SCHEMA,"note_type":"constraint","lifecycle":"active","provenance":{"kind":"user_instruction"},"scope":scope(),"claims":[{"id":"fill-in-supply","kind":"constraint","issue_key":"transformer_supply","text":"Fill-ins may have transformer equipment replacements, but must not have transformer-supply entries.","provenance":{"kind":"user_instruction"},"evidence":[{"kind":"external","locator":"owner-conversation:Gisagara:fill-in-supply","label":"Explicit user instruction; externally referenced evidence"}]}]}})).unwrap()
    }
    fn query() -> Query {
        Query {
            terms: vec!["replacement switching".into()],
            tags: vec!["mv staking".into()],
            tag_match: TagMatch::All,
            note_types: vec![NoteType::Constraint],
            lifecycle: vec![Lifecycle::Active],
            sort: Sort::Updated,
            direction: Direction::Desc,
            cursor: None,
            limit: 20,
        }
    }
    fn view(id: &str, version: u64) -> serde_json::Value {
        json!({"project_id":"p","id":id,"version":version,"document":{"category":"project_note"},"memory":draft().memory,"legacy":false})
    }
    #[test]
    fn individually_sourced_actual_user_ruling_retains_scope_and_rejected_assumption_boundary() {
        let d = draft();
        d.validate().unwrap();
        assert_eq!(
            d.memory.scope.conditions,
            vec!["Gisagara fill-in replacement equipment"]
        );
        let mut wrong = d.clone();
        wrong.memory.claims[0].provenance.kind = Basis::Inference;
        assert!(wrong.validate().is_err());
        wrong = d.clone();
        wrong.memory.claims[0].evidence.clear();
        assert!(wrong.validate().is_err());
    }
    #[test]
    fn public_writes_cannot_forge_server_outcome_or_retire_without_backup() {
        let d = draft();
        let mut raw = serde_json::to_value(&d).unwrap();
        raw["memory"]["claims"][0]["provenance"]["kind"] = json!("server_outcome");
        assert!(serde_json::from_value::<Draft>(raw).is_err());
        let mut d = d;
        d.memory.lifecycle = Lifecycle::Discarded;
        assert!(d.validate().is_err());
    }
    #[test]
    fn query_alias_terms_and_context_scope_round_trip_without_extra_authority() {
        let c = Context {
            query: query(),
            scope: Some(scope()),
        };
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(v["terms"], json!(["replacement switching"]));
        assert!(v.get("query").is_none());
        let restored: Context = serde_json::from_value(v).unwrap();
        assert_eq!(restored, c);
        restored.validate().unwrap();
        let mut retired = c;
        retired.query.lifecycle = vec![Lifecycle::Archived];
        assert!(retired.validate().is_err());
    }
    #[test]
    fn no_match_is_distinct_from_incomplete_or_cross_project_context() {
        let c = Command::Context(Context {
            query: query(),
            scope: Some(scope()),
        });
        let good = json!({"project_id":"p","items":[],"claims":[],"conflicts":[],"scanned":0,"truncated":false,"complete":true,"context_revision":"a".repeat(64)});
        c.verify_response("p", &good).unwrap();
        let mut incomplete = good.clone();
        incomplete["complete"] = json!(false);
        incomplete["truncated"] = json!(true);
        c.verify_response("p", &incomplete).unwrap();
        assert_ne!(incomplete["complete"], true);
        let mut bad = good.clone();
        bad["project_id"] = json!("foreign");
        assert!(c.verify_response("p", &bad).is_err());
        bad = good;
        bad["truncated"] = json!(true);
        assert!(c.verify_response("p", &bad).is_err());
    }
    #[test]
    fn cas_and_replay_receipts_keep_original_version_and_authenticated_project() {
        let w = Write {
            operation_id: "op1".into(),
            id: "n1".into(),
            expected_version: Some(2),
            reason: "Explicit owner correction".into(),
            draft: draft(),
        };
        let c = Command::Write(w);
        c.validate().unwrap();
        let good = json!({"project_id":"p","note":view("n1",3),"operation_id":"op1","replayed":false,"context_revision":"a".repeat(64)});
        c.verify_response("p", &good).unwrap();
        let mut replay = good.clone();
        replay["replayed"] = json!(true);
        c.verify_response("p", &replay).unwrap();
        for field in ["version", "id"] {
            let mut wrong = good.clone();
            wrong["note"][field] = if field == "version" {
                json!(4)
            } else {
                json!("other")
            };
            assert!(c.verify_response("p", &wrong).is_err());
        }
    }
    #[test]
    fn retirement_and_restore_require_exact_verified_backup_and_source_fences() {
        let source = VersionRef {
            id: "n1".into(),
            version: 3,
        };
        let b = Command::Backup(Backup {
            operation_id: "b1".into(),
            sources: vec![source.clone()],
        });
        let good = json!({"project_id":"p","operation_id":"b1","replayed":false,"backup":{"project_id":"p","id":"backup1","sha256":"b".repeat(64),"verified":true,"sources":[source],"history_count":3,"json_bytes":512,"created_at":"2026-10-13T12:00:00Z","created_by":"owner@example.com"}});
        b.verify_response("p", &good).unwrap();
        let mut failed = good.clone();
        failed["backup"]["verified"] = json!(false);
        assert!(b.verify_response("p", &failed).is_err());
        let r = Retire {
            operation_id: "retire1".into(),
            sources: vec![VersionRef {
                id: "n1".into(),
                version: 3,
            }],
            backup_id: "backup1".into(),
            backup_sha256: "b".repeat(64),
            lifecycle: Lifecycle::Archived,
            reason: "Consolidated with source history retained".into(),
            replacement: None,
        };
        r.validate().unwrap();
        let mut missing = r.clone();
        missing.backup_sha256.clear();
        assert!(missing.validate().is_err());
        let restore = Restore {
            operation_id: "restore1".into(),
            backup_id: r.backup_id,
            backup_sha256: r.backup_sha256,
            sources: r.sources,
            expected_versions: vec![VersionRef {
                id: "other".into(),
                version: 4,
            }],
        };
        assert!(restore.validate().is_err());
    }
    #[test]
    fn consolidation_cannot_retire_or_promote_a_proposal() {
        let mut d = draft();
        d.memory.note_type = NoteType::ContextConsolidation;
        let c = Consolidate {
            operation_id: "consolidate1".into(),
            id: "new-note".into(),
            reason: "Concise scoped context".into(),
            draft: d,
            sources: vec![VersionRef {
                id: "n1".into(),
                version: 3,
            }],
        };
        c.validate().unwrap();
        let wire = Command::Consolidate(c.clone()).payload("p").unwrap();
        assert_eq!(wire["action"], "project_note_consolidate");
        assert!(wire["project_note"].get("retire").is_none());
        let mut altered = c;
        altered.draft.memory.claims[0].provenance.kind = Basis::Proposal;
        assert!(altered.validate().is_err());
    }
    #[test]
    fn unknown_fields_and_unpinned_evidence_refuse() {
        let mut evidence: Evidence = serde_json::from_value(
            json!({"kind":"asset","id":"a1","asset_version":2,"sha256":"c".repeat(64)}),
        )
        .unwrap();
        evidence.validate().unwrap();
        evidence.sha256 = None;
        assert!(evidence.validate().is_err());
        assert!(serde_json::from_value::<Read>(json!({"id":"n1","force":true})).is_err());
        let mut q = query();
        q.limit = 51;
        assert!(q.validate().is_err());
    }
}

/// Facts captured by the existing native execution/journal owner. This is not
/// an HTTP DTO and accepts neither raw arguments nor a purported user decision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MutationOutcome {
    pub principal_uid: String,
    pub audience_sha256: String,
    pub lane: String,
    pub project: String,
    pub install_scope: String,
    pub operation_id: String,
    pub command_id: String,
    pub command_contract: u32,
    pub intent_sha256: String,
    pub agent_label: Option<String>,
    pub session_ref: Option<String>,
    pub receipt_sha256: Option<String>,
    pub status: OutcomeStatus,
    pub effect_known: bool,
    pub filed_note_id: Option<String>,
    pub filed_note_version: Option<u64>,
    pub filed_content_sha256: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutcomeStatus {
    Completed,
    Partial,
    Failed,
    Unknown,
}
impl OutcomeStatus {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Partial => "partially completed",
            Self::Failed => "failed",
            Self::Unknown => "unknown after interruption",
        }
    }
}
/// The stable, observation-only filing intent. The journal must persist it
/// before spending a filing call, checkpoint the actual returned note receipt,
/// and retry this note intent only. Never retry the underlying data effect here.
pub struct MutationOutcomePlan {
    pub request: Option<Write>,
    pub content_sha256: String,
    pub note_id: String,
    pub existing_version: Option<u64>,
}
pub fn mutation_outcome_note(o: &MutationOutcome) -> Result<MutationOutcomePlan, String> {
    require(
        id(&o.principal_uid)
            && sha(&o.audience_sha256)
            && matches!(o.lane.as_str(), "stable" | "canary")
            && id(&o.project)
            && id(&o.install_scope)
            && id(&o.operation_id)
            && id(&o.command_id)
            && o.command_contract > 0
            && sha(&o.intent_sha256),
        "Outcome filing needs the captured authenticated principal/project/install and actual invocation identity",
    )?;
    require(
        o.receipt_sha256.as_ref().is_none_or(|s| sha(s))
            && (!matches!(o.status, OutcomeStatus::Completed | OutcomeStatus::Partial)
                || (o.effect_known && o.receipt_sha256.is_some())),
        "Completed/partial outcomes require an actual verified effect receipt; uncertainty stays explicit",
    )?;
    require(
        !matches!(o.status, OutcomeStatus::Unknown) || !o.effect_known,
        "An interrupted unknown outcome cannot assert a known effect",
    )?;
    let scope_bytes = serde_json::to_vec(&serde_json::json!([
        o.principal_uid,
        o.audience_sha256,
        o.lane,
        o.project,
        o.install_scope,
        o.operation_id
    ]))
    .map_err(|e| e.to_string())?;
    use sha2::{Digest, Sha256};
    let note_id = format!("native-outcome-{:x}", Sha256::digest(&scope_bytes));
    require(
        match (
            &o.filed_note_id,
            o.filed_note_version,
            &o.filed_content_sha256,
        ) {
            (None, None, None) => true,
            (Some(id), Some(v), Some(d)) => {
                *id == note_id && v > 0 && v < i64::MAX as u64 && sha(d)
            }
            _ => false,
        },
        "Retained outcome filing pins must cover the exact note/version/content",
    )?;
    let provenance = Provenance {
        kind: Basis::Observation,
        agent_label: o.agent_label.clone(),
        session_ref: o.session_ref.clone(),
        source_author: None,
    };
    provenance.validate()?;
    let known = if o.effect_known {
        "verified native outcome"
    } else {
        "effect not verified; the data operation must not be blindly retried"
    };
    let text = format!(
        "Native command {} (contract {}) reported {}: {}.",
        o.command_id,
        o.command_contract,
        o.status.label(),
        known
    );
    let evidence = vec![Evidence {
        kind: EvidenceKind::External,
        id: None,
        version: None,
        asset_version: None,
        revision_id: None,
        claim_id: None,
        sha256: o.receipt_sha256.clone(),
        locator: Some(format!(
            "ds-native-execution:{}:{}",
            o.command_id, o.operation_id
        )),
        label: Some(
            "Captured native execution receipt; authenticated author is recorded separately".into(),
        ),
    }];
    let draft = Draft {
        subject: format!("{}: {}", o.command_id, o.status.label()),
        body: text.clone(),
        tags: vec![],
        memory: Memory {
            schema: MEMORY_SCHEMA.into(),
            note_type: NoteType::MutationOutcome,
            lifecycle: Lifecycle::Active,
            provenance: provenance.clone(),
            scope: Scope {
                kind: ScopeKind::Project,
                asset_ids: vec![],
                workstream: None,
                object_refs: vec![],
                structure_ids: vec![],
                conditions: vec![],
                user_ref: None,
            },
            claims: vec![Claim {
                id: "native-outcome".into(),
                kind: ClaimKind::Observation,
                issue_key: format!("native_execution:{}", o.command_id),
                text,
                provenance,
                scope: None,
                evidence,
                supersedes: vec![],
            }],
            evidence: vec![],
            supersedes: vec![],
            unresolved: if matches!(o.status, OutcomeStatus::Unknown) {
                vec![
                    "Verify the original data outcome before any attempted mutation replay.".into(),
                ]
            } else {
                vec![]
            },
        },
    };
    let draft_bytes = serde_json::to_vec(&draft).map_err(|e| e.to_string())?;
    let content_sha256 = format!("{:x}", Sha256::digest(&draft_bytes));
    if o.filed_content_sha256.as_deref() == Some(content_sha256.as_str()) {
        return Ok(MutationOutcomePlan {
            request: None,
            content_sha256,
            note_id,
            existing_version: o.filed_note_version,
        });
    }
    let operation_id = format!(
        "note-outcome-{:x}",
        Sha256::digest([scope_bytes.as_slice(), draft_bytes.as_slice()].concat())
    );
    let request = Write {
        operation_id,
        id: note_id,
        expected_version: o.filed_note_version,
        reason: "Record the actual native invocation outcome without inferring a user decision"
            .into(),
        draft,
    };
    request.validate()?;
    Ok(MutationOutcomePlan {
        note_id: request.id.clone(),
        request: Some(request),
        content_sha256,
        existing_version: o.filed_note_version,
    })
}

#[cfg(test)]
mod outcome_tests {
    use super::*;
    fn outcome() -> MutationOutcome {
        MutationOutcome {
            principal_uid: "uid1".into(),
            audience_sha256: "a".repeat(64),
            lane: "stable".into(),
            project: "p".into(),
            install_scope: "install1".into(),
            operation_id: "data1".into(),
            command_id: "design.tag.set".into(),
            command_contract: 1,
            intent_sha256: "b".repeat(64),
            agent_label: Some("agent".into()),
            session_ref: Some("session1".into()),
            receipt_sha256: Some("c".repeat(64)),
            status: OutcomeStatus::Completed,
            effect_known: true,
            filed_note_id: None,
            filed_note_version: None,
            filed_content_sha256: None,
        }
    }
    #[test]
    fn observed_outcome_replay_is_stable_and_same_filed_content_needs_zero_calls() {
        let mut o = outcome();
        let plan = mutation_outcome_note(&o).unwrap();
        let again = mutation_outcome_note(&o).unwrap();
        assert_eq!(plan.request, again.request);
        let request = plan.request.unwrap();
        assert_eq!(request.draft.memory.claims[0].kind, ClaimKind::Observation);
        o.filed_note_id = Some(request.id);
        o.filed_note_version = Some(1);
        o.filed_content_sha256 = Some(plan.content_sha256);
        let filed = mutation_outcome_note(&o).unwrap();
        assert!(filed.request.is_none());
        assert_eq!(filed.existing_version, Some(1));
    }
    #[test]
    fn unknown_interruption_never_claims_completion_or_replays_data() {
        let mut o = outcome();
        o.effect_known = false;
        assert!(mutation_outcome_note(&o).is_err());
        o.status = OutcomeStatus::Unknown;
        o.receipt_sha256 = None;
        let plan = mutation_outcome_note(&o).unwrap();
        let note = plan.request.unwrap();
        assert!(note.draft.body.contains("must not be blindly retried"));
        assert!(!note.draft.memory.unresolved.is_empty());
        o.effect_known = true;
        assert!(mutation_outcome_note(&o).is_err());
    }
    #[test]
    fn changed_checkpoint_updates_same_note_under_last_filed_version() {
        let mut o = outcome();
        o.status = OutcomeStatus::Partial;
        let partial = mutation_outcome_note(&o).unwrap();
        let first = partial.request.unwrap();
        o.filed_note_id = Some(first.id.clone());
        o.filed_note_version = Some(1);
        o.filed_content_sha256 = Some(partial.content_sha256);
        o.status = OutcomeStatus::Completed;
        let complete = mutation_outcome_note(&o).unwrap().request.unwrap();
        assert_eq!(complete.id, first.id);
        assert_eq!(complete.expected_version, Some(1));
        assert_ne!(complete.operation_id, first.operation_id);
    }
}
