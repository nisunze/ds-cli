//! `ds survey form field add | set | remove` — change one field of a form
//! through the same governed `update` the Form Factory editor uses.
//!
//! The application re-reads the form, applies the one change to the whole
//! field list, and saves it against the version it read. A form saved by
//! someone else in between is refused, not merged.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Command, Effect, Example, Execution, Refusal};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::{DESCRIPTOR_ARG, FIELD_ARG, FORM_ARG};

const LABEL_ARG: Arg = Arg::value(
    "label",
    "<text>",
    "The label surveyors see. Up to 200 characters.",
);
const REQUIRED_ARG: Arg = Arg::value(
    "required",
    "<true|false>",
    "Whether an entry must carry a value.",
);
const OPTION_ARG: Arg = Arg::repeated(
    "option",
    "<choice>",
    "One choice for a choice field. Repeat in order; replaces the whole list.",
);
const HELP_ARG: Arg = Arg::value(
    "help-text",
    "<text>",
    "Guidance shown under the field. Up to 500 characters.",
);
const PLACEHOLDER_ARG: Arg = Arg::value(
    "placeholder",
    "<text>",
    "Hint shown in an empty input. Up to 160 characters.",
);

fn common(inputs: &Inputs, arguments: &mut Map<String, Value>) -> Result<(), Failure> {
    if let Some(label) = inputs.value("label") {
        arguments.insert("label".into(), json!(crate::text(label, "label", 200)?));
    }
    if let Some(required) = inputs.value("required") {
        arguments.insert(
            "required".into(),
            json!(crate::boolean(required, "required")?),
        );
    }
    let options = inputs.repeated("option");
    if !options.is_empty() {
        arguments.insert("options".into(), json!(crate::options(options)?));
    }
    if let Some(help) = inputs.value("help-text") {
        arguments.insert(
            "helpText".into(),
            json!(crate::text(help, "help-text", 500)?),
        );
    }
    Ok(())
}

const WRITE_REFUSALS: &[Refusal] = &[
    crate::NOT_PAIRED,
    crate::AMBIGUOUS,
    crate::UNREACHABLE,
    crate::PAIRING_REJECTED,
    crate::SURVEY_REFUSED,
    crate::FORM_NOT_FOUND,
    crate::FIELD_NOT_FOUND,
    crate::FIELD_EXISTS,
    crate::VERSION_CONFLICT,
    crate::NOT_PERMITTED,
    crate::INVALID_FORM,
    crate::INVALID_FIELD,
    crate::INVALID_INPUT,
    crate::CONFIRMATION_REQUIRED,
    crate::UNSUPPORTED,
    crate::UNREADABLE,
    crate::SIGNED_OUT,
];

fn receipt(result: Value) -> Value {
    json!({
        "slug": result["slug"],
        "field": result["field"],
        "previous": result["previous"].as_object().map(|_| crate::form::read::field_row(&result["previous"])),
        "current": result["current"].as_object().map(|_| crate::form::read::field_row(&result["current"])),
        "version": result["version"],
        "field_total": result["fieldTotal"],
    })
}

fn render_change(data: &Value, verb: &str) -> String {
    format!(
        "{verb} field `{}` on {}  ·  now v{}  ·  {}\n",
        data["field"].as_str().unwrap_or("?"),
        data["slug"].as_str().unwrap_or("?"),
        data["version"].as_u64().unwrap_or(0),
        crate::plural(data["field_total"].as_u64().unwrap_or(0), "field"),
    )
}

pub mod add {
    use super::*;

    const KEY_ARG: Arg = Arg::value(
        "key",
        "<key>",
        "The new field's key: a letter, then letters, digits or '_'. Unique in the form.",
    )
    .required();
    const TYPE_ARG: Arg = Arg::value(
        "type",
        "<type>",
        "A Form Factory field type, e.g. text, number, dropdown, photo. The backend validates it.",
    )
    .required();

    pub static COMMAND: Command = Command {
        id: "survey.form.field.add",
        path: &["survey", "form", "field", "add"],
        contract: 1,
        summary: "Append one field to a form.",
        purpose: "\
Adds one field at the end of the form through the Form Factory's own update \
action. The field starts from the editor's defaults for its type; --label, \
--required, --option and --help-text set what the defaults leave open. A key \
already in the form is refused rather than duplicated, and a form saved by \
someone else since it was read is refused rather than merged.",
        effect: Effect::GlobalWrite,
        authority: Authority::Project,
        execution: Execution::Sync,
        args: &[
            FORM_ARG,
            KEY_ARG,
            TYPE_ARG,
            LABEL_ARG,
            REQUIRED_ARG,
            OPTION_ARG,
            HELP_ARG,
            DESCRIPTOR_ARG,
        ],
        output: "`slug`, the new `field` key, its `current` row, the form's new `version` and `field_total`.",
        examples: &[Example {
            command: "ds survey form field add --form edcl_customers_survey --key roof_type --type dropdown --label \"Roof type\" --option Iron --option Tile --yes",
            note: "Without --yes dispatch refuses before the bridge is opened.",
            runnable: false,
        }],
        refusals: WRITE_REFUSALS,
        reference: Some("docs/reference/survey.md"),
        availability: crate::paired_availability,
    };

    pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
        let mut arguments = Map::new();
        arguments.insert(
            "form".into(),
            json!(crate::form_slug(inputs.require("form")?)?),
        );
        arguments.insert(
            "key".into(),
            json!(crate::field_key(inputs.require("key")?)?),
        );
        let kind = inputs.require("type")?;
        if kind.is_empty()
            || kind.len() > 40
            || !kind.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return Err(Failure::invalid(
                "invalid_input",
                "`--type` must be a Form Factory field type name",
            )
            .remedy(crate::INVALID_INPUT.remedy));
        }
        arguments.insert("type".into(), json!(kind));
        common(inputs, &mut arguments)?;
        let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
        crate::invoke(
            &descriptor,
            &crate::FIELD_ADD,
            Value::Object(arguments),
            crate::WRITE_TIMEOUT,
        )
        .map(receipt)
        .map_err(crate::classify_survey_failure)
    }

    pub fn render(data: &Value) -> String {
        render_change(data, "added")
    }
}

pub mod set {
    use super::*;

    pub static COMMAND: Command = Command {
        id: "survey.form.field.set",
        path: &["survey", "form", "field", "set"],
        contract: 1,
        summary: "Change one field's label, requirement, choices or hints.",
        purpose: "\
Changes only the flags given, on one existing field, through the Form \
Factory's own update action. --option replaces the whole choice list in the \
order given. The key and the type never change here: renaming a key would \
orphan every entry already written under it, and changing a type is a new \
field. A form saved by someone else since it was read is refused, not merged.",
        effect: Effect::GlobalWrite,
        authority: Authority::Project,
        execution: Execution::Sync,
        args: &[
            FORM_ARG,
            FIELD_ARG,
            LABEL_ARG,
            REQUIRED_ARG,
            OPTION_ARG,
            HELP_ARG,
            PLACEHOLDER_ARG,
            DESCRIPTOR_ARG,
        ],
        output: "`slug`, `field`, the `previous` and `current` rows, the form's new `version` and `field_total`.",
        examples: &[Example {
            command: "ds survey form field set --form edcl_customers_survey --field meter_number --required true --label \"Meter number\" --yes",
            note: "Read the form first; the write is refused if the form moved since.",
            runnable: false,
        }],
        refusals: WRITE_REFUSALS,
        reference: Some("docs/reference/survey.md"),
        availability: crate::paired_availability,
    };

    pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
        let mut arguments = Map::new();
        arguments.insert(
            "form".into(),
            json!(crate::form_slug(inputs.require("form")?)?),
        );
        arguments.insert(
            "field".into(),
            json!(crate::field_key(inputs.require("field")?)?),
        );
        common(inputs, &mut arguments)?;
        if let Some(placeholder) = inputs.value("placeholder") {
            arguments.insert(
                "placeholder".into(),
                json!(crate::text(placeholder, "placeholder", 160)?),
            );
        }
        if arguments.len() == 2 {
            return Err(Failure::invalid("invalid_input", "nothing to change: give at least one of --label, --required, --option, --help-text, --placeholder")
                .remedy(crate::INVALID_INPUT.remedy));
        }
        let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
        crate::invoke(
            &descriptor,
            &crate::FIELD_SET,
            Value::Object(arguments),
            crate::WRITE_TIMEOUT,
        )
        .map(receipt)
        .map_err(crate::classify_survey_failure)
    }

    pub fn render(data: &Value) -> String {
        render_change(data, "changed")
    }
}

pub mod remove {
    use super::*;

    pub static COMMAND: Command = Command {
        id: "survey.form.field.remove",
        path: &["survey", "form", "field", "remove"],
        contract: 1,
        summary: "Remove one field from a form's schema.",
        purpose: "\
Removes the field from the schema through the Form Factory's own update \
action. Entries already written keep their stored value; new entries and the \
form editor stop showing the field. A form saved by someone else since it was \
read is refused, not merged.",
        effect: Effect::GlobalWrite,
        authority: Authority::Project,
        execution: Execution::Sync,
        args: &[FORM_ARG, FIELD_ARG, DESCRIPTOR_ARG],
        output: "`slug`, the removed `field` key and its `previous` row, the form's new `version` and `field_total`.",
        examples: &[Example {
            command: "ds survey form field remove --form edcl_customers_survey --field old_note --yes",
            note: "Schema only; stored entry values are untouched.",
            runnable: false,
        }],
        refusals: WRITE_REFUSALS,
        reference: Some("docs/reference/survey.md"),
        availability: crate::paired_availability,
    };

    pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
        let form = crate::form_slug(inputs.require("form")?)?;
        let field = crate::field_key(inputs.require("field")?)?;
        let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
        crate::invoke(
            &descriptor,
            &crate::FIELD_REMOVE,
            json!({ "form": form, "field": field }),
            crate::WRITE_TIMEOUT,
        )
        .map(receipt)
        .map_err(crate::classify_survey_failure)
    }

    pub fn render(data: &Value) -> String {
        render_change(data, "removed")
    }
}
