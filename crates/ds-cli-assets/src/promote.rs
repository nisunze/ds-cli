//! `ds assets promote` — a geo asset, or a geo pack member, as a local layer.
//!
//! Promotion is the one place an asset becomes something the map draws
//! permanently, and it is always explicit and always local (§5, §12.7). The
//! geometry goes to this machine's prepared local layer store — the store
//! `ds map local register` writes and `ds map local list` reads, kept per
//! lane and DS account: no new store, no upload, one copied payload. The
//! asset is not touched, and the layer cannot be shared more widely than the
//! class it inherits.
//!
//! Headless, the store admits GeoJSON feature collections of one geometry
//! type; the asset (or the member) must be one, and its geometry type is read
//! from its features. A projected `sys:` row is refused by name, as every
//! byte-level command refuses it here.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_layer_store::prepared::{self, GeometryType, Host, Op, Register, Scope, StoreError};
use serde_json::{Map, Value, json};

use crate::{ASSET_ARG, LANE_ARG, MEMBER_ARG, PROJECT_ARG};

/// The prepared layer store's own refusals, in the words `ds map local`
/// declares them, so a caller who planned for them there has planned for
/// them here.
const INVALID_PAYLOAD: Refusal = Refusal {
    code: "invalid_payload",
    when: "the asset or member is not a bounded GeoJSON FeatureCollection of one geometry type",
    remedy: "promote a GeoJSON asset of at most 64 MiB whose features all carry one geometry type; convert another geo format first",
};
const MALFORMED_DESCRIPTOR: Refusal = Refusal {
    code: "malformed_descriptor",
    when: "the kernel refused the layer name, a bound or a row already in the catalogue",
    remedy: "use a trimmed name of 1 to 80 characters; repair a row the message names",
};
const STORE_REFUSED: Refusal = Refusal {
    code: "local_layer_refused",
    when: "this machine's prepared layer catalogue cannot be read or persisted, or is malformed",
    remedy: "repair the store under DS_LAYER_HOME (or the local data directory); it is never overwritten for you",
};

const AS_LAYER_ARG: Arg =
    Arg::value("as-layer", "<name>", "The local layer's display name.").required();

pub static COMMAND: Command = Command {
    id: "assets.promote",
    path: &["assets", "promote"],
    contract: 1,
    summary: "Promote a geo asset, or a geo pack member, to a local layer.",
    purpose: "\
Fetches the asset's bytes through the catalogue's signed read and admits them \
to this machine's prepared local layer store — the one `ds map local` reads \
and writes, per lane and DS account — never a new store, never an upload. \
The layer records the source asset as provenance; the asset is unchanged. The \
store admits a GeoJSON feature collection of one geometry type: a non-geo \
asset, another geo format, mixed geometries or a feature count over the cap \
is refused with the reason. A projected sys: row is refused by name.",
    chapter: Chapter::Assets,
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[ASSET_ARG, MEMBER_ARG, AS_LAYER_ARG, LANE_ARG, PROJECT_ARG],
    output: "\
`layer_id` of the prepared local layer, its `feature_count`, `geometry_type`, \
`source_name` (the asset's name, its provenance) and the copied `payload`.",
    examples: &[Example {
        command: "ds assets promote --project <exact-id> --asset a_7kq3nr2v0b1c --member Lot3/gis/poles.shp --as-layer Lot3-poles --output json",
        note: "The layer then appears in `ds map local list`; the asset is untouched.",
        runnable: false,
    }],
    refusals: &crate::refusals::<33>(&[
        crate::INVALID_ASSET_ID,
        crate::INVALID_LAYER_NAME,
        crate::INVALID_MEMBER,
        crate::ASSET_TOO_LARGE,
        crate::ORIGIN_READ_FAILED,
        crate::ORIGIN_READ_UNAVAILABLE,
        crate::ASSET_NOT_GEOGRAPHIC,
        crate::ASSETS_UNREADABLE,
        INVALID_PAYLOAD,
        MALFORMED_DESCRIPTOR,
        STORE_REFUSED,
    ]),
    reference: Some("docs/reference/assets.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

/// The promotion, validated locally, in the exact keys the operation
/// declares.
fn arguments(inputs: &Inputs) -> Result<Value, Failure> {
    let mut arguments = Map::new();
    arguments.insert(
        "asset".into(),
        json!(crate::asset_id(inputs.require("asset")?, "asset")?),
    );
    if let Some(member) = inputs
        .value("member")
        .map(str::trim)
        .filter(|member| !member.is_empty())
    {
        arguments.insert("member".into(), json!(member));
    }
    arguments.insert(
        "as_layer".into(),
        json!(layer_name(inputs.require("as-layer")?)?),
    );
    Ok(Value::Object(arguments))
}

/// `--as-layer`, held to exactly the bound the owning path holds it to: one
/// printable label of at most [`crate::MAX_LAYER_NAME_CHARS`] characters.
/// What the label may contain beyond that is the layer store's own rule, and
/// it is not second-guessed here.
fn layer_name(raw: &str) -> Result<String, Failure> {
    let trimmed = raw.trim();
    let characters = trimmed.chars().count();
    if trimmed.is_empty()
        || characters > crate::MAX_LAYER_NAME_CHARS
        || trimmed.chars().any(char::is_control)
    {
        return Err(Failure::invalid(
            "invalid_layer_name",
            format!(
                "`--as-layer` must be 1 to {} printable characters",
                crate::MAX_LAYER_NAME_CHARS
            ),
        )
        .remedy(crate::INVALID_LAYER_NAME.remedy)
        .detail(json!({ "given_chars": characters, "max": crate::MAX_LAYER_NAME_CHARS })));
    }
    Ok(trimmed.to_string())
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let arguments = arguments(inputs)?;
    let lane = inputs.value("lane").unwrap_or("stable");
    let project = inputs.require("project")?;
    let asset_id = arguments["asset"].as_str().unwrap_or_default().to_owned();
    let member = arguments["member"].as_str().map(str::to_owned);
    let as_layer = arguments["as_layer"]
        .as_str()
        .unwrap_or_default()
        .to_owned();

    let report = ds_cli_auth::read_asset_bytes_for_project(lane, project, &asset_id)?;
    let uid = report.identity().uid().to_owned();
    let (row, bytes) = report.into_result();
    if member.is_none() && row["kind"] != "geo" {
        return Err(Failure::invalid(
            "asset_not_geographic",
            format!(
                "{} is a {} asset, and only a geographic asset promotes",
                row["name"].as_str().unwrap_or(&asset_id),
                row["kind"].as_str().unwrap_or("?")
            ),
        )
        .remedy(crate::ASSET_NOT_GEOGRAPHIC.remedy));
    }
    let (payload, source_name) = match member.as_deref() {
        None => (bytes, row["name"].as_str().unwrap_or(&asset_id).to_owned()),
        Some(member) => {
            let extracted = crate::with_bytes(
                &bytes,
                &json!({
                    "schema": crate::REQUEST_SCHEMA,
                    "action": "extract_member",
                    "member": member,
                }),
            )?;
            (
                crate::decode_base64(&extracted["bytes_b64"])?,
                member.to_owned(),
            )
        }
    };
    let geometry = geometry_of(&payload)?;

    // The store copies a payload from a path on this host; the fetched bytes
    // are written beside the store's own root and removed once admitted.
    let staging = std::env::temp_dir().join(format!(
        "ds-assets-promote-{}-{}.geojson",
        std::process::id(),
        asset_id.trim_start_matches("a_")
    ));
    std::fs::write(&staging, &payload).map_err(|error| {
        Failure::failed(
            "origin_read_failed",
            format!(
                "could not stage the payload at {}: {error}",
                staging.display()
            ),
        )
        .remedy(crate::ORIGIN_READ_FAILED.remedy)
    })?;
    let scope = Scope {
        host: Host::Native,
        lane: Some(lane.to_owned()),
        uid: Some(uid),
    };
    let admitted = prepared::execute(
        &scope,
        Op::Register(Register {
            name: as_layer,
            geometry,
            source_name: Some(source_name),
            from: Some(staging.clone()),
            now_ms: None,
        }),
    );
    let _ = std::fs::remove_file(&staging);
    let answer = admitted.map_err(refuse)?;
    let receipt = &answer["receipt"];
    Ok(json!({
        "layer_id": receipt["id"],
        "name": receipt["name"],
        "feature_count": receipt["featureCount"],
        "geometry_type": receipt["geometryType"],
        "source_name": receipt["sourceName"],
        "color": receipt["color"],
        "payload": answer["payload"],
        "asset_id": asset_id,
        "member": member,
        "lane": lane,
    }))
}

/// The one geometry type a GeoJSON feature collection carries, read from its
/// features; a collection of two kinds, or of none, is not a layer the store
/// admits.
fn geometry_of(payload: &[u8]) -> Result<GeometryType, Failure> {
    let refuse = |why: &str| {
        Failure::invalid("invalid_payload", format!("the asset {why}"))
            .remedy(INVALID_PAYLOAD.remedy)
    };
    let document: Value =
        serde_json::from_slice(payload).map_err(|_| refuse("is not a GeoJSON document"))?;
    let features = document["features"]
        .as_array()
        .filter(|_| document["type"] == "FeatureCollection")
        .ok_or_else(|| refuse("is not a GeoJSON FeatureCollection"))?;
    let mut found: Option<GeometryType> = None;
    for feature in features {
        let kind = match feature["geometry"]["type"].as_str() {
            Some("Point" | "MultiPoint") => GeometryType::Point,
            Some("LineString" | "MultiLineString") => GeometryType::LineString,
            Some("Polygon" | "MultiPolygon") => GeometryType::Polygon,
            _ => {
                return Err(refuse(
                    "holds a feature with no point, line or polygon geometry",
                ));
            }
        };
        match found {
            None => found = Some(kind),
            Some(seen) if seen != kind => {
                return Err(refuse("mixes geometry types; a local layer holds one"));
            }
            Some(_) => {}
        }
    }
    found.ok_or_else(|| refuse("holds no features"))
}

/// Re-raise the store's refusal under its own name, as `ds map local` does.
fn refuse(error: StoreError) -> Failure {
    match error {
        StoreError::Refused { code, message } => match code.as_str() {
            "malformed_descriptor" | "duplicate_layer" | "scope_mismatch" => {
                Failure::invalid("malformed_descriptor", message)
                    .remedy(MALFORMED_DESCRIPTOR.remedy)
            }
            _ => Failure::invalid("local_layer_refused", message).remedy(STORE_REFUSED.remedy),
        },
        StoreError::Payload(message) => {
            Failure::invalid("invalid_payload", message).remedy(INVALID_PAYLOAD.remedy)
        }
        StoreError::Store(message) => {
            Failure::invalid("local_layer_refused", message).remedy(STORE_REFUSED.remedy)
        }
    }
}

pub fn render(data: &Value) -> String {
    format!(
        "prepared {} · {} ({} {} features)\n  filed under {}\n  from asset {}\n",
        data["layer_id"].as_str().unwrap_or("?"),
        data["name"].as_str().unwrap_or("?"),
        data["feature_count"],
        data["geometry_type"].as_str().unwrap_or("?"),
        data["source_name"].as_str().unwrap_or("?"),
        data["asset_id"].as_str().unwrap_or("?"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(tokens: &[&str]) -> Inputs {
        let mut tokens: Vec<String> = tokens.iter().map(|token| (*token).to_string()).collect();
        tokens.extend(["--project".to_string(), "test_project".to_string()]);
        ds_cli_contract::parse(&COMMAND, &tokens).expect("declared inputs")
    }

    #[test]
    fn a_layer_name_is_one_short_label_and_is_checked_before_any_round_trip() {
        let long = "x".repeat(crate::MAX_LAYER_NAME_CHARS + 1);
        for bad in ["", "   ", "poles\nlayer", "poles\tlayer", long.as_str()] {
            let failure = arguments(&parse(&["--asset", "a_7kq3nr2v0b1c", "--as-layer", bad]))
                .expect_err("must refuse");
            assert_eq!(
                failure.code(),
                "invalid_layer_name",
                "`{bad}` was accepted as a layer name"
            );
            let detail = failure.detail_value().cloned().unwrap_or(Value::Null);
            assert_eq!(
                detail["max"],
                json!(crate::MAX_LAYER_NAME_CHARS),
                "the bound must be said"
            );
        }
        // The bound itself is valid, and it is the owner's bound: a name the
        // application would take is never refused here first.
        let payload = arguments(&parse(&[
            "--asset",
            "a_7kq3nr2v0b1c",
            "--as-layer",
            &"x".repeat(crate::MAX_LAYER_NAME_CHARS),
        ]))
        .expect("the bound itself is valid");
        assert_eq!(payload["as_layer"].as_str().expect("name").len(), 80);
        // Punctuation is the owning path's business, not this CLI's.
        assert!(
            arguments(&parse(&[
                "--asset",
                "a_7kq3nr2v0b1c",
                "--as-layer",
                "Lot 3 / poles (v2)"
            ]))
            .is_ok()
        );
    }

    #[test]
    fn a_projected_row_is_promotable_because_promoting_writes_nothing_back() {
        // classify, attach and folder refuse a `sys:` id at the flag; promote,
        // read and preview accept the id (§7.1) and refuse it by name only
        // when its bytes are asked for, because headless the catalogue does
        // not serve a projected row's bytes.
        let payload = arguments(&parse(&[
            "--asset",
            "sys:transformer_version:AGASHARU:v3",
            "--as-layer",
            "AGASHARU v3",
        ]))
        .expect("valid");
        assert_eq!(
            payload["asset"],
            json!("sys:transformer_version:AGASHARU:v3")
        );
    }

    #[test]
    fn the_payload_carries_exactly_the_keys_the_operation_declares() {
        let with_member = arguments(&parse(&[
            "--asset",
            "a_7kq3nr2v0b1c",
            "--member",
            "Lot3/gis/poles.shp",
            "--as-layer",
            "Lot3-poles",
        ]))
        .expect("valid");
        let mut keys: Vec<&str> = with_member
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, ["as_layer", "asset", "member"]);
        // A whole-asset promotion sends no member at all rather than an empty
        // one, which the walk would read as a member named "".
        let whole = arguments(&parse(&[
            "--asset",
            "a_7kq3nr2v0b1c",
            "--member",
            "  ",
            "--as-layer",
            "Lot3-poles",
        ]))
        .expect("valid");
        assert!(whole.get("member").is_none());
    }

    #[test]
    fn the_human_projection_reports_the_layer_the_caller_can_now_style() {
        let rendered = render(&json!({
            "layer_id": "sketch-1758000000000-0", "name": "Lot3-poles", "feature_count": 412,
            "geometry_type": "point", "source_name": "poles.geojson", "asset_id": "a_7kq3nr2v0b1c"
        }));
        assert!(rendered.contains("prepared sketch-1758000000000-0"));
        assert!(rendered.contains("412 point features"));
        assert!(rendered.contains("from asset a_7kq3nr2v0b1c"));
    }

    #[test]
    fn the_geometry_type_is_read_from_the_features_and_must_be_one() {
        let collection = |geometries: &[&str]| {
            let features: Vec<Value> = geometries
                .iter()
                .map(|kind| json!({"type": "Feature", "geometry": {"type": kind, "coordinates": []}, "properties": {}}))
                .collect();
            serde_json::to_vec(&json!({"type": "FeatureCollection", "features": features})).unwrap()
        };
        assert_eq!(
            geometry_of(&collection(&["Point", "MultiPoint"])).expect("points"),
            GeometryType::Point
        );
        assert_eq!(
            geometry_of(&collection(&["LineString"])).expect("lines"),
            GeometryType::LineString
        );
        assert_eq!(
            geometry_of(&collection(&["Point", "Polygon"]))
                .expect_err("mixed")
                .code(),
            "invalid_payload"
        );
        assert_eq!(
            geometry_of(&collection(&[])).expect_err("empty").code(),
            "invalid_payload"
        );
        assert_eq!(
            geometry_of(b"not json").expect_err("not geojson").code(),
            "invalid_payload"
        );
        assert_eq!(
            geometry_of(br#"{"type":"Feature"}"#)
                .expect_err("not a collection")
                .code(),
            "invalid_payload"
        );
    }

    #[test]
    fn promoting_is_local_and_asks_for_no_confirmation() {
        // A local layer on this host is not governed shared state: it costs
        // nobody else anything, and gating it behind `--yes` would teach a
        // caller to pass `--yes` to a read.
        assert_eq!(COMMAND.effect, Effect::LocalFileWrite);
        assert!(!COMMAND.effect.needs_confirmation());
        assert!(!COMMAND.confirmation_required_for(&parse(&[
            "--asset",
            "a_7kq3nr2v0b1c",
            "--as-layer",
            "Lot3-poles"
        ])));
    }
}
