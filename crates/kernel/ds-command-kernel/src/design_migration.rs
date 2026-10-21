//! Design migration — one domain, one endpoint, one `kind`.
//!
//! A transformer and an MV DS Grid model are the same domain: a design
//! authored in a project, named by an id that is its identity, carrying
//! versions and derived artifacts that must not travel. Migrating one between
//! projects is therefore ONE verb with a `kind`, not two families of command.
//!
//! This module is deliberately narrow. It is NOT a cross-domain migration
//! registry: survey migration is its own endpoint and Solar is its own
//! endpoint with its own kinds. Adding a kind WITHIN this domain is cheap;
//! that is all "reusable" means here.
//!
//! What is decided here rather than at a surface:
//!
//! * the kind and mode vocabularies, so the CLI, the MCP surface and a UI can
//!   never admit a different set;
//! * the bound on one request, so an over-large selection is refused with a
//!   remedy rather than half-applied;
//! * what a target that already holds the object gets, per kind — the two
//!   kinds collide differently and the plan has to say which;
//! * **why nothing moved.** A migration that reports success having moved
//!   nothing, without saying why, is the defect this stack keeps paying for.
//!   `plan_state` can return no state only when something actually moves.
//!
//! Every phrase is a key with its parameters. The words are the host's; the
//! kernel never carries English.

use serde::{Deserialize, Serialize};

pub const SCHEMA: &str = "design_migration/1";

/// One request's bound. It is the design copy path's own Firestore batch: 200
/// writes plus the aggregate-tile fence inside one transaction.
pub const MAX_ITEMS: usize = 200;

/// The longest design object name this will carry. Transformer names are
/// Firestore document ids; DS Grid model ids are bounded at 128.
pub const MAX_ITEM_CHARS: usize = 200;

/// The design objects this endpoint migrates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// An LV transformer document. Its content lives IN Firestore.
    Transformer,
    /// A project DS Grid model — the MV design. Its content is a
    /// content-addressed object that is REUSED, never copied.
    Dsgrid,
}

impl Kind {
    pub const ALL: [Kind; 2] = [Kind::Transformer, Kind::Dsgrid];

    pub const fn wire(self) -> &'static str {
        match self {
            Kind::Transformer => "transformer",
            Kind::Dsgrid => "dsgrid",
        }
    }

    /// Parse the wire value. An ABSENT kind is the transformer kind, because
    /// that is the behaviour that existed before `kind` did; an unknown one is
    /// refused rather than defaulted, so a Solar or survey word can never be
    /// quietly answered by the design endpoint.
    pub fn parse(raw: &str) -> Result<Kind, Refusal> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "" | "transformer" | "transformers" | "tr" => Ok(Kind::Transformer),
            "dsgrid" | "grid" | "grid_model" | "mv" => Ok(Kind::Dsgrid),
            _ => Err(Refusal::UnknownKind),
        }
    }

    /// Whether this kind's objects carry revisions. Where they do, a collision
    /// resolves by adding one rather than by overwriting.
    pub const fn has_revisions(self) -> bool {
        matches!(self, Kind::Dsgrid)
    }
}

/// Plan, then apply. There is no default: the two differ by whether anything
/// is written, so a caller that does not say is refused rather than guessed at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Plan,
    Apply,
}

impl Mode {
    pub const fn wire(self) -> &'static str {
        match self {
            Mode::Plan => "plan",
            Mode::Apply => "apply",
        }
    }

    pub fn parse(raw: &str) -> Result<Mode, Refusal> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "plan" | "dry_run" | "preview" => Ok(Mode::Plan),
            "apply" | "commit" => Ok(Mode::Apply),
            _ => Err(Refusal::UnknownMode),
        }
    }
}

/// What a target that already holds the object gets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollisionPolicy {
    /// Reported and left alone.
    SkipExisting,
    /// The head is replaced and the TARGET's own version counter advances.
    /// Only a kind without revisions resolves this way.
    ReplaceExisting,
    /// The object gains a migrated revision under its expected-head fence. The
    /// previous head stays readable.
    ReviseExisting,
}

impl CollisionPolicy {
    pub const fn wire(self) -> &'static str {
        match self {
            CollisionPolicy::SkipExisting => "skip_existing",
            CollisionPolicy::ReplaceExisting => "replace_existing",
            CollisionPolicy::ReviseExisting => "revise_existing",
        }
    }
}

/// The policy for one kind under one overwrite request.
///
/// Where the object already carries revisions, an overwrite is a REVISION, not
/// a replacement: discarding a head that has lineage is not something an
/// operator asked for by ticking one box.
pub const fn collision_policy(kind: Kind, overwrite_existing: bool) -> CollisionPolicy {
    if !overwrite_existing {
        return CollisionPolicy::SkipExisting;
    }
    if kind.has_revisions() {
        CollisionPolicy::ReviseExisting
    } else {
        CollisionPolicy::ReplaceExisting
    }
}

/// What happened to one design object. Plan and apply share the vocabulary;
/// only the tense differs, so a plan can be compared with the apply that
/// followed it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Plan: it would be created on the target.
    WouldCopy,
    /// Plan: the target's head would be replaced.
    WouldReplace,
    /// Plan: the target model would gain a migrated revision.
    WouldRevise,
    /// Apply: created.
    Copied,
    /// Apply: the head was replaced.
    Replaced,
    /// Apply: a migrated revision landed.
    Revised,
    /// The target already holds this exact content. A second run is a stated
    /// no-op, never a duplicate and never a version bump.
    Identical,
    /// The target holds it and nothing was asked that would change it.
    TargetExists,
    /// The source project does not have it.
    MissingSource,
    /// The target moved while the migration ran.
    Conflict,
    /// The source is not in a state that may be migrated, and the item says why.
    Refused,
    /// Something failed, and the item says what.
    Error,
}

impl Outcome {
    pub const fn wire(self) -> &'static str {
        match self {
            Outcome::WouldCopy => "would_copy",
            Outcome::WouldReplace => "would_replace",
            Outcome::WouldRevise => "would_revise",
            Outcome::Copied => "copied",
            Outcome::Replaced => "replaced",
            Outcome::Revised => "revised",
            Outcome::Identical => "identical",
            Outcome::TargetExists => "target_exists",
            Outcome::MissingSource => "missing_source",
            Outcome::Conflict => "conflict",
            Outcome::Refused => "refused",
            Outcome::Error => "error",
        }
    }

    pub fn parse(raw: &str) -> Option<Outcome> {
        Some(match raw.trim() {
            "would_copy" => Outcome::WouldCopy,
            "would_replace" => Outcome::WouldReplace,
            "would_revise" => Outcome::WouldRevise,
            "copied" => Outcome::Copied,
            "replaced" => Outcome::Replaced,
            "revised" => Outcome::Revised,
            "identical" => Outcome::Identical,
            "target_exists" => Outcome::TargetExists,
            "missing_source" => Outcome::MissingSource,
            "conflict" => Outcome::Conflict,
            "refused" => Outcome::Refused,
            "error" => Outcome::Error,
            _ => return None,
        })
    }

    /// Whether this outcome moved, or would move, something.
    pub const fn moves(self) -> bool {
        matches!(
            self,
            Outcome::WouldCopy
                | Outcome::WouldReplace
                | Outcome::WouldRevise
                | Outcome::Copied
                | Outcome::Replaced
                | Outcome::Revised
        )
    }

    /// Whether this outcome is a failure rather than a stated decision.
    pub const fn is_failure(self) -> bool {
        matches!(self, Outcome::Error | Outcome::Conflict)
    }

    /// The key an operator reads this outcome under.
    pub const fn state_key(self) -> &'static str {
        match self {
            Outcome::WouldCopy => "mig_outcome_would_copy",
            Outcome::WouldReplace => "mig_outcome_would_replace",
            Outcome::WouldRevise => "mig_outcome_would_revise",
            Outcome::Copied => "mig_outcome_copied",
            Outcome::Replaced => "mig_outcome_replaced",
            Outcome::Revised => "mig_outcome_revised",
            Outcome::Identical => "mig_outcome_identical",
            Outcome::TargetExists => "mig_outcome_target_exists",
            Outcome::MissingSource => "mig_outcome_missing_source",
            Outcome::Conflict => "mig_outcome_conflict",
            Outcome::Refused => "mig_outcome_refused",
            Outcome::Error => "mig_outcome_error",
        }
    }
}

/// Why this migration was not attempted, or why a request cannot be admitted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Refusal {
    UnknownKind,
    UnknownMode,
    SameProject,
    NoItems,
    TooManyItems,
    ItemTooLong,
}

impl Refusal {
    pub const fn code(self) -> &'static str {
        match self {
            Refusal::UnknownKind => "unknown_kind",
            Refusal::UnknownMode => "unknown_mode",
            Refusal::SameProject => "same_project",
            Refusal::NoItems => "no_items",
            Refusal::TooManyItems => "too_many_items",
            Refusal::ItemTooLong => "item_too_long",
        }
    }
}

/// One design object's admitted name.
pub fn normalize_items(raw: &[String]) -> Result<Vec<String>, Refusal> {
    let mut seen: Vec<String> = Vec::new();
    for value in raw {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.chars().count() > MAX_ITEM_CHARS {
            return Err(Refusal::ItemTooLong);
        }
        if !seen.iter().any(|held| held == trimmed) {
            seen.push(trimmed.to_owned());
        }
    }
    if seen.is_empty() {
        return Err(Refusal::NoItems);
    }
    if seen.len() > MAX_ITEMS {
        return Err(Refusal::TooManyItems);
    }
    Ok(seen)
}

/// One counted outcome, as a plan or a receipt reports it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutcomeCount {
    pub outcome: Outcome,
    pub count: u64,
}

/// What the whole request did, folded from its items.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Totals {
    pub requested: u64,
    /// Would move (a plan) or did move (an apply).
    pub moving: u64,
    pub identical: u64,
    pub blocked: u64,
    pub failed: u64,
    pub bytes: u64,
}

/// Fold a run's item outcomes into its totals.
pub fn totals(outcomes: &[(Outcome, u64)]) -> Totals {
    let mut folded = Totals {
        requested: outcomes.len() as u64,
        ..Totals::default()
    };
    for (outcome, bytes) in outcomes {
        if outcome.moves() {
            folded.moving += 1;
            folded.bytes += bytes;
        } else if *outcome == Outcome::Identical {
            folded.identical += 1;
        } else if outcome.is_failure() {
            folded.failed += 1;
        } else {
            folded.blocked += 1;
        }
    }
    folded
}

/// The state an operator reads a whole run under.
///
/// `None` is returned ONLY when something moved or would move. Everything else
/// carries a key that says why nothing did — including the case nobody
/// remembers to handle, an empty selection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmptyState {
    pub key: &'static str,
    pub breakdown: Vec<OutcomeCount>,
}

pub fn empty_state(totals: &Totals, outcomes: &[(Outcome, u64)]) -> Option<EmptyState> {
    if totals.moving > 0 {
        return None;
    }
    let mut breakdown: Vec<OutcomeCount> = Vec::new();
    for (outcome, _) in outcomes {
        match breakdown.iter_mut().find(|held| held.outcome == *outcome) {
            Some(held) => held.count += 1,
            None => breakdown.push(OutcomeCount {
                outcome: *outcome,
                count: 1,
            }),
        }
    }
    breakdown.sort_by_key(|row| row.outcome.wire());
    let key = if totals.requested == 0 {
        "mig_empty_nothing_requested"
    } else if totals.failed > 0 {
        "mig_empty_all_failed"
    } else if totals.identical == totals.requested {
        "mig_empty_all_identical"
    } else {
        "mig_empty_all_blocked"
    };
    Some(EmptyState { key, breakdown })
}

/// Read a migration receipt's items into the kernel's vocabulary.
///
/// An item whose status the kernel does not know becomes `Error` rather than
/// being dropped: an unread row is still a row the operator asked about.
pub fn read_items(items: &serde_json::Value) -> Vec<(Outcome, u64)> {
    items
        .as_array()
        .map(|rows| {
            rows.iter()
                .map(|row| {
                    let outcome = row["status"]
                        .as_str()
                        .and_then(Outcome::parse)
                        .unwrap_or(Outcome::Error);
                    let bytes = row["bytes"].as_u64().unwrap_or(0);
                    (outcome, bytes)
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_absent_kind_is_the_behaviour_that_existed_before_kind_did() {
        assert_eq!(Kind::parse("").unwrap(), Kind::Transformer);
        assert_eq!(Kind::parse("  TR ").unwrap(), Kind::Transformer);
    }

    #[test]
    fn mv_models_and_transformers_are_the_same_domain_under_two_kinds() {
        assert_eq!(Kind::parse("mv").unwrap(), Kind::Dsgrid);
        assert_eq!(Kind::parse("dsgrid").unwrap(), Kind::Dsgrid);
        assert_eq!(Kind::ALL.len(), 2);
    }

    #[test]
    fn a_kind_from_another_domain_is_refused_not_defaulted() {
        for word in ["city", "portfolio", "survey", "entries"] {
            assert_eq!(Kind::parse(word), Err(Refusal::UnknownKind), "{word}");
        }
    }

    #[test]
    fn an_unstated_mode_is_refused_because_the_two_differ_by_whether_anything_is_written() {
        assert_eq!(Mode::parse(""), Err(Refusal::UnknownMode));
        assert_eq!(Mode::parse("plan").unwrap(), Mode::Plan);
        assert_eq!(Mode::parse("apply").unwrap(), Mode::Apply);
    }

    #[test]
    fn an_object_that_carries_revisions_is_revised_never_replaced() {
        assert_eq!(
            collision_policy(Kind::Dsgrid, true),
            CollisionPolicy::ReviseExisting
        );
        assert_eq!(
            collision_policy(Kind::Transformer, true),
            CollisionPolicy::ReplaceExisting
        );
        for kind in Kind::ALL {
            assert_eq!(
                collision_policy(kind, false),
                CollisionPolicy::SkipExisting,
                "{}",
                kind.wire()
            );
        }
    }

    #[test]
    fn the_item_bound_is_one_transaction_and_is_refused_with_its_number() {
        let items: Vec<String> = (0..MAX_ITEMS).map(|i| format!("tx_{i}")).collect();
        assert_eq!(normalize_items(&items).unwrap().len(), MAX_ITEMS);
        let over: Vec<String> = (0..MAX_ITEMS + 1).map(|i| format!("tx_{i}")).collect();
        assert_eq!(normalize_items(&over), Err(Refusal::TooManyItems));
    }

    #[test]
    fn repeats_collapse_so_running_twice_never_duplicates() {
        let items = vec!["TX-1".into(), " TX-1 ".into(), "TX-2".into(), "  ".into()];
        assert_eq!(normalize_items(&items).unwrap(), vec!["TX-1", "TX-2"]);
    }

    #[test]
    fn an_empty_selection_is_refused_rather_than_answered_as_success() {
        assert_eq!(normalize_items(&[]), Err(Refusal::NoItems));
        assert_eq!(normalize_items(&["   ".into()]), Err(Refusal::NoItems));
    }

    #[test]
    fn nothing_moved_always_carries_a_state_that_says_why() {
        let cases: Vec<(Vec<(Outcome, u64)>, &str)> = vec![
            (vec![], "mig_empty_nothing_requested"),
            (
                vec![(Outcome::Identical, 0), (Outcome::Identical, 0)],
                "mig_empty_all_identical",
            ),
            (
                vec![(Outcome::TargetExists, 0), (Outcome::MissingSource, 0)],
                "mig_empty_all_blocked",
            ),
            (
                vec![(Outcome::Error, 0), (Outcome::TargetExists, 0)],
                "mig_empty_all_failed",
            ),
        ];
        for (outcomes, expected) in cases {
            let folded = totals(&outcomes);
            let state = empty_state(&folded, &outcomes)
                .expect("a run that moved nothing must always state why");
            assert_eq!(state.key, expected);
        }
    }

    #[test]
    fn a_run_that_moved_something_states_no_empty_reason() {
        let outcomes = vec![(Outcome::WouldCopy, 10), (Outcome::TargetExists, 0)];
        let folded = totals(&outcomes);
        assert_eq!(folded.moving, 1);
        assert_eq!(folded.blocked, 1);
        assert_eq!(folded.bytes, 10);
        assert!(empty_state(&folded, &outcomes).is_none());
    }

    #[test]
    fn an_identical_item_is_a_stated_skip_not_a_failure() {
        let outcomes = vec![(Outcome::Identical, 999)];
        let folded = totals(&outcomes);
        assert_eq!(folded.identical, 1);
        assert_eq!(folded.failed, 0);
        assert_eq!(folded.blocked, 0);
        // Identical content moves no bytes, so it contributes none.
        assert_eq!(folded.bytes, 0);
    }

    #[test]
    fn an_unreadable_item_status_becomes_an_error_never_a_dropped_row() {
        let items = serde_json::json!([
            {"status": "copied", "bytes": 4},
            {"status": "who_knows"},
            {"bytes": 9}
        ]);
        let read = read_items(&items);
        assert_eq!(read.len(), 3);
        assert_eq!(read[0], (Outcome::Copied, 4));
        assert_eq!(read[1].0, Outcome::Error);
        assert_eq!(read[2].0, Outcome::Error);
    }

    #[test]
    fn every_outcome_round_trips_its_wire_value_and_has_a_state_key() {
        let all = [
            Outcome::WouldCopy,
            Outcome::WouldReplace,
            Outcome::WouldRevise,
            Outcome::Copied,
            Outcome::Replaced,
            Outcome::Revised,
            Outcome::Identical,
            Outcome::TargetExists,
            Outcome::MissingSource,
            Outcome::Conflict,
            Outcome::Refused,
            Outcome::Error,
        ];
        for outcome in all {
            assert_eq!(Outcome::parse(outcome.wire()), Some(outcome));
            assert!(outcome.state_key().starts_with("mig_outcome_"));
        }
    }
}
