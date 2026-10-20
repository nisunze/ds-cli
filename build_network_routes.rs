//! Compiles `routing/network-routes.json` into the Kernel's constant route
//! tables. Every gate a manifest must pass runs here, so a malformed or
//! incomplete manifest fails the build instead of reaching a host.

use serde_json::{Map, Value};
use std::{collections::BTreeSet, fs, path::Path};

const SCHEMA: &str = "ds.network-routes/v1";
/// The platforms the Kernel builds for, one cargo feature each.
const PLATFORMS: [&str; 3] = ["desktop", "web", "core"];
const EXECUTORS: [&str; 3] = ["core", "network_unit", "refuse"];
const KINDS: [&str; 2] = ["engineering_job", "in_page"];
const MAX_PAGE_LANES: u64 = 64;

fn fail(message: impl std::fmt::Display) -> ! {
    panic!("network_routes_invalid: {message}")
}

fn object<'a>(value: &'a Value, what: &str) -> &'a Map<String, Value> {
    value
        .as_object()
        .unwrap_or_else(|| fail(format!("{what} must be an object")))
}

fn closed(value: &Map<String, Value>, allowed: &[&str], what: &str) {
    if let Some(key) = value.keys().find(|key| !allowed.contains(&key.as_str())) {
        fail(format!("{what} carries the unknown member `{key}`"));
    }
}

fn text<'a>(value: Option<&'a Value>, what: &str) -> &'a str {
    value
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .unwrap_or_else(|| fail(format!("{what} must be non-empty text")))
}

/// A command id, door or input name: lowercase words joined by `.`, `_`, `-`.
fn name(value: Option<&Value>, what: &str) -> String {
    let text = text(value, what);
    let valid = text.len() <= 64
        && text.as_bytes()[0].is_ascii_lowercase()
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-'));
    if !valid {
        fail(format!("{what} `{text}` is not a lowercase identifier"));
    }
    text.to_owned()
}

fn literal(text: &str) -> String {
    format!("{text:?}")
}

fn optional(value: Option<String>) -> String {
    value.map_or_else(|| "None".to_owned(), |text| format!("Some({})", literal(&text)))
}

fn executor_variant(executor: &str) -> &'static str {
    match executor {
        "core" => "Executor::Core",
        "network_unit" => "Executor::NetworkUnit",
        "refuse" => "Executor::Refuse",
        other => fail(format!("unknown executor `{other}`")),
    }
}

/// The Core door a capability declares: one Core command through the host's
/// generic door, or one named host door.
fn core_spec(capability: &str, value: Option<&Value>, uses_core: bool) -> String {
    let Some(value) = value else {
        if uses_core {
            fail(format!("`{capability}` routes to core but names no Core command or door"));
        }
        return "None".to_owned();
    };
    if !uses_core {
        fail(format!("`{capability}` names a Core door but no platform routes to core"));
    }
    let spec = object(value, &format!("`{capability}` core"));
    closed(spec, &["command", "door", "input", "wrap", "answer", "switches"], &format!("`{capability}` core"));
    let field = |key: &str| spec.get(key).map(|value| name(Some(value), &format!("`{capability}` core.{key}")));
    let (command, door) = (field("command"), field("door"));
    if command.is_some() == door.is_some() {
        fail(format!("`{capability}` names exactly one Core command or one door"));
    }
    let (input, wrap, answer) = (field("input"), field("wrap"), field("answer"));
    if wrap.is_some() && input.is_none() {
        fail(format!("`{capability}` wraps an input it does not name"));
    }
    if door.is_some() && (input.is_some() || wrap.is_some() || answer.is_some() || spec.contains_key("switches")) {
        fail(format!("`{capability}` passes its input whole to a door"));
    }
    let switches: Vec<String> = match spec.get("switches") {
        None => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| name(Some(item), &format!("`{capability}` core.switches")))
            .collect(),
        Some(_) => fail(format!("`{capability}` core.switches must be a list")),
    };
    format!(
        "Some(CoreDoor {{ command: {}, door: {}, input: {}, wrap: {}, answer: {}, switches: &[{}] }})",
        optional(command),
        optional(door),
        optional(input),
        optional(wrap),
        optional(answer),
        switches.iter().map(|s| literal(s)).collect::<Vec<_>>().join(", ")
    )
}

fn route(
    capability: &str,
    kind: &str,
    platform: &str,
    value: &Value,
    exists: &dyn Fn(&str, &str) -> bool,
) -> (String, bool) {
    let what = format!("`{capability}` on {platform}");
    let route = object(value, &what);
    closed(route, &["executor", "page_lanes", "refusal"], &what);
    let executor = text(route.get("executor"), &format!("{what} executor"));
    if !EXECUTORS.contains(&executor) {
        fail(format!("{what} names the unknown executor `{executor}`"));
    }
    if !exists(executor, platform) {
        fail(format!("{what} names {executor}, which does not exist there"));
    }
    if kind == "engineering_job" && executor == "network_unit" && exists("core", platform) {
        fail(format!(
            "engineering job `{capability}` routes to the Network unit on {platform}, where Core is installed"
        ));
    }
    let refusal = match (executor, route.get("refusal")) {
        ("refuse", Some(refusal)) => {
            let refusal = object(refusal, &format!("{what} refusal"));
            closed(refusal, &["code", "message"], &format!("{what} refusal"));
            let code = name(refusal.get("code"), &format!("{what} refusal code"));
            let message = text(refusal.get("message"), &format!("{what} refusal message"));
            format!(
                "Some(Refusal {{ code: {}, message: {} }})",
                literal(&code),
                literal(message)
            )
        }
        ("refuse", None) => fail(format!("{what} refuses without a typed code and message")),
        (_, Some(_)) => fail(format!("{what} carries a refusal but does not refuse")),
        (_, None) => "None".to_owned(),
    };
    let page_lanes = match route.get("page_lanes") {
        None => "None".to_owned(),
        Some(lanes) => match lanes.as_u64() {
            Some(lanes) if (1..=MAX_PAGE_LANES).contains(&lanes) => format!("Some({lanes})"),
            _ => fail(format!("{what} bounds its page lanes outside 1..={MAX_PAGE_LANES}")),
        },
    };
    (
        format!(
            "Route {{ executor: {}, page_lanes: {page_lanes}, refusal: {refusal} }}",
            executor_variant(executor)
        ),
        executor == "core",
    )
}

/// The generated Rust for one manifest; panics with the gate it broke.
pub fn compile(json: &str) -> String {
    let manifest: Value =
        serde_json::from_str(json).unwrap_or_else(|error| fail(format!("not JSON: {error}")));
    let manifest = object(&manifest, "the manifest");
    closed(manifest, &["schema", "doc", "platforms", "executors", "capabilities"], "the manifest");
    if text(manifest.get("schema"), "schema") != SCHEMA {
        fail(format!("expected schema {SCHEMA}"));
    }
    let platforms: Vec<&str> = manifest
        .get("platforms")
        .and_then(Value::as_array)
        .unwrap_or_else(|| fail("platforms must be a list"))
        .iter()
        .map(|platform| text(Some(platform), "a platform"))
        .collect();
    let declared: BTreeSet<&str> = platforms.iter().copied().collect();
    if declared.len() != platforms.len() || declared != PLATFORMS.into_iter().collect() {
        fail("platforms must be desktop, web and core, each once");
    }
    let executors = object(
        manifest.get("executors").unwrap_or(&Value::Null),
        "executors",
    );
    for executor in EXECUTORS {
        let spec = object(
            executors.get(executor).unwrap_or_else(|| fail(format!("executor {executor} is not declared"))),
            &format!("executor {executor}"),
        );
        closed(spec, &["platforms"], &format!("executor {executor}"));
        for platform in spec.get("platforms").and_then(Value::as_array).unwrap_or_else(|| {
            fail(format!("executor {executor} must list its platforms"))
        }) {
            let platform = text(Some(platform), "an executor platform");
            if !declared.contains(platform) {
                fail(format!("executor {executor} names the undeclared platform `{platform}`"));
            }
        }
    }
    if let Some(unknown) = executors.keys().find(|key| !EXECUTORS.contains(&key.as_str())) {
        fail(format!("the unknown executor `{unknown}` is declared"));
    }
    let exists = |executor: &str, platform: &str| {
        executors[executor]["platforms"]
            .as_array()
            .is_some_and(|platforms| platforms.iter().any(|known| known == platform))
    };
    let capabilities = object(
        manifest.get("capabilities").unwrap_or(&Value::Null),
        "capabilities",
    );
    if capabilities.is_empty() {
        fail("no capability is routed");
    }
    let mut rows = Vec::new();
    for (capability, entry) in capabilities {
        name(Some(&Value::String(capability.clone())), "a capability");
        let entry = object(entry, &format!("`{capability}`"));
        closed(entry, &["kind", "doc", "core", "routes"], &format!("`{capability}`"));
        let kind = text(entry.get("kind"), &format!("`{capability}` kind"));
        if !KINDS.contains(&kind) {
            fail(format!("`{capability}` has the unknown kind `{kind}`"));
        }
        text(entry.get("doc"), &format!("`{capability}` doc"));
        let routes = object(entry.get("routes").unwrap_or(&Value::Null), &format!("`{capability}` routes"));
        let routed: BTreeSet<&str> = routes.keys().map(String::as_str).collect();
        if routed != declared {
            fail(format!("`{capability}` must route every declared platform, and only those"));
        }
        let mut uses_core = false;
        let mut columns = Vec::new();
        for platform in PLATFORMS {
            let (column, core) = route(capability, kind, platform, &routes[platform], &exists);
            uses_core |= core;
            columns.push(column);
        }
        let core = core_spec(capability, entry.get("core"), uses_core);
        let kind = if kind == "engineering_job" {
            "CapabilityKind::EngineeringJob"
        } else {
            "CapabilityKind::InPage"
        };
        rows.push(format!(
            "    Capability {{ name: {}, kind: {kind}, core: {core}, desktop: {}, web: {}, core_route: {} }},",
            literal(capability),
            columns[0],
            columns[1],
            columns[2]
        ));
    }
    format!(
        "// Generated by build.rs from routing/network-routes.json; do not edit.\npub(crate) static CAPABILITIES: &[Capability] = &[\n{}\n];\n",
        rows.join("\n")
    )
}

/// Write the compiled tables into OUT_DIR for `src/network_routes.rs`.
pub fn emit(root: &Path) {
    let manifest = root.join("routing/network-routes.json");
    println!("cargo:rerun-if-changed={}", manifest.display());
    let json = fs::read_to_string(&manifest).expect("routing/network-routes.json is a Kernel build input");
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("Cargo sets OUT_DIR"));
    fs::write(out.join("network_routes_table.rs"), compile(&json))
        .expect("the compiled route table is writable");
}
