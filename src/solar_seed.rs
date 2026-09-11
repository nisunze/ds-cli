//! Governed Solar city seeding — the bounds, the applicability verdict and the
//! digest fence.
//!
//! Seeding copies governed Solar cities into a project. The grant enforces the
//! same bounds again and owns the write; what lives here is the part that was
//! held three times — once in the browser card's executor, once in the CLI, and
//! once in Go — and could therefore answer three ways for one request.
//!
//! Three properties are load-bearing:
//!
//! * **The selected project IS the destination.** A project id is proof of
//!   nothing; the destination root is resolved from the caller's own session
//!   and must name that project as a path segment. Resolving a root for an
//!   unknown project yields the governed CATALOG, which is a source and never a
//!   destination — so this is a refusal, not a warning.
//! * **Evidence is echoed, never derived.** A digest is confirmed against its
//!   shape and against the plan that carried it; nothing here computes one.
//! * **A preview that claims it mutated is a contract break**, not a state any
//!   caller should be handed — so it drifts rather than applying.
use serde::{Deserialize, Serialize};

pub const SCHEMA: &str = "solar_seed/1";

/// The grant's own per-request city bound (`solarSeedMaxCities`).
pub const MAX_CITIES: usize = 64;
/// Longest exact source city id a request may carry.
pub const MAX_CITY_CHARS: usize = 128;
/// Longest governed seed source root a request may carry.
pub const MAX_SOURCE_CHARS: usize = 512;

/// `seed_digest` is `hex.EncodeToString` over a SHA-256 sum: 64 lowercase hex
/// characters with no `sha256:` prefix, unlike Solar's other digests.
pub fn is_seed_digest(value: &str) -> bool {
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

/// The context a seeding door is bound to. Both doors send exactly this.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    /// Destination, resolved from the caller's own session.
    #[serde(default)]
    pub root: String,
    /// Governed seed source. Empty means the grant's global catalog.
    #[serde(default)]
    pub seed_source_root: String,
    #[serde(default)]
    pub ds_project: String,
    /// Empty means every live city in the seed source.
    #[serde(default)]
    pub cities: Vec<String>,
}

/// The facts a plan carries that any of these three decisions reads.
///
/// Deliberately open to unknown members: this is the grant's own document,
/// already bounded by the parser that produced it, and a member named later
/// still describes a plan these decisions answer the same way.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Plan {
    #[serde(default)]
    pub root: String,
    #[serde(default)]
    pub ds_project: String,
    #[serde(default)]
    pub seed_source_root: String,
    /// False on every preview. Read, never inferred from the action name.
    #[serde(default)]
    pub mutated: bool,
    #[serde(default)]
    pub create_count: u64,
    #[serde(default)]
    pub skip_count: u64,
    #[serde(default)]
    pub changed_count: u64,
    #[serde(default)]
    pub missing_count: u64,
    #[serde(default)]
    pub document_count: u64,
    #[serde(default)]
    pub asset_count: u64,
    #[serde(default)]
    pub excluded_asset_count: u64,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub cities: Vec<PlanCity>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct PlanCity {
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
    /// It describes another governed source.
    Source,
    /// A preview that claims it wrote.
    Mutated,
}

/// Refuse a plan that no longer describes this call's own context.
///
/// This is not a duplicate of the grant's check — that one catches the source
/// or destination CHANGING between preview and apply. This one catches the
/// plan coming back describing another project or source.
pub fn drift(plan: &Plan, context: &Context) -> Drift {
    if plan.root != context.root
        || (!context.ds_project.is_empty() && plan.ds_project != context.ds_project)
    {
        return Drift::Project;
    }
    if !context.seed_source_root.is_empty() && plan.seed_source_root != context.seed_source_root {
        return Drift::Source;
    }
    if plan.mutated {
        return Drift::Mutated;
    }
    Drift::None
}

/// May this plan be applied as it stands? A plan that creates nothing is not
/// applyable — there is no write to authorize.
pub fn applyable(plan: &Plan, context: &Context) -> bool {
    plan.create_count > 0 && matches!(drift(plan, context), Drift::None)
}

/// Validate the request envelope before any credential is touched.
///
/// `require_destination` is the headless door's extra obligation: a browser
/// card is already bound to the open project, while a headless caller must be
/// proven to be aiming at the project it selected rather than at the catalog.
pub fn validate_context(context: &Context, require_destination: bool) -> Result<(), Refusal> {
    if !context.seed_source_root.is_empty()
        && (context.seed_source_root.trim() != context.seed_source_root
            || context.seed_source_root.len() > MAX_SOURCE_CHARS)
    {
        return Err(Refusal::new(
            "solar_seed_source_invalid",
            "seed_source_root",
        ));
    }
    if context.cities.len() > MAX_CITIES {
        return Err(Refusal::new("SOLAR_SEED_BOUNDED", "cities"));
    }
    for city in &context.cities {
        if city.is_empty() || city.trim() != city || city.len() > MAX_CITY_CHARS {
            return Err(Refusal::new("solar_seed_city_invalid", "cities"));
        }
    }
    let unique: std::collections::BTreeSet<&String> = context.cities.iter().collect();
    if unique.len() != context.cities.len() {
        return Err(Refusal::new("solar_seed_cities_duplicated", "cities"));
    }
    if require_destination {
        if context.ds_project.is_empty() {
            return Err(Refusal::new("solar_seed_project_required", "ds_project"));
        }
        if !context
            .root
            .split('/')
            .any(|segment| segment == context.ds_project)
        {
            return Err(Refusal::new("solar_seed_destination_mismatch", "root"));
        }
    }
    Ok(())
}

/// Confirm a digest echoed from a plan the caller previewed.
pub fn confirm_digest(value: &str) -> Result<(), Refusal> {
    if is_seed_digest(value) {
        Ok(())
    } else {
        Err(Refusal::new("SOLAR_SEED_DIGEST_REQUIRED", "seed_digest"))
    }
}

/// What a plan amounts to, for the card's summary line and the CLI's receipt.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Summary {
    pub create: u64,
    pub skip: u64,
    pub changed: u64,
    pub missing: u64,
    pub documents: u64,
    pub assets: u64,
    pub excluded_assets: u64,
    /// Plan warnings first, then each city's, de-duplicated in first-seen order.
    pub warnings: Vec<String>,
    pub empty: bool,
}

pub fn summarize(plan: Option<&Plan>) -> Summary {
    let Some(plan) = plan else {
        return Summary {
            create: 0,
            skip: 0,
            changed: 0,
            missing: 0,
            documents: 0,
            assets: 0,
            excluded_assets: 0,
            warnings: Vec::new(),
            empty: true,
        };
    };
    let mut seen = std::collections::BTreeSet::new();
    let mut warnings = Vec::new();
    for warning in plan
        .warnings
        .iter()
        .chain(plan.cities.iter().flat_map(|city| city.warnings.iter()))
    {
        if seen.insert(warning.clone()) {
            warnings.push(warning.clone());
        }
    }
    Summary {
        create: plan.create_count,
        skip: plan.skip_count,
        changed: plan.changed_count,
        missing: plan.missing_count,
        documents: plan.document_count,
        assets: plan.asset_count,
        excluded_assets: plan.excluded_asset_count,
        warnings,
        empty: plan.cities.is_empty(),
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
        seed_digest: Option<String>,
    },
    /// Does this plan still describe this context, and may it be applied?
    Verdict { plan: Plan, context: Context },
    /// What does this plan amount to?
    Summarize {
        #[serde(default)]
        plan: Option<Plan>,
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
    max_cities: usize,
    max_city_chars: usize,
    max_source_chars: usize,
}

const BOUNDS: Bounds = Bounds {
    max_cities: MAX_CITIES,
    max_city_chars: MAX_CITY_CHARS,
    max_source_chars: MAX_SOURCE_CHARS,
};

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > 8 * 1024 * 1024 {
        return Err("solar_seed_input_too_large".into());
    }
    let request: Request = serde_json::from_slice(input)
        .map_err(|error| format!("solar_seed_request_invalid: {error}"))?;
    let reply = match request {
        Request::Validate {
            context,
            require_destination,
            seed_digest,
        } => {
            let refusal = validate_context(&context, require_destination)
                .err()
                .or_else(|| {
                    seed_digest
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
    };
    serde_json::to_string(&reply).map_err(|_| "solar_seed_reply_invalid".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> Context {
        Context {
            root: "eds_project/p1/eds_solar".into(),
            seed_source_root: "eds_solar".into(),
            ds_project: "p1".into(),
            cities: vec!["kigali".into()],
        }
    }

    fn plan() -> Plan {
        Plan {
            root: "eds_project/p1/eds_solar".into(),
            ds_project: "p1".into(),
            seed_source_root: "eds_solar".into(),
            create_count: 1,
            ..Plan::default()
        }
    }

    #[test]
    fn drift_is_project_then_source_then_mutated() {
        assert_eq!(drift(&plan(), &context()), Drift::None);
        let mut moved = plan();
        moved.root = "eds_project/p2/eds_solar".into();
        assert_eq!(drift(&moved, &context()), Drift::Project);
        let mut other_project = plan();
        other_project.ds_project = "p2".into();
        assert_eq!(drift(&other_project, &context()), Drift::Project);
        let mut other_source = plan();
        other_source.seed_source_root = "eds_solar_archive".into();
        assert_eq!(drift(&other_source, &context()), Drift::Source);
        let mut wrote = plan();
        wrote.mutated = true;
        assert_eq!(drift(&wrote, &context()), Drift::Mutated);
    }

    /// An empty source in the context means "the grant's catalog", so a plan
    /// naming any source still describes it. A drifted plan is never applyable.
    #[test]
    fn applyable_needs_a_creation_and_no_drift() {
        let mut open = context();
        open.seed_source_root = String::new();
        assert_eq!(drift(&plan(), &open), Drift::None);
        assert!(applyable(&plan(), &context()));
        let mut nothing = plan();
        nothing.create_count = 0;
        assert!(!applyable(&nothing, &context()));
        let mut wrote = plan();
        wrote.mutated = true;
        assert!(!applyable(&wrote, &context()));
    }

    /// The bound is the grant's, stated once. A headless caller must be aiming
    /// at its selected project; the catalog is a source and never a destination.
    #[test]
    fn the_envelope_is_bounded_and_the_destination_is_the_project() {
        assert!(validate_context(&context(), true).is_ok());
        let mut many = context();
        many.cities = (0..=MAX_CITIES).map(|index| index.to_string()).collect();
        assert_eq!(
            validate_context(&many, false).unwrap_err().code,
            "SOLAR_SEED_BOUNDED"
        );
        let mut padded = context();
        padded.cities = vec![" kigali".into()];
        assert_eq!(
            validate_context(&padded, false).unwrap_err().code,
            "solar_seed_city_invalid"
        );
        let mut twice = context();
        twice.cities = vec!["kigali".into(), "kigali".into()];
        assert_eq!(
            validate_context(&twice, false).unwrap_err().code,
            "solar_seed_cities_duplicated"
        );
        let mut catalog = context();
        catalog.root = "eds_solar".into();
        assert_eq!(
            validate_context(&catalog, true).unwrap_err().code,
            "solar_seed_destination_mismatch"
        );
        assert!(validate_context(&catalog, false).is_ok());
        let mut projectless = context();
        projectless.ds_project = String::new();
        assert_eq!(
            validate_context(&projectless, true).unwrap_err().code,
            "solar_seed_project_required"
        );
    }

    #[test]
    fn a_digest_is_sixty_four_lowercase_hex_characters() {
        assert!(confirm_digest(&"a".repeat(64)).is_ok());
        for bad in [
            "",
            &"A".repeat(64),
            &"g".repeat(64),
            &"a".repeat(63),
            "sha256:",
        ] {
            assert_eq!(
                confirm_digest(bad).unwrap_err().code,
                "SOLAR_SEED_DIGEST_REQUIRED"
            );
        }
    }

    #[test]
    fn a_summary_folds_plan_and_city_warnings_once() {
        let mut plan = plan();
        plan.warnings = vec!["w1".into()];
        plan.cities = vec![
            PlanCity {
                warnings: vec!["w1".into(), "w2".into()],
            },
            PlanCity {
                warnings: vec!["w2".into()],
            },
        ];
        let summary = summarize(Some(&plan));
        assert_eq!(summary.warnings, vec!["w1".to_string(), "w2".to_string()]);
        assert!(!summary.empty);
        assert!(summarize(None).empty);
    }

    #[test]
    fn evaluate_answers_every_door() {
        let reply = evaluate(
            br#"{"op":"validate","context":{"root":"eds_project/p1/eds_solar","ds_project":"p1"},"require_destination":true}"#,
        )
        .unwrap();
        assert!(reply.contains("\"ok\":true") && reply.contains("\"max_cities\":64"));
        let reply = evaluate(
            br#"{"op":"verdict","plan":{"root":"r","create_count":2},"context":{"root":"r"}}"#,
        )
        .unwrap();
        assert!(reply.contains("\"drift\":\"none\"") && reply.contains("\"applyable\":true"));
        assert!(evaluate(br#"{"op":"apply"}"#).is_err());
    }
}
