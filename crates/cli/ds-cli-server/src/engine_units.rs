//! Typed Core CLI projection of the page engine unit: the separately released
//! Network browser WASM unit (`--component network --runtime wasm`). Core
//! installs, verifies and selects its opaque bytes for the Desktop page; the
//! page's Kernel admits the identity the loaded unit exports. A working-tree
//! build enters only the separate Linux canary local-dev scope. This OS-user
//! scope grants no project or engineering authority.
use crate::core_components::owner_failure;
use base64::Engine;
use ds_cli_contract::{Context, Failure, Inputs};
use ds_command_kernel::module_install::ActivationGuard;
use ds_command_kernel::module_release::{ModuleComponent, ui::UiScope};
use ds_update_runtime::modules::engine_unit::{EngineUnitStore, MAX_MEMBER_CHUNK};
use serde_json::{Value, json};

/// Explicit `--runtime wasm` selects the Network page unit; native stays the
/// default and keeps selecting the native Network addon.
pub fn requested(inputs: &Inputs) -> Result<bool, Failure> {
    Ok(inputs.value("runtime") == Some("wasm") && inputs.require("component")? == "network")
}

fn scope(inputs: &Inputs) -> Result<UiScope, Failure> {
    UiScope::parse(inputs.value("scope").unwrap_or("release"))
        .map_err(|e| owner_failure(e.to_string()))
}
fn selected(inputs: &Inputs) -> Result<(ModuleComponent, &str, UiScope), Failure> {
    #[cfg(windows)]
    ds_cli_auth::windows_host::register_ui_store_owner().map_err(owner_failure)?;
    ds_update_runtime::modules::ui::ui_platform().map_err(owner_failure)?;
    let channel = inputs.require("channel")?;
    let component = ds_command_kernel::engine_unit::admit_target(inputs.require("component")?, channel)
        .map_err(|e| owner_failure(e.to_string()))?;
    let scope = scope(inputs)?;
    if scope == UiScope::LocalDev && channel != "canary" {
        return Err(owner_failure(
            "module_scope_invalid: local engine builds cannot enter the stable release lane".into(),
        ));
    }
    Ok((component, channel, scope))
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
// The foreground installer owns no editor session or job; a loaded page keeps
// its captured unit and adopts a later capture when it loads again.
fn guard() -> ActivationGuard {
    ActivationGuard::default()
}

/// Download and verify the person's confirmed published unit; no activation.
pub fn stage(inputs: &Inputs) -> Result<Value, Failure> {
    let (component, channel, scope) = selected(inputs)?;
    if scope != UiScope::Release {
        return Err(owner_failure(
            "module_scope_invalid: published units stage into the release scope".into(),
        ));
    }
    let digest = inputs.require("sha256")?;
    if !ds_cli_contract::util::is_sha256_hex(digest, ds_cli_contract::util::HexCase::Lower) {
        return Err(crate::module_updates::native_failure(
            "module_release_invalid: name the exact lowercase engine unit SHA-256".into(),
        ));
    }
    let authority =
        ds_cli_auth::module_updates::capture_release_authority(ds_cli_auth::Lane::parse(channel)?)?;
    let grant = authority
        .acquire_engine_unit(component, Some(digest), &|| false)
        .map_err(crate::module_updates::native_failure)?;
    let store = EngineUnitStore::managed(component, channel, scope).map_err(owner_failure)?;
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
/// Stage one already built and tested working-tree unit into local-dev.
pub fn stage_local(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
    if !context.confirmed {
        return Err(Failure::invalid(
            "confirmation_required",
            "Confirm local engine unit staging with --yes",
        ));
    }
    let (component, channel, scope) = selected(inputs)?;
    if scope != UiScope::LocalDev {
        return Err(owner_failure(
            "module_scope_invalid: local builds require explicit local-dev scope".into(),
        ));
    }
    value(
        EngineUnitStore::managed(component, channel, scope)
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
pub fn selection(inputs: &Inputs) -> Result<Value, Failure> {
    let (component, channel, scope) = selected(inputs)?;
    value(EngineUnitStore::selection_managed(component, channel, scope).map_err(owner_failure)?)
}
pub fn activate(inputs: &Inputs) -> Result<Value, Failure> {
    let (component, channel, scope) = selected(inputs)?;
    let generation = generation(inputs)?;
    let id = inputs.require("receipt-id")?;
    if id.len() != 32
        || !id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(owner_failure(
            "module_receipt_invalid: name the exact engine unit staging receipt".into(),
        ));
    }
    value(
        EngineUnitStore::managed(component, channel, scope)
            .map_err(owner_failure)?
            .activate(id, generation, guard(), &|| false)
            .map_err(owner_failure)?,
    )
}
pub fn remove(inputs: &Inputs) -> Result<Value, Failure> {
    let (component, channel, scope) = selected(inputs)?;
    let generation = generation(inputs)?;
    value(
        EngineUnitStore::retained(component, channel, scope)
            .map_err(owner_failure)?
            .remove(generation, guard(), &|| false)
            .map_err(owner_failure)?,
    )
}
pub fn rollback(inputs: &Inputs) -> Result<Value, Failure> {
    let (component, channel, scope) = selected(inputs)?;
    let generation = generation(inputs)?;
    value(
        EngineUnitStore::retained(component, channel, scope)
            .map_err(owner_failure)?
            .rollback(generation, guard(), &|| false)
            .map_err(owner_failure)?,
    )
}
pub fn capture(inputs: &Inputs) -> Result<Value, Failure> {
    let (component, channel, scope) = selected(inputs)?;
    value(
        EngineUnitStore::retained(component, channel, scope)
            .map_err(owner_failure)?
            .capture(&|| false)
            .map_err(owner_failure)?,
    )
}
pub fn member(inputs: &Inputs) -> Result<Value, Failure> {
    let (component, channel, scope) = selected(inputs)?;
    let invalid = || {
        owner_failure(
            "module_artifact_invalid: name a fixed member and unsigned bounded range".into(),
        )
    };
    let unsigned = |name: &str| -> Result<u64, Failure> {
        let raw = inputs.require(name)?;
        if raw.is_empty() || !raw.bytes().all(|b| b.is_ascii_digit()) {
            return Err(invalid());
        }
        raw.parse::<u64>().map_err(|_| invalid())
    };
    let offset = unsigned("offset")?;
    let limit = unsigned("limit")? as usize;
    let digest = inputs.require("unit-sha256")?;
    let filename = inputs.require("filename")?;
    let names = ds_command_kernel::engine_unit::member_names(component)
        .map_err(|e| owner_failure(e.to_string()))?;
    if limit == 0
        || limit > MAX_MEMBER_CHUNK
        || !names.contains(&filename)
        || !ds_cli_contract::util::is_sha256_hex(digest, ds_cli_contract::util::HexCase::Lower)
    {
        return Err(invalid());
    }
    let (bytes, file) = EngineUnitStore::retained(component, channel, scope)
        .map_err(owner_failure)?
        .member(digest, filename, offset, limit, &|| false)
        .map_err(owner_failure)?;
    Ok(json!({
        "schema": "ds.engine-unit-member/v1",
        "unit_sha256": digest,
        "filename": filename,
        "offset": offset,
        "byte_count": bytes.len(),
        "total_bytes": file.size_bytes,
        "sha256": file.sha256,
        "body_base64": base64::engine::general_purpose::STANDARD.encode(&bytes),
        "more": offset + (bytes.len() as u64) < file.size_bytes as u64,
    }))
}

#[cfg(test)]
#[path = "engine_units_tests.rs"]
mod tests;
