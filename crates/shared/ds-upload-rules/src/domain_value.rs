//! Single-cell domain authority shared by cleaning and browser editors.
use crate::{
    ColumnPlanDomain, clean_domain_value, coerce_domain_value, domain_confirms_value,
    parse_numeric_domain_value, value_as_text,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, PartialEq)]
pub enum DomainValueError {
    InvalidRequest(String),
    InvalidResponse(String),
}
impl std::fmt::Display for DomainValueError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRequest(message) => write!(out, "domain_value_request_invalid: {message}"),
            Self::InvalidResponse(message) => {
                write!(out, "domain_value_response_invalid: {message}")
            }
        }
    }
}
impl std::error::Error for DomainValueError {}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "bindings", derive(tsify::Tsify))]
pub enum DomainValueAction {
    Clean,
    Coerce,
    Confirms,
    Numeric,
    Other,
    OffersOther,
    Format,
    Options,
    Type,
    Values,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
#[cfg_attr(feature = "bindings", derive(tsify::Tsify))]
pub enum DomainScalar {
    Text(String),
    Number(#[cfg_attr(feature = "bindings", tsify(type = "number"))] serde_json::Number),
    Boolean(bool),
}

impl TryFrom<Value> for DomainScalar {
    type Error = DomainValueError;
    fn try_from(value: Value) -> Result<Self, Self::Error> {
        match value {
            Value::String(value) => Ok(Self::Text(value)),
            Value::Number(value) => Ok(Self::Number(value)),
            Value::Bool(value) => Ok(Self::Boolean(value)),
            _ => Err(DomainValueError::InvalidResponse(
                "coercion must return a scalar".into(),
            )),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "bindings", derive(tsify::Tsify))]
pub struct DomainValueOption {
    pub value: String,
    pub label: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "action", content = "value", rename_all = "snake_case")]
#[cfg_attr(feature = "bindings", derive(tsify::Tsify))]
#[cfg_attr(feature = "bindings", tsify(hashmap_as_object, missing_as_null))]
pub enum DomainValueResponse {
    Clean(Value),
    Coerce(DomainScalar),
    Confirms(bool),
    Numeric(Option<f64>),
    Other(Option<f64>),
    OffersOther(bool),
    Format(String),
    Options(Vec<DomainValueOption>),
    Type(Option<DomainType>),
    Values(Vec<DomainScalar>),
}

impl DomainValueResponse {
    pub fn action(&self) -> DomainValueAction {
        match self {
            Self::Clean(_) => DomainValueAction::Clean,
            Self::Coerce(_) => DomainValueAction::Coerce,
            Self::Confirms(_) => DomainValueAction::Confirms,
            Self::Numeric(_) => DomainValueAction::Numeric,
            Self::Other(_) => DomainValueAction::Other,
            Self::OffersOther(_) => DomainValueAction::OffersOther,
            Self::Format(_) => DomainValueAction::Format,
            Self::Options(_) => DomainValueAction::Options,
            Self::Type(_) => DomainValueAction::Type,
            Self::Values(_) => DomainValueAction::Values,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "bindings", derive(tsify::Tsify))]
pub enum DomainType {
    CategoricalString,
    CategoricalInt,
    CategoricalNumber,
    Boolean,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "bindings", derive(tsify::Tsify))]
#[cfg_attr(feature = "bindings", tsify(hashmap_as_object))]
pub struct DomainValueInput {
    pub action: DomainValueAction,
    pub domain: Option<ColumnPlanDomain>,
    #[serde(default)]
    pub value: Value,
}

pub fn validate_response(
    request: &DomainValueInput,
    response: DomainValueResponse,
) -> Result<DomainValueResponse, DomainValueError> {
    if request.action != response.action() {
        return Err(DomainValueError::InvalidResponse(
            "answer belongs to a different operation".into(),
        ));
    }
    if matches!(&response, DomainValueResponse::Numeric(Some(value)) | DomainValueResponse::Other(Some(value)) if !value.is_finite())
    {
        return Err(DomainValueError::InvalidResponse(
            "numeric result must be finite".into(),
        ));
    }
    Ok(response)
}

pub fn evaluate(input: DomainValueInput) -> Result<DomainValueResponse, DomainValueError> {
    use DomainValueAction as Action;
    use DomainValueResponse as Response;
    let domain = input.domain.as_ref();
    let value = &input.value;
    let required =
        || domain.ok_or_else(|| DomainValueError::InvalidRequest("domain is required".into()));
    Ok(match input.action {
        Action::Clean => Response::Clean(clean_domain_value(required()?, value)),
        Action::Coerce => Response::Coerce(coerce_domain_value(required()?, value).try_into()?),
        Action::Confirms => {
            Response::Confirms(domain.is_some_and(|d| domain_confirms_value(d, value)))
        }
        Action::Numeric => Response::Numeric(parse_numeric_domain_value(value)),
        Action::Other => {
            let d = required()?;
            Response::Other(parse_numeric_domain_value(value).filter(|n| {
                *n > 0.0
                    && (d.kind == "categorical_number"
                        || (d.kind == "categorical_int" && n.fract() == 0.0))
            }))
        }
        Action::OffersOther => Response::OffersOther(
            value.as_bool().unwrap_or(false)
                && domain.is_some_and(|d| {
                    matches!(d.kind.as_str(), "categorical_int" | "categorical_number")
                }),
        ),
        Action::Format => Response::Format(format_value(domain, value)),
        Action::Options => Response::Options(
            domain
                .map(|d| {
                    d.values
                        .iter()
                        .map(|v| DomainValueOption {
                            value: value_as_text(v),
                            label: format_value(Some(d), v),
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
        ),
        Action::Type => {
            let key = crate::normalize_column(&value_as_text(value));
            Response::Type(match key.as_str() {
                "categorical_string" | "string" | "categorical" => {
                    Some(DomainType::CategoricalString)
                }
                "categorical_int" | "int" | "integer" => Some(DomainType::CategoricalInt),
                "categorical_number" | "number" | "float" => Some(DomainType::CategoricalNumber),
                "boolean" | "bool" => Some(DomainType::Boolean),
                _ => None,
            })
        }
        Action::Values => {
            let d = required()?;
            let raw = match value {
                Value::Array(values) => values.clone(),
                Value::String(text) if text.trim().starts_with('[') => {
                    serde_json::from_str::<Vec<Value>>(text).unwrap_or_default()
                }
                Value::String(text) => text
                    .split(if text.contains("##") { "##" } else { "," })
                    .map(|v| json!(v.trim()))
                    .collect(),
                _ => Vec::new(),
            };
            let mut values = Vec::new();
            for raw in raw {
                let v = coerce_domain_value(d, &raw);
                if v != json!("") && !values.contains(&v) {
                    values.push(v);
                }
            }
            Response::Values(
                values
                    .into_iter()
                    .map(DomainScalar::try_from)
                    .collect::<Result<_, _>>()?,
            )
        }
    })
}

fn format_value(domain: Option<&ColumnPlanDomain>, value: &Value) -> String {
    if let Some(d) = domain
        && d.kind == "categorical_number"
        && let Some(unit) = &d.unit
        && let Some(n) = parse_numeric_domain_value(value)
    {
        let text = format!("{n:.3}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string();
        return if unit.trim().is_empty() {
            text
        } else {
            format!("{} {}", text, unit.trim())
        };
    }
    value_as_text(value)
}

#[cfg(test)]
#[path = "domain_value_tests.rs"]
mod tests;
