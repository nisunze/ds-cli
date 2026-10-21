//! Compiles `routing/capabilities.json`, the one capability manifest, into the
//! Kernel's constant tables. Every gate the manifest must pass runs here, so
//! a malformed, incomplete or hand-formatted manifest fails the build instead
//! of reaching a host.
//!
//! The manifest is its own canonical text. `DS_CAPABILITIES_REWRITE=1 cargo
//! check` rewrites it canonically and gives every row that lacks a declared
//! platform the value `pending:<platform>_not_yet_supported`, adding that
//! reason, so declaring a platform never means editing every row by hand.

use serde_json::{Map, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::Path,
};

pub const SCHEMA: &str = "ds.capabilities/v1";
const MANIFEST: &str = "routing/capabilities.json";
const REWRITE: &str = "DS_CAPABILITIES_REWRITE";
/// The executors the Kernel has code for; a platform names the ones it has.
const EXECUTORS: [&str; 3] = ["core", "kernel", "network_unit"];
const RUN_MEMBERS: [&str; 6] = [
    "input",
    "wrap",
    "answer",
    "output",
    "switches",
    "page_lanes",
];
const MAX_PAGE_LANES: u64 = 64;

fn fail(message: impl std::fmt::Display) -> ! {
    panic!("capabilities_invalid: {message}")
}

/// What the manifest is checked against outside itself.
pub struct Environment<'a> {
    /// `src/gateway_exchange/<name>.rs` exists.
    pub exchange: &'a dyn Fn(&str) -> bool,
    /// Cargo.toml declares the feature.
    pub feature_declared: &'a dyn Fn(&str) -> bool,
    /// This build enables the feature.
    pub feature_enabled: &'a dyn Fn(&str) -> bool,
}

pub struct Platform {
    pub id: String,
    doc: String,
    pub executors: Vec<String>,
    pub feature: Option<String>,
    pub default: bool,
}

pub struct Row {
    /// The value for each platform the row names, by platform id.
    pub values: BTreeMap<String, String>,
    run: Option<Map<String, Value>>,
}

pub struct Manifest {
    doc: Vec<String>,
    pub platforms: Vec<Platform>,
    pub reasons: BTreeMap<String, String>,
    pub capabilities: BTreeMap<String, Row>,
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

fn text(value: Option<&Value>, what: &str) -> String {
    value
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .unwrap_or_else(|| fail(format!("{what} must be non-empty text")))
        .to_owned()
}

/// A platform, reason, exchange or input name: lowercase words joined
/// by `_`, `-` or (for `dotted`) `.`.
fn name(text: &str, what: &str, dotted: bool) -> String {
    let valid = !text.is_empty()
        && text.len() <= 96
        && text.as_bytes()[0].is_ascii_lowercase()
        && text.bytes().all(|b| {
            b.is_ascii_lowercase()
                || b.is_ascii_digit()
                || matches!(b, b'_' | b'-')
                || (dotted && b == b'.')
        });
    if !valid {
        fail(format!("{what} `{text}` is not a lowercase identifier"));
    }
    text.to_owned()
}

fn names(value: Option<&Value>, what: &str) -> Vec<String> {
    let items = value
        .and_then(Value::as_array)
        .unwrap_or_else(|| fail(format!("{what} must be a list")));
    let names: Vec<String> = items
        .iter()
        .map(|item| name(&text(Some(item), what), what, false))
        .collect();
    if names.iter().collect::<BTreeSet<_>>().len() != names.len() {
        fail(format!("{what} names an entry twice"));
    }
    names
}

/// The manifest's structure. Rows may still lack a declared platform here:
/// [`fill`] completes them, and [`compile`] refuses them.
pub fn parse(json: &str) -> Manifest {
    let manifest: Value =
        serde_json::from_str(json).unwrap_or_else(|error| fail(format!("not JSON: {error}")));
    let manifest = object(&manifest, "the manifest");
    closed(
        manifest,
        &["schema", "doc", "platforms", "reasons", "capabilities"],
        "the manifest",
    );
    if text(manifest.get("schema"), "schema") != SCHEMA {
        fail(format!("expected schema {SCHEMA}"));
    }
    let doc = manifest
        .get("doc")
        .and_then(Value::as_array)
        .unwrap_or_else(|| fail("doc must be a list of lines"))
        .iter()
        .map(|line| {
            line.as_str()
                .unwrap_or_else(|| fail("doc must be a list of lines"))
                .to_owned()
        })
        .collect();
    let mut platforms = Vec::new();
    for value in manifest
        .get("platforms")
        .and_then(Value::as_array)
        .unwrap_or_else(|| fail("platforms must be a list"))
    {
        let spec = object(value, "a platform");
        closed(
            spec,
            &["id", "doc", "executors", "feature", "default"],
            "a platform",
        );
        let id = name(
            &text(spec.get("id"), "a platform id"),
            "a platform id",
            false,
        );
        let what = format!("platform `{id}`");
        let executors = names(spec.get("executors"), &format!("{what} executors"));
        if let Some(unknown) = executors.iter().find(|e| !EXECUTORS.contains(&e.as_str())) {
            fail(format!("{what} names the unknown executor `{unknown}`"));
        }
        let feature = spec
            .get("feature")
            .map(|value| name(&text(Some(value), &what), &format!("{what} feature"), false));
        let default = match spec.get("default") {
            None => false,
            Some(Value::Bool(true)) => true,
            Some(_) => fail(format!("{what} default is `true` or absent")),
        };
        platforms.push(Platform {
            doc: text(spec.get("doc"), &format!("{what} doc")),
            id,
            executors,
            feature,
            default,
        });
    }
    let reasons = object(manifest.get("reasons").unwrap_or(&Value::Null), "reasons")
        .iter()
        .map(|(key, message)| {
            (
                name(key, "a reason", false),
                text(Some(message), &format!("reason `{key}`")),
            )
        })
        .collect();
    let capabilities = object(
        manifest.get("capabilities").unwrap_or(&Value::Null),
        "capabilities",
    )
    .iter()
    .map(|(id, row)| {
        let id = name(id, "a capability", true);
        let what = format!("`{id}`");
        let mut values = BTreeMap::new();
        let mut run = None;
        for (key, value) in object(row, &what) {
            if key == "run" {
                run = Some(object(value, &format!("{what} run")).clone());
            } else {
                values.insert(key.clone(), text(Some(value), &format!("{what} on {key}")));
            }
        }
        (id, Row { values, run })
    })
    .collect();
    Manifest {
        doc,
        platforms,
        reasons,
        capabilities,
    }
}

fn not_yet_supported(platform: &str) -> String {
    format!("{platform}_not_yet_supported")
}

/// Give every row that lacks a declared platform its pending value, and the
/// reason that value names.
pub fn fill(manifest: &mut Manifest) {
    for platform in &manifest.platforms {
        let reason = not_yet_supported(&platform.id);
        let mut filled = false;
        for row in manifest.capabilities.values_mut() {
            if !row.values.contains_key(&platform.id) {
                row.values
                    .insert(platform.id.clone(), format!("pending:{reason}"));
                filled = true;
            }
        }
        if filled {
            manifest
                .reasons
                .entry(reason)
                .or_insert_with(|| format!("Not yet supported on {}.", platform.id));
        }
    }
}

fn quoted(text: &str) -> String {
    serde_json::to_string(text).expect("text serializes")
}

fn list(items: &[String]) -> String {
    format!(
        "[{}]",
        items
            .iter()
            .map(|item| quoted(item))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// The manifest's canonical text: one line per platform, reason and row,
/// rows sorted by id, a row's values in platform order.
pub fn render(manifest: &Manifest) -> String {
    let mut out = format!("{{\n  \"schema\": {},\n  \"doc\": [\n", quoted(SCHEMA));
    let lines: Vec<String> = manifest
        .doc
        .iter()
        .map(|line| format!("    {}", quoted(line)))
        .collect();
    out += &lines.join(",\n");
    out += "\n  ],\n  \"platforms\": [\n";
    let lines: Vec<String> = manifest
        .platforms
        .iter()
        .map(|platform| {
            let mut line = format!(
                "    {{\"id\": {}, \"doc\": {}, \"executors\": {}",
                quoted(&platform.id),
                quoted(&platform.doc),
                list(&platform.executors)
            );
            if let Some(feature) = &platform.feature {
                line += &format!(", \"feature\": {}", quoted(feature));
            }
            if platform.default {
                line += ", \"default\": true";
            }
            line + "}"
        })
        .collect();
    out += &lines.join(",\n");
    out += "\n  ],\n  \"reasons\": {\n";
    let lines: Vec<String> = manifest
        .reasons
        .iter()
        .map(|(key, message)| format!("    {}: {}", quoted(key), quoted(message)))
        .collect();
    out += &lines.join(",\n");
    out += "\n  },\n  \"capabilities\": {\n";
    let lines: Vec<String> = manifest
        .capabilities
        .iter()
        .map(|(id, row)| {
            let mut members: Vec<String> = manifest
                .platforms
                .iter()
                .filter_map(|platform| {
                    row.values
                        .get(&platform.id)
                        .map(|value| format!("{}: {}", quoted(&platform.id), quoted(value)))
                })
                .collect();
            // Values for undeclared platforms stay visible so the gate names them.
            members.extend(
                row.values
                    .iter()
                    .filter(|(key, _)| !manifest.platforms.iter().any(|p| &p.id == *key))
                    .map(|(key, value)| format!("{}: {}", quoted(key), quoted(value))),
            );
            if let Some(run) = &row.run {
                let mut fields: Vec<String> = RUN_MEMBERS
                    .iter()
                    .filter_map(|key| {
                        run.get(*key)
                            .map(|value| format!("{}: {}", quoted(key), compact(value)))
                    })
                    .collect();
                fields.extend(
                    run.iter()
                        .filter(|(key, _)| !RUN_MEMBERS.contains(&key.as_str()))
                        .map(|(key, value)| format!("{}: {}", quoted(key), compact(value))),
                );
                members.push(format!("\"run\": {{{}}}", fields.join(", ")));
            }
            format!("    {}: {{{}}}", quoted(id), members.join(", "))
        })
        .collect();
    out += &lines.join(",\n");
    out += "\n  }\n}\n";
    out
}

fn compact(value: &Value) -> String {
    match value {
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(compact).collect::<Vec<_>>().join(", ")
        ),
        other => other.to_string(),
    }
}

fn literal(text: &str) -> String {
    format!("{text:?}")
}

fn optional(value: Option<String>) -> String {
    value.map_or_else(
        || "None".to_owned(),
        |text| format!("Some({})", literal(&text)),
    )
}

fn executor_variant(executor: &str) -> &'static str {
    match executor {
        "core" => "Executor::Core",
        "kernel" => "Executor::Kernel",
        "network_unit" => "Executor::NetworkUnit",
        other => fail(format!("unknown executor `{other}`")),
    }
}

/// One platform value as the Rust route it compiles to; `true` when it runs
/// on Core.
fn route(
    value: &str,
    platform: &Platform,
    what: &str,
    reasons: &BTreeMap<String, String>,
    used: &mut BTreeSet<String>,
    environment: &Environment,
) -> (String, bool) {
    let (head, tail) = match value.split_once(':') {
        Some((head, tail)) => (head, Some(tail)),
        None => (value, None),
    };
    let reason = |status: &str, key: Option<&str>, used: &mut BTreeSet<String>| {
        let key = key.unwrap_or_else(|| fail(format!("{what} is {status} without a reason")));
        let message = reasons
            .get(key)
            .unwrap_or_else(|| fail(format!("{what} names the undeclared reason `{key}`")));
        used.insert(key.to_owned());
        format!(
            "Route::{status}(Reason {{ key: {}, message: {} }})",
            literal(key),
            literal(message)
        )
    };
    match head {
        "pending" => (reason("Pending", tail, used), false),
        "discarded" => (reason("Discarded", tail, used), false),
        executor if EXECUTORS.contains(&executor) => {
            if !platform.executors.iter().any(|known| known == executor) {
                fail(format!(
                    "{what} names {executor}, which does not exist on {}",
                    platform.id
                ));
            }
            let exchange = match (executor, tail) {
                ("kernel", Some(exchange)) => {
                    let exchange = name(exchange, &format!("{what} exchange"), false);
                    if !(environment.exchange)(&exchange) {
                        fail(format!(
                            "{what} names the Kernel exchange `{exchange}`, which src/gateway_exchange does not have"
                        ));
                    }
                    Some(exchange)
                }
                ("kernel", None) => fail(format!("{what} names no Kernel exchange")),
                (_, Some(_)) => fail(format!("{what}: only `kernel` names an exchange")),
                (_, None) => None,
            };
            (
                format!(
                    "Route::Supported {{ executor: {}, exchange: {} }}",
                    executor_variant(executor),
                    optional(exchange)
                ),
                executor == "core",
            )
        }
        other => fail(format!("{what} has the unknown value `{other}`")),
    }
}

fn run_spec(id: &str, run: &Map<String, Value>, on_core: bool) -> String {
    let what = format!("`{id}` run");
    closed(run, &RUN_MEMBERS, &what);
    if !on_core {
        fail(format!(
            "{what} declares a Core call, but no platform runs it on core"
        ));
    }
    let field = |key: &str| {
        run.get(key).map(|value| {
            name(
                &text(Some(value), &format!("{what}.{key}")),
                &format!("{what}.{key}"),
                false,
            )
        })
    };
    let (input, wrap, answer, output) = (
        field("input"),
        field("wrap"),
        field("answer"),
        field("output"),
    );
    if wrap.is_some() && input.is_none() {
        fail(format!("{what} wraps an input it does not name"));
    }
    if output.is_some() && answer.is_some() {
        fail(format!(
            "{what} answers the file Core writes or a data member, not both"
        ));
    }
    let switches = match run.get("switches") {
        None => Vec::new(),
        Some(_) => names(run.get("switches"), &format!("{what}.switches")),
    };
    let page_lanes = match run.get("page_lanes") {
        None => "None".to_owned(),
        Some(lanes) => match lanes.as_u64() {
            Some(lanes) if (1..=MAX_PAGE_LANES).contains(&lanes) => format!("Some({lanes})"),
            _ => fail(format!(
                "{what} bounds its page lanes outside 1..={MAX_PAGE_LANES}"
            )),
        },
    };
    format!(
        "Some(Run {{ input: {}, wrap: {}, answer: {}, output: {}, switches: &[{}], page_lanes: {page_lanes} }})",
        optional(input),
        optional(wrap),
        optional(answer),
        optional(output),
        switches.iter().map(|s| literal(s)).collect::<Vec<_>>().join(", ")
    )
}

/// The generated Rust for one manifest; panics with the gate it broke.
pub fn compile(json: &str, environment: &Environment) -> String {
    let manifest = parse(json);
    let platforms = &manifest.platforms;
    if platforms.is_empty() {
        fail("no platform is declared");
    }
    let ids: BTreeSet<&str> = platforms.iter().map(|p| p.id.as_str()).collect();
    if ids.len() != platforms.len() {
        fail("a platform is declared twice");
    }
    let mut features = BTreeSet::new();
    for platform in platforms {
        if let Some(feature) = &platform.feature {
            if platform.executors.is_empty() {
                fail(format!(
                    "platform `{}` has a Kernel build but no executor",
                    platform.id
                ));
            }
            if !(environment.feature_declared)(feature) {
                fail(format!(
                    "platform `{}` names the feature `{feature}`, which Cargo.toml does not declare",
                    platform.id
                ));
            }
            if !features.insert(feature) {
                fail(format!("two platforms name the feature `{feature}`"));
            }
        }
    }
    let defaults: Vec<&Platform> = platforms.iter().filter(|p| p.default).collect();
    let [default] = defaults.as_slice() else {
        fail("exactly one platform is the default build");
    };
    if default.feature.is_none() {
        fail(format!(
            "the default platform `{}` has no Kernel build",
            default.id
        ));
    }
    let enabled: Vec<&Platform> = platforms
        .iter()
        .filter(|p| {
            p.feature
                .as_deref()
                .is_some_and(environment.feature_enabled)
        })
        .collect();
    let served = match enabled.as_slice() {
        [] => default,
        [one] => one,
        many => fail(format!(
            "a Kernel build serves one platform, but {} are enabled",
            many.iter()
                .map(|p| p.feature.as_deref().unwrap_or_default())
                .collect::<Vec<_>>()
                .join(" and ")
        )),
    };
    if manifest.capabilities.is_empty() {
        fail("no capability is declared");
    }
    let mut missing: BTreeMap<&str, usize> = BTreeMap::new();
    for row in manifest.capabilities.values() {
        for platform in platforms {
            if !row.values.contains_key(&platform.id) {
                *missing.entry(&platform.id).or_default() += 1;
            }
        }
    }
    if !missing.is_empty() {
        fail(format!(
            "{} lack a value; run `{REWRITE}=1 cargo check` to give each `pending:<platform>_not_yet_supported`",
            missing
                .iter()
                .map(|(platform, rows)| format!("{rows} rows on `{platform}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let mut used = BTreeSet::new();
    let mut routes: Vec<String> = Vec::new();
    let mut rows = Vec::new();
    for (id, row) in &manifest.capabilities {
        if let Some(stray) = row.values.keys().find(|key| !ids.contains(key.as_str())) {
            fail(format!("`{id}` names the undeclared platform `{stray}`"));
        }
        let mut on_core = false;
        let mut columns = Vec::new();
        for platform in platforms {
            let what = format!("`{id}` on {}", platform.id);
            let (compiled, core) = route(
                &row.values[&platform.id],
                platform,
                &what,
                &manifest.reasons,
                &mut used,
                environment,
            );
            on_core |= core;
            let index = routes
                .iter()
                .position(|known| *known == compiled)
                .unwrap_or_else(|| {
                    routes.push(compiled);
                    routes.len() - 1
                });
            columns.push(index.to_string());
        }
        let run = row
            .run
            .as_ref()
            .map_or_else(|| "None".to_owned(), |run| run_spec(id, run, on_core));
        rows.push(format!(
            "    Capability {{ id: {}, routes: [{}], run: {run} }},",
            literal(id),
            columns.join(", ")
        ));
    }
    if let Some(unused) = manifest.reasons.keys().find(|key| !used.contains(*key)) {
        fail(format!(
            "the reason `{unused}` is declared but no row names it"
        ));
    }
    if routes.len() > usize::from(u8::MAX) {
        fail("more distinct routes than the table indexes");
    }
    if render(&manifest) != json {
        fail(format!(
            "{MANIFEST} is not in its canonical form; run `{REWRITE}=1 cargo check` to rewrite it"
        ));
    }
    let served_index = platforms
        .iter()
        .position(|p| p.id == served.id)
        .expect("the served platform is declared");
    let union = |accept: &dyn Fn(&str) -> bool| {
        let ids: Vec<_> = manifest
            .capabilities
            .iter()
            .filter(|(_, row)| accept(&row.values[&served.id]))
            .map(|(id, _)| quoted(id))
            .collect();
        if ids.is_empty() {
            "never".to_owned()
        } else {
            ids.join(" | ")
        }
    };
    let types = format!(
        "/** Generated from routing/capabilities.json for {platform}; do not edit. */\n\
         export type CapabilityId = {all};\n\
         export type CoreCapabilityId = {core};\n\
         export type RunnableCapabilityId = {runnable};\n\
         export type SupportedCapabilityId = {supported};\n",
        platform = served.id,
        all = union(&|_| true),
        core = union(&|route| route == "core"),
        runnable = union(&|route| matches!(route, "core" | "network_unit")),
        supported = union(&|route| route == "core"
            || route == "network_unit"
            || route.starts_with("kernel:")),
    );
    format!(
        "// Generated by build.rs from {MANIFEST}; do not edit.\n\
         /// Every declared platform, in manifest order.\n\
         pub const PLATFORMS: [&str; {count}] = [{platforms}];\n\
         /// The platform this Kernel build serves.\n\
         pub const PLATFORM: &str = {served};\n\
         const PLATFORM_INDEX: usize = {served_index};\n\
         static ROUTES: [Route; {route_count}] = [\n    {routes}\n];\n\
         /// Every capability, sorted by Core command id.\n\
         pub static CAPABILITIES: &[Capability] = &[\n{rows}\n];\n\
         #[cfg(all(feature = \"wasm\", target_arch = \"wasm32\"))]\n\
         #[wasm_bindgen::prelude::wasm_bindgen(typescript_custom_section)]\n\
         const CAPABILITY_ID_TYPES: &str = {types};\n",
        count = platforms.len(),
        platforms = platforms
            .iter()
            .map(|p| literal(&p.id))
            .collect::<Vec<_>>()
            .join(", "),
        served = literal(&served.id),
        route_count = routes.len(),
        routes = routes.join(",\n    "),
        rows = rows.join("\n"),
        types = literal(&types),
    )
}

/// Rewrite the manifest when asked, then write the compiled tables into
/// OUT_DIR for `src/capabilities.rs`.
pub fn emit(root: &Path) {
    let path = root.join(MANIFEST);
    println!("cargo:rerun-if-changed={}", path.display());
    println!("cargo:rerun-if-env-changed={REWRITE}");
    let mut json =
        fs::read_to_string(&path).expect("routing/capabilities.json is a Kernel build input");
    if env::var_os(REWRITE).is_some_and(|value| value == "1") {
        let mut manifest = parse(&json);
        fill(&mut manifest);
        let canonical = render(&manifest);
        if canonical != json {
            fs::write(&path, &canonical).expect("the manifest is writable");
            json = canonical;
        }
    }
    let cargo_toml = fs::read_to_string(root.join("Cargo.toml")).expect("Cargo.toml");
    let exchanges = root.join("src/gateway_exchange");
    let environment = Environment {
        exchange: &|exchange| exchanges.join(format!("{exchange}.rs")).is_file(),
        feature_declared: &|feature| cargo_toml.contains(&format!("\n{feature} = ")),
        feature_enabled: &|feature| {
            env::var_os(format!(
                "CARGO_FEATURE_{}",
                feature.to_uppercase().replace('-', "_")
            ))
            .is_some()
        },
    };
    let out = std::path::PathBuf::from(env::var("OUT_DIR").expect("Cargo sets OUT_DIR"));
    fs::write(
        out.join("capabilities_table.rs"),
        compile(&json, &environment),
    )
    .expect("the compiled capability table is writable");
}
