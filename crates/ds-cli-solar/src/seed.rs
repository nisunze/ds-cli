//! `ds solar seed` — governed project seeding, previewed then confirmed.
//!
//! Seeding copies reviewed inputs and assets into an explicitly named project.
//! Rust client core and ds-brain own context, bounds, preview and digest-bound
//! apply. Native calls require no selected project or paired Desktop.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use ds_cli_desktop::ops;
use serde_json::{Map, Value, json};

use ds_cli_contract::spec::Availability;

const PREVIEW_OPERATION: &str = "solar.seed.preview";
const APPLY_OPERATION: &str = "solar.seed.apply";
const OVERWRITE_ARG: Arg = Arg::switch(
    "overwrite",
    "Replace changed city inputs and remove obsolete seeded input documents; bind this choice in both preview and apply.",
);
const LANE_ARG: Arg = Arg::value(
    "lane",
    "<stable|canary>",
    "Native authentication lane; defaults to stable.",
);
fn native_available() -> Availability {
    // The restored native identity owns backend availability at execution.
    Availability::Available
}

/// ds-brain refuses more than this many cities in one request
/// (`solarSeedMaxCities`). The bound was held three times — here, in the
/// application's own seeding door, and in the grant. The grant keeps its copy
/// because it is the security boundary; the two client copies are now one, in
/// `ds_command_kernel::solar_seed`, so an over-large selection is refused once,
/// locally, with the code the server would have used.
pub use ds_command_kernel::solar_seed::{MAX_CITIES, MAX_CITY_CHARS, MAX_SOURCE_CHARS};

/// The document `kind` marking a city root row, and the only kind ds-brain
/// names today.
pub const DOCUMENT_KIND_ROOT: &str = "root";

/// The refusal-detail key carrying ds-brain's own spelling of a refusal code.
const SERVER_CODE_DETAIL: &str = "server_code";

/// The two actions on ds-brain's unified Solar door, in the order this domain
/// exposes them. `ds` composes no third action.
pub const SERVER_ACTIONS: &[&str] = &["seed_preview", "seed_apply"];

/// Every wire key a seeding request can carry, including the destination
/// `root` the native client composes from the explicit authorized project.
pub const SERVER_REQUEST_KEYS: &[&str] = &["root", "seed_source_root", "cities", "seed_digest"];

/// ds-brain's own refusal codes, verbatim.
///
/// Native service metadata preserves these exact conditions. Historical
/// paired receipts retain their compatibility translation. Each maps to the CLI refusal of the same name in
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

const PROJECT_ARG: Arg = Arg::value(
    "project",
    "<id>",
    "Explicit authorized destination project.",
)
.required();

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
/// Refusals raised at the native seeding boundary.
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
        when: "the explicit request project does not resolve to a project Solar root",
        remedy: "pass the exact authorized Solar project, then retry",
    },
    Refusal {
        code: "solar_seed_component_disabled",
        when: "the destination project does not declare the `solar` component",
        remedy: "enable the Solar component on the project, then retry",
    },
    Refusal {
        code: "solar_seed_contract_mismatch",
        when: "the reply is not a Solar seed plan, or a preview reports that it mutated",
        remedy: "update ds and its native client core to matching releases",
    },
];

/// `apply` adds the two refusals that only exist because it is a confirmation.
static APPLY_REFUSALS: &[Refusal] = &[
    Refusal {
        code: "solar_seed_digest_required",
        when: "--seed-digest is not the exact 64-character lowercase digest a preview returned",
        remedy: "run `ds solar seed preview --project <exact-id>` and pass that plan's exact seed_digest",
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
        when: "the explicit request project does not resolve to a project Solar root",
        remedy: "pass the exact authorized Solar project, then retry",
    },
    Refusal {
        code: "solar_seed_component_disabled",
        when: "the destination project does not declare the `solar` component",
        remedy: "enable the Solar component on the project, then retry",
    },
    Refusal {
        code: "solar_seed_contract_mismatch",
        when: "the reply is not a seed result, or it does not echo the confirmed digest",
        remedy: "update ds and its native client core to matching releases",
    },
];

const NATIVE_REFUSALS: &[Refusal] = ds_cli_auth::PROJECT_STATUS_COMMAND.refusals;
const fn with_native<const N: usize>(legacy: &[Refusal]) -> [Refusal; N] {
    let mut result = [legacy[0]; N];
    let mut i = 0;
    while i < legacy.len() {
        result[i] = legacy[i];
        i += 1;
    }
    let mut j = 0;
    while j < NATIVE_REFUSALS.len() {
        result[i + j] = NATIVE_REFUSALS[j];
        j += 1;
    }
    result
}
const PREVIEW_ALL_REFUSALS: [Refusal; SEED_REFUSALS.len() + NATIVE_REFUSALS.len()] =
    with_native(SEED_REFUSALS);
const APPLY_ALL_REFUSALS: [Refusal; APPLY_REFUSALS.len() + NATIVE_REFUSALS.len()] =
    with_native(APPLY_REFUSALS);

const REFERENCE_LOCAL: &[Refusal] = &[
    Refusal {
        code: "solar_reference_refused",
        when: "the producer or sealed bundle verification refused",
        remedy: "read the producer detail and correct site/equipment before retrying",
    },
    Refusal {
        code: "solar_reference_scope_mismatch",
        when: "the workspace belongs to another project",
        remedy: "pass the workspace project explicitly with --project",
    },
    Refusal {
        code: "solar_reference_contract_mismatch",
        when: "the Solar request is malformed",
        remedy: "install matching ds and Solar releases",
    },
    Refusal {
        code: "solar_project_schema_unavailable",
        when: "the Solar owner lacks the local workspace schema",
        remedy: "install matching ds and Solar releases",
    },
    Refusal {
        code: "solar_project_io",
        when: "private owner request or receipt IO failed",
        remedy: "verify writable private directories and matching releases",
    },
    Refusal {
        code: "solar_engine_missing",
        when: "the Solar owner is absent",
        remedy: "install the complete Linux Server package",
    },
    Refusal {
        code: "engine_refused",
        when: "the Solar owner refused preparation or verification",
        remedy: "read the bounded engine detail and correct the city inputs",
    },
];
pub(crate) const REFERENCE_REFUSALS: &[Refusal] =
    &with_native::<{ REFERENCE_LOCAL.len() + NATIVE_REFUSALS.len() }>(REFERENCE_LOCAL);

pub static PREVIEW_COMMAND: Command = Command {
    id: "solar.seed.preview",
    path: &["solar", "seed", "preview"],
    contract: 2,
    summary: "Plan which governed Solar cities would seed into this project.",
    purpose: "\
Asks ds-brain headlessly which cities, input documents \
and network assets WOULD be copied from a governed seed source into the explicitly named \
project's Solar root. It writes nothing: the plan carries the server's own \
`mutated: false`, and every row's action, digest and warning is returned \
verbatim. The explicit project is the destination. --overwrite plans replacement of changed inputs. Confirm the returned `seed_digest` with \
`ds solar seed apply` to write it.",
    chapter: Chapter::Solar,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[PROJECT_ARG, SOURCE_ARG, CITY_ARG, OVERWRITE_ARG, LANE_ARG],
    output: "\
ds-brain's SolarSeedPlan verbatim: both resolved roots, the project, the \
`seed_digest` that binds this plan to its apply, one row per city with its \
action (create/replace/skip/changed/missing), source/root/destination digests, listed \
documents including the city root row, reported assets and warnings, plus the \
class counts, document count, asset counts and `mutated`.",
    examples: &[
        Example {
            command: "ds solar seed preview --project project-1 --output json",
            note: "Plans every live governed city into the explicit project. Writes nothing.",
            runnable: false,
        },
        Example {
            command: "ds solar seed preview --project project-1 --city huye --city gasabo --output json",
            note: "Read `create_count` and the `changed` rows before confirming anything.",
            runnable: false,
        },
    ],
    refusals: &PREVIEW_ALL_REFUSALS,
    reference: Some("docs/reference/solar.md"),
    availability: native_available,
};

pub static APPLY_COMMAND: Command = Command {
    id: "solar.seed.apply",
    path: &["solar", "seed", "apply"],
    contract: 2,
    summary: "Seed exactly the previewed plan, bound to its digest.",
    purpose: "\
Confirms one plan `ds solar seed preview --project project-1` returned. --seed-digest is echoed \
from that plan and is never derived here: it is what proves the set being \
written is the set someone saw. ds-brain re-plans server-side and refuses with \
`solar_seed_digest_mismatch` if either end moved. Without --overwrite changed cities are skipped. With it, replacement uses a transaction that rechecks the previewed destination. One city commits atomically.",
    chapter: Chapter::Solar,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "seed-digest",
            "<sha256>",
            "The exact 64-character seed_digest of the previewed plan being confirmed.",
        )
        .required(),
        PROJECT_ARG,
        SOURCE_ARG,
        CITY_ARG,
        OVERWRITE_ARG,
        LANE_ARG,
    ],
    output: "\
ds-brain's SolarSeedApplyResult verbatim: the re-planned plan, the confirmed \
`seed_digest`, applied and skipped city ids with their counts, \
`documents_written` (city roots included, counted after each commit returns) \
and `idempotent`.",
    examples: &[Example {
        command: "ds solar seed apply --project project-1 --seed-digest <64-hex from preview> --city huye --yes --output json",
        note: "Confirms exactly the previewed plan; a moved source or destination is refused, not re-planned.",
        runnable: false,
    }],
    refusals: &APPLY_ALL_REFUSALS,
    reference: Some("docs/reference/solar.md"),
    availability: native_available,
};

/// The exact argument object a seeding call sends.
///
/// Pure, and separate from the handlers, so the property that matters is
/// testable without authentication or transport: ds-brain decodes the seeding body with
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
    let source = inputs.value("source");
    let cities = inputs.repeated("city");
    validate_envelope(source, cities)?;
    if let Some(source) = source {
        arguments.insert("seed_source_root".into(), json!(source));
    }
    if !cities.is_empty() {
        arguments.insert("cities".into(), json!(cities));
    }
    Ok(arguments)
}

/// The envelope, bounded by the shared kernel. The destination is NOT checked
/// here: the native session authorizes the explicit project and composes its
/// destination root before sending the command.
fn validate_envelope(source: Option<&str>, cities: &[String]) -> Result<(), Failure> {
    let context = ds_command_kernel::solar_seed::Context {
        root: String::new(),
        seed_source_root: source.unwrap_or_default().to_string(),
        ds_project: String::new(),
        cities: cities.to_vec(),
    };
    // An empty --source is the one shape the kernel reads as "the governed
    // catalog"; this door was given the flag, so an empty value is a mistake.
    if source.is_some_and(str::is_empty) {
        return Err(invalid_source());
    }
    ds_command_kernel::solar_seed::validate_context(&context, false).map_err(|refusal| {
        match refusal.code {
            "SOLAR_SEED_BOUNDED" => Failure::invalid(
                "solar_seed_bounded",
                format!(
                    "{} cities were requested; one governed seed request carries at most {MAX_CITIES}",
                    cities.len()
                ),
            )
            .remedy("seed in smaller sets of at most 64 cities")
            .detail(json!({ "given": cities.len(), "max": MAX_CITIES })),
            "solar_seed_cities_duplicated" => Failure::invalid(
                "invalid_seed_city",
                "the same --city was named twice",
            )
            .remedy("name each source city id once"),
            "solar_seed_source_invalid" => invalid_source(),
            _ => Failure::invalid(
                "invalid_seed_city",
                "each --city must be one exact unpadded source city id",
            )
            .remedy("pass exact source city ids, one per --city"),
        }
    })
}

fn invalid_source() -> Failure {
    Failure::invalid(
        "invalid_seed_source",
        "--source must be one exact governed seed source root",
    )
    .remedy("omit --source for the governed catalog, or pass one unpadded root")
}

/// `seed_digest` is only ever echoed. This checks the caller echoed something
/// that could have been a digest at all — a truncated copy/paste is refused
/// here rather than becoming a server round trip that reports drift.
fn validate_digest(raw: &str) -> Result<&str, Failure> {
    if ds_command_kernel::solar_seed::confirm_digest(raw).is_err() {
        return Err(Failure::invalid(
            "solar_seed_digest_required",
            "--seed-digest must be the exact 64-character lowercase seed_digest of a previewed plan",
        )
        .remedy("run `ds solar seed preview --project <exact-id>` and pass that plan's exact seed_digest")
        .next("ds solar seed preview --project <exact-id> --output json"));
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
fn invoke(inputs: &Inputs, _operation: &'static str, arguments: Value) -> Result<Value, Failure> {
    let mut session = ds_cli_auth::solar_project_session_for_project(
        inputs.value("lane").unwrap_or("stable"),
        inputs.require("project")?,
    )?;
    session
        .execute(&ds_cli_auth::SolarProjectCommand::Seed {
            source: inputs.value("source").map(str::to_owned),
            cities: inputs.repeated("city").to_vec(),
            overwrite: inputs.switch("overwrite"),
            digest: arguments["seed_digest"].as_str().map(str::to_owned),
        })
        .map_err(classify_seed_failure)
}

/// Name the six conditions ds-brain gives a stable code, so a caller branching
/// on `error.code` sees what the server saw.
///
/// The match is on the CODE the application reports, not on prose. A code is
/// the stable half of that contract — the message is localized in the UI and
/// would be the wrong thing to key on.
pub fn classify_seed_failure(failure: Failure) -> Failure {
    let failure = ops::classify_signed_out(failure);
    let Some(detail) = failure.detail_value() else {
        return failure;
    };
    let native_code = detail["service_code"].as_str();
    let reported = if failure.code() == "desktop_refused" {
        format!(
            "{} {}",
            detail["code"].as_str().unwrap_or_default(),
            detail["detail"].as_str().unwrap_or_default(),
        )
    } else {
        String::new()
    };
    let Some((server_code, code)) = SERVER_CODES.iter().find(|(server_code, code)| {
        native_code == Some(*code) || (!reported.is_empty() && reported.contains(server_code))
    }) else {
        return failure;
    };
    let refusal = APPLY_REFUSALS
        .iter()
        .find(|refusal| refusal.code == *code)
        .expect("every mapped server code has a declared refusal");
    // `refusal_coverage.rs` resolves a constructor's code from the code
    // argument itself — a literal, or a declared `Refusal` constant — and,
    // where that argument is a variable as it is here, from the match arm
    // pattern immediately before the call. Built before the match so nothing
    // stands between an arm's pattern and its constructor, which is what keeps
    // the two named codes below visible to that scan.
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
        if matches!(city["action"].as_str(), Some("create" | "replace")) {
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
        && result["applied_count"].as_u64()
            == plan["create_count"]
                .as_u64()
                .map(|n| n + plan["replace_count"].as_u64().unwrap_or(0))
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
        "solar_seed_contract_mismatch",
        format!("the Solar service returned an invalid reply for `{operation}`: {detail}"),
    )
    .remedy("update ds and its native client core to matching releases")
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
        "plan      create {}, replace {}, skip {}, changed {}, missing {} ({} documents)\n",
        count("create_count"),
        count("replace_count"),
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
        let mut tokens: Vec<String> = tokens.iter().map(|token| (*token).to_string()).collect();
        tokens.extend(["--project".to_owned(), "demo".to_owned()]);
        ds_cli_contract::parse(command, &tokens).expect("declared tokens parse")
    }

    fn digest(byte: char) -> String {
        byte.to_string().repeat(64)
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

    /// The bound is the kernel's now, but it must still arrive here under the
    /// server's own code and with the same remedy — one vocabulary describes
    /// one condition, wherever it is enforced.
    #[test]
    fn a_selection_larger_than_one_governed_request_is_refused_locally() {
        let over: Vec<String> = (0..=MAX_CITIES)
            .map(|index| format!("city-{index}"))
            .collect();
        assert_eq!(
            validate_envelope(None, &over)
                .expect_err("must refuse")
                .code(),
            "solar_seed_bounded"
        );
        let exact: Vec<String> = (0..MAX_CITIES)
            .map(|index| format!("city-{index}"))
            .collect();
        assert!(
            validate_envelope(None, &exact).is_ok(),
            "64 cities is the bound"
        );
        for bad in ["", " huye", "huye ", &"c".repeat(MAX_CITY_CHARS + 1)] {
            assert_eq!(
                validate_envelope(None, &[bad.to_string()])
                    .expect_err("must refuse")
                    .code(),
                "invalid_seed_city"
            );
        }
        // The same city twice is one request that would seed it once; the
        // grant refuses it, so this door does too rather than sending it.
        assert_eq!(
            validate_envelope(None, &["huye".to_string(), "huye".to_string()])
                .expect_err("must refuse")
                .code(),
            "invalid_seed_city"
        );
        // An empty --source is not the governed catalog: omitting the flag is.
        assert_eq!(
            validate_envelope(Some(""), &[])
                .expect_err("must refuse")
                .code(),
            "invalid_seed_source"
        );
        assert_eq!(
            validate_envelope(Some(&"s".repeat(MAX_SOURCE_CHARS + 1)), &[])
                .expect_err("must refuse")
                .code(),
            "invalid_seed_source"
        );
        assert!(validate_envelope(None, &[]).is_ok());
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
            "solar_seed_contract_mismatch"
        );

        // The undercount itself: rows are right, the total is one short.
        let mut undercounted = good.clone();
        undercounted["document_count"] = json!(2);
        assert_eq!(
            require_plan(&undercounted, PREVIEW_OPERATION)
                .expect_err("a document_count excluding the root must be refused")
                .code(),
            "solar_seed_contract_mismatch"
        );

        // A root row belonging to a different city is not this city's root.
        let mut foreign_root = good.clone();
        foreign_root["cities"][0]["documents"][0]["doc_id"] = json!("gasabo");
        assert_eq!(
            require_plan(&foreign_root, PREVIEW_OPERATION)
                .expect_err("a foreign root row must be refused")
                .code(),
            "solar_seed_contract_mismatch"
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
            "solar_seed_contract_mismatch"
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
    fn exact_native_seed_codes_keep_their_remedies() {
        for (server_code, expected) in SERVER_CODES {
            let refused = Failure::invalid("headless_invalid_input", "refused")
                .detail(json!({"service_code": expected}));
            let named = classify_seed_failure(refused);
            assert_eq!(named.code(), *expected);
            assert_eq!(named.detail_value().unwrap()["server_code"], *server_code);
            assert!(named.remedy_text().is_some_and(|remedy| remedy.len() > 10));
        }
        for detail in [
            json!({"service_code":"solar_seed_unknown"}),
            json!({"service_code":"prefix_solar_seed_digest_mismatch_suffix"}),
            json!({"detail":"SOLAR_SEED_DIGEST_MISMATCH"}),
        ] {
            let failure = Failure::invalid("headless_invalid_input", "refused").detail(detail);
            assert_eq!(
                classify_seed_failure(failure).code(),
                "headless_invalid_input"
            );
        }
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
