//! `ds style` — read Style Center documents, author their guided base
//! appearance, and add a SECOND categorical dimension by a second field.
//!
//! ## Why this domain is a bridge domain
//!
//! A style is one MapLibre document per style ref, validated and published by
//! ds-brain. The paired application holds the `/layers` state those documents
//! come from, the pure module that turns "halo by drafting_status" into match
//! expressions, and the governed save payload its own *Save globally* button
//! sends. `ds` reuses all three through one named operation each, so an agent
//! and a person produce byte-identical documents and meet byte-identical
//! validation. `ds` carries no token and no second styling model.
//!
//! ## What the family is
//!
//! ```text
//!   list → read → appearance plan/set → dimension plan/set | dimension clear
//! ```
//!
//! `appearance` owns flat primary colour, catalog icon and base size through
//! the Style Center's guided schema. `dimension` adds a second field on a
//! channel the primary appearance does not use — halo, opacity, or size — as
//! plain `["match", ["get", field], …]` expressions the legend reads back.
//!
//! ## What is deliberately absent
//!
//! Raw document writes. A command that accepted arbitrary paint JSON would
//! bypass the guided invariants (one label type per match, no arm-less
//! match, the halo channel per layer type) that keep a document renderable.
//! The JSON tab of the Style Center remains the human escape hatch.

pub mod appearance;
pub mod dimension;
pub mod list;
pub mod read;

use std::time::Duration;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, ArgKind, Domain, Refusal};
use serde_json::{Value, json};

pub use ds_cli_desktop::ops::{
    AMBIGUOUS, BridgeOp, DESCRIPTOR_ARG, INVALID_NUMBER, NOT_PAIRED, PAIRING_REJECTED, REFUSED,
    SIGNED_OUT, SIGNED_OUT_MARKERS, UNREACHABLE, UNREADABLE, UNSUPPORTED, classify_signed_out,
    integer, invoke, paired, paired_availability, plural,
};

pub static DOMAIN: Domain = Domain {
    id: "style",
    summary: "Guided map appearance and a second field-driven dimension.",
    commands: &[
        &list::COMMAND,
        &read::COMMAND,
        &appearance::plan::COMMAND,
        &appearance::set::COMMAND,
        &dimension::plan::COMMAND,
        &dimension::set::COMMAND,
        &dimension::clear::COMMAND,
    ],
};

// ---------------------------------------------------------------------------
// The declared wire contract
// ---------------------------------------------------------------------------

pub const STYLE_LIST: BridgeOp = BridgeOp {
    operation: "style.list",
    arguments: &["query", "limit"],
};
pub const STYLE_READ: BridgeOp = BridgeOp {
    operation: "style.read",
    arguments: &["ref"],
};
pub const APPEARANCE_SET: BridgeOp = BridgeOp {
    operation: "style.appearance.set",
    arguments: &["ref", "color", "icon", "size", "apply"],
};
pub const DIMENSION_SET: BridgeOp = BridgeOp {
    operation: "style.dimension.set",
    arguments: &[
        "ref", "field", "channel", "values", "other", "color", "apply",
    ],
};
pub const DIMENSION_CLEAR: BridgeOp = BridgeOp {
    operation: "style.dimension.clear",
    arguments: &["ref", "apply"],
};

/// Every operation this domain can send, for the parity test to walk. `plan`
/// and `set` are one operation — `apply` false or true — so the list is
/// shorter than the command list.
pub const BRIDGE_OPS: &[&BridgeOp] = &[
    &STYLE_LIST,
    &STYLE_READ,
    &APPEARANCE_SET,
    &DIMENSION_SET,
    &DIMENSION_CLEAR,
];

/// The most values one dimension names. Matches the adapter's own bound so an
/// over-long list is refused once, locally.
pub const MAX_VALUES: usize = 50;

pub const READ_TIMEOUT: Duration = Duration::from_secs(60);
/// A publish is one governed round trip to ds-brain plus a local cache write.
pub const WRITE_TIMEOUT: Duration = Duration::from_secs(2 * 60);

// ---------------------------------------------------------------------------
// Refusals this domain adds to the shared pairing set
// ---------------------------------------------------------------------------

pub const STYLE_REFUSED: Refusal = Refusal {
    code: "desktop_refused",
    when: "no such style ref, an unknown field or channel, or ds-brain declined the document",
    remedy: "check the ref with `ds style list` and the fields with `ds style read`; read detail.detail for the message",
};
pub const CONFIRMATION_REQUIRED: Refusal = Refusal {
    code: "confirmation_required",
    when: "--yes was not given for a command that publishes a style document",
    remedy: "run `ds style dimension plan` first, then re-run with --yes once the expressions are what you intend",
};
pub const INVALID_VALUE_SPEC: Refusal = Refusal {
    code: "invalid_value_spec",
    when: "a --value flag is not `<value>[=<amount>[:<#hex>]]`",
    remedy: "pass e.g. --value approved=2.5:#00FF00 --value draft=0",
};
pub const INVALID_COLOR: Refusal = Refusal {
    code: "invalid_color",
    when: "a colour flag is not a hex colour",
    remedy: "pass e.g. --color #FFFFFF or #FFFFFF80",
};
pub const INVALID_APPEARANCE: Refusal = Refusal {
    code: "invalid_appearance",
    when: "no colour, icon or size was supplied",
    remedy: "pass at least one of --color, --icon or --size",
};

/// Ordinary operation refusals stay `desktop_refused`; only the signed-out
/// condition has its own code here, by the shared rule.
pub fn classify_style_failure(failure: Failure) -> Failure {
    classify_signed_out(failure)
}

// ---------------------------------------------------------------------------
// Flag shapes shared across the domain
// ---------------------------------------------------------------------------

pub const REF_ARG: Arg = Arg {
    name: "ref",
    kind: ArgKind::Value,
    value: "<style-ref>",
    required: true,
    default: None,
    choices: &[],
    summary: "The style ref, as `ds style list` reports it (e.g. bare Design GeoJSON master/lv_poles or tiled master/lv_poles_vt).",
};

/// A hex colour, held to the one representation the Style Center persists.
pub fn color(raw: &str, flag: &str) -> Result<String, Failure> {
    let trimmed = raw.trim();
    let hex = trimmed.strip_prefix('#').unwrap_or("");
    let valid = (hex.len() == 6 || hex.len() == 8) && hex.chars().all(|c| c.is_ascii_hexdigit());
    if !valid {
        return Err(Failure::invalid(
            "invalid_color",
            format!("`--{flag}` must be a hex colour like #FFFFFF or #FFFFFF80"),
        )
        .remedy(INVALID_COLOR.remedy)
        .detail(json!({ "given": raw })));
    }
    Ok(format!("#{}", hex.to_ascii_uppercase()))
}

/// One `--value` flag: `<value>[=<amount>[:<#hex>]]`.
pub fn value_spec(raw: &str) -> Result<Value, Failure> {
    let refuse = |why: &str| {
        Failure::invalid("invalid_value_spec", format!("`--value {raw}` {why}"))
            .remedy(INVALID_VALUE_SPEC.remedy)
            .detail(json!({ "given": raw }))
    };
    let (name, rest) = match raw.split_once('=') {
        Some((name, rest)) => (name.trim(), Some(rest)),
        None => (raw.trim(), None),
    };
    if name.is_empty() {
        return Err(refuse("names no value"));
    }
    let mut spec = serde_json::Map::new();
    spec.insert("value".into(), json!(name));
    if let Some(rest) = rest {
        let (amount, colour) = match rest.split_once(':') {
            Some((amount, colour)) => (amount.trim(), Some(colour.trim())),
            None => (rest.trim(), None),
        };
        if !amount.is_empty() {
            let parsed: f64 = amount
                .parse()
                .map_err(|_| refuse("has an amount that is not a number"))?;
            if !parsed.is_finite() {
                return Err(refuse("has an amount that is not a number"));
            }
            spec.insert("amount".into(), json!(parsed));
        }
        if let Some(colour) = colour {
            spec.insert("color".into(), json!(color(colour, "value")?));
        }
    }
    Ok(Value::Object(spec))
}

/// Keep a human line one line wide without hiding that it was cut.
pub fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    let kept: String = text.chars().take(width.saturating_sub(1)).collect();
    format!("{kept}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_flag_carries_name_amount_and_colour_and_refuses_the_rest() {
        assert_eq!(
            value_spec("approved=2.5:#00ff00").expect("valid"),
            json!({ "value": "approved", "amount": 2.5, "color": "#00FF00" })
        );
        assert_eq!(
            value_spec("draft").expect("valid"),
            json!({ "value": "draft" })
        );
        assert_eq!(
            value_spec("draft=0").expect("valid"),
            json!({ "value": "draft", "amount": 0.0 })
        );
        assert_eq!(
            value_spec("new=:#FFFFFF").expect("valid"),
            json!({ "value": "new", "color": "#FFFFFF" })
        );
        for bad in ["=2", "x=abc", "x=1:red", ""] {
            let code = value_spec(bad).expect_err("must refuse").code().to_string();
            assert!(
                code == "invalid_value_spec" || code == "invalid_color",
                "`{bad}` was accepted"
            );
        }
    }

    #[test]
    fn every_declared_operation_is_listed_for_the_parity_test_to_walk() {
        let mut names: Vec<&str> = BRIDGE_OPS.iter().map(|op| op.operation).collect();
        names.sort_unstable();
        let mut unique = names.clone();
        unique.dedup();
        assert_eq!(names, unique, "an operation is declared twice");
        // Appearance plan/set and dimension plan/set each share one operation.
        assert_eq!(names.len(), DOMAIN.commands.len() - 2);
    }
}
