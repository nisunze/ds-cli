//! Compiles `routing/capabilities.json`, the one capability manifest, into the
//! Kernel's constant tables. Every gate the manifest must pass runs here, so
//! a malformed, incomplete or hand-formatted manifest fails the build instead
//! of reaching a host.
//!
//! The manifest is its own canonical text. `DS_CAPABILITIES_REWRITE=1 cargo
//! check` rewrites it canonically and gives every row that lacks a declared
//! platform the value `pending:<platform>_not_yet_supported`, adding that
//! reason, so declaring a platform never means editing every row by hand.
//!
//! A row the page runs in-page names its one typed owner: `export`, the
//! wasm-bindgen JS name the page calls. The gate reads the crate's own
//! sources ([`wasm_exports`]), so a row naming an export the Kernel does not
//! define fails here, before any WASM link.

use serde_json::{Map, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
};

pub const SCHEMA: &str = "ds.capabilities/v1";
const MANIFEST: &str = "routing/capabilities.json";
const REWRITE: &str = "DS_CAPABILITIES_REWRITE";
/// The executors the Kernel has code for; a platform names the ones it has.
const EXECUTORS: [&str; 3] = ["core", "kernel", "network_unit"];
/// A row's members that are not platform values, in canonical order.
const ROW_MEMBERS: [&str; 2] = ["export", "run"];
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
    /// The Kernel module a `kernel` cell names exists: a gateway exchange
    /// `src/gateway_exchange/<name>.rs`, or an in-page operation `src/<name>.rs`.
    pub exchange: &'a dyn Fn(&str) -> bool,
    /// The sources that define this wasm-bindgen export (empty: none does).
    pub export: &'a dyn Fn(&str) -> Vec<String>,
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
    /// The wasm-bindgen JS name the page calls to run this row in-page.
    pub export: Option<String>,
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
        let mut export = None;
        let mut run = None;
        for (key, value) in object(row, &what) {
            if key == "run" {
                run = Some(object(value, &format!("{what} run")).clone());
            } else if key == "export" {
                export = Some(text(Some(value), &format!("{what} export")));
            } else {
                values.insert(key.clone(), text(Some(value), &format!("{what} on {key}")));
            }
        }
        (id, Row { values, export, run })
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
            if let Some(export) = &row.export {
                members.push(format!("\"export\": {}", quoted(export)));
            }
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
                            "{what} names the Kernel module `{exchange}`, which src/gateway_exchange does not have, nor src"
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
    if let Some(member) = ROW_MEMBERS.iter().find(|member| ids.contains(*member)) {
        fail(format!("a platform may not be named `{member}`, a row member"));
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
        let export = export_owner(id, row, platforms, environment);
        rows.push(format!(
            "    Capability {{ id: {}, routes: [{}], run: {run}, export: {} }},",
            literal(id),
            columns.join(", "),
            optional(export.map(str::to_owned))
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
    // The rows each export runs in this build's page: its Kernel and Network
    // unit cells, and where the platform runs the row on Core beside the
    // page, the page's own in-page run of it through the same typed owner.
    let in_page = |row: &Row| {
        let value = &row.values[&served.id];
        in_page_cell(value) || (value == "core" && served.executors.iter().any(|e| e == "kernel"))
    };
    let mut by_export: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for (id, row) in &manifest.capabilities {
        if let Some(export) = &row.export {
            let ids = by_export.entry(export).or_default();
            if in_page(row) {
                ids.push(quoted(id));
            }
        }
    }
    let exports: Vec<String> = by_export
        .iter()
        .map(|(export, ids)| {
            let union = if ids.is_empty() {
                "never".to_owned()
            } else {
                ids.join(" | ")
            };
            format!("    {export}: {union};")
        })
        .collect();
    let types = format!(
        "/** Generated from routing/capabilities.json for {platform}; do not edit. */\n\
         export type CapabilityId = {all};\n\
         export type CoreCapabilityId = {core};\n\
         export type RunnableCapabilityId = {runnable};\n\
         export type SupportedCapabilityId = {supported};\n\
         /** For each Kernel export, the capabilities this page runs through it. */\n\
         export interface CapabilityExports {{\n{exports}\n}}\n",
        platform = served.id,
        all = union(&|_| true),
        core = union(&|route| route == "core"),
        runnable = union(&|route| matches!(route, "core" | "network_unit")),
        supported = union(&|route| route == "core"
            || route == "network_unit"
            || route.starts_with("kernel:")),
        exports = exports.join("\n"),
    );
    let executors: Vec<String> = platforms
        .iter()
        .map(|platform| {
            format!(
                "&[{}]",
                platform
                    .executors
                    .iter()
                    .map(|executor| executor_variant(executor))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
        .collect();
    format!(
        "// Generated by build.rs from {MANIFEST}; do not edit.\n\
         /// Every declared platform, in manifest order.\n\
         pub const PLATFORMS: [&str; {count}] = [{platforms}];\n\
         /// Each platform's executors, in manifest order.\n\
         static EXECUTORS: [&[Executor]; {count}] = [{executors}];\n\
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
        executors = executors.join(", "),
        served = literal(&served.id),
        route_count = routes.len(),
        routes = routes.join(",\n    "),
        rows = rows.join("\n"),
        types = literal(&types),
    )
}

/// A cell the page runs itself: the in-page Kernel or its Network unit.
fn in_page_cell(value: &str) -> bool {
    value.starts_with("kernel:") || value == "network_unit"
}

/// A JS identifier, as wasm-bindgen exports it.
fn js_identifier(text: &str) -> bool {
    let mut chars = text.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
        && text.len() <= 96
}

/// The row's one typed owner in the page. A row runs on the Kernel on some
/// platform exactly when it names one, and that export must be one the
/// crate's sources define, once.
fn export_owner<'a>(
    id: &str,
    row: &'a Row,
    platforms: &[Platform],
    environment: &Environment,
) -> Option<&'a str> {
    let in_page: Vec<&str> = platforms
        .iter()
        .filter(|platform| in_page_cell(&row.values[&platform.id]))
        .map(|platform| platform.id.as_str())
        .collect();
    let Some(export) = row.export.as_deref() else {
        if !in_page.is_empty() {
            fail(format!(
                "`{id}` runs in the page on {} but names no `export`, the wasm-bindgen JS name the page calls",
                in_page.join(" and ")
            ));
        }
        return None;
    };
    if in_page.is_empty() {
        fail(format!(
            "`{id}` names the export `{export}`, but no platform runs it in the page"
        ));
    }
    if !js_identifier(export) {
        fail(format!("`{id}` export `{export}` is not a JS identifier"));
    }
    match (environment.export)(export).as_slice() {
        [] => fail(format!(
            "`{id}` names the export `{export}`, which no Kernel source exports"
        )),
        [_] => Some(export),
        many => fail(format!(
            "`{id}` names the export `{export}`, which {} all export",
            many.join(", ")
        )),
    }
}

/// The wasm-bindgen exports one Rust source defines at module level: free
/// functions, structs and enums (not class members inside an `impl`, not
/// `extern` imports, not parameter attributes, not the `start` function or
/// custom sections), each by its `js_name` or else its Rust identifier, with
/// the line its attribute starts on. A token scan that skips comments,
/// strings, raw strings and char literals, so a brace inside any of them
/// never shifts a block.
pub fn wasm_exports(source: &str) -> Vec<(String, usize)> {
    let bytes = source.as_bytes();
    let ident_start = |b: u8| b.is_ascii_alphabetic() || b == b'_';
    let ident_part = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut found = Vec::new();
    // For each open brace block: whether the items inside it are module items.
    let mut blocks: Vec<bool> = Vec::new();
    let mut parens = 0usize;
    // Identifier tokens since the last `;`, `{` or `}`.
    let mut statement: Vec<&str> = Vec::new();
    // A wasm-bindgen item attribute waiting for its item: (js_name, line, skip).
    let mut pending: Option<(Option<String>, usize, bool)> = None;
    let (mut i, mut line) = (0, 1);
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\n' {
            line += 1;
            i += 1;
        } else if b == b'/' && bytes.get(i + 1) == Some(&b'/') {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
        } else if b == b'/' && bytes.get(i + 1) == Some(&b'*') {
            let mut depth = 0;
            while i < bytes.len() {
                if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
                    depth += 1;
                    i += 2;
                } else if bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    line += usize::from(bytes[i] == b'\n');
                    i += 1;
                }
            }
        } else if b == b'"' {
            i = skip_string(bytes, i + 1, &mut line);
        } else if b == b'\'' {
            // A char literal ('x', '\n', '\'', '\u{..}', multi-byte) or a lifetime.
            if bytes.get(i + 1) == Some(&b'\\') {
                i += 3;
                while i < bytes.len() && bytes[i] != b'\'' {
                    i += 1;
                }
                i += 1;
            } else {
                let width = source
                    .get(i + 1..)
                    .and_then(|rest| rest.chars().next())
                    .map_or(1, char::len_utf8);
                i += if bytes.get(i + 1 + width) == Some(&b'\'') {
                    2 + width
                } else {
                    1
                };
            }
        } else if b == b'#' && matches!(bytes.get(i + 1), Some(b'[' | b'!')) {
            let inner = bytes[i + 1] == b'!';
            let open = i + 1 + usize::from(inner);
            if bytes.get(open) != Some(&b'[') {
                i += 1;
                continue;
            }
            let start_line = line;
            let mut depth = 0;
            let mut j = open;
            while j < bytes.len() {
                match bytes[j] {
                    b'"' => {
                        j = skip_string(bytes, j + 1, &mut line);
                        continue;
                    }
                    b'[' | b'(' => depth += 1,
                    b']' | b')' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    b'\n' => line += 1,
                    _ => {}
                }
                j += 1;
            }
            let attribute = &source[open..j.min(bytes.len())];
            if !inner && parens == 0 && is_wasm_bindgen(attribute) {
                let skip = attribute_flag(attribute, "start")
                    || attribute_flag(attribute, "typescript_custom_section");
                let name = js_name(attribute);
                pending = Some(match pending.take() {
                    Some((earlier, at, earlier_skip)) => (name.or(earlier), at, skip || earlier_skip),
                    None => (name, start_line, skip),
                });
            }
            i = j + 1;
        } else if let Some(end) = literal_prefix(bytes, i) {
            // A raw, byte or C string: r"..", r#".."#, br"..", b"..", c"..".
            i = end(&mut line);
        } else if b == b'(' {
            parens += 1;
            i += 1;
        } else if b == b')' {
            parens = parens.saturating_sub(1);
            i += 1;
        } else if b == b'{' {
            blocks.push(statement.len() >= 2 && statement[statement.len() - 2] == "mod");
            statement.clear();
            pending = None;
            i += 1;
        } else if b == b'}' {
            blocks.pop();
            statement.clear();
            pending = None;
            i += 1;
        } else if b == b';' {
            if parens == 0 {
                statement.clear();
                pending = None;
            }
            i += 1;
        } else if ident_start(b) && (i == 0 || !ident_part(bytes[i - 1])) {
            let start = i;
            while i < bytes.len() && ident_part(bytes[i]) {
                i += 1;
            }
            let word = &source[start..i];
            if parens == 0 {
                if let Some((name, at, skip)) = pending.as_ref() {
                    if matches!(statement.last(), Some(&("fn" | "struct" | "enum"))) {
                        if !skip && blocks.iter().all(|module| *module) {
                            found.push((name.clone().unwrap_or_else(|| word.to_owned()), *at));
                        }
                        pending = None;
                    } else if matches!(
                        word,
                        "impl" | "mod" | "use" | "static" | "type" | "trait" | "macro_rules" | "extern"
                    ) || (word == "const" && statement.is_empty())
                    {
                        pending = None;
                    }
                }
                statement.push(word);
            }
        } else {
            i += 1;
        }
    }
    found
}

/// When a raw, byte or C string starts at `i`: a step past it (counting its
/// lines).
fn literal_prefix(bytes: &[u8], i: usize) -> Option<impl FnOnce(&mut usize) -> usize + '_> {
    let first = *bytes.get(i)?;
    if !matches!(first, b'r' | b'b' | b'c') || (i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_')) {
        return None;
    }
    let mut k = i + 1;
    let raw = first == b'r' || (first == b'b' && bytes.get(k) == Some(&b'r'));
    if first == b'b' && bytes.get(k) == Some(&b'r') {
        k += 1;
    }
    let hashes = bytes[k.min(bytes.len())..]
        .iter()
        .take_while(|&&c| c == b'#')
        .count();
    if (hashes > 0 && !raw) || bytes.get(k + hashes) != Some(&b'"') {
        return None;
    }
    let body = k + hashes + 1;
    Some(move |line: &mut usize| {
        if !raw {
            return skip_string(bytes, body, line);
        }
        let mut k = body;
        while k < bytes.len() {
            if bytes[k] == b'"' && bytes[k + 1..].iter().take(hashes).filter(|&&c| c == b'#').count() == hashes {
                return k + 1 + hashes;
            }
            *line += usize::from(bytes[k] == b'\n');
            k += 1;
        }
        k
    })
}

/// The index just past a string literal whose body starts at `i`.
fn skip_string(bytes: &[u8], mut i: usize, line: &mut usize) -> usize {
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                *line += usize::from(bytes.get(i + 1) == Some(&b'\n'));
                i += 2;
            }
            b'"' => return i + 1,
            b'\n' => {
                *line += 1;
                i += 1;
            }
            _ => i += 1,
        }
    }
    i
}

/// Whether an attribute is a wasm-bindgen attribute, bare, pathed
/// (`wasm_bindgen::prelude::wasm_bindgen(..)`) or inside `cfg_attr`, and not
/// a sibling crate's (`wasm_bindgen_test`, `wasm_bindgen_futures`).
fn is_wasm_bindgen(attribute: &str) -> bool {
    let bytes = attribute.as_bytes();
    attribute.match_indices("wasm_bindgen").any(|(at, word)| {
        let before = at.checked_sub(1).map(|k| bytes[k]);
        let after = &attribute[at + word.len()..];
        !before.is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'"')
            && !after.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_' || c == '"')
            && !after.starts_with("::")
    })
}

/// Whether a bare word is one argument of the attribute.
fn attribute_flag(attribute: &str, flag: &str) -> bool {
    attribute
        .split(['(', ')', ','])
        .any(|part| part.trim() == flag)
}

/// The attribute's `js_name = X` or `js_name = "X"` (never `js_namespace`).
fn js_name(attribute: &str) -> Option<String> {
    attribute.match_indices("js_name").find_map(|(at, word)| {
        let rest = attribute[at + word.len()..].trim_start();
        let rest = rest.strip_prefix('=')?.trim_start();
        let rest = rest.strip_prefix('"').unwrap_or(rest);
        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '$')
            .collect();
        (!name.is_empty()).then_some(name)
    })
}

/// Every wasm-bindgen export the crate's `src` defines, by JS name, with the
/// sources (relative to `src`) that define it.
pub fn crate_exports(src: &Path) -> BTreeMap<String, Vec<String>> {
    fn walk(dir: &Path, files: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, files);
            } else if path.extension().is_some_and(|e| e == "rs") {
                files.push(path);
            }
        }
    }
    let mut files = Vec::new();
    walk(src, &mut files);
    files.sort();
    let mut exports: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for file in files {
        let Ok(source) = fs::read_to_string(&file) else {
            continue;
        };
        let name = file
            .strip_prefix(src)
            .unwrap_or(&file)
            .to_string_lossy()
            .replace('\\', "/");
        for (export, _) in wasm_exports(&source) {
            let files = exports.entry(export).or_default();
            if !files.contains(&name) {
                files.push(name.clone());
            }
        }
    }
    exports
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
    // build.rs already reruns on every change under src, so the scan is current.
    let exports = crate_exports(&root.join("src"));
    let environment = Environment {
        exchange: &|module| {
            exchanges.join(format!("{module}.rs")).is_file()
                || root.join("src").join(format!("{module}.rs")).is_file()
        },
        export: &|export| exports.get(export).cloned().unwrap_or_default(),
        feature_declared: &|feature| cargo_toml.contains(&format!("\n{feature} = ")),
        feature_enabled: &|feature| {
            env::var_os(format!(
                "CARGO_FEATURE_{}",
                feature.to_uppercase().replace('-', "_")
            ))
            .is_some()
        },
    };
    let out = PathBuf::from(env::var("OUT_DIR").expect("Cargo sets OUT_DIR"));
    fs::write(
        out.join("capabilities_table.rs"),
        compile(&json, &environment),
    )
    .expect("the compiled capability table is writable");
}
