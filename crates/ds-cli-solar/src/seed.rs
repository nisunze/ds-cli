//! `ds solar seed` — governed project seeding, previewed then confirmed.
//!
//! Seeding COPIES authored Solar city inputs from a governed seed source root
//! into the active project's Solar root. ds-brain owns every decision:
//! `docs/contracts/solar-project-seeding.md` in that repository is the
//! authority, and it names exactly two actions on the existing
//! `POST /api/v1/solar` door — `seed_preview` (read only) and `seed_apply`
//! (digest bound). This crate composes neither a third action nor a second
//! seed model.
//!
//! Three properties are load bearing here, and each is a thing this module
//! deliberately does NOT do:
//!
//! * **It never derives a digest.** `seed_digest` is echoed from a preview the
//!   caller actually performed. A locally computed digest would prove nothing
//!   about what anyone looked at, which is the entire point of propose then
//!   confirm.
//! * **It never re-plans.** A row's action, its digests and the counts are the
//!   server's answer, returned verbatim. `changed`, `missing` and `warnings`
//!   are the rows a human would have acted on, so summarizing them away would
//!   be the one edit that makes the plan misleading.
//! * **It never composes the destination.** The paired application owns
//!   project identity, exactly as the ds-web seeding card does; `ds` sends the
//!   optional source and city selection and nothing else. There is no project
//!   id argument, because a project id is not proof of anything.
//!
//! What this module DOES own is the boundary: the exact keys that leave the
//! process, the local bounds that make a refusal arrive once rather than
//! twice, and the checks that a reply is the contract's own shape before a
//! caller is told a governed write happened.

use std::time::Duration;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use ds_cli_desktop::ops;
use serde_json::{Map, Value, json};

use crate::paired;

/// Preview reads two roots and digests them; apply commits one transaction per
/// city. Both are ds-brain round trips through the paired application, and the
/// UI gives the same call sixty seconds.
const SEED_TIMEOUT: Duration = Duration::from_secs(60);

const PREVIEW_OPERATION: &str = "solar.seed.preview";
const APPLY_OPERATION: &str = "solar.seed.apply";

/// ds-brain refuses more than this many cities in one request
/// (`solarSeedMaxCities`). `ds` holds the same number so an over-large
/// selection is refused once, locally, with the code the server would have
/// used — not sent, planned and refused after a round trip.
pub const MAX_CITIES: usize = 64;

/// A city id is an identity, not a payload. The same bound every other Solar
/// context argument carries.
const MAX_CITY_CHARS: usize = 128;

/// A seed source root is a governed catalog path, not a document path.
const MAX_SOURCE_CHARS: usize = 512;

/// `seed_digest` is `hex.EncodeToString` over a SHA-256 sum: 64 lowercase hex
/// characters, with no `sha256:` prefix. Solar's *other* digests are prefixed,
/// so accepting either here would let a caller confirm an apply with a
/// membership revision.
const SEED_DIGEST_CHARS: usize = 64;

/// The document `kind` marking a city root row, and the only kind ds-brain
/// names today.
pub const DOCUMENT_KIND_ROOT: &str = "root";

/// The refusal-detail key carrying ds-brain's own spelling of a refusal code.
const SERVER_CODE_DETAIL: &str = "server_code";

/// The two actions on ds-brain's unified Solar door, in the order this domain
/// exposes them. `ds` composes no third action.
pub const SERVER_ACTIONS: &[&str] = &["seed_preview", "seed_apply"];

/// Every wire key a seeding request can carry, including the destination
/// `root` the paired application composes from its own selected project.
pub const SERVER_REQUEST_KEYS: &[&str] = &["root", "seed_source_root", "cities", "seed_digest"];

/// ds-brain's own refusal codes, verbatim.
///
/// These are matched against what the paired application reports, so the
/// server's identity for a condition survives the trip rather than being
/// re-derived from prose. Each maps to the CLI refusal of the same name in
/// snake_case, which is the casing `contract.rs` requires of every code.
pub const SERVER_CODES: &[(&str, &str)] = &[
    (
        "SOLAR_SEED_PROJECT_ROOT_REQUIRED",
        "solar_seed_project_root_required",
    ),
    ("SOLAR_SEED_SOURCE_INVALID", "solar_seed_source_invalid"),
    (
        "SOLAR_SEED_COMPONENT_DISABLED",
        "solar_seed_component_disabled",
    ),
    ("SOLAR_SEED_DIGEST_REQUIRED", "solar_seed_digest_required"),
    ("SOLAR_SEED_DIGEST_MISMATCH", "solar_seed_digest_mismatch"),
    ("SOLAR_SEED_BOUNDED", "solar_seed_bounded"),
];

pub const PREVIEW_OP: ops::BridgeOp = ops::BridgeOp {
    operation: PREVIEW_OPERATION,
    arguments: &["seed_source_root", "cities"],
};
pub const APPLY_OP: ops::BridgeOp = ops::BridgeOp {
    operation: APPLY_OPERATION,
    arguments: &["seed_source_root", "cities", "seed_digest"],
};

const SOURCE_ARG: Arg = Arg::value(
    "source",
    "<root>",
    "Governed seed source root. Omit for ds-brain's governed catalog.",
);
const CITY_ARG: Arg = Arg::repeated(
    "city",
    "<id>",
    "Seed only this source city. Repeat, up to 64. Omit for every live source city.",
);
const DESCRIPTOR_ARG: Arg = Arg::value(
    "desktop-descriptor",
    "<path>",
    "Use this bridge descriptor instead of discovering one.",
);

/// Refusals `ds solar seed` raises before or instead of the paired call.
///
/// The five server-owned codes appear here because the CLI re-raises them
/// under ds-brain's own names: a caller branching on `error.code` sees the
/// condition the server saw, and the remedies are the contract's.
static SEED_REFUSALS: &[Refusal] = &[
    Refusal {
        code: "solar_seed_bounded",
        when: "more than 64 cities were requested, or the seed exceeds one governed request",
        remedy: "seed in smaller sets; split an oversized city at the source",
    },
    Refusal {
        code: "invalid_seed_city",
        when: "a --city value is blank, padded, or longer than 128 characters",
        remedy: "pass exact source city ids, one per --city",
    },
    Refusal {
        code: "invalid_seed_source",
        when: "--source is blank, padded, or longer than 512 characters",
        remedy: "omit --source for the governed catalog, or pass one exact governed root",
    },
    Refusal {
        code: "solar_seed_source_invalid",
        when: "the seed source is malformed, equals the destination, or names another project",
        remedy: "omit --source to use the governed catalog",
    },
    Refusal {
        code: "solar_seed_project_root_required",
        when: "the paired session's project does not resolve to a project Solar root",
        remedy: "select a project in DS GridDesign, then retry",
    },
    Refusal {
        code: "solar_seed_component_disabled",
        when: "the destination project does not declare the `solar` component",
        remedy: "enable the Solar component on the project, then retry",
    },
    Refusal {
        code: "desktop_signed_out",
        when: "the application is running but signed out, or has no project selected",
        remedy: "sign in and select a project in DS GridDesign",
    },
    Refusal {
        code: "desktop_not_paired",
        when: "no DS GridDesign session is running on this machine",
        remedy: "start DS GridDesign, sign in, and retry",
    },
    Refusal {
        code: "desktop_ambiguous",
        when: "more than one DS GridDesign session is running",
        remedy: "name one with --desktop-descriptor <path>",
    },
    Refusal {
        code: "desktop_unreachable",
        when: "the bridge descriptor names a session that does not answer",
        remedy: "restart DS GridDesign and retry",
    },
    Refusal {
        code: "desktop_unreadable",
        when: "the paired session's reply could not be read",
        remedy: "restart DS GridDesign and retry",
    },
    Refusal {
        code: "desktop_operation_unsupported",
        when: "this DS GridDesign build does not offer the Solar seeding operation",
        remedy: "update DS GridDesign and ds to matching releases",
    },
    Refusal {
        code: "desktop_refused",
        when: "the paired application declined the seeding call",
        remedy: "read the refusal detail, correct the application state, and retry",
    },
    Refusal {
        code: "desktop_contract_mismatch",
        when: "the reply is not a Solar seed plan, or a preview reports that it mutated",
        remedy: "update DS GridDesign and ds to matching releases",
    },
    Refusal {
        code: "pairing_rejected",
        when: "the descriptor's pairing secret is stale",
        remedy: "restart DS GridDesign to publish a fresh descriptor",
    },
];

/// `apply` adds the two refusals that only exist because it is a confirmation.
static APPLY_REFUSALS: &[Refusal] = &[
    Refusal {
        code: "solar_seed_digest_required",
        when: "--seed-digest is not the exact 64-character lowercase digest a preview returned",
        remedy: "run `ds solar seed preview` and pass that plan's exact seed_digest",
    },
    Refusal {
        code: "solar_seed_digest_mismatch",
        when: "the source or destination moved since the plan was previewed",
        remedy: "preview again, review the new plan, and confirm that digest",
    },
    Refusal {
        code: "solar_seed_bounded",
        when: "more than 64 cities were requested, or the seed exceeds one governed request",
        remedy: "seed in smaller sets; split an oversized city at the source",
    },
    Refusal {
        code: "invalid_seed_city",
        when: "a --city value is blank, padded, or longer than 128 characters",
        remedy: "pass exact source city ids, one per --city",
    },
    Refusal {
        code: "invalid_seed_source",
        when: "--source is blank, padded, or longer than 512 characters",
        remedy: "omit --source for the governed catalog, or pass one exact governed root",
    },
    Refusal {
        code: "solar_seed_source_invalid",
        when: "the seed source is malformed, equals the destination, or names another project",
        remedy: "omit --source to use the governed catalog",
    },
    Refusal {
        code: "solar_seed_project_root_required",
        when: "the paired session's project does not resolve to a project Solar root",
        remedy: "select a project in DS GridDesign, then retry",
    },
    Refusal {
        code: "solar_seed_component_disabled",
        when: "the destination project does not declare the `solar` component",
        remedy: "enable the Solar component on the project, then retry",
    },
    Refusal {
        code: "desktop_signed_out",
        when: "the application is running but signed out, or has no project selected",
        remedy: "sign in and select a project in DS GridDesign",
    },
    Refusal {
        code: "desktop_not_paired",
        when: "no DS GridDesign session is running on this machine",
        remedy: "start DS GridDesign, sign in, and retry",
    },
    Refusal {
        code: "desktop_ambiguous",
        when: "more than one DS GridDesign session is running",
        remedy: "name one with --desktop-descriptor <path>",
    },
    Refusal {
        code: "desktop_unreachable",
        when: "the bridge descriptor names a session that does not answer",
        remedy: "restart DS GridDesign and retry",
    },
    Refusal {
        code: "desktop_unreadable",
        when: "the paired session's reply could not be read",
        remedy: "restart DS GridDesign and retry",
    },
    Refusal {
        code: "desktop_operation_unsupported",
        when: "this DS GridDesign build does not offer the Solar seeding operation",
        remedy: "update DS GridDesign and ds to matching releases",
    },
    Refusal {
        code: "desktop_refused",
        when: "the paired application declined the seeding call",
        remedy: "read the refusal detail, correct the application state, and retry",
    },
    Refusal {
        code: "desktop_contract_mismatch",
        when: "the reply is not a seed result, or it does not echo the confirmed digest",
        remedy: "update DS GridDesign and ds to matching releases",
    },
    Refusal {
        code: "pairing_rejected",
        when: "the descriptor's pairing secret is stale",
        remedy: "restart DS GridDesign to publish a fresh descriptor",
    },
];

pub static PREVIEW_COMMAND: Command = Command {
    id: "solar.seed.preview",
    path: &["solar", "seed", "preview"],
    contract: 1,
    summary: "Plan which governed Solar cities would seed into this project.",
    purpose: "\
Asks ds-brain, through the paired application, which cities, input documents \
and network assets WOULD be copied from a governed seed source into the active \
project's Solar root. It writes nothing: the plan carries the server's own \
`mutated: false`, and every row's action, digest and warning is returned \
verbatim rather than summarized. The destination is the paired session's \
selected project, never an argument. Confirm the returned `seed_digest` with \
`ds solar seed apply` to write it.",
    chapter: Chapter::Solar,
    effect: Effect::ReadOnly,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[SOURCE_ARG, CITY_ARG, DESCRIPTOR_ARG],
    output: "\
ds-brain's SolarSeedPlan verbatim: both resolved roots, the project, the \
`seed_digest` that binds this plan to its apply, one row per city with its \
action (create/skip/changed/missing), source/root/destination digests, listed \
documents including the city root row, reported assets and warnings, plus the \
class counts, document count, asset counts and `mutated`.",
    examples: &[
        Example {
            command: "ds solar seed preview --output json",
            note: "Plans every live governed city into the selected project. Writes nothing.",
            runnable: false,
        },
        Example {
            command: "ds solar seed preview --city huye --city gasabo --output json",
            note: "Read `create_count` and the `changed` rows before confirming anything.",
            runnable: false,
        },
    ],
    refusals: SEED_REFUSALS,
    reference: Some("docs/reference/solar.md"),
    availability: paired::available,
};

pub static APPLY_COMMAND: Command = Command {
    id: "solar.seed.apply",
    path: &["solar", "seed", "apply"],
    contract: 1,
    summary: "Seed exactly the previewed plan, bound to its digest.",
    purpose: "\
Confirms one plan `ds solar seed preview` returned. --seed-digest is echoed \
from that plan and is never derived here: it is what proves the set being \
written is the set someone saw. ds-brain re-plans server-side and refuses with \
`solar_seed_digest_mismatch` if either end moved. Seeding never overwrites — a \
`changed` city is reported and left alone — and one city commits in one \
transaction, so a second apply of the same digest reports `idempotent`.",
    chapter: Chapter::Solar,
    effect: Effect::GlobalWrite,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "seed-digest",
            "<sha256>",
            "The exact 64-character seed_digest of the previewed plan being confirmed.",
        )
        .required(),
        SOURCE_ARG,
        CITY_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "\
ds-brain's SolarSeedApplyResult verbatim: the re-planned plan, the confirmed \
`seed_digest`, applied and skipped city ids with their counts, \
`documents_written` (city roots included, counted after each commit returns) \
and `idempotent`.",
    examples: &[Example {
        command: "ds solar seed apply --seed-digest <64-hex from preview> --city huye --yes --output json",
        note: "Confirms exactly the previewed plan; a moved source or destination is refused, not re-planned.",
        runnable: false,
    }],
    refusals: APPLY_REFUSALS,
    reference: Some("docs/reference/solar.md"),
    availability: paired::available,
};

/// The exact argument object a seeding call sends.
///
/// Pure, and separate from the handlers, so the property that matters is
/// testable with no paired application: ds-brain decodes the seeding body with
/// `DisallowUnknownFields` and treats an ABSENT source as its governed catalog
/// and an ABSENT city list as every live city. An empty string or an empty
/// array would therefore mean something different from omission, so an unset
/// optional never leaves this process — which is also exactly what the ds-web
/// card's `solarSeedRequestPayload` does.
fn arguments(inputs: &Inputs, seed_digest: Option<&str>) -> Result<Map<String, Value>, Failure> {
    let mut arguments = Map::new();
    if let Some(digest) = seed_digest {
        arguments.insert("seed_digest".into(), json!(digest));
    }
    if let Some(source) = inputs.value("source") {
        arguments.insert("seed_source_root".into(), json!(validate_source(source)?));
    }
    let cities = validate_cities(inputs.repeated("city"))?;
    if !cities.is_empty() {
        arguments.insert("cities".into(), json!(cities));
    }
    Ok(arguments)
}

fn validate_source(source: &str) -> Result<&str, Failure> {
    if source.is_empty() || source.trim() != source || source.chars().count() > MAX_SOURCE_CHARS {
        return Err(Failure::invalid(
            "invalid_seed_source",
            "--source must be one exact governed seed source root",
        )
        .remedy("omit --source for the governed catalog, or pass one unpadded root"));
    }
    Ok(source)
}

fn validate_cities(cities: &[String]) -> Result<&[String], Failure> {
    if cities.len() > MAX_CITIES {
        return Err(Failure::invalid(
            "solar_seed_bounded",
            format!(
                "{} cities were requested; one governed seed request carries at most {MAX_CITIES}",
                cities.len()
            ),
        )
        .remedy("seed in smaller sets of at most 64 cities")
        .detail(json!({ "given": cities.len(), "max": MAX_CITIES })));
    }
    if cities
        .iter()
        .any(|city| city.is_empty() || city.trim() != city || city.chars().count() > MAX_CITY_CHARS)
    {
        return Err(Failure::invalid(
            "invalid_seed_city",
            "each --city must be one exact unpadded source city id",
        )
        .remedy("pass exact source city ids, one per --city"));
    }
    Ok(cities)
}

/// `seed_digest` is only ever echoed. This checks the caller echoed something
/// that could have been a digest at all — a truncated copy/paste is refused
/// here rather than becoming a server round trip that reports drift.
fn validate_digest(raw: &str) -> Result<&str, Failure> {
    let well_formed = raw.chars().count() == SEED_DIGEST_CHARS
        && raw
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    if !well_formed {
        return Err(Failure::invalid(
            "solar_seed_digest_required",
            "--seed-digest must be the exact 64-character lowercase seed_digest of a previewed plan",
        )
        .remedy("run `ds solar seed preview` and pass that plan's exact seed_digest")
        .next("ds solar seed preview --output json"));
    }
    Ok(raw)
}

pub fn preview(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let arguments = arguments(inputs, None)?;
    let result = invoke(inputs, PREVIEW_OPERATION, Value::Object(arguments))?;
    let plan = require_plan(&result, PREVIEW_OPERATION)?;
    // A preview that reports it mutated is a contract break, not a state this
    // command can render. `mutated` is on the wire precisely so no client
    // infers "this was safe" from which action it called.
    if plan["mutated"] != Value::Bool(false) {
        return Err(mismatch(
            PREVIEW_OPERATION,
            "a seed preview reported that it mutated",
        ));
    }
    Ok(result)
}

pub fn apply(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let confirmed = validate_digest(inputs.require("seed-digest")?)?;
    let arguments = arguments(inputs, Some(confirmed))?;
    let result = invoke(inputs, APPLY_OPERATION, Value::Object(arguments))?;
    require_plan(&result, APPLY_OPERATION)?;
    // The apply is the confirmation, so its receipt must name the digest that
    // was confirmed. A result echoing a different one describes a write the
    // caller never authorized — the same reasoning as `require_exact_identity`
    // for a run receipt, applied to the only identity an apply has.
    if result["seed_digest"].as_str() != Some(confirmed) {
        return Err(mismatch(
            APPLY_OPERATION,
            "the seed result does not echo the confirmed seed_digest",
        ));
    }
    Ok(result)
}

/// Send one seeding operation and translate ds-brain's own refusal codes back
/// into named CLI refusals.
fn invoke(inputs: &Inputs, operation: &'static str, arguments: Value) -> Result<Value, Failure> {
    let op = match operation {
        PREVIEW_OPERATION => &PREVIEW_OP,
        _ => &APPLY_OP,
    };
    let descriptor = ops::paired(inputs.value("desktop-descriptor"))?;
    ops::invoke(&descriptor, op, arguments, SEED_TIMEOUT).map_err(classify_seed_failure)
}

/// Name the six conditions ds-brain gives a stable code, so a caller branching
/// on `error.code` sees what the server saw.
///
/// The match is on the CODE the application reports, not on prose. A code is
/// the stable half of that contract — the message is localized in the UI and
/// would be the wrong thing to key on.
pub fn classify_seed_failure(failure: Failure) -> Failure {
    let failure = ops::classify_signed_out(failure);
    if failure.code() != "desktop_refused" {
        return failure;
    }
    let Some(detail) = failure.detail_value() else {
        return failure;
    };
    let reported = format!(
        "{} {}",
        detail["code"].as_str().unwrap_or_default(),
        detail["detail"].as_str().unwrap_or_default(),
    );
    let Some((server_code, code)) = SERVER_CODES
        .iter()
        .find(|(server_code, _)| reported.contains(server_code))
    else {
        return failure;
    };
    let refusal = APPLY_REFUSALS
        .iter()
        .find(|refusal| refusal.code == *code)
        .expect("every mapped server code has a declared refusal");
    // Built before the match so no bare string literal sits behind a
    // constructor whose code is a variable: `refusal_coverage.rs` reads the
    // first literal after each `Failure::…(` as that call's code, and a detail
    // KEY caught there would be reported as an undocumented refusal.
    let detail = json!({ SERVER_CODE_DETAIL: server_code });
    let named = match *code {
        "solar_seed_digest_mismatch" => Failure::conflict(*code, refusal.when),
        "solar_seed_component_disabled" => Failure::unauthorized(*code, refusal.when),
        _ => Failure::invalid(*code, refusal.when),
    };
    named.remedy(refusal.remedy).detail(detail)
}

/// Refuse a reply that is not the contract's own shape.
///
/// Mirrors the ds-web client's boundary parser rather than a cast: a plan with
/// no digest cannot be confirmed and one with no roots cannot be drift
/// checked, so neither is safe to hand back as a plan. The accounting check is
/// here for the same reason it exists in ds-brain — `document_count` and
/// `documents_written` describe the same population only while the city ROOT
/// is a listed row, and a plan that enumerated everything except the document
/// deciding whether the city exists once promised fewer documents than the
/// apply then wrote.
fn require_plan<'a>(result: &'a Value, operation: &'static str) -> Result<&'a Value, Failure> {
    let plan = if result["plan"].is_object() {
        &result["plan"]
    } else {
        result
    };
    for field in ["root", "seed_source_root", "seed_digest"] {
        if plan[field].as_str().is_none_or(str::is_empty) {
            return Err(mismatch(
                operation,
                &format!("the seed plan carries no `{field}`"),
            ));
        }
    }
    let cities = plan["cities"]
        .as_array()
        .ok_or_else(|| mismatch(operation, "the seed plan carries no city rows"))?;

    let mut creatable_documents = 0_u64;
    for city in cities {
        let city_id = city["city_id"].as_str().unwrap_or_default();
        let documents = city["documents"].as_array().map_or(&[][..], Vec::as_slice);
        if documents.is_empty() {
            continue;
        }
        let roots = documents
            .iter()
            .filter(|document| document["kind"].as_str() == Some(DOCUMENT_KIND_ROOT))
            .collect::<Vec<_>>();
        // Exactly one root row, first, with the city's own id and no
        // subcollection. `kind` is what identifies it; an empty subcollection
        // alone would also match a malformed ordinary row.
        if roots.len() != 1
            || roots[0]["doc_id"].as_str() != Some(city_id)
            || !roots[0]["subcollection"]
                .as_str()
                .unwrap_or_default()
                .is_empty()
            || documents[0]["kind"].as_str() != Some(DOCUMENT_KIND_ROOT)
        {
            return Err(mismatch(
                operation,
                &format!("city `{city_id}` does not list its city root as its first document"),
            ));
        }
        if city["action"].as_str() == Some("create") {
            creatable_documents += documents.len() as u64;
        }
    }
    if plan["document_count"].as_u64() != Some(creatable_documents) {
        return Err(mismatch(
            operation,
            "document_count does not equal the documents the creatable cities list",
        ));
    }

    // An apply reports what the store holds. When every creatable city
    // committed, that is exactly the population the plan promised; a run that
    // lost a create race legitimately writes fewer, so only the reconciling
    // case is asserted.
    if result["plan"].is_object()
        && result["applied_count"].as_u64() == plan["create_count"].as_u64()
        && result["documents_written"].as_u64() != Some(creatable_documents)
    {
        return Err(mismatch(
            operation,
            "documents_written does not reconcile with the plan it applied",
        ));
    }
    Ok(plan)
}

fn mismatch(operation: &'static str, detail: &str) -> Failure {
    Failure::unavailable(
        "desktop_contract_mismatch",
        format!("the paired session returned an invalid reply for `{operation}`: {detail}"),
    )
    .remedy("update DS GridDesign and ds to matching releases")
}

/// The human tier. Every class of row is named, because the whole value of a
/// preview is the rows nobody planned for: a `changed` destination that will
/// not be overwritten, a `missing` source city, and the assets a seeded city
/// will not have.
pub fn render(data: &Value) -> String {
    let plan = if data["plan"].is_object() {
        &data["plan"]
    } else {
        data
    };
    let count = |field: &str| plan[field].as_u64().unwrap_or(0);
    let mut out = String::new();
    out.push_str(&format!(
        "project   {}\nsource    {}\ndigest    {}\n",
        plan["ds_project"].as_str().unwrap_or("?"),
        plan["seed_source_root"].as_str().unwrap_or("?"),
        plan["seed_digest"].as_str().unwrap_or("?"),
    ));
    out.push_str(&format!(
        "plan      create {}, skip {}, changed {}, missing {} ({} documents)\n",
        count("create_count"),
        count("skip_count"),
        count("changed_count"),
        count("missing_count"),
        count("document_count"),
    ));
    for city in plan["cities"].as_array().into_iter().flatten() {
        let action = city["action"].as_str().unwrap_or("?");
        if action == "create" {
            continue;
        }
        out.push_str(&format!(
            "{action:<9} {}  {}\n",
            city["city_id"].as_str().unwrap_or("?"),
            city["reason"].as_str().unwrap_or(""),
        ));
    }
    if data["plan"].is_object() {
        out.push_str(&format!(
            "applied   {} cities, {} documents{}\n",
            data["applied_count"].as_u64().unwrap_or(0),
            data["documents_written"].as_u64().unwrap_or(0),
            if data["idempotent"] == Value::Bool(true) {
                " (idempotent; nothing was written)"
            } else {
                ""
            },
        ));
    }
    for warning in plan["warnings"].as_array().into_iter().flatten() {
        out.push_str(&format!("warning   {}\n", warning.as_str().unwrap_or("?")));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs(command: &'static Command, tokens: &[&str]) -> Inputs {
        let tokens: Vec<String> = tokens.iter().map(|token| (*token).to_string()).collect();
        ds_cli_contract::parse(command, &tokens).expect("declared tokens parse")
    }

    fn digest(byte: char) -> String {
        byte.to_string().repeat(SEED_DIGEST_CHARS)
    }

    fn city_row(city_id: &str, action: &str, documents: usize) -> Value {
        let mut rows = vec![json!({
            "subcollection": "",
            "doc_id": city_id,
            "kind": "root",
            "digest": "r",
            "bytes": 8,
        })];
        for index in 1..documents {
            rows.push(json!({
                "subcollection": "01_city_inputs",
                "doc_id": format!("input-{index}"),
                "digest": "d",
                "bytes": 4,
            }));
        }
        json!({
            "city_id": city_id,
            "action": action,
            "source_digest": digest('a'),
            "root_digest": digest('b'),
            "documents": rows,
            "assets": [],
        })
    }

    fn plan(cities: Vec<Value>) -> Value {
        let document_count: usize = cities
            .iter()
            .filter(|city| city["action"] == "create")
            .map(|city| city["documents"].as_array().map_or(0, Vec::len))
            .sum();
        json!({
            "root": "eds_project/demo/eds_solar",
            "seed_source_root": "eds_solar",
            "ds_project": "demo",
            "seed_digest": digest('d'),
            "cities": cities,
            "create_count": cities.iter().filter(|city| city["action"] == "create").count(),
            "skip_count": 0,
            "changed_count": 0,
            "missing_count": 0,
            "document_count": document_count,
            "asset_count": 0,
            "excluded_asset_count": 0,
            "mutated": false,
        })
    }

    #[test]
    fn an_unset_optional_never_reaches_the_governed_request() {
        // ds-brain decodes with DisallowUnknownFields and reads an ABSENT
        // source as its governed catalog and an ABSENT city list as every live
        // city. Sending "" or [] would be a different request, so this is the
        // negative control for the whole payload.
        let sent = arguments(&inputs(&PREVIEW_COMMAND, &[]), None).expect("no flags is valid");
        assert!(sent.is_empty(), "a bare preview must send no keys at all");

        let selected = arguments(
            &inputs(
                &PREVIEW_COMMAND,
                &[
                    "--city",
                    "huye",
                    "--city",
                    "gasabo",
                    "--source",
                    "eds_solar",
                ],
            ),
            None,
        )
        .expect("declared flags are valid");
        assert_eq!(
            Value::Object(selected),
            json!({ "seed_source_root": "eds_solar", "cities": ["huye", "gasabo"] })
        );
    }

    #[test]
    fn every_key_a_seed_sends_is_declared_by_its_closed_operation() {
        let confirmed = digest('d');
        let sent = Value::Object(
            arguments(
                &inputs(
                    &APPLY_COMMAND,
                    &["--seed-digest", &confirmed, "--city", "huye"],
                ),
                Some(&confirmed),
            )
            .expect("declared flags are valid"),
        );
        assert_eq!(sent["seed_digest"], json!(confirmed));
        assert_eq!(ops::undeclared_key(&APPLY_OP, &sent), None);
        // Preview may not carry a digest: it is not a confirmation, and the
        // operation that reads must not accept the argument that writes.
        assert_eq!(
            ops::undeclared_key(&PREVIEW_OP, &sent),
            Some("seed_digest".to_string())
        );
    }

    #[test]
    fn a_digest_is_only_ever_echoed_never_shaped_into_something_plausible() {
        assert!(validate_digest(&digest('d')).is_ok());
        for bad in [
            "",
            "  ",
            &digest('d')[..63],
            &format!("sha256:{}", digest('d')),
            &digest('D'),
            &digest('z'),
        ] {
            assert_eq!(
                validate_digest(bad)
                    .expect_err("a non-digest must be refused")
                    .code(),
                "solar_seed_digest_required",
                "`{bad}` was accepted as a previewed digest"
            );
        }
    }

    #[test]
    fn a_selection_larger_than_one_governed_request_is_refused_locally() {
        let over: Vec<String> = (0..=MAX_CITIES)
            .map(|index| format!("city-{index}"))
            .collect();
        assert_eq!(
            validate_cities(&over).expect_err("must refuse").code(),
            "solar_seed_bounded"
        );
        let exact: Vec<String> = (0..MAX_CITIES)
            .map(|index| format!("city-{index}"))
            .collect();
        assert!(validate_cities(&exact).is_ok(), "64 cities is the bound");
        for bad in ["", " huye", "huye ", &"c".repeat(MAX_CITY_CHARS + 1)] {
            assert_eq!(
                validate_cities(&[bad.to_string()])
                    .expect_err("must refuse")
                    .code(),
                "invalid_seed_city"
            );
        }
    }

    #[test]
    fn a_plan_that_omits_the_city_root_row_is_not_a_plan() {
        // The exact regression ds-brain bcd502d fixed and ds-web 607f6cbd
        // mirrored: the root is a document the apply writes, so a plan without
        // it undercounts what a caller is being asked to confirm.
        let good = plan(vec![city_row("huye", "create", 3)]);
        assert!(require_plan(&good, PREVIEW_OPERATION).is_ok());

        let mut rootless = good.clone();
        rootless["cities"][0]["documents"] = json!([{
            "subcollection": "01_city_inputs",
            "doc_id": "input-1",
            "digest": "d",
            "bytes": 4,
        }]);
        rootless["document_count"] = json!(1);
        assert_eq!(
            require_plan(&rootless, PREVIEW_OPERATION)
                .expect_err("a rootless city must be refused")
                .code(),
            "desktop_contract_mismatch"
        );

        // The undercount itself: rows are right, the total is one short.
        let mut undercounted = good.clone();
        undercounted["document_count"] = json!(2);
        assert_eq!(
            require_plan(&undercounted, PREVIEW_OPERATION)
                .expect_err("a document_count excluding the root must be refused")
                .code(),
            "desktop_contract_mismatch"
        );

        // A root row belonging to a different city is not this city's root.
        let mut foreign_root = good.clone();
        foreign_root["cities"][0]["documents"][0]["doc_id"] = json!("gasabo");
        assert_eq!(
            require_plan(&foreign_root, PREVIEW_OPERATION)
                .expect_err("a foreign root row must be refused")
                .code(),
            "desktop_contract_mismatch"
        );
    }

    #[test]
    fn only_creatable_rows_are_counted_and_reported_rows_keep_their_documents() {
        // `changed` and `skip` rows carry their documents too, but
        // `document_count` is what an apply would WRITE. Counting a reported
        // row would offer to write a city seeding refuses to touch.
        let mixed = plan(vec![
            city_row("huye", "create", 3),
            city_row("gasabo", "changed", 5),
            city_row("musanze", "skip", 2),
        ]);
        assert_eq!(mixed["document_count"], json!(3));
        assert!(require_plan(&mixed, PREVIEW_OPERATION).is_ok());
    }

    #[test]
    fn an_apply_receipt_must_reconcile_with_the_plan_it_says_it_applied() {
        let applied = plan(vec![city_row("huye", "create", 3)]);
        let receipt = json!({
            "status": "ok",
            "plan": applied,
            "seed_digest": digest('d'),
            "applied_cities": ["huye"],
            "skipped_cities": [],
            "applied_count": 1,
            "skipped_count": 0,
            "documents_written": 3,
            "idempotent": false,
        });
        assert!(require_plan(&receipt, APPLY_OPERATION).is_ok());

        let mut short = receipt.clone();
        short["documents_written"] = json!(2);
        assert_eq!(
            require_plan(&short, APPLY_OPERATION)
                .expect_err("a receipt that wrote fewer than it planned must be refused")
                .code(),
            "desktop_contract_mismatch"
        );

        // A city that lost the create race legitimately writes fewer, and
        // says so by applying fewer cities than the plan could create.
        let mut raced = receipt;
        raced["applied_count"] = json!(0);
        raced["skipped_count"] = json!(1);
        raced["documents_written"] = json!(0);
        assert!(
            require_plan(&raced, APPLY_OPERATION).is_ok(),
            "a lost create race is not a contract mismatch"
        );
    }

    #[test]
    fn a_servers_own_refusal_code_survives_the_paired_trip() {
        for (server_code, expected) in SERVER_CODES {
            let refused = Failure::failed("desktop_refused", "refused").detail(json!({
                "http_status": 409,
                "detail": format!("Solar seed plan changed since it was previewed ({server_code})"),
            }));
            let named = classify_seed_failure(refused);
            assert_eq!(named.code(), *expected, "{server_code} lost its identity");
            assert_eq!(
                named.detail_value().expect("detail")["server_code"],
                json!(server_code),
                "the server's own code must survive verbatim"
            );
            assert!(named.remedy_text().is_some_and(|remedy| remedy.len() > 10));
        }

        // An ordinary application refusal keeps its own name.
        let other = Failure::failed("desktop_refused", "refused")
            .detail(json!({ "detail": "the seeding card is busy" }));
        assert_eq!(classify_seed_failure(other).code(), "desktop_refused");
    }

    #[test]
    fn the_human_tier_never_hides_a_row_a_person_would_act_on() {
        let mut mixed = plan(vec![
            city_row("huye", "create", 3),
            city_row("gasabo", "changed", 5),
        ]);
        mixed["cities"][1]["reason"] = json!("destination_differs");
        mixed["warnings"] = json!(["network_assets_are_not_seeded"]);
        let rendered = render(&mixed);
        assert!(rendered.contains(&digest('d')), "{rendered}");
        assert!(
            rendered.contains("changed   gasabo  destination_differs"),
            "a changed destination must be named: {rendered}"
        );
        assert!(
            rendered.contains("warning   network_assets_are_not_seeded"),
            "an excluded asset warning must survive: {rendered}"
        );
    }
}
