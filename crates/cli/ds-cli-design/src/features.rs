//! Headless feature reads over one governed transformer context.

use std::collections::BTreeMap;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::design_features::{
    self, DesignFeatureSourceContext, DesignFeaturesSelectFault, FeatureSelectionError,
    FeatureSelectionResult, FeatureSelector, MAX_PROJECTED_IDS,
};
use ds_network_contracts::engine::json_object_order;
use ds_network_contracts::grid_operation::{
    GRID_OPERATION_REFUSAL_SCHEMA_V1, GridOperationRefusalV1,
};
use serde_json::{Value, json};

const TRANSFORMER: Arg = Arg::value(
    "transformer",
    "<name>",
    "One exact transformer in the project named by --project.",
)
.required();
const LAYER: Arg = Arg::repeated(
    "layer",
    "<name>",
    "Design layer to search. Repeat; omit for all.",
);
const WHERE: Arg = Arg::repeated(
    "where",
    "<key=value>",
    "Property must equal this. Repeat to AND; empty means unset.",
);
const BBOX: Arg = Arg::value(
    "bbox",
    "<w,s,e,n>",
    "Only features whose extent intersects this WGS84 box.",
);
const ID: Arg = Arg::repeated(
    "id",
    "<feature-id>",
    "Narrow to exactly these stable feature ids. Repeat.",
);
const SAMPLE: Arg = Arg::value(
    "sample",
    "<0-200>",
    "Return this many bounded property and top-level geometry samples.",
)
.default("0");
const IDS: Arg = Arg::value(
    "ids",
    "<0-5000>",
    "Return this many matched stable feature ids.",
)
.default("0");
const LANE: ds_cli_contract::spec::Arg =
    ds_cli_contract::spec::LANE.summary("Deployment lane; stable is the default.");

macro_rules! refusal {
    ($name:ident, $code:literal, $when:literal, $remedy:literal) => {
        const $name: Refusal = Refusal {
            code: $code,
            when: $when,
            remedy: $remedy,
        };
    };
}

refusal!(
    PROFILE,
    "native_profile_not_configured",
    "the exact packaged native profile is unavailable",
    "install one complete ds release"
);
refusal!(
    PROFILE_DIGEST,
    "native_profile_digest_mismatch",
    "the packaged catalog differs from the build pin",
    "reinstall one complete ds release"
);
refusal!(
    PROFILE_UNSAFE,
    "native_profile_unsafe",
    "the packaged native catalog is unsafe or malformed",
    "reinstall one complete ds release"
);
const SIGNED_OUT: Refusal = ds_cli_auth::SIGNED_OUT_REFUSAL;
refusal!(
    NO_PROJECT,
    "context_corrupt",
    "--project is not one exact project id (blank, untrimmed, too long or a path)",
    "copy one exact ds_project value from ds auth project list"
);
refusal!(
    CONTEXT_STALE,
    "project_context_stale",
    "the context belongs to another user, lane, or audience",
    "select the project again with ds auth project use"
);
refusal!(
    STATE,
    "native_state_unsafe",
    "protected native state is unsafe or unreadable",
    "repair the owner-only DS config directory"
);
refusal!(
    STATE_UNAVAILABLE,
    "native_state_unavailable",
    "protected native state cannot be accessed",
    "repair the owner-only DS config directory"
);
refusal!(
    STATE_PROTECTION,
    "native_state_protection_unavailable",
    "this build has no protected-state adapter",
    "install a supported native ds build"
);
refusal!(
    STATE_ROOT,
    "native_state_root_invalid",
    "the configured state root is not absolute",
    "unset it or provide an absolute path"
);
refusal!(
    STATE_CONFLICT,
    "native_state_conflict",
    "another native operation holds the state lease",
    "retry after that operation finishes"
);
refusal!(
    CLEANUP,
    "native_cleanup_required",
    "revoked identity cleanup could not clear context",
    "repair protected state and run auth logout"
);
refusal!(
    AUTH_INPUT,
    "auth_input_invalid",
    "the transformer or bound identity input is invalid",
    "pass one exact bounded transformer name"
);
// This code covers more than one rejection: the Firebase refresh that runs
// before the call can also be rejected, and then the credential is *not*
// verified. The declaration therefore has to hold for both. The route-only
// case is answered at runtime with `ds_cli_auth::TRANSFORMER_CONTEXT_ROUTE_REMEDY`,
// which is more specific than this and wins where it applies.
refusal!(
    AUTH_REJECTED,
    "auth_rejected",
    "the authentication service, or the transformer-context route, rejected this request",
    "confirm `ds auth status` authenticates this lane; if it does, the transformer-context route rejected a verified credential and no local change fixes it"
);
refusal!(
    AUTH_REVOKED,
    "auth_revoked",
    "Firebase permanently revoked the session",
    "sign in again interactively"
);
refusal!(
    IDENTITY,
    "auth_identity_mismatch",
    "Firebase returned another identity",
    "sign in again and report a repeated mismatch"
);
refusal!(
    TRANSIENT,
    "auth_transient",
    "the fixed native service is temporarily unavailable",
    "retry without changing local state"
);
refusal!(
    UNREADABLE,
    "auth_response_unreadable",
    "the transformer reply violates its closed contract",
    "retry once, then update ds if it persists"
);
refusal!(
    NOT_FOUND,
    "transformer_not_found",
    "the transformer does not exist in the named project",
    "pass one exact transformer name from that project"
);
refusal!(
    INVALID_NUMBER,
    "ids_projection_invalid",
    "--ids is not an integer from 0 through 5000",
    "pass --ids 0 through --ids 5000"
);
refusal!(
    INVALID_PAIR,
    "invalid_where_filter",
    "a --where value has no key=value separator",
    "pass --where key=value"
);
refusal!(
    TOO_MANY_LAYERS,
    "too_many_layers",
    "the source or selector exceeds 64 layers",
    "narrow the authoritative transformer context"
);
refusal!(
    INVALID_LAYER,
    "invalid_layer_name",
    "a layer name violates the kernel bound",
    "pass a bounded exact layer name"
);
refusal!(
    DUPLICATE_LAYER,
    "duplicate_layer_filter",
    "a layer filter is repeated",
    "pass each --layer once"
);
refusal!(
    TOO_MANY_WHERE,
    "too_many_where_filters",
    "the selector exceeds 64 property filters",
    "remove property filters"
);
refusal!(
    INVALID_WHERE_KEY,
    "invalid_where_key",
    "a property key violates the kernel bound",
    "pass a bounded exact property key"
);
refusal!(
    INVALID_WHERE_VALUE,
    "invalid_where_value",
    "a property value violates the scalar bound",
    "pass a bounded scalar value"
);
refusal!(
    INVALID_BBOX,
    "invalid_bbox",
    "the WGS84 box is malformed or unordered",
    "pass west,south,east,north inside WGS84"
);
refusal!(
    TOO_MANY_IDS,
    "too_many_ids",
    "the selector exceeds 5000 ids",
    "narrow the id selector"
);
refusal!(
    INVALID_ID,
    "invalid_id",
    "a feature id violates the kernel bound",
    "pass a bounded exact stable id"
);
refusal!(
    DUPLICATE_ID,
    "duplicate_id",
    "a feature id is repeated",
    "pass each --id once"
);
refusal!(
    SAMPLE_TOO_LARGE,
    "sample_too_large",
    "--sample is not an integer from 0 through 200",
    "pass --sample 0 through --sample 200"
);
refusal!(
    INVALID_COLLECTION,
    "invalid_feature_collection",
    "a selected source layer is not GeoJSON FeatureCollection",
    "update ds and report the source layer"
);
refusal!(
    INVALID_FEATURE,
    "invalid_feature",
    "a selected source row is not a GeoJSON Feature",
    "update ds and report the source layer"
);
refusal!(
    SCAN_LIMIT,
    "scan_limit_exceeded",
    "the selector would scan over 100000 features",
    "narrow with --layer"
);
refusal!(
    MATCH_LIMIT,
    "match_limit_exceeded",
    "the selector matches over 20000 features",
    "narrow with --where, --bbox, or --id"
);
refusal!(
    ENGINE_MISSING,
    "network_engine_missing",
    "the compatible native Network engine that selects features is absent",
    "install the matching Network engine addon"
);
refusal!(
    ENGINE_INCOMPATIBLE,
    "network_engine_incompatible",
    "the captured Network engine lacks feature_selection_v1",
    "install the compatible Network engine"
);
refusal!(
    ENGINE_ORDER,
    "network_serialization_incompatible",
    "the captured Network engine orders JSON members differently from Core",
    "install the compatible Network engine"
);

pub static COMMAND: Command = Command {
    id: "design.features.select",
    path: &["design", "features", "select"],
    contract: 2,
    chapter: Chapter::Design,
    summary: "Select design features without opening a map.",
    purpose: "Restores the native user, reads one exact transformer from the project named by --project (the saved selection is never read) through the fixed gateway call, and runs the authoritative bounded Rust selector locally. The server remains membership authority. No Desktop descriptor or arbitrary request is accepted.",
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        crate::PROJECT_ARG,
        TRANSFORMER,
        LAYER,
        WHERE,
        BBOX,
        ID,
        SAMPLE,
        IDS,
        LANE,
    ],
    output: "Deterministic source fence state, selected layers, scan and match counts, per-layer counts, missing identities, requested ids, and bounded samples. Sample geometry is the Feature's authoritative top-level WGS84 GeoJSON geometry; omission state/reason and with/without/oversize counts are explicit. Legacy properties.geometry/x/y are not geometry fallbacks and have undeclared CRS unless source metadata says otherwise. Legacy source metadata remains explicit and no digest is synthesized.",
    examples: &[Example {
        command: "ds design features select --project <id> --transformer T-1042 --layer lv_lines --where drafting_status= --sample 5",
        note: "Headlessly previews unset drafting-status rows in one layer.",
        runnable: false,
    }],
    refusals: &[
        PROFILE,
        PROFILE_DIGEST,
        PROFILE_UNSAFE,
        SIGNED_OUT,
        NO_PROJECT,
        CONTEXT_STALE,
        STATE,
        STATE_UNAVAILABLE,
        STATE_PROTECTION,
        STATE_ROOT,
        STATE_CONFLICT,
        CLEANUP,
        AUTH_INPUT,
        AUTH_REJECTED,
        AUTH_REVOKED,
        IDENTITY,
        TRANSIENT,
        UNREADABLE,
        NOT_FOUND,
        INVALID_NUMBER,
        INVALID_PAIR,
        TOO_MANY_LAYERS,
        INVALID_LAYER,
        DUPLICATE_LAYER,
        TOO_MANY_WHERE,
        INVALID_WHERE_KEY,
        INVALID_WHERE_VALUE,
        INVALID_BBOX,
        TOO_MANY_IDS,
        INVALID_ID,
        DUPLICATE_ID,
        SAMPLE_TOO_LARGE,
        INVALID_COLLECTION,
        INVALID_FEATURE,
        SCAN_LIMIT,
        MATCH_LIMIT,
        ENGINE_MISSING,
        ENGINE_INCOMPATIBLE,
        ENGINE_ORDER,
    ],
    reference: Some("docs/reference/design.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let transformer = inputs.require("transformer")?;
    let lane = inputs.require("lane")?;
    let ids_wanted = bounded_usize(
        inputs.require("ids")?,
        MAX_PROJECTED_IDS,
        "ids_projection_invalid",
    )?;
    let selector = selector(inputs)?;
    let headless = ds_cli_auth::transformer_context_for_project(
        lane,
        inputs.require("project")?,
        transformer,
    )?;
    let snapshot = headless.snapshot();
    let source = DesignFeatureSourceContext::TransformerSnapshot {
        ds_project: snapshot.ds_project().to_owned(),
        transformer: snapshot.transformer_name().to_owned(),
        version: snapshot.metadata().version(),
        content_digest: snapshot.metadata().content_digest().map(str::to_owned),
    };
    let result = select_in_engine(source, snapshot.layers(), selector)?;
    let (selection, omitted) = selection_projection(&result.selection, ids_wanted);
    let version = snapshot.metadata().version();
    let digest = snapshot.metadata().content_digest();
    let fenced = version.is_some() && digest.is_some();
    let mut output = json!({
        "lane": lane,
        "project": { "ds_project": snapshot.ds_project() },
        "transformer": snapshot.transformer_name(),
        "source": {
            "state": if fenced { "fenced" } else { "legacy" },
            "fenced": fenced,
            "legacy": !fenced,
            "version": version,
            "content_digest": digest,
        },
    });
    output
        .as_object_mut()
        .expect("static command output is an object")
        .extend(
            selection
                .as_object()
                .expect("selection projection is an object")
                .clone(),
        );
    if ids_wanted > 0 && omitted > 0 {
        output["more"] = json!({
            "omitted": omitted,
            "remedy": "narrow the selector, or raise --ids",
        });
    }
    Ok(output)
}

/// The Network engine selects; Core stages the captured layers and admits the
/// reply for the source it captured before the call.
fn select_in_engine(
    source: DesignFeatureSourceContext,
    layers: &BTreeMap<String, Value>,
    selector: FeatureSelector,
) -> Result<design_features::DesignFeaturesSelectResponse, Failure> {
    let (call, inputs) =
        design_features::engine_request(&source, layers, selector).map_err(map_application)?;
    let engine =
        ds_network_client::CapturedNetworkClient::capture(1, &|| false).map_err(engine_failure)?;
    // Sample properties keep their source member order only when the engine's
    // JSON maps are this process's; another profile would reorder the reply.
    if engine.identity().json_object_order != json_object_order() {
        return Err(
            Failure::failed("network_serialization_incompatible", ENGINE_ORDER.when)
                .remedy(ENGINE_ORDER.remedy),
        );
    }
    // DS's own private disk stage, never system tmpfs or the working directory.
    let scratch = ds_network_client::private_disk_stage().map_err(engine_failure)?;
    let outcome = engine
        .grid_operation(&call, None, &inputs, scratch.path(), &|| false)
        .map_err(engine_failure)?;
    design_features::admit_engine_result(source, outcome.result.result).map_err(map_application)
}

/// The selector's own refusal keeps its code, message and detail; every
/// other failure is the engine boundary's typed refusal.
fn engine_failure(error: ds_network_client::NetworkFailure) -> Failure {
    let envelope = error
        .owner_detail
        .as_deref()
        .and_then(|detail| serde_json::from_value::<GridOperationRefusalV1>(detail.clone()).ok())
        .filter(|envelope| envelope.schema == GRID_OPERATION_REFUSAL_SCHEMA_V1);
    if let (Some(code), Some(envelope)) = (error.owner_code(), envelope) {
        return map_application(design_features::engine_refusal(
            code,
            &error.message,
            envelope.detail,
        ));
    }
    let code = error.owner_code().unwrap_or(error.code()).to_owned();
    let message = error.to_string();
    match code.as_str() {
        "network_engine_missing" | "module_removed" => {
            Failure::unavailable(&code, message).remedy(ENGINE_MISSING.remedy)
        }
        "network_engine_incompatible" => {
            Failure::failed(&code, message).remedy(ENGINE_INCOMPATIBLE.remedy)
        }
        _ if error.owner_invalid_input == Some(true) => Failure::invalid(&code, message),
        _ => Failure::failed(&code, message),
    }
}

fn selection_projection(result: &FeatureSelectionResult, ids_wanted: usize) -> (Value, usize) {
    let receipt = &result.receipt;
    let ids: Vec<&str> = result
        .ids
        .iter()
        .take(ids_wanted)
        .map(String::as_str)
        .collect();
    let omitted = receipt.identified_matches.saturating_sub(ids.len());
    (
        json!({
            "selected_layers": receipt.selected_layers,
            "scanned_features": receipt.scanned_features,
            "matched": receipt.matched_features,
            "matched_by_layer": receipt.matched_by_layer,
            "missing_identity_features": receipt.missing_identity_features,
            "identified_matches": receipt.identified_matches,
            "ids": ids,
            "sample": result.sample,
            "sample_with_geometry": receipt.sample_with_geometry,
            "sample_without_geometry": receipt.sample_without_geometry,
            "sample_geometry_oversize_omissions": receipt.sample_geometry_oversize_omissions,
        }),
        omitted,
    )
}

fn selector(inputs: &Inputs) -> Result<FeatureSelector, Failure> {
    let mut where_filters = BTreeMap::new();
    for pair in inputs.repeated("where") {
        let (key, value) = pair.split_once('=').ok_or_else(|| {
            Failure::invalid("invalid_where_filter", "--where must be key=value")
                .remedy("pass --where key=value; an empty value means unset")
        })?;
        where_filters.insert(
            key.to_owned(),
            if value.is_empty() {
                Value::Null
            } else {
                Value::String(value.to_owned())
            },
        );
    }
    let bbox = inputs.value("bbox").map(parse_bbox).transpose()?;
    let sample = bounded_usize(inputs.require("sample")?, usize::MAX, "sample_too_large")?;
    Ok(FeatureSelector {
        layers: inputs.repeated("layer").to_vec(),
        where_filters,
        bbox,
        ids: inputs.repeated("id").to_vec(),
        sample,
        region: None,
    })
}

fn bounded_usize(value: &str, max: usize, code: &'static str) -> Result<usize, Failure> {
    value
        .parse::<usize>()
        .ok()
        .filter(|value| *value <= max)
        .ok_or_else(|| {
            Failure::invalid(
                code,
                format!("value must be an integer from 0 through {max}"),
            )
        })
}

fn parse_bbox(value: &str) -> Result<[f64; 4], Failure> {
    ds_cli_contract::args::bbox(value).map_err(|_| map_kernel(FeatureSelectionError::InvalidBbox))
}

fn map_kernel(error: FeatureSelectionError) -> Failure {
    map_application(error.into())
}

fn map_application(error: DesignFeaturesSelectFault) -> Failure {
    let failure = match error.code.as_str() {
        "invalid_feature_collection" | "invalid_feature" => {
            Failure::failed(&error.code, &error.message)
                .remedy("update ds and report the named authoritative source layer")
        }
        "scan_limit_exceeded" => {
            Failure::invalid(&error.code, &error.message).remedy("narrow the selector with --layer")
        }
        "match_limit_exceeded" => Failure::invalid(&error.code, &error.message)
            .remedy("narrow the selector with --where, --bbox, or --id"),
        _ => Failure::invalid(&error.code, &error.message)
            .remedy("correct the bounded selector and retry"),
    };
    if error
        .details
        .as_object()
        .is_some_and(|details| details.is_empty())
    {
        failure
    } else {
        failure.detail(error.details)
    }
}

pub fn render(data: &Value) -> String {
    format!(
        "{} matched in {} ({} source)\n",
        data["matched"].as_u64().unwrap_or(0),
        data["transformer"].as_str().unwrap_or(""),
        data["source"]["state"].as_str().unwrap_or("legacy"),
    )
}

#[cfg(test)]
#[path = "features_tests.rs"]
mod tests;
