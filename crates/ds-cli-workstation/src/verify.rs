use std::path::Path;
use std::process::{Command as ProcessCommand, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution, Requires};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::detect::{self, Platform};

pub static COMMAND: Command = Command {
    id: "workstation.verify",
    path: &["workstation", "verify"],
    contract: 1,
    chapter: Chapter::Workstation,
    summary: "Verify discovered executables and governed component receipts.",
    purpose: "Runs fixed harmless probes. The Linux tiling component requires the exact kernel-pinned Tippecanoe and PMTiles pair. LibreOffice and an existing Chrome, Edge, or Chromium browser require executable identity/version and a task-owned headless HTML-to-PDF conversion; native Windows LibreOffice additionally reports package registration. Other tools require executable identity/version, and reference data requires its governed receipt and file hashes.",
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
    search: &[],
    requires: Requires::Server,
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

pub(crate) fn verify_component(component: &detect::Component, platform: Platform) -> Value {
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
    if component.id == "chromium" {
        let smoke = snapshot["path"]
            .as_str()
            .ok_or_else(|| "Chromium executable was not discovered".to_string())
            .and_then(|path| chromium_smoke(Path::new(path), platform));
        let verified = snapshot["state"] == "installed"
            && snapshot["version"].is_string()
            && smoke.as_ref().is_ok_and(|value| value["passed"] == true);
        return json!({
            "id": component.id,
            "verified": verified,
            "proof": if verified { "executable_version_and_headless_pdf_smoke" } else { "not_proven" },
            "discovery": snapshot,
            "functional_smoke": smoke.unwrap_or_else(|reason| json!({"passed": false, "reason": reason})),
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
    if component.id == "tippecanoe" && platform == Platform::Linux {
        let verified = snapshot["state"] == "installed" && snapshot["suitable"] == true;
        return json!({
            "id": component.id,
            "verified": verified,
            "proof": if verified { "kernel_pinned_tippecanoe_and_pmtiles_versions" } else { "not_proven" },
            "discovery": snapshot,
            "functional_smoke": null,
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

/// Only local task-owned HTML enters this process. The Linux smoke uses the
/// same no-sandbox Chromium mode as the bundled report renderer because some
/// desktop/server AppArmor policies disable unprivileged user namespaces.
pub(crate) fn chromium_smoke(executable: &Path, platform: Platform) -> Result<Value, String> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "ds-workstation-chromium-smoke-{}-{nonce}",
        std::process::id()
    ));
    crate::policy::private_dir(&root)
        .map_err(|error| format!("smoke directory could not be created: {}", error.kind()))?;
    let source = root.join("smoke.html");
    let output = root.join("smoke.pdf");
    let result = (|| {
        crate::policy::private_write(
            &source,
            b"<!doctype html><meta charset=utf-8><title>DS smoke</title><h1>DS workstation smoke</h1>",
        )
        .map_err(|error| format!("smoke input could not be created: {}", error.kind()))?;
        let source_url = smoke_file_url(&source, platform)?;
        let mut command = ProcessCommand::new(executable);
        command.arg("--headless");
        if platform == Platform::Linux {
            command.arg("--no-sandbox");
        }
        let mut child = command
            .arg("--disable-gpu")
            .arg("--disable-background-networking")
            .arg("--no-first-run")
            .arg("--no-pdf-header-footer")
            .arg(format!(
                "--user-data-dir={}",
                root.join("profile").display()
            ))
            .arg(format!("--print-to-pdf={}", output.display()))
            .arg(source_url.as_str())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("headless Chromium could not start: {}", error.kind()))?;
        let deadline = Instant::now() + Duration::from_secs(30);
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(50))
                }
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("headless Chromium timed out after 30 seconds".to_string());
                }
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("headless Chromium wait failed: {}", error.kind()));
                }
            }
        };
        if !status.success() {
            return Err(format!(
                "headless Chromium exited {}",
                status.code().unwrap_or(-1)
            ));
        }
        let output_size = std::fs::metadata(&output)
            .map_err(|error| format!("headless Chromium produced no PDF: {}", error.kind()))?
            .len();
        if output_size > 2 * 1024 * 1024 {
            return Err("headless Chromium smoke PDF exceeded the 2 MiB limit".to_string());
        }
        let bytes = std::fs::read(&output)
            .map_err(|error| format!("headless Chromium produced no PDF: {}", error.kind()))?;
        if bytes.len() < 1_000 || !bytes.starts_with(b"%PDF-") {
            return Err("headless Chromium did not produce a valid PDF header".to_string());
        }
        Ok(bytes.len())
    })();
    let cleaned = std::fs::remove_dir_all(&root).is_ok();
    match (result, cleaned) {
        (Ok(output_bytes), true) => Ok(json!({
            "passed": true,
            "operation": "headless-html-to-pdf",
            "output_bytes": output_bytes,
            "cleanup": {"remaining": false},
        })),
        (Ok(_), false) => {
            Err("Chromium PDF smoke passed but task-owned cleanup was incomplete".to_string())
        }
        (Err(reason), true) => Err(reason),
        (Err(reason), false) => Err(format!("{reason}; task-owned cleanup remained")),
    }
}

fn smoke_file_url(path: &Path, platform: Platform) -> Result<url::Url, String> {
    if platform == Platform::Windows {
        // Build this explicitly so a Linux unit test can exercise Windows
        // drive letters and spaces without pretending Linux Path is Windows.
        let raw = path.to_string_lossy().replace('\\', "/");
        if raw.starts_with("//") {
            return url::Url::parse(&format!("file:{raw}"))
                .map_err(|_| "Windows UNC smoke path could not become a file URL".to_string());
        }
        if raw.len() >= 3
            && raw.as_bytes()[0].is_ascii_alphabetic()
            && raw.as_bytes()[1] == b':'
            && raw.as_bytes()[2] == b'/'
        {
            let mut url = url::Url::parse("file:///").expect("fixed file URL is valid");
            url.set_path(&format!("/{raw}"));
            return Ok(url);
        }
        return Err("Windows smoke path is not an absolute drive or UNC path".to_string());
    }
    url::Url::from_file_path(path)
        .map_err(|_| "smoke input path could not be represented as a file URL".to_string())
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
    crate::policy::private_dir(&root)
        .map_err(|error| format!("smoke directory could not be created: {}", error.kind()))?;
    let source = root.join("smoke.html");
    let output = root.join("smoke.pdf");
    if let Err(error) = crate::policy::private_write(
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
        if result["id"] == "chromium"
            && let Some(path) = result["discovery"]["path"].as_str()
        {
            out.push_str(&format!("    {path}\n"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // Run on the owning Linux workstation to prove the real installed browser.
    // CI hosts need not carry a browser just to compile the workstation CLI.
    #[test]
    #[ignore = "requires a local Chromium executable"]
    fn installed_chromium_prints_a_real_pdf_and_cleans_its_scratch() {
        let component = detect::component("chromium").unwrap();
        let result = verify_component(&component, Platform::Linux);
        assert_eq!(result["verified"], true, "{result}");
        assert_eq!(result["functional_smoke"]["cleanup"]["remaining"], false);
    }

    #[test]
    fn windows_smoke_file_url_encodes_drive_paths_and_spaces() {
        let url = smoke_file_url(
            Path::new(r"C:\Users\Nixon Mages\AppData\Local\Temp\ds smoke\smoke.html"),
            Platform::Windows,
        )
        .unwrap();
        assert_eq!(
            url.as_str(),
            "file:///C:/Users/Nixon%20Mages/AppData/Local/Temp/ds%20smoke/smoke.html"
        );
    }
}
