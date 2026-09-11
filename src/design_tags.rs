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

/// A bound field a definition carries. Which ones a value type HAS is this
/// module's, and it is read in two places that must not disagree: the editor
/// renders exactly these inputs, and `constraints_from_text` reads exactly
/// these members out of what an operator typed.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Bound {
    MaxLength,
    Min,
    Max,
}

/// The bound fields a definition of this value type carries, in offer order.
/// Anything outside this list is not a bound this type has — it is neither
/// read from what an operator typed nor admitted by `validate_definition`.
pub fn bounds_for(value_type: ValueType) -> &'static [Bound] {
    match value_type {
        ValueType::Text => &[Bound::MaxLength],
        ValueType::Integer | ValueType::Number => &[Bound::Min, Bound::Max],
        ValueType::Choice => &[],
    }
}

/// Does a definition of this value type carry a vocabulary? A type that does
/// REQUIRES one; a type that does not accepts none.
pub fn carries_values(value_type: ValueType) -> bool {
    matches!(value_type, ValueType::Choice)
}

/// Must this value type's bounds be whole numbers?
pub fn whole_number_bounds(value_type: ValueType) -> bool {
    matches!(value_type, ValueType::Integer)
}

/// Every cardinality this value type may use, in offer order — the pairs the
/// matrix leaves a control for. A type with one is a type an editor offers no
/// cardinality choice for at all.
pub fn cardinalities_for(value_type: ValueType) -> Vec<Cardinality> {
    [Cardinality::Single, Cardinality::Multiple]
        .into_iter()
        .filter(|cardinality| !controls_for(value_type, *cardinality).is_empty())
        .collect()
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
/// member that carries it; `key` is the sentence the host renders it under.
/// None of the three is prose.
///
/// The key travels WITH the refusal because it is derived from the condition
/// and the member — `design_tag_bounds_not_finite` reads as the min sentence
/// or the max one depending on `field` — and a host switch over codes is a
/// second reading of that rule, free to drift from it and free to fall through
/// to a generic sentence for a condition named later.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Refusal {
    pub code: &'static str,
    pub field: &'static str,
    pub key: &'static str,
}

impl Refusal {
    fn new(code: &'static str, field: &'static str) -> Self {
        Self {
            code,
            field,
            key: refusal_key(code, field),
        }
    }
}

/// The i18n key each refusal is shown under.
fn refusal_key(code: &str, field: &str) -> &'static str {
    match code {
        "design_tag_control_incompatible" => "pctl_tags_invalid_control",
        "design_tag_cardinality_unsupported" => "pctl_tags_invalid_cardinality",
        "design_tag_values_required"
        | "design_tag_values_forbidden"
        | "design_tag_values_bounded" => "pctl_tags_invalid_values",
        "design_tag_bounds_inverted" => "pctl_tags_invalid_range",
        "design_tag_bounds_not_finite" if field == "constraints.max" => "pctl_tags_invalid_max",
        "design_tag_bounds_not_finite" => "pctl_tags_invalid_min",
        "design_tag_bounds_not_integer" => "pctl_tags_invalid_integer",
        "design_tag_max_length_bounded" => "pctl_tags_invalid_max_length",
        "design_tag_constraints_forbidden" => "pctl_tags_invalid_constraints",
        _ => "pctl_tags_invalid_definition",
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
    if carries_values(value_type) {
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
    // WHICH bounds a type carries is `bounds_for`'s, read here rather than
    // restated: a bound the type does not have is one the editor never offers,
    // `constraints_from_text` never reads, and this refuses.
    let carried = bounds_for(value_type);
    if !carried.contains(&Bound::Min) && (constraints.min.is_some() || constraints.max.is_some()) {
        return Err(Refusal::new(
            "design_tag_constraints_forbidden",
            "constraints.min",
        ));
    }
    if !carried.contains(&Bound::MaxLength) && constraints.max_length.is_some() {
        return Err(Refusal::new(
            "design_tag_constraints_forbidden",
            "constraints.max_length",
        ));
    }
    if carried.contains(&Bound::MaxLength) {
        let length = constraints.max_length.unwrap_or(MAX_TEXT_LENGTH);
        if length == 0 || length > MAX_TEXT_LENGTH {
            return Err(Refusal::new(
                "design_tag_max_length_bounded",
                "constraints.max_length",
            ));
        }
        constraints.max_length = Some(length);
    }
    if whole_number_bounds(value_type) {
        for (bound, field) in [
            (constraints.min, "constraints.min"),
            (constraints.max, "constraints.max"),
        ] {
            if bound.is_some_and(|value| value.trunc() != value) {
                return Err(Refusal::new("design_tag_bounds_not_integer", field));
            }
        }
    }
    Ok(constraints)
}

/// What an operator typed into the three bound fields, before any of it has
/// been read as a bound. Absent and blank are the same thing: a field nobody
/// filled in.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TypedBounds {
    #[serde(default)]
    pub min: Option<String>,
    #[serde(default)]
    pub max: Option<String>,
    #[serde(default)]
    pub max_length: Option<String>,
}

/// The bounds a definition of this value type carries, read from what an
/// operator typed. `Err` is the i18n key the refusal is shown under.
///
/// Turning text into a number is a boundary — an editor's input and a
/// `--min` flag both arrive as text. WHICH bounds a value type has, what a
/// blank field means, and what an unreadable one costs are decisions, and they
/// were made in the browser: a bound that is not a number became `NaN`, which
/// `JSON.stringify` writes as `null` and this module reads as "no bound at
/// all", so a bound the operator typed was silently dropped from a definition
/// that then saved cleanly.
///
/// A `max_length` is refused unless it is a whole number a length can be:
/// the browser passed `2.5` and `-5` straight through to a decoder that could
/// not read them, so the editor raised an exception where a refusal belongs.
/// Whether a readable bound is ADMISSIBLE stays `validate_definition`'s — this
/// reads `0.5` as a number for an integer definition and lets that verdict
/// name it.
pub fn constraints_from_text(
    value_type: ValueType,
    typed: &TypedBounds,
) -> Result<Constraints, &'static str> {
    let mut constraints = Constraints::default();
    for bound in bounds_for(value_type) {
        match bound {
            Bound::MaxLength => {
                if let Some(text) = filled(typed.max_length.as_deref()) {
                    constraints.max_length = Some(read_length(text)?);
                }
            }
            Bound::Min => {
                constraints.min = read_bound(typed.min.as_deref(), "pctl_tags_invalid_min")?;
            }
            Bound::Max => {
                constraints.max = read_bound(typed.max.as_deref(), "pctl_tags_invalid_max")?;
            }
        }
    }
    Ok(constraints)
}

/// The vocabulary an operator typed, read as the list a definition carries.
///
/// Three decisions an editor held: a value ends at a newline or a comma,
/// space around it is not part of it, and a value typed twice is one value —
/// the first spelling stays, so what the operator typed keeps its order. The
/// browser deduplicated before calling and `ds design tag define` did not, so
/// the same vocabulary passed `MAX_VALUES` at one door and was refused at the
/// other. Whether the list that comes out is ADMISSIBLE stays
/// `validate_definition`'s; this only reads it.
pub fn values_from_text(text: &str) -> Vec<String> {
    let mut values: Vec<String> = Vec::new();
    for part in text.split(['\n', ',']) {
        let value = part.trim();
        if value.is_empty() || values.iter().any(|held| held == value) {
            continue;
        }
        values.push(value.to_owned());
    }
    values
}

fn filled(text: Option<&str>) -> Option<&str> {
    text.map(str::trim).filter(|text| !text.is_empty())
}

fn read_bound(text: Option<&str>, key: &'static str) -> Result<Option<f64>, &'static str> {
    let Some(text) = filled(text) else {
        return Ok(None);
    };
    match text.parse::<f64>() {
        Ok(value) if value.is_finite() => Ok(Some(value)),
        _ => Err(key),
    }
}

fn read_length(text: &str) -> Result<u32, &'static str> {
    const KEY: &str = "pctl_tags_invalid_max_length";
    let value = text.parse::<f64>().map_err(|_| KEY)?;
    if !value.is_finite() || value.fract() != 0.0 || value < 0.0 || value > f64::from(u32::MAX) {
        return Err(KEY);
    }
    Ok(value as u32)
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
    /// What shape does a definition of this value type have, before any of it
    /// is filled in?
    Shape { value_type: ValueType },
    /// Is this definition valid, and what does it resolve to?
    Validate { definition: Definition },
    /// What vocabulary does this typed text name?
    ValuesFromText {
        #[serde(default)]
        text: String,
    },
    /// What do these typed bounds read as for a definition of this type?
    ConstraintsFromText {
        value_type: ValueType,
        #[serde(default)]
        min: Option<String>,
        #[serde(default)]
        max: Option<String>,
        #[serde(default)]
        max_length: Option<String>,
    },
}

#[derive(Serialize)]
struct ControlsReply {
    controls: Vec<Control>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_control: Option<Control>,
}

/// What an editor may offer for one value type. Every member is read from the
/// same rule the verdict reads, so a field the editor renders is a field a
/// definition may carry and a cardinality it offers is one the matrix has a
/// control for.
#[derive(Serialize)]
struct ShapeReply {
    cardinalities: Vec<Cardinality>,
    /// The one to fall back to when the current choice is not among them.
    #[serde(skip_serializing_if = "Option::is_none")]
    default_cardinality: Option<Cardinality>,
    carries_values: bool,
    bounds: &'static [Bound],
    whole_number_bounds: bool,
}

#[derive(Serialize)]
struct TypedBoundsReply {
    #[serde(skip_serializing_if = "Option::is_none")]
    constraints: Option<Constraints>,
    #[serde(skip_serializing_if = "Option::is_none")]
    refusal_key: Option<&'static str>,
}

#[derive(Serialize)]
struct ValuesReply {
    values: Vec<String>,
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
        Request::Shape { value_type } => {
            let cardinalities = cardinalities_for(value_type);
            serde_json::to_string(&ShapeReply {
                default_cardinality: cardinalities.first().copied(),
                cardinalities,
                carries_values: carries_values(value_type),
                bounds: bounds_for(value_type),
                whole_number_bounds: whole_number_bounds(value_type),
            })
        }
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
        Request::ValuesFromText { text } => serde_json::to_string(&ValuesReply {
            values: values_from_text(&text),
        }),
        Request::ConstraintsFromText {
            value_type,
            min,
            max,
            max_length,
        } => {
            let typed = TypedBounds {
                min,
                max,
                max_length,
            };
            match constraints_from_text(value_type, &typed) {
                Ok(constraints) => serde_json::to_string(&TypedBoundsReply {
                    constraints: Some(constraints),
                    refusal_key: None,
                }),
                Err(key) => serde_json::to_string(&TypedBoundsReply {
                    constraints: None,
                    refusal_key: Some(key),
                }),
            }
        }
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

    fn typed(min: &str, max: &str, max_length: &str) -> TypedBounds {
        TypedBounds {
            min: Some(min.into()),
            max: Some(max.into()),
            max_length: Some(max_length.into()),
        }
    }

    /// What an operator typed, read once — the editor and `ds design tag
    /// define` both ask this rather than parsing on their own side.
    #[test]
    fn typed_bounds_are_read_per_value_type() {
        // A bound belongs only to the type that has one.
        assert_eq!(
            constraints_from_text(ValueType::Number, &typed("0", "100", "")),
            Ok(Constraints {
                min: Some(0.0),
                max: Some(100.0),
                max_length: None,
            })
        );
        assert_eq!(
            constraints_from_text(ValueType::Text, &typed("1", "2", " 120 ")),
            Ok(Constraints {
                min: None,
                max: None,
                max_length: Some(120),
            })
        );
        assert_eq!(
            constraints_from_text(ValueType::Choice, &typed("1", "2", "5")),
            Ok(Constraints::default())
        );
        // Blank is a field nobody filled in, not a bound of zero.
        assert_eq!(
            constraints_from_text(ValueType::Integer, &typed("  ", "", "")),
            Ok(Constraints::default())
        );
        // Decimal text is a number; whether an INTEGER definition may carry it
        // is the verdict's, not the reading's.
        assert_eq!(
            constraints_from_text(ValueType::Integer, &typed("0.5", "", "")),
            Ok(Constraints {
                min: Some(0.5),
                max: None,
                max_length: None,
            })
        );
    }

    #[test]
    fn an_unreadable_bound_is_refused_under_its_own_key() {
        for (value_type, bounds, key) in [
            (
                ValueType::Number,
                typed("abc", "", ""),
                "pctl_tags_invalid_min",
            ),
            (
                ValueType::Integer,
                typed("", "1,5", ""),
                "pctl_tags_invalid_max",
            ),
            // Beyond f64 is not a finite bound.
            (
                ValueType::Number,
                typed("1e400", "", ""),
                "pctl_tags_invalid_min",
            ),
            (
                ValueType::Text,
                typed("", "", "2.5"),
                "pctl_tags_invalid_max_length",
            ),
            (
                ValueType::Text,
                typed("", "", "abc"),
                "pctl_tags_invalid_max_length",
            ),
            // A length no length can be: the browser handed both straight to a
            // decoder that raised instead of refusing.
            (
                ValueType::Text,
                typed("", "", "-5"),
                "pctl_tags_invalid_max_length",
            ),
            (
                ValueType::Text,
                typed("", "", "1e30"),
                "pctl_tags_invalid_max_length",
            ),
        ] {
            assert_eq!(constraints_from_text(value_type, &bounds), Err(key));
        }
    }

    /// The key is the refusal's own, so no host switches over codes to find it.
    #[test]
    fn every_refusal_names_the_sentence_it_is_shown_under() {
        let mut inverted = definition(ValueType::Number, Cardinality::Single);
        inverted.constraints.min = Some(10.0);
        inverted.constraints.max = Some(1.0);
        assert_eq!(
            validate_definition(&inverted).unwrap_err().key,
            "pctl_tags_invalid_range"
        );
        let mut not_finite = definition(ValueType::Number, Cardinality::Single);
        not_finite.constraints.max = Some(f64::INFINITY);
        assert_eq!(
            validate_definition(&not_finite).unwrap_err().key,
            "pctl_tags_invalid_max"
        );
        let mut min_not_finite = definition(ValueType::Number, Cardinality::Single);
        min_not_finite.constraints.min = Some(f64::NAN);
        assert_eq!(
            validate_definition(&min_not_finite).unwrap_err().key,
            "pctl_tags_invalid_min"
        );
        let multiple = definition(ValueType::Text, Cardinality::Multiple);
        assert_eq!(
            validate_definition(&multiple).unwrap_err().key,
            "pctl_tags_invalid_cardinality"
        );
        assert_eq!(
            refusal_key("design_tag_named_later", "values"),
            "pctl_tags_invalid_definition"
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
        assert!(reply.contains(r#""key":"pctl_tags_invalid_control""#));
        let reply = evaluate(
            br#"{"op":"constraints_from_text","value_type":"number","min":"0","max":"100"}"#,
        )
        .unwrap();
        assert_eq!(reply, r#"{"constraints":{"min":0.0,"max":100.0}}"#);
        let reply =
            evaluate(br#"{"op":"constraints_from_text","value_type":"text","max_length":"x"}"#)
                .unwrap();
        assert_eq!(reply, r#"{"refusal_key":"pctl_tags_invalid_max_length"}"#);
        let reply = evaluate(br#"{"op":"constraints_from_text","value_type":"choice"}"#).unwrap();
        assert_eq!(reply, r#"{"constraints":{}}"#);
        let reply = evaluate(br#"{"op":"shape","value_type":"integer"}"#).unwrap();
        assert_eq!(
            reply,
            r#"{"cardinalities":["single"],"default_cardinality":"single","carries_values":false,"bounds":["min","max"],"whole_number_bounds":true}"#
        );
        assert!(evaluate(br#"{"op":"nope"}"#).is_err());
    }

    /// A vocabulary is what the operator typed, not what an editor happened to
    /// split it into: the separators, the trimming and "typed twice is one
    /// value" are all this module's, and a CLI door that skipped them refused
    /// a list the editor accepted.
    #[test]
    fn a_typed_vocabulary_keeps_its_order_and_loses_its_repeats() {
        assert_eq!(values_from_text(""), Vec::<String>::new());
        assert_eq!(values_from_text("   \n , \n"), Vec::<String>::new());
        assert_eq!(
            values_from_text(" b , a\nb\n\nc "),
            vec!["b".to_owned(), "a".to_owned(), "c".to_owned()]
        );
        // A textarea on Windows sends CRLF; the carriage return is space.
        assert_eq!(
            values_from_text("a\r\nb"),
            vec!["a".to_owned(), "b".to_owned()]
        );
        // The separator is the pair, not either one alone.
        assert_eq!(values_from_text("a;b"), vec!["a;b".to_owned()]);
        let reply = evaluate(br#"{"op":"values_from_text","text":"a, a ,b"}"#).unwrap();
        assert_eq!(reply, r#"{"values":["a","b"]}"#);
        let reply = evaluate(br#"{"op":"values_from_text"}"#).unwrap();
        assert_eq!(reply, r#"{"values":[]}"#);
    }

    /// The shape an editor renders from, stated as the table it is: which
    /// cardinalities a type may use, whether it carries a vocabulary, and
    /// which bound fields it has.
    #[test]
    fn the_shape_is_the_matrixs_shape() {
        use Cardinality::{Multiple, Single};
        let table: &[(ValueType, &[Cardinality], bool, &[Bound], bool)] = &[
            (ValueType::Choice, &[Single, Multiple], true, &[], false),
            (
                ValueType::Text,
                &[Single],
                false,
                &[Bound::MaxLength],
                false,
            ),
            (
                ValueType::Integer,
                &[Single],
                false,
                &[Bound::Min, Bound::Max],
                true,
            ),
            (
                ValueType::Number,
                &[Single],
                false,
                &[Bound::Min, Bound::Max],
                false,
            ),
        ];
        for (value_type, cardinalities, values, bounds, whole) in table {
            assert_eq!(
                &cardinalities_for(*value_type),
                cardinalities,
                "{value_type:?}"
            );
            assert_eq!(carries_values(*value_type), *values, "{value_type:?}");
            assert_eq!(bounds_for(*value_type), *bounds, "{value_type:?}");
            assert_eq!(whole_number_bounds(*value_type), *whole, "{value_type:?}");
        }
    }

    /// The shape is not a second table: a cardinality it omits has no control,
    /// a bound it omits is refused as forbidden, and a bound it names survives
    /// `constraints_from_text`. An editor built from it cannot compose a
    /// definition this module then refuses for its shape.
    #[test]
    fn the_shape_agrees_with_the_verdict() {
        for value_type in [
            ValueType::Choice,
            ValueType::Text,
            ValueType::Integer,
            ValueType::Number,
        ] {
            for cardinality in [Cardinality::Single, Cardinality::Multiple] {
                let offered = cardinalities_for(value_type).contains(&cardinality);
                assert_eq!(offered, !controls_for(value_type, cardinality).is_empty());
                let mut definition = definition(value_type, cardinality);
                definition.values = if carries_values(value_type) {
                    vec!["a".into()]
                } else {
                    Vec::new()
                };
                assert_eq!(validate_definition(&definition).is_ok(), offered);
            }
            let typed = TypedBounds {
                min: Some("2".into()),
                max: Some("4".into()),
                max_length: Some("7".into()),
            };
            let read = constraints_from_text(value_type, &typed).unwrap();
            let carried = bounds_for(value_type);
            assert_eq!(read.min.is_some(), carried.contains(&Bound::Min));
            assert_eq!(read.max.is_some(), carried.contains(&Bound::Max));
            assert_eq!(
                read.max_length.is_some(),
                carried.contains(&Bound::MaxLength)
            );
            let mut definition = definition(value_type, Cardinality::Single);
            definition.constraints = read;
            assert!(validate_definition(&definition).is_ok(), "{value_type:?}");
        }
    }
}
