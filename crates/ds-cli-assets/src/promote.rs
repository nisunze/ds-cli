//! `ds assets promote` — a geo asset, or a geo pack member, as a local layer.
//!
//! Promotion is the one place an asset becomes something the map draws
//! permanently, and it is always explicit and always local (§5, §12.7). The
//! geometry goes to the local-overlay admission path `ds map layer add`
//! already uses: no new store, no upload, no second copy of the bytes. The
//! asset is not touched, and the layer cannot be shared more widely than the
//! class it inherits.
//!
//! A projected `sys:` row is promotable — a transformer version or an
//! attachment is exactly the thing an operator wants on the map — because
//! promoting reads a source and writes nothing back to it.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::{ASSET_ARG, DESCRIPTOR_ARG, MEMBER_ARG};

const AS_LAYER_ARG: Arg =
    Arg::value("as-layer", "<name>", "The local layer's display name.").required();

pub static COMMAND: Command = Command {
    id: "assets.promote",
    path: &["assets", "promote"],
    contract: 1,
    summary: "Promote a geo asset, or a geo pack member, to a local layer.",
    purpose: "\
Hands the asset's geometry to the existing local-overlay admission path behind \
`ds map layer add` — never a new store, never an upload. Promotion is local and \
explicit: the layer becomes orderable, stylable and printable on this device, \
records the source asset as provenance, inherits its sensitivity and cannot be \
shared more widely than it. The asset itself is unchanged. A non-geo asset, a \
feature count over the owning path's cap, or a promotion that would widen \
sensitivity is refused with the reason.",
    chapter: Chapter::Assets,
    effect: Effect::LocalUi,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[ASSET_ARG, MEMBER_ARG, AS_LAYER_ARG, DESCRIPTOR_ARG],
    output: "\
`layer_id` of the created local layer, its `feature_count`, and its `style_ref` \
for `ds style` once it is ready.",
    examples: &[Example {
        command: "ds assets promote --asset a_7kq3nr2v0b1c --member Lot3/gis/poles.shp --as-layer Lot3-poles --output json",
        note: "The layer then appears in `ds map layer list`; the asset is untouched.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        crate::ASSETS_REFUSED,
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::SIGNED_OUT,
        crate::INVALID_ASSET_ID,
        crate::INVALID_LAYER_NAME,
        crate::ASSET_NOT_FOUND,
        crate::ASSET_CLASS_FORBIDDEN,
        crate::ASSET_REQUEST_INVALID,
        crate::ASSET_RULE_REFUSED,
        crate::ASSETS_NOT_IMPLEMENTED,
        crate::ASSETS_SERVICE_FAILED,
        crate::OFFLINE,
        crate::BACKEND_UNREACHABLE,
        crate::ASSET_IS_NOT_A_FILE,
        crate::ASSET_TOO_LARGE,
        crate::ORIGIN_READ_FAILED,
        crate::ORIGIN_UNREACHABLE,
        crate::ORIGIN_READ_UNAVAILABLE,
        crate::INVALID_MEMBER,
        crate::ASSET_NOT_GEOGRAPHIC,
    ],
    reference: Some("docs/reference/assets.md"),
    availability: crate::paired_availability,
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
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::ASSETS_PROMOTE,
        arguments,
        crate::WRITE_TIMEOUT,
    )
    .map_err(crate::classify_assets_failure)
}

pub fn render(data: &Value) -> String {
    format!(
        "local layer {} · {} · style {}\n",
        data["layer_id"].as_str().unwrap_or("?"),
        crate::plural(data["feature_count"].as_u64().unwrap_or(0), "feature"),
        data["style_ref"].as_str().unwrap_or("—"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ds_cli_desktop::ops::undeclared_key;

    fn parse(tokens: &[&str]) -> Inputs {
        let tokens: Vec<String> = tokens.iter().map(|token| (*token).to_string()).collect();
        ds_cli_contract::parse(&COMMAND, &tokens).expect("declared inputs")
    }

    #[test]
    fn a_layer_name_is_one_short_label_and_is_checked_before_the_bridge() {
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
        // classify, attach and folder refuse a `sys:` id; promote, read and
        // preview accept one (§7.1). A transformer version on the map is the
        // whole point of indexing it.
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
        assert_eq!(undeclared_key(&crate::ASSETS_PROMOTE, &with_member), None);
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
            "layer_id": "local:lot3-poles", "feature_count": 412, "style_ref": "sty_7"
        }));
        assert!(rendered.contains("local layer local:lot3-poles"));
        assert!(rendered.contains("412 features"));
        assert!(rendered.contains("style sty_7"));
    }

    #[test]
    fn promoting_is_local_and_asks_for_no_confirmation() {
        // A local overlay on this device is not governed shared state: it
        // costs nobody else anything, and gating it behind `--yes` would
        // teach a caller to pass `--yes` to a read.
        assert_eq!(COMMAND.effect, Effect::LocalUi);
        assert!(!COMMAND.effect.needs_confirmation());
        assert!(!COMMAND.confirmation_required_for(&parse(&[
            "--asset",
            "a_7kq3nr2v0b1c",
            "--as-layer",
            "Lot3-poles"
        ])));
    }
}
