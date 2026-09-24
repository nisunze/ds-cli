//! Guided native style authoring over a named project's backend catalogue.
//! The shared command kernel owns transformations; native auth owns transport.
//! The visual Style Center uses the same transformations through WASM.

pub use native::{LANE_ARG, PROJECT_ARG};
pub mod appearance;
pub mod cartography;
pub mod dimension;
pub mod label;
pub mod list;
pub mod native;
pub mod print_variant;
pub mod read;
pub mod seed;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, ArgKind, Domain, Refusal};
use serde_json::{Value, json};

// Neutral argument helpers: a numeric bound and an English count say
// nothing about a paired window, so they come from the contract crate.
pub use ds_cli_contract::args::{INVALID_NUMBER, integer, plural};

pub(crate) const MIN_ZOOM_ARG: Arg = Arg::value(
    "min-zoom",
    "<0..24>",
    "Minimum display zoom; fractions allowed, omission preserves the bound.",
);
pub(crate) const MAX_ZOOM_ARG: Arg = Arg::value(
    "max-zoom",
    "<0..24>",
    "Maximum display zoom; fractions allowed, omission preserves the bound.",
);

pub static DOMAIN: Domain = Domain {
    id: "style",
    summary: "Guided appearance, labels, a second field dimension, and cartography.",
    commands: &[
        &list::COMMAND,
        &read::COMMAND,
        &seed::plan::COMMAND,
        &seed::create::COMMAND,
        &print_variant::plan::COMMAND,
        &print_variant::create::COMMAND,
        &appearance::plan::COMMAND,
        &appearance::set::COMMAND,
        &label::plan::COMMAND,
        &label::set::COMMAND,
        &dimension::plan::COMMAND,
        &dimension::set::COMMAND,
        &dimension::clear::COMMAND,
        &cartography::plan::COMMAND,
        &cartography::set::COMMAND,
    ],
};

/// The seamless pattern tile sizes. MapLibre repeats a pattern image by
/// tiling it, so a size that is not a power of two seams visibly at every
/// tile edge. Declared here as one number list because both the CLI choice
/// set and the parity test read it.
pub const PATTERN_SPACINGS: &[i64] = &[4, 8, 16, 32];

/// The most values one dimension names. Matches the shared kernel planner's
/// own 1..50 bound, so an over-long list is refused once, locally, before a
/// round trip that would only refuse it again.
pub const MAX_VALUES: usize = 50;

// ---------------------------------------------------------------------------
// The input grammars this domain parses for itself
// ---------------------------------------------------------------------------

pub const CONFIRMATION_REQUIRED: Refusal = Refusal {
    code: "confirmation_required",
    when: "--yes was not given for a command that publishes a style document",
    remedy: "run the matching `plan` command, read the document it returns, then re-run the same flags with --yes",
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
    when: "no colour, icon, size or icon overlap was supplied",
    remedy: "pass at least one of --color, --icon, --size or --icon-overlap",
};
pub const INVALID_LABEL: Refusal = Refusal {
    code: "invalid_label",
    when: "--field is empty, has surrounding whitespace, or exceeds the shared field-name bound",
    remedy: "copy one exact field from `ds style read <ref>` .data.fields",
};
pub const INVALID_CARTOGRAPHY: Refusal = Refusal {
    code: "invalid_cartography",
    when: "no cartography flag was supplied, or direction/pattern detail contradicts the line type or fill pattern set in the same call",
    remedy: "pass at least one cartography flag; keep --direction-* with `--line-type directional`, and --pattern-* with a --fill-pattern other than solid",
};

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
    fn the_seamless_tile_sizes_are_one_list_the_flag_reads() {
        let declared: Vec<String> = PATTERN_SPACINGS
            .iter()
            .map(|size| size.to_string())
            .collect();
        let offered: Vec<String> = cartography::plan::COMMAND
            .arg("pattern-spacing")
            .expect("--pattern-spacing is declared")
            .choices
            .iter()
            .map(|choice| (*choice).to_string())
            .collect();
        assert_eq!(
            declared, offered,
            "--pattern-spacing must offer exactly the power-of-two tile sizes MapLibre repeats seamlessly"
        );
    }

    /// Fifteen commands, one route. A style document is governed state behind
    /// ds-brain, so every one of them names its lane and none of them names a
    /// window.
    #[test]
    fn no_style_command_takes_a_host_or_a_pairing_descriptor() {
        for command in DOMAIN.commands {
            let flags: Vec<&str> = command.args.iter().map(|arg| arg.name).collect();
            assert!(
                flags.contains(&"lane"),
                "`{}` does not name the deployment lane it authenticates on",
                command.id
            );
            for windowed in ["host", "project", "desktop-descriptor", "target"] {
                assert!(
                    !flags.contains(&windowed),
                    "`{}` still declares `--{windowed}`, which only a paired window needed",
                    command.id
                );
            }
        }
    }
}
