//! Where a Design Activities capture lives, how long it lives and when the
//! next one is owed.
//!
//! [`crate::design_activities`] turns one project's status rows into a ledger
//! and folds retained ledgers into an answer. It does not say where a ledger
//! is kept. Two hosts keep them — `ds design activities sweep` under
//! `$XDG_STATE_HOME/ds/design-activities/…` on the Linux server, and the
//! Governance page under an IndexedDB `persist:` key — and both must agree on
//! the same identity, the same retention and the same order of work, or the
//! same owner reading the same captures on two surfaces sees two histories.
//!
//! So the bytes and the atomic write stay with the host, and this module owns
//! the decisions:
//!
//! * `validate` — a snapshot envelope (`ds.design-activities-snapshot/v1`) is
//!   admitted only when it is internally true: its ledger is the project it
//!   names, its digest is the ledger it carries, its counts are the counts.
//! * `digest` — the digest of a ledger, computed here so that a host never
//!   has to reproduce the canonical bytes itself.
//! * `key` — the path segments and the file name a snapshot is stored under.
//!   The account is hashed, never spelled, and the name is the capture time
//!   zero-padded to 13 digits so a directory listing is already in order.
//! * `retain` — which captures survive. The record is the *differences*
//!   between captures, so thinning is not a cache eviction: the last 30 stay
//!   whole, and one capture per UTC day keeps the older shape of the history.
//! * `select` — the latest capture and the one before it, which is the pair
//!   the fold diffs.
//! * `plan` — which projects a sweep should re-capture, in which order, with
//!   the etiquette attached. A full status scan is serialised to ONE slot per
//!   ds-brain instance, so a plan is always `sequential` with a pause; it
//!   never fans out.
//!
//! Pure by the crate's contract: `now_ms` is an input, there is no clock, no
//! randomness and no fetch. Nothing here schedules anything — a sweep happens
//! because a person or an agent asked for one, and the plan only says what
//! that sweep should do.
//!
//! What is NOT claimed (`docs/contracts/design-activities.md`): a snapshot
//! carries no device or installation attribution, because the rows it was
//! built from carry none; nothing that happened between two captures is
//! visible; and a project that refused a capture leaves a gap, never a
//! placeholder.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::design_activities::{Ledger, LEDGER_SCHEMA};
use crate::execution_context::valid_project;

/// The request/reply schema of every action here.
pub const SCHEMA: &str = "ds.design-activities-store/v1";
/// The schema of the envelope a host writes to disk or to IndexedDB.
pub const SNAPSHOT_SCHEMA: &str = "ds.design-activities-snapshot/v1";
/// The first path segment of every stored capture.
pub const ROOT_SEGMENT: &str = "design-activities";

/// A snapshot carries a whole ledger and, optionally, the project's Dashboard
/// model — the same bound the fold carries.
const INPUT_BYTES: usize = 64 * 1024 * 1024;
const DAY_MS: i64 = 24 * 60 * 60 * 1000;
/// The newest captures kept whole.
const DEFAULT_KEEP_LAST: usize = 30;
/// How far back one capture per UTC day is kept.
const DEFAULT_KEEP_DAYS: i64 = 90;
/// A capture older than this is worth taking again.
const DEFAULT_MAX_AGE_MS: i64 = 6 * 60 * 60 * 1000;
/// How many projects one sweep may re-capture.
const DEFAULT_LIMIT: usize = 500;
/// How long a host waits between two full status scans.
const DEFAULT_PAUSE_MS: i64 = 2000;
const MAX_LIMIT: usize = 5000;
const MAX_KEEP_DAYS: i64 = 3650;
const MAX_PAUSE_MS: i64 = 10 * 60 * 1000;
/// Captures are listed, not streamed; a store this large is a host defect.
const MAX_EXISTING: usize = 100_000;
/// The capture time must fit the 13-digit file name that keeps a directory
/// listing in chronological order.
const MAX_CAPTURED_AT_MS: i64 = 9_999_999_999_999;
/// How long one path segment may be, before any host's own limit.
const MAX_SEGMENT_CHARS: usize = 96;
/// How many hex characters of the account digest name a store.
const ACCOUNT_DIGEST_CHARS: usize = 32;

// ── the envelope ────────────────────────────────────────────────────────

/// Which release lane captured the snapshot. Two lanes are two stores: a
/// canary capture never answers for stable.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Lane {
    Stable,
    Canary,
}

impl Lane {
    /// The lane as its one path segment.
    pub const fn as_str(self) -> &'static str {
        match self {
            Lane::Stable => "stable",
            Lane::Canary => "canary",
        }
    }
}

/// Where the diagnostics folded into the retained Dashboard model came from —
/// or that there were none. Named so a reader never mistakes a capture taken
/// without server diagnostics for one whose diagnostics were empty.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticsSource {
    None,
    Server,
    Browser,
}

/// What the capture actually read.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SnapshotSource {
    /// Rows the status list returned, special rows included.
    pub rows: usize,
    pub diagnostics_source: DiagnosticsSource,
    /// A capture written before 2026-09-22 said which processing lane the
    /// host asked for. There is one way to process a transformer now: the
    /// member is read so an older capture still opens, and never written.
    #[serde(default, rename = "fast_lane", skip_serializing)]
    #[allow(dead_code)]
    retired_fast_lane: Retired,
}

/// A member an older record still carries: read from any value, never written.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Retired;

impl<'de> Deserialize<'de> for Retired {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        serde::de::IgnoredAny::deserialize(deserializer).map(|_| Retired)
    }
}

/// One retained capture of one project.
///
/// The envelope is the kernel's own: a host writes exactly these members and
/// nothing else, so an unknown member is refused rather than carried. The
/// ledger inside it is read LENIENTLY (see [`Ledger`]) because a ledger
/// written by a later kernel must still be readable by this one.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub schema: String,
    pub ds_project: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    pub lane: Lane,
    /// When the host captured the rows, epoch millis.
    pub captured_at_ms: i64,
    /// The account the capture was taken as. Not a device: rows carry none.
    pub captured_by: String,
    pub source: SnapshotSource,
    pub ledger: Ledger,
    /// The project's Dashboard model at capture time, when the host folded
    /// one. Carried opaquely: `design_dashboard` owns its shape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dashboard: Option<Value>,
    /// `sha256:<hex>` over the canonical (compact) bytes of `ledger`.
    pub digest: String,
}

/// The digest of a ledger: `sha256:` and the hex of SHA-256 over the
/// canonical compact JSON of the ledger as this crate writes it.
///
/// Hosts do not have to reproduce those bytes — the `digest` action returns
/// this — but a native host holding a [`Ledger`] can call it directly.
pub fn ledger_digest(ledger: &Ledger) -> String {
    let bytes = serde_json::to_vec(ledger).unwrap_or_default();
    format!("sha256:{:x}", Sha256::digest(&bytes))
}

// ── the requests ────────────────────────────────────────────────────────

/// The two members every request carries, read first and leniently so the
/// schema and the action can each be refused by name.
#[derive(Deserialize)]
struct Head {
    #[serde(default)]
    schema: String,
    #[serde(default)]
    action: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ValidateRequest {
    #[allow(dead_code)]
    schema: String,
    #[allow(dead_code)]
    action: String,
    snapshot: Snapshot,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DigestRequest {
    #[allow(dead_code)]
    schema: String,
    #[allow(dead_code)]
    action: String,
    ledger: Ledger,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyRequest {
    #[allow(dead_code)]
    schema: String,
    #[allow(dead_code)]
    action: String,
    /// The signed-in account's stable id. Hashed, never stored in a path.
    principal_uid: String,
    lane: Lane,
    /// The credential audience digest the host is signed in against.
    audience_sha256: String,
    ds_project: String,
    captured_at_ms: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RetainRequest {
    #[allow(dead_code)]
    schema: String,
    #[allow(dead_code)]
    action: String,
    /// Every capture time the store holds for one project.
    existing: Vec<i64>,
    #[serde(default = "default_keep_last")]
    keep_last: usize,
    #[serde(default = "default_keep_days")]
    keep_days: i64,
    now_ms: i64,
}

fn default_keep_last() -> usize {
    DEFAULT_KEEP_LAST
}
fn default_keep_days() -> i64 {
    DEFAULT_KEEP_DAYS
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectRequest {
    #[allow(dead_code)]
    schema: String,
    #[allow(dead_code)]
    action: String,
    existing: Vec<i64>,
}

/// Which projects a sweep is allowed to touch.
#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Bucket {
    #[default]
    Active,
    Archived,
    Testing,
    Template,
    All,
}

impl Bucket {
    /// Whether a project of this status belongs to the bucket. A project
    /// whose status the directory did not carry counts as active, the way
    /// the project list itself reads it.
    fn admits(self, status: Option<&str>) -> bool {
        let status = status.unwrap_or("active").trim();
        match self {
            Bucket::All => true,
            Bucket::Active => status.is_empty() || status == "active",
            Bucket::Archived => status == "archived",
            Bucket::Testing => status == "testing",
            Bucket::Template => status == "template",
        }
    }
}

/// One project the caller can reach, as the directory lists it.
#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct VisibleProject {
    pub ds_project: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
}

/// One capture the store already holds.
#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct CachedCapture {
    pub ds_project: String,
    pub captured_at_ms: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanRequest {
    #[allow(dead_code)]
    schema: String,
    #[allow(dead_code)]
    action: String,
    visible: Vec<VisibleProject>,
    #[serde(default)]
    cached: Vec<CachedCapture>,
    now_ms: i64,
    #[serde(default)]
    bucket: Bucket,
    #[serde(default = "default_max_age_ms")]
    max_age_ms: i64,
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default = "default_pause_ms")]
    pause_ms: i64,
}

fn default_max_age_ms() -> i64 {
    DEFAULT_MAX_AGE_MS
}
fn default_limit() -> usize {
    DEFAULT_LIMIT
}
fn default_pause_ms() -> i64 {
    DEFAULT_PAUSE_MS
}

// ── the replies ─────────────────────────────────────────────────────────

/// What a valid snapshot is, restated from the envelope so a host can record
/// the admission without re-reading the file.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct ValidateReply {
    pub schema: &'static str,
    pub ok: bool,
    pub ds_project: String,
    pub captured_at_ms: i64,
    pub digest: String,
    pub ledger_transformers: usize,
}

/// The digest of a ledger and the two members that identify it.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct DigestReply {
    pub schema: &'static str,
    pub ds_project: String,
    pub captured_at_ms: i64,
    pub digest: String,
    pub ledger_transformers: usize,
}

/// Where one capture is stored: the segments a host joins with its own
/// separator, and the file name inside them.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct KeyReply {
    pub schema: &'static str,
    pub segments: Vec<String>,
    pub file_name: String,
}

/// Which captures survive this retention and which the host must unlink.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct RetainReply {
    pub schema: &'static str,
    pub keep: Vec<i64>,
    pub drop: Vec<i64>,
}

/// The pair the fold diffs.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct SelectReply {
    pub schema: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous: Option<i64>,
}

/// Why a project is in the sweep.
#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RefreshReason {
    /// The store holds no capture of this project at all.
    Missing,
    /// The newest capture is older than `max_age_ms`.
    Stale,
}

/// Why a project is not in the sweep.
#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    /// Its newest capture is young enough.
    Fresh,
    /// It is not in the bucket the caller asked for.
    Bucket,
    /// It was owed a capture but fell outside `limit`. Named rather than
    /// hidden: a project the sweep did not reach must be visible as one.
    Limit,
    /// It is archived. Nobody can upload into an archived project, so its
    /// ledger cannot move and re-reading it can only return what the last
    /// capture already holds. A caller who asks for `--bucket archived` by
    /// name is asking for exactly these and is not skipped.
    Archived,
}

/// One project the sweep should capture.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct RefreshEntry {
    pub ds_project: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub reason: RefreshReason,
}

/// One project the sweep should leave alone.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct SkipEntry {
    pub ds_project: String,
    pub reason: SkipReason,
}

/// The sweep, in order, with its etiquette attached.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct PlanReply {
    pub schema: &'static str,
    pub refresh: Vec<RefreshEntry>,
    pub skip: Vec<SkipEntry>,
    /// How long a host waits between two captures.
    pub pause_ms: i64,
    /// Always true. A full status scan holds the single scan slot of a
    /// ds-brain instance, so captures are taken one after another.
    pub sequential: bool,
}

// ── validate ────────────────────────────────────────────────────────────

fn refuse(field: &str, why: &str) -> String {
    format!("Design activities store refuses `{field}`: {why}.")
}

fn check_snapshot(snapshot: &Snapshot) -> Result<(), String> {
    if snapshot.schema != SNAPSHOT_SCHEMA {
        return Err(refuse(
            "snapshot.schema",
            &format!("`{}` is not {SNAPSHOT_SCHEMA}", snapshot.schema),
        ));
    }
    if !valid_project(&snapshot.ds_project) {
        return Err(refuse(
            "snapshot.ds_project",
            &format!("`{}` is not a project name", snapshot.ds_project),
        ));
    }
    check_captured_at("snapshot.captured_at_ms", snapshot.captured_at_ms)?;
    if snapshot.captured_by.trim().is_empty() {
        return Err(refuse(
            "snapshot.captured_by",
            "a capture is taken as somebody",
        ));
    }
    if snapshot.ledger.schema != LEDGER_SCHEMA {
        return Err(refuse(
            "snapshot.ledger.schema",
            &format!("`{}` is not {LEDGER_SCHEMA}", snapshot.ledger.schema),
        ));
    }
    if snapshot.ledger.ds_project != snapshot.ds_project {
        return Err(refuse(
            "snapshot.ledger.ds_project",
            &format!(
                "`{}` is the ledger of another project than `{}`",
                snapshot.ledger.ds_project, snapshot.ds_project
            ),
        ));
    }
    if snapshot.ledger.captured_at_ms != snapshot.captured_at_ms {
        return Err(refuse(
            "snapshot.ledger.captured_at_ms",
            &format!(
                "the ledger was captured at {} and the envelope says {}",
                snapshot.ledger.captured_at_ms, snapshot.captured_at_ms
            ),
        ));
    }
    if snapshot.ledger.transformer_count != snapshot.ledger.transformers.len() {
        return Err(refuse(
            "snapshot.ledger.transformer_count",
            &format!(
                "the ledger counts {} transformers and carries {}",
                snapshot.ledger.transformer_count,
                snapshot.ledger.transformers.len()
            ),
        ));
    }
    if snapshot.source.rows < snapshot.ledger.row_count {
        return Err(refuse(
            "snapshot.source.rows",
            &format!(
                "the capture read {} rows and the ledger saw {}",
                snapshot.source.rows, snapshot.ledger.row_count
            ),
        ));
    }
    let digest = ledger_digest(&snapshot.ledger);
    if snapshot.digest != digest {
        return Err(refuse(
            "snapshot.digest",
            &format!("`{}` is not the ledger it carries", snapshot.digest),
        ));
    }
    Ok(())
}

fn check_captured_at(field: &str, captured_at_ms: i64) -> Result<(), String> {
    if captured_at_ms <= 0 {
        return Err(refuse(field, "a capture time is a positive instant"));
    }
    if captured_at_ms > MAX_CAPTURED_AT_MS {
        return Err(refuse(
            field,
            &format!("{captured_at_ms} does not fit a 13-digit name"),
        ));
    }
    Ok(())
}

fn build_validate(request: ValidateRequest) -> Result<ValidateReply, String> {
    check_snapshot(&request.snapshot)?;
    Ok(ValidateReply {
        schema: SCHEMA,
        ok: true,
        ds_project: request.snapshot.ds_project,
        captured_at_ms: request.snapshot.captured_at_ms,
        digest: request.snapshot.digest,
        ledger_transformers: request.snapshot.ledger.transformers.len(),
    })
}

fn build_digest(request: DigestRequest) -> Result<DigestReply, String> {
    if request.ledger.schema != LEDGER_SCHEMA {
        return Err(refuse(
            "ledger.schema",
            &format!("`{}` is not {LEDGER_SCHEMA}", request.ledger.schema),
        ));
    }
    if !valid_project(&request.ledger.ds_project) {
        return Err(refuse(
            "ledger.ds_project",
            &format!("`{}` is not a project name", request.ledger.ds_project),
        ));
    }
    Ok(DigestReply {
        schema: SCHEMA,
        ds_project: request.ledger.ds_project.clone(),
        captured_at_ms: request.ledger.captured_at_ms,
        digest: ledger_digest(&request.ledger),
        ledger_transformers: request.ledger.transformers.len(),
    })
}

// ── key ─────────────────────────────────────────────────────────────────

/// Whether a string is one safe path component on every host: not empty, not
/// a traversal, not a separator, nothing invisible, nothing a shell or a
/// listing would render as another name.
fn safe_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment.chars().count() <= MAX_SEGMENT_CHARS
        && segment != "."
        && segment != ".."
        && !segment.ends_with('.')
        && !segment.chars().any(|character| {
            character == '/'
                || character == '\\'
                || character == ':'
                || character.is_whitespace()
                || character.is_control()
        })
}

/// The account's store name: the first [`ACCOUNT_DIGEST_CHARS`] hex
/// characters of SHA-256 over `principal_uid`, a newline and the audience
/// digest. Two accounts, two stores; the same account on two audiences (a
/// canary credential and a stable one) also keeps two, because a capture
/// answers only for the credential that took it.
fn account_segment(principal_uid: &str, audience_sha256: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(principal_uid.as_bytes());
    hasher.update(b"\n");
    hasher.update(audience_sha256.as_bytes());
    let hex = format!("{:x}", hasher.finalize());
    hex[..ACCOUNT_DIGEST_CHARS].to_string()
}

fn build_key(request: KeyRequest) -> Result<KeyReply, String> {
    if request.principal_uid.trim().is_empty() {
        return Err(refuse("principal_uid", "a store belongs to an account"));
    }
    if request.audience_sha256.trim().is_empty() {
        return Err(refuse(
            "audience_sha256",
            "a store belongs to one credential audience",
        ));
    }
    if !valid_project(&request.ds_project) {
        return Err(refuse(
            "ds_project",
            &format!("`{}` is not a project name", request.ds_project),
        ));
    }
    check_captured_at("captured_at_ms", request.captured_at_ms)?;
    let segments = vec![
        ROOT_SEGMENT.to_string(),
        request.lane.as_str().to_string(),
        account_segment(&request.principal_uid, &request.audience_sha256),
        request.ds_project.clone(),
    ];
    for segment in &segments {
        if !safe_segment(segment) {
            return Err(refuse(
                "segments",
                &format!("`{segment}` is not one safe path component"),
            ));
        }
    }
    Ok(KeyReply {
        schema: SCHEMA,
        segments,
        // Zero-padded so a listing sorts chronologically without parsing.
        file_name: format!("{:013}.json", request.captured_at_ms),
    })
}

// ── retain and select ───────────────────────────────────────────────────

/// Every capture time, newest first, without duplicates.
fn newest_first(existing: &[i64], field: &str) -> Result<Vec<i64>, String> {
    if existing.len() > MAX_EXISTING {
        return Err(refuse(
            field,
            &format!("a store of {} captures is not a store", existing.len()),
        ));
    }
    let mut seen: BTreeSet<i64> = BTreeSet::new();
    for captured_at_ms in existing {
        check_captured_at(field, *captured_at_ms)?;
        seen.insert(*captured_at_ms);
    }
    Ok(seen.into_iter().rev().collect())
}

fn build_retain(request: RetainRequest) -> Result<RetainReply, String> {
    if request.keep_last == 0 || request.keep_last > MAX_LIMIT {
        return Err(refuse(
            "keep_last",
            &format!("{} is not between 1 and {MAX_LIMIT}", request.keep_last),
        ));
    }
    if request.keep_days < 1 || request.keep_days > MAX_KEEP_DAYS {
        return Err(refuse(
            "keep_days",
            &format!("{} is not between 1 and {MAX_KEEP_DAYS}", request.keep_days),
        ));
    }
    check_captured_at("now_ms", request.now_ms)?;
    let times = newest_first(&request.existing, "existing")?;
    let now_day = request.now_ms.div_euclid(DAY_MS);
    let mut keep: BTreeSet<i64> = BTreeSet::new();
    let mut day_held: BTreeSet<i64> = BTreeSet::new();
    for (index, captured_at_ms) in times.iter().enumerate() {
        // The newest captures stay whole: they are the pair the fold diffs
        // and the sweeps whose differences are the recent history.
        if index < request.keep_last {
            keep.insert(*captured_at_ms);
        }
        // Then the shape of the older history: the newest capture of each
        // UTC day still inside the window.
        let day = captured_at_ms.div_euclid(DAY_MS);
        if now_day - day < request.keep_days && day_held.insert(day) {
            keep.insert(*captured_at_ms);
        }
    }
    let drop: Vec<i64> = times
        .iter()
        .rev()
        .filter(|captured_at_ms| !keep.contains(captured_at_ms))
        .copied()
        .collect();
    Ok(RetainReply {
        schema: SCHEMA,
        keep: keep.into_iter().collect(),
        drop,
    })
}

fn build_select(request: SelectRequest) -> Result<SelectReply, String> {
    let times = newest_first(&request.existing, "existing")?;
    Ok(SelectReply {
        schema: SCHEMA,
        latest: times.first().copied(),
        previous: times.get(1).copied(),
    })
}

// ── plan ────────────────────────────────────────────────────────────────

fn build_plan(request: PlanRequest) -> Result<PlanReply, String> {
    if request.limit == 0 || request.limit > MAX_LIMIT {
        return Err(refuse(
            "limit",
            &format!("{} is not between 1 and {MAX_LIMIT}", request.limit),
        ));
    }
    if request.pause_ms < 0 || request.pause_ms > MAX_PAUSE_MS {
        return Err(refuse(
            "pause_ms",
            &format!("{} is not between 0 and {MAX_PAUSE_MS}", request.pause_ms),
        ));
    }
    if request.max_age_ms < 0 {
        return Err(refuse("max_age_ms", "an age is not negative"));
    }
    check_captured_at("now_ms", request.now_ms)?;
    if request.visible.len() > MAX_LIMIT {
        return Err(refuse(
            "visible",
            &format!(
                "{} projects is more than {MAX_LIMIT}",
                request.visible.len()
            ),
        ));
    }
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for project in &request.visible {
        if !valid_project(&project.ds_project) {
            return Err(refuse(
                "visible.ds_project",
                &format!("`{}` is not a project name", project.ds_project),
            ));
        }
        if !seen.insert(project.ds_project.as_str()) {
            return Err(refuse(
                "visible.ds_project",
                &format!("`{}` is listed twice", project.ds_project),
            ));
        }
    }
    // The newest capture the store holds per project; older ones do not
    // change whether a capture is owed.
    let mut newest: BTreeMap<&str, i64> = BTreeMap::new();
    for capture in &request.cached {
        if !valid_project(&capture.ds_project) {
            return Err(refuse(
                "cached.ds_project",
                &format!("`{}` is not a project name", capture.ds_project),
            ));
        }
        check_captured_at("cached.captured_at_ms", capture.captured_at_ms)?;
        let held = newest
            .entry(capture.ds_project.as_str())
            .or_insert(i64::MIN);
        *held = (*held).max(capture.captured_at_ms);
    }

    let mut stale: Vec<(i64, RefreshEntry)> = Vec::new();
    let mut missing: Vec<RefreshEntry> = Vec::new();
    let mut skip: Vec<SkipEntry> = Vec::new();
    for project in &request.visible {
        if !request.bucket.admits(project.status.as_deref()) {
            skip.push(SkipEntry {
                ds_project: project.ds_project.clone(),
                reason: SkipReason::Bucket,
            });
            continue;
        }
        // An archived project accepts no uploads, so its ledger cannot change
        // and a second capture cannot differ from the first. Only a caller who
        // named `archived` as the bucket is asking for them.
        if request.bucket != Bucket::Archived
            && project.status.as_deref().map(str::trim) == Some("archived")
        {
            skip.push(SkipEntry {
                ds_project: project.ds_project.clone(),
                reason: SkipReason::Archived,
            });
            continue;
        }
        let entry = |reason| RefreshEntry {
            ds_project: project.ds_project.clone(),
            display_name: project.display_name.clone(),
            reason,
        };
        match newest.get(project.ds_project.as_str()) {
            None => missing.push(entry(RefreshReason::Missing)),
            Some(captured_at_ms) => {
                if request.now_ms - *captured_at_ms > request.max_age_ms {
                    stale.push((*captured_at_ms, entry(RefreshReason::Stale)));
                } else {
                    skip.push(SkipEntry {
                        ds_project: project.ds_project.clone(),
                        reason: SkipReason::Fresh,
                    });
                }
            }
        }
    }
    // Oldest capture first — the project whose record is furthest behind is
    // the one a sweep that is cut short should still have reached — and then
    // the projects with no capture at all, in name order.
    stale.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.ds_project.cmp(&right.1.ds_project))
    });
    missing.sort_by(|left, right| left.ds_project.cmp(&right.ds_project));
    let mut refresh: Vec<RefreshEntry> = stale.into_iter().map(|(_, entry)| entry).collect();
    refresh.extend(missing);
    for entry in refresh.split_off(request.limit.min(refresh.len())) {
        skip.push(SkipEntry {
            ds_project: entry.ds_project,
            reason: SkipReason::Limit,
        });
    }
    Ok(PlanReply {
        schema: SCHEMA,
        refresh,
        skip,
        pause_ms: request.pause_ms,
        sequential: true,
    })
}

// ── the call ────────────────────────────────────────────────────────────

/// Evaluate one store request: `validate` admits a snapshot envelope,
/// `digest` names a ledger's bytes, `key` places a capture, `retain` thins a
/// project's captures, `select` names the pair the fold diffs and `plan`
/// orders the next sweep.
pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > INPUT_BYTES {
        return Err("Design activities store request exceeds its bound.".into());
    }
    let head: Head = serde_json::from_slice(input)
        .map_err(|error| format!("Invalid design activities store request: {error}"))?;
    if head.schema != SCHEMA {
        return Err("Unsupported design activities store schema.".into());
    }
    fn read<T: serde::de::DeserializeOwned>(input: &[u8]) -> Result<T, String> {
        serde_json::from_slice(input)
            .map_err(|error| format!("Invalid design activities store request: {error}"))
    }
    let reply = match head.action.as_str() {
        "validate" => serde_json::to_string(&build_validate(read(input)?)?),
        "digest" => serde_json::to_string(&build_digest(read(input)?)?),
        "key" => serde_json::to_string(&build_key(read(input)?)?),
        "retain" => serde_json::to_string(&build_retain(read(input)?)?),
        "select" => serde_json::to_string(&build_select(read(input)?)?),
        "plan" => serde_json::to_string(&build_plan(read(input)?)?),
        other => {
            return Err(format!(
                "Unsupported design activities store action `{other}`."
            ));
        }
    };
    reply.map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const UID: &str = "uid-0001";
    const AUDIENCE: &str = "1111111111111111111111111111111111111111111111111111111111111111";

    fn run(request: Value) -> Result<Value, String> {
        evaluate(&serde_json::to_vec(&request).unwrap())
            .map(|reply| serde_json::from_str(&reply).expect("reply parses"))
    }

    fn ledger(project: &str, captured_at_ms: i64) -> Value {
        json!({
            "schema": LEDGER_SCHEMA,
            "ds_project": project,
            "captured_at_ms": captured_at_ms,
            "row_count": 2,
            "transformer_count": 1,
            "transformers": [{
                "name": "T-1",
                "phases": {"draft": {"status": "success", "when_ms": captured_at_ms - 1000, "user": "a@x.com"}},
                "health": {"tone": "green", "error_count": 0, "warning_count": 0},
            }],
        })
    }

    fn snapshot(project: &str, captured_at_ms: i64) -> Value {
        let ledger = ledger(project, captured_at_ms);
        let parsed: Ledger = serde_json::from_value(ledger.clone()).expect("ledger reads");
        json!({
            "schema": SNAPSHOT_SCHEMA,
            "ds_project": project,
            "lane": "stable",
            "captured_at_ms": captured_at_ms,
            "captured_by": "owner@example.com",
            "source": {"rows": 2, "diagnostics_source": "none"},
            "ledger": ledger,
            "digest": ledger_digest(&parsed),
        })
    }

    fn validate(snapshot: Value) -> Result<Value, String> {
        run(json!({"schema": SCHEMA, "action": "validate", "snapshot": snapshot}))
    }

    #[test]
    fn a_true_snapshot_is_admitted_and_names_its_digest() {
        let reply = validate(snapshot("arjgpydw_nyam", 1_789_000_000_000)).expect("admitted");
        assert_eq!(reply["ok"], json!(true));
        assert_eq!(reply["ds_project"], "arjgpydw_nyam");
        assert_eq!(reply["ledger_transformers"], 1);
        assert!(reply["digest"]
            .as_str()
            .expect("digest")
            .starts_with("sha256:"));
    }

    #[test]
    fn the_digest_action_names_the_bytes_a_host_would_have_to_reproduce() {
        let captured = 1_789_000_000_000;
        let reply = run(json!({
            "schema": SCHEMA, "action": "digest", "ledger": ledger("arjgpydw_nyam", captured),
        }))
        .expect("digest");
        let admitted = validate(snapshot("arjgpydw_nyam", captured)).expect("admitted");
        assert_eq!(reply["digest"], admitted["digest"]);
        assert_eq!(reply["captured_at_ms"], json!(captured));
    }

    #[test]
    fn a_snapshot_that_is_not_internally_true_is_refused_by_field() {
        let mut wrong = snapshot("arjgpydw_nyam", 1_789_000_000_000);
        wrong["digest"] = json!("sha256:0000");
        assert!(validate(wrong).unwrap_err().contains("`snapshot.digest`"));

        let mut other = snapshot("arjgpydw_nyam", 1_789_000_000_000);
        other["ledger"]["ds_project"] = json!("another");
        assert!(validate(other)
            .unwrap_err()
            .contains("`snapshot.ledger.ds_project`"));

        let mut moment = snapshot("arjgpydw_nyam", 1_789_000_000_000);
        moment["captured_at_ms"] = json!(1_789_000_000_001i64);
        assert!(validate(moment)
            .unwrap_err()
            .contains("`snapshot.ledger.captured_at_ms`"));

        let mut counted = snapshot("arjgpydw_nyam", 1_789_000_000_000);
        counted["ledger"]["transformer_count"] = json!(9);
        assert!(validate(counted)
            .unwrap_err()
            .contains("`snapshot.ledger.transformer_count`"));

        let mut rows = snapshot("arjgpydw_nyam", 1_789_000_000_000);
        rows["source"]["rows"] = json!(1);
        assert!(validate(rows)
            .unwrap_err()
            .contains("`snapshot.source.rows`"));

        let mut schema = snapshot("arjgpydw_nyam", 1_789_000_000_000);
        schema["schema"] = json!("ds.design-activities-snapshot/v0");
        assert!(validate(schema).unwrap_err().contains("`snapshot.schema`"));

        let mut taken_by = snapshot("arjgpydw_nyam", 1_789_000_000_000);
        taken_by["captured_by"] = json!("  ");
        assert!(validate(taken_by)
            .unwrap_err()
            .contains("`snapshot.captured_by`"));

        let mut project = snapshot("arjgpydw_nyam", 1_789_000_000_000);
        project["ds_project"] = json!("../escape");
        project["ledger"]["ds_project"] = json!("../escape");
        assert!(validate(project)
            .unwrap_err()
            .contains("`snapshot.ds_project`"));

        let mut lane = snapshot("arjgpydw_nyam", 1_789_000_000_000);
        lane["lane"] = json!("nightly");
        assert!(validate(lane).unwrap_err().contains("Invalid design"));

        // An unknown envelope member is refused; the one retired member an
        // older capture still carries is read and dropped.
        let mut invented = snapshot("arjgpydw_nyam", 1_789_000_000_000);
        invented["source"]["invented"] = json!(true);
        assert!(validate(invented).is_err());
        let mut older = snapshot("arjgpydw_nyam", 1_789_000_000_000);
        older["source"]["fast_lane"] = json!(true);
        let parsed: Snapshot =
            serde_json::from_value(older.clone()).expect("an older capture reads");
        assert!(!serde_json::to_value(&parsed).unwrap()["source"]
            .as_object()
            .unwrap()
            .contains_key("fast_lane"));
        assert_eq!(validate(older).expect("admitted")["ok"], json!(true));
    }

    #[test]
    fn a_ledger_written_by_a_later_kernel_is_still_read() {
        let mut later = snapshot("arjgpydw_nyam", 1_789_000_000_000);
        later["ledger"]["transformers"][0]["invented_later"] = json!({"a": 1});
        // The unknown member is tolerated but not part of the digest bytes,
        // so the envelope's digest still has to be recomputed by the host.
        let parsed: Ledger = serde_json::from_value(later["ledger"].clone()).expect("lenient");
        later["digest"] = json!(ledger_digest(&parsed));
        assert_eq!(validate(later).expect("admitted")["ok"], json!(true));
    }

    #[test]
    fn a_key_hashes_the_account_and_pads_the_capture_time() {
        let reply = run(json!({
            "schema": SCHEMA, "action": "key", "principal_uid": UID, "lane": "canary",
            "audience_sha256": AUDIENCE, "ds_project": "arjgpydw_nyam",
            "captured_at_ms": 1_789_000_000_000i64,
        }))
        .expect("key");
        let segments: Vec<String> = serde_json::from_value(reply["segments"].clone()).unwrap();
        assert_eq!(segments[0], "design-activities");
        assert_eq!(segments[1], "canary");
        assert_eq!(segments[2].len(), 32);
        assert!(segments[2].chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(segments[3], "arjgpydw_nyam");
        assert_eq!(reply["file_name"], "1789000000000.json");
        assert!(!segments[2].contains(UID));
    }

    #[test]
    fn a_short_capture_time_still_sorts_as_a_name() {
        let reply = run(json!({
            "schema": SCHEMA, "action": "key", "principal_uid": UID, "lane": "stable",
            "audience_sha256": AUDIENCE, "ds_project": "p", "captured_at_ms": 42,
        }))
        .expect("key");
        assert_eq!(reply["file_name"], "0000000000042.json");
    }

    #[test]
    fn two_accounts_and_two_audiences_are_four_stores() {
        let key = |uid: &str, audience: &str| {
            run(json!({
                "schema": SCHEMA, "action": "key", "principal_uid": uid, "lane": "stable",
                "audience_sha256": audience, "ds_project": "p", "captured_at_ms": 1,
            }))
            .expect("key")["segments"][2]
                .as_str()
                .expect("segment")
                .to_string()
        };
        let names = BTreeSet::from([
            key(UID, AUDIENCE),
            key("uid-0002", AUDIENCE),
            key(UID, "2222"),
            key("uid-0002", "2222"),
        ]);
        assert_eq!(names.len(), 4);
    }

    #[test]
    fn a_key_refuses_an_account_a_project_or_a_time_it_cannot_place() {
        let key = |uid: &str, project: &str, captured_at_ms: i64| {
            run(json!({
                "schema": SCHEMA, "action": "key", "principal_uid": uid, "lane": "stable",
                "audience_sha256": AUDIENCE, "ds_project": project,
                "captured_at_ms": captured_at_ms,
            }))
        };
        assert!(key("", "p", 1).unwrap_err().contains("`principal_uid`"));
        for project in ["..", "../escape", "a/b", "p.", "", "a\u{200b}b", "a b"] {
            assert!(
                key(UID, project, 1).is_err(),
                "`{project}` reached a path segment"
            );
        }
        assert!(key(UID, "p", 0).unwrap_err().contains("`captured_at_ms`"));
        assert!(key(UID, "p", -1).unwrap_err().contains("`captured_at_ms`"));
        assert!(key(UID, "p", 10_000_000_000_000)
            .unwrap_err()
            .contains("13-digit"));
        assert!(run(json!({
            "schema": SCHEMA, "action": "key", "principal_uid": UID, "lane": "stable",
            "audience_sha256": "", "ds_project": "p", "captured_at_ms": 1,
        }))
        .unwrap_err()
        .contains("`audience_sha256`"));
    }

    fn retain(existing: Vec<i64>, keep_last: usize, keep_days: i64, now_ms: i64) -> Value {
        run(json!({
            "schema": SCHEMA, "action": "retain", "existing": existing,
            "keep_last": keep_last, "keep_days": keep_days, "now_ms": now_ms,
        }))
        .expect("retain")
    }

    #[test]
    fn retention_keeps_the_last_thirty_and_one_capture_a_day() {
        // 40 captures, four a day over ten days, ending at `now`.
        let now = 400 * DAY_MS;
        let mut existing = Vec::new();
        for day in 0..10i64 {
            for slot in 0..4i64 {
                existing.push((390 + day) * DAY_MS + slot * 6 * 60 * 60 * 1000);
            }
        }
        let reply = retain(existing.clone(), 30, 90, now);
        let keep: Vec<i64> = serde_json::from_value(reply["keep"].clone()).unwrap();
        let dropped: Vec<i64> = serde_json::from_value(reply["drop"].clone()).unwrap();
        assert_eq!(keep.len() + dropped.len(), existing.len());
        // The newest 30 are whole — seven and a half days of them — and the
        // two days they do not reach keep their newest capture each.
        assert_eq!(keep.len(), 32);
        assert!(keep.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(dropped.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(*keep.last().unwrap(), *existing.iter().max().unwrap());
        for day in 390..400i64 {
            let newest = existing
                .iter()
                .filter(|ms| ms.div_euclid(DAY_MS) == day)
                .max()
                .copied()
                .unwrap();
            assert!(keep.contains(&newest), "day {day} lost its newest capture");
        }
        // Idempotent: retaining what was kept keeps exactly that.
        let again = retain(keep.clone(), 30, 90, now);
        assert_eq!(again["keep"], json!(keep));
        assert_eq!(again["drop"], json!([] as [i64; 0]));
    }

    #[test]
    fn retention_forgets_a_day_outside_the_window_but_never_the_newest() {
        let now = 400 * DAY_MS;
        let ancient = 100 * DAY_MS;
        let reply = retain(vec![ancient, now - DAY_MS, now], 2, 90, now);
        assert_eq!(reply["keep"], json!([now - DAY_MS, now]));
        assert_eq!(reply["drop"], json!([ancient]));
    }

    #[test]
    fn retention_of_nothing_of_one_and_of_exactly_the_last_is_stable() {
        let now = 400 * DAY_MS;
        let empty = retain(vec![], 30, 90, now);
        assert_eq!(empty["keep"], json!([] as [i64; 0]));
        assert_eq!(empty["drop"], json!([] as [i64; 0]));
        let one = retain(vec![now], 30, 90, now);
        assert_eq!(one["keep"], json!([now]));
        let thirty: Vec<i64> = (0..30).map(|index| now - index * 60_000).collect();
        let reply = retain(thirty.clone(), 30, 90, now);
        assert_eq!(reply["keep"].as_array().unwrap().len(), 30);
        assert_eq!(reply["drop"], json!([] as [i64; 0]));
        // A thirty-first capture on the same day is dropped: the day is
        // already represented and the newest thirty are already whole.
        let mut thirty_one = thirty;
        thirty_one.push(now - 31 * 60_000);
        let reply = retain(thirty_one, 30, 90, now);
        assert_eq!(reply["drop"], json!([now - 31 * 60_000]));
    }

    #[test]
    fn retention_deduplicates_and_refuses_what_it_cannot_place() {
        let now = 400 * DAY_MS;
        let reply = retain(vec![now, now, now - 1], 30, 90, now);
        assert_eq!(reply["keep"], json!([now - 1, now]));
        for (keep_last, keep_days) in [(0usize, 90i64), (5001, 90), (30, 0), (30, 4000)] {
            assert!(run(json!({
                "schema": SCHEMA, "action": "retain", "existing": [now],
                "keep_last": keep_last, "keep_days": keep_days, "now_ms": now,
            }))
            .is_err());
        }
        assert!(run(json!({
            "schema": SCHEMA, "action": "retain", "existing": [0], "now_ms": now,
        }))
        .unwrap_err()
        .contains("`existing`"));
    }

    #[test]
    fn select_names_the_pair_the_fold_diffs() {
        let select = |existing: Value| {
            run(json!({"schema": SCHEMA, "action": "select", "existing": existing}))
                .expect("select")
        };
        assert_eq!(select(json!([])), json!({"schema": SCHEMA}));
        assert_eq!(select(json!([7])), json!({"schema": SCHEMA, "latest": 7}));
        assert_eq!(
            select(json!([5, 9, 9, 1])),
            json!({"schema": SCHEMA, "latest": 9, "previous": 5})
        );
    }

    fn plan(request: Value) -> Result<Value, String> {
        run(request)
    }

    #[test]
    fn a_plan_asks_for_the_missing_and_the_stale_and_names_every_skip() {
        let now = 1_789_000_000_000i64;
        let reply = plan(json!({
            "schema": SCHEMA, "action": "plan", "now_ms": now,
            "visible": [
                {"ds_project": "fresh", "display_name": "Fresh"},
                {"ds_project": "old", "status": "active"},
                {"ds_project": "older"},
                {"ds_project": "never", "display_name": "Never"},
                {"ds_project": "shelved", "status": "archived"},
            ],
            "cached": [
                {"ds_project": "fresh", "captured_at_ms": now - 60_000},
                {"ds_project": "old", "captured_at_ms": now - 8 * 60 * 60 * 1000},
                {"ds_project": "older", "captured_at_ms": now - 48 * 60 * 60 * 1000},
                {"ds_project": "shelved", "captured_at_ms": 1},
            ],
        }))
        .expect("plan");
        assert_eq!(reply["sequential"], json!(true));
        assert_eq!(reply["pause_ms"], json!(2000));
        assert_eq!(
            reply["refresh"],
            json!([
                {"ds_project": "older", "reason": "stale"},
                {"ds_project": "old", "reason": "stale"},
                {"ds_project": "never", "display_name": "Never", "reason": "missing"},
            ])
        );
        assert_eq!(
            reply["skip"],
            json!([
                {"ds_project": "fresh", "reason": "fresh"},
                {"ds_project": "shelved", "reason": "bucket"},
            ])
        );
    }

    /// Nobody can upload into an archived project, so a second capture can
    /// only repeat the first. `all` therefore means "everything whose ledger
    /// can still move", and the one way to reach an archived project is to
    /// name it as the bucket.
    #[test]
    fn an_archived_project_is_swept_only_when_the_caller_names_that_bucket() {
        let visible = json!([
            {"ds_project": "live", "status": "active"},
            {"ds_project": "shelved", "status": "archived"},
        ]);
        let planned = |bucket: &str| {
            plan(json!({
                "schema": SCHEMA, "action": "plan", "now_ms": 10_000, "max_age_ms": 1,
                "bucket": bucket, "visible": visible, "cached": [],
            }))
            .expect("plan")
        };

        let all = planned("all");
        assert_eq!(
            all["refresh"]
                .as_array()
                .expect("refresh")
                .iter()
                .map(|row| row["ds_project"].as_str().expect("name"))
                .collect::<Vec<_>>(),
            ["live"],
            "`all` swept an archived project whose ledger cannot move",
        );
        assert_eq!(
            all["skip"],
            json!([{"ds_project": "shelved", "reason": "archived"}]),
            "an archived project must be skipped AS archived, not as a bucket miss",
        );

        // Named by the caller, it is exactly what was asked for.
        assert_eq!(
            planned("archived")["refresh"]
                .as_array()
                .expect("refresh")
                .iter()
                .map(|row| row["ds_project"].as_str().expect("name"))
                .collect::<Vec<_>>(),
            ["shelved"],
        );
    }

    #[test]
    fn a_bucket_is_what_the_caller_asked_for_and_no_status_is_active() {
        let now = 1_789_000_000_000i64;
        let visible = json!([
            {"ds_project": "plain"},
            {"ds_project": "live", "status": "active"},
            {"ds_project": "shelved", "status": "archived"},
            {"ds_project": "trial", "status": "testing"},
            {"ds_project": "seed", "status": "template"},
        ]);
        let names = |bucket: &str| -> Vec<String> {
            plan(json!({
                "schema": SCHEMA, "action": "plan", "now_ms": now,
                "bucket": bucket, "visible": visible, "cached": [],
            }))
            .expect("plan")["refresh"]
                .as_array()
                .unwrap()
                .iter()
                .map(|entry| entry["ds_project"].as_str().unwrap().to_string())
                .collect()
        };
        assert_eq!(names("active"), ["live", "plain"]);
        assert_eq!(names("archived"), ["shelved"]);
        assert_eq!(names("testing"), ["trial"]);
        assert_eq!(names("template"), ["seed"]);
        // `all` is every project whose ledger can still move. An archived
        // project accepts no uploads, so it is reached only by name.
        assert_eq!(names("all"), ["live", "plain", "seed", "trial"]);
    }

    #[test]
    fn a_limit_leaves_the_projects_it_cut_visible_as_skipped() {
        let now = 1_789_000_000_000i64;
        let reply = plan(json!({
            "schema": SCHEMA, "action": "plan", "now_ms": now, "limit": 1, "pause_ms": 5000,
            "visible": [{"ds_project": "a"}, {"ds_project": "b"}, {"ds_project": "c"}],
            "cached": [{"ds_project": "b", "captured_at_ms": 1}],
        }))
        .expect("plan");
        assert_eq!(
            reply["refresh"],
            json!([{"ds_project": "b", "reason": "stale"}])
        );
        assert_eq!(
            reply["skip"],
            json!([
                {"ds_project": "a", "reason": "limit"},
                {"ds_project": "c", "reason": "limit"},
            ])
        );
        assert_eq!(reply["pause_ms"], json!(5000));
    }

    #[test]
    fn the_newest_capture_of_a_project_decides_whether_one_is_owed() {
        let now = 1_789_000_000_000i64;
        let reply = plan(json!({
            "schema": SCHEMA, "action": "plan", "now_ms": now,
            "visible": [{"ds_project": "p"}],
            "cached": [
                {"ds_project": "p", "captured_at_ms": 1},
                {"ds_project": "p", "captured_at_ms": now - 1000},
            ],
        }))
        .expect("plan");
        assert_eq!(reply["refresh"], json!([] as [Value; 0]));
        assert_eq!(
            reply["skip"],
            json!([{"ds_project": "p", "reason": "fresh"}])
        );
    }

    #[test]
    fn a_plan_refuses_a_directory_it_cannot_trust() {
        let now = 1_789_000_000_000i64;
        let base = |extra: Value| {
            let mut request = json!({
                "schema": SCHEMA, "action": "plan", "now_ms": now,
                "visible": [{"ds_project": "p"}], "cached": [],
            });
            for (key, value) in extra.as_object().unwrap() {
                request[key] = value.clone();
            }
            run(request)
        };
        assert!(
            base(json!({"visible": [{"ds_project": "p"}, {"ds_project": "p"}]}))
                .unwrap_err()
                .contains("listed twice")
        );
        assert!(base(json!({"visible": [{"ds_project": "../x"}]}))
            .unwrap_err()
            .contains("`visible.ds_project`"));
        assert!(
            base(json!({"cached": [{"ds_project": "..", "captured_at_ms": 1}]}))
                .unwrap_err()
                .contains("`cached.ds_project`")
        );
        assert!(base(json!({"limit": 0})).unwrap_err().contains("`limit`"));
        assert!(base(json!({"limit": 5001}))
            .unwrap_err()
            .contains("`limit`"));
        assert!(base(json!({"pause_ms": -1}))
            .unwrap_err()
            .contains("`pause_ms`"));
        assert!(base(json!({"pause_ms": 600_001}))
            .unwrap_err()
            .contains("`pause_ms`"));
        assert!(base(json!({"max_age_ms": -1}))
            .unwrap_err()
            .contains("`max_age_ms`"));
        assert!(base(json!({"bucket": "everything"})).is_err());
    }

    #[test]
    fn a_request_of_another_schema_action_or_shape_is_refused() {
        assert!(evaluate(b"{}").is_err());
        assert!(evaluate(br#"{"schema":"nope","action":"select"}"#)
            .unwrap_err()
            .contains("schema"));
        assert!(
            evaluate(br#"{"schema":"ds.design-activities-store/v1","action":"sweep"}"#)
                .unwrap_err()
                .contains("action `sweep`")
        );
        assert!(evaluate(
            br#"{"schema":"ds.design-activities-store/v1","action":"select","existing":[],"nope":1}"#
        )
        .is_err());
    }
}
