//! Project tag definitions — the ONE compatibility matrix.
//!
//! A tag definition declares a value type, a cardinality and the input control
//! an operator uses to supply values. Which controls a (value_type,
//! cardinality) pair may use was decided three times: once as a Svelte
//! `$derived` in the tag editor, once as a flat vocabulary check in the CLI
//! executor (which enforced no compatibility rule at all), and once in the
//! grant. The grant keeps its copy — it is the security boundary — and the two
//! client copies collapse here.
//!
//! Everything this module emits is a key or a stable code. The words an
//! operator reads are the host's.
use serde::{Deserialize, Serialize};

pub const SCHEMA: &str = "design_tags/1";

/// Upper bound on a choice vocabulary, mirroring the grant's own.
pub const MAX_VALUES: usize = 100;
/// Upper bound on a text tag's `max_length`, mirroring the grant's own.
pub const MAX_TEXT_LENGTH: u32 = 500;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ValueType {
    Choice,
    Text,
    Integer,
    Number,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Cardinality {
    Single,
    Multiple,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Control {
    Radio,
    Dropdown,
    Multiselect,
    Text,
    Number,
}

/// Every control a (value_type, cardinality) pair may use, in the order the
/// editor offers them: the first is the default the grant applies when the
/// caller names none. An empty list means the pair itself is unusable.
pub fn controls_for(value_type: ValueType, cardinality: Cardinality) -> &'static [Control] {
    match (value_type, cardinality) {
        (ValueType::Choice, Cardinality::Multiple) => &[Control::Multiselect],
        (ValueType::Choice, Cardinality::Single) => &[Control::Dropdown, Control::Radio],
        (_, Cardinality::Multiple) => &[],
        (ValueType::Text, Cardinality::Single) => &[Control::Text],
        (ValueType::Integer | ValueType::Number, Cardinality::Single) => &[Control::Number],
    }
}

/// The control the grant stores when the caller names none.
pub fn default_control(value_type: ValueType, cardinality: Cardinality) -> Option<Control> {
    controls_for(value_type, cardinality).first().copied()
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Constraints {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_length: Option<u32>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub value_type: ValueType,
    #[serde(default = "single")]
    pub cardinality: Cardinality,
    /// Absent means "the default for this pair", exactly as the grant reads it.
    #[serde(default)]
    pub input_control: Option<Control>,
    /// The choice vocabulary in display order; empty for every other type.
    #[serde(default)]
    pub values: Vec<String>,
    #[serde(default)]
    pub constraints: Constraints,
}

fn single() -> Cardinality {
    Cardinality::Single
}

/// A refused definition. `code` is the stable condition; `field` names the
/// member that carries it. Neither is prose.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Refusal {
    pub code: &'static str,
    pub field: &'static str,
}

impl Refusal {
    const fn new(code: &'static str, field: &'static str) -> Self {
        Self { code, field }
    }
}

/// What the definition resolves to once the matrix has been applied.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Resolved {
    pub value_type: ValueType,
    pub cardinality: Cardinality,
    pub input_control: Control,
    /// Every control this pair may use, in offer order.
    pub controls: Vec<Control>,
    pub constraints: Constraints,
}

/// Apply the matrix and the constraint rules. The order is the grant's own:
/// cardinality, then control, then the vocabulary, then the bounds — so a
/// definition wrong in two ways is refused under the same code on both sides.
pub fn validate_definition(definition: &Definition) -> Result<Resolved, Refusal> {
    let Definition {
        value_type,
        cardinality,
        input_control,
        values,
        constraints,
    } = definition;
    let (value_type, cardinality) = (*value_type, *cardinality);
    let offered = controls_for(value_type, cardinality);
    if offered.is_empty() {
        return Err(Refusal::new(
            "design_tag_cardinality_unsupported",
            "cardinality",
        ));
    }
    let control = match input_control {
        Some(control) if !offered.contains(control) => {
            return Err(Refusal::new(
                "design_tag_control_incompatible",
                "input_control",
            ));
        }
        Some(control) => *control,
        None => offered[0],
    };
    if matches!(value_type, ValueType::Choice) {
        if values.is_empty() {
            return Err(Refusal::new("design_tag_values_required", "values"));
        }
        if values.len() > MAX_VALUES {
            return Err(Refusal::new("design_tag_values_bounded", "values"));
        }
    } else if !values.is_empty() {
        return Err(Refusal::new("design_tag_values_forbidden", "values"));
    }
    let constraints = validate_constraints(value_type, *constraints)?;
    Ok(Resolved {
        value_type,
        cardinality,
        input_control: control,
        controls: offered.to_vec(),
        constraints,
    })
}

fn validate_constraints(
    value_type: ValueType,
    mut constraints: Constraints,
) -> Result<Constraints, Refusal> {
    for (bound, field) in [
        (constraints.min, "constraints.min"),
        (constraints.max, "constraints.max"),
    ] {
        if bound.is_some_and(|value| !value.is_finite()) {
            return Err(Refusal::new("design_tag_bounds_not_finite", field));
        }
    }
    if let (Some(min), Some(max)) = (constraints.min, constraints.max) {
        if min > max {
            return Err(Refusal::new(
                "design_tag_bounds_inverted",
                "constraints.min",
            ));
        }
    }
    match value_type {
        ValueType::Choice => {
            if constraints.min.is_some() || constraints.max.is_some() {
                return Err(Refusal::new(
                    "design_tag_constraints_forbidden",
                    "constraints.min",
                ));
            }
            if constraints.max_length.is_some() {
                return Err(Refusal::new(
                    "design_tag_constraints_forbidden",
                    "constraints.max_length",
                ));
            }
        }
        ValueType::Text => {
            if constraints.min.is_some() || constraints.max.is_some() {
                return Err(Refusal::new(
                    "design_tag_constraints_forbidden",
                    "constraints.min",
                ));
            }
            let length = constraints.max_length.unwrap_or(MAX_TEXT_LENGTH);
            if length == 0 || length > MAX_TEXT_LENGTH {
                return Err(Refusal::new(
                    "design_tag_max_length_bounded",
                    "constraints.max_length",
                ));
            }
            constraints.max_length = Some(length);
        }
        ValueType::Integer | ValueType::Number => {
            if constraints.max_length.is_some() {
                return Err(Refusal::new(
                    "design_tag_constraints_forbidden",
                    "constraints.max_length",
                ));
            }
            if matches!(value_type, ValueType::Integer) {
                for (bound, field) in [
                    (constraints.min, "constraints.min"),
                    (constraints.max, "constraints.max"),
                ] {
                    if bound.is_some_and(|value| value.trunc() != value) {
                        return Err(Refusal::new("design_tag_bounds_not_integer", field));
                    }
                }
            }
        }
    }
    Ok(constraints)
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    /// What may this pair use, and what does it default to?
    Controls {
        value_type: ValueType,
        #[serde(default = "single")]
        cardinality: Cardinality,
    },
    /// Is this definition valid, and what does it resolve to?
    Validate { definition: Definition },
}

#[derive(Serialize)]
struct ControlsReply {
    controls: Vec<Control>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_control: Option<Control>,
}

#[derive(Serialize)]
struct ValidateReply {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    resolved: Option<Resolved>,
    #[serde(skip_serializing_if = "Option::is_none")]
    refusal: Option<Refusal>,
}

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > 64 * 1024 {
        return Err("design_tags_input_too_large".into());
    }
    let request: Request = serde_json::from_slice(input)
        .map_err(|error| format!("design_tags_request_invalid: {error}"))?;
    let reply = match request {
        Request::Controls {
            value_type,
            cardinality,
        } => serde_json::to_string(&ControlsReply {
            controls: controls_for(value_type, cardinality).to_vec(),
            default_control: default_control(value_type, cardinality),
        }),
        Request::Validate { definition } => match validate_definition(&definition) {
            Ok(resolved) => serde_json::to_string(&ValidateReply {
                ok: true,
                resolved: Some(resolved),
                refusal: None,
            }),
            Err(refusal) => serde_json::to_string(&ValidateReply {
                ok: false,
                resolved: None,
                refusal: Some(refusal),
            }),
        },
    };
    reply.map_err(|_| "design_tags_reply_invalid".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn definition(value_type: ValueType, cardinality: Cardinality) -> Definition {
        Definition {
            value_type,
            cardinality,
            input_control: None,
            values: if matches!(value_type, ValueType::Choice) {
                vec!["a".into()]
            } else {
                Vec::new()
            },
            constraints: Constraints::default(),
        }
    }

    /// The matrix, stated once as a table and asserted whole — this is the
    /// answer the editor's disabled options and the CLI's refusal both read.
    #[test]
    fn the_matrix_is_the_grants_matrix() {
        use Cardinality::{Multiple, Single};
        let table: &[(ValueType, Cardinality, &[Control])] = &[
            (ValueType::Choice, Multiple, &[Control::Multiselect]),
            (
                ValueType::Choice,
                Single,
                &[Control::Dropdown, Control::Radio],
            ),
            (ValueType::Text, Single, &[Control::Text]),
            (ValueType::Text, Multiple, &[]),
            (ValueType::Integer, Single, &[Control::Number]),
            (ValueType::Integer, Multiple, &[]),
            (ValueType::Number, Single, &[Control::Number]),
            (ValueType::Number, Multiple, &[]),
        ];
        for (value_type, cardinality, controls) in table {
            assert_eq!(controls_for(*value_type, *cardinality), *controls);
        }
        assert_eq!(
            default_control(ValueType::Choice, Single),
            Some(Control::Dropdown)
        );
        assert_eq!(
            default_control(ValueType::Choice, Multiple),
            Some(Control::Multiselect)
        );
        assert_eq!(default_control(ValueType::Text, Multiple), None);
    }

    /// Every combination is decided; a control off the pair's list is refused
    /// under one code, which is precisely the loose CLI acceptance this closes.
    #[test]
    fn every_combination_is_decided() {
        for value_type in [
            ValueType::Choice,
            ValueType::Text,
            ValueType::Integer,
            ValueType::Number,
        ] {
            for cardinality in [Cardinality::Single, Cardinality::Multiple] {
                for control in [
                    Control::Radio,
                    Control::Dropdown,
                    Control::Multiselect,
                    Control::Text,
                    Control::Number,
                ] {
                    let mut definition = definition(value_type, cardinality);
                    definition.input_control = Some(control);
                    let offered = controls_for(value_type, cardinality);
                    match validate_definition(&definition) {
                        Ok(resolved) => {
                            assert!(offered.contains(&control));
                            assert_eq!(resolved.input_control, control);
                        }
                        Err(refusal) => assert!(
                            !offered.contains(&control)
                                || refusal.code == "design_tag_cardinality_unsupported"
                        ),
                    }
                }
            }
        }
    }

    #[test]
    fn a_choice_owns_a_vocabulary_and_nothing_else() {
        let mut choice = definition(ValueType::Choice, Cardinality::Single);
        choice.values.clear();
        assert_eq!(
            validate_definition(&choice).unwrap_err().code,
            "design_tag_values_required"
        );
        choice.values = (0..=MAX_VALUES).map(|index| index.to_string()).collect();
        assert_eq!(
            validate_definition(&choice).unwrap_err().code,
            "design_tag_values_bounded"
        );
        choice.values = vec!["a".into()];
        choice.constraints.max_length = Some(12);
        assert_eq!(
            validate_definition(&choice).unwrap_err().code,
            "design_tag_constraints_forbidden"
        );
        let mut text = definition(ValueType::Text, Cardinality::Single);
        text.values = vec!["a".into()];
        assert_eq!(
            validate_definition(&text).unwrap_err().code,
            "design_tag_values_forbidden"
        );
    }

    #[test]
    fn bounds_are_finite_ordered_and_typed() {
        let mut number = definition(ValueType::Number, Cardinality::Single);
        number.constraints.min = Some(f64::NAN);
        assert_eq!(
            validate_definition(&number).unwrap_err().code,
            "design_tag_bounds_not_finite"
        );
        number.constraints.min = Some(10.0);
        number.constraints.max = Some(1.0);
        assert_eq!(
            validate_definition(&number).unwrap_err().code,
            "design_tag_bounds_inverted"
        );
        number.constraints.max = Some(100.0);
        assert!(validate_definition(&number).is_ok());
        let mut integer = definition(ValueType::Integer, Cardinality::Single);
        integer.constraints.min = Some(0.5);
        assert_eq!(
            validate_definition(&integer).unwrap_err().code,
            "design_tag_bounds_not_integer"
        );
    }

    /// An unbounded text tag resolves to the grant's own default length, so a
    /// definition written headlessly and one written in the editor store the
    /// same document.
    #[test]
    fn text_length_defaults_to_the_grants_bound() {
        let text = definition(ValueType::Text, Cardinality::Single);
        let resolved = validate_definition(&text).unwrap();
        assert_eq!(resolved.constraints.max_length, Some(MAX_TEXT_LENGTH));
        let mut over = text.clone();
        over.constraints.max_length = Some(MAX_TEXT_LENGTH + 1);
        assert_eq!(
            validate_definition(&over).unwrap_err().code,
            "design_tag_max_length_bounded"
        );
    }

    #[test]
    fn evaluate_answers_both_questions() {
        let reply =
            evaluate(br#"{"op":"controls","value_type":"choice","cardinality":"multiple"}"#)
                .unwrap();
        assert_eq!(
            reply,
            r#"{"controls":["multiselect"],"default_control":"multiselect"}"#
        );
        let reply = evaluate(
            br#"{"op":"validate","definition":{"value_type":"text","input_control":"number"}}"#,
        )
        .unwrap();
        assert!(reply.contains("design_tag_control_incompatible"));
        assert!(evaluate(br#"{"op":"nope"}"#).is_err());
    }
}
