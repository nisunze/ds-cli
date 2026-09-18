//! One route to the governed style documents: the restored native user.
//!
//! A style document is governed shared state behind ds-brain — it has a
//! project, a ref and a publication, and no window is part of any of that.
//! `ds-client-core` reads the catalogue through `get_style_catalog` and
//! publishes through `update_style`, and the transformations in between are
//! `ds_command_kernel::style_plan`, the same module the Style Center runs as
//! WASM. So the answer does not depend on where the command is typed.

use ds_cli_contract::Inputs;
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Refusal};
use serde_json::{Value, json};

pub const LANE_ARG: Arg = Arg::value(
    "lane",
    "<stable|canary>",
    "Deployment lane; stable is the default.",
)
.default("stable")
.choices(&["stable", "canary"]);
pub const TRANSFORMER_ARG: Arg = Arg::value(
    "transformer",
    "<name>",
    "Canonical project data to read this layer's values, counts and field types from. Omit and nothing is observed.",
);

/// The two ways to read the governed style catalogue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Read {
    /// A bounded page of the project's style editor refs.
    List,
    /// One editor's document, field vocabulary and — with `--transformer` —
    /// what the canonical data behind it holds.
    Describe,
}

/// The guided edits, closed. `plan` and `set` are one edit with `apply` false
/// or true, so there are fewer edits than there are commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edit {
    Seed,
    PrintVariant,
    Appearance,
    Label,
    Dimension,
    ClearDimension,
    Cartography,
}

pub const STYLE_REFUSED: Refusal = Refusal {
    code: "style_refused",
    when: "no such style ref, an unknown field or channel, a cartography property this layer type has no place for, or ds-brain declined the document",
    remedy: "check the ref with `ds style list`, and the fields, channels and layer type with `ds style read`",
};
pub const HEADLESS_NO_PROJECT: Refusal = Refusal {
    code: "headless_project_not_selected",
    when: "the user has no audience-fenced selected project",
    remedy: "run ds auth project use --project <exact-id>",
};
pub const PROJECT_CONTEXT_STALE: Refusal = Refusal {
    code: "project_context_stale",
    when: "the saved project belongs to another identity, lane, or audience",
    remedy: "select the project again with ds auth project use",
};
pub const AUTH_CONTEXT_MISMATCH: Refusal = Refusal {
    code: "auth_context_mismatch",
    when: "the protected native providers disagree on identity or selected project",
    remedy: "sign out or revoke the unintended provider before retrying",
};
pub const AUTH_INPUT: Refusal = Refusal {
    code: "auth_input_invalid",
    when: "the selected project identity violates the fixed request bound",
    remedy: "select a freshly visible project again",
};
pub const TRANSFORMER_NOT_FOUND: Refusal = Refusal {
    code: "transformer_not_found",
    when: "the named canonical source does not exist in the selected project",
    remedy: "pass one exact transformer name from that project, or omit --transformer",
};
pub const FIELD_DOMAIN_REFUSED: Refusal = Refusal {
    code: "field_domain_refused",
    when: "the canonical features of the named source violate the field-domain bound",
    remedy: "narrow the source, or omit --transformer and read fieldDomains instead",
};

/// What this domain decides for itself: the project fence it needs on top of a
/// restored user, the two identity disagreements a fenced call can end in, the
/// canonical observation `--transformer` asks for, the backend's own verdict on
/// a document, and the five input grammars parsed here.
const OWN: &[Refusal] = &[
    HEADLESS_NO_PROJECT,
    PROJECT_CONTEXT_STALE,
    AUTH_CONTEXT_MISMATCH,
    AUTH_INPUT,
    TRANSFORMER_NOT_FOUND,
    FIELD_DOMAIN_REFUSED,
    STYLE_REFUSED,
    crate::INVALID_NUMBER,
    crate::INVALID_VALUE_SPEC,
    crate::INVALID_COLOR,
    crate::INVALID_APPEARANCE,
    crate::INVALID_LABEL,
    crate::INVALID_CARTOGRAPHY,
];

/// Every refusal the native user path can return, taken from the owner's own
/// declaration rather than copied. Copying is what let `ds style cartography`
/// document a paired window's seven refusals and none of the fifteen the
/// native route it actually used could raise.
const OWNER: &[Refusal] = ds_cli_auth::PROJECT_LIST_COMMAND.refusals;

const PLANNING_LEN: usize = OWN.len() + OWNER.len();
const fn planning() -> [Refusal; PLANNING_LEN] {
    let mut all = [STYLE_REFUSED; PLANNING_LEN];
    let mut index = 0;
    while index < OWN.len() {
        all[index] = OWN[index];
        index += 1;
    }
    let mut owner = 0;
    while owner < OWNER.len() {
        all[OWN.len() + owner] = OWNER[owner];
        owner += 1;
    }
    all
}
const PLANNING_SET: [Refusal; PLANNING_LEN] = planning();

const PUBLISHING_LEN: usize = PLANNING_LEN + 1;
const fn publishing() -> [Refusal; PUBLISHING_LEN] {
    let mut all = [crate::CONFIRMATION_REQUIRED; PUBLISHING_LEN];
    let planned = planning();
    let mut index = 0;
    while index < PLANNING_LEN {
        all[index + 1] = planned[index];
        index += 1;
    }
    all
}
const PUBLISHING_SET: [Refusal; PUBLISHING_LEN] = publishing();

/// For the commands that publish nothing. `confirmation_required` is absent
/// because a plan has nothing to confirm.
pub const REFUSALS: &[Refusal] = &PLANNING_SET;
/// For the commands that write a style document.
pub const PUBLISH_REFUSALS: &[Refusal] = &PUBLISHING_SET;

/// Read the project's governed style catalogue as the restored native user.
pub fn read(inputs: &Inputs, action: Read, args: Value) -> Result<Value, Failure> {
    let lane = inputs.require("lane")?;
    let snapshot = ds_cli_auth::style_catalog(lane)?;
    let result = match action {
        Read::List => ds_command_kernel::style_plan::list_styles(
            snapshot.result().document(),
            args["query"].as_str(),
            args["limit"].as_u64().unwrap_or(100) as usize,
        ),
        Read::Describe => {
            let reference = inputs.require("ref")?;
            let observed =
                observe_canonical(lane, snapshot.result().document(), reference, inputs)?;
            ds_command_kernel::style_plan::describe_style(
                snapshot.result().document(),
                reference,
                observed.as_ref(),
            )
        }
    };
    result
        .map(|mut data| {
            data["lane"] = json!(snapshot.lane());
            data["warnings"] = snapshot.result().document()["warnings"].clone();
            data
        })
        .map_err(refused)
}

/// Plan or publish one guided edit against the project's style document.
pub fn edit(inputs: &Inputs, action: Edit, args: Value) -> Result<Value, Failure> {
    let lane = inputs.require("lane")?;
    let reference = inputs.require("ref")?;
    let apply = args["apply"].as_bool().unwrap_or(false);
    let instruction = instruction(action, args, inputs)?;
    let receipt = ds_cli_auth::style_edit(lane, reference, &instruction, apply)?;
    let mut data = receipt.result().data().clone();
    data["lane"] = json!(receipt.lane());
    Ok(data)
}

/// The typed instruction one edit carries.
///
/// The kernel's `StyleInstruction` and its `CartographyChange` are both
/// `deny_unknown_fields`, so this is where a key no decoder accepts stops: the
/// closed vocabulary is the kernel's own, and nothing here re-states it.
pub(crate) fn instruction(
    action: Edit,
    args: Value,
    inputs: &Inputs,
) -> Result<ds_cli_auth::StyleInstruction, Failure> {
    Ok(match action {
        Edit::Seed => ds_cli_auth::StyleInstruction::Seed,
        Edit::PrintVariant => ds_cli_auth::StyleInstruction::PrintVariant,
        Edit::Appearance => ds_cli_auth::StyleInstruction::Appearance {
            color: args["color"].as_str().map(str::to_owned),
            icon: args["icon"].as_str().map(str::to_owned),
            size: args["size"].as_f64(),
            icon_overlap: args["icon_overlap"].as_bool(),
        },
        Edit::Label => ds_cli_auth::StyleInstruction::Label {
            field: args["field"].as_str().unwrap_or_default().to_owned(),
            options: if args["options"].is_null() {
                Default::default()
            } else {
                serde_json::from_value(args["options"].clone())
                    .map_err(|_| refused("invalid label options"))?
            },
        },
        Edit::Dimension => ds_cli_auth::StyleInstruction::Dimension {
            field: args["field"].as_str().unwrap_or_default().to_owned(),
            channel: args["channel"].as_str().unwrap_or("halo").to_owned(),
            values: serde_json::from_value(args["values"].clone())
                .map_err(|_| refused("invalid dimension values"))?,
            other: args["other"].as_f64(),
            color: args["color"].as_str().map(str::to_owned),
            field_type: inputs.value("field-type").map(str::to_owned),
            keep_other_channels: inputs.switch("keep-other-channels"),
        },
        Edit::ClearDimension => ds_cli_auth::StyleInstruction::ClearDimension,
        Edit::Cartography => {
            let mut args = args;
            let obj = args
                .as_object_mut()
                .ok_or_else(|| refused("invalid cartography arguments"))?;
            obj.remove("ref");
            obj.remove("apply");
            ds_cli_auth::StyleInstruction::Cartography {
                change: serde_json::from_value(args)
                    .map_err(|_| refused("invalid cartography arguments"))?,
            }
        }
    })
}

fn refused(message: impl Into<String>) -> Failure {
    Failure::invalid("style_refused", message).remedy(STYLE_REFUSED.remedy)
}

/// What this layer's fields carry, read from CANONICAL project data with no
/// map, no browser and no DOM.
///
/// The scalar type of a property is what a MapLibre `match` compares against,
/// and ds-brain publishes one only for fields carrying a known-column domain.
/// The rest used to be recovered in the browser from
/// `map.queryRenderedFeatures` — the features that happened to be painted —
/// so `ds` could never answer at all and `style read` reported `onMap: null`.
///
/// `--transformer` names the canonical source: one exact transformer in the
/// selected project, fetched through the same fixed gateway call
/// `ds design features select` uses. Without it nothing is observed, and the
/// kernel says so by name rather than reporting a confidently empty answer.
fn observe_canonical(
    lane: &str,
    snapshot: &Value,
    reference: &str,
    inputs: &Inputs,
) -> Result<Option<Value>, Failure> {
    let Some(transformer) = inputs.value("transformer") else {
        return Ok(None);
    };
    let editor = snapshot["style_editors"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|editor| editor["style_ref"] == reference)
        .ok_or_else(|| refused("style editor is missing"))?;
    let layer_name = editor["layer_name"].as_str().unwrap_or_default();
    let context = ds_cli_auth::transformer_context(lane, transformer)?;
    let features = canonical_features(context.snapshot().layers(), layer_name);
    let request = json!({
        "schema": ds_command_kernel::field_domain::REQUEST_SCHEMA,
        "operation": "observe",
        "layer": reference,
        "canonical": {"kind": "local", "source": "design_room"},
        "features": features,
        "declared": editor["field_domains"],
        "present": editor["present_values"],
        "published": editor["field_values"],
        "fields": editor["available_fields"],
    });
    let encoded = serde_json::to_vec(&request)
        .map_err(|_| field_domain_refused("request is not encodable"))?;
    ds_command_kernel::field_domain::evaluate(&encoded)
        .map(Some)
        .map_err(field_domain_refused)
}

fn field_domain_refused(message: impl Into<String>) -> Failure {
    Failure::invalid(FIELD_DOMAIN_REFUSED.code, message).remedy(FIELD_DOMAIN_REFUSED.remedy)
}

/// The transformer room's features for one design layer, as property bags.
///
/// Geometry never travels: the question is about properties, so shipping
/// coordinates through the kernel would be pure cost. A design style ref and a
/// room key agree up to the `_vt` suffix and case, the same normalisation the
/// Style Center applies.
fn canonical_features(
    layers: &std::collections::BTreeMap<String, Value>,
    layer_name: &str,
) -> Vec<Value> {
    let wanted = normalized_layer(layer_name);
    if wanted.is_empty() {
        return Vec::new();
    }
    let mut features = Vec::new();
    for (key, collection) in layers {
        if normalized_layer(key) != wanted {
            continue;
        }
        for feature in collection["features"].as_array().into_iter().flatten() {
            if features.len() >= ds_command_kernel::field_domain::MAX_OBSERVED_FEATURES {
                return features;
            }
            features.push(json!({"properties": feature["properties"]}));
        }
    }
    features
}

fn normalized_layer(value: &str) -> String {
    value
        .trim()
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .trim_end_matches("_vt")
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use ds_cli_contract::parse;

    use super::*;

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| (*part).to_string()).collect()
    }

    /// Every edit, with every flag it declares set, must reach a typed
    /// instruction — the decoder being `deny_unknown_fields` on both the
    /// instruction and its flattened cartography change is what makes that a
    /// real bound and not a shape check. The handlers' own argument builders
    /// are the input, so a key a handler invents but no variant names is red
    /// here rather than at the gateway.
    #[test]
    fn every_edit_builds_only_keys_the_kernel_decoder_accepts() {
        let seeded =
            parse(&crate::seed::create::COMMAND, &argv(&["--ref", "pc/c"])).expect("seed inputs");
        for (edit, built) in [
            (
                Edit::Seed,
                json!({"ref": "print_context/contours_index", "apply": true}),
            ),
            (
                Edit::PrintVariant,
                json!({"ref": "master/lv_lines", "apply": true}),
            ),
            (
                Edit::ClearDimension,
                json!({"ref": "master/lv_poles", "apply": true}),
            ),
        ] {
            let instruction = instruction(edit, built, &seeded).expect("typed instruction");
            serde_json::to_value(&instruction).expect("the kernel encodes its own instruction");
        }

        let appearance = parse(
            &crate::appearance::set::COMMAND,
            &argv(&[
                "--ref",
                "master/lv_poles",
                "--color",
                "#00ff00",
                "--icon",
                "pole",
                "--size",
                "4",
                "--icon-overlap",
                "on",
            ]),
        )
        .expect("appearance inputs");
        let built = crate::appearance::arguments(&appearance, true).expect("appearance arguments");
        let encoded = serde_json::to_value(
            instruction(Edit::Appearance, built, &appearance).expect("appearance instruction"),
        )
        .expect("encoded");
        assert_eq!(
            encoded,
            json!({"kind":"appearance","color":"#00FF00","icon":"pole","size":4.0,"icon_overlap":true}),
            "every appearance flag must reach the closed instruction"
        );

        let label = parse(
            &crate::label::set::COMMAND,
            &argv(&[
                "--ref",
                "gt/roads_print",
                "--field",
                "road_no",
                "--size",
                "12",
            ]),
        )
        .expect("label inputs");
        let built = crate::label::arguments(&label, true).expect("label arguments");
        let encoded = serde_json::to_value(
            instruction(Edit::Label, built, &label).expect("label instruction"),
        )
        .expect("encoded");
        assert_eq!(encoded["kind"], "label");
        assert_eq!(encoded["field"], "road_no");
        assert_eq!(encoded["options"]["size"], json!(12.0));

        let dimension = parse(
            &crate::dimension::set::COMMAND,
            &argv(&[
                "--ref",
                "master/lv_poles",
                "--field",
                "drafting_status",
                "--field-type",
                "string",
                "--channel",
                "halo",
                "--keep-other-channels",
                "--value",
                "draft=3:#ffffff",
                "--other",
                "0",
                "--color",
                "#112233",
            ]),
        )
        .expect("dimension inputs");
        let built = crate::dimension::arguments(&dimension, true).expect("dimension arguments");
        let encoded = serde_json::to_value(
            instruction(Edit::Dimension, built, &dimension).expect("dimension instruction"),
        )
        .expect("encoded");
        assert_eq!(
            encoded,
            json!({
                "kind": "dimension",
                "field": "drafting_status",
                "channel": "halo",
                "values": [{"value": "draft", "amount": 3.0, "color": "#FFFFFF"}],
                "other": 0.0,
                "color": "#112233",
                "field_type": "string",
                "keep_other_channels": true,
            }),
            "--field-type and --keep-other-channels are read from the inputs, not the args, \
             so they are the two keys a builder-only check would miss"
        );

        let cartography = parse(
            &crate::cartography::set::COMMAND,
            &argv(&[
                "--ref",
                "master/water_mains",
                "--line-type",
                "directional",
                "--direction-size",
                "14",
                "--direction-spacing",
                "140",
                "--casing-color",
                "#0f172a",
                "--casing-width",
                "2.5",
            ]),
        )
        .expect("cartography inputs");
        let built =
            crate::cartography::arguments(&cartography, true).expect("cartography arguments");
        let encoded = serde_json::to_value(
            instruction(Edit::Cartography, built, &cartography).expect("cartography instruction"),
        )
        .expect("encoded");
        assert_eq!(encoded["kind"], "cartography");
        assert_eq!(encoded["lineType"], "directional");
        assert_eq!(encoded["casingWidth"], json!(2.5));
        assert!(
            encoded.get("ref").is_none() && encoded.get("apply").is_none(),
            "the addressing keys are the call's, never the change's"
        );
    }

    /// The flattened cartography change is `deny_unknown_fields`, so a key the
    /// kernel does not name is refused here — in `ds`, with this domain's
    /// code — rather than being posted to ds-brain and returned as prose.
    #[test]
    fn a_cartography_key_the_kernel_does_not_name_is_refused_locally() {
        let inputs = parse(
            &crate::cartography::set::COMMAND,
            &argv(&["--ref", "master/water_mains", "--casing-width", "2"]),
        )
        .expect("cartography inputs");
        let failure = instruction(
            Edit::Cartography,
            json!({"ref": "master/water_mains", "apply": true, "glowRadius": 4}),
            &inputs,
        )
        .expect_err("an unnamed property must not travel");
        assert_eq!(failure.code(), "style_refused");
    }

    /// Composition, not a copy: the owner's list arrives whole, this domain's
    /// own arrives whole, and only a publishing command documents the
    /// confirmation it is the only one that can require.
    #[test]
    fn the_refusals_are_composed_from_the_native_owner_and_this_domain() {
        let planning: BTreeSet<&str> = REFUSALS.iter().map(|refusal| refusal.code).collect();
        for owner in OWNER {
            assert!(
                planning.contains(owner.code),
                "`{}` is a refusal the native user path can return and this domain drops it",
                owner.code
            );
        }
        for own in OWN {
            assert!(planning.contains(own.code), "`{}` was dropped", own.code);
        }
        assert!(
            !planning.contains("confirmation_required"),
            "a plan publishes nothing and has nothing to confirm"
        );
        let publishing: BTreeSet<&str> = PUBLISH_REFUSALS
            .iter()
            .map(|refusal| refusal.code)
            .collect();
        assert!(publishing.contains("confirmation_required"));
        assert_eq!(
            publishing.len(),
            planning.len() + 1,
            "publishing adds the confirmation and nothing else"
        );
        for code in [
            "desktop_not_paired",
            "desktop_refused",
            "desktop_signed_out",
        ] {
            assert!(
                !publishing.contains(code),
                "`{code}` belongs to a window this domain no longer opens"
            );
        }
    }
}
