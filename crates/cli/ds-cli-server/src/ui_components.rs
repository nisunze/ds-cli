//! Typed Core CLI projection of opaque UI installation. This OS-user scope
//! bootstraps before sign-in; it grants no project or engineering authority.
use crate::core_components::{CHANNEL, REFUSALS, RUNTIME, owner_failure};
use base64::Engine;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_command_kernel::module_install::ActivationGuard;
use ds_command_kernel::module_release::ui::{UiComponent, UiScope};
use ds_update_runtime::modules::ui::{MAX_MEMBER_CHUNK, UiModuleStore};
use serde_json::{Value, json};

const UI_COMPONENT: Arg = Arg::value(
    "component",
    "<interactions|command-kernel|network>",
    "Exact fixed UI component owner, or the Network page engine unit.",
)
.choices(&["interactions", "command-kernel", "network"])
.required();

pub(crate) const SCOPE: Arg = Arg::value(
    "scope",
    "<release|local-dev>",
    "Release selection or the separate Linux canary local-dev UI selection.",
)
.choices(&["release", "local-dev"])
.default("release");

fn scope(inputs: &Inputs) -> Result<UiScope, Failure> {
    UiScope::parse(inputs.value("scope").unwrap_or("release"))
        .map_err(|e| owner_failure(e.to_string()))
}

pub fn requested(inputs: &Inputs) -> Result<bool, Failure> {
    let runtime = inputs.value("runtime").unwrap_or("native");
    let component = inputs.require("component")?;
    let ui = matches!(component, "interactions" | "command-kernel");
    if !ui && scope(inputs)? != UiScope::Release {
        return Err(owner_failure(
            "module_scope_invalid: native components have no local UI scope".into(),
        ));
    }
    if !matches!(runtime, "native" | "wasm") {
        return Err(owner_failure(
            "invalid_choice: select native or wasm".into(),
        ));
    }
    if (runtime == "wasm" && !ui) || (runtime == "native" && ui) {
        return Err(owner_failure("module_release_component_invalid: UI installation requires explicit runtime wasm; native engines remain native".into()));
    }
    Ok(ui)
}
fn selected(inputs: &Inputs) -> Result<(UiComponent, &str), Failure> {
    if !requested(inputs)? {
        return Err(owner_failure(
            "module_release_component_invalid: this operation selects UI WASM only".into(),
        ));
    }
    #[cfg(windows)]
    ds_cli_auth::windows_host::register_ui_store_owner().map_err(owner_failure)?;
    ds_update_runtime::modules::ui::ui_platform().map_err(owner_failure)?;
    let component = UiComponent::parse(inputs.require("component")?)
        .map_err(|e| owner_failure(e.to_string()))?;
    let channel = inputs.require("channel")?;
    if !matches!(channel, "stable" | "canary") {
        return Err(owner_failure(
            "invalid_choice: select stable or canary".into(),
        ));
    }
    if scope(inputs)? == UiScope::LocalDev && channel != "canary" {
        return Err(owner_failure(
            "module_scope_invalid: local UI builds cannot enter the stable release lane".into(),
        ));
    }
    Ok((component, channel))
}
fn generation(inputs: &Inputs) -> Result<u64, Failure> {
    let raw = inputs.require("expected-generation")?;
    raw.parse()
        .ok()
        .filter(|_| !raw.is_empty() && raw.bytes().all(|b| b.is_ascii_digit()))
        .ok_or_else(|| {
            owner_failure("component_generation_invalid: use an unsigned decimal generation".into())
        })
}
fn value(value: impl serde::Serialize) -> Result<Value, Failure> {
    serde_json::to_value(value)
        .map_err(|e| Failure::internal("module_result_invalid", e.to_string()))
}
pub fn stage(inputs: &Inputs) -> Result<Value, Failure> {
    let (component, channel) = selected(inputs)?;
    let digest = inputs.require("sha256")?;
    if !ds_cli_contract::util::is_sha256_hex(digest, ds_cli_contract::util::HexCase::Lower) {
        return Err(crate::module_updates::native_failure(
            "module_release_invalid: name the exact lowercase UI unit SHA-256".into(),
        ));
    }
    let authority =
        ds_cli_auth::module_updates::capture_release_authority(ds_cli_auth::Lane::parse(channel)?)?;
    let grant = authority
        .acquire_ui(component, digest, &|| false)
        .map_err(crate::module_updates::native_failure)?;
    let store = UiModuleStore::managed(component, channel).map_err(owner_failure)?;
    value(
        store
            .stage(
                grant,
                |progress| {
                    ds_cli_contract::output::note(&format!(
                        "{}: {} {}% ({}/{})",
                        component.as_str(),
                        progress.status,
                        progress.progress,
                        progress.bytes_downloaded,
                        progress.bytes_total
                    ))
                },
                &|| false,
            )
            .map_err(crate::module_updates::native_failure)?,
    )
}
pub fn selection(inputs: &Inputs) -> Result<Value, Failure> {
    let (component, channel) = selected(inputs)?;
    value(
        UiModuleStore::selection_managed_scoped(component, channel, scope(inputs)?)
            .map_err(owner_failure)?,
    )
}

const UI_ONLY: Arg = Arg::value(
    "component",
    "<interactions|command-kernel>",
    "Exact fixed UI component owner.",
)
.choices(&["interactions", "command-kernel"])
.required();

/// Every code this composite can return: catalog admission and staging
/// (core.module-stage), installed-page admission and activation.
const INSTALL_REFUSALS: &[Refusal] = &[
    Refusal {
        code: "ui_component_platform_unavailable",
        when: "UI installation runs on an unqualified native platform",
        remedy: "use Linux x86_64 or the registered Windows x86_64 SID/DACL owner",
    },
    Refusal {
        code: "module_release_component_invalid",
        when: "the component is not a UI unit or the runtime is not wasm",
        remedy: "select interactions or command-kernel with runtime wasm",
    },
    Refusal {
        code: "invalid_choice",
        when: "component or channel is outside the declared closed choices",
        remedy: "select interactions or command-kernel and stable or canary",
    },
    Refusal {
        code: "module_scope_invalid",
        when: "a UI release selection crosses its scope or lane",
        remedy: "install published units into the release scope of the installed lane",
    },
    Refusal {
        code: "module_authority_unavailable",
        when: "the packaged native profile cannot authorize a public release read",
        remedy: "use the exact packaged production native profile for this installed lane",
    },
    Refusal {
        code: "native_profile_not_configured",
        when: "this binary has no complete packaged native client catalog",
        remedy: "install a complete release with its digest-pinned native client profiles",
    },
    Refusal {
        code: "native_profile_unsafe",
        when: "the packaged native catalog is missing, unsafe, excessive or malformed",
        remedy: "reinstall one complete release with its native client profiles",
    },
    Refusal {
        code: "native_profile_digest_mismatch",
        when: "the packaged native catalog bytes differ from this binary's pin",
        remedy: "reinstall one complete release with its exact native client profiles",
    },
    Refusal {
        code: "module_release_unavailable",
        when: "the one public release read fails",
        remedy: "retain the installed unit and retry once connectivity is restored",
    },
    Refusal {
        code: "module_release_unpublished",
        when: "the lane publishes no release of this UI unit",
        remedy: "retain the installed unit until its owner publishes a qualified release",
    },
    Refusal {
        code: "module_release_inactive",
        when: "the published release is withdrawn, stopped or blocked",
        remedy: "retain the installed unit until an active qualified release exists",
    },
    Refusal {
        code: "module_release_invalid",
        when: "the release response, unit identity or digest is malformed or crossed",
        remedy: "retain the installed unit and obtain a corrected qualified publication",
    },
    Refusal {
        code: "module_release_schema_invalid",
        when: "the release names an unsupported schema",
        remedy: "obtain the fixed owner's supported UI release schema",
    },
    Refusal {
        code: "module_release_identity_mismatch",
        when: "the release crosses component, lane or product identity",
        remedy: "retain the lane and obtain its exact owner publication",
    },
    Refusal {
        code: "module_release_provenance_invalid",
        when: "the release has incomplete or crossed producer source closure",
        remedy: "obtain the owner's complete qualified producer provenance",
    },
    Refusal {
        code: "module_release_url_invalid",
        when: "the release names a mutable or unadmitted member URL",
        remedy: "obtain an immutable public HTTPS owner publication",
    },
    Refusal {
        code: "module_artifact_invalid",
        when: "downloaded member bytes differ from the admitted unit",
        remedy: "retain the installed unit and retry; repeated refusal needs a corrected publication",
    },
    Refusal {
        code: "ui_unit_incompatible_with_installed_page",
        when: "the active release lacks exports the installed Desktop page calls",
        remedy: "retain the installed unit until a release providing them is published",
    },
    Refusal {
        code: "ui_page_requirements_invalid",
        when: "the recorded installed-page requirements are malformed",
        remedy: "reinstall the Desktop so its page records valid requirements",
    },
    Refusal {
        code: "module_selection_changed",
        when: "the selection changed while this install ran",
        remedy: "run the install again; the newer selection is retained",
    },
    Refusal {
        code: "module_activation_deferred",
        when: "native observations defer activation",
        remedy: "finish the owning native session, then run the install again",
    },
    Refusal {
        code: "module_update_busy",
        when: "another update holds this UI component and lane",
        remedy: "wait for that update to finish",
    },
    Refusal {
        code: "module_update_cancelled",
        when: "cancellation interrupts the install before activation",
        remedy: "run the install again when ready; verified bytes may be reused",
    },
    Refusal {
        code: "module_store_invalid",
        when: "the native UI store is linked, unavailable or unsafe",
        remedy: "retain the current selection and repair its native owned store",
    },
    Refusal {
        code: "module_receipt_invalid",
        when: "retained UI receipts or selection are corrupt or crossed",
        remedy: "retain the current selection and repair its exact UI installation",
    },
    Refusal {
        code: "module_stage_failed",
        when: "verified transfer, staging or activation cannot complete",
        remedy: "retain the current selection and resolve the named native failure",
    },
];
fn install_failure(message: String) -> Failure {
    let code = message.split(':').next().unwrap_or_default();
    let refusal = INSTALL_REFUSALS
        .iter()
        .find(|refusal| refusal.code == code)
        .unwrap_or_else(|| {
            INSTALL_REFUSALS
                .iter()
                .find(|refusal| refusal.code == "module_stage_failed")
                .expect("declared fallback")
        });
    let bounded: String = message.chars().take(1024).collect();
    let failure = match refusal.code {
        "module_release_component_invalid"
        | "invalid_choice"
        | "module_scope_invalid"
        | "module_release_invalid"
        | "module_release_schema_invalid"
        | "module_release_identity_mismatch"
        | "module_release_provenance_invalid"
        | "module_release_url_invalid" => Failure::invalid(refusal.code, bounded),
        "module_selection_changed" | "module_update_busy" => {
            Failure::conflict(refusal.code, bounded)
        }
        "ui_component_platform_unavailable"
        | "module_authority_unavailable"
        | "native_profile_not_configured"
        | "module_release_unavailable"
        | "module_release_unpublished"
        | "module_release_inactive"
        | "ui_unit_incompatible_with_installed_page"
        | "module_activation_deferred" => Failure::unavailable(refusal.code, bounded),
        _ => Failure::failed(refusal.code, bounded),
    };
    failure.remedy(refusal.remedy)
}

pub static INSTALL_LATEST: Command = Command {
    id: "core.component.install-latest",
    path: &["core", "component", "install-latest"],
    contract: 1,
    summary: "Install the active published UI unit before sign-in.",
    purpose: "First run and recovery for Interactions or UI Kernel, with no UI unit loaded. Reads the packaged lane's fixed public catalog once, admits the active release with its complete trusted identity, stages all four members by exact digest, admits the installed page's exports and activates at the current generation. Absent or removed units are installed; a current unit is kept without download. No login, project data or JavaScript/WASM evaluation.",
    chapter: Chapter::Workstation,
    effect: Effect::MachineWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        UI_ONLY,
        CHANNEL,
        Arg::value("runtime", "<wasm>", "UI units are WASM.")
            .choices(&["wasm"])
            .default("wasm"),
    ],
    output: "ds.ui-module-convergence/v1: component, channel, outcome (installed, updated or current), generation, releaseVersion and unitSha256. Step progress goes to stderr as `<component>: <step> <percent>% (<bytes>/<total>)`.",
    examples: &[],
    refusals: INSTALL_REFUSALS,
    reference: Some("docs/reference/core.md"),
    search: &["recover", "bootstrap", "first-run"],
    requires: Requires::Server,
    availability: ui_availability,
};
fn note(component: UiComponent, step: &str, progress: u8, done: u64, total: u64) {
    ds_cli_contract::output::note(&format!(
        "{}: {step} {progress}% ({done}/{total})",
        component.as_str()
    ));
}
pub fn install_latest(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
    if !context.confirmed {
        return Err(Failure::invalid(
            "confirmation_required",
            "Confirm UI installation with --yes",
        ));
    }
    if inputs.value("runtime").unwrap_or("wasm") != "wasm" {
        return Err(install_failure(
            "module_release_component_invalid: UI installation requires runtime wasm".into(),
        ));
    }
    #[cfg(windows)]
    ds_cli_auth::windows_host::register_ui_store_owner().map_err(install_failure)?;
    ds_update_runtime::modules::ui::ui_platform().map_err(install_failure)?;
    let component = UiComponent::parse(inputs.require("component")?)
        .map_err(|e| install_failure(e.to_string()))?;
    let channel = inputs.require("channel")?;
    let lane = ds_cli_auth::Lane::parse(channel)?;
    note(component, "checking", 0, 0, 0);
    let authority = ds_cli_auth::module_updates::capture_release_authority(lane)?;
    let grant = authority
        .acquire_active_ui(component, &|| false)
        .map_err(install_failure)?;
    let store = UiModuleStore::managed(component, channel).map_err(install_failure)?;
    let mut total = 0;
    let installed = store
        .install_active(
            grant,
            guard(),
            |progress| {
                total = progress.bytes_total;
                note(
                    component,
                    progress.status,
                    progress.progress,
                    progress.bytes_downloaded,
                    progress.bytes_total,
                )
            },
            &|| false,
        )
        .map_err(install_failure)?;
    note(component, "installed", 100, total, total);
    value(installed)
}
pub static STAGE_LOCAL: Command = Command {
    id: "core.component.stage-local",
    path: &["core", "component", "stage-local"],
    contract: 1,
    summary: "Stage one built local UI or Network page unit, unpublished.",
    purpose: "Explicit Linux canary local-dev operation over four fixed opaque producer files, their actual exported UI identity, exact source snapshot and tested WASM digest. The Network page engine unit (component network) takes the identity its tested bytes export (network_engine_identity), whose sourceSha must equal --source-sha. Core admits the identity and hashes, captures the same bytes into its separate private local-dev store and returns a generation-fenced ready receipt. No gateway, publication, build, JavaScript/WASM evaluation, release selection or project access occurs. The Desktop must select local-dev explicitly; this artifact cannot be promoted into a release selection.",
    chapter: Chapter::Workstation,
    effect: Effect::MachineWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        UI_COMPONENT,
        CHANNEL,
        Arg::value(
            "runtime",
            "<wasm>",
            "The fixed UI WASM owners and the Network page engine unit.",
        )
        .choices(&["wasm"])
        .default("wasm"),
        Arg::value(
            "scope",
            "<local-dev>",
            "Separate local development selection; never a release.",
        )
        .choices(&["local-dev"])
        .default("local-dev"),
        Arg::value(
            "directory",
            "<absolute-directory>",
            "Four already built fixed JS/WASM/type members.",
        )
        .required(),
        Arg::value(
            "identity-file",
            "<absolute-json-file>",
            "Actual uiEngineBuildIdentity output from these tested bytes.",
        )
        .required(),
        Arg::value(
            "source-sha",
            "<git-snapshot-sha>",
            "Exact captured source snapshot stamped into the built identity.",
        )
        .required(),
        Arg::value(
            "wasm-sha256",
            "<sha256>",
            "Exact SHA-256 of the already tested WASM artifact.",
        )
        .required(),
    ],
    output: "ds.ui-module-stage/v1 ready receipt, complete local unit SHA-256 and expectedGeneration. No active selection changes.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/core.md"),
    search: &["wasm", "development"],
    requires: Requires::Server,
    availability: ui_availability,
};
pub fn stage_local(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
    if crate::engine_units::requested(inputs)? {
        return crate::engine_units::stage_local(inputs, context);
    }
    if !context.confirmed {
        return Err(Failure::invalid(
            "confirmation_required",
            "Confirm local UI staging with --yes",
        ));
    }
    let (component, channel) = selected(inputs)?;
    if scope(inputs)? != UiScope::LocalDev {
        return Err(owner_failure(
            "module_scope_invalid: local builds require explicit local-dev scope".into(),
        ));
    }
    value(
        UiModuleStore::managed_scoped(component, channel, UiScope::LocalDev)
            .map_err(owner_failure)?
            .stage_local(
                std::path::Path::new(inputs.require("directory")?),
                std::path::Path::new(inputs.require("identity-file")?),
                inputs.require("source-sha")?,
                inputs.require("wasm-sha256")?,
                &|| false,
            )
            .map_err(owner_failure)?,
    )
}
// These observations belong to this foreground OS-user installer: it owns no
// editor sessions, jobs, or restart-dependent UI execution. Existing loaded UI
// captures remain immutable and the loader explicitly adopts a later capture.
fn guard() -> ActivationGuard {
    ActivationGuard::default()
}
pub fn activate(inputs: &Inputs) -> Result<Value, Failure> {
    let (component, channel) = selected(inputs)?;
    let generation = generation(inputs)?;
    let id = inputs.require("receipt-id")?;
    if id.len() != 32
        || !id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(owner_failure(
            "module_receipt_invalid: name the exact native UI staging receipt".into(),
        ));
    }
    value(
        UiModuleStore::managed_scoped(component, channel, scope(inputs)?)
            .map_err(owner_failure)?
            .activate(id, generation, guard(), &|| false)
            .map_err(owner_failure)?,
    )
}
pub fn remove(inputs: &Inputs) -> Result<Value, Failure> {
    let (component, channel) = selected(inputs)?;
    let generation = generation(inputs)?;
    value(
        UiModuleStore::retained_managed_scoped(component, channel, scope(inputs)?)
            .map_err(owner_failure)?
            .remove(generation, guard(), &|| false)
            .map_err(owner_failure)?,
    )
}
pub fn rollback(inputs: &Inputs) -> Result<Value, Failure> {
    let (component, channel) = selected(inputs)?;
    let generation = generation(inputs)?;
    value(
        UiModuleStore::retained_managed_scoped(component, channel, scope(inputs)?)
            .map_err(owner_failure)?
            .rollback(generation, guard(), &|| false)
            .map_err(owner_failure)?,
    )
}

fn ui_availability() -> ds_cli_contract::spec::Availability {
    #[cfg(windows)]
    if let Err(reason) = ds_cli_auth::windows_host::register_ui_store_owner() {
        return ds_cli_contract::spec::Availability::unavailable(
            "ui_component_platform_unavailable",
            reason,
            "use the qualified Windows SID/DACL owner",
        );
    }
    match ds_update_runtime::modules::ui::ui_platform() {
        Ok(()) => ds_cli_contract::spec::Availability::Available,
        Err(reason) => ds_cli_contract::spec::Availability::unavailable(
            "ui_component_platform_unavailable",
            reason,
            "use qualified Linux x86_64 storage or the Windows x86_64 SID/DACL owner",
        ),
    }
}
pub static CAPTURE: Command = Command {
    id: "core.component.capture",
    path: &["core", "component", "capture"],
    contract: 1,
    summary: "Capture one installed UI or Network page unit for the Desktop.",
    purpose: "Read the OS-user owned UI selection offline and verify all four retained fixed members under the shared updater lock. Return its immutable installation receipt, whole-unit digest and generation. Later updates do not change this capture. No catalog, install, JS/WASM evaluation, login or project access occurs. Explicit runtime wasm is required. Component network captures the Network page engine unit.",
    chapter: Chapter::Workstation,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[UI_COMPONENT, CHANNEL, RUNTIME, SCOPE],
    output: "UiCapture with generation and complete ds.ui-module-installation/v1 receipt. receipt.unit.unit_sha256 pins the four member reads; no private filesystem path is exposed.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/core.md"),
    search: &["wasm", "frontend"],
    requires: Requires::Server,
    availability: ui_availability,
};
pub static MEMBER: Command = Command {
    id: "core.component.member",
    path: &["core", "component", "member"],
    contract: 1,
    summary: "Read one verified bounded chunk from a captured UI unit.",
    purpose: "Read from the Core-owned immutable UI installation named by a captured whole-unit digest. Only the selected component's four fixed filenames are admitted. Reverify the complete member and read its chunk from the same locked handle. A captured unit remains readable after update/removal. The loader collects all members, verifies their receipt hashes and admits the actual exported build identity before using the UI. No arbitrary path, URL, caller receipt, JavaScript evaluation, private database or network access is admitted.",
    chapter: Chapter::Workstation,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        UI_COMPONENT,
        CHANNEL,
        RUNTIME,
        SCOPE,
        Arg::value(
            "unit-sha256",
            "<sha256>",
            "Exact whole-unit digest returned by capture.",
        )
        .required(),
        Arg::value(
            "filename",
            "<fixed-member>",
            "One of the component's four fixed admitted member names.",
        )
        .required(),
        Arg::value(
            "offset",
            "<u64>",
            "Byte offset, bounded by the complete member size.",
        )
        .default("0"),
        Arg::value(
            "limit",
            "<bytes>",
            "Maximum chunk bytes; 1 through 1048576.",
        )
        .default("1048576"),
    ],
    output: "ds.ui-module-member/v1 (ds.engine-unit-member/v1 for the Network page unit): unit_sha256, filename, offset, byte_count, total_bytes, sha256, body_base64 and more. At most 1 MiB decoded bytes; more explicitly reports a remaining suffix.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/core.md"),
    search: &["wasm", "loader", "bytes"],
    requires: Requires::Server,
    availability: ui_availability,
};
pub fn capture(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    if crate::engine_units::requested(inputs)? {
        return crate::engine_units::capture(inputs);
    }
    let (component, channel) = selected(inputs)?;
    value(
        UiModuleStore::retained_managed_scoped(component, channel, scope(inputs)?)
            .map_err(owner_failure)?
            .capture(&|| false)
            .map_err(owner_failure)?,
    )
}
pub fn member(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    if crate::engine_units::requested(inputs)? {
        return crate::engine_units::member(inputs);
    }
    let (component, channel) = selected(inputs)?;
    let invalid = || {
        owner_failure(
            "module_artifact_invalid: name a fixed member and unsigned bounded range".into(),
        )
    };
    let offset_raw = inputs.require("offset")?;
    if offset_raw.is_empty() || !offset_raw.bytes().all(|b| b.is_ascii_digit()) {
        return Err(invalid());
    }
    let offset = offset_raw.parse::<u64>().map_err(|_| invalid())?;
    let limit_raw = inputs.require("limit")?;
    if limit_raw.is_empty() || !limit_raw.bytes().all(|b| b.is_ascii_digit()) {
        return Err(invalid());
    }
    let limit = limit_raw.parse::<usize>().map_err(|_| invalid())?;
    if limit == 0 || limit > MAX_MEMBER_CHUNK {
        return Err(invalid());
    }
    let digest = inputs.require("unit-sha256")?;
    let filename = inputs.require("filename")?;
    if !component.files().iter().any(|(name, _)| *name == filename)
        || !ds_cli_contract::util::is_sha256_hex(digest, ds_cli_contract::util::HexCase::Lower)
    {
        return Err(invalid());
    }
    let (bytes, identity) =
        UiModuleStore::retained_managed_scoped(component, channel, scope(inputs)?)
            .map_err(owner_failure)?
            .member(digest, filename, offset, limit, &|| false)
            .map_err(owner_failure)?;
    Ok(
        json!({ "schema":"ds.ui-module-member/v1", "unit_sha256":digest, "filename":filename, "offset":offset, "byte_count":bytes.len(), "total_bytes":identity.size_bytes, "sha256":identity.sha256, "body_base64":base64::engine::general_purpose::STANDARD.encode(&bytes), "more":offset + (bytes.len() as u64) < identity.size_bytes as u64 }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_refusals_are_declared_and_unknown_codes_fall_back() {
        let codes = |refusals: &[Refusal]| refusals.iter().map(|r| r.code).collect::<Vec<_>>();
        assert_eq!(codes(INSTALL_LATEST.refusals), codes(INSTALL_REFUSALS));
        for refusal in INSTALL_REFUSALS {
            let failure = install_failure(format!("{}: owner refusal", refusal.code));
            assert_eq!(failure.code(), refusal.code);
        }
        let unknown = install_failure(format!("undeclared_code: {}", "x".repeat(4096)));
        assert_eq!(unknown.code(), "module_stage_failed");
        assert_eq!(unknown.message().chars().count(), 1024);
    }

    #[test]
    fn install_latest_is_confirmed_ui_only_wasm_and_takes_no_digest_url_or_path() {
        assert!(INSTALL_LATEST.requires_confirmation());
        assert_eq!(INSTALL_LATEST.authority, Authority::None);
        assert_eq!(
            INSTALL_LATEST
                .args
                .iter()
                .map(|arg| arg.name)
                .collect::<Vec<_>>(),
            ["component", "channel", "runtime"]
        );
        let context = |confirmed| Context {
            confirmed,
            output: ds_cli_contract::Output::resolve(ds_cli_contract::Format::Json, false, true),
        };
        let parse = |args: &[&str]| {
            ds_cli_contract::parse(
                &INSTALL_LATEST,
                &args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>(),
            )
        };
        let input = parse(&["--component", "command-kernel", "--channel", "stable"]).unwrap();
        assert_eq!(
            install_latest(&input, &context(false)).unwrap_err().code(),
            "confirmation_required"
        );
        // Native engines are not UI units; the closed choice refuses before any effect.
        assert!(parse(&["--component", "network", "--channel", "stable"]).is_err());
        assert!(
            parse(&[
                "--component",
                "interactions",
                "--channel",
                "stable",
                "--runtime",
                "native"
            ])
            .is_err()
        );
    }

    #[test]
    fn local_stage_requires_confirmation_and_release_consumers_cannot_select_local_scope() {
        let input = inputs(
            &STAGE_LOCAL,
            "command-kernel",
            None,
            &[
                "--directory",
                "/abs/producer",
                "--identity-file",
                "/abs/identity.json",
                "--source-sha",
                &"a".repeat(40),
                "--wasm-sha256",
                &"b".repeat(64),
            ],
        );
        assert!(STAGE_LOCAL.requires_confirmation());
        assert_eq!(scope(&input).unwrap(), UiScope::LocalDev);
        let context = Context {
            confirmed: false,
            output: ds_cli_contract::Output::resolve(ds_cli_contract::Format::Json, false, true),
        };
        assert_eq!(
            stage_local(&input, &context).unwrap_err().code(),
            "confirmation_required"
        );
        let native = inputs(
            &crate::core_components::SELECTION,
            "network",
            None,
            &["--scope", "local-dev"],
        );
        assert_eq!(
            requested(&native).unwrap_err().code(),
            "module_scope_invalid"
        );
        let release = inputs(&CAPTURE, "command-kernel", Some("wasm"), &[]);
        assert_eq!(scope(&release).unwrap(), UiScope::Release);
        let mut stable_args = vec![
            "--component",
            "command-kernel",
            "--channel",
            "stable",
            "--runtime",
            "wasm",
            "--scope",
            "local-dev",
        ];
        let stable = ds_cli_contract::parse(
            &CAPTURE,
            &stable_args.drain(..).map(str::to_owned).collect::<Vec<_>>(),
        )
        .unwrap();
        assert_eq!(
            selected(&stable).unwrap_err().code(),
            "module_scope_invalid"
        );
    }

    fn inputs(command: &Command, component: &str, runtime: Option<&str>, extra: &[&str]) -> Inputs {
        let mut args = vec!["--component", component, "--channel", "canary"];
        if let Some(runtime) = runtime {
            args.extend(["--runtime", runtime]);
        }
        args.extend(extra);
        ds_cli_contract::parse(
            command,
            &args.into_iter().map(str::to_owned).collect::<Vec<_>>(),
        )
        .unwrap()
    }
    #[test]
    fn explicit_runtime_fences_ui_and_native_before_any_authority_or_filesystem_effect() {
        for component in ["interactions", "command-kernel"] {
            assert_eq!(
                requested(&inputs(
                    &crate::module_updates::STAGE,
                    component,
                    None,
                    &["--sha256", "bad"]
                ))
                .unwrap_err()
                .code(),
                "module_release_component_invalid"
            );
            let selected = inputs(
                &crate::module_updates::STAGE,
                component,
                Some("wasm"),
                &["--sha256", "bad"],
            );
            assert!(requested(&selected).unwrap());
            assert_eq!(
                stage(&selected).unwrap_err().code(),
                "module_release_invalid"
            );
        }
        for component in ["solar", "reporter", "network"] {
            assert!(
                !requested(&inputs(
                    &crate::core_components::SELECTION,
                    component,
                    None,
                    &[]
                ))
                .unwrap()
            );
            assert_eq!(
                requested(&inputs(
                    &crate::core_components::SELECTION,
                    component,
                    Some("wasm"),
                    &[]
                ))
                .unwrap_err()
                .code(),
                "module_release_component_invalid"
            );
        }
    }
    #[test]
    fn loader_commands_are_read_only_bounded_and_cannot_accept_paths_receipts_or_urls() {
        for command in [&CAPTURE, &MEMBER] {
            assert_eq!(command.effect, Effect::ReadOnly);
            assert_eq!(command.authority, Authority::None);
            assert!(!command.requires_confirmation());
            for forbidden in [
                "path",
                "url",
                "file",
                "receipt",
                "authority",
                "dirty",
                "idle",
            ] {
                assert!(!command.args.iter().any(|arg| arg.name == forbidden));
            }
        }
        let context = Context {
            confirmed: false,
            output: ds_cli_contract::Output::resolve(ds_cli_contract::Format::Json, false, true),
        };
        for (name, limit) in [
            ("../selection.json", "10"),
            ("ds_interactions_wasm.js", "1048577"),
            ("ds_interactions_wasm.js", "0"),
        ] {
            let input = inputs(
                &MEMBER,
                "interactions",
                Some("wasm"),
                &[
                    "--unit-sha256",
                    &"a".repeat(64),
                    "--filename",
                    name,
                    "--limit",
                    limit,
                ],
            );
            assert_eq!(
                member(&input, &context).unwrap_err().code(),
                if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
                    "module_artifact_invalid"
                } else {
                    "ui_component_platform_unavailable"
                }
            );
        }
    }
}
