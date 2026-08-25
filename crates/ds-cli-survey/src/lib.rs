//! `ds survey` — survey forms and their data: form schema, fields, export
//! and the Working Area.
//!
//! ## Why this domain is a bridge domain
//!
//! A survey form is a governed Form Factory document (`eds_forms/{slug}`)
//! behind ds-brain, which is the only gateway: it validates field types,
//! resolves permissions, and refuses an update authored against a version
//! that has moved. Survey entries are field data under the same gate. None
//! of that is reachable from a file, and none of it may be reached with an
//! ambient credential — so every command here is one named semantic
//! operation the *paired application* performs under the session it holds.
//!
//! ## What the family is
//!
//! ```text
//!   form list → form read → form field add | set | remove
//!   export
//!   working-area read      (materialize with `ds map survey download`)
//! ```
//!
//! Reads are bounded projections of the same schema and Working Area the
//! application renders. Writes go through the same `update` action the Form
//! Factory editor uses: the whole field list, one version guard, applied
//! atomically or refused whole.
//!
//! ## What is deliberately absent
//!
//! **Survey rows.** No command returns, edits or deletes an entry. Entries are
//! field data owned by the surveyor's outbox and the governed mutate API; the
//! CLI receives bounded counts and artifacts, never rows. Migration stays
//! under `ds map survey migrate` with its fixed policy.

pub mod export;
pub mod field;
pub mod form;
pub mod working_area;

use std::time::Duration;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Domain, Refusal};

pub use ds_cli_desktop::ops::{
    AMBIGUOUS, BridgeOp, DESCRIPTOR_ARG, NOT_PAIRED, PAIRING_REJECTED, REFUSED, SIGNED_OUT,
    UNREACHABLE, UNREADABLE, UNSUPPORTED, classify_signed_out, invoke, paired, paired_availability,
    plural,
};

pub static DOMAIN: Domain = Domain {
    id: "survey",
    summary: "Survey forms and data: schema, fields, export, working area.",
    commands: &[
        &form::list::COMMAND,
        &form::read::COMMAND,
        &field::add::COMMAND,
        &field::set::COMMAND,
        &field::remove::COMMAND,
        &export::COMMAND,
        &working_area::read::COMMAND,
    ],
};

// ---------------------------------------------------------------------------
// The declared wire contract
// ---------------------------------------------------------------------------

pub const FORMS_LIST: BridgeOp = BridgeOp {
    operation: "survey.forms.list",
    arguments: &[],
};
pub const FORM_READ: BridgeOp = BridgeOp {
    operation: "survey.form.read",
    arguments: &["form"],
};
pub const FIELD_ADD: BridgeOp = BridgeOp {
    operation: "survey.form.field.add",
    arguments: &[
        "form", "key", "type", "label", "required", "options", "helpText",
    ],
};
pub const FIELD_SET: BridgeOp = BridgeOp {
    operation: "survey.form.field.set",
    arguments: &[
        "form",
        "field",
        "label",
        "required",
        "options",
        "helpText",
        "placeholder",
    ],
};
pub const FIELD_REMOVE: BridgeOp = BridgeOp {
    operation: "survey.form.field.remove",
    arguments: &["form", "field"],
};
pub const EXPORT: BridgeOp = BridgeOp {
    operation: "survey.export",
    arguments: &["format", "forms", "dateFrom", "dateTo", "surveyors", "bbox"],
};
pub const WORKING_AREA_READ: BridgeOp = BridgeOp {
    operation: "survey.working_area.read",
    arguments: &[],
};

/// Every operation this domain can send, for the parity test to walk.
pub const BRIDGE_OPS: &[&BridgeOp] = &[
    &FORMS_LIST,
    &FORM_READ,
    &FIELD_ADD,
    &FIELD_SET,
    &FIELD_REMOVE,
    &EXPORT,
    &WORKING_AREA_READ,
];

/// The most options one choice field may carry through this door.
pub const MAX_OPTIONS: usize = 200;
/// The most forms or surveyors one export may name.
pub const MAX_EXPORT_SELECTORS: usize = 50;

// ---------------------------------------------------------------------------
// Timeouts
// ---------------------------------------------------------------------------

/// A schema read is one Form Factory round trip, cache-first.
pub const READ_TIMEOUT: Duration = Duration::from_secs(3 * 60);
/// A schema write re-reads the form, then saves once.
pub const WRITE_TIMEOUT: Duration = Duration::from_secs(3 * 60);
/// The export API itself allows five minutes; leave room for signing.
pub const EXPORT_TIMEOUT: Duration = Duration::from_secs(6 * 60);

// ---------------------------------------------------------------------------
// Shared arguments and refusals
// ---------------------------------------------------------------------------

pub const FORM_ARG: Arg = Arg::value(
    "form",
    "<slug>",
    "The form's slug, as listed by `ds survey form list`.",
)
.required();
pub const FIELD_ARG: Arg = Arg::value(
    "field",
    "<key>",
    "The field's key, as shown by `ds survey form read`.",
)
.required();

pub const SURVEY_REFUSED: Refusal = Refusal {
    code: "desktop_refused",
    when: "the application or the Form Factory API declined the request",
    remedy: "read detail.detail for the application's exact message",
};
pub const FORM_NOT_FOUND: Refusal = Refusal {
    code: "survey_form_not_found",
    when: "no form with that slug exists",
    remedy: "list forms with `ds survey form list` and pass an exact slug",
};
pub const FIELD_NOT_FOUND: Refusal = Refusal {
    code: "survey_field_not_found",
    when: "the form has no field with that key",
    remedy: "read the form with `ds survey form read --form <slug>` and pass an exact key",
};
pub const FIELD_EXISTS: Refusal = Refusal {
    code: "survey_field_exists",
    when: "the form already has a field with that key",
    remedy: "choose another key, or change the existing field with `ds survey form field set`",
};
pub const VERSION_CONFLICT: Refusal = Refusal {
    code: "survey_version_conflict",
    when: "the form was saved by someone else while the command was in flight",
    remedy: "re-read the form with `ds survey form read` and issue the command again",
};
pub const NOT_PERMITTED: Refusal = Refusal {
    code: "survey_not_permitted",
    when: "the signed-in user may read this form but not change it",
    remedy: "ask a form owner or project admin for form-edit access",
};
pub const INVALID_FORM: Refusal = Refusal {
    code: "invalid_form",
    when: "the slug is empty, padded, too long, or not lowercase letters, digits, '_' or '-'",
    remedy: "pass the exact slug shown by `ds survey form list`",
};
pub const INVALID_FIELD: Refusal = Refusal {
    code: "invalid_field",
    when: "a field key is empty, too long, or not a letter followed by letters, digits or '_'",
    remedy: "use a key like `meter_number`",
};
pub const INVALID_INPUT: Refusal = Refusal {
    code: "invalid_input",
    when: "a value is malformed: a date not yyyy-mm-dd, a bbox not w,s,e,n, too many options",
    remedy: "read the flag's summary in `--help` and pass the declared shape",
};
pub const CONFIRMATION_REQUIRED: Refusal = Refusal {
    code: "confirmation_required",
    when: "--yes was not given for a command that writes",
    remedy: "re-run with --yes once the change is intended",
};

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

pub fn form_slug(raw: &str) -> Result<&str, Failure> {
    let ok = !raw.is_empty()
        && raw.len() <= 120
        && raw.trim() == raw
        && raw
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && raw
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '-'));
    if !ok {
        return Err(
            Failure::invalid("invalid_form", "--form is not a canonical form slug")
                .remedy(INVALID_FORM.remedy),
        );
    }
    Ok(raw)
}

pub fn field_key(raw: &str) -> Result<&str, Failure> {
    let ok = !raw.is_empty()
        && raw.len() <= 80
        && raw.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && raw.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !ok {
        return Err(
            Failure::invalid("invalid_field", "the field key is not a canonical key")
                .remedy(INVALID_FIELD.remedy),
        );
    }
    Ok(raw)
}

pub fn boolean(raw: &str, flag: &str) -> Result<bool, Failure> {
    match raw {
        "true" | "yes" | "on" => Ok(true),
        "false" | "no" | "off" => Ok(false),
        _ => Err(
            Failure::invalid("invalid_input", format!("`--{flag}` must be true or false"))
                .remedy(INVALID_INPUT.remedy),
        ),
    }
}

pub fn date<'a>(raw: &'a str, flag: &str) -> Result<&'a str, Failure> {
    let bytes = raw.as_bytes();
    let ok = bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(i, b)| matches!(i, 4 | 7) || b.is_ascii_digit());
    if !ok {
        return Err(
            Failure::invalid("invalid_input", format!("`--{flag}` must be yyyy-mm-dd"))
                .remedy(INVALID_INPUT.remedy),
        );
    }
    Ok(raw)
}

pub fn bbox(raw: &str) -> Result<[f64; 4], Failure> {
    let parts: Vec<f64> = raw
        .split(',')
        .map(|p| p.trim().parse::<f64>())
        .collect::<Result<_, _>>()
        .map_err(|_| {
            Failure::invalid(
                "invalid_input",
                "`--bbox` must be four numbers: west,south,east,north",
            )
            .remedy(INVALID_INPUT.remedy)
        })?;
    if parts.len() != 4 {
        return Err(Failure::invalid(
            "invalid_input",
            "`--bbox` must be four numbers: west,south,east,north",
        )
        .remedy(INVALID_INPUT.remedy));
    }
    let [w, s, e, n] = [parts[0], parts[1], parts[2], parts[3]];
    let ok = (-180.0..=180.0).contains(&w)
        && (-180.0..=180.0).contains(&e)
        && (-90.0..=90.0).contains(&s)
        && (-90.0..=90.0).contains(&n)
        && w < e
        && s < n;
    if !ok {
        return Err(Failure::invalid(
            "invalid_input",
            "`--bbox` must be west<east within ±180 and south<north within ±90",
        )
        .remedy(INVALID_INPUT.remedy));
    }
    Ok([w, s, e, n])
}

pub fn options(raw: &[String]) -> Result<Vec<String>, Failure> {
    if raw.len() > MAX_OPTIONS {
        return Err(Failure::invalid(
            "invalid_input",
            format!("at most {MAX_OPTIONS} --option values are accepted"),
        )
        .remedy(INVALID_INPUT.remedy));
    }
    let mut out = Vec::with_capacity(raw.len());
    for value in raw {
        let trimmed = value.trim();
        if trimmed.is_empty() || trimmed.len() > 160 {
            return Err(Failure::invalid(
                "invalid_input",
                "each --option must be 1 to 160 characters",
            )
            .remedy(INVALID_INPUT.remedy));
        }
        out.push(trimmed.to_string());
    }
    Ok(out)
}

pub fn text<'a>(raw: &'a str, flag: &str, max: usize) -> Result<&'a str, Failure> {
    if raw.len() > max {
        return Err(Failure::invalid(
            "invalid_input",
            format!("`--{flag}` must be at most {max} characters"),
        )
        .remedy(INVALID_INPUT.remedy));
    }
    Ok(raw)
}

// ---------------------------------------------------------------------------
// Failure classification
// ---------------------------------------------------------------------------

/// The application reports every refusal as `desktop_refused` with its own
/// message. The adapter prefixes the messages this domain can act on, so a
/// caller branching on `error.code` learns which one it was.
pub fn classify_survey_failure(failure: Failure) -> Failure {
    let failure = classify_signed_out(failure);
    if failure.code() != "desktop_refused" {
        return failure;
    }
    let detail = failure
        .detail_value()
        .and_then(|value| value["detail"].as_str())
        .unwrap_or_default()
        .to_string();
    let lowered = detail.to_ascii_lowercase();
    if lowered.starts_with("form not found") {
        return Failure::invalid("survey_form_not_found", detail).remedy(FORM_NOT_FOUND.remedy);
    }
    if lowered.starts_with("field not found") {
        return Failure::invalid("survey_field_not_found", detail).remedy(FIELD_NOT_FOUND.remedy);
    }
    if lowered.starts_with("field exists") {
        return Failure::invalid("survey_field_exists", detail).remedy(FIELD_EXISTS.remedy);
    }
    if lowered.starts_with("version conflict") {
        return Failure::conflict("survey_version_conflict", detail)
            .remedy(VERSION_CONFLICT.remedy);
    }
    if lowered.starts_with("not permitted") {
        return Failure::failed("survey_not_permitted", detail).remedy(NOT_PERMITTED.remedy);
    }
    failure
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_and_keys_are_canonical() {
        assert!(form_slug("edcl_customers_survey").is_ok());
        assert!(form_slug("Edcl").is_err());
        assert!(form_slug(" padded").is_err());
        assert!(field_key("meter_number").is_ok());
        assert!(field_key("1st").is_err());
        assert!(field_key("has-dash").is_err());
    }

    #[test]
    fn bbox_and_dates_are_shaped() {
        assert_eq!(
            bbox("29.5,-2.1,30.2,-1.4").unwrap(),
            [29.5, -2.1, 30.2, -1.4]
        );
        assert!(bbox("30,-1,29,-2").is_err());
        assert!(bbox("1,2,3").is_err());
        assert!(date("2026-08-25", "from").is_ok());
        assert!(date("25/08/2026", "from").is_err());
    }

    #[test]
    fn every_operation_is_declared_once() {
        let mut seen = std::collections::BTreeSet::new();
        for op in BRIDGE_OPS {
            assert!(seen.insert(op.operation), "{} declared twice", op.operation);
        }
        assert_eq!(DOMAIN.commands.len(), 7);
    }

    #[test]
    fn writes_need_confirmation_and_reads_do_not() {
        use ds_cli_contract::spec::Effect;
        assert_eq!(form::list::COMMAND.effect, Effect::ReadOnly);
        assert_eq!(form::read::COMMAND.effect, Effect::ReadOnly);
        assert_eq!(working_area::read::COMMAND.effect, Effect::ReadOnly);
        assert!(field::add::COMMAND.effect.needs_confirmation());
        assert!(field::set::COMMAND.effect.needs_confirmation());
        assert!(field::remove::COMMAND.effect.needs_confirmation());
        assert!(export::COMMAND.effect.needs_confirmation());
    }
}
