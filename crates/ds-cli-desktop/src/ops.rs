//! The declared wire contract every paired-application domain shares.
//!
//! `ds map` established the shape and `ds work` needed the same one, so it
//! lives here rather than being written twice: an operation is a value, its
//! argument keys are declared beside it, and [`invoke`] refuses to send a key
//! the operation does not declare. Combined with each domain's parity test —
//! which proves the declaration matches the application's own input schema —
//! a misspelled field is caught inside `ds`, at the boundary, instead of
//! arriving as a validation error from a webview.
//!
//! Nothing domain-specific belongs here. A bound, a marker or a projection
//! field that only one domain reads stays in that domain, where its parity
//! test can hold it to the application's source.

use std::time::Duration;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, ArgKind, Availability, Refusal};
use serde_json::{Value, json};

use crate::bridge;
use crate::discover::Descriptor;

/// One bridge operation and the exact argument keys a domain may send it.
///
/// A key written as `settings.intervalM` declares a nested key inside the
/// `settings` object — which is where the dangerous hand copies live, because
/// the application's own settings are camelCase and CLI flags are not.
pub struct BridgeOp {
    pub operation: &'static str,
    pub arguments: &'static [&'static str],
}

/// Resolve the paired application from an explicit descriptor path, or
/// discover it.
pub fn paired(explicit: Option<&str>) -> Result<Descriptor, Failure> {
    Ok(bridge::paired(explicit)?.descriptor)
}

/// Send one declared operation.
///
/// The guard is the point: an argument key the operation does not declare
/// never leaves this process.
pub fn invoke(
    descriptor: &Descriptor,
    op: &BridgeOp,
    arguments: Value,
    timeout: Duration,
) -> Result<Value, Failure> {
    if let Some(undeclared) = undeclared_key(op, &arguments) {
        return Err(Failure::internal(
            "undeclared_bridge_argument",
            format!(
                "`{}` does not declare the argument `{undeclared}`",
                op.operation
            ),
        )
        .remedy("this is a defect in ds; report it with the command you ran"));
    }
    bridge::invoke(descriptor, op.operation, arguments, timeout)
}

/// The first key in `arguments` the operation does not declare, if any.
pub fn undeclared_key(op: &BridgeOp, arguments: &Value) -> Option<String> {
    let object = arguments.as_object()?;
    for (key, value) in object {
        let nested_prefix = format!("{key}.");
        let has_nested = op
            .arguments
            .iter()
            .any(|declared| declared.starts_with(&nested_prefix));
        if has_nested {
            let Some(inner) = value.as_object() else {
                return Some(key.clone());
            };
            for inner_key in inner.keys() {
                let qualified = format!("{key}.{inner_key}");
                if !op.arguments.contains(&qualified.as_str()) {
                    return Some(qualified);
                }
            }
        } else if !op.arguments.contains(&key.as_str()) {
            return Some(key.clone());
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Availability and the shared refusals
// ---------------------------------------------------------------------------

/// Always available, and the reasoning settles it for every paired domain.
///
/// It is tempting to gate these commands on a descriptor existing, so
/// `ds doctor` says something about the application. Two things make that
/// wrong:
///
/// * **It would make `--desktop-descriptor` unreachable.** Dispatch checks
///   availability *before* it has parsed a single flag, so a gate that
///   refused because discovery found nothing would refuse the one invocation
///   that was about to name where to look.
/// * **It would make the input contract untestable.** Every handler
///   validates its own flags before it touches the bridge, so a malformed
///   argument is a typed refusal whether or not an application is running —
///   which is the only reachable behaviour on a CI machine.
///
/// `ds desktop status` answers whether there is a session, and every paired
/// command documents `desktop_not_paired` with the same code and remedy the
/// gate would have used.
pub fn paired_availability() -> Availability {
    Availability::Available
}

pub const NOT_PAIRED: Refusal = Refusal {
    code: "desktop_not_paired",
    when: "no DS GridDesign session is running on this machine",
    remedy: "start DS GridDesign, then run `ds desktop status`",
};
pub const AMBIGUOUS: Refusal = Refusal {
    code: "desktop_ambiguous",
    when: "Stable, Canary or a dev build are running together",
    remedy: "pass --desktop-descriptor <path> to name which one",
};
pub const UNREACHABLE: Refusal = Refusal {
    code: "desktop_unreachable",
    when: "the descriptor is stale, or the application did not answer in time",
    remedy: "DS GridDesign may have exited; restart it and retry",
};
pub const PAIRING_REJECTED: Refusal = Refusal {
    code: "pairing_rejected",
    when: "the application refused the descriptor's pairing secret",
    remedy: "the descriptor is stale; restart DS GridDesign",
};
pub const REFUSED: Refusal = Refusal {
    code: "desktop_refused",
    when: "the application answered and refused the operation",
    remedy: "read detail.detail for the application's own message",
};
pub const UNSUPPORTED: Refusal = Refusal {
    code: "desktop_operation_unsupported",
    when: "this DS GridDesign build does not offer the operation",
    remedy: "update DS GridDesign; `ds desktop status` reports the profile",
};
pub const UNREADABLE: Refusal = Refusal {
    code: "desktop_unreadable",
    when: "the application's reply could not be read within its bound",
    remedy: "restart DS GridDesign and retry",
};
pub const SIGNED_OUT: Refusal = Refusal {
    code: "desktop_signed_out",
    when: "the application is running but signed out, or has no project selected",
    remedy: "sign in and select a project in DS GridDesign",
};

/// What the application says when a project operation is asked for without a
/// project session. Matched case-insensitively against its own message.
///
/// This is a hand copy of the application's prose, which is the least stable
/// thing to key on anywhere in the CLI — so each calling domain's parity test
/// requires a marker to still appear in the application's source, and the
/// fallback when none match is the untranslated refusal rather than a wrong
/// one.
pub const SIGNED_OUT_MARKERS: &[&str] = &[
    "no active project",
    "open a project",
    "sign in",
    "signed out",
];

/// Turn the application's own refusal into the signed-out refusal when that
/// is what it actually is.
///
/// Project operations require a signed-in session with a project selected,
/// and the application reports that as an ordinary operation refusal. Letting
/// it through as `desktop_refused` would send a caller to read `detail` for a
/// condition that has a name, a remedy and a `ds` command that diagnoses it.
pub fn classify_signed_out(failure: Failure) -> Failure {
    if failure.code() != "desktop_refused" {
        return failure;
    }
    let detail = failure
        .detail_value()
        .and_then(|detail| detail["detail"].as_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !SIGNED_OUT_MARKERS
        .iter()
        .any(|marker| detail.contains(marker))
    {
        return failure;
    }
    Failure::unauthorized(
        "desktop_signed_out",
        "the paired session is signed out, or has no project selected",
    )
    .remedy(SIGNED_OUT.remedy)
    .next("ds desktop status")
}

// ---------------------------------------------------------------------------
// Flag shapes shared by every paired domain
// ---------------------------------------------------------------------------

/// The `--desktop-descriptor` flag, declared identically by every paired
/// command so a caller who learned it once has learned it everywhere.
pub const DESCRIPTOR_ARG: Arg = Arg {
    name: "desktop-descriptor",
    kind: ArgKind::Value,
    value: "<path>",
    required: false,
    default: None,
    choices: &[],
    summary: "Use this bridge descriptor instead of discovering one.",
};

pub const INVALID_NUMBER: Refusal = Refusal {
    code: "invalid_number",
    when: "a numeric flag is not a number, or falls outside the bound in its summary",
    remedy: "the refusal carries the accepted range",
};

/// A whole-number flag, held to the bound stated in its own summary.
pub fn integer(raw: &str, flag: &str, min: i64, max: i64) -> Result<i64, Failure> {
    let parsed = raw.parse::<i64>().map_err(|_| {
        Failure::invalid(
            "invalid_number",
            format!("`--{flag}` must be a whole number"),
        )
        .remedy(format!("pass {min}..{max}"))
    })?;
    if parsed < min || parsed > max {
        return Err(
            Failure::invalid("invalid_number", format!("`--{flag}` is outside its bound"))
                .remedy(format!("pass {min}..{max}"))
                .detail(json!({ "given": parsed, "min": min, "max": max })),
        );
    }
    Ok(parsed)
}

/// Render a count with its noun, so a human line reads as English.
pub fn plural(count: u64, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ZOOM_TO: BridgeOp = BridgeOp {
        operation: "map.zoom_to",
        arguments: &["bbox", "padding"],
    };
    const POINTS_ALONG: BridgeOp = BridgeOp {
        operation: "gis.points_along",
        arguments: &["layerId", "settings.intervalM", "settings.includeEnds"],
    };

    #[test]
    fn an_argument_key_the_operation_does_not_declare_never_leaves_this_process() {
        // The guard behind `invoke`. It is what makes a declared wire
        // contract load-bearing rather than documentation: a handler cannot
        // send a key its domain's parity test has not proved.
        assert_eq!(
            undeclared_key(&ZOOM_TO, &json!({ "bbox": [1, 2, 3, 4], "padding": 8 })),
            None
        );
        assert_eq!(
            undeclared_key(&ZOOM_TO, &json!({ "bbox": [1, 2, 3, 4], "zoom": 8 })),
            Some("zoom".to_string())
        );

        // Nested keys are checked one level in, because that is where the
        // application's camelCase settings live.
        assert_eq!(
            undeclared_key(
                &POINTS_ALONG,
                &json!({ "layerId": "sketch:x", "settings": { "intervalM": 25 } })
            ),
            None
        );
        assert_eq!(
            undeclared_key(
                &POINTS_ALONG,
                &json!({ "layerId": "sketch:x", "settings": { "interval_m": 25 } })
            ),
            Some("settings.interval_m".to_string()),
            "a settings key that drifts case must be caught here"
        );
        // A declared object that arrives as a scalar is undeclared too: the
        // application would reject the whole payload rather than one field.
        assert_eq!(
            undeclared_key(&POINTS_ALONG, &json!({ "settings": 25 })),
            Some("settings".to_string())
        );
    }

    #[test]
    fn a_numeric_flag_is_held_to_the_bound_its_summary_states() {
        assert_eq!(integer("4", "parallel", 1, 32).expect("in range"), 4);
        for bad in ["0", "33", "four", "", "1.5"] {
            assert_eq!(
                integer(bad, "parallel", 1, 32)
                    .expect_err("must refuse")
                    .code(),
                "invalid_number",
                "`{bad}` was accepted as a bounded whole number"
            );
        }
    }

    #[test]
    fn an_application_refusal_is_only_reclassified_when_it_really_is_signed_out() {
        let signed_out = Failure::failed("desktop_refused", "refused")
            .detail(json!({ "detail": "No active project. Open a project first." }));
        assert_eq!(
            classify_signed_out(signed_out).code(),
            "desktop_signed_out",
            "the application's own signed-out prose must become the named refusal"
        );

        let other = Failure::failed("desktop_refused", "refused")
            .detail(json!({ "detail": "that transformer has no staged edits" }));
        assert_eq!(
            classify_signed_out(other).code(),
            "desktop_refused",
            "an ordinary refusal must not be renamed"
        );

        let unreachable = Failure::unavailable("desktop_unreachable", "gone");
        assert_eq!(
            classify_signed_out(unreachable).code(),
            "desktop_unreachable"
        );
    }
}
