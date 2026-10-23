//! Admission of a UI request against Core's captured command descriptor.
//! Both native forwarding and WASM forms consume this decision.
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Failure {
    pub class: String,
    pub code: String,
    pub message: String,
    pub retryable: bool,
    pub remedy: String,
}

impl Failure {
    pub fn new(code: &str, message: impl Into<String>, remedy: impl Into<String>) -> Self {
        Self {
            class: "unavailable".into(),
            code: code.into(),
            message: message.into(),
            retryable: false,
            remedy: remedy.into(),
        }
    }
}

/// Preserve an owner's typed failure across a host/JS rejection. A transport
/// that lost its owner still yields one typed, actionable answer. No result is
/// retried here: a disconnected write may already have committed.
pub fn connection_failure(code: Option<&str>, message: &str, remedy: Option<&str>) -> Failure {
    let (prefix, message) = message
        .split_once(": ")
        .filter(|(code, _)| {
            !code.is_empty()
                && code.len() <= 96
                && code
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        })
        .map_or((None, message), |(code, text)| (Some(code), text));
    let code = code.or(prefix).unwrap_or("engine_connection_lost");
    let mut failure = Failure::new(code, message,
        remedy.filter(|text| !text.is_empty()).unwrap_or(
            "Check DS Core and its required module in Modules, then retry. Check the previous operation's result before repeating a write."));
    failure.retryable = matches!(
        code,
        "engine_connection_lost"
            | "engine_timed_out"
            | "callee_timed_out"
            | "core_missing"
            | "network_engine_missing"
    );
    failure
}

/// The Kernel's decision for running `id` through this build's Core door. An
/// id this build's manifest lacks is left to installed Core where the page
/// runs beside it ([`super::core_door`]): Core's own descriptor then decides
/// in [`admit`].
pub fn core_refusal(id: &str) -> Option<Failure> {
    core_refusal_on(super::PLATFORM, id)
}

/// [`core_refusal`] for a build serving `platform`.
pub fn core_refusal_on(platform: &str, id: &str) -> Option<Failure> {
    match super::route_on(platform, id) {
        Ok(route) if route.executor == Some(super::Executor::Core) => None,
        Err(_) if super::capability(id).is_err() && super::beside_core(platform) => None,
        Ok(route) => Some(match route.refusal {
            Some(refusal) => Failure::new(
                refusal.code,
                refusal.message,
                "Choose a capability supported by this build, or open a compatible DS client.",
            ),
            None => Failure::new(
                "capability_not_on_core",
                format!(
                    "`{id}` uses its typed {:?} export on {}.",
                    route.executor, route.platform
                ),
                "Use this capability's typed export instead of the Core door.",
            ),
        }),
        Err(message) => Some(Failure::new(
            super::CODE_UNKNOWN,
            message,
            "Choose a command declared by this client's capability manifest.",
        )),
    }
}

/// A descriptor must name this available server command. Required arguments
/// cannot be silently omitted by a form. Machine-owned values and staged
/// outputs are supplied by the native adapter, not the page.
pub fn admit(id: &str, descriptor: &Value, args: &Value, project: &str) -> Option<Failure> {
    admit_on(super::PLATFORM, id, descriptor, args, project)
}

/// [`admit`] for a build serving `platform`. An id this build's manifest
/// lacks runs only when installed Core's own descriptor declares exactly it;
/// Core still authorizes and validates the run.
pub fn admit_on(
    platform: &str,
    id: &str,
    descriptor: &Value,
    args: &Value,
    project: &str,
) -> Option<Failure> {
    if let Some(refusal) = core_refusal_on(platform, id) {
        return Some(refusal);
    }
    if super::capability(id).is_err() && descriptor["id"] != id {
        return Some(Failure::new(
            super::CODE_UNKNOWN,
            format!("Neither this client's capability manifest nor installed DS Core declares `{id}`."),
            "Choose a command installed DS Core declares, or update DS Core.",
        ));
    }
    let incompatible = |message: String| {
        Some(Failure::new(
            "engine_incompatible",
            message,
            "Update DS Core to a version that declares this command contract, then retry.",
        ))
    };
    if descriptor["id"] != id || !descriptor["contract"].is_u64() {
        return incompatible(format!(
            "Core did not declare the requested `{id}` contract."
        ));
    }
    if descriptor["availability"] != "available" || descriptor["requires"] == "window" {
        return incompatible(format!(
            "Core cannot execute `{id}` through the command door."
        ));
    }
    let Some(inputs) = descriptor["inputs"].as_array() else {
        return incompatible(format!(
            "Core's `{id}` descriptor has no input declarations."
        ));
    };
    let invalid = |message: String| {
        let mut refusal = Failure::new(
            "inputs_invalid",
            message,
            "Fill the inputs declared by this command before running it.",
        );
        refusal.class = "invalid_input".into();
        Some(refusal)
    };
    let Some(args) = args.as_object() else {
        return invalid("Command arguments must be an object.".into());
    };
    for name in args.keys() {
        if !inputs.iter().any(|input| input["name"] == name.as_str()) {
            return invalid(format!("`{name}` is not a declared input of `{id}`."));
        }
    }
    for input in inputs {
        let Some(name) = input["name"].as_str() else {
            return incompatible("Malformed input declaration.".into());
        };
        let value = args.get(name);
        let supplied = if name == "project" {
            !project.is_empty()
        } else {
            value.is_some_and(|value| {
                !value.is_null() && value.as_str().is_none_or(|text| !text.trim().is_empty())
            })
        };
        // These declarations are fulfilled by the existing native door.
        let native = matches!(name, "lane" | "state-dir" | "out" | "bytes-out")
            || name.ends_with("-out")
            || input["value"].as_str().is_some_and(|v| v.contains("-new-"));
        if input["required"] == true && input.get("default").is_none() && !supplied && !native {
            return invalid(format!("`{id}` requires `{name}`."));
        }
        let Some(value) = value else {
            continue;
        };
        if input["kind"] == "switch" && !value.is_boolean() {
            return invalid(format!("`{name}` is a switch (true or false)."));
        }
        if let Some(choices) = input["choices"].as_array() {
            let values = value
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_else(|| std::slice::from_ref(value));
            if values.iter().any(|value| !choices.contains(value)) {
                return invalid(format!(
                    "`{name}` must be one of the command's declared choices."
                ));
            }
        }
    }
    None
}

/// A fixed application request needs a newer installed Core.
pub const CORE_UPDATE_REQUIRED: &str = "core_update_required";

/// Admission of a fixed application request (not a descriptor-driven form)
/// against the installed Core's own descriptor. Every input and choice the
/// request carries must be declared there: an older Core that lacks one is
/// answered `core_update_required` before anything is sent. Compatibility is
/// the declared inputs and choices, never a release or contract number; an
/// additive descriptor field or input never refuses.
pub fn admit_request(id: &str, descriptor: &Value, args: &Value, project: &str) -> Option<Failure> {
    if let Some(refusal) = core_refusal(id) {
        return Some(refusal);
    }
    if let (true, Some(inputs), Some(args)) = (
        descriptor["id"] == id,
        descriptor["inputs"].as_array(),
        args.as_object(),
    ) {
        for (name, value) in args {
            let missing = match inputs.iter().find(|input| input["name"] == name.as_str()) {
                None => Some(format!("`--{name}`")),
                Some(input) => input["choices"].as_array().and_then(|choices| {
                    value
                        .as_array()
                        .map(Vec::as_slice)
                        .unwrap_or_else(|| std::slice::from_ref(value))
                        .iter()
                        .find(|value| !choices.contains(value))
                        .map(|value| match value.as_str() {
                            Some(text) => format!("`--{name} {text}`"),
                            None => format!("`--{name} {value}`"),
                        })
                }),
            };
            if let Some(missing) = missing {
                let mut refusal = Failure::new(
                    CORE_UPDATE_REQUIRED,
                    format!(
                        "The DS Core installed on this device does not support {missing} for `{id}`, which this window needs."
                    ),
                    "Update DS Core from Modules; it updates independently of this window. Then retry.",
                );
                refusal.class = "unavailable".into();
                return Some(refusal);
            }
        }
    }
    admit(id, descriptor, args, project)
}

#[cfg(test)]
#[path = "capability_command_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "capability_command_request_tests.rs"]
mod request_tests;
