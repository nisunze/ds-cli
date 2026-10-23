//! Core-owned UI installation effects. The UI is an opaque complete byte unit:
//! this owner never evaluates JavaScript, WASM, or an Interactions operation.
use super::*;
use ds_command_kernel::module_release::ui::{
    self as policy, InstalledUiModule, PublishedUiAuthority, UiAuthority, UiCapture, UiComponent,
    UiScope, UiSelection, UiSelectionState, UiUnit, VerifiedFile,
};
use ds_command_kernel::module_release::ui_page::{self as page, UiPageRequirements};

/// The installed Desktop page's recorded requirements, beside the selection.
const PAGE_REQUIREMENTS_FILE: &str = "page-requirements.json";
const MAX_PAGE_REQUIREMENTS_BYTES: u64 = 512 * 1024;

const STAGE_SCHEMA: &str = "ds.ui-module-stage/v1";
const MAX_STORED_BYTES: u64 = 1024 * 1024;
pub const MAX_MEMBER_CHUNK: usize = 1024 * 1024;

/// Native host IO hooks reuse the existing protected Windows state owner.
/// These are process-local implementation plumbing, never an IPC/SDK surface.
#[cfg(windows)]
pub struct WindowsUiStoreOwner {
    pub prepare_directory: fn(&Path) -> Result<(), String>,
    pub validate_directory: fn(&Path) -> Result<(), String>,
    pub validate_file: fn(&Path) -> Result<(), String>,
}
#[cfg(windows)]
static WINDOWS_OWNER: std::sync::OnceLock<&'static WindowsUiStoreOwner> =
    std::sync::OnceLock::new();
#[cfg(windows)]
pub fn register_windows_store_owner(owner: &'static WindowsUiStoreOwner) -> Result<(), String> {
    let held = WINDOWS_OWNER.get_or_init(|| owner);
    if !std::ptr::eq(*held, owner) {
        return Err(
            "module_store_invalid: another Windows UI state owner is already registered".into(),
        );
    }
    Ok(())
}
pub fn ui_platform() -> Result<(), String> {
    if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") {
        return Ok(());
    }
    #[cfg(all(windows, target_arch = "x86_64"))]
    if WINDOWS_OWNER.get().is_some() {
        return Ok(());
    }
    Err("ui_component_platform_unavailable: UI installation requires qualified Linux x86_64 storage or the registered Windows x86_64 SID/DACL owner".into())
}
#[cfg(windows)]
pub(super) fn windows_owner() -> Result<&'static WindowsUiStoreOwner, String> {
    WINDOWS_OWNER.get().copied().ok_or_else(|| {
        "ui_component_platform_unavailable: the Windows SID/DACL owner is not registered".into()
    })
}
#[cfg(windows)]
pub(super) fn audit_windows_tree(root: &Path) -> Result<(), String> {
    let owner = windows_owner()?;
    let mut pending = vec![(root.to_path_buf(), 0usize)];
    let mut count = 0usize;
    while let Some((directory, depth)) = pending.pop() {
        (owner.validate_directory)(&directory)?;
        for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
            let path = entry.map_err(|e| e.to_string())?.path();
            count += 1;
            if count > 10_000 || depth >= 16 {
                return Err(
                    "module_store_invalid: Windows UI ownership inventory exceeds its bound".into(),
                );
            }
            let metadata = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
            if transfer::metadata_is_link_or_reparse(&metadata) {
                return Err("module_store_invalid: linked or reparsed UI state".into());
            }
            if metadata.is_dir() {
                pending.push((path, depth + 1));
            } else if metadata.is_file() {
                (owner.validate_file)(&path)?;
            } else {
                return Err("module_store_invalid: nonordinary UI state".into());
            }
        }
    }
    Ok(())
}

/// Minted only by this native gateway acquisition, never by held catalog JSON.
#[derive(Debug)]
pub struct TrustedUiGrant {
    unit: UiUnit,
    authority: UiAuthority,
}
impl CapturedReleaseAuthority {
    /// A person's confirmed unit: refuses when the channel moved since it was read.
    pub fn acquire_ui(
        &self,
        component: UiComponent,
        expected_sha256: &str,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<TrustedUiGrant, String> {
        self.acquire_ui_with(component, Some(expected_sha256), cancelled, |url| {
            self.read_release(url)
        })
    }
    /// The channel's active release is the target; no person chooses a version.
    /// An earlier unit published as the active release is acquired the same way.
    pub fn acquire_active_ui(
        &self,
        component: UiComponent,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<TrustedUiGrant, String> {
        self.acquire_ui_with(component, None, cancelled, |url| self.read_release(url))
    }
    pub(super) fn read_release(&self, url: &str) -> Result<Vec<u8>, String> {
        let mut response = ureq::get(url)
            .header("Accept", "application/json")
            .header("x-api-key", &self.gateway_key)
            .config()
            .max_redirects(0)
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_global(Some(Duration::from_secs(30)))
            .build()
            .call()
            .map_err(|e| format!("module_release_unavailable: {e}"))?;
        let mut bytes = Vec::new();
        response
            .body_mut()
            .as_reader()
            .take(MAX_RELEASE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("module_release_unavailable: {e}"))?;
        Ok(bytes)
    }
    fn acquire_ui_with(
        &self,
        component: UiComponent,
        expected_sha256: Option<&str>,
        cancelled: &dyn Fn() -> bool,
        read: impl FnOnce(&str) -> Result<Vec<u8>, String>,
    ) -> Result<TrustedUiGrant, String> {
        ui_platform()?;
        check_cancel(cancelled)?;
        if expected_sha256.is_some_and(|digest| !transfer::exact_sha256(digest)) {
            return Err("module_release_invalid: name the exact selected UI unit digest".into());
        }
        let operation = ds_client_core::gateway_operation("domains.releases.desktop_manifest")
            .ok_or("module_authority_unavailable: release operation is undeclared")?;
        if operation.method != "GET"
            || operation.path != "/api/v1/app_releases"
            || operation.authority != "release"
        {
            return Err("module_authority_unavailable: release operation contract changed".into());
        }
        let url = format!(
            "{}{}?component={}&channel={}&runtime=wasm",
            self.gateway_origin,
            operation.path,
            component.as_str(),
            self.channel
        );
        let bytes = read(&url)?;
        check_cancel(cancelled)?;
        if bytes.len() as u64 > MAX_RELEASE_BYTES {
            return Err("module_release_invalid: excessive release response".into());
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Envelope {
            success: bool,
            release: Value,
        }
        let envelope: Envelope = serde_json::from_slice(&bytes)
            .map_err(|_| "module_release_invalid: release authority response is malformed")?;
        if !envelope.success || envelope.release.is_null() {
            return Err("module_release_unpublished: no UI release is published".into());
        }
        // Complete catalog and source/compiler/configuration admission occurs
        // before the grant exists and before any member transfer is possible.
        let unit = policy::inspect(component.as_str(), self.channel, &envelope.release)
            .map_err(|e| e.to_string())?;
        if unit.published().expect("published inspection").status != "active" {
            return Err("module_release_inactive: retain the installed UI unit".into());
        }
        if expected_sha256.is_some_and(|digest| unit.unit_sha256() != digest) {
            return Err(
                "module_release_changed: refresh explicitly before confirming the UI unit".into(),
            );
        }
        policy::acquisition_plan(&unit, self.channel, unit.unit_sha256(), true)
            .map_err(|e| e.to_string())?;
        Ok(TrustedUiGrant {
            unit,
            authority: UiAuthority::Published(PublishedUiAuthority {
                gateway_origin: self.gateway_origin.clone(),
                route_manifest_sha256: self.catalog_sha256.clone(),
            }),
        })
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StagedUiModule {
    schema: String,
    pub state: String,
    pub receipt_id: String,
    pub component: UiComponent,
    pub channel: String,
    pub version: String,
    pub sha256: String,
    pub expected_generation: u64,
}
const CONVERGENCE_SCHEMA: &str = "ds.ui-module-convergence/v1";
/// What one automatic pass did for one UI component (philosophy §8).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UiConvergenceOutcome {
    /// Never installed here: a machine without the Desktop needs no UI unit.
    Absent,
    /// Explicitly removed: retained until the unit is installed again.
    Removed,
    /// The active unit already is the channel's active release.
    Current,
    /// The channel's active release was verified and activated.
    Updated,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiConvergence {
    pub schema: &'static str,
    pub component: UiComponent,
    pub channel: String,
    pub outcome: UiConvergenceOutcome,
    pub generation: u64,
    /// The channel's active release; absent when no catalog read was needed.
    pub release_version: Option<String>,
    pub unit_sha256: Option<String>,
}
/// Only an installed unit follows its channel. The active release decides the
/// target, including an earlier unit published again; no person picks one.
pub fn convergence_plan(
    selection: Option<&UiSelection>,
    active_release_sha256: &str,
) -> UiConvergenceOutcome {
    match selection.map(|selected| &selected.selection) {
        None => UiConvergenceOutcome::Absent,
        Some(UiSelectionState::Removed { .. }) => UiConvergenceOutcome::Removed,
        Some(UiSelectionState::Installed { active, .. })
            if active.unit.unit_sha256() == active_release_sha256 =>
        {
            UiConvergenceOutcome::Current
        }
        Some(UiSelectionState::Installed { .. }) => UiConvergenceOutcome::Updated,
    }
}
/// Offline answer for a unit that does not follow its channel, so the caller
/// reads no catalog for it; `None` means the installed unit follows its channel.
pub fn retained_convergence(
    component: UiComponent,
    channel: &str,
    selection: Option<&UiSelection>,
) -> Option<UiConvergence> {
    let outcome = match selection.map(|selected| &selected.selection) {
        None => UiConvergenceOutcome::Absent,
        Some(UiSelectionState::Removed { .. }) => UiConvergenceOutcome::Removed,
        Some(UiSelectionState::Installed { .. }) => return None,
    };
    Some(UiConvergence {
        schema: CONVERGENCE_SCHEMA,
        component,
        channel: channel.into(),
        outcome,
        generation: selection.map_or(0, |selected| selected.generation),
        release_version: None,
        unit_sha256: None,
    })
}
// Stored receipt is intentionally distinct from its public ready projection.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredUiStage {
    schema: String,
    receipt_id: String,
    expected_generation: u64,
    installed: InstalledUiModule,
}

fn admit_scope(channel: &str, scope: UiScope) -> Result<(), String> {
    if scope == UiScope::LocalDev
        && (channel != "canary" || !cfg!(all(target_os = "linux", target_arch = "x86_64")))
    {
        return Err(
            "module_scope_invalid: local UI builds require Linux x86_64 canary local-dev scope"
                .into(),
        );
    }
    Ok(())
}
fn scoped_base(base: &Path, scope: UiScope, create: bool) -> Result<PathBuf, String> {
    if scope == UiScope::Release {
        return Ok(base.to_path_buf());
    }
    if create {
        transfer::ensure_owned_directory(base, Path::new("local-dev"))
    } else {
        Ok(base.join("local-dev"))
    }
}

pub struct UiModuleStore {
    root: PathBuf,
    component: UiComponent,
    channel: String,
    scope: UiScope,
}
impl UiModuleStore {
    pub fn managed(component: UiComponent, channel: &str) -> Result<Self, String> {
        Self::managed_scoped(component, channel, UiScope::Release)
    }
    pub fn managed_scoped(
        component: UiComponent,
        channel: &str,
        scope: UiScope,
    ) -> Result<Self, String> {
        admit_scope(channel, scope)?;
        ui_platform()?;
        let base = ds_cli_exec::addons::managed_module_base().map_err(|e| e.to_string())?;
        fs::create_dir_all(&base).map_err(|e| e.to_string())?;
        ordinary_directory(&base)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            if fs::symlink_metadata(&base)
                .map_err(|e| e.to_string())?
                .uid()
                != unsafe { libc::geteuid() }
            {
                return Err("module_store_invalid: UI store belongs to another OS account".into());
            }
            fs::set_permissions(&base, fs::Permissions::from_mode(0o700))
                .map_err(|e| e.to_string())?;
        }
        Self::new_scoped(&base, component, channel, scope)
    }
    /// Open retained bytes without creating any installation state.
    pub fn retained_managed(component: UiComponent, channel: &str) -> Result<Self, String> {
        Self::retained_managed_scoped(component, channel, UiScope::Release)
    }
    pub fn retained_managed_scoped(
        component: UiComponent,
        channel: &str,
        scope: UiScope,
    ) -> Result<Self, String> {
        admit_scope(channel, scope)?;
        ui_platform()?;
        if !matches!(channel, "stable" | "canary") {
            return Err("module_store_invalid: invalid UI lane".into());
        }
        let base = ds_cli_exec::addons::managed_module_base().map_err(|e| e.to_string())?;
        let base = scoped_base(&base, scope, false)?;
        let root = base.join(component.as_str()).join(channel);
        if !exists_plain(&root)? {
            return Err("module_not_installed: no retained UI installation".into());
        }
        let store = Self {
            root,
            component,
            channel: channel.into(),
            scope,
        };
        store.check_root()?;
        Ok(store)
    }
    pub fn new(base: &Path, component: UiComponent, channel: &str) -> Result<Self, String> {
        Self::new_scoped(base, component, channel, UiScope::Release)
    }
    pub fn new_scoped(
        base: &Path,
        component: UiComponent,
        channel: &str,
        scope: UiScope,
    ) -> Result<Self, String> {
        admit_scope(channel, scope)?;
        ui_platform()?;
        if !base.is_absolute() || !matches!(channel, "stable" | "canary") {
            return Err(
                "module_store_invalid: use the exact absolute native store and lane".into(),
            );
        }
        let scoped = scoped_base(base, scope, true)?;
        let base = scoped.as_path();
        #[cfg(windows)]
        {
            let owner = windows_owner()?;
            (owner.prepare_directory)(&base.join(component.as_str()))?;
            (owner.prepare_directory)(&base.join(component.as_str()).join(channel))?;
        }
        let root = transfer::ensure_owned_directory(
            base,
            &PathBuf::from(component.as_str()).join(channel),
        )?;
        for name in ["versions", "receipts"] {
            #[cfg(windows)]
            (windows_owner()?.prepare_directory)(&root.join(name))?;
            transfer::ensure_owned_directory(&root, Path::new(name))?;
        }
        let store = Self {
            root,
            component,
            channel: channel.into(),
            scope,
        };
        store.check_root()?;
        Ok(store)
    }
    /// Offline retained read does not create a store or acquire a catalog.
    pub fn selection_managed(
        component: UiComponent,
        channel: &str,
    ) -> Result<Option<UiSelection>, String> {
        let base = ds_cli_exec::addons::managed_module_base().map_err(|e| e.to_string())?;
        Self::selection_from(&base, component, channel)
    }
    pub fn selection_managed_scoped(
        component: UiComponent,
        channel: &str,
        scope: UiScope,
    ) -> Result<Option<UiSelection>, String> {
        let base = ds_cli_exec::addons::managed_module_base().map_err(|e| e.to_string())?;
        Self::selection_from_scoped(&base, component, channel, scope)
    }
    fn selection_from(
        base: &Path,
        component: UiComponent,
        channel: &str,
    ) -> Result<Option<UiSelection>, String> {
        Self::selection_from_scoped(base, component, channel, UiScope::Release)
    }
    fn selection_from_scoped(
        base: &Path,
        component: UiComponent,
        channel: &str,
        scope: UiScope,
    ) -> Result<Option<UiSelection>, String> {
        admit_scope(channel, scope)?;
        let scoped = scoped_base(base, scope, false)?;
        let base = scoped.as_path();
        ui_platform()?;
        if !base.is_absolute() || !matches!(channel, "stable" | "canary") {
            return Err("module_store_invalid: invalid UI store or lane".into());
        }
        let root = base.join(component.as_str()).join(channel);
        for directory in [base, root.parent().unwrap(), root.as_path()] {
            if !exists_plain(directory)? {
                return Ok(None);
            }
            ordinary_directory(directory)?;
        }
        Self {
            root,
            component,
            channel: channel.into(),
            scope,
        }
        .selection()
    }
    fn check_root(&self) -> Result<(), String> {
        #[cfg(windows)]
        {
            (windows_owner()?.validate_directory)(
                self.root
                    .parent()
                    .ok_or("module_store_invalid: missing UI parent")?,
            )?;
            audit_windows_tree(&self.root)?;
        }
        for path in self.root.ancestors().take(3) {
            ordinary_directory(path)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::{MetadataExt, PermissionsExt};
                let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
                if metadata.uid() != unsafe { libc::geteuid() }
                    || metadata.permissions().mode() & 0o077 != 0
                {
                    return Err(
                        "module_store_invalid: UI directories must be private to this OS account"
                            .into(),
                    );
                }
            }
        }
        Ok(())
    }
    fn lock(&self) -> Result<ModuleLock, String> {
        self.check_root()?;
        let file = transfer::open_download_partial(&self.root.join("update.lock"))?;
        file.try_lock_exclusive()
            .map_err(|_| "module_update_busy: another update owns this UI component".to_owned())?;
        Ok(ModuleLock(file))
    }
    fn lock_retained(&self) -> Result<ModuleLock, String> {
        self.check_root()?;
        let path = self.root.join("update.lock");
        ordinary_file(&path)?;
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        let file = options
            .open(path)
            .map_err(|e| format!("module_store_invalid: {e}"))?;
        if !file
            .metadata()
            .is_ok_and(|m| m.is_file() && !transfer::metadata_is_link_or_reparse(&m))
        {
            return Err("module_store_invalid: invalid retained UI lock".into());
        }
        file.try_lock_exclusive()
            .map_err(|_| "module_update_busy: another update owns this UI component".to_owned())?;
        Ok(ModuleLock(file))
    }
    pub fn selection(&self) -> Result<Option<UiSelection>, String> {
        self.check_root()?;
        let path = self.root.join("selection.json");
        if !exists_plain(&path)? {
            return Ok(None);
        }
        let selected: UiSelection =
            serde_json::from_slice(&read_plain(&path, MAX_STORED_BYTES)?)
                .map_err(|_| "module_receipt_invalid: malformed UI selection")?;
        policy::validate_selection(&selected, self.component, &self.channel)
            .map_err(|e| e.to_string())?;
        match &selected.selection {
            UiSelectionState::Installed { active, previous } => {
                self.admit_receipt(active)?;
                if let Some(previous) = previous {
                    self.admit_receipt(previous)?;
                }
            }
            UiSelectionState::Removed { previous } => {
                if let Some(previous) = previous {
                    self.admit_receipt(previous)?;
                }
            }
        }
        Ok(Some(selected))
    }
    fn unit_directory(&self, digest: &str) -> Result<PathBuf, String> {
        self.check_root()?;
        if !transfer::exact_sha256(digest) {
            return Err("module_receipt_invalid: invalid UI unit digest".into());
        }
        ordinary_directory(&self.root.join("versions"))?;
        let directory = self.root.join("versions").join(digest);
        ordinary_directory(&directory)?;
        Ok(directory)
    }
    fn admit_receipt(&self, receipt: &InstalledUiModule) -> Result<(), String> {
        if receipt.unit.component() != self.component || receipt.unit.scope() != self.scope {
            return Err("module_receipt_invalid: crossed UI component".into());
        }
        policy::admit_retained(receipt, &self.channel, receipt.authority.gateway_origin())
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    fn verify_at(
        &self,
        receipt: &InstalledUiModule,
        directory: &Path,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<VerifiedFile>, String> {
        self.admit_receipt(receipt)?;
        ordinary_directory(directory)?;
        let mut verified = Vec::new();
        for file in receipt.unit.files() {
            transfer::reopen_verified_download(
                &directory.join(&file.filename),
                file.size_bytes as u64,
                &file.sha256,
                || check_cancel(cancelled),
            )
            .map_err(|e| format!("module_artifact_invalid: {e}"))?;
            verified.push(file);
        }
        policy::verify_files(&receipt.unit, &verified).map_err(|e| e.to_string())?;
        Ok(verified)
    }
    fn verify(
        &self,
        receipt: &InstalledUiModule,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), String> {
        self.verify_at(
            receipt,
            &self.unit_directory(receipt.unit.unit_sha256())?,
            cancelled,
        )
        .map(|_| ())
    }
    pub fn stage<F: FnMut(TransferProgress)>(
        &self,
        grant: TrustedUiGrant,
        report: F,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<StagedUiModule, String> {
        self.stage_with_admission(grant, report, cancelled, || Ok(()))
    }
    pub fn stage_with_admission<
        F: FnMut(TransferProgress),
        A: Fn() -> Result<(), String> + Copy,
    >(
        &self,
        grant: TrustedUiGrant,
        mut report: F,
        cancelled: &dyn Fn() -> bool,
        admit_transport: A,
    ) -> Result<StagedUiModule, String> {
        self.stage_with(grant, cancelled, |asset, path| {
            admit_transport()?;
            transfer::download_verified(
                &asset.url,
                path,
                &asset.sha256,
                asset.size_bytes as u64,
                self.component.as_str(),
                &mut report,
                TransferAdmission {
                    check_cancelled: || check_cancel(cancelled),
                    admit_transport,
                },
            )
            .map(|_| ())
        })
    }
    fn stage_with(
        &self,
        grant: TrustedUiGrant,
        cancelled: &dyn Fn() -> bool,
        mut download: impl FnMut(
            &ds_command_kernel::module_release::ModuleReleaseAsset,
            &Path,
        ) -> Result<(), String>,
    ) -> Result<StagedUiModule, String> {
        if self.scope != UiScope::Release {
            return Err(
                "module_scope_invalid: published acquisition cannot stage into local-dev".into(),
            );
        }
        let assets =
            policy::acquisition_plan(&grant.unit, &self.channel, grant.unit.unit_sha256(), true)
                .map_err(|e| e.to_string())?;
        let provisional = InstalledUiModule {
            schema: policy::INSTALLATION_SCHEMA.into(),
            engine_identity: grant.unit.identity().clone(),
            unit: grant.unit,
            authority: grant.authority,
        };
        self.retain_unit(provisional, cancelled, |directory| {
            for asset in &assets {
                check_cancel(cancelled)?;
                download(
                    asset,
                    &directory.join(asset.url.rsplit('/').next().unwrap()),
                )?;
            }
            Ok(())
        })
    }

    /// Explicit OS-user local build import. Inputs stay opaque: the loader
    /// later re-admits the actual exported identity before invoking any UI ABI.
    pub fn stage_local(
        &self,
        directory: &Path,
        identity_file: &Path,
        source_sha: &str,
        wasm_sha256: &str,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<StagedUiModule, String> {
        if self.scope != UiScope::LocalDev || self.channel != "canary" {
            return Err(
                "module_scope_invalid: local builds require the canary local-dev store".into(),
            );
        }
        if !directory.is_absolute()
            || !identity_file.is_absolute()
            || !transfer::exact_sha256(wasm_sha256)
        {
            return Err(
                "module_artifact_invalid: use absolute local producer inputs and exact WASM digest"
                    .into(),
            );
        }
        ordinary_directory(directory)?;
        let identity: Value = serde_json::from_slice(&read_plain(identity_file, 32 * 1024)?)
            .map_err(|_| "module_engine_incompatible: malformed local producer identity")?;
        let mut bytes = Vec::new();
        let mut verified = Vec::new();
        let mut total = 0usize;
        for (filename, _) in self.component.files() {
            check_cancel(cancelled)?;
            let value = read_plain(&directory.join(filename), 128 * 1024 * 1024)?;
            total = total
                .checked_add(value.len())
                .ok_or("module_artifact_invalid: local UI size overflow")?;
            if total > 128 * 1024 * 1024 {
                return Err("module_artifact_invalid: local UI unit exceeds 128 MiB".into());
            }
            let sha256 = format!("{:x}", Sha256::digest(&value));
            if filename.ends_with("_bg.wasm") && sha256 != wasm_sha256 {
                return Err(
                    "module_artifact_invalid: local WASM differs from the selected tested artifact"
                        .into(),
                );
            }
            verified.push(VerifiedFile {
                filename: filename.into(),
                sha256,
                size_bytes: value.len() as i64,
            });
            bytes.push((filename, value));
        }
        let unit = policy::local_unit(
            self.component,
            &self.channel,
            source_sha,
            &identity,
            &verified,
        )
        .map_err(|e| e.to_string())?;
        let provisional =
            policy::prepare_local(unit, source_sha, &verified, true).map_err(|e| e.to_string())?;
        self.retain_unit(provisional, cancelled, |destination| {
            for (filename, value) in &bytes {
                write_new_bounded(&destination.join(filename), value, 128 * 1024 * 1024)?;
            }
            Ok(())
        })
    }

    fn retain_unit(
        &self,
        provisional: InstalledUiModule,
        cancelled: &dyn Fn() -> bool,
        mut populate: impl FnMut(&Path) -> Result<(), String>,
    ) -> Result<StagedUiModule, String> {
        check_cancel(cancelled)?;
        self.admit_receipt(&provisional)?;
        let _lock = self.lock()?;
        let generation = self
            .selection()?
            .map_or(0, |selection| selection.generation);
        let directory = self
            .root
            .join("versions")
            .join(provisional.unit.unit_sha256());
        let finish = |provisional: InstalledUiModule, verified: &[VerifiedFile]| {
            if provisional.unit.scope() == UiScope::LocalDev {
                let source = provisional.engine_identity.source_sha.clone();
                policy::prepare_local(provisional.unit, &source, verified, true)
            } else {
                policy::prepare(provisional.unit, provisional.authority, verified, true)
            }
            .map_err(|e| e.to_string())
        };
        let installed;
        if exists_plain(&directory)? {
            let verified = self.verify_at(&provisional, &directory, cancelled)?;
            installed = finish(provisional, &verified)?;
            let retained: InstalledUiModule = serde_json::from_slice(&read_plain(
                &directory.join("installation.json"),
                MAX_STORED_BYTES,
            )?)
            .map_err(|_| "module_receipt_invalid: malformed retained UI installation")?;
            self.admit_receipt(&retained)?;
            if retained.unit != installed.unit
                || retained.engine_identity != installed.engine_identity
            {
                return Err("module_receipt_invalid: retained UI installation unit changed".into());
            }
        } else {
            let temporary = PrivateDirectory(transfer::create_fresh_private_directory(
                &self.root, "stage",
            )?);
            populate(&temporary.0)?;
            let verified = self.verify_at(&provisional, &temporary.0, cancelled)?;
            installed = finish(provisional, &verified)?;
            write_new_bounded(
                &temporary.0.join("installation.json"),
                &serde_json::to_vec(&installed).map_err(|e| e.to_string())?,
                MAX_STORED_BYTES,
            )?;
            check_cancel(cancelled)?;
            sync_directory(&temporary.0)?;
            fs::rename(&temporary.0, &directory)
                .map_err(|e| format!("module_stage_failed: {e}"))?;
            sync_directory(&self.root.join("versions"))?;
        }
        let receipt_id = uuid::Uuid::new_v4().simple().to_string();
        let stored = StoredUiStage {
            schema: STAGE_SCHEMA.into(),
            receipt_id,
            expected_generation: generation,
            installed,
        };
        write_new_bounded(
            &self.receipt_path(&stored.receipt_id)?,
            &serde_json::to_vec(&stored).map_err(|e| e.to_string())?,
            MAX_STORED_BYTES,
        )?;
        Ok(staged(stored))
    }
    /// Keep an installed unit on the channel's active release; absence and
    /// removal are retained. Activation changes later captures only, so a
    /// loaded Desktop keeps its unit until it loads again.
    pub fn converge<F: FnMut(TransferProgress)>(
        &self,
        grant: TrustedUiGrant,
        guard: ActivationGuard,
        report: F,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<UiConvergence, String> {
        self.converge_with(grant, guard, cancelled, |grant| {
            self.stage(grant, report, cancelled)
        })
    }
    fn converge_with(
        &self,
        grant: TrustedUiGrant,
        guard: ActivationGuard,
        cancelled: &dyn Fn() -> bool,
        stage: impl FnOnce(TrustedUiGrant) -> Result<StagedUiModule, String>,
    ) -> Result<UiConvergence, String> {
        check_cancel(cancelled)?;
        if grant.unit.component() != self.component || grant.unit.channel() != self.channel {
            return Err("module_receipt_invalid: crossed UI grant".into());
        }
        let current = self.selection()?;
        let planned = current.as_ref().map_or(0, |selected| selected.generation);
        let version = grant.unit.version();
        let digest = grant.unit.unit_sha256().to_owned();
        let outcome = convergence_plan(current.as_ref(), &digest);
        let generation = if outcome == UiConvergenceOutcome::Updated {
            let staged = stage(grant)?;
            // The installed page decides, not a version: a unit lacking an
            // export it calls keeps the installed unit (owner 22 Oct).
            self.admit_for_installed_page(current.as_ref(), &staged.sha256, cancelled)?;
            // The planned generation fences a removal or person's change made
            // after this plan: the update then refuses instead of reinstalling.
            self.activate(&staged.receipt_id, planned, guard, cancelled)?
                .generation
        } else {
            planned
        };
        Ok(UiConvergence {
            schema: CONVERGENCE_SCHEMA,
            component: self.component,
            channel: self.channel.clone(),
            outcome,
            generation,
            release_version: Some(version),
            unit_sha256: Some(digest),
        })
    }
    /// Record what the installed Desktop page calls of this component (the
    /// page build's `ds.ui-page-requirements` document), replacing an earlier
    /// record. Later convergence admits only units that provide it.
    pub fn record_page_requirements(
        &self,
        requirements: &serde_json::Value,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<UiPageRequirements, String> {
        let admitted = UiPageRequirements::admit(requirements).map_err(|e| e.to_string())?;
        let bytes = serde_json::to_vec(&admitted).map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_PAGE_REQUIREMENTS_BYTES {
            return Err(
                "ui_page_requirements_invalid: page requirements exceed their bound".into(),
            );
        }
        let _lock = self.lock()?;
        let path = self.root.join(PAGE_REQUIREMENTS_FILE);
        if exists_plain(&path)? {
            ordinary_file(&path)?;
        }
        let mut staged = tempfile::NamedTempFile::new_in(&self.root).map_err(|e| e.to_string())?;
        staged
            .write_all(&bytes)
            .and_then(|_| staged.as_file().sync_all())
            .map_err(|e| e.to_string())?;
        check_cancel(cancelled)?;
        staged
            .persist(&path)
            .map_err(|e| format!("ui_page_requirements_invalid: {}", e.error))?;
        sync_directory(&self.root)?;
        Ok(admitted)
    }
    /// The installed page's recorded requirements; `None` when no page has
    /// recorded any (a page installed before pages recorded them).
    pub fn page_requirements(&self) -> Result<Option<UiPageRequirements>, String> {
        let path = self.root.join(PAGE_REQUIREMENTS_FILE);
        if !exists_plain(&path)? {
            return Ok(None);
        }
        let value: serde_json::Value =
            serde_json::from_slice(&read_plain(&path, MAX_PAGE_REQUIREMENTS_BYTES)?).map_err(
                |_| "ui_page_requirements_invalid: malformed recorded page requirements",
            )?;
        UiPageRequirements::admit(&value)
            .map(Some)
            .map_err(|e| e.to_string())
    }
    /// Admit a staged unit only when it provides every export the installed
    /// page calls: its recorded requirements, or for a page that recorded none
    /// the exports of the unit it loads today. Nothing installed, nothing to keep.
    fn admit_for_installed_page(
        &self,
        current: Option<&UiSelection>,
        candidate_sha256: &str,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), String> {
        let declared = self.page_requirements()?;
        let installed = match current.map(|selected| &selected.selection) {
            Some(UiSelectionState::Installed { active, .. }) if declared.is_none() => {
                Some(self.unit_exports(active.unit.unit_sha256(), cancelled)?)
            }
            _ => None,
        };
        let Some(required) =
            page::installed_page_requirement(declared.as_ref(), self.component, installed.as_ref())
        else {
            return Ok(());
        };
        let provided = self.unit_exports(candidate_sha256, cancelled)?;
        page::admit_for_installed_page(self.component, &required, &provided)
            .map_err(|e| e.to_string())
    }
    /// The runtime exports a retained unit's verified JavaScript member declares.
    pub fn unit_exports(
        &self,
        unit_sha256: &str,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<std::collections::BTreeSet<String>, String> {
        let name = self.component.files()[0].0;
        let mut bytes = Vec::new();
        loop {
            let (chunk, file) = self.member(
                unit_sha256,
                name,
                bytes.len() as u64,
                MAX_MEMBER_CHUNK,
                cancelled,
            )?;
            if file.size_bytes as usize > page::MAX_GLUE_BYTES {
                return Err(
                    "module_artifact_invalid: the UI JavaScript member exceeds its export bound"
                        .into(),
                );
            }
            if chunk.is_empty() {
                break;
            }
            bytes.extend_from_slice(&chunk);
            if bytes.len() as u64 >= file.size_bytes as u64 {
                break;
            }
        }
        page::unit_exports(&bytes).map_err(|e| e.to_string())
    }
    fn receipt_path(&self, id: &str) -> Result<PathBuf, String> {
        if id.len() != 32
            || !id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("module_receipt_invalid: invalid UI staging receipt".into());
        }
        ordinary_directory(&self.root.join("receipts"))?;
        Ok(self.root.join("receipts").join(format!("{id}.json")))
    }
    fn recover_stage(&self, id: &str) -> Result<StoredUiStage, String> {
        let stored: StoredUiStage =
            serde_json::from_slice(&read_plain(&self.receipt_path(id)?, MAX_STORED_BYTES)?)
                .map_err(|_| "module_receipt_invalid: malformed UI stage")?;
        if stored.schema != STAGE_SCHEMA || stored.receipt_id != id {
            return Err("module_receipt_invalid: crossed UI stage".into());
        }
        self.admit_receipt(&stored.installed)?;
        Ok(stored)
    }
    pub fn activate(
        &self,
        receipt_id: &str,
        generation: u64,
        guard: ActivationGuard,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<UiSelection, String> {
        guard.admit()?;
        check_cancel(cancelled)?;
        let _lock = self.lock()?;
        let staged = self.recover_stage(receipt_id)?;
        if staged.expected_generation != generation {
            return Err("module_selection_changed: UI stage captured another generation".into());
        }
        let current = self.selection()?;
        self.verify(&staged.installed, cancelled)?;
        if let Some(UiSelection {
            selection: UiSelectionState::Installed { active, .. },
            ..
        }) = &current
        {
            self.verify(active, cancelled)?;
        }
        let selected =
            policy::activate(current.as_ref(), staged.installed, generation, true, guard)
                .map_err(|e| e.to_string())?;
        self.write_selection(&selected, cancelled)?;
        Ok(selected)
    }
    pub fn remove(
        &self,
        generation: u64,
        guard: ActivationGuard,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<UiSelection, String> {
        guard.admit()?;
        check_cancel(cancelled)?;
        let _lock = self.lock()?;
        let current = self
            .selection()?
            .ok_or("module_not_installed: no UI unit is installed")?;
        let selected = policy::remove(&current, generation, true).map_err(|e| e.to_string())?;
        self.write_selection(&selected, cancelled)?;
        Ok(selected)
    }
    pub fn rollback(
        &self,
        generation: u64,
        guard: ActivationGuard,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<UiSelection, String> {
        guard.admit()?;
        check_cancel(cancelled)?;
        let _lock = self.lock()?;
        let mut current = self
            .selection()?
            .ok_or("module_rollback_unavailable: no retained UI selection")?;
        let previous = match &current.selection {
            UiSelectionState::Installed { previous, .. }
            | UiSelectionState::Removed { previous } => previous.as_ref(),
        }
        .ok_or("module_rollback_unavailable: no previous UI unit")?;
        self.verify(previous, cancelled)?;
        // Recover from damaged active bytes without retaining a broken rollback target.
        if let UiSelectionState::Installed { active, previous } = &current.selection {
            if self.verify(active, cancelled).is_err() {
                current.selection = UiSelectionState::Removed {
                    previous: previous.clone(),
                };
            }
        }
        let selected =
            policy::rollback(&current, generation, true, guard).map_err(|e| e.to_string())?;
        self.write_selection(&selected, cancelled)?;
        Ok(selected)
    }
    fn write_selection(
        &self,
        selected: &UiSelection,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), String> {
        policy::validate_selection(selected, self.component, &self.channel)
            .map_err(|e| e.to_string())?;
        check_cancel(cancelled)?;
        let bytes = serde_json::to_vec(selected).map_err(|e| e.to_string())?;
        persist_selection(&self.root, &bytes, MAX_STORED_BYTES, cancelled)
    }
    pub fn capture(&self, cancelled: &dyn Fn() -> bool) -> Result<UiCapture, String> {
        let _lock = self.lock_retained()?;
        let selected = self
            .selection()?
            .ok_or("module_not_installed: no UI unit is installed")?;
        let UiSelectionState::Installed { active, .. } = &selected.selection else {
            return Err("module_removed: UI component is removed".into());
        };
        self.verify(active, cancelled)?;
        policy::capture(
            Some(&selected),
            self.component,
            &self.channel,
            active.authority.gateway_origin(),
        )
        .map_err(|e| e.to_string())
    }
    /// Unit digest pins the owner's immutable installation receipt, never a
    /// caller-supplied receipt or path. Old captures survive removal/update.
    pub fn member(
        &self,
        unit_sha256: &str,
        filename: &str,
        offset: u64,
        limit: usize,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(Vec<u8>, VerifiedFile), String> {
        if limit == 0
            || limit > MAX_MEMBER_CHUNK
            || !self
                .component
                .files()
                .iter()
                .any(|(name, _)| *name == filename)
        {
            return Err(
                "module_artifact_invalid: select a fixed UI member and bounded range".into(),
            );
        }
        let _lock = self.lock_retained()?;
        let directory = self.unit_directory(unit_sha256)?;
        let receipt: InstalledUiModule = serde_json::from_slice(&read_plain(
            &directory.join("installation.json"),
            MAX_STORED_BYTES,
        )?)
        .map_err(|_| "module_receipt_invalid: malformed retained installation")?;
        if receipt.unit.unit_sha256() != unit_sha256 {
            return Err("module_receipt_invalid: crossed retained UI unit".into());
        }
        self.admit_receipt(&receipt)?;
        let file = receipt
            .unit
            .files()
            .into_iter()
            .find(|file| file.filename == filename)
            .ok_or("module_receipt_invalid: missing fixed UI member")?;
        let mut held = transfer::reopen_verified_download(
            &directory.join(filename),
            file.size_bytes as u64,
            &file.sha256,
            || check_cancel(cancelled),
        )
        .map_err(|e| format!("module_artifact_invalid: {e}"))?;
        check_cancel(cancelled)?;
        let bytes = held.read_range(offset, limit)?;
        check_cancel(cancelled)?;
        Ok((bytes, file))
    }
}
fn staged(stored: StoredUiStage) -> StagedUiModule {
    StagedUiModule {
        schema: stored.schema,
        state: "ready".into(),
        receipt_id: stored.receipt_id,
        component: stored.installed.unit.component(),
        channel: stored.installed.unit.channel().to_owned(),
        version: stored.installed.unit.version(),
        sha256: stored.installed.unit.unit_sha256().to_owned(),
        expected_generation: stored.expected_generation,
    }
}

#[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
#[path = "ui_tests.rs"]
mod tests;
