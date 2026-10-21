//! Core-owned UI installation effects. The UI is an opaque complete byte unit:
//! this owner never evaluates JavaScript, WASM, or an Interactions operation.
use super::*;
use ds_command_kernel::module_release::ui::{
    self as policy, InstalledUiModule, UiAuthority, UiCapture, UiComponent, UiSelection,
    UiSelectionState, UiUnit, VerifiedFile,
};

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
fn windows_owner() -> Result<&'static WindowsUiStoreOwner, String> {
    WINDOWS_OWNER.get().copied().ok_or_else(|| {
        "ui_component_platform_unavailable: the Windows SID/DACL owner is not registered".into()
    })
}
#[cfg(windows)]
fn audit_windows_tree(root: &Path) -> Result<(), String> {
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
        if unit.release.status != "active" {
            return Err("module_release_inactive: retain the installed UI unit".into());
        }
        if expected_sha256.is_some_and(|digest| unit.unit_sha256 != digest) {
            return Err(
                "module_release_changed: refresh explicitly before confirming the UI unit".into(),
            );
        }
        policy::acquisition_plan(&unit, self.channel, &unit.unit_sha256, true)
            .map_err(|e| e.to_string())?;
        Ok(TrustedUiGrant {
            unit,
            authority: UiAuthority {
                gateway_origin: self.gateway_origin.clone(),
                route_manifest_sha256: self.catalog_sha256.clone(),
            },
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
            if active.unit.unit_sha256 == active_release_sha256 =>
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

pub struct UiModuleStore {
    root: PathBuf,
    component: UiComponent,
    channel: String,
}
impl UiModuleStore {
    pub fn managed(component: UiComponent, channel: &str) -> Result<Self, String> {
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
        Self::new(&base, component, channel)
    }
    /// Open retained bytes without creating any installation state.
    pub fn retained_managed(component: UiComponent, channel: &str) -> Result<Self, String> {
        ui_platform()?;
        if !matches!(channel, "stable" | "canary") {
            return Err("module_store_invalid: invalid UI lane".into());
        }
        let base = ds_cli_exec::addons::managed_module_base().map_err(|e| e.to_string())?;
        let root = base.join(component.as_str()).join(channel);
        if !exists_plain(&root)? {
            return Err("module_not_installed: no retained UI installation".into());
        }
        let store = Self {
            root,
            component,
            channel: channel.into(),
        };
        store.check_root()?;
        Ok(store)
    }
    pub fn new(base: &Path, component: UiComponent, channel: &str) -> Result<Self, String> {
        ui_platform()?;
        if !base.is_absolute() || !matches!(channel, "stable" | "canary") {
            return Err(
                "module_store_invalid: use the exact absolute native store and lane".into(),
            );
        }
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
    fn selection_from(
        base: &Path,
        component: UiComponent,
        channel: &str,
    ) -> Result<Option<UiSelection>, String> {
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
        if receipt.unit.release.component != self.component {
            return Err("module_receipt_invalid: crossed UI component".into());
        }
        policy::admit_retained(receipt, &self.channel, &receipt.authority.gateway_origin)
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
        for (filename, _) in self.component.files() {
            let asset = receipt
                .unit
                .release
                .assets
                .iter()
                .find(|asset| asset.url.ends_with(&format!("/{filename}")))
                .ok_or("module_receipt_invalid: missing fixed UI member")?;
            transfer::reopen_verified_download(
                &directory.join(filename),
                asset.size_bytes as u64,
                &asset.sha256,
                || check_cancel(cancelled),
            )
            .map_err(|e| format!("module_artifact_invalid: {e}"))?;
            verified.push(VerifiedFile {
                filename: filename.into(),
                sha256: asset.sha256.clone(),
                size_bytes: asset.size_bytes,
            });
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
            &self.unit_directory(&receipt.unit.unit_sha256)?,
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
        check_cancel(cancelled)?;
        if grant.unit.release.component != self.component
            || grant.unit.release.channel != self.channel
        {
            return Err("module_receipt_invalid: crossed UI grant".into());
        }
        let assets =
            policy::acquisition_plan(&grant.unit, &self.channel, &grant.unit.unit_sha256, true)
                .map_err(|e| e.to_string())?;
        let _lock = self.lock()?;
        let generation = self.selection()?.map_or(0, |s| s.generation);
        let directory = self.root.join("versions").join(&grant.unit.unit_sha256);
        // Preparation copies only trusted catalog identity; no executable probe.
        let provisional = InstalledUiModule {
            schema: policy::INSTALLATION_SCHEMA.into(),
            engine_identity: grant.unit.release.build_manifest.clone(),
            unit: grant.unit,
            authority: grant.authority,
        };
        let installed;
        if exists_plain(&directory)? {
            let verified = self.verify_at(&provisional, &directory, cancelled)?;
            installed = policy::prepare(provisional.unit, provisional.authority, &verified, true)
                .map_err(|e| e.to_string())?;
            let retained: InstalledUiModule = serde_json::from_slice(&read_plain(
                &directory.join("installation.json"),
                MAX_STORED_BYTES,
            )?)
            .map_err(|_| "module_receipt_invalid: malformed retained UI installation")?;
            self.admit_receipt(&retained)?;
            // A later Core profile can grant the same complete public unit.
            // Keep its historical byte receipt immutable; the new ready
            // receipt retains the authority captured for this acquisition.
            if retained.unit != installed.unit
                || retained.engine_identity != installed.engine_identity
            {
                return Err("module_receipt_invalid: retained UI installation unit changed".into());
            }
        } else {
            let temporary = PrivateDirectory(transfer::create_fresh_private_directory(
                &self.root, "stage",
            )?);
            for asset in &assets {
                check_cancel(cancelled)?;
                download(
                    asset,
                    &temporary.0.join(asset.url.rsplit('/').next().unwrap()),
                )?;
            }
            let verified = self.verify_at(&provisional, &temporary.0, cancelled)?;
            installed = policy::prepare(provisional.unit, provisional.authority, &verified, true)
                .map_err(|e| e.to_string())?;
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
            receipt_id: receipt_id.clone(),
            expected_generation: generation,
            installed,
        };
        write_new_bounded(
            &self.receipt_path(&receipt_id)?,
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
        if grant.unit.release.component != self.component
            || grant.unit.release.channel != self.channel
        {
            return Err("module_receipt_invalid: crossed UI grant".into());
        }
        let current = self.selection()?;
        let planned = current.as_ref().map_or(0, |selected| selected.generation);
        let version = grant.unit.release.version.clone();
        let digest = grant.unit.unit_sha256.clone();
        let outcome = convergence_plan(current.as_ref(), &digest);
        let generation = if outcome == UiConvergenceOutcome::Updated {
            let staged = stage(grant)?;
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
            &active.authority.gateway_origin,
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
        if receipt.unit.unit_sha256 != unit_sha256 {
            return Err("module_receipt_invalid: crossed retained UI unit".into());
        }
        self.admit_receipt(&receipt)?;
        let asset = receipt
            .unit
            .release
            .assets
            .iter()
            .find(|a| a.url.ends_with(&format!("/{filename}")))
            .ok_or("module_receipt_invalid: missing fixed UI member")?;
        let mut held = transfer::reopen_verified_download(
            &directory.join(filename),
            asset.size_bytes as u64,
            &asset.sha256,
            || check_cancel(cancelled),
        )
        .map_err(|e| format!("module_artifact_invalid: {e}"))?;
        check_cancel(cancelled)?;
        let bytes = held.read_range(offset, limit)?;
        check_cancel(cancelled)?;
        Ok((
            bytes,
            VerifiedFile {
                filename: filename.into(),
                sha256: asset.sha256.clone(),
                size_bytes: asset.size_bytes,
            },
        ))
    }
}
fn staged(stored: StoredUiStage) -> StagedUiModule {
    StagedUiModule {
        schema: stored.schema,
        state: "ready".into(),
        receipt_id: stored.receipt_id,
        component: stored.installed.unit.release.component,
        channel: stored.installed.unit.release.channel.clone(),
        version: stored.installed.unit.release.version.clone(),
        sha256: stored.installed.unit.unit_sha256.clone(),
        expected_generation: stored.expected_generation,
    }
}

#[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
#[path = "ui_tests.rs"]
mod tests;
