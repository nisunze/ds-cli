//! Fixed native addon effects over the existing release authority and transfer.
//! Only native HTTPS acquisition mints a grant. Held/UI catalog JSON cannot.
pub mod engine_unit;
pub mod release_identity;
pub mod ui;
use crate::transfer::{self, TransferAdmission, TransferProgress};
use ds_client_core::{ClientProfile, DeploymentLane};
use ds_command_kernel::module_install::{
    self, ActivationGuard, InstalledModule, ModuleSelection, ModuleSelectionRecord,
    ReleaseAuthorityReceipt, RemovedModuleSelection, RemovedModuleState, INSTALLED_SCHEMA,
    REMOVED_SELECTION_SCHEMA, SELECTION_SCHEMA,
};
use ds_command_kernel::module_release::{validate_module_release, ModuleComponent, ModuleRelease};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

const MAX_RELEASE_BYTES: u64 = 128 * 1024;
const MAX_STAGE_BYTES: u64 = module_install::MAX_SELECTION_BYTES as u64;
const STAGE_SCHEMA: &str = "ds.module-stage/v1";

/// Release the OS lock explicitly before closing its descriptor. A concurrent
/// native engine spawn can briefly inherit an open description across fork.
struct ModuleLock(File);
impl Drop for ModuleLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.0);
    }
}

/// Privileged native adapter input. Construct only from the exact production
/// catalog loaded by the owning native profile loader; this is not an IPC DTO.
/// The loader refuses development catalogs and local-Go before calling here.
pub struct CapturedReleaseAuthority {
    gateway_origin: String,
    channel: &'static str,
    catalog_sha256: String,
    gateway_key: String,
}
impl CapturedReleaseAuthority {
    pub fn from_packaged_profile(
        profile: ClientProfile,
        catalog_sha256: String,
        gateway_key: String,
    ) -> Result<Self, String> {
        if !transfer::exact_sha256(&catalog_sha256)
            || profile.source_revision().len() != 40
            || !profile
                .source_revision()
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || gateway_key.is_empty()
            || gateway_key.len() > 512
        {
            return Err(
                "module_authority_unavailable: use the exact packaged production native profile"
                    .into(),
            );
        }
        let channel = match profile.lane() {
            DeploymentLane::Stable => "stable",
            DeploymentLane::Canary => "canary",
        };
        Ok(Self {
            gateway_origin: profile.gateway_origin().to_owned(),
            channel,
            catalog_sha256,
            gateway_key,
        })
    }
    pub fn channel(&self) -> &'static str {
        self.channel
    }
    fn url(&self, component: ModuleComponent) -> Result<String, String> {
        self.catalog_url(&format!(
            "component={}&channel={}",
            component.as_str(),
            self.channel()
        ))
    }
    /// The declared public release read of this lane, with one fixed query.
    fn catalog_url(&self, query: &str) -> Result<String, String> {
        let operation = ds_client_core::gateway_operation("domains.releases.desktop_manifest")
            .ok_or("module_authority_unavailable: release operation is undeclared")?;
        if operation.method != "GET"
            || operation.path != "/api/v1/app_releases"
            || operation.authority != "release"
        {
            return Err("module_authority_unavailable: release operation contract changed".into());
        }
        Ok(format!(
            "{}{}?{query}",
            self.gateway_origin.as_str(),
            operation.path
        ))
    }
    /// One intentional read, with no redirects or retry/polling loop.
    pub fn acquire(
        &self,
        component: ModuleComponent,
        expected_sha256: &str,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<TrustedModuleGrant, String> {
        self.acquire_with(component, expected_sha256, cancelled, |url| {
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
        })
    }
    fn acquire_with<F>(
        &self,
        component: ModuleComponent,
        expected_sha256: &str,
        cancelled: &dyn Fn() -> bool,
        read: F,
    ) -> Result<TrustedModuleGrant, String>
    where
        F: FnOnce(&str) -> Result<Vec<u8>, String>,
    {
        check_cancel(cancelled)?;
        if !transfer::exact_sha256(expected_sha256) {
            return Err("module_release_invalid: name the exact selected asset digest".into());
        }
        let url = self.url(component)?;
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
            return Err(
                "module_release_unpublished: no admitted module release is published".into(),
            );
        }
        let release =
            validate_module_release(component.as_str(), self.channel(), &envelope.release)
                .map_err(|e| e.to_string())?;
        if release.status != "active" {
            return Err("module_release_inactive: retain the installed unit; this release is withdrawn or blocked".into());
        }
        let platform = current_platform()?;
        let asset = release
            .assets
            .iter()
            .find(|a| a.platform == platform)
            .ok_or("module_platform_unsupported: this release has no native asset for this host")?;
        if asset.sha256 != expected_sha256 {
            return Err("module_release_changed: the selected asset no longer matches the authority; refresh explicitly".into());
        }
        if asset.size_bytes as u64 > module_install::MAX_EXECUTABLE_BYTES {
            return Err("module_release_invalid: executable exceeds the native bound".into());
        }
        let authority = ReleaseAuthorityReceipt {
            gateway_origin: self.gateway_origin.as_str().to_owned(),
            native_catalog_sha256: self.catalog_sha256.clone(),
            release_sha256: release_digest(&release)?,
        };
        Ok(TrustedModuleGrant {
            release,
            platform: platform.to_owned(),
            authority,
        })
    }
}
/// No Deserialize/constructor: semantic catalog admission is not this grant.
#[derive(Debug)]
pub struct TrustedModuleGrant {
    release: ModuleRelease,
    platform: String,
    authority: ReleaseAuthorityReceipt,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StagedModule {
    state: &'static str,
    receipt_id: String,
    component: ModuleComponent,
    channel: String,
    version: String,
    sha256: String,
    #[serde(skip)]
    expected_generation: u64,
    #[serde(skip)]
    installed: InstalledModule,
}
impl StagedModule {
    pub fn receipt_id(&self) -> &str {
        &self.receipt_id
    }
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredStage {
    schema: String,
    receipt_id: String,
    expected_generation: u64,
    installed: InstalledModule,
}

/// The host supplies its captured component/lane, never a caller artifact path.
pub struct ModuleStore {
    root: PathBuf,
    component: ModuleComponent,
    channel: String,
    platform: String,
}
impl ModuleStore {
    /// Read retained selection without creating, changing or locking the store.
    /// This is recorded installation provenance, not a fresh executable probe.
    pub fn selection_managed(
        component: ModuleComponent,
        channel: &str,
    ) -> Result<Option<ModuleSelection>, String> {
        let base = ds_cli_exec::addons::managed_module_base().map_err(|e| e.to_string())?;
        Self::selection_from(&base, component, channel)
    }
    /// Read both installed-v1 and removed-v2 records without creating a store.
    pub fn selection_record_managed(
        component: ModuleComponent,
        channel: &str,
    ) -> Result<Option<ModuleSelectionRecord>, String> {
        let base = ds_cli_exec::addons::managed_module_base().map_err(|e| e.to_string())?;
        Self::selection_record_from(&base, component, channel)
    }
    /// Apply before loading release authority or acquiring any automatic update.
    /// The host also applies its existing automatic-update preferences.
    pub fn admit_automatic_acquisition_managed(
        component: ModuleComponent,
        channel: &str,
    ) -> Result<(), String> {
        Self::selection_record_managed(component, channel)?
            .map(ModuleSelectionRecord::installed)
            .transpose()?;
        Ok(())
    }
    fn selection_from(
        base: &Path,
        component: ModuleComponent,
        channel: &str,
    ) -> Result<Option<ModuleSelection>, String> {
        Self::selection_record_from(base, component, channel)?
            .map(ModuleSelectionRecord::installed)
            .transpose()
    }
    fn selection_record_from(
        base: &Path,
        component: ModuleComponent,
        channel: &str,
    ) -> Result<Option<ModuleSelectionRecord>, String> {
        if !base.is_absolute() || !matches!(channel, "stable" | "canary") {
            return Err(
                "module_store_invalid: use an absolute owned base and exact installed lane".into(),
            );
        }
        let platform = current_platform()?.to_owned();
        let component_root = base.join(component.as_str());
        let root = component_root.join(channel);
        for directory in [base, component_root.as_path(), root.as_path()] {
            if !exists_plain(directory)? {
                return Ok(None);
            }
            ordinary_directory(directory)?;
        }
        Self {
            root,
            component,
            channel: channel.to_owned(),
            platform,
        }
        .read_selection_record()
    }
    pub fn managed(component: ModuleComponent, channel: &str) -> Result<Self, String> {
        current_platform()?;
        let base = ds_cli_exec::addons::managed_module_base().map_err(|e| e.to_string())?;
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(&base)
            .map_err(|e| format!("module_store_invalid: {e}"))?;
        ordinary_directory(&base)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&base, fs::Permissions::from_mode(0o700))
                .map_err(|e| e.to_string())?;
        }
        Self::new(&base, component, channel)
    }
    pub fn new(base: &Path, component: ModuleComponent, channel: &str) -> Result<Self, String> {
        let platform = current_platform()?.to_owned();
        if !base.is_absolute() || !matches!(channel, "stable" | "canary") {
            return Err(
                "module_store_invalid: use an absolute owned base and exact installed lane".into(),
            );
        }
        let root = transfer::ensure_owned_directory(
            base,
            &PathBuf::from(component.as_str()).join(channel),
        )?;
        transfer::ensure_owned_directory(&root, Path::new("versions"))?;
        transfer::ensure_owned_directory(&root, Path::new("receipts"))?;
        Ok(Self {
            root,
            component,
            channel: channel.to_owned(),
            platform,
        })
    }
    fn lock(&self) -> Result<ModuleLock, String> {
        self.check_root()?;
        let file = transfer::open_download_partial(&self.root.join("update.lock"))?;
        file.try_lock_exclusive().map_err(|_| {
            "module_update_busy: another native update owns this component".to_owned()
        })?;
        Ok(ModuleLock(file))
    }
    fn read_selection(&self) -> Result<Option<ModuleSelection>, String> {
        self.read_selection_record()?
            .map(ModuleSelectionRecord::installed)
            .transpose()
    }
    fn read_selection_record(&self) -> Result<Option<ModuleSelectionRecord>, String> {
        self.check_root()?;
        let path = self.root.join("selection.json");
        if !exists_plain(&path)? {
            return Ok(None);
        }
        let bytes = read_plain(&path, MAX_STAGE_BYTES)?;
        let selected: ModuleSelectionRecord = serde_json::from_slice(&bytes)
            .map_err(|_| "module_receipt_invalid: malformed native selection")?;
        module_install::validate_selection_record(
            &selected,
            self.component,
            &self.channel,
            &self.platform,
        )?;
        Ok(Some(selected))
    }
    fn check_root(&self) -> Result<(), String> {
        for directory in self.root.ancestors().take(3) {
            ordinary_directory(directory)?;
        }
        Ok(())
    }
    fn path(&self, receipt: &InstalledModule) -> Result<PathBuf, String> {
        module_install::validate_installed(receipt, self.component, &self.channel, &self.platform)?;
        let asset = module_install::selected_asset(receipt)?;
        ordinary_directory(&self.root)?;
        ordinary_directory(&self.root.join("versions"))?;
        let directory = self.root.join("versions").join(&asset.sha256);
        ordinary_directory(&directory)?;
        Ok(directory.join(module_install::executable_name(
            self.component,
            &self.platform,
        )?))
    }
    fn verify(
        &self,
        receipt: &InstalledModule,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<PathBuf, String> {
        let path = self.path(receipt)?;
        let asset = module_install::selected_asset(receipt)?;
        transfer::reopen_verified_download(&path, asset.size_bytes as u64, &asset.sha256, || {
            check_cancel(cancelled)
        })?;
        Ok(path)
    }
    /// Stage acquires no project data and changes no active selection.
    pub fn stage<F>(
        &self,
        grant: TrustedModuleGrant,
        report: F,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<StagedModule, String>
    where
        F: FnMut(TransferProgress),
    {
        self.stage_with_admission(grant, report, cancelled, || Ok(()))
    }
    /// The host injects only its held connectivity admission at the existing
    /// transfer points; acquisition and transfer policy remain with this owner.
    pub fn stage_with_admission<F, A>(
        &self,
        grant: TrustedModuleGrant,
        report: F,
        cancelled: &dyn Fn() -> bool,
        admit_transport: A,
    ) -> Result<StagedModule, String>
    where
        F: FnMut(TransferProgress),
        A: Fn() -> Result<(), String> + Copy,
    {
        self.stage_transfer(grant, report, cancelled, admit_transport, false)
    }
    /// Automatic updates recheck removed state while holding the same lock as
    /// removal. Hosts admit before authority loading, then use this stage path
    /// so an update acquired before removal cannot reinstall afterwards.
    pub fn stage_automatic_with_admission<F, A>(
        &self,
        grant: TrustedModuleGrant,
        report: F,
        cancelled: &dyn Fn() -> bool,
        admit_transport: A,
    ) -> Result<StagedModule, String>
    where
        F: FnMut(TransferProgress),
        A: Fn() -> Result<(), String> + Copy,
    {
        self.stage_transfer(grant, report, cancelled, admit_transport, true)
    }
    fn stage_transfer<F, A>(
        &self,
        grant: TrustedModuleGrant,
        report: F,
        cancelled: &dyn Fn() -> bool,
        admit_transport: A,
        automatic: bool,
    ) -> Result<StagedModule, String>
    where
        F: FnMut(TransferProgress),
        A: Fn() -> Result<(), String> + Copy,
    {
        self.stage_with_mode(grant, cancelled, automatic, |asset, path| {
            // An already-offline host refuses before entering transfer retry
            // scheduling. The transfer retains its existing mid-read checks.
            admit_transport()?;
            transfer::download_verified(
                &asset.url,
                path,
                &asset.sha256,
                asset.size_bytes as u64,
                self.component.as_str(),
                report,
                TransferAdmission {
                    check_cancelled: || check_cancel(cancelled),
                    admit_transport,
                },
            )
            .map(|_| ())
        })
    }
    #[cfg(test)]
    fn stage_with<F>(
        &self,
        grant: TrustedModuleGrant,
        cancelled: &dyn Fn() -> bool,
        download: F,
    ) -> Result<StagedModule, String>
    where
        F: FnOnce(
            &ds_command_kernel::module_release::ModuleReleaseAsset,
            &Path,
        ) -> Result<(), String>,
    {
        self.stage_with_mode(grant, cancelled, false, download)
    }
    fn stage_with_mode<F>(
        &self,
        grant: TrustedModuleGrant,
        cancelled: &dyn Fn() -> bool,
        automatic: bool,
        download: F,
    ) -> Result<StagedModule, String>
    where
        F: FnOnce(
            &ds_command_kernel::module_release::ModuleReleaseAsset,
            &Path,
        ) -> Result<(), String>,
    {
        check_cancel(cancelled)?;
        if grant.release.component != self.component
            || grant.release.channel != self.channel
            || grant.platform != self.platform
        {
            return Err(
                "module_receipt_invalid: grant belongs to another component or installed lane"
                    .into(),
            );
        }
        let _lock = self.lock()?;
        let selected = self.read_selection_record()?;
        if automatic && matches!(selected, Some(ModuleSelectionRecord::Removed(_))) {
            return Err(module_install::module_removed());
        }
        let generation = selected.map_or(0, |s| s.generation());
        let asset = grant
            .release
            .assets
            .iter()
            .find(|a| a.platform == self.platform)
            .ok_or("module_platform_unsupported")?;
        ordinary_directory(&self.root)?;
        ordinary_directory(&self.root.join("versions"))?;
        let directory = self.root.join("versions").join(&asset.sha256);
        let filename = module_install::executable_name(self.component, &self.platform)?;
        let path = if exists_plain(&directory)? {
            ordinary_directory(&directory)?;
            directory.join(&filename)
        } else {
            let private = transfer::create_fresh_private_directory(&self.root, "stage")?;
            let temporary = PrivateDirectory(private);
            let candidate = temporary.0.join(&filename);
            download(asset, &candidate)?;
            check_cancel(cancelled)?;
            transfer::reopen_verified_download(
                &candidate,
                asset.size_bytes as u64,
                &asset.sha256,
                || check_cancel(cancelled),
            )?;
            executable_permissions(&candidate)?;
            // The destination is never overwritten. The component lock owns
            // this digest placement, and every retained job keeps its path.
            sync_directory(&temporary.0)?;
            fs::rename(&temporary.0, &directory)
                .map_err(|e| format!("module_stage_failed: retain active selection; {e}"))?;
            sync_directory(&self.root.join("versions"))?;
            sync_directory(&self.root)?;
            directory.join(&filename)
        };
        let held = transfer::reopen_verified_download(
            &path,
            asset.size_bytes as u64,
            &asset.sha256,
            || check_cancel(cancelled),
        )?;
        let owner = match self.component {
            ModuleComponent::Solar => &ds_cli_exec::addons::SOLAR,
            ModuleComponent::Reporter => &ds_cli_exec::addons::REPORTER,
            ModuleComponent::Network => &ds_cli_exec::addons::NETWORK,
        };
        let engine = owner
            .capture_pinned(&path)
            .map_err(|e| format!("module_engine_incompatible: {e}"))?;
        let completed = engine
            .call_cancelled("build-info", &[], Duration::from_secs(20), 1, cancelled)
            .map_err(|e| format!("module_engine_incompatible: {e}"))?;
        if completed.truncated || !completed.succeeded() || completed.stdout.len() > 16 * 1024 {
            return Err(
                "module_engine_incompatible: fixed build-info task failed or exceeded its bound"
                    .into(),
            );
        }
        let identity = serde_json::from_str(&completed.stdout)
            .map_err(|_| "module_engine_incompatible: malformed build-info")?;
        drop(held);
        transfer::reopen_verified_download(&path, asset.size_bytes as u64, &asset.sha256, || {
            check_cancel(cancelled)
        })?;
        let installed = InstalledModule {
            schema: INSTALLED_SCHEMA.into(),
            release: grant.release,
            platform: grant.platform,
            authority: grant.authority,
            engine_identity: identity,
        };
        module_install::validate_installed(
            &installed,
            self.component,
            &self.channel,
            &self.platform,
        )?;
        let receipt_id = uuid::Uuid::new_v4().simple().to_string();
        let stored = StoredStage {
            schema: STAGE_SCHEMA.into(),
            receipt_id: receipt_id.clone(),
            expected_generation: generation,
            installed: installed.clone(),
        };
        write_new(
            &self.receipt_path(&receipt_id)?,
            &serde_json::to_vec(&stored).map_err(|e| e.to_string())?,
        )?;
        Ok(staged(stored))
    }
    fn receipt_path(&self, id: &str) -> Result<PathBuf, String> {
        if id.len() != 32
            || !id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("module_receipt_invalid: name exactly the native staging receipt".into());
        }
        Ok(self.root.join("receipts").join(format!("{id}.json")))
    }
    /// Reopen a native-owned stage after restart; no network or child probe.
    pub fn recover_stage(&self, id: &str) -> Result<StagedModule, String> {
        ordinary_directory(&self.root)?;
        ordinary_directory(&self.root.join("receipts"))?;
        let bytes = read_plain(&self.receipt_path(id)?, MAX_STAGE_BYTES)?;
        let stored: StoredStage = serde_json::from_slice(&bytes)
            .map_err(|_| "module_receipt_invalid: malformed native staging receipt")?;
        if stored.schema != STAGE_SCHEMA || stored.receipt_id != id {
            return Err("module_receipt_invalid: crossed staging receipt".into());
        }
        module_install::validate_installed(
            &stored.installed,
            self.component,
            &self.channel,
            &self.platform,
        )?;
        Ok(staged(stored))
    }
    pub fn activate(
        &self,
        ready: &StagedModule,
        guard: ActivationGuard,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<ModuleSelection, String> {
        self.activate_at_generation(ready, ready.expected_generation, guard, cancelled)
    }
    /// Confirm the caller's held generation under the same selection lock as
    /// stage recovery and the atomic write. A ready receipt is not consent to
    /// overwrite a different generation.
    pub fn activate_at_generation(
        &self,
        ready: &StagedModule,
        expected_generation: u64,
        guard: ActivationGuard,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<ModuleSelection, String> {
        guard.admit()?;
        check_cancel(cancelled)?;
        let _lock = self.lock()?;
        if ready.expected_generation != expected_generation {
            return Err("module_selection_changed: activation receipt differs from the caller's held generation".into());
        }
        let recovered = self.recover_stage(ready.receipt_id())?;
        if serde_json::to_vec(&recovered.installed).map_err(|e| e.to_string())?
            != serde_json::to_vec(&ready.installed).map_err(|e| e.to_string())?
            || recovered.expected_generation != ready.expected_generation
        {
            return Err("module_receipt_invalid: staged admission facts changed".into());
        }
        let prior_record = self.read_selection_record()?;
        if prior_record.as_ref().map_or(0, |s| s.generation()) != ready.expected_generation {
            return Err("module_selection_changed: a newer native activation already owns selection; retain this staged unit".into());
        }
        let previous = match prior_record {
            Some(ModuleSelectionRecord::Installed(s)) => Some(s),
            _ => None,
        };
        self.verify(&ready.installed, cancelled)?;
        if let Some(current) = &previous {
            self.verify(&current.active, cancelled)?;
        }
        let selected = ModuleSelection {
            schema: SELECTION_SCHEMA.into(),
            component: self.component,
            channel: self.channel.clone(),
            generation: ready
                .expected_generation
                .checked_add(1)
                .ok_or("module_receipt_invalid: selection generation exhausted")?,
            active: ready.installed.clone(),
            previous: previous.map(|s| s.active),
        };
        self.write_selection(
            &ModuleSelectionRecord::Installed(selected.clone()),
            cancelled,
        )?;
        Ok(selected)
    }
    pub fn rollback(
        &self,
        expected_generation: u64,
        guard: ActivationGuard,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<ModuleSelection, String> {
        guard.admit()?;
        check_cancel(cancelled)?;
        let _lock = self.lock()?;
        let current = self
            .read_selection()?
            .ok_or("module_rollback_unavailable: no active module installation")?;
        if current.generation != expected_generation {
            return Err(
                "module_selection_changed: rollback belongs to an earlier selection".into(),
            );
        }
        let previous = current
            .previous
            .ok_or("module_rollback_unavailable: no immediately usable previous unit")?;
        self.verify(&previous, cancelled)?;
        // A damaged active executable need not prevent recovery to a verified
        // previous unit. Never advertise that damaged unit as rollback-ready.
        let prior = self
            .verify(&current.active, cancelled)
            .ok()
            .map(|_| current.active);
        let selected = ModuleSelection {
            schema: SELECTION_SCHEMA.into(),
            component: self.component,
            channel: self.channel.clone(),
            generation: expected_generation
                .checked_add(1)
                .ok_or("module_receipt_invalid: selection generation exhausted")?,
            active: previous,
            previous: prior,
        };
        self.write_selection(
            &ModuleSelectionRecord::Installed(selected.clone()),
            cancelled,
        )?;
        Ok(selected)
    }
    /// Confirmed host removal fences the held generation and preserves immutable
    /// executables for jobs that captured them before this atomic transition.
    pub fn remove(
        &self,
        expected_generation: u64,
        guard: ActivationGuard,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<RemovedModuleSelection, String> {
        guard.admit()?;
        check_cancel(cancelled)?;
        let _lock = self.lock()?;
        let current = self.read_selection_record()?;
        if current.as_ref().map_or(0, |s| s.generation()) != expected_generation {
            return Err("module_selection_changed: removal belongs to an earlier selection".into());
        }
        let removed = RemovedModuleSelection {
            schema: REMOVED_SELECTION_SCHEMA.into(),
            component: self.component,
            channel: self.channel.clone(),
            generation: expected_generation
                .checked_add(1)
                .ok_or("module_receipt_invalid: selection generation exhausted")?,
            state: RemovedModuleState::Removed,
        };
        self.write_selection(&ModuleSelectionRecord::Removed(removed.clone()), cancelled)?;
        Ok(removed)
    }
    pub fn admit_automatic_acquisition(&self) -> Result<(), String> {
        self.read_selection()?;
        Ok(())
    }
    /// Intentional job capture uses retained bytes only, without a child probe
    /// or an authority freshness read. Callers keep this immutable path.
    pub fn selected_executable(
        &self,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Option<(PathBuf, InstalledModule)>, String> {
        let selected = self.read_selection()?;
        selected
            .map(|s| {
                self.verify(&s.active, cancelled)
                    .map(|path| (path, s.active))
            })
            .transpose()
    }
    pub fn selection(&self) -> Result<Option<ModuleSelection>, String> {
        self.read_selection()
    }
    pub fn selection_record(&self) -> Result<Option<ModuleSelectionRecord>, String> {
        self.read_selection_record()
    }
    fn write_selection(
        &self,
        selected: &ModuleSelectionRecord,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), String> {
        module_install::validate_selection_record(
            selected,
            self.component,
            &self.channel,
            &self.platform,
        )?;
        check_cancel(cancelled)?;
        let bytes = serde_json::to_vec(selected).map_err(|e| e.to_string())?;
        persist_selection(
            &self.root,
            &bytes,
            module_install::MAX_SELECTION_BYTES as u64,
            cancelled,
        )
    }
}
fn persist_selection(
    root: &Path,
    bytes: &[u8],
    max: u64,
    cancelled: &dyn Fn() -> bool,
) -> Result<(), String> {
    if bytes.len() as u64 > max {
        return Err("module_receipt_invalid: excessive selection".into());
    }
    let path = root.join("selection.json");
    if exists_plain(&path)? {
        ordinary_file(&path)?;
    }
    let mut staged = tempfile::NamedTempFile::new_in(root).map_err(|e| e.to_string())?;
    staged
        .write_all(bytes)
        .and_then(|_| staged.as_file().sync_all())
        .map_err(|e| e.to_string())?;
    check_cancel(cancelled)?;
    // tempfile persists by one atomic replace on both Unix and Windows.
    // Active and previous are one record; there is no torn two-file pair.
    staged.persist(&path).map_err(|e| {
        format!(
            "module_activation_failed: prior selection is retained; {}",
            e.error
        )
    })?;
    sync_directory(root)
}
fn staged(stored: StoredStage) -> StagedModule {
    let asset = stored
        .installed
        .release
        .assets
        .iter()
        .find(|a| a.platform == stored.installed.platform)
        .expect("validated staged asset");
    StagedModule {
        state: "ready",
        receipt_id: stored.receipt_id,
        component: stored.installed.release.component,
        channel: stored.installed.release.channel.clone(),
        version: stored.installed.release.version.clone(),
        sha256: asset.sha256.clone(),
        expected_generation: stored.expected_generation,
        installed: stored.installed,
    }
}
fn check_cancel(cancelled: &dyn Fn() -> bool) -> Result<(), String> {
    if cancelled() {
        Err("module_update_cancelled: retain the current installation".into())
    } else {
        Ok(())
    }
}
/// Side-effect-free admission for the native component store and release
/// acquisition. Call before credential/catalog reads or creating a store.
/// This admits metadata targets, not proof of installed engine execution.
pub fn current_platform() -> Result<&'static str, String> {
    if cfg!(target_arch = "x86_64") && cfg!(target_os = "linux") {
        Ok("linux")
    } else if cfg!(target_arch = "x86_64") && cfg!(target_os = "windows") {
        Ok("windows")
    } else {
        Err("module_platform_unsupported: use an admitted Linux or Windows x86_64 host".into())
    }
}
fn release_digest(release: &ModuleRelease) -> Result<String, String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(release).map_err(|e| e.to_string())?)
    ))
}
fn exists_plain(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(m) if !transfer::metadata_is_link_or_reparse(&m) => Ok(true),
        Ok(_) => Err("module_store_invalid: linked or reparsed native path".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.to_string()),
    }
}
fn ordinary_directory(path: &Path) -> Result<(), String> {
    let m = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if transfer::metadata_is_link_or_reparse(&m) || !m.is_dir() {
        Err("module_store_invalid: directory is linked, reparsed or absent".into())
    } else {
        Ok(())
    }
}
fn ordinary_file(path: &Path) -> Result<(), String> {
    let m = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if transfer::metadata_is_link_or_reparse(&m) || !m.is_file() {
        Err("module_store_invalid: receipt is linked, reparsed or not ordinary".into())
    } else {
        Ok(())
    }
}
fn read_plain(path: &Path, max: u64) -> Result<Vec<u8>, String> {
    ordinary_file(path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000).share_mode(1);
    }
    let mut file = options.open(path).map_err(|e| e.to_string())?;
    if !file
        .metadata()
        .is_ok_and(|m| m.is_file() && !transfer::metadata_is_link_or_reparse(&m) && m.len() <= max)
    {
        return Err("module_receipt_invalid: excessive or linked receipt".into());
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > max {
        return Err("module_receipt_invalid: receipt grew beyond its bound".into());
    }
    Ok(bytes)
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    write_new_bounded(path, bytes, MAX_STAGE_BYTES)
}
fn write_new_bounded(path: &Path, bytes: &[u8], max: u64) -> Result<(), String> {
    if bytes.len() as u64 > max {
        return Err("module_receipt_invalid: excessive native receipt".into());
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000).share_mode(1);
    }
    let mut file = options.open(path).map_err(|e| e.to_string())?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())?;
    sync_directory(
        path.parent()
            .ok_or("module_store_invalid: missing receipt parent")?,
    )
}
fn sync_directory(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        File::open(path)
            .and_then(|f| f.sync_all())
            .map_err(|e| format!("module_store_invalid: persist directory: {e}"))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
fn executable_permissions(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
struct PrivateDirectory(PathBuf);
impl Drop for PrivateDirectory {
    fn drop(&mut self) {
        let _ = transfer::remove_owned_directory_if_present(&self.0, "native module staging");
    }
}

#[cfg(all(test, unix))]
#[path = "modules_tests.rs"]
mod tests;
