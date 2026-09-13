//! `ds workstation install` — one proven native installation path.

use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::detect::{self, Platform};
use crate::policy::{self, INSTALL_RECEIPT_SCHEMA, InstallReceipt};

const LIBREOFFICE_PACKAGE_ID: &str = "TheDocumentFoundation.LibreOffice";
const RWANDA_SOURCE: &str = "NISR Village Boundary 2022 (Open Data)";
const RWANDA_SOURCE_URL: &str = "https://gis-server.statistics.gov.rw/server/rest/services/Hosted/Village_Boundary_2022/FeatureServer/3";
const RWANDA_LICENSE: &str =
    "NISR Open Data designation; attribution: Village boundary data produced by NISR in 2022";
const APPROVAL_ARG: Arg = Arg::value(
    "approval",
    "<interactive>",
    "Assert that the user is present to accept native UAC if Windows requests it.",
)
.choices(&["interactive"]);

const PACKAGE_MANAGER_MISSING: Refusal = Refusal {
    code: "workstation_package_manager_missing",
    when: "the proven Windows package manager executable is absent",
    remedy: "repair winget, then run the reviewed plan again",
};
const PACKAGE_MANAGER_FAILED: Refusal = Refusal {
    code: "workstation_package_manager_failed",
    when: "the trusted package-manager installation exits unsuccessfully or times out",
    remedy: "inspect package-manager diagnostics; use only an official manifest/hash fallback",
};
const DATASET_ACQUISITION_FAILED: Refusal = Refusal {
    code: "workstation_dataset_acquisition_failed",
    when: "the fixed official dataset service cannot be read, validated, or committed",
    remedy: "inspect the official NISR service availability and retry; do not substitute an ungoverned mirror",
};

pub static COMMAND: Command = Command {
    id: "workstation.install",
    path: &["workstation", "install"],
    contract: 1,
    chapter: Chapter::Workstation,
    summary: "Install one explicitly requested, proven workstation component.",
    purpose: "Installs the fixed LibreOffice package on native Windows or acquires the fixed official NISR Rwanda Village Boundary 2022 component. Existing verified components remain unchanged; all other acquisition paths fail closed.",
    effect: Effect::MachineWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[crate::COMPONENT_ARG, APPROVAL_ARG],
    output: "An idempotent receipt with source, change, verification, and task-ownership evidence.",
    examples: &[
        Example {
            command: "ds workstation install --component libreoffice --approval interactive --yes --output json",
            note: "Native Windows only; may show UAC and never bypasses it.",
            runnable: false,
        },
        Example {
            command: "ds workstation install --component rwanda-reference --yes --output json",
            note: "Acquires the fixed official 2022 NISR village boundary layer and writes its governed receipt.",
            runnable: false,
        },
    ],
    refusals: &[
        crate::COMPONENT_UNKNOWN,
        crate::MUTATION_UNSUPPORTED,
        crate::APPROVAL_REQUIRED,
        crate::SOURCE_UNVERIFIED,
        crate::VERIFICATION_FAILED,
        crate::RECEIPT_CONFLICT,
        PACKAGE_MANAGER_MISSING,
        PACKAGE_MANAGER_FAILED,
        DATASET_ACQUISITION_FAILED,
        Refusal {
            code: "confirmation_required",
            when: "--yes was not given for a machine installation",
            remedy: "review `ds workstation plan`, then re-run with --yes",
        },
    ],
    reference: Some("docs/reference/workstation.md"),
    availability: crate::always,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Decision {
    AlreadySatisfied,
    InstallLibreOffice,
    AcquireRwandaReference,
    /// A component the platform's own package catalog provides. Local tiling
    /// and local document conversion are capabilities a Linux desktop or
    /// server must own rather than call a cloud service for, so their
    /// prerequisites install from the distribution's signed metadata.
    InstallSystemPackage,
}

/// The package each system-provided component is known by. tippecanoe carries
/// no governed Windows distribution, so it is absent there by design and
/// Windows keeps using the deployed tiler.
fn system_package_name(component: &str, platform: Platform) -> Option<&'static str> {
    match (component, platform) {
        ("tippecanoe", Platform::Linux | Platform::Macos) => Some("tippecanoe"),
        ("pandoc", _) => Some(if platform == Platform::Windows {
            "JohnMacFarlane.Pandoc"
        } else {
            "pandoc"
        }),
        _ => None,
    }
}

fn decision(
    platform: Platform,
    component: &str,
    state: &str,
    approval: Option<&str>,
) -> Result<Decision, &'static str> {
    if state == "installed" {
        return Ok(Decision::AlreadySatisfied);
    }
    if component == "rwanda-reference" {
        return Ok(Decision::AcquireRwandaReference);
    }
    if system_package_name(component, platform).is_some() {
        return Ok(Decision::InstallSystemPackage);
    }
    if platform != Platform::Windows || component != "libreoffice" {
        return Err("unsupported");
    }
    if approval != Some("interactive") {
        return Err("approval");
    }
    Ok(Decision::InstallLibreOffice)
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let component_id = inputs.require("component")?;
    let component = detect::component(component_id).ok_or_else(|| {
        Failure::invalid(
            "workstation_component_unknown",
            format!("`{component_id}` is not a governed workstation component"),
        )
        .remedy(crate::COMPONENT_UNKNOWN.remedy)
    })?;
    let platform = Platform::current();
    let before = detect::snapshot(&component, platform, true);
    let state = before["state"].as_str().unwrap_or("unknown");
    match decision(platform, component_id, state, inputs.value("approval")) {
        Ok(Decision::AlreadySatisfied) => {
            let verification = if component_id == "libreoffice" {
                let registered = if platform == Platform::Windows {
                    crate::install::libreoffice_registered().unwrap_or(false)
                } else {
                    true
                };
                let smoke = before["path"]
                    .as_str()
                    .ok_or_else(|| "LibreOffice executable path is unavailable".to_string())
                    .and_then(|path| crate::verify::libreoffice_smoke(Path::new(path)));
                let verified = registered
                    && before["version"].is_string()
                    && smoke.as_ref().is_ok_and(|value| value["passed"] == true);
                if !verified {
                    return Err(Failure::failed(
                        "workstation_verification_failed",
                        "the existing LibreOffice installation was preserved but did not pass the complete proof",
                    )
                    .remedy(crate::VERIFICATION_FAILED.remedy)
                    .detail(json!({
                        "changed": false,
                        "registration_verified": registered,
                        "smoke": smoke.unwrap_or_else(|reason| json!({"passed": false, "reason": reason})),
                    })));
                }
                Some(json!({
                    "registration_verified": registered,
                    "version": before["version"],
                    "smoke": smoke.expect("verified existing smoke exists"),
                }))
            } else {
                None
            };
            return Ok(json!({
                "component": component_id,
                "platform": platform.token(),
                "action": "already_satisfied",
                "changed": false,
                "package_id": if component_id == "libreoffice" { Some(LIBREOFFICE_PACKAGE_ID) } else { None },
                "before": before,
                "after": before,
                "ownership": policy::install_ownership(platform, component_id),
                "reference_receipt": before.get("receipt").cloned().unwrap_or(Value::Null),
                "verification": verification,
                "temporary_cleanup": [],
            }));
        }
        Err("approval") => {
            return Err(Failure::unauthorized(
                "workstation_approval_required",
                "installation may require Windows UAC while the user is present",
            )
            .remedy(crate::APPROVAL_REQUIRED.remedy));
        }
        Err(_) => {
            return Err(Failure::unavailable(
                "workstation_mutation_unsupported",
                format!(
                    "installation of `{component_id}` is not proven on {}",
                    platform.token()
                ),
            )
            .remedy(crate::MUTATION_UNSUPPORTED.remedy));
        }
        Ok(Decision::InstallLibreOffice) => {}
        Ok(Decision::AcquireRwandaReference) => {
            return acquire_rwanda(platform, &before);
        }
        Ok(Decision::InstallSystemPackage) => {
            return install_system_package(component_id, &component, platform, &before);
        }
    }

    let receipt_path =
        policy::ensure_install_receipt_slot(platform, component_id).map_err(|reason| {
            Failure::conflict("workstation_receipt_conflict", reason)
                .remedy(crate::RECEIPT_CONFLICT.remedy)
        })?;
    let winget = find_on_path(&["winget.exe", "winget"]).ok_or_else(|| {
        cleanup_empty_receipt_parent(&receipt_path);
        Failure::unavailable(
            "workstation_package_manager_missing",
            "winget was not found on PATH",
        )
        .remedy(PACKAGE_MANAGER_MISSING.remedy)
    })?;
    let args = [
        "install",
        "--id",
        LIBREOFFICE_PACKAGE_ID,
        "--exact",
        "--silent",
        "--accept-package-agreements",
        "--accept-source-agreements",
        "--disable-interactivity",
    ];
    let status = run_quiet(&winget, &args, Duration::from_secs(30 * 60)).map_err(|reason| {
        cleanup_empty_receipt_parent(&receipt_path);
        Failure::failed("workstation_package_manager_failed", reason)
            .remedy(PACKAGE_MANAGER_FAILED.remedy)
            .detail(json!({"package_id": LIBREOFFICE_PACKAGE_ID}))
    })?;
    if status != 0 {
        cleanup_empty_receipt_parent(&receipt_path);
        return Err(Failure::failed(
            "workstation_package_manager_failed",
            format!("winget exited with status {status}"),
        )
        .remedy(PACKAGE_MANAGER_FAILED.remedy)
        .detail(json!({"package_id": LIBREOFFICE_PACKAGE_ID, "exit_code": status})));
    }

    let after = detect::snapshot(&component, platform, true);
    let registered = package_registered(&winget);
    let executable = after["path"].as_str().map(str::to_string);
    let version = after["version"].as_str().map(str::to_string);
    let smoke = match executable.as_deref() {
        Some(path) => crate::verify::libreoffice_smoke(Path::new(path)),
        None => Err("LibreOffice executable was not discovered after installation".to_string()),
    };
    let verified = registered
        && after["state"] == "installed"
        && version.is_some()
        && smoke.as_ref().is_ok_and(|value| value["passed"] == true);
    let receipt = InstallReceipt {
        schema: INSTALL_RECEIPT_SCHEMA.to_string(),
        run_id: format!("{}-{}", unix_seconds(), std::process::id()),
        component: component_id.to_string(),
        package_id: LIBREOFFICE_PACKAGE_ID.to_string(),
        source: "windows-package-manager".to_string(),
        installed_at_unix_s: unix_seconds(),
        task_owned: true,
        preexisting: false,
        executable: executable.clone(),
        version: version.clone(),
        verified,
        smoke: if verified { "passed" } else { "failed" }.to_string(),
    };
    policy::write_install_receipt(&receipt_path, &receipt).map_err(|reason| {
        Failure::failed("workstation_receipt_conflict", reason)
            .remedy(crate::RECEIPT_CONFLICT.remedy)
            .detail(json!({"installed": true, "package_id": LIBREOFFICE_PACKAGE_ID}))
    })?;
    if !verified {
        return Err(Failure::failed(
            "workstation_verification_failed",
            "LibreOffice installed but its complete registration/version/smoke proof failed",
        )
        .remedy(crate::VERIFICATION_FAILED.remedy)
        .detail(json!({
            "receipt": receipt_path.to_string_lossy(),
            "registered": registered,
            "after": after,
            "smoke": smoke.unwrap_or_else(|reason| json!({"passed": false, "reason": reason})),
        })));
    }
    Ok(json!({
        "component": component_id,
        "platform": platform.token(),
        "action": "installed",
        "changed": true,
        "source": "windows-package-manager",
        "package_id": LIBREOFFICE_PACKAGE_ID,
        "approval": "interactive",
        "before": before,
        "after": after,
        "registration": {"verified": registered, "mechanism": "winget-list"},
        "smoke": smoke.expect("verified smoke exists"),
        "ownership": policy::install_ownership(platform, component_id),
        "temporary_cleanup": [],
    }))
}

fn acquire_rwanda(platform: Platform, before: &Value) -> Result<Value, Failure> {
    let root = policy::component_root(platform).ok_or_else(|| {
        Failure::unavailable(
            "workstation_dataset_acquisition_failed",
            "the platform component directory is unavailable",
        )
        .remedy(DATASET_ACQUISITION_FAILED.remedy)
    })?;
    std::fs::create_dir_all(&root).map_err(|error| dataset_failure("component root", error))?;
    let destination = root.join("rwanda-reference");
    if destination.exists() {
        return Err(Failure::conflict(
            "workstation_receipt_conflict",
            format!(
                "an unverified Rwanda component already exists at {}",
                destination.display()
            ),
        )
        .remedy(crate::RECEIPT_CONFLICT.remedy));
    }
    let staging = root.join(format!(
        ".rwanda-reference-{}-{}",
        std::process::id(),
        unix_seconds()
    ));
    std::fs::create_dir(&staging).map_err(|error| dataset_failure("staging directory", error))?;
    let data_path = staging.join("villages.geojson");
    let acquisition = download_rwanda_geojson(&data_path);
    let feature_count = match acquisition {
        Ok(count) => count,
        Err(reason) => {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(
                Failure::unavailable("workstation_dataset_acquisition_failed", reason)
                    .remedy(DATASET_ACQUISITION_FAILED.remedy),
            );
        }
    };
    let digest = policy::sha256(&data_path).map_err(|reason| {
        let _ = std::fs::remove_dir_all(&staging);
        Failure::failed("workstation_dataset_acquisition_failed", reason)
            .remedy(DATASET_ACQUISITION_FAILED.remedy)
    })?;
    let receipt = json!({
        "component": "rwanda-reference",
        "version": "NISR-village-boundary-2022",
        "source": RWANDA_SOURCE_URL,
        "license": RWANDA_LICENSE,
        "installed_at": unix_seconds().to_string(),
        "task_owned": true,
        "files": [{"path": "villages.geojson", "sha256": digest}],
    });
    std::fs::write(
        staging.join("receipt.json"),
        serde_json::to_vec_pretty(&receipt).expect("bounded receipt encodes"),
    )
    .map_err(|error| {
        let _ = std::fs::remove_dir_all(&staging);
        dataset_failure("receipt", error)
    })?;
    std::fs::rename(&staging, &destination).map_err(|error| {
        let _ = std::fs::remove_dir_all(&staging);
        dataset_failure("component commit", error)
    })?;
    let component = detect::component("rwanda-reference").expect("catalogue component exists");
    let after = detect::snapshot(&component, platform, false);
    if after["receipt"]["verified"] != true {
        let cleaned = std::fs::remove_dir_all(&destination).is_ok();
        return Err(Failure::failed(
            "workstation_verification_failed",
            "the acquired Rwanda component receipt did not re-verify",
        )
        .remedy(crate::VERIFICATION_FAILED.remedy)
        .detail(json!({"after": after, "task_owned_cleanup_completed": cleaned})));
    }
    Ok(json!({
        "component": "rwanda-reference",
        "platform": platform.token(),
        "action": "acquired",
        "changed": true,
        "source": RWANDA_SOURCE_URL,
        "source_name": RWANDA_SOURCE,
        "version": "NISR-village-boundary-2022",
        "license": RWANDA_LICENSE,
        "feature_count": feature_count,
        "sha256": digest,
        "before": before,
        "after": after,
        "temporary_cleanup": [],
    }))
}

fn download_rwanda_geojson(path: &Path) -> Result<usize, String> {
    const PAGE_SIZE: usize = 2_000;
    const MAX_FEATURES: usize = 20_000;
    // National village polygons are deliberately requested at source precision.
    // The official service can be slow even on a healthy connection, so bound
    // memory and feature count but tolerate a long transfer instead of asking
    // ArcGIS to simplify coordinates.
    const MAX_PAGE_BYTES: u64 = 128 * 1024 * 1024;
    const PAGE_TIMEOUT: Duration = Duration::from_secs(20 * 60);
    let mut features = Vec::new();
    for offset in (0..MAX_FEATURES).step_by(PAGE_SIZE) {
        let url = rwanda_page_url(offset, PAGE_SIZE);
        let mut response = ureq::get(&url)
            .config()
            .timeout_global(Some(PAGE_TIMEOUT))
            .build()
            .call()
            .map_err(|error| format!("official NISR query failed: {error}"))?;
        let body = response
            .body_mut()
            .with_config()
            .limit(MAX_PAGE_BYTES)
            .read_to_string()
            .map_err(|error| format!("official NISR page could not be read: {error}"))?;
        let mut page: Value = serde_json::from_str(&body)
            .map_err(|error| format!("official NISR page was not GeoJSON: {error}"))?;
        if page["type"] != "FeatureCollection" {
            return Err("official NISR response was not a GeoJSON FeatureCollection".to_string());
        }
        let page_features = page["features"]
            .as_array_mut()
            .ok_or_else(|| "official NISR GeoJSON omitted features".to_string())?;
        let count = page_features.len();
        features.append(page_features);
        if count < PAGE_SIZE {
            if features.len() < 14_000 {
                return Err(format!(
                    "official NISR dataset returned only {} features; expected the governed national layer",
                    features.len()
                ));
            }
            let properties = features
                .first()
                .and_then(|feature| feature["properties"].as_object())
                .ok_or_else(|| "official NISR features omitted properties".to_string())?;
            for field in [
                "province_id",
                "district_id",
                "sector_id",
                "cell_id",
                "village_id",
            ] {
                if !properties.contains_key(field) {
                    return Err(format!(
                        "official NISR dataset omitted governed field `{field}`"
                    ));
                }
            }
            let document = json!({"type": "FeatureCollection", "features": features});
            let bytes = serde_json::to_vec(&document)
                .map_err(|error| format!("GeoJSON could not be encoded: {error}"))?;
            std::fs::write(path, bytes)
                .map_err(|error| format!("GeoJSON could not be persisted: {}", error.kind()))?;
            return Ok(document["features"].as_array().map_or(0, Vec::len));
        }
    }
    Err(format!(
        "official NISR dataset exceeded the governed {MAX_FEATURES}-feature bound"
    ))
}

fn rwanda_page_url(offset: usize, page_size: usize) -> String {
    format!(
        "{RWANDA_SOURCE_URL}/query?where=1%3D1&outFields=%2A&returnGeometry=true&f=geojson&outSR=4326&orderByFields=objectid&resultRecordCount={page_size}&resultOffset={offset}"
    )
}

fn dataset_failure(stage: &str, error: std::io::Error) -> Failure {
    Failure::failed(
        "workstation_dataset_acquisition_failed",
        format!("Rwanda {stage} failed: {}", error.kind()),
    )
    .remedy(DATASET_ACQUISITION_FAILED.remedy)
}

fn find_on_path(names: &[&str]) -> Option<PathBuf> {
    let names = names
        .iter()
        .map(|name| (*name).to_string())
        .collect::<Vec<_>>();
    detect::find_in_directories(&names, &detect::path_directories(std::env::var_os("PATH")))
}

/// The distribution package manager and the argument vector that installs
/// non-interactively under it. Detection is by executable presence, in a fixed
/// order, so the answer is the same on every run of the same machine.
fn linux_package_manager(package: &str) -> Option<(PathBuf, Vec<String>)> {
    let candidates: [(&str, &[&str]); 4] = [
        ("apt-get", &["install", "-y", "--no-install-recommends"]),
        ("dnf", &["install", "-y"]),
        ("zypper", &["--non-interactive", "install"]),
        ("pacman", &["-S", "--noconfirm"]),
    ];
    for (executable, flags) in candidates {
        if let Some(path) = find_on_path(&[executable]) {
            let mut args: Vec<String> = flags.iter().map(|flag| (*flag).to_string()).collect();
            args.push(package.to_string());
            return Some((path, args));
        }
    }
    None
}

/// Install a component the platform's own catalog provides.
///
/// An unattended install must never block on a credential prompt, so when this
/// process is not already root the elevation is probed with `sudo -n` first and
/// refused with a remedy rather than left waiting on a password nobody will
/// type.
fn install_system_package(
    component_id: &str,
    component: &detect::Component,
    platform: Platform,
    before: &Value,
) -> Result<Value, Failure> {
    let package = system_package_name(component_id, platform).ok_or_else(|| {
        Failure::invalid(
            "workstation_mutation_unsupported",
            format!(
                "`{component_id}` has no governed package on {}",
                platform.token()
            ),
        )
        .remedy(crate::MUTATION_UNSUPPORTED.remedy)
    })?;
    let receipt_path =
        policy::ensure_install_receipt_slot(platform, component_id).map_err(|reason| {
            Failure::conflict("workstation_receipt_conflict", reason)
                .remedy(crate::RECEIPT_CONFLICT.remedy)
        })?;

    let (executable, args, source) = match platform {
        Platform::Windows => {
            let winget = find_on_path(&["winget.exe", "winget"]).ok_or_else(|| {
                cleanup_empty_receipt_parent(&receipt_path);
                Failure::unavailable(
                    "workstation_package_manager_missing",
                    "winget was not found on PATH",
                )
                .remedy(PACKAGE_MANAGER_MISSING.remedy)
            })?;
            let args: Vec<String> = [
                "install",
                "--id",
                package,
                "--exact",
                "--silent",
                "--accept-package-agreements",
                "--accept-source-agreements",
                "--disable-interactivity",
            ]
            .iter()
            .map(|value| (*value).to_string())
            .collect();
            (winget, args, "windows-package-manager")
        }
        Platform::Macos => {
            let brew = find_on_path(&["brew"]).ok_or_else(|| {
                cleanup_empty_receipt_parent(&receipt_path);
                Failure::unavailable(
                    "workstation_package_manager_missing",
                    "brew was not found on PATH",
                )
                .remedy(PACKAGE_MANAGER_MISSING.remedy)
            })?;
            (
                brew,
                vec!["install".to_string(), package.to_string()],
                "macos-package-manager",
            )
        }
        Platform::Linux => {
            let (manager, mut args) = linux_package_manager(package).ok_or_else(|| {
                cleanup_empty_receipt_parent(&receipt_path);
                Failure::unavailable(
                    "workstation_package_manager_missing",
                    "no supported Linux package manager was found on PATH",
                )
                .remedy(PACKAGE_MANAGER_MISSING.remedy)
            })?;
            // Root already holds the rights; anyone else needs elevation that
            // cannot prompt, or this refuses instead of hanging forever.
            let is_root = std::env::var("USER")
                .map(|user| user == "root")
                .unwrap_or(false)
                || std::env::var("HOME")
                    .map(|home| home == "/root")
                    .unwrap_or(false);
            if is_root {
                (manager, args, "linux-package-manager")
            } else {
                let sudo = find_on_path(&["sudo"]).ok_or_else(|| {
                    cleanup_empty_receipt_parent(&receipt_path);
                    Failure::unavailable(
                        "workstation_package_manager_missing",
                        "installation needs root and sudo was not found on PATH",
                    )
                    .remedy("run as root, or install sudo")
                })?;
                if run_quiet(&sudo, &["-n", "true"], Duration::from_secs(15)) != Ok(0) {
                    cleanup_empty_receipt_parent(&receipt_path);
                    return Err(Failure::failed(
                        "workstation_package_manager_failed",
                        "sudo requires a password, and an unattended install must not wait for one",
                    )
                    .remedy("grant passwordless sudo for the package manager, or run this as root")
                    .detail(json!({"package_id": package, "component": component_id})));
                }
                let mut elevated = vec![manager.to_string_lossy().to_string()];
                elevated.append(&mut args);
                (sudo, elevated, "linux-package-manager")
            }
        }
    };

    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    let status =
        run_quiet(&executable, &borrowed, Duration::from_secs(30 * 60)).map_err(|reason| {
            cleanup_empty_receipt_parent(&receipt_path);
            Failure::failed("workstation_package_manager_failed", reason)
                .remedy(PACKAGE_MANAGER_FAILED.remedy)
                .detail(json!({"package_id": package}))
        })?;
    if status != 0 {
        cleanup_empty_receipt_parent(&receipt_path);
        return Err(Failure::failed(
            "workstation_package_manager_failed",
            format!("the package manager exited with status {status}"),
        )
        .remedy(PACKAGE_MANAGER_FAILED.remedy)
        .detail(json!({"package_id": package, "exit_code": status})));
    }

    // The installed executable's own version report is the proof. A package
    // manager reporting success while the executable stays undiscoverable is a
    // failed install, not a successful one.
    let after = detect::snapshot(component, platform, true);
    let executable_path = after["path"].as_str().map(str::to_string);
    let version = after["version"].as_str().map(str::to_string);
    let verified = after["state"] == "installed" && version.is_some();
    let receipt = InstallReceipt {
        schema: INSTALL_RECEIPT_SCHEMA.to_string(),
        run_id: format!("{}-{}", unix_seconds(), std::process::id()),
        component: component_id.to_string(),
        package_id: package.to_string(),
        source: source.to_string(),
        installed_at_unix_s: unix_seconds(),
        task_owned: true,
        preexisting: false,
        executable: executable_path,
        version,
        verified,
        smoke: if verified { "passed" } else { "failed" }.to_string(),
    };
    policy::write_install_receipt(&receipt_path, &receipt).map_err(|reason| {
        Failure::failed("workstation_receipt_conflict", reason)
            .remedy(crate::RECEIPT_CONFLICT.remedy)
            .detail(json!({"installed": true, "package_id": package}))
    })?;
    if !verified {
        return Err(Failure::failed(
            "workstation_verification_failed",
            format!("{component_id} installed but its executable and version proof failed"),
        )
        .remedy(crate::VERIFICATION_FAILED.remedy)
        .detail(json!({"receipt": receipt_path.to_string_lossy(), "after": after})));
    }
    Ok(json!({
        "component": component_id,
        "platform": platform.token(),
        "action": "installed",
        "changed": true,
        "source": source,
        "package_id": package,
        "before": before,
        "after": after,
        "ownership": policy::install_ownership(platform, component_id),
        "temporary_cleanup": [],
    }))
}

fn run_quiet(executable: &Path, args: &[&str], timeout: Duration) -> Result<i32, String> {
    let mut child = ProcessCommand::new(executable)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("package manager could not start: {}", error.kind()))?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status.code().unwrap_or(-1)),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "package manager timed out after {}s",
                    timeout.as_secs()
                ));
            }
            Err(error) => {
                let _ = child.kill();
                return Err(format!("package manager wait failed: {}", error.kind()));
            }
        }
    }
}

pub(crate) fn package_registered(winget: &Path) -> bool {
    run_quiet(
        winget,
        &[
            "list",
            "--id",
            LIBREOFFICE_PACKAGE_ID,
            "--exact",
            "--disable-interactivity",
        ],
        Duration::from_secs(60),
    )
    .is_ok_and(|status| status == 0)
}

pub(crate) fn libreoffice_registered() -> Option<bool> {
    find_on_path(&["winget.exe", "winget"]).map(|winget| package_registered(&winget))
}

fn cleanup_empty_receipt_parent(path: &Path) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::remove_dir(parent);
    }
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn render(data: &Value) -> String {
    format!(
        "{} · {} · {}\n",
        data["component"].as_str().unwrap_or("?"),
        data["action"].as_str().unwrap_or("?"),
        if data["changed"].as_bool().unwrap_or(false) {
            "machine changed"
        } else {
            "no change"
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn already_installed_is_idempotent_before_platform_or_approval_checks() {
        assert_eq!(
            decision(Platform::Linux, "libreoffice", "installed", None),
            Ok(Decision::AlreadySatisfied)
        );
        // Local tiling is a Linux capability: a desktop or server installs
        // tippecanoe itself instead of calling the deployed tiler. No approval
        // flag is required — the distribution's own signed catalog is the
        // source, and nothing prompts.
        assert_eq!(
            decision(Platform::Linux, "tippecanoe", "absent", None),
            Ok(Decision::InstallSystemPackage)
        );
        assert_eq!(
            decision(Platform::Linux, "pandoc", "absent", None),
            Ok(Decision::InstallSystemPackage)
        );
        // tippecanoe has no governed Windows distribution, so it is skipped
        // there by design; pandoc does, so Windows may install it.
        assert_eq!(
            decision(Platform::Windows, "tippecanoe", "absent", None),
            Err("unsupported")
        );
        assert_eq!(
            decision(Platform::Windows, "pandoc", "absent", None),
            Ok(Decision::InstallSystemPackage)
        );
        // An existing installation is still left untouched.
        assert_eq!(
            decision(Platform::Linux, "tippecanoe", "installed", None),
            Ok(Decision::AlreadySatisfied)
        );
        assert_eq!(
            decision(Platform::Windows, "git-bash", "installed", None),
            Ok(Decision::AlreadySatisfied)
        );
    }

    #[test]
    fn native_install_needs_both_a_proven_target_and_present_user() {
        assert_eq!(
            decision(
                Platform::Linux,
                "libreoffice",
                "absent",
                Some("interactive")
            ),
            Err("unsupported")
        );
        assert_eq!(
            decision(Platform::Windows, "libreoffice", "absent", None),
            Err("approval")
        );
        assert_eq!(
            decision(
                Platform::Windows,
                "libreoffice",
                "absent",
                Some("interactive")
            ),
            Ok(Decision::InstallLibreOffice)
        );
        assert_eq!(
            decision(Platform::Linux, "rwanda-reference", "absent", None),
            Ok(Decision::AcquireRwandaReference)
        );
    }

    #[test]
    fn rwanda_query_is_fixed_paginated_and_never_reduces_geometry_precision() {
        let url = rwanda_page_url(4_000, 2_000);
        assert!(url.starts_with(RWANDA_SOURCE_URL));
        assert!(url.contains("resultOffset=4000"));
        assert!(url.contains("resultRecordCount=2000"));
        assert!(!url.contains("geometryPrecision"));
        assert!(!url.contains("maxAllowableOffset"));
        assert!(!url.contains("quantization"));
    }
}
