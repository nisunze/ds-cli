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

use serde::{Deserialize, Serialize};

/// The longest display name, id or CRS token this catalogue keeps.
pub const MAX_NAME_CHARS: usize = 200;
/// The most models one machine's catalogue may hold. A machine that reaches it
/// is keeping working copies it has stopped using; the refusal names that
/// rather than letting a listing grow without bound.
pub const MAX_MODELS: usize = 500;

pub const SCHEMA: &str = "ds.local-models/v1";

// ── Refusals ────────────────────────────────────────────────────────────

pub const SCOPE_MISMATCH: &str = "local_model_scope_mismatch";
pub const UNKNOWN_MODEL: &str = "local_model_not_found";
pub const DUPLICATE_NAME: &str = "local_model_name_taken";
pub const CATALOGUE_FULL: &str = "local_model_catalogue_full";
pub const MALFORMED: &str = "local_model_request_invalid";

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
        /// Open it as the active model once it is registered. A machine's first
        /// model is active whether or not this is set: a catalogue with exactly
        /// one working copy and nothing active would be a state an operator has
        /// no way to read as deliberate.
        #[serde(default)]
        activate: bool,
    },
    /// Open one existing model as the active one.
    SetActive { id: String },
    /// Forget one model. The host deletes its bytes after this answers.
    Forget { id: String },
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
            };
            if activate || next.models.is_empty() {
                next.active = Some(model.id.clone());
            }
            next.models.push(model.clone());
            Some(model)
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
    };

    Ok(Outcome {
        active_changed: next.active != before,
        catalogue: next,
        model,
    })
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
            activate: false,
        }
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
}
