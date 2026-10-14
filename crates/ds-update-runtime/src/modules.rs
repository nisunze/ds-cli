//! Fixed native addon effects over the existing release authority and transfer.
//! Only native HTTPS acquisition mints a grant. Held/UI catalog JSON cannot.
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
        let operation = ds_client_core::gateway_operation("domains.releases.desktop_manifest")
            .ok_or("module_authority_unavailable: release operation is undeclared")?;
        if operation.method != "GET"
            || operation.path != "/api/v1/app_releases"
            || operation.authority != "release"
        {
            return Err("module_authority_unavailable: release operation contract changed".into());
        }
        Ok(format!(
            "{}{}?component={}&channel={}",
            self.gateway_origin.as_str(),
            operation.path,
            component.as_str(),
            self.channel()
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
    fn lock(&self) -> Result<File, String> {
        self.check_root()?;
        let file = transfer::open_download_partial(&self.root.join("update.lock"))?;
        file.try_lock_exclusive().map_err(|_| {
            "module_update_busy: another native update owns this component".to_owned()
        })?;
        Ok(file)
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
        if bytes.len() > module_install::MAX_SELECTION_BYTES {
            return Err("module_receipt_invalid: excessive selection".into());
        }
        let path = self.root.join("selection.json");
        if exists_plain(&path)? {
            ordinary_file(&path)?;
        }
        let mut staged = tempfile::NamedTempFile::new_in(&self.root).map_err(|e| e.to_string())?;
        staged
            .write_all(&bytes)
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
        sync_directory(&self.root)
    }
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
    if bytes.len() as u64 > MAX_STAGE_BYTES {
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
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::Cell;
    fn identity(component: ModuleComponent, source: &str, manifest: &str) -> Value {
        match component {
            ModuleComponent::Network => {
                json!({"schema":"ds.engine-build/v1","module":"network","engineIdentity":{"schema":"ds.network-engine/v1","protocolMajor":1,"protocolMinor":0,"coreVersion":"0.1.0","sourceSha":source,"dependencyClosureSha256":manifest,"environment":"native","runtime":{"environment":"native","logicalCpus":8,"internalWorkerThreads":1},"jsonObjectOrder":"insertion","capabilities":["lv_processing_v1","lv_publication_v1","lv_analysis_validation_v1","grid_query_v1","grid_inspect_v1","grid_apply_v1","grid_publication_v1","profile_session_v1","report_engineering_v1","report_sizing_v1","report_feeder_selection_v1"]},"build":{"target":"x86_64-unknown-linux-gnu","profile":"release","rustc":"synthetic owner process fixture","cargoLockSha256":"a".repeat(64)}})
            }
            ModuleComponent::Solar => {
                json!({"schema":"ds.engine-build/v1","name":"ds-solar-engine","version":"0.1.0","source_sha":source,"cargo_lock_sha256":"a".repeat(64),"rustc":"synthetic owner process fixture","target":"x86_64-unknown-linux-gnu","profile":"release","schemas":["ds-solar.prepared-calculation-result/v1","ds-solar.result/v1","ds-solar.prepared-city-input/v1","ds-solar.worker-budget/v1"],"build_manifest_sha256":manifest})
            }
            ModuleComponent::Reporter => {
                json!({"schema":"ds.engine-build/v1","service":"ds-network-reporter","binary":"ds-report","package_version":"0.1.0","engine_version":format!("ds-network-reporter@0.1.0+{source}"),"source_sha":source,"cargo_lock_sha256":"a".repeat(64),"features":[],"rustc":"synthetic owner process fixture","target":"x86_64-unknown-linux-gnu","profile":"release","schemas":["ds.engine-build/v1","ds-network-reporter.task-result/v1"],"build_manifest_sha256":manifest})
            }
        }
    }
    fn fixture(component: ModuleComponent, n: u64) -> (ModuleRelease, Vec<u8>) {
        let source = format!("{n:040x}");
        let manifest = format!("{n:064x}");
        let info = serde_json::to_string(&identity(component, &source, &manifest)).unwrap();
        assert!(!info.contains('\''));
        let bytes=format!("#!/bin/sh\ncase \"$1\" in\n build-info|work) printf '%s\\n' '{info}';;\n *) exit 2;;\nesac\n").into_bytes();
        let release=validate_module_release(component.as_str(),"canary",&json!({"schema":"ds.module-release/v1","component":component,"channel":"canary","product_id":format!("rw.datasolutions.module.{}.canary",component.as_str()),"version":format!("0.1.0-r{n}"),"protocol":component.protocol(),"released_at":"2026-10-14T10:00:00Z","status":"active","assets":[{"platform":"linux","arch":"x86_64","kind":"module-executable","protocol":component.protocol(),"url":format!("https://storage.googleapis.com/synthetic-module-fixtures/modules/{}/canary/0.1.0-r{n}/{}",component.as_str(),component.executable()),"size_bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes)),"source_sha":source,"built_at":"2026-10-14T09:00:00Z","source_revisions":{component.engine_owner():source},"engines":[{"engine":component.engine_owner(),"release":format!("{}@0.1.0+{}",component.engine_owner(),source),"manifest_sha256":manifest}]}]})).unwrap();
        (release, bytes)
    }
    fn authority() -> CapturedReleaseAuthority {
        CapturedReleaseAuthority {
            gateway_origin: "https://synthetic-module-authority.ue.gateway.dev".into(),
            channel: "canary",
            catalog_sha256: "a".repeat(64),
            gateway_key: "synthetic public test key".into(),
        }
    }
    fn grant(release: ModuleRelease) -> TrustedModuleGrant {
        let component = release.component;
        let digest = release.assets[0].sha256.clone();
        authority()
            .acquire_with(component, &digest, &|| false, |_| {
                Ok(serde_json::to_vec(&json!({"success":true,"release":release})).unwrap())
            })
            .unwrap()
    }
    fn stage(store: &ModuleStore, release: ModuleRelease, bytes: &[u8]) -> StagedModule {
        store
            .stage_with(grant(release), &|| false, |_, path| write_new(path, bytes))
            .unwrap()
    }
    fn owner(component: ModuleComponent) -> &'static ds_cli_exec::External {
        match component {
            ModuleComponent::Solar => &ds_cli_exec::addons::SOLAR,
            ModuleComponent::Reporter => &ds_cli_exec::addons::REPORTER,
            ModuleComponent::Network => &ds_cli_exec::addons::NETWORK,
        }
    }
    #[test]
    fn host_connectivity_and_pre_cancel_stop_before_transfer_or_probe() {
        for component in [
            ModuleComponent::Solar,
            ModuleComponent::Reporter,
            ModuleComponent::Network,
        ] {
            let base = tempfile::tempdir().unwrap();
            let store = ModuleStore::new(base.path(), component, "canary").unwrap();
            let (release, _) = fixture(component, 1);
            let admissions = Cell::new(0);
            let error = store
                .stage_with_admission(
                    grant(release.clone()),
                    |_| {},
                    &|| false,
                    || {
                        admissions.set(admissions.get() + 1);
                        Err("offline: operator selected offline mode".into())
                    },
                )
                .unwrap_err();
            assert!(error.contains("offline:"));
            assert_eq!(admissions.get(), 1);
            assert!(store.selection().unwrap().is_none());
            assert_eq!(
                fs::read_dir(store.root.join("versions")).unwrap().count(),
                0
            );
            let error = store
                .stage_with_admission(
                    grant(release),
                    |_| {},
                    &|| true,
                    || panic!("pre-cancelled work must not consume connectivity or transport"),
                )
                .unwrap_err();
            assert!(error.contains("module_update_cancelled"));
            assert!(store.selection().unwrap().is_none());
        }
    }

    #[test]
    fn retained_selection_read_is_read_only_and_checks_existing_ancestors() {
        use std::os::unix::{fs::symlink, fs::PermissionsExt};
        let scratch = tempfile::tempdir().unwrap();
        let base = scratch.path().join("absent");
        assert!(
            ModuleStore::selection_from(&base, ModuleComponent::Network, "canary")
                .unwrap()
                .is_none()
        );
        assert!(!base.exists());
        fs::create_dir(&base).unwrap();
        fs::set_permissions(&base, fs::Permissions::from_mode(0o750)).unwrap();
        assert!(
            ModuleStore::selection_from(&base, ModuleComponent::Network, "canary")
                .unwrap()
                .is_none()
        );
        assert_eq!(
            fs::metadata(&base).unwrap().permissions().mode() & 0o777,
            0o750
        );
        assert_eq!(fs::read_dir(&base).unwrap().count(), 0);
        fs::write(base.join("network"), b"wrong type").unwrap();
        assert!(
            ModuleStore::selection_from(&base, ModuleComponent::Network, "canary")
                .unwrap_err()
                .contains("module_store_invalid")
        );
        fs::remove_file(base.join("network")).unwrap();
        symlink(scratch.path().join("missing"), base.join("network")).unwrap();
        assert!(
            ModuleStore::selection_from(&base, ModuleComponent::Network, "canary")
                .unwrap_err()
                .contains("module_store_invalid")
        );
        fs::remove_file(base.join("network")).unwrap();
        let store = ModuleStore::new(&base, ModuleComponent::Network, "canary").unwrap();
        let (release, bytes) = fixture(ModuleComponent::Network, 1);
        let staged = stage(&store, release, &bytes);
        let selected = store
            .activate(&staged, ActivationGuard::default(), &|| false)
            .unwrap();
        let path = store.root.join("selection.json");
        let held = fs::read(&path).unwrap();
        let lock_modified = fs::metadata(store.root.join("update.lock"))
            .unwrap()
            .modified()
            .unwrap();
        let root_modified = fs::metadata(&store.root).unwrap().modified().unwrap();
        assert_eq!(
            ModuleStore::selection_from(&base, ModuleComponent::Network, "canary").unwrap(),
            Some(selected.clone())
        );
        assert_eq!(fs::read(&path).unwrap(), held);
        assert_eq!(
            fs::metadata(store.root.join("update.lock"))
                .unwrap()
                .modified()
                .unwrap(),
            lock_modified
        );
        assert_eq!(
            fs::metadata(&store.root).unwrap().modified().unwrap(),
            root_modified
        );
        assert!(
            ModuleStore::selection_from(&base, ModuleComponent::Network, "stable")
                .unwrap()
                .is_none()
        );
        assert!(!base.join("network/stable").exists());
        let mut crossed = selected;
        crossed.channel = "stable".into();
        fs::write(&path, serde_json::to_vec(&crossed).unwrap()).unwrap();
        assert!(ModuleStore::selection_from(&base, ModuleComponent::Network, "canary").is_err());
        fs::write(&path, b"{broken").unwrap();
        assert!(
            ModuleStore::selection_from(&base, ModuleComponent::Network, "canary")
                .unwrap_err()
                .contains("module_receipt_invalid")
        );
        fs::remove_file(&path).unwrap();
        symlink(scratch.path().join("missing-receipt"), &path).unwrap();
        assert!(
            ModuleStore::selection_from(&base, ModuleComponent::Network, "canary")
                .unwrap_err()
                .contains("module_store_invalid")
        );
    }

    #[test]
    fn stage_fixture_reads_one_authority_downloads_once_and_probes_only_selected_owner() {
        for component in [
            ModuleComponent::Solar,
            ModuleComponent::Reporter,
            ModuleComponent::Network,
        ] {
            let scratch = tempfile::tempdir().unwrap();
            let store = ModuleStore::new(scratch.path(), component, "canary").unwrap();
            let probe_log = scratch.path().join("probe-count");
            let log_path = probe_log.to_str().unwrap();
            assert!(!log_path.contains('\''));
            let (mut release, original) = fixture(component, 1);
            let bytes = String::from_utf8(original)
                .unwrap()
                .replacen(
                    "#!/bin/sh\n",
                    &format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{log_path}'\n"),
                    1,
                )
                .into_bytes();
            release.assets[0].size_bytes = bytes.len() as _;
            release.assets[0].sha256 = format!("{:x}", Sha256::digest(&bytes));
            let authority_reads = Cell::new(0);
            let downloads = Cell::new(0);
            let grant = authority().acquire_with(component, &release.assets[0].sha256, &|| false, |url| {
                authority_reads.set(authority_reads.get() + 1);
                assert_eq!(url, format!("https://synthetic-module-authority.ue.gateway.dev/api/v1/app_releases?component={}&channel=canary", component.as_str()));
                Ok(serde_json::to_vec(&json!({"success":true,"release":release})).unwrap())
            }).unwrap();
            let ready = store
                .stage_with(grant, &|| false, |_, path| {
                    downloads.set(downloads.get() + 1);
                    write_new(path, &bytes)
                })
                .unwrap();
            assert_eq!(authority_reads.get(), 1);
            assert_eq!(downloads.get(), 1);
            assert_eq!(fs::read_to_string(&probe_log).unwrap().trim(), "build-info");
            assert_eq!(ready.component, component);
            assert_eq!(ready.state, "ready");
            assert!(store.selection().unwrap().is_none());
            assert!(!store.root.join("selection.json").exists());
            for unused in [
                ModuleComponent::Solar,
                ModuleComponent::Reporter,
                ModuleComponent::Network,
            ] {
                if unused != component {
                    assert!(!scratch.path().join(unused.as_str()).exists());
                }
            }
        }
    }

    #[test]
    fn activation_requires_callers_generation_as_well_as_staging_generation() {
        let base = tempfile::tempdir().unwrap();
        let store = ModuleStore::new(base.path(), ModuleComponent::Network, "canary").unwrap();
        let (release, bytes) = fixture(ModuleComponent::Network, 1);
        let ready = stage(&store, release, &bytes);
        let error = store
            .activate_at_generation(&ready, 1, ActivationGuard::default(), &|| false)
            .unwrap_err();
        assert!(error.starts_with("module_selection_changed:"));
        assert!(store.selection().unwrap().is_none());
        assert_eq!(
            store
                .activate_at_generation(&ready, 0, ActivationGuard::default(), &|| false)
                .unwrap()
                .generation,
            1
        );
        let before = std::fs::read(store.root.join("selection.json")).unwrap();
        assert!(store
            .activate_at_generation(&ready, 0, ActivationGuard::default(), &|| false)
            .unwrap_err()
            .starts_with("module_selection_changed:"));
        assert_eq!(std::fs::read(store.root.join("selection.json")).unwrap(), before);
    }

    #[test]
    fn native_three_component_stage_activate_restart_rollback_keeps_captured_old_job() {
        for component in [
            ModuleComponent::Solar,
            ModuleComponent::Reporter,
            ModuleComponent::Network,
        ] {
            let base = tempfile::tempdir().unwrap();
            let store = ModuleStore::new(base.path(), component, "canary").unwrap();
            assert!(store.selected_executable(&|| false).unwrap().is_none());
            let (one, bytes) = fixture(component, 1);
            let first = stage(&store, one, &bytes);
            let mut incomplete = first.installed.clone();
            let compiler = if component == ModuleComponent::Network {
                &mut incomplete.engine_identity["build"]["rustc"]
            } else {
                &mut incomplete.engine_identity["rustc"]
            };
            *compiler = json!("");
            assert!(module_install::validate_engine_identity(&incomplete).is_err());
            assert!(store.selection().unwrap().is_none());
            let first_selection = store
                .activate(&first, ActivationGuard::default(), &|| false)
                .unwrap();
            assert_eq!(first_selection.generation, 1);
            let (old_path, _) = store.selected_executable(&|| false).unwrap().unwrap();
            let captured = owner(component)
                .capture_for_lane_in("canary", base.path())
                .unwrap();
            let (two, bytes) = fixture(component, 2);
            let second = stage(&store, two, &bytes);
            let recovered = ModuleStore::new(base.path(), component, "canary")
                .unwrap()
                .recover_stage(second.receipt_id())
                .unwrap();
            let second_selection = store
                .activate(&recovered, ActivationGuard::default(), &|| false)
                .unwrap();
            assert_eq!(second_selection.generation, 2);
            assert!(old_path.is_file());
            let (new_path, _) = store.selected_executable(&|| false).unwrap().unwrap();
            assert_ne!(old_path, new_path);
            let old_result = captured
                .call_json("work", &[], Duration::from_secs(2))
                .unwrap();
            let new_result = owner(component)
                .capture_for_lane_in("canary", base.path())
                .unwrap()
                .call_json("work", &[], Duration::from_secs(2))
                .unwrap();
            assert_ne!(old_result, new_result);
            assert_eq!(old_result, first_selection.active.engine_identity);
            assert_eq!(new_result, second_selection.active.engine_identity);
            let rolled = store
                .rollback(2, ActivationGuard::default(), &|| false)
                .unwrap();
            assert_eq!(rolled.generation, 3);
            assert_eq!(
                store.selected_executable(&|| false).unwrap().unwrap().0,
                old_path
            );
            assert!(new_path.is_file());
            assert!(store
                .rollback(2, ActivationGuard::default(), &|| false)
                .unwrap_err()
                .contains("module_selection_changed"));
        }
    }
    #[test]
    fn removal_restart_stale_stage_reinstall_and_captured_jobs_preserve_one_authority() {
        for component in [
            ModuleComponent::Solar,
            ModuleComponent::Reporter,
            ModuleComponent::Network,
        ] {
            let base = tempfile::tempdir().unwrap();
            let store = ModuleStore::new(base.path(), component, "canary").unwrap();
            let (release, bytes) = fixture(component, 1);
            let first = stage(&store, release.clone(), &bytes);
            let installed = store
                .activate(&first, ActivationGuard::default(), &|| false)
                .unwrap();
            let v1 = fs::read(store.root.join("selection.json")).unwrap();
            assert_eq!(
                serde_json::from_slice::<ModuleSelection>(&v1).unwrap(),
                installed
            );
            assert!(matches!(
                serde_json::from_slice::<ModuleSelectionRecord>(&v1).unwrap(),
                ModuleSelectionRecord::Installed(_)
            ));
            let captured = owner(component)
                .capture_for_lane_in("canary", base.path())
                .unwrap();
            let (candidate, candidate_bytes) = fixture(component, 2);
            let stale = stage(&store, candidate, &candidate_bytes);
            let removed = store
                .remove(1, ActivationGuard::default(), &|| false)
                .unwrap();
            assert_eq!(removed.generation, 2);
            let removed_bytes = fs::read(store.root.join("selection.json")).unwrap();
            assert!(serde_json::from_slice::<ModuleSelection>(&removed_bytes).is_err());
            let restarted = ModuleStore::new(base.path(), component, "canary").unwrap();
            assert_eq!(
                restarted.selection_record().unwrap(),
                Some(ModuleSelectionRecord::Removed(removed))
            );
            assert!(restarted
                .selection()
                .unwrap_err()
                .starts_with("module_removed:"));
            assert!(restarted
                .selected_executable(&|| false)
                .unwrap_err()
                .starts_with("module_removed:"));
            assert_eq!(
                owner(component)
                    .capture_for_lane_in("canary", base.path())
                    .err()
                    .unwrap()
                    .code(),
                "module_removed"
            );
            let rollback_error = restarted
                .rollback(2, ActivationGuard::default(), &|| false)
                .unwrap_err();
            assert!(
                rollback_error.starts_with("module_removed:"),
                "{rollback_error}"
            );
            let stale = restarted.recover_stage(stale.receipt_id()).unwrap();
            assert!(restarted
                .activate(&stale, ActivationGuard::default(), &|| false)
                .unwrap_err()
                .starts_with("module_selection_changed:"));
            assert_eq!(
                fs::read(store.root.join("selection.json")).unwrap(),
                removed_bytes
            );
            assert_eq!(
                captured
                    .call_json("work", &[], Duration::from_secs(2))
                    .unwrap(),
                installed.active.engine_identity
            );
            let fresh = stage(&restarted, release, &bytes);
            assert_eq!(fresh.expected_generation, 2);
            let restored = restarted
                .activate(&fresh, ActivationGuard::default(), &|| false)
                .unwrap();
            assert_eq!(restored.generation, 3);
            assert!(restored.previous.is_none());
            assert!(owner(component)
                .capture_for_lane_in("canary", base.path())
                .is_ok());
        }
    }
    #[test]
    fn removal_admission_fences_busy_dirty_cancelled_changed_and_exhausted_generations() {
        let base = tempfile::tempdir().unwrap();
        let store = ModuleStore::new(base.path(), ModuleComponent::Network, "canary").unwrap();
        let (release, bytes) = fixture(ModuleComponent::Network, 1);
        let staged = stage(&store, release, &bytes);
        store
            .activate(&staged, ActivationGuard::default(), &|| false)
            .unwrap();
        let held = fs::read(store.root.join("selection.json")).unwrap();
        for guard in [
            ActivationGuard {
                unsaved_edits: true,
                ..Default::default()
            },
            ActivationGuard {
                uncaptured_sessions: 1,
                ..Default::default()
            },
            ActivationGuard {
                host_restart_required: true,
                ..Default::default()
            },
        ] {
            assert!(store
                .remove(1, guard, &|| false)
                .unwrap_err()
                .starts_with("module_activation_deferred:"));
        }
        assert!(store
            .remove(1, ActivationGuard::default(), &|| true)
            .unwrap_err()
            .contains("cancel"));
        assert!(store
            .remove(0, ActivationGuard::default(), &|| false)
            .unwrap_err()
            .starts_with("module_selection_changed:"));
        let lock = store.lock().unwrap();
        assert!(store
            .remove(1, ActivationGuard::default(), &|| false)
            .unwrap_err()
            .starts_with("module_update_busy:"));
        drop(lock);
        assert_eq!(fs::read(store.root.join("selection.json")).unwrap(), held);
        let checks = Cell::new(0);
        assert!(store
            .remove(1, ActivationGuard::default(), &|| {
                checks.set(checks.get() + 1);
                checks.get() == 3
            })
            .is_err());
        assert_eq!(fs::read(store.root.join("selection.json")).unwrap(), held);
        let mut installed = store.selection().unwrap().unwrap();
        installed.generation = u64::MAX;
        store
            .write_selection(&ModuleSelectionRecord::Installed(installed), &|| false)
            .unwrap();
        assert!(store
            .remove(u64::MAX, ActivationGuard::default(), &|| false)
            .unwrap_err()
            .contains("generation exhausted"));
    }
    #[test]
    fn removal_and_removed_automatic_admission_need_no_executable_authority_or_transfer() {
        let base = tempfile::tempdir().unwrap();
        let store = ModuleStore::new(base.path(), ModuleComponent::Network, "canary").unwrap();
        assert!(store.admit_automatic_acquisition().is_ok());
        let (release, bytes) = fixture(ModuleComponent::Network, 1);
        let pre_removal_grant = grant(release.clone());
        let staged = stage(&store, release, &bytes);
        store
            .activate(&staged, ActivationGuard::default(), &|| false)
            .unwrap();
        let (path, _) = store.selected_executable(&|| false).unwrap().unwrap();
        // A broken executable must not be probed or reopened by removal/admission.
        fs::write(&path, b"damaged executable").unwrap();
        store
            .remove(1, ActivationGuard::default(), &|| false)
            .unwrap();
        let reads = Cell::new(0);
        let automatic = || -> Result<(), String> {
            store.admit_automatic_acquisition()?;
            reads.set(reads.get() + 1);
            let _ = authority();
            Ok(())
        };
        assert!(automatic().unwrap_err().starts_with("module_removed:"));
        assert_eq!(reads.get(), 0);
        assert!(store
            .stage_automatic_with_admission(
                pre_removal_grant,
                |_| panic!("unexpected transfer"),
                &|| false,
                || panic!("unexpected transport")
            )
            .unwrap_err()
            .starts_with("module_removed:"));
        let stable = ModuleStore::new(base.path(), ModuleComponent::Network, "stable").unwrap();
        assert!(stable.admit_automatic_acquisition().is_ok());
        assert!(owner(ModuleComponent::Network)
            .capture_managed_for_lane_in("stable", base.path())
            .unwrap()
            .is_none());
        let solar = ModuleStore::new(base.path(), ModuleComponent::Solar, "canary").unwrap();
        assert!(solar.admit_automatic_acquisition().is_ok());
        assert!(store.admit_automatic_acquisition().is_err());
    }
    #[test]
    fn removed_record_read_and_removal_refuse_corrupt_linked_and_crossed_authority() {
        use std::os::unix::fs::symlink;
        let base = tempfile::tempdir().unwrap();
        let store = ModuleStore::new(base.path(), ModuleComponent::Network, "canary").unwrap();
        store
            .remove(0, ActivationGuard::default(), &|| false)
            .unwrap();
        let path = store.root.join("selection.json");
        let held = fs::read(&path).unwrap();
        assert!(matches!(
            ModuleStore::selection_record_from(base.path(), ModuleComponent::Network, "canary")
                .unwrap(),
            Some(ModuleSelectionRecord::Removed(_))
        ));
        for value in [
            json!({"schema":REMOVED_SELECTION_SCHEMA,"component":"network","channel":"stable","generation":1,"state":"removed"}),
            json!({"schema":REMOVED_SELECTION_SCHEMA,"component":"network","channel":"canary","generation":1,"state":"removed","active":null}),
        ] {
            let corrupted = serde_json::to_vec(&value).unwrap();
            fs::write(&path, &corrupted).unwrap();
            assert!(store
                .remove(1, ActivationGuard::default(), &|| false)
                .is_err());
            assert!(store.admit_automatic_acquisition().is_err());
            assert_eq!(fs::read(&path).unwrap(), corrupted);
        }
        let linked = base.path().join("removed-record.json");
        fs::write(&linked, &held).unwrap();
        fs::remove_file(&path).unwrap();
        symlink(&linked, &path).unwrap();
        assert!(store
            .remove(1, ActivationGuard::default(), &|| false)
            .is_err());
        assert!(store.admit_automatic_acquisition().is_err());
        assert_eq!(fs::read(&linked).unwrap(), held);
        fs::remove_file(&path).unwrap();
        fs::write(&path, &held).unwrap();
        let replaced = store.root.with_file_name("retained-canary");
        fs::rename(&store.root, &replaced).unwrap();
        symlink(&replaced, &store.root).unwrap();
        assert!(store
            .remove(1, ActivationGuard::default(), &|| false)
            .is_err());
        assert!(store.selection_record().is_err());
        assert_eq!(fs::read(replaced.join("selection.json")).unwrap(), held);
    }
    #[test]
    fn corrupt_incompatible_interrupted_cancelled_or_changed_candidate_preserves_current() {
        let base = tempfile::tempdir().unwrap();
        let store = ModuleStore::new(base.path(), ModuleComponent::Network, "canary").unwrap();
        let (one, bytes) = fixture(ModuleComponent::Network, 1);
        let first = stage(&store, one, &bytes);
        store
            .activate(&first, ActivationGuard::default(), &|| false)
            .unwrap();
        let held = fs::read(store.root.join("selection.json")).unwrap();
        let (two, bytes) = fixture(ModuleComponent::Network, 2);
        assert!(store
            .stage_with(grant(two.clone()), &|| false, |_, path| {
                write_new(path, b"corrupt")?;
                Ok(())
            })
            .is_err());
        assert_eq!(fs::read(store.root.join("selection.json")).unwrap(), held);
        assert!(store
            .stage_with(grant(two.clone()), &|| false, |_, path| {
                write_new(path, &bytes[..bytes.len() / 2])?;
                Err("synthetic interrupted transfer".into())
            })
            .is_err());
        assert_eq!(fs::read(store.root.join("selection.json")).unwrap(), held);
        let mut crossed = two.clone();
        crossed.assets[0].engines[0].manifest_sha256 = "f".repeat(64);
        assert!(store
            .stage_with(grant(crossed), &|| false, |_, path| write_new(path, &bytes))
            .unwrap_err()
            .contains("module_engine_incompatible"));
        assert_eq!(fs::read(store.root.join("selection.json")).unwrap(), held);
        let staged = stage(&store, two, &bytes);
        assert!(store
            .activate(
                &staged,
                ActivationGuard {
                    unsaved_edits: true,
                    ..Default::default()
                },
                &|| false
            )
            .unwrap_err()
            .contains("module_activation_deferred"));
        assert!(store
            .activate(
                &staged,
                ActivationGuard {
                    uncaptured_sessions: 1,
                    ..Default::default()
                },
                &|| false
            )
            .is_err());
        assert!(store
            .activate(
                &staged,
                ActivationGuard {
                    host_restart_required: true,
                    ..Default::default()
                },
                &|| false
            )
            .is_err());
        assert!(store
            .activate(&staged, ActivationGuard::default(), &|| true)
            .unwrap_err()
            .contains("module_update_cancelled"));
        assert_eq!(fs::read(store.root.join("selection.json")).unwrap(), held);
        let candidate = store.path(&staged.installed).unwrap();
        fs::write(&candidate, b"changed candidate").unwrap();
        assert!(store
            .activate(&staged, ActivationGuard::default(), &|| false)
            .is_err());
        assert_eq!(fs::read(store.root.join("selection.json")).unwrap(), held);
        assert!(store.selected_executable(&|| false).unwrap().is_some());
    }
    #[test]
    fn authority_read_is_one_fixed_request_and_refusals_launch_nothing() {
        let authority = authority();
        let (release, _) = fixture(ModuleComponent::Network, 1);
        let digest = release.assets[0].sha256.clone();
        let reads = Cell::new(0);
        let read = |url: &str| {
            reads.set(reads.get() + 1);
            assert_eq!(url,"https://synthetic-module-authority.ue.gateway.dev/api/v1/app_releases?component=network&channel=canary");
            Ok(serde_json::to_vec(&json!({"success":true,"release":release})).unwrap())
        };
        let grant = authority
            .acquire_with(ModuleComponent::Network, &digest, &|| false, read)
            .unwrap();
        assert_eq!(reads.get(), 1);
        assert_eq!(grant.release, release);
        assert!(authority
            .acquire_with(ModuleComponent::Network, "invalid", &|| false, |_| panic!(
                "invalid intent must not read"
            ))
            .is_err());
        assert!(authority
            .acquire_with(ModuleComponent::Network, &digest, &|| true, |_| panic!(
                "cancelled intent must not read"
            ))
            .is_err());
        for status in ["blocked", "stopped", "rolled_back"] {
            let mut inactive = release.clone();
            inactive.status = status.into();
            assert!(authority
                .acquire_with(ModuleComponent::Network, &digest, &|| false, |_| Ok(
                    serde_json::to_vec(&json!({"success":true,"release":inactive})).unwrap()
                ))
                .unwrap_err()
                .contains("module_release_inactive"));
        }
        assert!(authority
            .acquire_with(ModuleComponent::Network, &"f".repeat(64), &|| false, read)
            .unwrap_err()
            .contains("module_release_changed"));
        assert!(authority
            .acquire_with(ModuleComponent::Network, &digest, &|| false, |_| Ok(
                vec![b' '; MAX_RELEASE_BYTES as usize + 1]
            ))
            .is_err());
        assert!(authority
            .acquire_with(ModuleComponent::Network, &digest, &|| false, |_| Ok(
                br#"{"success":true,"release":null}"#.to_vec()
            ))
            .unwrap_err()
            .contains("module_release_unpublished"));
    }
    #[test]
    fn atomic_pointer_recovery_and_generation_fence_do_not_need_network_or_probes() {
        let base = tempfile::tempdir().unwrap();
        let store = ModuleStore::new(base.path(), ModuleComponent::Solar, "canary").unwrap();
        let (one, bytes) = fixture(ModuleComponent::Solar, 1);
        let first = stage(&store, one, &bytes);
        store
            .activate(&first, ActivationGuard::default(), &|| false)
            .unwrap();
        // Power loss before atomic replacement leaves this incomplete sibling
        // ignored. Only selection.json is ever a reader authority.
        fs::write(store.root.join("interrupted-selection.tmp"), b"{torn").unwrap();
        let restarted = ModuleStore::new(base.path(), ModuleComponent::Solar, "canary").unwrap();
        assert_eq!(restarted.selection().unwrap().unwrap().generation, 1);
        let (two, bytes) = fixture(ModuleComponent::Solar, 2);
        let second = stage(&store, two, &bytes);
        let (three, bytes) = fixture(ModuleComponent::Solar, 3);
        let third = stage(&store, three, &bytes);
        store
            .activate(&second, ActivationGuard::default(), &|| false)
            .unwrap();
        let held = fs::read(store.root.join("selection.json")).unwrap();
        let stale_error = store
            .activate(&third, ActivationGuard::default(), &|| false)
            .unwrap_err();
        assert!(stale_error.contains("module_selection_changed"), "{stale_error}");
        assert_eq!(fs::read(store.root.join("selection.json")).unwrap(), held);
        let active = store.selected_executable(&|| false).unwrap().unwrap().0;
        fs::write(&active, b"damaged active unit").unwrap();
        assert!(store.selected_executable(&|| false).is_err());
        assert_eq!(
            owner(ModuleComponent::Solar)
                .capture_for_lane_in("canary", base.path())
                .err()
                .unwrap()
                .code(),
            "module_receipt_invalid"
        );
        let recovered = store
            .rollback(2, ActivationGuard::default(), &|| false)
            .unwrap();
        assert!(recovered.previous.is_none());
        assert!(store.selected_executable(&|| false).unwrap().is_some());
    }
    #[test]
    fn linked_receipts_and_version_paths_and_crossed_component_or_lane_refuse() {
        let base = tempfile::tempdir().unwrap();
        let store = ModuleStore::new(base.path(), ModuleComponent::Reporter, "canary").unwrap();
        let (one, bytes) = fixture(ModuleComponent::Reporter, 1);
        let first = stage(&store, one, &bytes);
        store
            .activate(&first, ActivationGuard::default(), &|| false)
            .unwrap();
        let path = store.root.join("selection.json");
        let backup = store.root.join("untouched-original");
        fs::rename(&path, &backup).unwrap();
        std::os::unix::fs::symlink(&backup, &path).unwrap();
        assert!(store.selection().is_err());
        fs::remove_file(&path).unwrap();
        fs::rename(&backup, &path).unwrap();
        let (other, bytes) = fixture(ModuleComponent::Solar, 2);
        assert!(store
            .stage_with(grant(other), &|| false, |_, _| panic!(
                "crossed owner must not transfer"
            ))
            .is_err());
        let _ = bytes;
        assert!(store.recover_stage("../selection").is_err());
        let stage_path = store.receipt_path(first.receipt_id()).unwrap();
        let real = stage_path.with_extension("original");
        fs::rename(&stage_path, &real).unwrap();
        std::os::unix::fs::symlink(&real, &stage_path).unwrap();
        assert!(store.recover_stage(first.receipt_id()).is_err());
        let mut selected = store.selection().unwrap().unwrap();
        selected.channel = "stable".into();
        assert!(module_install::validate_selection(
            &selected,
            ModuleComponent::Reporter,
            "canary",
            "linux"
        )
        .is_err());
    }
    fn qualified_network_fixture(
        binary_key: &str,
        receipt_key: &str,
        version: &str,
    ) -> (ModuleRelease, Vec<u8>, Value) {
        let binary =
            PathBuf::from(std::env::var_os(binary_key).expect("qualified Network artifact"));
        let producer: Value = serde_json::from_slice(
            &fs::read(std::env::var_os(receipt_key).expect("producer receipt")).unwrap(),
        )
        .unwrap();
        let bytes = fs::read(&binary).unwrap();
        assert_eq!(producer["contract"], "ds.module-artifact/v1");
        assert_eq!(producer["component"], "network");
        assert_eq!(producer["lane"], "canary");
        assert_eq!(producer["platform"], "linux");
        assert_eq!(producer["protocol"], "ds.network-engine/v1");
        assert_eq!(producer["filename"], "ds-network");
        assert_eq!(producer["sha256"], format!("{:x}", Sha256::digest(&bytes)));
        assert_eq!(producer["size_bytes"], bytes.len());
        let mut asset = producer.as_object().unwrap().clone();
        for field in [
            "contract",
            "component",
            "lane",
            "version",
            "platform",
            "arch",
            "filename",
        ] {
            asset.remove(field);
        }
        asset.insert("platform".into(), json!("linux"));
        asset.insert("arch".into(), json!("x86_64"));
        asset.insert("kind".into(), json!("module-executable"));
        asset.insert("url".into(),json!(format!("https://storage.googleapis.com/synthetic-module-fixtures/modules/network/canary/{version}/ds-network")));
        let release=validate_module_release("network","canary",&json!({"schema":"ds.module-release/v1","component":"network","channel":"canary","product_id":"rw.datasolutions.module.network.canary","version":version,"protocol":"ds.network-engine/v1","released_at":"2026-10-14T10:00:00Z","status":"active","assets":[asset]})).unwrap();
        (release, bytes, producer)
    }
    /// Real producer bytes, isolated authority/staging. Run explicitly after
    /// supplying the owning qualified artifact/receipt; this is not a public
    /// installation or cloud publication assertion.
    #[test]
    #[ignore = "requires the explicitly selected qualified Network producer artifact"]
    fn qualified_network_artifact_runs_lv_from_native_managed_selection() {
        let (release, bytes, producer) = qualified_network_fixture(
            "DS_MODULE_TEST_NETWORK_BIN",
            "DS_MODULE_TEST_NETWORK_RECEIPT",
            "0.1.0-r1",
        );
        let base = tempfile::tempdir().unwrap();
        let store = ModuleStore::new(base.path(), ModuleComponent::Network, "canary").unwrap();
        let ready = store
            .stage_with(grant(release), &|| false, |_, path| {
                use std::os::unix::fs::OpenOptionsExt;
                let mut output = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(path)
                    .map_err(|e| e.to_string())?;
                output
                    .write_all(&bytes)
                    .and_then(|_| output.sync_all())
                    .map_err(|e| e.to_string())
            })
            .unwrap();
        let selected = store
            .activate(&ready, ActivationGuard::default(), &|| false)
            .unwrap();
        assert_eq!(
            selected.active.engine_identity["engineIdentity"]["sourceSha"],
            producer["source_sha"]
        );
        assert_eq!(
            selected.active.engine_identity["engineIdentity"]["dependencyClosureSha256"],
            producer["engines"][0]["manifest_sha256"]
        );
        let captured = ds_cli_exec::addons::NETWORK
            .capture_for_lane_in("canary", base.path())
            .unwrap();
        assert_eq!(fs::read(captured.executable()).unwrap(), bytes);
        let request = PathBuf::from(
            std::env::var_os("DS_MODULE_TEST_LV_INPUT").expect("owning immutable LV fixture"),
        );
        let output = base.path().join("lv-output");
        fs::create_dir(&output).unwrap();
        let args = [
            "--request".into(),
            request.as_os_str().to_owned(),
            "--out".into(),
            output.clone().into_os_string(),
            "--workers".into(),
            "1".into(),
        ];
        let result = captured
            .call_cancelled("lv-process", &args, Duration::from_secs(30), 1, &|| false)
            .unwrap();
        assert!(
            result.succeeded(),
            "status={:?} stdout={} stderr={}",
            result.status,
            result.stdout,
            result.stderr
        );
        let receipt: Value = serde_json::from_str(&result.stdout).unwrap();
        assert_eq!(receipt["outcome"], "completed");
        assert_eq!(
            receipt["inputSha256"],
            format!("{:x}", Sha256::digest(fs::read(&request).unwrap()))
        );
        let payload = fs::read(output.join("result.json")).unwrap();
        let expected = PathBuf::from(
            std::env::var_os("DS_MODULE_TEST_LV_EXPECTED")
                .expect("owning immutable LV result bytes"),
        );
        assert_eq!(payload, fs::read(expected).unwrap());
        assert_eq!(receipt["artifact"]["byteCount"], payload.len());
        assert_eq!(
            receipt["artifact"]["sha256"],
            format!("{:x}", Sha256::digest(&payload))
        );
        assert_eq!(
            receipt["engineIdentity"]["sourceSha"],
            producer["source_sha"]
        );
        assert_eq!(
            receipt["engineIdentity"]["dependencyClosureSha256"],
            producer["engines"][0]["manifest_sha256"]
        );
        assert!(store.selection().unwrap().unwrap().previous.is_none());
    }

    /// Two exact producer artifacts exercise retained old work, a compatible
    /// activation and rollback under private synthetic authority. No public
    /// catalog, installed product selection or unrelated engine is changed.
    #[test]
    #[ignore = "requires exact qualified Network A/B artifacts, receipts and retained host/Solar/Reporter artifacts"]
    fn qualified_network_update_keeps_held_a_runs_b_and_rolls_back_to_a() {
        let (release_a, bytes_a, producer_a) = qualified_network_fixture(
            "DS_MODULE_TEST_NETWORK_BIN_A",
            "DS_MODULE_TEST_NETWORK_RECEIPT_A",
            "0.1.0-r1",
        );
        let (release_b, bytes_b, producer_b) = qualified_network_fixture(
            "DS_MODULE_TEST_NETWORK_BIN_B",
            "DS_MODULE_TEST_NETWORK_RECEIPT_B",
            "0.1.0-r2",
        );
        assert_ne!(producer_a["sha256"], producer_b["sha256"]);
        assert_ne!(producer_a["source_sha"], producer_b["source_sha"]);
        assert_ne!(
            producer_a["engines"][0]["manifest_sha256"],
            producer_b["engines"][0]["manifest_sha256"]
        );
        let mut untouched = Vec::new();
        for key in [
            "DS_MODULE_TEST_HOST_BIN",
            "DS_MODULE_TEST_SOLAR_BIN",
            "DS_MODULE_TEST_REPORTER_BIN",
        ] {
            let path = PathBuf::from(std::env::var_os(key).expect("retained unrelated artifact"));
            let sha256 = format!("{:x}", Sha256::digest(fs::read(&path).unwrap()));
            untouched.push((key, path, sha256));
        }
        let host = std::env::current_exe().unwrap();
        untouched.push((
            "test-host",
            host.clone(),
            format!("{:x}", Sha256::digest(fs::read(host).unwrap())),
        ));
        let request = PathBuf::from(
            std::env::var_os("DS_MODULE_TEST_LV_INPUT").expect("owning immutable LV fixture"),
        );
        let expected = fs::read(
            std::env::var_os("DS_MODULE_TEST_LV_EXPECTED").expect("owning immutable LV result"),
        )
        .unwrap();
        let base = tempfile::tempdir().unwrap();
        let store = ModuleStore::new(base.path(), ModuleComponent::Network, "canary").unwrap();
        let authority_reads = Cell::new(0);
        let acquire = |release: ModuleRelease| {
            let digest = release.assets[0].sha256.clone();
            authority()
                .acquire_with(ModuleComponent::Network, &digest, &|| false, |_| {
                    authority_reads.set(authority_reads.get() + 1);
                    Ok(serde_json::to_vec(&json!({"success":true,"release":release})).unwrap())
                })
                .unwrap()
        };
        let stage_real = |release: ModuleRelease, bytes: &[u8]| {
            store
                .stage_with(acquire(release), &|| false, |_, path| {
                    use std::os::unix::fs::OpenOptionsExt;
                    let mut output = OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .mode(0o600)
                        .open(path)
                        .map_err(|e| e.to_string())?;
                    output
                        .write_all(bytes)
                        .and_then(|_| output.sync_all())
                        .map_err(|e| e.to_string())
                })
                .unwrap()
        };
        let run = |label: &str, captured: &ds_cli_exec::CapturedExternal, producer: &Value| {
            eprintln!("module-update-phase {label} begin");
            let out = base.path().join(label);
            fs::create_dir(&out).unwrap();
            let args = [
                "--request".into(),
                request.as_os_str().to_owned(),
                "--out".into(),
                out.clone().into_os_string(),
                "--workers".into(),
                "1".into(),
            ];
            let result = captured
                .call_cancelled("lv-process", &args, Duration::from_secs(30), 1, &|| false)
                .unwrap();
            assert!(result.succeeded() && !result.truncated, "{}", result.stdout);
            let receipt: Value = serde_json::from_str(&result.stdout).unwrap();
            assert_eq!(receipt["outcome"], "completed");
            assert_eq!(
                receipt["inputSha256"],
                format!("{:x}", Sha256::digest(fs::read(&request).unwrap()))
            );
            let payload = fs::read(out.join("result.json")).unwrap();
            assert_eq!(payload, expected);
            assert_eq!(receipt["artifact"]["byteCount"], payload.len());
            assert_eq!(
                receipt["artifact"]["sha256"],
                format!("{:x}", Sha256::digest(&payload))
            );
            assert_eq!(
                receipt["engineIdentity"]["sourceSha"],
                producer["source_sha"]
            );
            assert_eq!(
                receipt["engineIdentity"]["dependencyClosureSha256"],
                producer["engines"][0]["manifest_sha256"]
            );
            assert_eq!(fs::read_dir(out).unwrap().count(), 1);
            eprintln!("module-update-phase {label} end");
        };
        eprintln!("module-update-phase stage-a begin");
        let ready_a = stage_real(release_a, &bytes_a);
        eprintln!("module-update-phase stage-a end");
        eprintln!("module-update-phase activate-a begin");
        let selected_a = store
            .activate(&ready_a, ActivationGuard::default(), &|| false)
            .unwrap();
        let held_a = ds_cli_exec::addons::NETWORK
            .capture_for_lane_in("canary", base.path())
            .unwrap();
        assert_eq!(selected_a.generation, 1);
        assert_eq!(fs::read(held_a.executable()).unwrap(), bytes_a);
        assert_eq!(authority_reads.get(), 1);
        eprintln!("module-update-phase activate-a end");
        eprintln!("module-update-phase stage-b begin");
        let ready_b = stage_real(release_b, &bytes_b);
        eprintln!("module-update-phase stage-b end");
        eprintln!("module-update-phase activate-b begin");
        let selected_b = store
            .activate(&ready_b, ActivationGuard::default(), &|| false)
            .unwrap();
        let new_b = ds_cli_exec::addons::NETWORK
            .capture_for_lane_in("canary", base.path())
            .unwrap();
        assert_eq!(selected_b.generation, 2);
        assert_eq!(selected_b.previous.as_ref().unwrap(), &selected_a.active);
        assert_eq!(fs::read(new_b.executable()).unwrap(), bytes_b);
        assert_ne!(held_a.executable(), new_b.executable());
        assert_eq!(fs::read(held_a.executable()).unwrap(), bytes_a);
        assert_eq!(authority_reads.get(), 2);
        eprintln!("module-update-phase activate-b end");
        run("held-a", &held_a, &producer_a);
        run("new-b", &new_b, &producer_b);
        eprintln!("module-update-phase rollback begin");
        let rolled_back = store
            .rollback(2, ActivationGuard::default(), &|| false)
            .unwrap();
        let new_a = ds_cli_exec::addons::NETWORK
            .capture_for_lane_in("canary", base.path())
            .unwrap();
        assert_eq!(rolled_back.generation, 3);
        assert_eq!(rolled_back.active, selected_a.active);
        assert_eq!(rolled_back.previous.as_ref().unwrap(), &selected_b.active);
        assert_eq!(new_a.executable(), held_a.executable());
        assert_eq!(authority_reads.get(), 2);
        eprintln!("module-update-phase rollback end");
        run("rollback-a", &new_a, &producer_a);
        assert_eq!(fs::read(held_a.executable()).unwrap(), bytes_a);
        for (key, path, before) in &untouched {
            assert_eq!(
                &format!("{:x}", Sha256::digest(fs::read(path).unwrap())),
                before,
                "{key}"
            );
        }
        eprintln!(
            "MODULE_QUALIFIED_NETWORK_UPDATE_PROOF {}",
            json!({
                "artifact_a":producer_a,"artifact_b":producer_b,
                "authority_fixture_reads":authority_reads.get(),
                "unrelated_artifacts":untouched.iter().map(|(key,path,sha)|json!({"role":key,"path":path,"sha256":sha})).collect::<Vec<_>>(),
                "exact_lv_results":3,"held_a_preserved":true,"rollback_generation":rolled_back.generation,
                "authority":"isolated synthetic authority; no public or installed-product update claim"
            })
        );
    }
}
