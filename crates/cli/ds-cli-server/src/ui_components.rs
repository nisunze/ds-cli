//! Typed Core CLI projection of opaque UI installation. This OS-user scope
//! bootstraps before sign-in; it grants no project or engineering authority.
use crate::core_components::{CHANNEL, REFUSALS, RUNTIME, owner_failure};
use base64::Engine;
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Execution, Requires};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_command_kernel::module_install::ActivationGuard;
use ds_command_kernel::module_release::ui::{UiComponent, UiScope};
use ds_update_runtime::modules::ui::{MAX_MEMBER_CHUNK, UiModuleStore};
use serde_json::{Value, json};

const UI_COMPONENT: Arg = Arg::value(
    "component",
    "<interactions|command-kernel>",
    "Exact fixed UI component owner.",
)
.choices(&["interactions", "command-kernel"])
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

pub static STAGE_LOCAL: Command = Command {
    id: "core.component.stage-local",
    path: &["core", "component", "stage-local"],
    contract: 1,
    summary: "Stage one already built local UI unit without publication or activation.",
    purpose: "Explicit Linux canary local-dev operation over four fixed opaque producer files, their actual exported UI identity, exact source snapshot and tested WASM digest. Core admits the identity and hashes, captures the same bytes into its separate private local-dev store and returns a generation-fenced ready receipt. No gateway, publication, build, JavaScript/WASM evaluation, release selection or project access occurs. The Desktop must select local-dev explicitly; this artifact cannot be promoted into a release selection.",
    chapter: Chapter::Workstation,
    effect: Effect::MachineWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        UI_COMPONENT,
        CHANNEL,
        Arg::value("runtime", "<wasm>", "Only the two fixed UI WASM owners.")
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
    search: &["ui", "wasm", "local", "development"],
    requires: Requires::Server,
    availability: ui_availability,
};
pub fn stage_local(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
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
    summary: "Capture one installed complete UI unit for the Desktop loader.",
    purpose: "Read the OS-user owned UI selection offline and verify all four retained fixed members under the shared updater lock. Return its immutable installation receipt, whole-unit digest and generation. Updates or removal affect later captures; this receipt keeps the captured unit. No catalog request, installation, JavaScript/WASM evaluation, login or project access occurs. Explicit runtime wasm is required.",
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
    output: "ds.ui-module-member/v1: unit_sha256, filename, offset, byte_count, total_bytes, sha256, body_base64 and more. At most 1 MiB decoded bytes; more explicitly reports a remaining suffix.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/core.md"),
    search: &["wasm", "loader", "bytes"],
    requires: Requires::Server,
    availability: ui_availability,
};
pub fn capture(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let (component, channel) = selected(inputs)?;
    value(
        UiModuleStore::retained_managed_scoped(component, channel, scope(inputs)?)
            .map_err(owner_failure)?
            .capture(&|| false)
            .map_err(owner_failure)?,
    )
}
pub fn member(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
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
