//! Generated gateway wire coordinates, below the native and browser clients.
//!
//! No HTTP, credential store, cache or retry implementation lives here. The
//! application policy port owns those decisions and admits the response. The
//! gateway specification is incomplete, so an unspecified body is not presented
//! as a complete generated domain model.

use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};

pub mod generated;
pub use generated::Operation;

/// HTTP methods declared by the copied gateway specification.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Method {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
}

impl Method {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
            Self::Head => "HEAD",
        }
    }
}

/// A schema-declared parameter location; no location is inferred by a host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Location {
    Path,
    Query,
    Header,
    Cookie,
}

/// Wire parameter metadata from OpenAPI. Domain validation stays in its owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Parameter {
    pub name: &'static str,
    pub location: Location,
    pub required: bool,
    /// OpenAPI's scalar/container type, or None where no type is declared.
    pub schema_type: Option<&'static str>,
}

/// Generated operation metadata. This is wire description, not DS policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Descriptor {
    pub operation_id: &'static str,
    pub method: Method,
    pub path_template: &'static str,
    pub path_parameters: &'static [&'static str],
    pub parameters: &'static [Parameter],
    pub has_request_schema: bool,
    pub has_response_schema: bool,
}

/// A fixed generated operation with its admitted path parameters expanded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoutePlan {
    operation: Operation,
    method: Method,
    path: String,
}

impl RoutePlan {
    pub fn operation(&self) -> Operation {
        self.operation
    }

    pub fn method(&self) -> Method {
        self.method
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn into_path(self) -> String {
        self.path
    }
}

/// A parameter refusal before any credential or network effect is requested.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlanError {
    UnknownParameter(String),
    DuplicateParameter(String),
    MissingParameter(&'static str),
    InvalidParameter(&'static str),
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownParameter(name) => write!(f, "Unknown path parameter: {name}"),
            Self::DuplicateParameter(name) => write!(f, "Duplicate path parameter: {name}"),
            Self::MissingParameter(name) => write!(f, "Missing path parameter: {name}"),
            Self::InvalidParameter(name) => write!(f, "Invalid path parameter: {name}"),
        }
    }
}

impl std::error::Error for PlanError {}

// RFC 3986 path-segment encoding: separators and template delimiters are data.
const PATH_SEGMENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

fn plan(operation: Operation, parameters: &[(&str, &str)]) -> Result<RoutePlan, PlanError> {
    let descriptor = operation.descriptor();
    for (index, (name, _)) in parameters.iter().enumerate() {
        if !descriptor.path_parameters.contains(name) {
            return Err(PlanError::UnknownParameter((*name).into()));
        }
        if parameters[..index]
            .iter()
            .any(|(previous, _)| previous == name)
        {
            return Err(PlanError::DuplicateParameter((*name).into()));
        }
    }
    let mut path = descriptor.path_template.to_owned();
    for name in descriptor.path_parameters {
        let value = parameters
            .iter()
            .find_map(|(key, value)| (key == name).then_some(*value))
            .ok_or(PlanError::MissingParameter(name))?;
        if value.is_empty() || matches!(value, "." | "..") || value.chars().any(char::is_control) {
            return Err(PlanError::InvalidParameter(name));
        }
        path = path.replace(
            &format!("{{{name}}}"),
            &utf8_percent_encode(value, PATH_SEGMENT).to_string(),
        );
    }
    Ok(RoutePlan {
        operation,
        method: descriptor.method,
        path,
    })
}

/// The existing application's single DS policy owner.
///
/// The port captures explicit principal/project/revision/cancellation context;
/// coalesces refresh through its existing auth owner; serves held data without
/// credential/network effects; applies the operation's existing retry budget;
/// and returns its existing typed result/refusal, including transport cause.
/// It must not infer policy from the HTTP verb. Output may be a synchronous
/// result or a future, so this port imposes no executor or platform dependency.
///
/// This seam does not replace those policies with a second implementation.
pub trait Policy {
    type Context;
    type Output;

    fn exchange(&mut self, context: &Self::Context, route: RoutePlan) -> Self::Output;
}

/// A typed generated-route client delegating exactly once to the DS policy port.
pub struct Client<P> {
    policy: P,
}

impl<P: Policy> Client<P> {
    pub fn new(policy: P) -> Self {
        Self { policy }
    }

    pub fn call(&mut self, context: &P::Context, route: RoutePlan) -> P::Output {
        self.policy.exchange(context, route)
    }

    pub fn into_policy(self) -> P {
        self.policy
    }
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
