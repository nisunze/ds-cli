//! Core-owned page engine unit installation (the Network and Reporter browser
//! WASM units). Units are opaque complete byte sets: this owner never evaluates
//! JavaScript or WASM. Published units follow their channel; a working-tree
//! build enters only the separate Linux canary local-dev scope. The page's
//! Kernel admits the identity the loaded bytes export (`engine_unit`).
use super::ui::ui_platform;
use super::*;
use ds_command_kernel::engine_unit::{
    self as policy, EngineSelectionState, EngineUnit, EngineUnitCapture, EngineUnitSelection,
    InstalledEngineUnit,
};
use ds_command_kernel::module_release::browser::VerifiedFile;
use ds_command_kernel::module_release::ui::{UiAuthority, UiScope};

const STAGE_SCHEMA: &str = "ds.engine-unit-stage/v1";
const CONVERGENCE_SCHEMA: &str = "ds.engine-unit-convergence/v1";
const MAX_STORED_BYTES: u64 = 1024 * 1024;
const MAX_UNIT_BYTES: usize = 128 * 1024 * 1024;
pub const MAX_MEMBER_CHUNK: usize = 1024 * 1024;
/// Separate from the native addon store, which keys directories by component.
const STORE: &str = "engine-units";

/// Minted only by this native gateway acquisition, never by held catalog JSON.
#[derive(Debug)]
pub struct TrustedEngineGrant {
    unit: EngineUnit,
    authority: UiAuthority,
}
impl CapturedReleaseAuthority {
    /// The channel's active published unit, or the exact unit a person
    /// confirmed (refused when the channel moved since it was read).
    pub fn acquire_engine_unit(
        &self,
        component: ModuleComponent,
        expected_sha256: Option<&str>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<TrustedEngineGrant, String> {
        self.acquire_engine_unit_with(component, expected_sha256, cancelled, |url| {
            self.read_release(url)
        })
    }
    fn acquire_engine_unit_with(
        &self,
        component: ModuleComponent,
        expected_sha256: Option<&str>,
        cancelled: &dyn Fn() -> bool,
        read: impl FnOnce(&str) -> Result<Vec<u8>, String>,
    ) -> Result<TrustedEngineGrant, String> {
        ui_platform()?;
        policy::admit_target(component.as_str(), self.channel).map_err(|e| e.to_string())?;
        check_cancel(cancelled)?;
        if expected_sha256.is_some_and(|digest| !transfer::exact_sha256(digest)) {
            return Err("module_release_invalid: name the exact engine unit digest".into());
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
        struct Envelope {
            success: bool,
            release: Value,
        }
        let envelope: Envelope = serde_json::from_slice(&bytes)
            .map_err(|_| "module_release_invalid: release authority response is malformed")?;
        if !envelope.success || envelope.release.is_null() {
            return Err("module_release_unpublished: no engine unit is published".into());
        }
        let unit = policy::inspect(component, self.channel, &envelope.release)
            .map_err(|e| e.to_string())?;
        let EngineUnit::Published(published) = &unit else {
            unreachable!("catalog inspection yields a published unit")
        };
        if published.release.status != "active" {
            return Err("module_release_inactive: retain the installed engine unit".into());
        }
        if expected_sha256.is_some_and(|digest| unit.unit_sha256() != digest) {
            return Err(
                "module_release_changed: refresh explicitly before confirming the engine unit"
                    .into(),
            );
        }
        policy::acquisition_plan(&unit, self.channel, unit.unit_sha256(), true)
            .map_err(|e| e.to_string())?;
        Ok(TrustedEngineGrant {
            authority: policy::published_authority(&self.gateway_origin, &self.catalog_sha256),
            unit,
        })
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StagedEngineUnit {
    schema: String,
    pub state: String,
    pub receipt_id: String,
    pub component: ModuleComponent,
    pub channel: String,
    pub scope: UiScope,
    pub version: String,
    pub sha256: String,
    pub expected_generation: u64,
}
/// What one automatic pass did for one engine unit (philosophy §8).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineConvergenceOutcome {
    Absent,
    Removed,
    Current,
    Updated,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineConvergence {
    pub schema: &'static str,
    pub component: ModuleComponent,
    pub channel: String,
    pub outcome: EngineConvergenceOutcome,
    pub generation: u64,
    pub release_version: Option<String>,
    pub unit_sha256: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredEngineStage {
    schema: String,
    receipt_id: String,
    expected_generation: u64,
    installed: InstalledEngineUnit,
}

fn admit_scope(channel: &str, scope: UiScope) -> Result<(), String> {
    if !matches!(channel, "stable" | "canary") {
        return Err("module_store_invalid: invalid engine unit lane".into());
    }
    if scope == UiScope::LocalDev
        && (channel != "canary" || !cfg!(all(target_os = "linux", target_arch = "x86_64")))
    {
        return Err("module_scope_invalid: local engine builds require Linux x86_64 canary local-dev scope".into());
    }
    Ok(())
}
/// `<base>/engine-units` or `<base>/local-dev/engine-units`.
fn store_base(base: &Path, scope: UiScope, create: bool) -> Result<PathBuf, String> {
    let relative = match scope {
        UiScope::Release => PathBuf::from(STORE),
        UiScope::LocalDev => Path::new("local-dev").join(STORE),
    };
    if !create {
        return Ok(base.join(relative));
    }
    let mut current = base.to_path_buf();
    for part in relative.components() {
        #[cfg(windows)]
        (super::ui::windows_owner()?.prepare_directory)(&current.join(part))?;
        current = transfer::ensure_owned_directory(&current, Path::new(part.as_os_str()))?;
    }
    Ok(current)
}
fn private_base() -> Result<PathBuf, String> {
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
            return Err("module_store_invalid: engine store belongs to another OS account".into());
        }
        fs::set_permissions(&base, fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
    }
    Ok(base)
}

pub struct EngineUnitStore {
    root: PathBuf,
    component: ModuleComponent,
    channel: String,
    scope: UiScope,
}
impl EngineUnitStore {
    /// Open or create this machine's store for one unit, lane and scope.
    pub fn managed(
        component: ModuleComponent,
        channel: &str,
        scope: UiScope,
    ) -> Result<Self, String> {
        Self::new(&private_base()?, component, channel, scope)
    }
    /// Open retained bytes without creating any installation state.
    pub fn retained(
        component: ModuleComponent,
        channel: &str,
        scope: UiScope,
    ) -> Result<Self, String> {
        admit_scope(channel, scope)?;
        ui_platform()?;
        policy::admit_target(component.as_str(), channel).map_err(|e| e.to_string())?;
        let base = ds_cli_exec::addons::managed_module_base().map_err(|e| e.to_string())?;
        let root = store_base(&base, scope, false)?
            .join(component.as_str())
            .join(channel);
        if !exists_plain(&root)? {
            return Err("module_not_installed: no retained engine unit installation".into());
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
    pub fn new(
        base: &Path,
        component: ModuleComponent,
        channel: &str,
        scope: UiScope,
    ) -> Result<Self, String> {
        admit_scope(channel, scope)?;
        ui_platform()?;
        policy::admit_target(component.as_str(), channel).map_err(|e| e.to_string())?;
        if !base.is_absolute() {
            return Err("module_store_invalid: use the exact absolute native store".into());
        }
        let scoped = store_base(base, scope, true)?;
        #[cfg(windows)]
        {
            let owner = super::ui::windows_owner()?;
            (owner.prepare_directory)(&scoped.join(component.as_str()))?;
            (owner.prepare_directory)(&scoped.join(component.as_str()).join(channel))?;
        }
        let root = transfer::ensure_owned_directory(
            &scoped,
            &PathBuf::from(component.as_str()).join(channel),
        )?;
        for name in ["versions", "receipts"] {
            #[cfg(windows)]
            (super::ui::windows_owner()?.prepare_directory)(&root.join(name))?;
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
    /// Offline retained read: creates nothing and reads no catalog.
    pub fn selection_managed(
        component: ModuleComponent,
        channel: &str,
        scope: UiScope,
    ) -> Result<Option<EngineUnitSelection>, String> {
        let base = ds_cli_exec::addons::managed_module_base().map_err(|e| e.to_string())?;
        Self::selection_from(&base, component, channel, scope)
    }
    fn selection_from(
        base: &Path,
        component: ModuleComponent,
        channel: &str,
        scope: UiScope,
    ) -> Result<Option<EngineUnitSelection>, String> {
        admit_scope(channel, scope)?;
        ui_platform()?;
        policy::admit_target(component.as_str(), channel).map_err(|e| e.to_string())?;
        if !base.is_absolute() {
            return Err("module_store_invalid: invalid engine store".into());
        }
        let scoped = store_base(base, scope, false)?;
        let root = scoped.join(component.as_str()).join(channel);
        let parent = root.parent().expect("component directory");
        for directory in [base, scoped.as_path(), parent, root.as_path()] {
            if !exists_plain(&directory)? {
                return Ok(None);
            }
            ordinary_directory(&directory)?;
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
            (super::ui::windows_owner()?.validate_directory)(
                self.root
                    .parent()
                    .ok_or("module_store_invalid: missing engine store parent")?,
            )?;
            super::ui::audit_windows_tree(&self.root)?;
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
                    return Err("module_store_invalid: engine unit directories must be private to this OS account".into());
                }
            }
        }
        Ok(())
    }
    fn lock(&self) -> Result<ModuleLock, String> {
        self.check_root()?;
        let file = transfer::open_download_partial(&self.root.join("update.lock"))?;
        file.try_lock_exclusive().map_err(|_| {
            "module_update_busy: another update owns this engine unit".to_owned()
        })?;
        Ok(ModuleLock(file))
    }
    /// Shared read lock of a retained store: captures and member reads.
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
            return Err("module_store_invalid: invalid retained engine unit lock".into());
        }
        // Readers share the lock (concurrent member reads of one page load);
        // only a writer's exclusive `lock` refuses them, and they refuse it.
        FileExt::try_lock_shared(&file)
            .map_err(|_| "module_update_busy: another update owns this engine unit".to_owned())?;
        Ok(ModuleLock(file))
    }
    pub fn selection(&self) -> Result<Option<EngineUnitSelection>, String> {
        self.check_root()?;
        let path = self.root.join("selection.json");
        if !exists_plain(&path)? {
            return Ok(None);
        }
        let selected: EngineUnitSelection =
            serde_json::from_slice(&read_plain(&path, MAX_STORED_BYTES)?)
                .map_err(|_| "module_receipt_invalid: malformed engine unit selection")?;
        policy::validate_selection(&selected, self.component, &self.channel)
            .map_err(|e| e.to_string())?;
        let (active, previous) = match &selected.selection {
            EngineSelectionState::Installed { active, previous } => (Some(active), previous),
            EngineSelectionState::Removed { previous } => (None, previous),
        };
        for held in active.into_iter().chain(previous) {
            self.admit_receipt(held)?;
        }
        Ok(Some(selected))
    }
    fn unit_directory(&self, digest: &str) -> Result<PathBuf, String> {
        self.check_root()?;
        if !transfer::exact_sha256(digest) {
            return Err("module_receipt_invalid: invalid engine unit digest".into());
        }
        ordinary_directory(&self.root.join("versions"))?;
        let directory = self.root.join("versions").join(digest);
        ordinary_directory(&directory)?;
        Ok(directory)
    }
    fn admit_receipt(&self, receipt: &InstalledEngineUnit) -> Result<(), String> {
        if receipt.unit.component() != self.component || receipt.unit.scope() != self.scope {
            return Err("module_receipt_invalid: crossed engine unit component or scope".into());
        }
        policy::admit_retained(receipt, &self.channel).map_err(|e| e.to_string())?;
        Ok(())
    }
    fn verify_at(
        &self,
        receipt: &InstalledEngineUnit,
        directory: &Path,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<VerifiedFile>, String> {
        self.admit_receipt(receipt)?;
        ordinary_directory(directory)?;
        let files = receipt.unit.files();
        for file in &files {
            transfer::reopen_verified_download(
                &directory.join(&file.filename),
                file.size_bytes as u64,
                &file.sha256,
                || check_cancel(cancelled),
            )
            .map_err(|e| format!("module_artifact_invalid: {e}"))?;
        }
        policy::verify_files(&receipt.unit, &files).map_err(|e| e.to_string())?;
        Ok(files)
    }
    fn verify(
        &self,
        receipt: &InstalledEngineUnit,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), String> {
        self.verify_at(
            receipt,
            &self.unit_directory(receipt.unit.unit_sha256())?,
            cancelled,
        )
        .map(|_| ())
    }
    /// Download and verify a granted published unit; no selection changes.
    pub fn stage<F: FnMut(TransferProgress)>(
        &self,
        grant: TrustedEngineGrant,
        mut report: F,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<StagedEngineUnit, String> {
        self.stage_with(grant, cancelled, |asset, path| {
            transfer::download_verified(
                &asset.url,
                path,
                &asset.sha256,
                asset.size_bytes as u64,
                self.component.as_str(),
                &mut report,
                TransferAdmission {
                    check_cancelled: || check_cancel(cancelled),
                    admit_transport: || Ok(()),
                },
            )
            .map(|_| ())
        })
    }
    fn stage_with(
        &self,
        grant: TrustedEngineGrant,
        cancelled: &dyn Fn() -> bool,
        mut download: impl FnMut(
            &ds_command_kernel::module_release::ModuleReleaseAsset,
            &Path,
        ) -> Result<(), String>,
    ) -> Result<StagedEngineUnit, String> {
        if self.scope != UiScope::Release {
            return Err(
                "module_scope_invalid: published acquisition cannot stage into local-dev".into(),
            );
        }
        if grant.unit.component() != self.component || grant.unit.channel() != self.channel {
            return Err("module_receipt_invalid: crossed engine unit grant".into());
        }
        let assets =
            policy::acquisition_plan(&grant.unit, &self.channel, grant.unit.unit_sha256(), true)
                .map_err(|e| e.to_string())?;
        self.retain_unit(grant.unit, grant.authority, cancelled, |directory| {
            for asset in &assets {
                check_cancel(cancelled)?;
                download(
                    asset,
                    &directory.join(asset.url.rsplit('/').next().unwrap_or_default()),
                )?;
            }
            Ok(())
        })
    }
    /// Explicit OS-user import of one already built working-tree unit: four
    /// fixed files, the identity those tested bytes exported and their WASM
    /// digest. Nothing is built, fetched or evaluated here.
    pub fn stage_local(
        &self,
        directory: &Path,
        identity_file: &Path,
        source_sha: &str,
        wasm_sha256: &str,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<StagedEngineUnit, String> {
        if self.scope != UiScope::LocalDev || self.channel != "canary" {
            return Err(
                "module_scope_invalid: local builds require the canary local-dev store".into(),
            );
        }
        if !directory.is_absolute()
            || !identity_file.is_absolute()
            || !transfer::exact_sha256(wasm_sha256)
        {
            return Err("module_artifact_invalid: use absolute local producer inputs and an exact WASM digest".into());
        }
        ordinary_directory(directory)?;
        let identity: Value = serde_json::from_slice(&read_plain(identity_file, 16 * 1024)?)
            .map_err(|_| "module_engine_incompatible: malformed local engine identity")?;
        if identity["sourceSha"].as_str() != Some(source_sha) {
            return Err("module_engine_incompatible: the local identity names another source snapshot".into());
        }
        let names = policy::member_names(self.component).map_err(|e| e.to_string())?;
        let mut held = Vec::new();
        let mut verified = Vec::new();
        let mut total = 0usize;
        for filename in names {
            check_cancel(cancelled)?;
            let bytes = read_plain(&directory.join(filename), MAX_UNIT_BYTES as u64)?;
            total = total
                .checked_add(bytes.len())
                .filter(|total| *total <= MAX_UNIT_BYTES)
                .ok_or("module_artifact_invalid: local engine unit exceeds 128 MiB")?;
            let sha256 = format!("{:x}", Sha256::digest(&bytes));
            if filename.ends_with("_bg.wasm") && sha256 != wasm_sha256 {
                return Err("module_artifact_invalid: local WASM differs from the selected tested artifact".into());
            }
            verified.push(VerifiedFile {
                filename: filename.into(),
                sha256,
                size_bytes: bytes.len() as i64,
            });
            held.push((filename, bytes));
        }
        let unit = policy::local_unit(self.component, &self.channel, &identity, &verified)
            .map_err(|e| e.to_string())?;
        let authority = policy::local_authority(&unit).map_err(|e| e.to_string())?;
        self.retain_unit(unit, authority, cancelled, |destination| {
            for (filename, bytes) in &held {
                write_new_bounded(&destination.join(filename), bytes, MAX_UNIT_BYTES as u64)?;
            }
            Ok(())
        })
    }
    fn retain_unit(
        &self,
        unit: EngineUnit,
        authority: UiAuthority,
        cancelled: &dyn Fn() -> bool,
        mut populate: impl FnMut(&Path) -> Result<(), String>,
    ) -> Result<StagedEngineUnit, String> {
        check_cancel(cancelled)?;
        if unit.component() != self.component || unit.scope() != self.scope {
            return Err("module_receipt_invalid: crossed engine unit component or scope".into());
        }
        let _lock = self.lock()?;
        let generation = self
            .selection()?
            .map_or(0, |selection| selection.generation);
        let directory = self.root.join("versions").join(unit.unit_sha256());
        let expected = unit.files();
        let installed = if exists_plain(&directory)? {
            let retained: InstalledEngineUnit = serde_json::from_slice(&read_plain(
                &directory.join("installation.json"),
                MAX_STORED_BYTES,
            )?)
            .map_err(|_| "module_receipt_invalid: malformed retained engine installation")?;
            self.admit_receipt(&retained)?;
            if retained.unit != unit {
                return Err(
                    "module_receipt_invalid: retained engine installation unit changed".into(),
                );
            }
            self.verify_at(&retained, &directory, cancelled)?;
            retained
        } else {
            let temporary = PrivateDirectory(transfer::create_fresh_private_directory(
                &self.root, "stage",
            )?);
            populate(&temporary.0)?;
            // Verify the bytes actually written before minting the receipt.
            for file in &expected {
                transfer::reopen_verified_download(
                    &temporary.0.join(&file.filename),
                    file.size_bytes as u64,
                    &file.sha256,
                    || check_cancel(cancelled),
                )
                .map_err(|e| format!("module_artifact_invalid: {e}"))?;
            }
            let installed =
                policy::prepare(unit, authority, &expected, true).map_err(|e| e.to_string())?;
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
            installed
        };
        let stored = StoredEngineStage {
            schema: STAGE_SCHEMA.into(),
            receipt_id: uuid::Uuid::new_v4().simple().to_string(),
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
    /// Keep an installed published unit on the channel's active release;
    /// absence and removal are retained. A loaded page keeps its capture.
    pub fn converge<F: FnMut(TransferProgress)>(
        &self,
        grant: TrustedEngineGrant,
        guard: ActivationGuard,
        report: F,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<EngineConvergence, String> {
        check_cancel(cancelled)?;
        if grant.unit.component() != self.component || grant.unit.channel() != self.channel {
            return Err("module_receipt_invalid: crossed engine unit grant".into());
        }
        let current = self.selection()?;
        let planned = current.as_ref().map_or(0, |selected| selected.generation);
        let version = grant.unit.version();
        let digest = grant.unit.unit_sha256().to_owned();
        let outcome = convergence_plan(current.as_ref(), &digest);
        let generation = if outcome == EngineConvergenceOutcome::Updated {
            let staged = self.stage(grant, report, cancelled)?;
            self.activate(&staged.receipt_id, planned, guard, cancelled)?
                .generation
        } else {
            planned
        };
        Ok(EngineConvergence {
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
            return Err("module_receipt_invalid: invalid engine unit staging receipt".into());
        }
        ordinary_directory(&self.root.join("receipts"))?;
        Ok(self.root.join("receipts").join(format!("{id}.json")))
    }
    fn recover_stage(&self, id: &str) -> Result<StoredEngineStage, String> {
        let stored: StoredEngineStage =
            serde_json::from_slice(&read_plain(&self.receipt_path(id)?, MAX_STORED_BYTES)?)
                .map_err(|_| "module_receipt_invalid: malformed engine unit stage")?;
        if stored.schema != STAGE_SCHEMA || stored.receipt_id != id {
            return Err("module_receipt_invalid: crossed engine unit stage".into());
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
    ) -> Result<EngineUnitSelection, String> {
        guard.admit()?;
        check_cancel(cancelled)?;
        let _lock = self.lock()?;
        let staged = self.recover_stage(receipt_id)?;
        if staged.expected_generation != generation {
            return Err(
                "module_selection_changed: engine unit stage captured another generation".into(),
            );
        }
        let current = self.selection()?;
        self.verify(&staged.installed, cancelled)?;
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
    ) -> Result<EngineUnitSelection, String> {
        guard.admit()?;
        check_cancel(cancelled)?;
        let _lock = self.lock()?;
        let current = self
            .selection()?
            .ok_or("module_not_installed: no engine unit is installed")?;
        let selected = policy::remove(&current, generation, true).map_err(|e| e.to_string())?;
        self.write_selection(&selected, cancelled)?;
        Ok(selected)
    }
    pub fn rollback(
        &self,
        generation: u64,
        guard: ActivationGuard,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<EngineUnitSelection, String> {
        guard.admit()?;
        check_cancel(cancelled)?;
        let _lock = self.lock()?;
        let mut current = self
            .selection()?
            .ok_or("module_rollback_unavailable: no retained engine unit selection")?;
        let previous = match &current.selection {
            EngineSelectionState::Installed { previous, .. }
            | EngineSelectionState::Removed { previous } => previous.as_ref(),
        }
        .ok_or("module_rollback_unavailable: no previous engine unit")?;
        self.verify(previous, cancelled)?;
        // Recover from damaged active bytes without keeping a broken rollback target.
        if let EngineSelectionState::Installed { active, previous } = &current.selection {
            if self.verify(active, cancelled).is_err() {
                current.selection = EngineSelectionState::Removed {
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
        selected: &EngineUnitSelection,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), String> {
        policy::validate_selection(selected, self.component, &self.channel)
            .map_err(|e| e.to_string())?;
        check_cancel(cancelled)?;
        let bytes = serde_json::to_vec(selected).map_err(|e| e.to_string())?;
        persist_selection(&self.root, &bytes, MAX_STORED_BYTES, cancelled)
    }
    /// Verify and capture the active unit for one page load.
    pub fn capture(&self, cancelled: &dyn Fn() -> bool) -> Result<EngineUnitCapture, String> {
        let _lock = self.lock_retained()?;
        let selected = self
            .selection()?
            .ok_or("module_not_installed: no engine unit is installed")?;
        if let EngineSelectionState::Installed { active, .. } = &selected.selection {
            self.verify(active, cancelled)?;
        }
        policy::capture(Some(&selected), self.component, &self.channel).map_err(|e| e.to_string())
    }
    /// One bounded chunk of a fixed member of the unit a capture names. Old
    /// captures stay readable after an update or removal.
    pub fn member(
        &self,
        unit_sha256: &str,
        filename: &str,
        offset: u64,
        limit: usize,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(Vec<u8>, VerifiedFile), String> {
        let names = policy::member_names(self.component).map_err(|e| e.to_string())?;
        if limit == 0 || limit > MAX_MEMBER_CHUNK || !names.contains(&filename) {
            return Err(
                "module_artifact_invalid: select a fixed engine member and bounded range".into(),
            );
        }
        let _lock = self.lock_retained()?;
        let directory = self.unit_directory(unit_sha256)?;
        let receipt: InstalledEngineUnit = serde_json::from_slice(&read_plain(
            &directory.join("installation.json"),
            MAX_STORED_BYTES,
        )?)
        .map_err(|_| "module_receipt_invalid: malformed retained engine installation")?;
        if receipt.unit.unit_sha256() != unit_sha256 {
            return Err("module_receipt_invalid: crossed retained engine unit".into());
        }
        self.admit_receipt(&receipt)?;
        let file = receipt
            .unit
            .files()
            .into_iter()
            .find(|file| file.filename == filename)
            .ok_or("module_receipt_invalid: missing fixed engine member")?;
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
/// Offline answer for a unit that does not follow its channel (absent or
/// removed), so the caller reads no catalog; `None` means it follows.
pub fn retained_convergence(
    component: ModuleComponent,
    channel: &str,
    selection: Option<&EngineUnitSelection>,
) -> Option<EngineConvergence> {
    let outcome = match selection.map(|selected| &selected.selection) {
        None => EngineConvergenceOutcome::Absent,
        Some(EngineSelectionState::Removed { .. }) => EngineConvergenceOutcome::Removed,
        Some(EngineSelectionState::Installed { .. }) => return None,
    };
    Some(EngineConvergence {
        schema: CONVERGENCE_SCHEMA,
        component,
        channel: channel.into(),
        outcome,
        generation: selection.map_or(0, |selected| selected.generation),
        release_version: None,
        unit_sha256: None,
    })
}
/// Only an installed unit follows its channel; the active release decides.
pub fn convergence_plan(
    selection: Option<&EngineUnitSelection>,
    active_release_sha256: &str,
) -> EngineConvergenceOutcome {
    match selection.map(|selected| &selected.selection) {
        None => EngineConvergenceOutcome::Absent,
        Some(EngineSelectionState::Removed { .. }) => EngineConvergenceOutcome::Removed,
        Some(EngineSelectionState::Installed { active, .. })
            if active.unit.unit_sha256() == active_release_sha256 =>
        {
            EngineConvergenceOutcome::Current
        }
        Some(EngineSelectionState::Installed { .. }) => EngineConvergenceOutcome::Updated,
    }
}
fn staged(stored: StoredEngineStage) -> StagedEngineUnit {
    StagedEngineUnit {
        schema: stored.schema,
        state: "ready".into(),
        receipt_id: stored.receipt_id,
        component: stored.installed.unit.component(),
        channel: stored.installed.unit.channel().to_owned(),
        scope: stored.installed.unit.scope(),
        version: stored.installed.unit.version(),
        sha256: stored.installed.unit.unit_sha256().to_owned(),
        expected_generation: stored.expected_generation,
    }
}

#[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
#[path = "engine_unit_tests.rs"]
mod tests;
