use std::path::Path;
use std::process::{Command as ProcessCommand, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::detect::{self, Platform};

pub static COMMAND: Command = Command {
    id: "workstation.verify",
    path: &["workstation", "verify"],
    contract: 1,
    chapter: Chapter::Workstation,
    summary: "Verify discovered executables and governed component receipts.",
    purpose: "Runs fixed harmless probes. LibreOffice verification requires executable identity/version and a task-owned headless HTML-to-PDF conversion; native Windows additionally reports package registration. Reference data requires its governed receipt and file hashes.",
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[crate::OPTIONAL_COMPONENT_ARG],
    output: "Per-component discovery and bounded verification result, including functional smoke and exact temporary cleanup evidence.",
    examples: &[Example {
        command: "ds workstation verify --component libreoffice --output json",
        note: "Creates and removes only a task-owned temporary smoke document.",
        runnable: true,
    }],
    refusals: &[crate::COMPONENT_UNKNOWN],
    reference: Some("docs/reference/workstation.md"),
    availability: crate::always,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let selected = inputs.value("component");
    let catalog = detect::catalog();
    if let Some(id) = selected
        && !catalog.iter().any(|component| component.id == id)
    {
        return Err(Failure::invalid(
            "workstation_component_unknown",
            format!("`{id}` is not a governed workstation component"),
        )
        .remedy(crate::COMPONENT_UNKNOWN.remedy));
    }
    let platform = Platform::current();
    let results = catalog
        .iter()
        .filter(|component| selected.is_none_or(|id| component.id == id))
        .map(|component| verify_component(component, platform))
        .collect::<Vec<_>>();
    Ok(json!({
        "platform": platform.token(),
        "mutated": false,
        "temporary_only": true,
        "results": results,
    }))
}

fn verify_component(component: &detect::Component, platform: Platform) -> Value {
    let snapshot = detect::snapshot(component, platform, true);
    if component.id == "rwanda-reference" {
        let verified = snapshot["receipt"]["verified"] == true;
        return json!({
            "id": component.id,
            "verified": verified,
            "proof": if verified { "receipt_and_file_hashes" } else { "not_proven" },
            "discovery": snapshot,
            "functional_smoke": null,
            "mutated": false,
        });
    }
    if component.id == "git-bash" && platform != Platform::Windows {
        return json!({
            "id": component.id,
            "verified": true,
            "proof": "not_applicable_native_shell",
            "discovery": snapshot,
            "functional_smoke": null,
            "mutated": false,
        });
    }
    if component.id == "libreoffice" {
        let smoke = snapshot["path"]
            .as_str()
            .ok_or_else(|| "LibreOffice executable was not discovered".to_string())
            .and_then(|path| libreoffice_smoke(Path::new(path)));
        let registration = if platform == Platform::Windows {
            match crate::install::libreoffice_registered() {
                Some(value) => {
                    json!({"state": if value { "registered" } else { "not_registered" }, "verified": value, "mechanism": "winget-list"})
                }
                None => {
                    json!({"state": "unknown", "verified": false, "mechanism": "winget-unavailable"})
                }
            }
        } else {
            json!({"state": "not_applicable", "verified": true, "mechanism": null})
        };
        let verified = snapshot["state"] == "installed"
            && snapshot["version"].is_string()
            && registration["verified"] == true
            && smoke.as_ref().is_ok_and(|value| value["passed"] == true);
        return json!({
            "id": component.id,
            "verified": verified,
            "proof": if verified { "registration_executable_version_and_headless_smoke" } else { "not_proven" },
            "discovery": snapshot,
            "registration": registration,
            "functional_smoke": smoke.unwrap_or_else(|reason| json!({"passed": false, "reason": reason})),
            "mutated": false,
        });
    }
    let verified = snapshot["state"] == "installed" && snapshot["version"].is_string();
    json!({
        "id": component.id,
        "verified": verified,
        "proof": if verified { "executable_and_version" } else { "not_proven" },
        "discovery": snapshot,
        "functional_smoke": null,
        "mutated": false,
    })
}

pub(crate) fn libreoffice_smoke(executable: &Path) -> Result<Value, String> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "ds-workstation-libreoffice-smoke-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir(&root)
        .map_err(|error| format!("smoke directory could not be created: {}", error.kind()))?;
    let source = root.join("smoke.html");
    let output = root.join("smoke.pdf");
    if let Err(error) = std::fs::write(
        &source,
        b"<!doctype html><meta charset=utf-8><title>DS smoke</title><p>DS workstation smoke</p>",
    ) {
        let _ = std::fs::remove_dir(&root);
        return Err(format!(
            "smoke input could not be created: {}",
            error.kind()
        ));
    }

    let outcome = run_conversion(executable, &root, &source).and_then(|status| {
        if status != 0 {
            return Err(format!("headless conversion exited with status {status}"));
        }
        let bytes = std::fs::metadata(&output)
            .map_err(|error| format!("headless conversion produced no PDF: {}", error.kind()))?
            .len();
        if bytes == 0 {
            return Err("headless conversion produced an empty PDF".to_string());
        }
        Ok(bytes)
    });
    let removed_count = [&output, &source]
        .into_iter()
        .filter(|path| std::fs::remove_file(path).is_ok())
        .count();
    let root_removed = std::fs::remove_dir(&root).is_ok();
    match outcome {
        Ok(output_bytes) if root_removed => Ok(json!({
            "passed": true,
            "operation": "headless-html-to-pdf",
            "output_bytes": output_bytes,
            "cleanup": {"removed_count": removed_count, "remaining": false},
        })),
        Ok(_) => {
            Err("headless conversion passed but task-owned cleanup was incomplete".to_string())
        }
        Err(reason) => Err(format!(
            "{reason}; task-owned cleanup remaining={}",
            !root_removed
        )),
    }
}

fn run_conversion(executable: &Path, root: &Path, source: &Path) -> Result<i32, String> {
    let mut child = ProcessCommand::new(executable)
        .arg("--headless")
        .arg("--convert-to")
        .arg("pdf")
        .arg("--outdir")
        .arg(root)
        .arg(source)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("headless conversion could not start: {}", error.kind()))?;
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status.code().unwrap_or(-1)),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("headless conversion timed out after 60 seconds".to_string());
            }
            Err(error) => {
                let _ = child.kill();
                return Err(format!("headless conversion wait failed: {}", error.kind()));
            }
        }
    }
}

pub fn render(data: &Value) -> String {
    let mut out = String::from("workstation verification · durable state unchanged\n");
    for result in data["results"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:<17} {} ({})\n",
            result["id"].as_str().unwrap_or("?"),
            if result["verified"].as_bool().unwrap_or(false) {
                "verified"
            } else {
                "not proven"
            },
            result["proof"].as_str().unwrap_or("unknown")
        ));
    }
    out
}
