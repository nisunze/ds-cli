//! Headless feature reads over one governed transformer context.

use std::collections::BTreeMap;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use ds_geo::feature_selection::{
    FeatureSelectionError, FeatureSelector, MAX_PROJECTED_IDS, select_geojson_features,
};
use serde_json::{Value, json};

const TRANSFORMER: Arg = Arg::value(
    "transformer",
    "<name>",
    "One exact transformer in the selected headless project.",
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
    "Return this many bounded property samples.",
)
.default("0");
const IDS: Arg = Arg::value(
    "ids",
    "<0-5000>",
    "Return this many matched stable feature ids.",
)
.default("0");
const LANE: Arg = Arg::value(
    "lane",
    "<stable|canary>",
    "Deployment lane; stable is the default.",
)
.default("stable")
.choices(&["stable", "canary"]);

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
refusal!(
    SIGNED_OUT,
    "headless_signed_out",
    "the lane has no restorable native user",
    "run ds auth login --email <address>"
);
refusal!(
    NO_PROJECT,
    "headless_project_not_selected",
    "the restored user has no audience-fenced project selection",
    "run ds auth project use --project <exact-id>"
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
refusal!(
    AUTH_REJECTED,
    "auth_rejected",
    "the fixed gateway rejects the verified request",
    "verify the account and its project access"
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
    "the transformer does not exist in the selected project",
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

pub static COMMAND: Command = Command {
    id: "design.features.select",
    path: &["design", "features", "select"],
    contract: 1,
    chapter: Chapter::Design,
    summary: "Select design features without opening a map.",
    purpose: "Restores the native user, reads one exact transformer from the audience-fenced selected project through the fixed gateway call, and runs the authoritative bounded Rust selector locally. The server remains membership authority. No Desktop descriptor, project override, arbitrary request, or processing-lane value is accepted.",
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[TRANSFORMER, LAYER, WHERE, BBOX, ID, SAMPLE, IDS, LANE],
    output: "Deterministic source fence state, selected layers, scan and match counts, per-layer counts, missing identities, requested ids, and bounded samples. Legacy metadata remains explicit and no digest is synthesized.",
    examples: &[Example {
        command: "ds design features select --transformer T-1042 --layer lv_lines --where drafting_status= --sample 5",
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
    ],
    reference: Some("docs/reference/design.md"),
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
    let headless = ds_cli_auth::transformer_context(lane, transformer)?;
    let snapshot = headless.snapshot();
    let result = select_geojson_features(snapshot.layers(), selector).map_err(map_kernel)?;
    let receipt = &result.receipt;
    let ids: Vec<&str> = result
        .ids
        .iter()
        .take(ids_wanted)
        .map(String::as_str)
        .collect();
    let omitted = receipt.identified_matches.saturating_sub(ids.len());
    let version = snapshot.metadata().version();
    let digest = snapshot.metadata().content_digest();
    let fenced = version.is_some() && digest.is_some();
    let mut output = json!({
        "lane": headless.lane(),
        "project": {
            "ds_project": snapshot.ds_project(),
            "project_name": headless.project_name(),
            "status": headless.project_status(),
        },
        "transformer": snapshot.transformer_name(),
        "source": {
            "state": if fenced { "fenced" } else { "legacy" },
            "fenced": fenced,
            "legacy": !fenced,
            "version": version,
            "content_digest": digest,
        },
        "selected_layers": receipt.selected_layers,
        "scanned_features": receipt.scanned_features,
        "matched": receipt.matched_features,
        "matched_by_layer": receipt.matched_by_layer,
        "missing_identity_features": receipt.missing_identity_features,
        "identified_matches": receipt.identified_matches,
        "ids": ids,
        "sample": result.sample,
    });
    if ids_wanted > 0 && omitted > 0 {
        output["more"] = json!({
            "omitted": omitted,
            "remedy": "narrow the selector, or raise --ids",
        });
    }
    Ok(output)
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
    let values = value
        .split(',')
        .map(str::parse::<f64>)
        .collect::<Result<Vec<_>, _>>();
    let values = values.map_err(|_| map_kernel(FeatureSelectionError::InvalidBbox))?;
    values
        .try_into()
        .map_err(|_| map_kernel(FeatureSelectionError::InvalidBbox))
}

fn map_kernel(error: FeatureSelectionError) -> Failure {
    let code = error.code();
    let message = error.to_string();
    let detail = match &error {
        FeatureSelectionError::TooManyLayers { count }
        | FeatureSelectionError::TooManyWhereFilters { count }
        | FeatureSelectionError::TooManyIds { count }
        | FeatureSelectionError::SampleTooLarge { count }
        | FeatureSelectionError::ScanLimitExceeded { count } => json!({ "count": count }),
        FeatureSelectionError::DuplicateLayerFilter { layer }
        | FeatureSelectionError::InvalidFeatureCollection { layer } => json!({ "layer": layer }),
        FeatureSelectionError::InvalidWhereValue { key } => json!({ "key": key }),
        FeatureSelectionError::DuplicateId { id } => json!({ "id": id }),
        FeatureSelectionError::InvalidFeature { layer, index } => {
            json!({ "layer": layer, "index": index })
        }
        _ => Value::Null,
    };
    let failure = match error {
        FeatureSelectionError::InvalidFeatureCollection { .. }
        | FeatureSelectionError::InvalidFeature { .. } => Failure::failed(code, message)
            .remedy("update ds and report the named authoritative source layer"),
        FeatureSelectionError::ScanLimitExceeded { .. } => {
            Failure::invalid(code, message).remedy("narrow the selector with --layer")
        }
        FeatureSelectionError::MatchLimitExceeded => Failure::invalid(code, message)
            .remedy("narrow the selector with --where, --bbox, or --id"),
        _ => Failure::invalid(code, message).remedy("correct the bounded selector and retry"),
    };
    if detail.is_null() {
        failure
    } else {
        failure.detail(detail)
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
mod tests {
    use super::*;

    #[test]
    fn every_kernel_error_has_its_exact_public_code() {
        let errors = [
            FeatureSelectionError::TooManyLayers { count: 65 },
            FeatureSelectionError::InvalidLayerName,
            FeatureSelectionError::DuplicateLayerFilter { layer: "a".into() },
            FeatureSelectionError::TooManyWhereFilters { count: 65 },
            FeatureSelectionError::InvalidWhereKey,
            FeatureSelectionError::InvalidWhereValue { key: "a".into() },
            FeatureSelectionError::InvalidBbox,
            FeatureSelectionError::TooManyIds { count: 5001 },
            FeatureSelectionError::InvalidId,
            FeatureSelectionError::DuplicateId { id: "a".into() },
            FeatureSelectionError::SampleTooLarge { count: 201 },
            FeatureSelectionError::InvalidFeatureCollection { layer: "a".into() },
            FeatureSelectionError::InvalidFeature {
                layer: "a".into(),
                index: 0,
            },
            FeatureSelectionError::ScanLimitExceeded { count: 100001 },
            FeatureSelectionError::MatchLimitExceeded,
        ];
        for error in errors {
            let expected = error.code();
            assert_eq!(map_kernel(error).code(), expected);
        }
    }

    #[test]
    fn bbox_parser_rejects_bad_arity_and_kernel_rejects_nonfinite_coordinates() {
        assert_eq!(parse_bbox("1,2,3").unwrap_err().code(), "invalid_bbox");
        let parsed = parse_bbox("NaN,2,3,4").unwrap();
        let error = select_geojson_features(
            &BTreeMap::new(),
            FeatureSelector {
                bbox: Some(parsed),
                ..FeatureSelector::default()
            },
        )
        .unwrap_err();
        assert_eq!(map_kernel(error).code(), "invalid_bbox");
    }
}
