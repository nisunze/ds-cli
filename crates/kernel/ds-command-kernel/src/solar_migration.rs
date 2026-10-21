//! Solar project-to-project migration — the kind vocabulary, the bounds, the
//! applicability verdict and the digest fence.
//!
//! ONE endpoint in the Solar domain, with a `kind` INSIDE it: `city` or
//! `portfolio`. This is deliberately not a cross-domain migration registry —
//! design owns its own migration, survey already has one, and an abstraction
//! over all three would describe none of them. What is shared between the two
//! Solar kinds is real: one plan shape, one digest, one plan-before-apply
//! discipline, and one constraint.
//!
//! THE CONSTRAINT: **computation results do not migrate.** A migrated city
//! arrives with everything needed to be computed and nothing that was computed.
//! Three properties here exist to keep that checkable rather than asserted:
//!
//! * **A plan that claims it mutated is a contract break**, not a state a
//!   caller should be handed — so it drifts rather than applying.
//! * **A receipt that claims results migrated is the same break**, one layer
//!   later, and is refused by `results_are_honest`.
//! * **Evidence is echoed, never derived.** A digest is confirmed against its
//!   shape and against the plan that carried it; nothing here computes one.
use serde::{Deserialize, Serialize};

pub const SCHEMA: &str = "solar_migration/1";

/// The two kinds this endpoint accepts, and the only two.
pub const KIND_CITY: &str = "city";
pub const KIND_PORTFOLIO: &str = "portfolio";
pub const KINDS: &[&str] = &[KIND_CITY, KIND_PORTFOLIO];

/// The grant's own per-request selection bound (`solarMigrateMaxSelection`).
pub const MAX_SELECTION: usize = 64;
/// Longest exact source object id or portfolio name a request may carry.
pub const MAX_ID_CHARS: usize = 128;
/// Longest project id a request may carry.
pub const MAX_PROJECT_CHARS: usize = 128;

/// `migrate_digest` is `hex.EncodeToString` over a SHA-256 sum: 64 lowercase
/// hex characters with no `sha256:` prefix, like Solar's seed digest and unlike
/// its membership revisions.
pub fn is_migrate_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// A refused request. `code` is the stable condition; where the grant already
/// names one, that name is kept so a single vocabulary describes it.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Refusal {
    pub code: &'static str,
    pub field: &'static str,
}

impl Refusal {
    const fn new(code: &'static str, field: &'static str) -> Self {
        Self { code, field }
    }
}

/// The context a migration door is bound to. Both doors send exactly this.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    /// `city` or `portfolio`. There is no default: guessing which of two
    /// different objects an operator meant is the assumption this contract
    /// exists to remove.
    #[serde(default)]
    pub kind: String,
    /// Destination Solar root, resolved from the caller's own session.
    #[serde(default)]
    pub root: String,
    /// Destination project.
    #[serde(default)]
    pub ds_project: String,
    /// SOURCE project id — never a root. The grant resolves the root from it,
    /// which is what makes a non-project root unreachable as a source.
    #[serde(default)]
    pub source_project: String,
    /// Empty means every eligible source object of this kind.
    #[serde(default)]
    pub selection: Vec<String>,
}

/// The facts a plan carries that the decisions below read.
///
/// Deliberately open to unknown members: this is the grant's own document,
/// already bounded by the parser that produced it, and a member named later
/// still describes a plan these decisions answer the same way.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Plan {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub root: String,
    #[serde(default)]
    pub ds_project: String,
    #[serde(default)]
    pub source_project: String,
    /// False on every plan. Read, never inferred from the action name.
    #[serde(default)]
    pub mutated: bool,
    #[serde(default, deserialize_with = "crate::host_counts::count")]
    pub create_count: u64,
    #[serde(default, deserialize_with = "crate::host_counts::count")]
    pub replace_count: u64,
    #[serde(default, deserialize_with = "crate::host_counts::count")]
    pub skip_count: u64,
    #[serde(default, deserialize_with = "crate::host_counts::count")]
    pub changed_count: u64,
    #[serde(default, deserialize_with = "crate::host_counts::count")]
    pub missing_count: u64,
    #[serde(default, deserialize_with = "crate::host_counts::count")]
    pub blocked_count: u64,
    #[serde(default, deserialize_with = "crate::host_counts::count")]
    pub document_count: u64,
    #[serde(default, deserialize_with = "crate::host_counts::count")]
    pub excluded_document_count: u64,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub cities: Vec<PlanRow>,
    #[serde(default)]
    pub portfolios: Vec<PlanRow>,
}

/// One planned row, of either kind, as much of one as these decisions read.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct PlanRow {
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Drift {
    /// The plan describes this context.
    None,
    /// It describes another destination or another project.
    Project,
    /// It describes another source project.
    Source,
    /// It describes the other kind.
    Kind,
    /// A plan that claims it wrote.
    Mutated,
}

/// Refuse a plan that no longer describes this call's own context.
///
/// This is not a duplicate of the grant's digest check — that one catches
/// either end CHANGING between plan and apply. This one catches the plan coming
/// back describing another project, another source, or the other kind.
///
/// Kind is checked because the two kinds share a plan shape: a portfolio plan
/// returned to a city call would render as "0 cities" rather than as an error,
/// which is precisely the confident empty answer this endpoint is written
/// against.
pub fn drift(plan: &Plan, context: &Context) -> Drift {
    if plan.root != context.root
        || (!context.ds_project.is_empty() && plan.ds_project != context.ds_project)
    {
        return Drift::Project;
    }
    if !context.source_project.is_empty() && plan.source_project != context.source_project {
        return Drift::Source;
    }
    if !context.kind.is_empty() && plan.kind != context.kind {
        return Drift::Kind;
    }
    if plan.mutated {
        return Drift::Mutated;
    }
    Drift::None
}

/// May this plan be applied as it stands? A plan that writes nothing is not
/// applyable — there is no write to authorize, and confirming one would report
/// a success that moved nothing.
pub fn applyable(plan: &Plan, context: &Context) -> bool {
    (plan.create_count > 0 || plan.replace_count > 0) && matches!(drift(plan, context), Drift::None)
}

/// Is this receipt honest about the endpoint's defining constraint?
///
/// `computation_results_migrated` is on the wire precisely so no reader has to
/// know the contract to know the numbers stayed behind. A receipt asserting
/// otherwise did not come from this contract, and is refused rather than
/// rendered.
pub fn results_are_honest(computation_results_migrated: bool) -> bool {
    !computation_results_migrated
}

/// Validate the request envelope before any credential is touched.
///
/// `require_destination` is the headless door's extra obligation: a browser
/// card is already bound to the open project, while a headless caller must be
/// proven to be aiming at the project it named.
pub fn validate_context(context: &Context, require_destination: bool) -> Result<(), Refusal> {
    if !KINDS.contains(&context.kind.as_str()) {
        return Err(Refusal::new("SOLAR_MIGRATE_KIND_INVALID", "kind"));
    }
    // Counted in CHARACTERS, as the names say: an id spelled with accented
    // letters must not be refused at half the length an ASCII one is allowed.
    if context.source_project.is_empty()
        || context.source_project.trim() != context.source_project
        || context.source_project.chars().count() > MAX_PROJECT_CHARS
    {
        return Err(Refusal::new(
            "SOLAR_MIGRATE_SOURCE_INVALID",
            "source_project",
        ));
    }
    if !context.ds_project.is_empty() && context.source_project == context.ds_project {
        return Err(Refusal::new(
            "SOLAR_MIGRATE_SOURCE_INVALID",
            "source_project",
        ));
    }
    if context.selection.len() > MAX_SELECTION {
        return Err(Refusal::new("SOLAR_MIGRATE_BOUNDED", "selection"));
    }
    for item in &context.selection {
        if item.is_empty() || item.trim() != item || item.chars().count() > MAX_ID_CHARS {
            return Err(Refusal::new("SOLAR_MIGRATE_SELECTION_INVALID", "selection"));
        }
    }
    let unique: std::collections::BTreeSet<&String> = context.selection.iter().collect();
    if unique.len() != context.selection.len() {
        // Deduplicating in silence would make the plan describe fewer rows than
        // the caller named, which is the one thing a plan must not do.
        return Err(Refusal::new("SOLAR_MIGRATE_SELECTION_INVALID", "selection"));
    }
    if require_destination {
        if context.ds_project.is_empty() {
            return Err(Refusal::new("SOLAR_MIGRATE_PROJECT_REQUIRED", "ds_project"));
        }
        if !context
            .root
            .split('/')
            .any(|segment| segment == context.ds_project)
        {
            return Err(Refusal::new("SOLAR_MIGRATE_DESTINATION_MISMATCH", "root"));
        }
    }
    Ok(())
}

/// Confirm a digest echoed from a plan the caller read.
pub fn confirm_digest(value: &str) -> Result<(), Refusal> {
    if is_migrate_digest(value) {
        Ok(())
    } else {
        Err(Refusal::new(
            "SOLAR_MIGRATE_DIGEST_REQUIRED",
            "migrate_digest",
        ))
    }
}

/// What a plan amounts to, for a card's summary line and the CLI's receipt.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Summary {
    pub kind: String,
    pub create: u64,
    pub replace: u64,
    pub skip: u64,
    pub changed: u64,
    pub missing: u64,
    /// Rows that CANNOT be applied and say why. A summary with blocked rows
    /// and nothing creatable is a refusal, not an empty success.
    pub blocked: u64,
    pub documents: u64,
    pub excluded_documents: u64,
    /// Plan warnings first, then each row's, de-duplicated in first-seen order.
    pub warnings: Vec<String>,
    /// True when the plan has no rows at all — a fact a renderer must SAY
    /// rather than render as a clean success.
    pub empty: bool,
}

pub fn summarize(plan: Option<&Plan>) -> Summary {
    let Some(plan) = plan else {
        return Summary {
            kind: String::new(),
            create: 0,
            replace: 0,
            skip: 0,
            changed: 0,
            missing: 0,
            blocked: 0,
            documents: 0,
            excluded_documents: 0,
            warnings: Vec::new(),
            empty: true,
        };
    };
    let mut seen = std::collections::BTreeSet::new();
    let mut warnings = Vec::new();
    for warning in plan.warnings.iter().chain(
        plan.cities
            .iter()
            .chain(plan.portfolios.iter())
            .flat_map(|row| row.warnings.iter()),
    ) {
        if seen.insert(warning.clone()) {
            warnings.push(warning.clone());
        }
    }
    Summary {
        kind: plan.kind.clone(),
        create: plan.create_count,
        replace: plan.replace_count,
        skip: plan.skip_count,
        changed: plan.changed_count,
        missing: plan.missing_count,
        blocked: plan.blocked_count,
        documents: plan.document_count,
        excluded_documents: plan.excluded_document_count,
        warnings,
        empty: plan.cities.is_empty() && plan.portfolios.is_empty(),
    }
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    /// Is this request envelope admissible at all?
    Validate {
        context: Context,
        #[serde(default)]
        require_destination: bool,
        #[serde(default)]
        migrate_digest: Option<String>,
    },
    /// Does this plan still describe this context, and may it be applied?
    Verdict { plan: Plan, context: Context },
    /// What does this plan amount to?
    Summarize {
        #[serde(default)]
        plan: Option<Plan>,
    },
    /// Is this receipt honest about what did not migrate?
    Receipt {
        #[serde(default)]
        computation_results_migrated: bool,
    },
}

#[derive(Serialize)]
struct Reply {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    refusal: Option<Refusal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    drift: Option<Drift>,
    #[serde(skip_serializing_if = "Option::is_none")]
    applyable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<Summary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bounds: Option<Bounds>,
}

#[derive(Serialize)]
struct Bounds {
    kinds: &'static [&'static str],
    max_selection: usize,
    max_id_chars: usize,
    max_project_chars: usize,
}

const BOUNDS: Bounds = Bounds {
    kinds: KINDS,
    max_selection: MAX_SELECTION,
    max_id_chars: MAX_ID_CHARS,
    max_project_chars: MAX_PROJECT_CHARS,
};

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > 8 * 1024 * 1024 {
        return Err("solar_migration_input_too_large".into());
    }
    let request: Request = serde_json::from_slice(input)
        .map_err(|error| format!("solar_migration_request_invalid: {error}"))?;
    let reply = match request {
        Request::Validate {
            context,
            require_destination,
            migrate_digest,
        } => {
            let refusal = validate_context(&context, require_destination)
                .err()
                .or_else(|| {
                    migrate_digest
                        .as_deref()
                        .and_then(|digest| confirm_digest(digest).err())
                });
            Reply {
                ok: refusal.is_none(),
                refusal,
                drift: None,
                applyable: None,
                summary: None,
                bounds: Some(BOUNDS),
            }
        }
        Request::Verdict { plan, context } => Reply {
            ok: true,
            refusal: None,
            drift: Some(drift(&plan, &context)),
            applyable: Some(applyable(&plan, &context)),
            summary: Some(summarize(Some(&plan))),
            bounds: None,
        },
        Request::Summarize { plan } => Reply {
            ok: true,
            refusal: None,
            drift: None,
            applyable: None,
            summary: Some(summarize(plan.as_ref())),
            bounds: None,
        },
        Request::Receipt {
            computation_results_migrated,
        } => Reply {
            ok: results_are_honest(computation_results_migrated),
            refusal: (!results_are_honest(computation_results_migrated)).then(|| {
                Refusal::new(
                    "SOLAR_MIGRATE_RESULTS_CLAIMED",
                    "computation_results_migrated",
                )
            }),
            drift: None,
            applyable: None,
            summary: None,
            bounds: None,
        },
    };
    serde_json::to_string(&reply).map_err(|_| "solar_migration_reply_invalid".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> Context {
        Context {
            kind: KIND_CITY.into(),
            root: "eds_project/chad-test/eds_solar".into(),
            ds_project: "chad-test".into(),
            source_project: "aderm".into(),
            selection: Vec::new(),
        }
    }

    fn plan() -> Plan {
        Plan {
            kind: KIND_CITY.into(),
            root: "eds_project/chad-test/eds_solar".into(),
            ds_project: "chad-test".into(),
            source_project: "aderm".into(),
            create_count: 2,
            ..Plan::default()
        }
    }

    #[test]
    fn a_request_without_a_kind_is_refused() {
        let mut context = context();
        context.kind = String::new();
        assert_eq!(
            validate_context(&context, true).unwrap_err().code,
            "SOLAR_MIGRATE_KIND_INVALID"
        );
    }

    #[test]
    fn a_design_kind_is_not_a_solar_kind() {
        let mut context = context();
        context.kind = "transformer".into();
        assert_eq!(
            validate_context(&context, true).unwrap_err().code,
            "SOLAR_MIGRATE_KIND_INVALID"
        );
    }

    #[test]
    fn a_project_cannot_migrate_into_itself() {
        let mut context = context();
        context.source_project = context.ds_project.clone();
        assert_eq!(
            validate_context(&context, true).unwrap_err().field,
            "source_project"
        );
    }

    #[test]
    fn a_duplicated_selection_entry_is_refused_not_deduplicated() {
        let mut context = context();
        context.selection = vec!["fianga".into(), "fianga".into()];
        assert_eq!(
            validate_context(&context, true).unwrap_err().code,
            "SOLAR_MIGRATE_SELECTION_INVALID"
        );
    }

    #[test]
    fn a_selection_beyond_the_bound_is_refused() {
        let mut context = context();
        context.selection = (0..MAX_SELECTION + 1)
            .map(|i| format!("city-{i}"))
            .collect();
        assert_eq!(
            validate_context(&context, true).unwrap_err().code,
            "SOLAR_MIGRATE_BOUNDED"
        );
    }

    #[test]
    fn a_destination_the_root_does_not_name_is_refused() {
        let mut context = context();
        context.ds_project = "someone-else".into();
        assert_eq!(
            validate_context(&context, true).unwrap_err().code,
            "SOLAR_MIGRATE_DESTINATION_MISMATCH"
        );
    }

    #[test]
    fn a_plan_of_the_other_kind_drifts_rather_than_rendering_as_empty() {
        let mut returned = plan();
        returned.kind = KIND_PORTFOLIO.into();
        assert_eq!(drift(&returned, &context()), Drift::Kind);
        assert!(!applyable(&returned, &context()));
    }

    #[test]
    fn a_plan_claiming_it_mutated_is_a_contract_break() {
        let mut returned = plan();
        returned.mutated = true;
        assert_eq!(drift(&returned, &context()), Drift::Mutated);
    }

    #[test]
    fn a_plan_that_writes_nothing_is_not_applyable() {
        let mut returned = plan();
        returned.create_count = 0;
        returned.blocked_count = 3;
        assert!(!applyable(&returned, &context()));
    }

    #[test]
    fn a_replace_only_plan_is_applyable() {
        let mut returned = plan();
        returned.create_count = 0;
        returned.replace_count = 1;
        assert!(applyable(&returned, &context()));
    }

    #[test]
    fn a_receipt_claiming_results_migrated_is_refused() {
        assert!(results_are_honest(false));
        assert!(!results_are_honest(true));
    }

    #[test]
    fn a_digest_is_confirmed_by_shape_never_computed() {
        assert!(confirm_digest(&"a".repeat(64)).is_ok());
        assert!(confirm_digest(&"A".repeat(64)).is_err());
        assert!(confirm_digest("sha256:").is_err());
    }

    #[test]
    fn an_empty_plan_is_summarized_as_empty() {
        let summary = summarize(Some(&Plan::default()));
        assert!(summary.empty);
        assert_eq!(summary.create, 0);
    }
}
