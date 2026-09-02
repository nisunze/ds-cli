//! `ds map design set` — stage a property change on selected design features.
//!
//! This is the command the family exists for. Marking an as-built network
//! approved — `drafting_status=approved` — is what stops the kernel from
//! redesigning an installed transformer, and nothing in the application could
//! write it in bulk, so as-built rows stayed unmarked and every process run
//! was free to resize them.
//!
//! It stages. The change lands in the operator's local room and marks it
//! dirty; the project is untouched until `ds map design save`. `staged` and
//! `persisted` are both reported on every call so the two can never be
//! confused, and `--dry-run` answers "how many would change?" without writing
//! even locally.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::DESCRIPTOR_ARG;
use crate::design::{BBOX_ARG, ID_ARG, LAYER_ARG, TRANSFORMER_ARG, WHERE_ARG};

const SET_ARG: Arg = Arg {
    name: "set",
    kind: ArgKind::Repeated,
    value: "<key=value|key:=scalar>",
    required: true,
    default: None,
    choices: &[],
    summary: "Literal string as key=value; typed JSON scalar as key:=value. Repeat.",
};

const PROTECTED_PROPERTY: Refusal = Refusal {
    code: "protected_property",
    when: "a --set assignment targets an immutable design identity property",
    remedy: "write an ordinary mutable property; identity rewrite requires a separate confirmation-gated command",
};

const INVALID_PROPERTY_VALUE: Refusal = Refusal {
    code: "invalid_property_value",
    when: "a typed key:=value is not a JSON scalar, or is an array or object",
    remedy: "use key=value for a literal string, or key:=null|true|false|number for a typed scalar",
};

const DUPLICATE_PROPERTY: Refusal = Refusal {
    code: "duplicate_property",
    when: "the same property is assigned more than once",
    remedy: "pass each property exactly once so one value has one unambiguous type",
};

pub static COMMAND: Command = Command {
    id: "map.design.set",
    path: &["map", "design", "set"],
    contract: 3,
    summary: "Stage a property change on selected design features.",
    purpose: "\
Writes properties onto every design feature a selector matches — the bulk edit \
the application has no other way to make, and the one that marks an as-built \
network approved so the kernel stops redesigning it. It stages into the \
operator's local room and marks it dirty; nothing reaches the project until \
`ds map design save`. Immutable identity properties refuse before dry-run or \
staging. Literal strings use key=value; null, booleans and numbers use the \
unambiguous key:=JSON-scalar form. Use --dry-run to count first.",
    chapter: Chapter::Design,
    effect: Effect::LocalUi,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[
        TRANSFORMER_ARG,
        SET_ARG,
        LAYER_ARG,
        WHERE_ARG,
        BBOX_ARG,
        ID_ARG,
        Arg::switch("dry-run", "Report what would change; stage nothing."),
        DESCRIPTOR_ARG,
    ],
    output: "\
How many features matched and changed, the change count per layer, requested \
values, and exactly one of applied values or would-apply values. Scalar JSON \
types are preserved; `staged` and `persisted` remain separate.",
    examples: &[
        Example {
            command: "ds map design set --transformer T-1042 --layer lv_lines --where drafting_status= --set drafting_status=approved --dry-run",
            note: "Count the unmarked as-built rows before touching them.",
            runnable: false,
        },
        Example {
            command: "ds map design set --transformer T-1042 --layer lv_poles --set energized:=true --set phase_count:=3 --dry-run",
            note: "Preview typed boolean and integer values; no value is claimed as applied.",
            runnable: false,
        },
        Example {
            command: "ds map design set --transformer T-1042 --layer lv_lines --where drafting_status= --set drafting_status=approved --output json",
            note: "Stage it. Read .data.staged; the project is still untouched.",
            runnable: false,
        },
    ],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        super::DESIGN_REFUSED,
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::SIGNED_OUT,
        crate::INVALID_PAIR,
        crate::INVALID_BBOX,
        super::TOO_MANY_IDS,
        PROTECTED_PROPERTY,
        INVALID_PROPERTY_VALUE,
        DUPLICATE_PROPERTY,
        Refusal {
            code: "no_properties",
            when: "every --set was parsed away, leaving nothing to write",
            remedy: "pass at least one --set name=value",
        },
    ],
    reference: Some("docs/reference/map.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let transformer = inputs.require("transformer")?;
    let dry_run = inputs.switch("dry-run");

    let properties = property_values(inputs.repeated("set"))?;
    if properties.is_empty() {
        return Err(
            Failure::invalid("no_properties", "no property was given to write")
                .remedy("pass at least one --set name=value"),
        );
    }

    let selector = super::selector(inputs, "")?;
    let mut arguments = Map::new();
    arguments.insert("transformer".into(), json!(transformer));
    arguments.insert("properties".into(), Value::Object(properties.clone()));
    arguments.insert("dryRun".into(), json!(dry_run));

    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    let result = crate::invoke(
        &descriptor,
        &crate::DESIGN_SET,
        super::with_selector(arguments, selector.clone()),
        crate::DESIGN_STAGE_TIMEOUT,
    )
    .map_err(classify_set_failure)?;

    receipt(transformer, selector, properties, dry_run, &result)
}

fn property_values(raw: &[String]) -> Result<Map<String, Value>, Failure> {
    let mut properties = Map::new();
    for assignment in raw {
        let Some((raw_key, raw_value)) = assignment.split_once('=') else {
            return Err(Failure::invalid(
                "invalid_pair",
                format!("`--set {assignment}` has no `=`"),
            )
            .remedy(
                "write a literal string as --set name=value or a typed scalar as --set name:=value",
            ));
        };
        let (raw_key, typed) = raw_key
            .strip_suffix(':')
            .map_or((raw_key, false), |key| (key, true));
        let key = raw_key.trim();
        if key.is_empty() {
            return Err(Failure::invalid(
                "invalid_pair",
                format!("`--set {assignment}` has an empty name"),
            )
            .remedy("write it as --set name=value"));
        }
        if properties.contains_key(key) {
            return Err(Failure::invalid(
                "duplicate_property",
                format!("`{key}` was assigned more than once"),
            )
            .remedy(DUPLICATE_PROPERTY.remedy)
            .detail(json!({ "property": key })));
        }
        let value = if typed {
            let parsed: Value = serde_json::from_str(raw_value).map_err(|_| {
                Failure::invalid(
                    "invalid_property_value",
                    format!("`{key}` is not a valid typed JSON scalar"),
                )
                .remedy(INVALID_PROPERTY_VALUE.remedy)
                .detail(json!({ "property": key }))
            })?;
            if parsed.is_array() || parsed.is_object() {
                return Err(Failure::invalid(
                    "invalid_property_value",
                    format!("`{key}` is an array or object, not a scalar"),
                )
                .remedy(INVALID_PROPERTY_VALUE.remedy)
                .detail(json!({ "property": key })));
            }
            parsed
        } else if raw_value.is_empty() {
            Value::Null
        } else {
            Value::String(raw_value.to_string())
        };
        properties.insert(key.to_string(), value);
    }
    Ok(properties)
}

fn receipt(
    transformer: &str,
    selector: Map<String, Value>,
    requested_values: Map<String, Value>,
    dry_run: bool,
    result: &Value,
) -> Result<Value, Failure> {
    let result_dry_run = result["dryRun"]
        .as_bool()
        .ok_or_else(|| unreadable_reply("the application omitted `dryRun`"))?;
    if result_dry_run != dry_run {
        return Err(unreadable_reply(
            "the application's dry-run receipt does not match the requested mode",
        ));
    }
    let effective_value_features = result["effectiveValueFeatures"]
        .as_u64()
        .ok_or_else(|| unreadable_reply("the application omitted `effectiveValueFeatures`"))?;
    let matched = result["matched"]
        .as_u64()
        .ok_or_else(|| unreadable_reply("the application omitted `matched`"))?;
    if effective_value_features != matched {
        return Err(unreadable_reply(
            "the application's applied-value projection does not cover every matched feature",
        ));
    }
    let applied_values =
        property_receipt_value(result, "appliedValues", &requested_values, matched)?;
    let would_apply_values =
        property_receipt_value(result, "wouldApplyValues", &requested_values, matched)?;
    let truthful_mode = if dry_run {
        applied_values.is_null() && would_apply_values.is_object()
    } else {
        applied_values.is_object() && would_apply_values.is_null()
    };
    if !truthful_mode {
        return Err(unreadable_reply(
            "the application confused applied and would-apply property values",
        ));
    }

    Ok(json!({
        "transformer": transformer,
        "project": result["project"],
        "selector": super::describe(&selector),
        "requested_values": Value::Object(requested_values),
        "applied_values": applied_values,
        "would_apply_values": would_apply_values,
        "effective_value_features": effective_value_features,
        "dry_run": result_dry_run,
        "matched": matched,
        "changed": result["changed"].as_u64().unwrap_or(0),
        "unchanged": result["unchanged"].as_u64().unwrap_or(0),
        "changed_by_layer": result["changedByLayer"],
        "staged": result["staged"].as_bool().unwrap_or(false),
        "persisted": result["persisted"].as_bool().unwrap_or(false),
    }))
}

fn property_receipt_value(
    result: &Value,
    key: &str,
    requested_values: &Map<String, Value>,
    matched: u64,
) -> Result<Value, Failure> {
    match result.get(key) {
        Some(Value::Null) => Ok(Value::Null),
        Some(Value::Object(values)) => {
            let exact_keys = if matched == 0 {
                values.is_empty()
            } else {
                values.len() == requested_values.len()
                    && requested_values
                        .keys()
                        .all(|property| values.contains_key(property))
            };
            if !exact_keys {
                return Err(unreadable_reply(format!(
                    "the application returned incomplete or extra `{key}` properties"
                )));
            }
            if values
                .values()
                .any(|value| value.is_array() || value.is_object())
            {
                return Err(unreadable_reply(format!(
                    "the application returned a non-scalar `{key}` property"
                )));
            }
            Ok(Value::Object(values.clone()))
        }
        _ => Err(unreadable_reply(format!(
            "the application omitted or malformed `{key}`"
        ))),
    }
}

fn unreadable_reply(message: impl Into<String>) -> Failure {
    Failure::unavailable("desktop_unreadable", message)
        .remedy("restart DS GridDesign and retry with the matching CLI release")
}

fn classify_set_failure(failure: Failure) -> Failure {
    let failure = crate::classify_design_failure(failure);
    if failure.code() != "desktop_refused" {
        return failure;
    }
    let detail = failure
        .detail_value()
        .and_then(|value| value["detail"].as_str())
        .unwrap_or_default();
    let Some(rest) = detail.strip_prefix("protected_property:") else {
        let Some(rest) = detail.strip_prefix("invalid_property_value:") else {
            return failure;
        };
        let Some((property, _message)) = rest.split_once(':') else {
            return failure;
        };
        if property.is_empty() {
            return failure;
        }
        return Failure::invalid(
            "invalid_property_value",
            format!("`{property}` is not a supported scalar property value"),
        )
        .remedy(INVALID_PROPERTY_VALUE.remedy)
        .detail(json!({ "property": property }));
    };
    let Some((property, _message)) = rest.split_once(':') else {
        return failure;
    };
    if property.is_empty() {
        return failure;
    }
    Failure::invalid(
        "protected_property",
        format!("`{property}` is an immutable design identity property"),
    )
    .remedy(PROTECTED_PROPERTY.remedy)
    .detail(json!({ "property": property }))
}

pub fn render(data: &Value) -> String {
    let changed = data["changed"].as_u64().unwrap_or(0);
    let mut out = format!(
        "{} matched  ·  {} would change\n  selector  {}\n",
        data["matched"],
        changed,
        data["selector"].as_str().unwrap_or(""),
    );
    if let Some(unchanged) = data["unchanged"].as_u64().filter(|count| *count > 0) {
        out.push_str(&format!("  {unchanged} already carry the value\n"));
    }
    if let Some(layers) = data["changed_by_layer"].as_object() {
        for (layer, count) in layers {
            out.push_str(&format!("  {layer:<28} {count:>7}\n"));
        }
    }
    if data["dry_run"].as_bool().unwrap_or(false) {
        out.push_str("\ndry run; nothing was staged\n");
        return out;
    }
    out.push('\n');
    out.push_str(super::staging_note(data));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protected_property_bridge_refusal_keeps_the_exact_key() {
        let failure = Failure::failed("desktop_refused", "the paired session refused the write")
            .detail(json!({
                "detail": "protected_property:source_feature_id: design identity properties are immutable"
            }));

        let classified = classify_set_failure(failure);
        assert_eq!(classified.code(), "protected_property");
        assert_eq!(
            classified.detail_value(),
            Some(&json!({ "property": "source_feature_id" }))
        );
        assert_eq!(classified.remedy_text(), Some(PROTECTED_PROPERTY.remedy));
    }

    #[test]
    fn invalid_scalar_bridge_refusal_keeps_the_exact_key() {
        let failure = Failure::failed("desktop_refused", "the paired session refused the write")
            .detail(json!({
                "detail": "invalid_property_value:metadata: design set accepts only finite JSON scalar values"
            }));

        let classified = classify_set_failure(failure);
        assert_eq!(classified.code(), "invalid_property_value");
        assert_eq!(
            classified.detail_value(),
            Some(&json!({ "property": "metadata" }))
        );
    }

    #[test]
    fn unrelated_bridge_refusal_stays_generic() {
        let failure = Failure::failed("desktop_refused", "the paired session refused the write")
            .detail(json!({ "detail": "transformer not found" }));

        assert_eq!(classify_set_failure(failure).code(), "desktop_refused");
    }

    #[test]
    fn property_values_keep_literal_strings_and_typed_json_scalars_distinct() {
        let values = property_values(&[
            "label=true".into(),
            "enabled:=true".into(),
            "phase_count:=4".into(),
            "voltage:=230.5".into(),
            "removed:=null".into(),
            "empty=".into(),
            r#"quoted:="true""#.into(),
        ])
        .expect("valid scalar assignments");

        assert_eq!(values["label"], json!("true"));
        assert_eq!(values["enabled"], json!(true));
        assert_eq!(values["phase_count"], json!(4));
        assert_eq!(values["voltage"], json!(230.5));
        assert_eq!(values["removed"], Value::Null);
        assert_eq!(values["empty"], Value::Null);
        assert_eq!(values["quoted"], json!("true"));
    }

    #[test]
    fn property_values_refuse_composites_and_duplicate_assignments() {
        for assignment in ["tags:=[1]", "metadata:={\"a\":1}", "enabled:=truthy"] {
            assert_eq!(
                property_values(&[assignment.into()])
                    .expect_err("not a typed scalar")
                    .code(),
                "invalid_property_value"
            );
        }
        assert_eq!(
            property_values(&["enabled=true".into(), "enabled:=true".into()])
                .expect_err("duplicate property")
                .code(),
            "duplicate_property"
        );
    }

    #[test]
    fn receipt_uses_application_values_and_keeps_dry_run_hypothetical() {
        let result = json!({
            "project": "project-1",
            "dryRun": true,
            "matched": 2,
            "changed": 2,
            "unchanged": 0,
            "changedByLayer": { "lv_poles": 2 },
            "appliedValues": null,
            "wouldApplyValues": { "enabled": true, "phase_count": 4 },
            "effectiveValueFeatures": 2,
            "staged": false,
            "persisted": false,
        });
        let shaped = receipt(
            "T",
            Map::new(),
            Map::from_iter([
                ("enabled".into(), json!("true")),
                ("phase_count".into(), json!(4)),
            ]),
            true,
            &result,
        )
        .expect("truthful receipt");

        assert_eq!(shaped["requested_values"]["enabled"], json!("true"));
        assert_eq!(shaped["applied_values"], Value::Null);
        assert_eq!(shaped["would_apply_values"]["enabled"], json!(true));
        assert_eq!(shaped["effective_value_features"], json!(2));

        let staged_result = json!({
            "project": "project-1",
            "dryRun": false,
            "matched": 1,
            "changed": 1,
            "unchanged": 0,
            "changedByLayer": { "lv_poles": 1 },
            "appliedValues": { "enabled": true },
            "wouldApplyValues": null,
            "effectiveValueFeatures": 1,
            "staged": true,
            "persisted": false,
        });
        let staged = receipt(
            "T",
            Map::new(),
            Map::from_iter([("enabled".into(), json!("true"))]),
            false,
            &staged_result,
        )
        .expect("truthful staged receipt");
        assert_eq!(staged["requested_values"]["enabled"], json!("true"));
        assert_eq!(staged["applied_values"]["enabled"], json!(true));
        assert_eq!(staged["would_apply_values"], Value::Null);
    }

    #[test]
    fn receipt_refuses_an_older_or_mode_confused_application_reply() {
        let old = json!({ "project": "project-1", "dryRun": false });
        assert_eq!(
            receipt("T", Map::new(), Map::new(), false, &old)
                .expect_err("missing applied values")
                .code(),
            "desktop_unreadable"
        );

        let confused = json!({
            "project": "project-1",
            "dryRun": true,
            "matched": 1,
            "appliedValues": { "enabled": true },
            "wouldApplyValues": null,
            "effectiveValueFeatures": 1,
        });
        assert_eq!(
            receipt(
                "T",
                Map::new(),
                Map::from_iter([("enabled".into(), json!(true))]),
                true,
                &confused,
            )
            .expect_err("dry run cannot claim applied values")
            .code(),
            "desktop_unreadable"
        );
    }

    #[test]
    fn receipt_refuses_missing_extra_or_composite_applied_values() {
        let requested = Map::from_iter([
            ("enabled".into(), json!(true)),
            ("phase_count".into(), json!(4)),
        ]);
        for applied_values in [
            json!({ "enabled": true }),
            json!({ "enabled": true, "phase_count": 4, "extra": "no" }),
            json!({ "enabled": [true], "phase_count": 4 }),
            json!({ "enabled": { "nested": true }, "phase_count": 4 }),
        ] {
            let result = json!({
                "project": "project-1",
                "dryRun": false,
                "matched": 1,
                "appliedValues": applied_values,
                "wouldApplyValues": null,
                "effectiveValueFeatures": 1,
            });
            assert_eq!(
                receipt("T", Map::new(), requested.clone(), false, &result)
                    .expect_err("untruthful applied values")
                    .code(),
                "desktop_unreadable"
            );
        }

        let dry_composite = json!({
            "project": "project-1",
            "dryRun": true,
            "matched": 1,
            "appliedValues": null,
            "wouldApplyValues": { "enabled": true, "phase_count": [4] },
            "effectiveValueFeatures": 1,
        });
        assert_eq!(
            receipt("T", Map::new(), requested, true, &dry_composite)
                .expect_err("untruthful would-apply values")
                .code(),
            "desktop_unreadable"
        );
    }

    #[test]
    fn receipt_requires_the_value_projection_to_cover_every_match() {
        let result = json!({
            "project": "project-1",
            "dryRun": false,
            "matched": 2,
            "appliedValues": { "enabled": true },
            "wouldApplyValues": null,
            "effectiveValueFeatures": 1,
        });
        assert_eq!(
            receipt(
                "T",
                Map::new(),
                Map::from_iter([("enabled".into(), json!(true))]),
                false,
                &result,
            )
            .expect_err("incomplete feature projection")
            .code(),
            "desktop_unreadable"
        );

        let missing_matched = json!({
            "project": "project-1",
            "dryRun": false,
            "appliedValues": { "enabled": true },
            "wouldApplyValues": null,
            "effectiveValueFeatures": 1,
        });
        assert_eq!(
            receipt(
                "T",
                Map::new(),
                Map::from_iter([("enabled".into(), json!(true))]),
                false,
                &missing_matched,
            )
            .expect_err("matched is required")
            .code(),
            "desktop_unreadable"
        );
    }

    #[test]
    fn zero_matches_requires_an_empty_active_value_projection() {
        let requested = Map::from_iter([("enabled".into(), json!(true))]);
        let dry = json!({
            "project": "project-1",
            "dryRun": true,
            "matched": 0,
            "appliedValues": null,
            "wouldApplyValues": {},
            "effectiveValueFeatures": 0,
            "changed": 0,
            "unchanged": 0,
            "changedByLayer": {},
            "staged": false,
            "persisted": false,
        });
        let shaped = receipt("T", Map::new(), requested.clone(), true, &dry)
            .expect("empty zero-match projection");
        assert_eq!(shaped["requested_values"]["enabled"], json!(true));
        assert_eq!(shaped["would_apply_values"], json!({}));
        assert_eq!(shaped["effective_value_features"], json!(0));

        let false_claim = json!({
            "project": "project-1",
            "dryRun": false,
            "matched": 0,
            "appliedValues": { "enabled": true },
            "wouldApplyValues": null,
            "effectiveValueFeatures": 0,
        });
        assert_eq!(
            receipt("T", Map::new(), requested, false, &false_claim)
                .expect_err("no value landed on zero matches")
                .code(),
            "desktop_unreadable"
        );
    }
}
