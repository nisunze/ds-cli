//! Release-time engine build admission.
//!
//! Reads one bounded request on stdin and answers with the producer facts the
//! Kernel admits, or refuses with the typed reason an installation would give.
//! Native executables use platform `linux` or `windows`; browser engine and
//! UI units use platform `browser`. The decision is `src/module_engine.rs`,
//! compiled here unchanged.
use std::io::{Read, Write};
use std::process::ExitCode;

use serde::Deserialize;
use serde_json::{json, Value};

#[path = "../../../src/module_engine.rs"]
mod module_engine;

use module_engine::{
    admit_browser_engine_build, admit_engine_build, admit_ui_identity, browser_engine_profile,
    parse_ui_identity, platform_target, ModuleComponent, UiComponent, UI_PROTOCOL,
};

pub const REQUEST_SCHEMA: &str = "ds.module-build-admission-request/v1";
pub const RESPONSE_SCHEMA: &str = "ds.module-build-admission/v1";
const MAX_REQUEST_BYTES: u64 = 64 * 1024;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    schema: String,
    component: String,
    platform: String,
    build_info: Value,
}

fn admit(bytes: &[u8]) -> Result<Value, String> {
    if bytes.len() as u64 > MAX_REQUEST_BYTES {
        return Err("module_admission_request_invalid: request exceeds 64 KiB".into());
    }
    let request: Request = serde_json::from_slice(bytes).map_err(|_| {
        "module_admission_request_invalid: request is outside its closed shape".to_string()
    })?;
    if request.schema != REQUEST_SCHEMA {
        return Err("module_admission_request_invalid: unsupported request schema".into());
    }
    if request.platform == "browser" {
        return admit_browser(&request);
    }
    let component = ModuleComponent::from_name(&request.component).ok_or_else(|| {
        "module_admission_request_invalid: component must be solar, reporter or network".to_string()
    })?;
    let target = platform_target(&request.platform)?;
    let admitted = admit_engine_build(component, &request.platform, &request.build_info, None)?;
    let suffix = if request.platform == "windows" {
        ".exe"
    } else {
        ""
    };
    Ok(json!({
        "schema": RESPONSE_SCHEMA,
        "component": component.as_str(),
        "platform": request.platform,
        "target": target,
        "owner": component.engine_owner(),
        "protocol": component.protocol(),
        "filename": format!("{}{suffix}", component.executable()),
        "version": admitted.version,
        "sourceSha": admitted.source_sha,
        "manifestSha256": admitted.manifest_sha256,
        "release": admitted.release,
    }))
}

/// Browser units: the two engine units and the two UI units.
fn admit_browser(request: &Request) -> Result<Value, String> {
    let files = |stem: &str| {
        json!([
            format!("{stem}.js"),
            format!("{stem}.d.ts"),
            format!("{stem}_bg.wasm"),
            format!("{stem}_bg.wasm.d.ts")
        ])
    };
    if let Some(component) = UiComponent::from_name(&request.component) {
        let identity = parse_ui_identity(&request.build_info)?;
        admit_ui_identity(component, &identity, None)?;
        return Ok(json!({
            "schema": RESPONSE_SCHEMA,
            "component": component.as_str(),
            "platform": "browser",
            "target": "wasm32-unknown-unknown",
            "kind": "ui",
            "owner": component.owner(),
            "protocol": UI_PROTOCOL,
            "files": files(component.stem()),
            "sourceSha": identity.source_sha,
            "manifestSha256": identity.manifest_sha256(),
            "buildManifest": identity,
        }));
    }
    let component = ModuleComponent::from_name(&request.component)
        .filter(|component| browser_engine_profile(*component).is_some())
        .ok_or_else(|| {
            "module_admission_request_invalid: browser units are network, reporter, interactions or command-kernel".to_string()
        })?;
    let profile = browser_engine_profile(component).expect("filtered browser engine");
    let admitted = admit_browser_engine_build(component, &request.build_info, None)?;
    Ok(json!({
        "schema": RESPONSE_SCHEMA,
        "component": component.as_str(),
        "platform": "browser",
        "target": "wasm32-unknown-unknown",
        "kind": "engine",
        "owner": component.engine_owner(),
        "protocol": profile.protocol,
        "files": files(profile.stem),
        "version": admitted.version,
        "sourceSha": admitted.source_sha,
        "manifestSha256": admitted.manifest_sha256,
        "release": admitted.release,
    }))
}

fn main() -> ExitCode {
    if std::env::args_os().len() != 1 {
        eprintln!("usage: ds-module-admission < request.json (no arguments)");
        return ExitCode::from(2);
    }
    let mut input = Vec::new();
    if let Err(error) = std::io::stdin()
        .take(MAX_REQUEST_BYTES + 1)
        .read_to_end(&mut input)
    {
        eprintln!("module_admission_request_invalid: {error}");
        return ExitCode::from(2);
    }
    match admit(&input) {
        Ok(value) => {
            let mut stdout = std::io::stdout().lock();
            if writeln!(stdout, "{value}").is_err() {
                return ExitCode::from(2);
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
