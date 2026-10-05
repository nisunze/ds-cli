//! The native JSON control-plane registry. Hosts only bound and transport calls.
use schemars::{schema_for, JsonSchema, Schema};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::value::RawValue;
use sha2::{Digest, Sha256};

pub const KERNEL_VERSION: &str = concat!(
    "ds-command-kernel@",
    env!("CARGO_PKG_VERSION"),
    "+",
    env!("DS_KERNEL_SOURCE_SHA256")
);

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub operation: String,
    pub kernel_version: Option<String>,
    pub input: Box<RawValue>,
}

#[derive(Debug, Serialize)]
pub struct Response {
    pub operation: String,
    pub kernel_version: &'static str,
    /// Serialized once. SHA-256 covers exactly these UTF-8 JSON bytes.
    pub answer: Box<RawValue>,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Refusal {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

impl Refusal {
    pub fn new(code: &str, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            retryable,
        }
    }
}

/// Every registration fixes a name and both typed contracts in the kernel.
pub trait Operation {
    const NAME: &'static str;
    type Input: DeserializeOwned + JsonSchema;
    type Output: Serialize + JsonSchema;
    fn run(input: Self::Input) -> Result<Self::Output, Refusal>;
}

pub struct OperationSpec {
    pub name: &'static str,
    pub input_schema: fn() -> Schema,
    pub output_schema: fn() -> Schema,
    run: fn(&RawValue) -> Result<Box<RawValue>, Refusal>,
}

impl OperationSpec {
    pub const fn of<O: Operation>() -> Self {
        Self {
            name: O::NAME,
            input_schema: || schema_for!(O::Input),
            output_schema: || schema_for!(O::Output),
            run: run_typed::<O>,
        }
    }
}

fn run_typed<O: Operation>(value: &RawValue) -> Result<Box<RawValue>, Refusal> {
    // Decode the original bytes so duplicate typed fields are rejected too.
    let input = serde_json::from_str(value.get())
        .map_err(|e| Refusal::new("invalid_input", e.to_string(), false))?;
    let output = O::run(input)?;
    serde_json::value::to_raw_value(&output).map_err(|_| {
        Refusal::new(
            "internal_error",
            "Kernel answer could not be serialized",
            false,
        )
    })
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PrintScalingAnswer {
    pub scaling: crate::print_scaling::PrintScaling,
}

pub struct PrintScalingValidate;
impl Operation for PrintScalingValidate {
    const NAME: &'static str = "print_scaling.validate";
    type Input = crate::print_scaling::PrintScaling;
    type Output = PrintScalingAnswer;
    fn run(scaling: Self::Input) -> Result<Self::Output, Refusal> {
        scaling
            .validate()
            .map_err(|e| Refusal::new("invalid_print_scaling", e, false))?;
        Ok(PrintScalingAnswer { scaling })
    }
}

/// Plan one governed printing template's legacy style_refs → style_keys
/// migration for ds-brain, which holds the stored template and the governed
/// catalogue and persists only a `migrate` answer's layout.
pub struct PrintStyleKeysPlan;
impl Operation for PrintStyleKeysPlan {
    const NAME: &'static str = "print_style_keys.plan";
    type Input = crate::printing::style_migration::Request;
    type Output = crate::printing::style_migration::Answer;
    fn run(request: Self::Input) -> Result<Self::Output, Refusal> {
        crate::printing::style_migration::answer(request)
            .map_err(|e| Refusal::new("print_style_keys_invalid", e, false))
    }
}

pub static OPERATIONS: &[OperationSpec] = &[
    OperationSpec::of::<PrintScalingValidate>(),
    OperationSpec::of::<PrintStyleKeysPlan>(),
];

pub fn dispatch(request: Request) -> Result<Response, Refusal> {
    if request
        .kernel_version
        .as_deref()
        .is_some_and(|v| v != KERNEL_VERSION)
    {
        return Err(Refusal::new(
            "version_mismatch",
            "Requested kernel version is not served",
            false,
        ));
    }
    let spec = OPERATIONS
        .iter()
        .find(|spec| spec.name == request.operation)
        .ok_or_else(|| {
            Refusal::new(
                "unknown_operation",
                "Kernel operation is not registered",
                false,
            )
        })?;
    let answer = (spec.run)(&request.input)?;
    let sha256 = format!("{:x}", Sha256::digest(answer.get().as_bytes()));
    Ok(Response {
        operation: request.operation,
        kernel_version: KERNEL_VERSION,
        answer,
        sha256,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    fn request(input: Value) -> Request {
        Request {
            operation: PrintScalingValidate::NAME.into(),
            kernel_version: Some(KERNEL_VERSION.into()),
            input: serde_json::value::to_raw_value(&input).unwrap(),
        }
    }
    #[test]
    fn typed_registration_has_closed_schemas_and_exact_receipt() {
        let names: std::collections::BTreeSet<_> = OPERATIONS.iter().map(|s| s.name).collect();
        assert_eq!(names.len(), OPERATIONS.len(), "duplicate operation name");
        let response = dispatch(request(
            serde_json::to_value(crate::print_scaling::PrintScaling::default()).unwrap(),
        ))
        .unwrap();
        assert_eq!(
            response.sha256,
            format!("{:x}", Sha256::digest(response.answer.get()))
        );
        let decoded: PrintScalingAnswer = serde_json::from_str(response.answer.get()).unwrap();
        assert_eq!(
            decoded.scaling,
            crate::print_scaling::PrintScaling::default()
        );
        assert_eq!(
            (OPERATIONS[0].input_schema)().as_value()["additionalProperties"],
            false
        );
        assert_eq!(
            (OPERATIONS[0].output_schema)().as_value()["additionalProperties"],
            false
        );
    }
    #[test]
    fn print_style_keys_plan_is_one_registered_operation() {
        let mut layout = crate::printing::default_layout();
        layout
            .style_refs
            .insert("lv_lines".into(), "master/lv_lines_print".into());
        let key = json!({"entity_class":"lv_lines","source_kind":"saved_design","target":"print","role":"project"});
        let catalogue = json!({"revision_id":"c".repeat(64),
            "bindings":[{"key":key,"style_ref":"master/lv_lines_print"}]});
        let call = |catalogue: Value| {
            dispatch(Request {
                operation: PrintStyleKeysPlan::NAME.into(),
                kernel_version: Some(KERNEL_VERSION.into()),
                input: serde_json::value::to_raw_value(
                    &json!({"layout":layout,"catalogue":catalogue}),
                )
                .unwrap(),
            })
        };
        let response = call(catalogue.clone()).unwrap();
        assert_eq!(
            response.sha256,
            format!("{:x}", Sha256::digest(response.answer.get()))
        );
        let answer: Value = serde_json::from_str(response.answer.get()).unwrap();
        assert_eq!(answer["plan"]["status"], "migrate");
        assert_eq!(answer["plan"]["catalogue_revision"], "c".repeat(64));
        assert_eq!(answer["layout"]["style_keys"]["lv_lines"], key);
        assert_eq!(answer["layout"]["style_refs"], json!({}));
        let mut unpinned = catalogue;
        unpinned["revision_id"] = json!("not-a-digest");
        assert_eq!(call(unpinned).unwrap_err().code, "print_style_keys_invalid");
    }
    #[test]
    fn invalid_inputs_and_pins_are_terminal_refusals() {
        let duplicate: Request = serde_json::from_str(r#"{"operation":"print_scaling.validate","input":{"reference_paper":"A3","reference_paper":"A3","units":"css_px","factors":{}}}"#).unwrap();
        assert_eq!(dispatch(duplicate).unwrap_err().code, "invalid_input");
        let good = serde_json::to_value(crate::print_scaling::PrintScaling::default()).unwrap();
        for (key, value) in [("units", json!("pixels")), ("extra", json!(true))] {
            let mut invalid = good.clone();
            invalid[key] = value;
            assert_eq!(
                dispatch(request(invalid)).unwrap_err().code,
                "invalid_input"
            );
        }
        for value in [0., 21.] {
            let mut invalid = good.clone();
            invalid["factors"]["A0"] = json!(value);
            assert_eq!(
                dispatch(request(invalid)).unwrap_err().code,
                "invalid_print_scaling"
            );
        }
        let mut req = request(good.clone());
        req.kernel_version = Some("other".into());
        assert_eq!(dispatch(req).unwrap_err().code, "version_mismatch");
        let mut req = request(good);
        req.operation = "absent".into();
        assert_eq!(dispatch(req).unwrap_err().code, "unknown_operation");
        assert!(serde_json::from_value::<Request>(
            json!({"operation":"x","input":{},"extra":true})
        )
        .is_err());
    }
}
