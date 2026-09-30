//! The local DS Grid model workspace: what a machine holds before anything is
//! published, answered without an application.
//!
//! # Why this module exists
//!
//! A local model is a `.dsgrid` working copy that belongs to a machine, not to
//! a project: created empty, imported from a file, or downloaded from a
//! project's governed head to work on. The browser keeps that catalogue in
//! IndexedDB, which is why `ds dsgrid model list|create-local|import-external|
//! set-active` asked the paired application for it — and therefore refused
//! `desktop_not_paired` on a server, where an engineer most naturally holds a
//! working copy. Model management was the last part of the DS Grid domain that
//! could not answer without a window.
//!
//! So the catalogue becomes a fact of the machine, kept by `ds-layer-store`
//! beside the local-layer catalogue it already keeps, and every decision about
//! it is made here: the id, the display name, the active pointer, what a
//! duplicate is, and what each refusal is called. The CLI and the UI both ask
//! this module; whichever host is running it — server, desktop or cloud — does
//! the IO and owns the package bytes. This module never reads a `.dsgrid`,
//! because the kernel does not link the engine that understands one
//! (`ds-network`).
//!
//! # What is deliberately NOT here
//!
//! - **Package validity.** Whether bytes are a readable `.dsgrid`, what CRS it
//!   declares and what its authored revision is belongs to `ds-grid-exchange`.
//!   A host validates first and hands this module the identity it read.
//! - **Governed project models.** `grid_models` in the catalog is the project's
//!   authority; a local copy pinned to a governed revision records that pin and
//!   claims nothing more.
//! - **The browser's catalogue.** One host's local workspace is not another's.
//!   Two machines holding a working copy of one model is ordinary.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

mod drafts;
pub use drafts::*;

/// The longest display name, id or CRS token this catalogue keeps.
pub const MAX_NAME_CHARS: usize = 200;
/// The most models one machine's catalogue may hold. A machine that reaches it
/// is keeping working copies it has stopped using; the refusal names that
/// rather than letting a listing grow without bound.
pub const MAX_MODELS: usize = 500;

/// A workspace path may be long on a shared drive; it is still one line.
pub const MAX_PATH_CHARS: usize = 1_024;
/// The most native member families one link records (a 16.81 workspace
/// carries about a dozen: DON, NUM, XYZ, TIN, CRI, FEA, STR, PPS, BRK, CON,
/// STRUCT, CABLE …).
pub const MAX_MEMBER_FAMILIES: usize = 64;

pub const SCHEMA: &str = "ds.local-models/v1";

// ── Refusals ────────────────────────────────────────────────────────────

pub const SCOPE_MISMATCH: &str = "local_model_scope_mismatch";
pub const UNKNOWN_MODEL: &str = "local_model_not_found";
pub const DUPLICATE_NAME: &str = "local_model_name_taken";
pub const CATALOGUE_FULL: &str = "local_model_catalogue_full";
pub const MALFORMED: &str = "local_model_request_invalid";
/// A revision was built on a package the working copy no longer holds:
/// another writer advanced it between the caller's read and its write.
pub const REVISION_CONFLICT: &str = "local_model_revision_conflict";

/// Why an operation changed nothing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Refusal {
    pub code: &'static str,
    pub message: String,
}

fn refusal<T>(code: &'static str, message: impl Into<String>) -> Result<T, Refusal> {
    Err(Refusal {
        code,
        message: message.into(),
    })
}

fn bounded(value: &str) -> bool {
    !value.is_empty()
        && value.trim() == value
        && value.chars().count() <= MAX_NAME_CHARS
        && !value.chars().any(char::is_control)
}

/// A workspace path: one non-empty trimmed line, longer than a name may be.
fn bounded_path(value: &str) -> bool {
    !value.is_empty()
        && value.trim() == value
        && value.chars().count() <= MAX_PATH_CHARS
        && !value.chars().any(char::is_control)
}

/// `sha256:` followed by 64 lowercase hex digits — the exchange's spelling.
pub fn digest_is_wellformed(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// An id names a file on a host that keeps files: the store writes
/// `<id>.dsgrid` under its payload directory. So an id is ONE path segment
/// carrying no separator, no traversal and no surprise.
pub fn id_is_safe(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id != "."
        && id != ".."
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

// ── The catalogue ───────────────────────────────────────────────────────

/// Where a local model came from. It decides nothing on its own; it is what a
/// listing shows an operator who is deciding which copy to work on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// Authored empty on this machine.
    Created,
    /// Acquired from a `.dsgrid` file the operator named.
    Imported,
    /// Downloaded from a project's governed head.
    Project,
}

/// One working copy this machine holds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalModel {
    pub id: String,
    pub display_name: String,
    pub origin: Origin,
    /// The coordinate system the package declares, as the engine read it.
    pub crs: String,
    /// The package's own monotonic revision, as the engine read it.
    pub model_revision: u64,
    pub bytes: u64,
    pub sha256: String,
    /// RFC3339, supplied by the host: the kernel has no clock.
    pub created_at: String,
    /// Set only for `Origin::Project`: the exact governed revision this copy
    /// was taken from. A copy that has been edited since keeps the pin, because
    /// the pin says where it CAME from, never what it still equals.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<ProjectPin>,
    /// The authored head revision the engine reported for the package as it
    /// stands (`rev:…`), when the host opened the engine to read it. `None`
    /// for a copy registered without one; a listing prints what it has and
    /// never invents a head.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_revision: Option<String>,
    /// RFC3339, supplied by the host at the last `Revise`; absent for a copy
    /// never revised in place.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revised_at: Option<String>,
    /// The live PLS-CADD workspace this copy is linked to, when it is one. A
    /// link is what lets `ds dsgrid-exchange sync` write DS edits back INTO
    /// that workspace instead of into a new folder, and what lets `pull` read
    /// it again after a PLS-CADD session. The digest pins the member tree the
    /// link was made against; a workspace whose bytes moved since is refused,
    /// never silently overwritten.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pls_source: Option<PlsSourceLink>,
    /// Independently pinned local recovery content; never a validated package.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft: Option<LocalDraft>,
}

/// One PLS-CADD workspace a working copy is linked to (contract 02 §1).
///
/// The kernel records what a host established: the folder path as the host
/// names it, the exchange digest of the member tree (`sha256:…`, the same
/// digest `ds dsgrid-exchange inspect` prints for that folder), the PLS-CADD
/// program version the DON declares, and each native member family's
/// `TYPE`/`VERSION` (`DON` → `57`, `CRI` → `94`, `FEA` → `15`, …). Nothing here
/// is derived by the kernel: it links no engine and reads no bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlsSourceLink {
    /// The workspace folder, as the host resolved it. Never relative.
    pub path: String,
    /// `sha256:<hex>` over the workspace member tree, as the exchange digests
    /// a folder source. Sync and pull refuse when the folder no longer
    /// digests to this.
    pub digest: String,
    /// The PLS-CADD program version the workspace's DON declares
    /// (`SOURCE='PLS-CADD Version 16.81'` → `16.81`).
    pub pls_version: String,
    /// Native member family → member VERSION, one entry per family the
    /// workspace carries: `{"DON": "57", "NUM": "14", "CRI": "94", "FEA": "15",
    /// "STRUCT": "13", "XYZ": "5", …}`. Families are the header's TYPE with
    /// the ` FILE` suffix removed.
    #[serde(default)]
    pub member_versions: BTreeMap<String, String>,
    /// How many members the linked tree held when it was pinned.
    pub member_count: u64,
    /// RFC3339, supplied by the host: the kernel has no clock.
    pub linked_at: String,
}

impl PlsSourceLink {
    fn validate(&self) -> Result<(), Refusal> {
        if !bounded_path(&self.path)
            || !digest_is_wellformed(&self.digest)
            || !bounded(&self.pls_version)
            || !bounded(&self.linked_at)
            || self.member_count == 0
            || self.member_versions.len() > MAX_MEMBER_FAMILIES
            || self
                .member_versions
                .iter()
                .any(|(family, version)| !bounded(family) || !bounded(version))
        {
            return refusal(
                MALFORMED,
                "a PLS source link needs an absolute path, a sha256 digest, the PLS-CADD \
                 version, at least one member and the host's clock",
            );
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectPin {
    pub project_id: String,
    pub model_id: String,
    pub revision_id: String,
    pub digest: String,
}

/// Whose catalogue this is. A native store is keyed by lane and DS account, so
/// one machine shared by two accounts never shows one operator the other's
/// working copies.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub lane: String,
    pub uid: String,
}

impl Scope {
    fn validate(&self) -> Result<(), Refusal> {
        if bounded(&self.lane) && bounded(&self.uid) {
            Ok(())
        } else {
            refusal(
                MALFORMED,
                "a local model scope needs a bounded lane and uid",
            )
        }
    }
}

/// The catalogue a machine keeps, and the only thing an operation rewrites.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalogue {
    pub schema: String,
    pub scope: Scope,
    #[serde(default)]
    pub models: Vec<LocalModel>,
    /// The model an editing session opens by default. `None` is ordinary: a
    /// machine with no working copy has nothing active, and so is a machine
    /// whose active copy was just removed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<String>,
}

impl Catalogue {
    pub fn empty(scope: Scope) -> Self {
        Self {
            schema: SCHEMA.to_owned(),
            scope,
            models: Vec::new(),
            active: None,
        }
    }

    /// Read one row with the same account/lane fence as a catalogue decision.
    pub fn model(&self, scope: &Scope, id: &str) -> Result<&LocalModel, Refusal> {
        scope.validate()?;
        self.fence(scope)?;
        Ok(&self.models[self.position(id)?])
    }

    /// Every operation names the scope it believes it is in, so a catalogue
    /// belonging to another lane or account is never silently rewritten.
    fn fence(&self, scope: &Scope) -> Result<(), Refusal> {
        if &self.scope == scope {
            Ok(())
        } else {
            refusal(
                SCOPE_MISMATCH,
                "this catalogue belongs to another lane or DS account",
            )
        }
    }

    fn position(&self, id: &str) -> Result<usize, Refusal> {
        self.models
            .iter()
            .position(|model| model.id == id)
            .ok_or_else(|| Refusal {
                code: UNKNOWN_MODEL,
                message: format!("no local model {id} on this machine"),
            })
    }

    /// A name an operator can tell apart. Two working copies of one governed
    /// model are ordinary, so the check is on the name the operator chose, and
    /// the remedy is to choose another.
    fn refuse_duplicate_name(&self, name: &str, except: Option<&str>) -> Result<(), Refusal> {
        let taken = self.models.iter().any(|model| {
            model.display_name.eq_ignore_ascii_case(name) && Some(model.id.as_str()) != except
        });
        if taken {
            return refusal(
                DUPLICATE_NAME,
                format!("this machine already holds a local model named {name}"),
            );
        }
        Ok(())
    }
}

// ── Operations ──────────────────────────────────────────────────────────

/// What a host asks for. Each variant carries the facts the host has already
/// established — the engine's reading of the package, the clock, the minted id
/// — because this module has neither an engine nor a clock.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Op {
    /// Register a model the host has just written or copied into the store.
    Register {
        id: String,
        display_name: String,
        origin: Origin,
        crs: String,
        model_revision: u64,
        bytes: u64,
        sha256: String,
        /// RFC3339. `None` asks the host to read its own clock once, before
        /// anything is decided, the way `ds-layer-store` already does for a
        /// prepared local layer — the kernel has no clock and will refuse a
        /// registration that still carries none.
        created_at: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project: Option<ProjectPin>,
        /// The authored head the engine read off the package, when the host
        /// opened it. Optional: acquisition never has to open the engine.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        head_revision: Option<String>,
        /// Open it as the active model once it is registered. A machine's first
        /// model is active whether or not this is set: a catalogue with exactly
        /// one working copy and nothing active would be a state an operator has
        /// no way to read as deliberate.
        #[serde(default)]
        activate: bool,
    },
    /// Advance one working copy in place: the host has written a new package
    /// for the same id and hands over its identity. The row keeps its id,
    /// name, origin, coordinate system, creation time and project pin; the
    /// package revision, byte count, digest and head move. `expected_sha256`
    /// is the digest the caller read before building the revision — when the
    /// row no longer carries it, another writer got there first and the
    /// operation is refused rather than silently overwriting their work.
    Revise {
        id: String,
        expected_sha256: Option<String>,
        model_revision: u64,
        bytes: u64,
        sha256: String,
        head_revision: Option<String>,
        /// RFC3339. `None` asks the host for its clock, exactly as `Register`.
        revised_at: Option<String>,
    },
    /// Pin local recovery bytes after the host has durably staged them.
    /// Both the previous pin and immutable base identity must still match.
    StageDraft {
        id: String,
        expected_base: PackageIdentity,
        expected_draft: Option<DraftCheckpointPin>,
        draft: LocalDraft,
    },
    /// Open one existing model as the active one.
    SetActive { id: String },
    /// Forget one model. The host deletes its bytes after this answers.
    Forget { id: String },
    /// Link one existing model to a live PLS-CADD workspace, or replace the
    /// link it already carries. `linked_at: None` asks the host for its clock
    /// exactly as `Register` does for `created_at`.
    Link {
        id: String,
        pls_source: PlsSourceLink,
    },
    /// Forget the workspace link of one model. The working copy itself stays.
    Unlink { id: String },
}

/// What changed, for a host that must now write bytes or delete them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Outcome {
    pub catalogue: Catalogue,
    /// The model the operation was about.
    pub model: Option<LocalModel>,
    /// True when `active` now names a different model than before.
    pub active_changed: bool,
}

/// Apply one operation to one catalogue.
///
/// Pure: no clock, no filesystem, no network. The host persists
/// `outcome.catalogue` and, for `Forget`, removes the payload afterwards — in
/// that order, so a crash between them leaves a file with no row rather than a
/// row with no file.
pub fn evaluate(catalogue: &Catalogue, scope: &Scope, op: Op) -> Result<Outcome, Refusal> {
    scope.validate()?;
    catalogue.fence(scope)?;
    let mut next = catalogue.clone();
    let before = next.active.clone();

    let model = match op {
        Op::Register {
            id,
            display_name,
            origin,
            crs,
            model_revision,
            bytes,
            sha256,
            created_at,
            project,
            head_revision,
            activate,
        } => {
            if !id_is_safe(&id) {
                return refusal(
                    MALFORMED,
                    "a local model id is one lowercase path segment of at most 64 characters",
                );
            }
            let created_at = created_at.filter(|value| bounded(value));
            if !bounded(&display_name) || !bounded(&crs) || created_at.is_none() {
                return refusal(
                    MALFORMED,
                    "a local model needs a bounded name, coordinate system and creation time",
                );
            }
            let created_at = created_at.expect("checked above");
            if bytes == 0 {
                return refusal(MALFORMED, "a local model package cannot be empty");
            }
            if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
                return refusal(MALFORMED, "a local model needs its package digest");
            }
            if next.models.iter().any(|model| model.id == id) {
                return refusal(
                    MALFORMED,
                    format!("local model {id} is already registered on this machine"),
                );
            }
            if next.models.len() >= MAX_MODELS {
                return refusal(
                    CATALOGUE_FULL,
                    format!(
                        "this machine already holds {MAX_MODELS} local models; \
                         remove one you have finished with"
                    ),
                );
            }
            next.refuse_duplicate_name(&display_name, None)?;
            let model = LocalModel {
                id,
                display_name,
                origin,
                crs,
                model_revision,
                bytes,
                sha256,
                created_at,
                project,
                head_revision: head_revision.filter(|value| bounded(value)),
                revised_at: None,
                // Acquisition never links: the link is a separate, explicit
                // act whose digest the host has just verified.
                pls_source: None,
                draft: None,
            };
            if activate || next.models.is_empty() {
                next.active = Some(model.id.clone());
            }
            next.models.push(model.clone());
            Some(model)
        }
        Op::Revise {
            id,
            expected_sha256,
            model_revision,
            bytes,
            sha256,
            head_revision,
            revised_at,
        } => {
            let index = next.position(&id)?;
            if bytes == 0 {
                return refusal(MALFORMED, "a revised package cannot be empty");
            }
            if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
                return refusal(MALFORMED, "a revised package needs its digest");
            }
            let revised_at = revised_at.filter(|value| bounded(value));
            if revised_at.is_none() {
                return refusal(MALFORMED, "a revision needs the host's clock");
            }
            let row = &mut next.models[index];
            if row.draft.is_some() {
                return refusal(
                    DRAFT_PENDING,
                    "a pending local draft must be resolved before revising its base package",
                );
            }
            if let Some(expected) = expected_sha256 {
                if expected != row.sha256 {
                    return refusal(
                        REVISION_CONFLICT,
                        format!(
                            "local model {id} moved: the revision was built on package {} but the working copy now holds {}",
                            &expected[..expected.len().min(12)],
                            &row.sha256[..row.sha256.len().min(12)]
                        ),
                    );
                }
            }
            if model_revision < row.model_revision {
                return refusal(
                    MALFORMED,
                    format!(
                        "a revision never goes backwards: package revision {model_revision} is below the working copy's {}",
                        row.model_revision
                    ),
                );
            }
            row.model_revision = model_revision;
            row.bytes = bytes;
            row.sha256 = sha256;
            row.head_revision = head_revision.filter(|value| bounded(value));
            row.revised_at = revised_at;
            Some(row.clone())
        }
        Op::StageDraft {
            id,
            expected_base,
            expected_draft,
            draft,
        } => {
            let index = next.position(&id)?;
            drafts::advance(
                &mut next.models[index],
                &expected_base,
                expected_draft.as_ref(),
                draft,
            )?;
            Some(next.models[index].clone())
        }
        Op::SetActive { id } => {
            let index = next.position(&id)?;
            next.active = Some(id);
            Some(next.models[index].clone())
        }
        Op::Forget { id } => {
            let index = next.position(&id)?;
            let model = next.models.remove(index);
            if next.active.as_deref() == Some(model.id.as_str()) {
                // The next-newest copy, so a machine that still holds working
                // copies always has one open. Derived, never invented: an empty
                // catalogue clears the pointer honestly.
                next.active = next.models.last().map(|model| model.id.clone());
            }
            Some(model)
        }
        Op::Link { id, pls_source } => {
            let index = next.position(&id)?;
            pls_source.validate()?;
            next.models[index].pls_source = Some(pls_source);
            Some(next.models[index].clone())
        }
        Op::Unlink { id } => {
            let index = next.position(&id)?;
            if next.models[index].pls_source.is_none() {
                return refusal(
                    MALFORMED,
                    format!("local model {id} is not linked to a PLS-CADD workspace"),
                );
            }
            next.models[index].pls_source = None;
            Some(next.models[index].clone())
        }
    };

    Ok(Outcome {
        active_changed: next.active != before,
        catalogue: next,
        model,
    })
}

// ── a project's governed heads against this machine's working copies ─────

/// One governed head of a project, as the catalogue read lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GovernedHead {
    pub model_id: String,
    pub display_name: String,
    pub head_revision_id: String,
    /// `sha256:<hex>` or bare hex, as the service wrote it.
    pub head_model_digest: String,
}

/// Whether one governed head is held on this machine, and by which copy.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeadReadiness {
    pub model_id: String,
    pub name: String,
    pub revision: String,
    /// Bare hex.
    pub digest: String,
    pub cached: bool,
    /// The working copy that holds these exact bytes, when one does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
}

/// The fold `ds dsgrid model prepare-project` answers with: which of a
/// project's exact MV heads this machine already holds as a working copy.
///
/// A head is *cached* when a copy pinned to exactly that project, model,
/// revision and digest exists (`Origin::Project`, the pin says where it came
/// from), or when any copy's bytes ARE that digest — a package imported by
/// hand that happens to be the governed head is the governed head. A copy
/// edited since it was pinned keeps the pin but no longer carries the digest,
/// and is therefore not the head: what is asked is whether the exact bytes
/// are here, never whether they once were. Order follows the service's
/// listing; nothing is sorted or deduplicated for the caller.
pub fn project_readiness(
    project_id: &str,
    heads: &[GovernedHead],
    catalogue: &Catalogue,
) -> Vec<HeadReadiness> {
    heads
        .iter()
        .map(|head| {
            let digest = head
                .head_model_digest
                .trim_start_matches("sha256:")
                .to_ascii_lowercase();
            let holder = catalogue.models.iter().find(|model| {
                model.sha256.eq_ignore_ascii_case(&digest)
                    && (model.origin != Origin::Project
                        || model.project.as_ref().is_none_or(|pin| {
                            pin.project_id == project_id
                                && pin.model_id == head.model_id
                                && pin.revision_id == head.head_revision_id
                        }))
            });
            HeadReadiness {
                model_id: head.model_id.clone(),
                name: head.display_name.clone(),
                revision: head.head_revision_id.clone(),
                digest,
                cached: holder.is_some(),
                local_id: holder.map(|model| model.id.clone()),
                bytes: holder.map(|model| model.bytes),
            }
        })
        .collect()
}

/// Is this governed row an MV model the project's Design and printing consume?
/// The browser's rule, carried over: not deleted, and of kind `mv_line` or of
/// no declared kind (rows older than the kind field are MV lines).
pub fn is_mv_head(state: Option<&str>, model_kind: Option<&str>) -> bool {
    state != Some("deleted") && matches!(model_kind, None | Some("mv_line"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope() -> Scope {
        Scope {
            lane: "stable".into(),
            uid: "uid-1".into(),
        }
    }

    fn register(id: &str, name: &str) -> Op {
        Op::Register {
            id: id.into(),
            display_name: name.into(),
            origin: Origin::Created,
            crs: "EPSG:32635".into(),
            model_revision: 0,
            bytes: 2048,
            sha256: "a".repeat(64),
            created_at: Some("2026-09-18T10:00:00Z".into()),
            project: None,
            head_revision: None,
            activate: false,
        }
    }

    fn revise(id: &str, expected: Option<&str>, revision: u64) -> Op {
        Op::Revise {
            id: id.into(),
            expected_sha256: expected.map(str::to_owned),
            model_revision: revision,
            bytes: 4096,
            sha256: "b".repeat(64),
            head_revision: Some("rev:1".into()),
            revised_at: Some("2026-09-20T12:00:00Z".into()),
        }
    }

    #[test]
    fn a_revision_moves_the_package_identity_and_nothing_else_on_the_row() {
        let one = evaluate(&Catalogue::empty(scope()), &scope(), register("m-1", "A")).unwrap();
        let two = evaluate(&one.catalogue, &scope(), register("m-2", "B")).unwrap();
        let before = two.catalogue.models[0].clone();
        let revised = evaluate(
            &two.catalogue,
            &scope(),
            revise("m-1", Some(&"a".repeat(64)), 1),
        )
        .expect("revises");
        let row = revised.model.expect("the revised row");
        assert_eq!(row.id, before.id);
        assert_eq!(row.display_name, before.display_name);
        assert_eq!(row.origin, before.origin);
        assert_eq!(row.crs, before.crs);
        assert_eq!(row.created_at, before.created_at);
        assert_eq!(row.project, before.project);
        assert_eq!(row.model_revision, 1);
        assert_eq!(row.bytes, 4096);
        assert_eq!(row.sha256, "b".repeat(64));
        assert_eq!(row.head_revision.as_deref(), Some("rev:1"));
        assert_eq!(row.revised_at.as_deref(), Some("2026-09-20T12:00:00Z"));
        assert_eq!(
            revised.catalogue.models.len(),
            2,
            "no row was added or lost"
        );
        assert_eq!(revised.catalogue.active.as_deref(), Some("m-1"));
        assert!(!revised.active_changed);
    }

    #[test]
    fn a_revision_built_on_a_package_the_copy_no_longer_holds_is_refused() {
        let one = evaluate(&Catalogue::empty(scope()), &scope(), register("m-1", "A")).unwrap();
        let stale = evaluate(
            &one.catalogue,
            &scope(),
            revise("m-1", Some(&"c".repeat(64)), 1),
        )
        .unwrap_err();
        assert_eq!(stale.code, REVISION_CONFLICT);
        // Without an expectation the caller declared none, and the write is
        // admitted; a backwards package revision never is.
        assert!(evaluate(&one.catalogue, &scope(), revise("m-1", None, 1)).is_ok());
        let forward = evaluate(&one.catalogue, &scope(), revise("m-1", None, 3)).unwrap();
        let backwards = evaluate(&forward.catalogue, &scope(), revise("m-1", None, 2)).unwrap_err();
        assert_eq!(backwards.code, MALFORMED);
        let unknown = evaluate(&one.catalogue, &scope(), revise("m-9", None, 1)).unwrap_err();
        assert_eq!(unknown.code, UNKNOWN_MODEL);
        let mut clockless = revise("m-1", None, 1);
        if let Op::Revise { revised_at, .. } = &mut clockless {
            *revised_at = None;
        }
        assert_eq!(
            evaluate(&one.catalogue, &scope(), clockless)
                .unwrap_err()
                .code,
            MALFORMED,
            "the kernel invented a revision time"
        );
    }

    #[test]
    fn a_machines_first_working_copy_is_the_one_it_opens() {
        let catalogue = Catalogue::empty(scope());
        let first = evaluate(&catalogue, &scope(), register("m-1", "Feeder A")).expect("registers");
        assert_eq!(first.catalogue.active.as_deref(), Some("m-1"));
        assert!(first.active_changed);

        // A second copy does not steal the operator's open model.
        let second =
            evaluate(&first.catalogue, &scope(), register("m-2", "Feeder B")).expect("registers");
        assert_eq!(second.catalogue.active.as_deref(), Some("m-1"));
        assert!(!second.active_changed);

        // Unless it was asked for.
        let mut opened = register("m-3", "Feeder C");
        if let Op::Register { activate, .. } = &mut opened {
            *activate = true;
        }
        let third = evaluate(&second.catalogue, &scope(), opened).expect("registers");
        assert_eq!(third.catalogue.active.as_deref(), Some("m-3"));
        assert!(third.active_changed);
    }

    #[test]
    fn forgetting_the_open_copy_opens_another_and_never_invents_one() {
        let one = evaluate(&Catalogue::empty(scope()), &scope(), register("m-1", "A")).unwrap();
        let two = evaluate(&one.catalogue, &scope(), register("m-2", "B")).unwrap();
        let gone = evaluate(
            &two.catalogue,
            &scope(),
            Op::Forget {
                id: "m-1".to_owned(),
            },
        )
        .expect("forgets");
        assert_eq!(gone.catalogue.active.as_deref(), Some("m-2"));
        assert!(gone.active_changed);

        let empty = evaluate(
            &gone.catalogue,
            &scope(),
            Op::Forget {
                id: "m-2".to_owned(),
            },
        )
        .expect("forgets");
        assert!(empty.catalogue.models.is_empty());
        assert_eq!(
            empty.catalogue.active, None,
            "an empty machine opens nothing"
        );
    }

    #[test]
    fn a_catalogue_from_another_account_is_never_rewritten() {
        let mine = Catalogue::empty(scope());
        let theirs = Scope {
            lane: "stable".into(),
            uid: "uid-2".into(),
        };
        let refused = evaluate(&mine, &theirs, register("m-1", "A")).unwrap_err();
        assert_eq!(refused.code, SCOPE_MISMATCH);
    }

    #[test]
    fn an_id_stays_one_path_segment() {
        for bad in [
            "",
            "../escape",
            "a/b",
            "A-UPPER",
            "with space",
            &"x".repeat(65),
        ] {
            assert!(!id_is_safe(bad), "{bad:?} was accepted as an id");
            let refused = evaluate(&Catalogue::empty(scope()), &scope(), register(bad, "A"));
            assert_eq!(refused.unwrap_err().code, MALFORMED);
        }
        assert!(id_is_safe("model-01"));
    }

    #[test]
    fn two_copies_may_not_share_the_name_an_operator_reads() {
        let one = evaluate(
            &Catalogue::empty(scope()),
            &scope(),
            register("m-1", "Feeder A"),
        )
        .unwrap();
        let refused = evaluate(&one.catalogue, &scope(), register("m-2", "feeder a")).unwrap_err();
        assert_eq!(refused.code, DUPLICATE_NAME);
        // The id is the machine's, the name is the operator's: a different name
        // is accepted for the same underlying model.
        assert!(evaluate(
            &one.catalogue,
            &scope(),
            register("m-2", "Feeder A (rev 2)")
        )
        .is_ok());
    }

    #[test]
    fn a_package_without_an_identity_is_refused_before_it_is_registered() {
        let catalogue = Catalogue::empty(scope());
        let mut op = register("m-1", "A");
        if let Op::Register { bytes, .. } = &mut op {
            *bytes = 0;
        }
        assert_eq!(
            evaluate(&catalogue, &scope(), op).unwrap_err().code,
            MALFORMED
        );

        let mut op = register("m-1", "A");
        if let Op::Register { sha256, .. } = &mut op {
            *sha256 = "short".into();
        }
        assert_eq!(
            evaluate(&catalogue, &scope(), op).unwrap_err().code,
            MALFORMED
        );
    }

    #[test]
    fn a_registration_without_the_hosts_clock_is_refused_rather_than_invented() {
        let mut op = register("m-1", "A");
        if let Op::Register { created_at, .. } = &mut op {
            *created_at = None;
        }
        assert_eq!(
            evaluate(&Catalogue::empty(scope()), &scope(), op)
                .unwrap_err()
                .code,
            MALFORMED,
            "the kernel invented a creation time"
        );
    }

    #[test]
    fn opening_a_copy_this_machine_does_not_hold_is_refused() {
        let catalogue = Catalogue::empty(scope());
        let refused = evaluate(
            &catalogue,
            &scope(),
            Op::SetActive {
                id: "m-9".to_owned(),
            },
        )
        .unwrap_err();
        assert_eq!(refused.code, UNKNOWN_MODEL);
    }

    #[test]
    fn a_project_copy_keeps_the_revision_it_came_from() {
        let mut op = register("m-1", "Governed head");
        if let Op::Register {
            origin, project, ..
        } = &mut op
        {
            *origin = Origin::Project;
            *project = Some(ProjectPin {
                project_id: "wnmajona_edcl2".into(),
                model_id: "mv-line-a".into(),
                revision_id: "rev_7".into(),
                digest: "b".repeat(64),
            });
        }
        let out = evaluate(&Catalogue::empty(scope()), &scope(), op).expect("registers");
        let model = out.model.expect("the registered model");
        assert_eq!(model.origin, Origin::Project);
        assert_eq!(
            model.project.expect("pin").revision_id,
            "rev_7",
            "a working copy records where it came from"
        );
    }

    fn link() -> PlsSourceLink {
        PlsSourceLink {
            path: "/srv/pls/Nyamagabe".into(),
            digest: format!("sha256:{}", "7".repeat(64)),
            pls_version: "16.81".into(),
            member_versions: BTreeMap::from([
                ("DON".to_string(), "57".to_string()),
                ("CRI".to_string(), "94".to_string()),
                ("FEA".to_string(), "15".to_string()),
            ]),
            member_count: 42,
            linked_at: "2026-09-20T10:00:00Z".into(),
        }
    }

    #[test]
    fn a_working_copy_links_to_one_workspace_and_can_relink_or_unlink() {
        let one = evaluate(&Catalogue::empty(scope()), &scope(), register("m-1", "A")).unwrap();
        assert_eq!(
            one.model.as_ref().unwrap().pls_source,
            None,
            "acquisition never links"
        );

        let linked = evaluate(
            &one.catalogue,
            &scope(),
            Op::Link {
                id: "m-1".into(),
                pls_source: link(),
            },
        )
        .expect("links");
        let recorded = linked
            .model
            .as_ref()
            .unwrap()
            .pls_source
            .as_ref()
            .expect("link");
        assert_eq!(recorded.member_versions["DON"], "57");
        assert_eq!(recorded.member_count, 42);
        assert!(
            !linked.active_changed,
            "linking never changes the open model"
        );

        // A second link replaces the first: one working copy, one workspace.
        let mut moved = link();
        moved.digest = format!("sha256:{}", "8".repeat(64));
        let relinked = evaluate(
            &linked.catalogue,
            &scope(),
            Op::Link {
                id: "m-1".into(),
                pls_source: moved.clone(),
            },
        )
        .expect("relinks");
        assert_eq!(
            relinked.catalogue.models[0]
                .pls_source
                .as_ref()
                .unwrap()
                .digest,
            moved.digest
        );

        let unlinked = evaluate(
            &relinked.catalogue,
            &scope(),
            Op::Unlink { id: "m-1".into() },
        )
        .expect("unlinks");
        assert_eq!(unlinked.catalogue.models[0].pls_source, None);
        assert_eq!(
            evaluate(
                &unlinked.catalogue,
                &scope(),
                Op::Unlink { id: "m-1".into() }
            )
            .unwrap_err()
            .code,
            MALFORMED,
            "unlinking an unlinked copy is refused, not ignored"
        );
    }

    #[test]
    fn a_link_names_a_copy_this_machine_holds_and_carries_its_evidence() {
        let one = evaluate(&Catalogue::empty(scope()), &scope(), register("m-1", "A")).unwrap();
        assert_eq!(
            evaluate(
                &one.catalogue,
                &scope(),
                Op::Link {
                    id: "m-9".into(),
                    pls_source: link(),
                }
            )
            .unwrap_err()
            .code,
            UNKNOWN_MODEL
        );
        for broken in [
            PlsSourceLink {
                digest: "7243b0bd".into(),
                ..link()
            },
            PlsSourceLink {
                path: "".into(),
                ..link()
            },
            PlsSourceLink {
                member_count: 0,
                ..link()
            },
            PlsSourceLink {
                linked_at: "".into(),
                ..link()
            },
        ] {
            assert_eq!(
                evaluate(
                    &one.catalogue,
                    &scope(),
                    Op::Link {
                        id: "m-1".into(),
                        pls_source: broken,
                    }
                )
                .unwrap_err()
                .code,
                MALFORMED
            );
        }
    }

    #[test]
    fn a_catalogue_written_before_links_existed_still_reads() {
        // `pls_source` is optional on the wire so an existing catalogue.json
        // opens unchanged, and a row without a link serializes without one.
        let row = serde_json::json!({
            "id": "m-1", "display_name": "A", "origin": "imported",
            "crs": "EPSG:32635", "model_revision": 0, "bytes": 1, "sha256": "a".repeat(64),
            "created_at": "2026-09-18T10:00:00Z"
        });
        let model: LocalModel = serde_json::from_value(row).expect("reads");
        assert_eq!(model.pls_source, None);
        let text = serde_json::to_string(&model).unwrap();
        assert!(!text.contains("pls_source"));
    }

    fn sample(id: &str) -> LocalModel {
        evaluate(&Catalogue::empty(scope()), &scope(), register(id, id))
            .unwrap()
            .catalogue
            .models
            .remove(0)
    }

    #[test]
    fn a_project_head_is_ready_when_its_exact_bytes_are_held_by_any_copy() {
        let mut catalogue = Catalogue::empty(scope());
        let mut pinned = sample("m-pinned");
        pinned.origin = Origin::Project;
        pinned.sha256 = "a".repeat(64);
        pinned.bytes = 10;
        pinned.project = Some(ProjectPin {
            project_id: "p1".into(),
            model_id: "gm-1".into(),
            revision_id: "rev-1".into(),
            digest: "a".repeat(64),
        });
        let mut imported = sample("m-imported");
        imported.origin = Origin::Imported;
        imported.sha256 = "b".repeat(64);
        imported.bytes = 20;
        let mut edited = sample("m-edited");
        edited.origin = Origin::Project;
        edited.sha256 = "d".repeat(64);
        edited.project = Some(ProjectPin {
            project_id: "p1".into(),
            model_id: "gm-3".into(),
            revision_id: "rev-3".into(),
            digest: "c".repeat(64),
        });
        catalogue.models = vec![pinned, imported, edited];
        let heads = [
            GovernedHead {
                model_id: "gm-1".into(),
                display_name: "Pinned".into(),
                head_revision_id: "rev-1".into(),
                head_model_digest: format!("sha256:{}", "A".repeat(64)),
            },
            GovernedHead {
                model_id: "gm-2".into(),
                display_name: "Imported by hand".into(),
                head_revision_id: "rev-2".into(),
                head_model_digest: "b".repeat(64),
            },
            GovernedHead {
                model_id: "gm-3".into(),
                display_name: "Edited since".into(),
                head_revision_id: "rev-3".into(),
                head_model_digest: "c".repeat(64),
            },
            GovernedHead {
                model_id: "gm-1".into(),
                display_name: "Pinned, newer head".into(),
                head_revision_id: "rev-9".into(),
                head_model_digest: "a".repeat(64),
            },
        ];
        let rows = project_readiness("p1", &heads, &catalogue);
        assert_eq!(rows.len(), 4);
        assert!(rows[0].cached && rows[0].local_id.as_deref() == Some("m-pinned"));
        assert_eq!(
            rows[0].digest,
            "a".repeat(64),
            "the digest is bare lowercase hex"
        );
        assert_eq!(rows[0].bytes, Some(10));
        assert!(rows[1].cached && rows[1].local_id.as_deref() == Some("m-imported"));
        assert!(
            !rows[2].cached && rows[2].local_id.is_none(),
            "edited bytes are not the head"
        );
        // Same bytes, but pinned to another revision of the same model: the
        // pin disagrees, so the pinned copy does not answer for it.
        assert!(!rows[3].cached);
        assert!(is_mv_head(None, None) && is_mv_head(Some("active"), Some("mv_line")));
        assert!(!is_mv_head(Some("deleted"), None) && !is_mv_head(None, Some("lv_design")));
    }
}
