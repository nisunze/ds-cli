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
pub const PROJECT_ARG: Arg = Arg::value(
    "project",
    "<exact-id>",
    "Exact ds_project whose style documents this call reads or writes; the saved selection is never read.",
)
.required();
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
    Preset,
    Instruction,
    Categorical,
    ColorRange,
    Zoom,
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
pub const STYLE_EXISTS: Refusal = Refusal {
    code: "style_exists",
    when: "the create-only style (a `_print` variant, a seeded print-context source) already exists; a create never overwrites",
    remedy: "read it with `ds style read --ref <ref>`, then customise it with the appearance, label, dimension or cartography commands",
};
pub const STYLE_NOT_PERMITTED: Refusal = Refusal {
    code: "style_not_permitted",
    when: "ds-brain refused this account the style write, which needs the `styles.edit` capability (and --project membership)",
    remedy: "ask a platform admin for a role granting `styles.edit`; check --project with `ds auth project list`",
};
pub const PROJECT_INVALID: Refusal = Refusal {
    code: "context_corrupt",
    when: "--project is not one exact DS project id: blank, untrimmed, too long, or a path",
    remedy: "copy one exact ds_project value from ds auth project list",
};
pub const AUTH_CONTEXT_MISMATCH: Refusal = Refusal {
    code: "auth_context_mismatch",
    when: "the protected native providers disagree on identity",
    remedy: "sign out or revoke the unintended provider before retrying",
};
pub const AUTH_INPUT: Refusal = Refusal {
    code: "auth_input_invalid",
    when: "the named project identity violates the fixed request bound",
    remedy: "pass one exact ds_project value from ds auth project list",
};
pub const TRANSFORMER_NOT_FOUND: Refusal = Refusal {
    code: "transformer_not_found",
    when: "the named canonical source does not exist in the named project",
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
/// a document, a create-only style that already exists (a plan sees it in the
/// catalogue, a create meets ds-brain's 409), and the five input grammars
/// parsed here.
const OWN: &[Refusal] = &[
    PROJECT_INVALID,
    AUTH_CONTEXT_MISMATCH,
    AUTH_INPUT,
    TRANSFORMER_NOT_FOUND,
    FIELD_DOMAIN_REFUSED,
    STYLE_REFUSED,
    STYLE_EXISTS,
    Refusal {
        code: "style_digest_conflict",
        when: "the authored style storage digest changed since review",
        remedy: "read the current document and contentSha256, then review the plan again",
    },
    Refusal {
        code: "style_head_conflict",
        when: "the print style head changed since the reviewed restore",
        remedy: "list current revisions and compare before restoring against the new expected head",
    },
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

/// What only a write can meet: the confirmation it requires, and ds-brain
/// refusing the account the write itself.
const WRITE_ONLY: &[Refusal] = &[crate::CONFIRMATION_REQUIRED, STYLE_NOT_PERMITTED];

const PUBLISHING_LEN: usize = PLANNING_LEN + WRITE_ONLY.len();
const fn publishing() -> [Refusal; PUBLISHING_LEN] {
    let mut all = [crate::CONFIRMATION_REQUIRED; PUBLISHING_LEN];
    let mut index = 0;
    while index < WRITE_ONLY.len() {
        all[index] = WRITE_ONLY[index];
        index += 1;
    }
    let planned = planning();
    let mut index = 0;
    while index < PLANNING_LEN {
        all[WRITE_ONLY.len() + index] = planned[index];
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
    let project = inputs.require("project")?;
    let snapshot = ds_cli_auth::style_catalog(lane, project)?;
    let result = match action {
        Read::List => ds_command_kernel::style_plan::list_styles(
            snapshot.result().document(),
            args["query"].as_str(),
            args["limit"].as_u64().unwrap_or(100) as usize,
        ),
        Read::Describe => {
            let reference = inputs.require("ref")?;
            let observed = observe_canonical(
                lane,
                project,
                snapshot.result().document(),
                reference,
                inputs,
            )?;
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
    let project = inputs.require("project")?;
    let reference = inputs.require("ref")?;
    let apply = args["apply"].as_bool().unwrap_or(false);
    let instruction = instruction(action, args, inputs)?;
    // The ref a create-only edit would publish: the one to read when it exists.
    let published = match action {
        Edit::PrintVariant => format!("{reference}_print"),
        _ => reference.to_owned(),
    };
    let receipt = ds_cli_auth::style_edit(lane, project, reference, &instruction, apply).map_err(
        |failure| {
            let failure = named(failure);
            if failure.code() == STYLE_EXISTS.code {
                failure.next(format!(
                    "ds style read --project {project} --ref {published} --lane {lane}"
                ))
            } else {
                failure
            }
        },
    )?;
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
        Edit::Instruction => serde_json::from_value(args["instruction"].clone())
            .map_err(|e| refused(format!("invalid instruction: {e}")))?,
        Edit::Seed => ds_cli_auth::StyleInstruction::Seed,
        Edit::PrintVariant => ds_cli_auth::StyleInstruction::PrintVariant,
        Edit::Appearance => ds_cli_auth::StyleInstruction::Appearance {
            color: args["color"].as_str().map(str::to_owned),
            icon: args["icon"].as_str().map(str::to_owned),
            size: args["size"].as_f64(),
            icon_overlap: args["icon_overlap"].as_bool(),
            halo_color: args["halo_color"].as_str().map(str::to_owned),
            halo_width: args["halo_width"].as_f64(),
        },
        Edit::Preset | Edit::Categorical | Edit::ColorRange | Edit::Zoom => {
            let mut args = args;
            let object = args
                .as_object_mut()
                .ok_or_else(|| refused("invalid style instruction"))?;
            object.remove("ref");
            object.remove("apply");
            object.insert(
                "kind".into(),
                json!(match action {
                    Edit::Preset => "preset",
                    Edit::Categorical => "categorical",
                    Edit::ColorRange => "color_range",
                    _ => "zoom",
                }),
            );
            serde_json::from_value(args)
                .map_err(|e| refused(format!("invalid instruction: {e}")))?
        }
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

/// The style route's refusals, named by this domain.
///
/// `ds-cli-auth` hands a governed refusal on with the route's `http_status`,
/// `service_code` and `service_message` in `detail`, under the shared kind
/// mapping's code — `auth_input_invalid` for a refused document, whose
/// declared remedy is about the project id. That is how `ds style print
/// create --ref gt/rivers` answered "pass one exact ds_project value" on
/// 2026-09-27 when `gt/rivers_print` already existed. The kernel names who
/// refused: `style_exists` (the catalogue already holds the create-only ref,
/// status 0, decided before any request) or ds-brain's 409; `style_refused`
/// (the planner's rule, status 0) or ds-brain's 400/422 with its issues; a
/// 401/403 is the write capability. The match is on the status and code,
/// never on prose; anything else keeps the shared mapping.
pub(crate) fn named(failure: Failure) -> Failure {
    let Some(detail) = failure.detail_value() else {
        return failure;
    };
    let status = detail["http_status"].as_u64();
    let code = detail["service_code"].as_str();
    let sentence = detail["service_message"]
        .as_str()
        .unwrap_or(failure.message())
        .to_owned();
    let kept = detail.clone();
    match (status, code) {
        (Some(0 | 409), Some("style_digest_conflict")) => {
            Failure::invalid("style_digest_conflict", sentence)
                .remedy(
                    "read the current style and contentSha256, review, then replay with its digest",
                )
                .detail(kept)
        }
        (Some(409), Some("style_head_conflict")) => Failure::invalid(
            "style_head_conflict",
            sentence,
        )
        .remedy("read the current version list and compare before restoring against the new head")
        .detail(kept),
        (Some(0), Some("style_exists")) | (Some(409), _) => {
            Failure::invalid(STYLE_EXISTS.code, sentence)
                .remedy(STYLE_EXISTS.remedy)
                .detail(kept)
        }
        (Some(401 | 403), _) => Failure::unauthorized(STYLE_NOT_PERMITTED.code, sentence)
            .remedy(STYLE_NOT_PERMITTED.remedy)
            .detail(kept),
        (Some(0), Some("style_refused")) | (Some(400 | 404 | 422), _) => {
            Failure::invalid(STYLE_REFUSED.code, sentence)
                .remedy(STYLE_REFUSED.remedy)
                .detail(kept)
        }
        _ => failure,
    }
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
/// project named by `--project`, fetched through the same fixed gateway call
/// `ds design features select` uses. Without it nothing is observed, and the
/// kernel says so by name rather than reporting a confidently empty answer.
fn observe_canonical(
    lane: &str,
    project: &str,
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
    let context = ds_cli_auth::transformer_context_for_project(lane, project, transformer)?;
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

    /// Every style call names its project; the saved selection is never read.
    fn parse(
        command: &ds_cli_contract::spec::Command,
        tokens: &[String],
    ) -> Result<ds_cli_contract::Inputs, ds_cli_contract::outcome::Failure> {
        let mut all = vec!["--project".to_owned(), "test-project".to_owned()];
        all.extend_from_slice(tokens);
        ds_cli_contract::parse(command, &all)
    }

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

    /// What `ds-cli-auth` hands this domain for one governed refusal: the
    /// shared kind mapping's code and sentence, with the route's status, code
    /// and words in `detail`.
    fn handed(shared: Failure, status: u16, service_code: &str, sentence: &str) -> Failure {
        let message = format!("{} (HTTP {status}): {sentence}", shared.message());
        shared.with_message(message).detail(json!({
            "http_status": status,
            "service_code": service_code,
            "service_message": sentence,
        }))
    }
    fn input() -> Failure {
        Failure::invalid("auth_input_invalid", "owner")
    }

    /// `ds style print create --ref gt/rivers` on 2026-09-27 answered
    /// `auth_input_invalid` "pass one exact ds_project value" because
    /// `gt/rivers_print` already existed. Each style refusal now reads as the
    /// rule that refused it, with the route's own sentence.
    #[test]
    fn a_style_refusal_is_named_for_the_rule_that_refused_it() {
        let exists = "gt/rivers_print already exists; a print variant is created once and never overwritten, so customise gt/rivers_print itself";
        for failure in [
            handed(input(), 0, "style_exists", exists),
            handed(
                input(),
                409,
                "conflict",
                "Print style already exists; edit its _print style instead",
            ),
        ] {
            let sentence = failure.detail_value().unwrap()["service_message"].clone();
            let named = named(failure);
            assert_eq!(named.code(), STYLE_EXISTS.code);
            // Not `conflict`: nothing raced, and an unchanged retry can never
            // succeed, so it must not read as retryable.
            assert_eq!(
                named.class(),
                ds_cli_contract::outcome::ExitClass::InvalidInput
            );
            assert_eq!(named.message(), sentence.as_str().unwrap());
            assert_eq!(named.remedy_text(), Some(STYLE_EXISTS.remedy));
            assert!(!named.remedy_text().unwrap().contains("ds_project"));
        }

        let planner = named(handed(
            input(),
            0,
            "style_refused",
            "icon is not in the published catalog",
        ));
        assert_eq!(planner.code(), STYLE_REFUSED.code);
        assert_eq!(planner.message(), "icon is not in the published catalog");

        let document = named(handed(
            input(),
            400,
            "validation_failed",
            "Invalid style document: paint.line-glow: unknown paint property",
        ));
        assert_eq!(document.code(), STYLE_REFUSED.code);
        assert!(document.message().contains("paint.line-glow"));

        let forbidden = named(handed(
            Failure::unauthorized("auth_rejected", "owner"),
            403,
            "insufficient_permissions",
            "Permission denied",
        ));
        assert_eq!(forbidden.code(), STYLE_NOT_PERMITTED.code);
        assert!(forbidden.remedy_text().unwrap().contains("styles.edit"));

        // Negative controls: a refusal this domain does not own keeps the
        // shared mapping untouched — a bare class with no route words, and a
        // kernel receipt refusal that is not a style rule.
        let bare = Failure::invalid("auth_input_invalid", "invalid style reference");
        let kept = named(bare.clone());
        assert_eq!(kept.code(), "auth_input_invalid");
        assert_eq!(kept.message(), bare.message());
        let receipt = named(handed(
            Failure::unavailable("auth_response_unreadable", "owner"),
            0,
            "contract_violation",
            "style publish receipt does not match the authored target",
        ));
        assert_eq!(receipt.code(), "auth_response_unreadable");
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
        assert!(publishing.contains("style_not_permitted"));
        assert!(
            planning.contains("style_exists"),
            "a print plan sees an existing variant in the catalogue"
        );
        assert!(
            !planning.contains("style_not_permitted"),
            "a plan writes nothing, so the write capability never refuses it"
        );
        assert_eq!(
            publishing.len(),
            planning.len() + 2,
            "publishing adds the confirmation and the write capability, nothing else"
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
