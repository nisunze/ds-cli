//! Protected host state: leased refresh bytes and UID/audience-fenced context.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[cfg(test)]
use std::sync::atomic::{AtomicU64, Ordering};

use ds_cli_contract::Failure;
use ds_client_core::{
    ClientProfile, Project, ProjectStatus, RefreshTokenStore, StoreError, StoreKey,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::Zeroize;

const MAX_STATE_BYTES: u64 = 64 * 1024;
const CONTEXT_SCHEMA: &str = "ds-cli.project-context/v1";
const LOCK_WAIT: Duration = Duration::from_secs(5);
const LOCK_POLL: Duration = Duration::from_millis(25);
#[cfg(test)]
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(1);

pub struct NativeRefreshStore {
    root: PathBuf,
    lease: Option<(String, File)>,
}

impl NativeRefreshStore {
    pub fn open() -> Result<Self, Failure> {
        let root = state_root()?.join("credentials");
        secure_dir(&root).map_err(state_failure)?;
        Ok(Self { root, lease: None })
    }

    fn paths(&self, key: &StoreKey) -> (String, PathBuf, PathBuf) {
        let identity = format!("{:x}", Sha256::digest(key.as_str().as_bytes()));
        (
            identity.clone(),
            self.root.join(format!("{identity}.json")),
            self.root.join(format!("{identity}.lock")),
        )
    }

    fn prove_lease(&self, key: &StoreKey) -> Result<PathBuf, StoreError> {
        let (identity, path, _) = self.paths(key);
        if self.lease.as_ref().map(|lease| lease.0.as_str()) != Some(identity.as_str()) {
            return Err(StoreError::Conflict);
        }
        Ok(path)
    }
}

impl Drop for NativeRefreshStore {
    fn drop(&mut self) {
        if let Some((_, file)) = self.lease.take() {
            let _ = unlock(&file);
        }
    }
}

impl RefreshTokenStore for NativeRefreshStore {
    fn acquire(&mut self, key: &StoreKey) -> Result<(), StoreError> {
        if self.lease.is_some() {
            return Err(StoreError::Conflict);
        }
        let (identity, _, lock_path) = self.paths(key);
        let file = protected_open_lock(&lock_path)?;
        lock_exclusive(&file)?;
        let (_, data_path, _) = self.paths(key);
        if let Err(error) = cleanup_stage(&data_path) {
            let _ = unlock(&file);
            return Err(error);
        }
        self.lease = Some((identity, file));
        Ok(())
    }

    fn load(&mut self, key: &StoreKey) -> Result<Option<Vec<u8>>, StoreError> {
        let path = self.prove_lease(key)?;
        protected_read(&path)
    }

    fn compare_and_swap(
        &mut self,
        key: &StoreKey,
        expected: Option<&[u8]>,
        replacement: Option<&[u8]>,
    ) -> Result<(), StoreError> {
        let path = self.prove_lease(key)?;
        let mut current = protected_read(&path)?;
        if current.as_deref() != expected {
            if let Some(bytes) = current.as_mut() {
                bytes.zeroize();
            }
            return Err(StoreError::Conflict);
        }
        let result = match replacement {
            Some(bytes) if bytes.len() as u64 <= MAX_STATE_BYTES => atomic_write(&path, bytes),
            Some(_) => Err(StoreError::UnsafeOrUnreadable),
            None => remove_and_sync(&path),
        };
        if let Some(bytes) = current.as_mut() {
            bytes.zeroize();
        }
        result
    }

    fn release(&mut self, key: &StoreKey) -> Result<(), StoreError> {
        let (identity, _, _) = self.paths(key);
        let Some((held, file)) = self.lease.take() else {
            return Err(StoreError::Conflict);
        };
        if held != identity {
            self.lease = Some((held, file));
            return Err(StoreError::Conflict);
        }
        unlock(&file)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProjectContext {
    schema: String,
    lane: String,
    credential_audience_sha256: String,
    uid: String,
    email: String,
    ds_project: String,
    project_name: String,
    display_name: Option<String>,
    role: Option<String>,
    status: String,
}

/// Crash-released serialization fence shared by login/logout/project use.
pub struct ProjectContextLease {
    path: PathBuf,
    lock: File,
}

impl ProjectContextLease {
    pub fn acquire(profile: &ClientProfile) -> Result<Self, Failure> {
        let (path, lock_path) = context_paths(profile)?;
        let lock = protected_open_lock(&lock_path).map_err(state_failure)?;
        lock_exclusive(&lock).map_err(state_failure)?;
        if let Err(error) = cleanup_stage(&path) {
            let _ = unlock(&lock);
            return Err(state_failure(error));
        }
        Ok(Self { path, lock })
    }

    pub fn save(
        &self,
        profile: &ClientProfile,
        uid: &str,
        email: &str,
        project: &Project,
    ) -> Result<ProjectContext, Failure> {
        let context = ProjectContext::from_project(profile, uid, email, project);
        let bytes = serde_json::to_vec(&context)
            .map_err(|_| state_failure(StoreError::UnsafeOrUnreadable))?;
        atomic_write(&self.path, &bytes).map_err(state_failure)?;
        Ok(context)
    }

    pub fn load(
        &self,
        profile: &ClientProfile,
        uid: &str,
        email: &str,
    ) -> Result<Option<ProjectContext>, Failure> {
        let Some(bytes) = protected_read(&self.path).map_err(state_failure)? else {
            return Ok(None);
        };
        let context: ProjectContext = serde_json::from_slice(&bytes)
            .map_err(|_| state_failure(StoreError::UnsafeOrUnreadable))?;
        if !context.matches(profile, uid, email) {
            return Err(Failure::conflict(
                "project_context_stale",
                "the saved project context belongs to another user, lane, or client audience",
            )
            .remedy("run ds auth project use with an exact visible project id"));
        }
        Ok(Some(context))
    }

    /// Load one identity- and audience-fenced context snapshot, then release
    /// the serialization lease before the caller begins remote work.
    pub fn load_snapshot(
        self,
        profile: &ClientProfile,
        uid: &str,
        email: &str,
    ) -> Result<Option<ProjectContext>, Failure> {
        let selected = self.load(profile, uid, email)?;
        drop(self);
        Ok(selected)
    }

    pub fn clear(&self) -> Result<(), Failure> {
        remove_and_sync(&self.path).map_err(state_failure)
    }

    /// Clear only the exact context an operation previously loaded. A native
    /// request may release this lease while it is in flight; a concurrent
    /// project selection must not then be removed by the older request's
    /// revoked-identity cleanup.
    pub fn clear_if_unchanged(&self, expected: &ProjectContext) -> Result<(), Failure> {
        let Some(bytes) = protected_read(&self.path).map_err(state_failure)? else {
            return Ok(());
        };
        let current: ProjectContext = serde_json::from_slice(&bytes)
            .map_err(|_| state_failure(StoreError::UnsafeOrUnreadable))?;
        if &current == expected {
            remove_and_sync(&self.path).map_err(state_failure)?;
        }
        Ok(())
    }
}

impl Drop for ProjectContextLease {
    fn drop(&mut self) {
        let _ = unlock(&self.lock);
    }
}

impl ProjectContext {
    pub fn from_project(
        profile: &ClientProfile,
        uid: &str,
        email: &str,
        project: &Project,
    ) -> Self {
        Self {
            schema: CONTEXT_SCHEMA.to_owned(),
            lane: profile.lane().token().to_owned(),
            credential_audience_sha256: profile.credential_audience_sha256().to_owned(),
            uid: uid.to_owned(),
            email: email.to_owned(),
            ds_project: project.ds_project().to_owned(),
            project_name: project.project_name().to_owned(),
            display_name: project.display_name().map(str::to_owned),
            role: project.role().map(str::to_owned),
            status: status_token(project.status()).to_owned(),
        }
    }

    pub fn project_id(&self) -> &str {
        &self.ds_project
    }

    pub fn project_name(&self) -> &str {
        &self.project_name
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    fn matches(&self, profile: &ClientProfile, uid: &str, email: &str) -> bool {
        self.schema == CONTEXT_SCHEMA
            && self.lane == profile.lane().token()
            && self.credential_audience_sha256 == profile.credential_audience_sha256()
            && self.uid == uid
            && self.email == email
    }
}

fn context_paths(profile: &ClientProfile) -> Result<(PathBuf, PathBuf), Failure> {
    let root = state_root()?.join("contexts");
    secure_dir(&root).map_err(state_failure)?;
    let identity = format!(
        "{}-{}.json",
        profile.lane().token(),
        profile.credential_audience_sha256()
    );
    let path = root.join(&identity);
    let lock = root.join(format!("{identity}.lock"));
    Ok((path, lock))
}

fn state_root() -> Result<PathBuf, Failure> {
    #[cfg(windows)]
    {
        return Err(Failure::unavailable(
            "native_state_protection_unavailable",
            "this Windows build has no DPAPI-backed native credential adapter",
        )
        .remedy("use a ds build that includes the Windows protected-state adapter"));
    }
    #[cfg(not(windows))]
    {
        let base = if let Some(path) = std::env::var_os("DS_CONFIG_HOME").filter(|v| !v.is_empty())
        {
            let path = PathBuf::from(path);
            if !path.is_absolute() {
                return Err(Failure::invalid(
                    "native_state_root_invalid",
                    "DS_CONFIG_HOME must be an absolute path",
                ));
            }
            path
        } else if let Some(path) = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
            let path = PathBuf::from(path);
            if !path.is_absolute() {
                return Err(Failure::invalid(
                    "native_state_root_invalid",
                    "XDG_CONFIG_HOME must be an absolute path",
                ));
            }
            path
        } else {
            std::env::var_os("HOME")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .map(|path| path.join(".config"))
                .ok_or_else(|| {
                    Failure::unavailable(
                        "native_state_unavailable",
                        "the absolute per-user config root cannot be resolved",
                    )
                })?
        };
        let root = base.join("ds");
        secure_dir(&root).map_err(state_failure)?;
        Ok(root)
    }
}

fn status_token(status: ProjectStatus) -> &'static str {
    match status {
        ProjectStatus::Active => "active",
        ProjectStatus::Archived => "archived",
        ProjectStatus::Testing => "testing",
    }
}

fn state_failure(error: StoreError) -> Failure {
    let code = match error {
        StoreError::Conflict => "native_state_conflict",
        StoreError::Unavailable => "native_state_unavailable",
        StoreError::UnsafeOrUnreadable => "native_state_unsafe",
    };
    let failure = if error == StoreError::Conflict {
        Failure::conflict(
            code,
            "another native auth operation still holds the protected state lease",
        )
    } else {
        Failure::unavailable(
            code,
            "the protected native auth state could not be safely accessed",
        )
    };
    failure.remedy("check that the per-user DS config directory is owner-only, contains no symlinks, and has free space")
}

fn protected_read(path: &Path) -> Result<Option<Vec<u8>>, StoreError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(StoreError::Unavailable),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > MAX_STATE_BYTES
    {
        return Err(StoreError::UnsafeOrUnreadable);
    }
    validate_file_metadata(&metadata)?;
    let file = protected_open_read(path)?;
    let opened = file.metadata().map_err(|_| StoreError::Unavailable)?;
    if !opened.is_file() || opened.len() > MAX_STATE_BYTES {
        return Err(StoreError::UnsafeOrUnreadable);
    }
    validate_file_metadata(&opened)?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    if file
        .take(MAX_STATE_BYTES + 1)
        .read_to_end(&mut bytes)
        .is_err()
    {
        bytes.zeroize();
        return Err(StoreError::Unavailable);
    }
    if bytes.len() as u64 > MAX_STATE_BYTES {
        bytes.zeroize();
        return Err(StoreError::UnsafeOrUnreadable);
    }
    Ok(Some(bytes))
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    if bytes.len() as u64 > MAX_STATE_BYTES {
        return Err(StoreError::UnsafeOrUnreadable);
    }
    let parent = path.parent().ok_or(StoreError::UnsafeOrUnreadable)?;
    secure_dir(parent)?;
    if path.exists() {
        let mut existing = protected_read(path)?;
        if let Some(bytes) = existing.as_mut() {
            bytes.zeroize();
        }
    }
    let temp = stage_path(path)?;
    let mut file = protected_create_new(&temp)?;
    let result = (|| {
        file.write_all(bytes).map_err(|_| StoreError::Unavailable)?;
        file.sync_all().map_err(|_| StoreError::Unavailable)?;
        fs::rename(&temp, path).map_err(|_| StoreError::Unavailable)?;
        sync_dir(parent)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn stage_path(path: &Path) -> Result<PathBuf, StoreError> {
    let parent = path.parent().ok_or(StoreError::UnsafeOrUnreadable)?;
    let leaf = path
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .ok_or(StoreError::UnsafeOrUnreadable)?;
    Ok(parent.join(format!(".{leaf}.stage")))
}

/// Remove only this destination's crash-left stage while its caller holds the
/// matching lease. Unsafe stages are refused and left untouched.
fn cleanup_stage(path: &Path) -> Result<(), StoreError> {
    remove_and_sync(&stage_path(path)?)
}

fn remove_and_sync(path: &Path) -> Result<(), StoreError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(StoreError::UnsafeOrUnreadable);
            }
            validate_file_metadata(&metadata)?;
            fs::remove_file(path).map_err(|_| StoreError::Unavailable)?;
            sync_dir(path.parent().ok_or(StoreError::UnsafeOrUnreadable)?)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(StoreError::Unavailable),
    }
}

#[cfg(unix)]
fn secure_dir(path: &Path) -> Result<(), StoreError> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink()
                || !metadata.is_dir()
                || metadata.uid() != unsafe { libc::geteuid() }
                || metadata.permissions().mode() & 0o077 != 0
            {
                return Err(StoreError::UnsafeOrUnreadable);
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let parent = path.parent().ok_or(StoreError::UnsafeOrUnreadable)?;
            if !parent.exists() {
                fs::create_dir_all(parent).map_err(|_| StoreError::Unavailable)?;
            }
            let mut builder = fs::DirBuilder::new();
            builder.mode(0o700);
            match builder.create(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    return secure_dir(path);
                }
                Err(_) => return Err(StoreError::Unavailable),
            }
        }
        Err(_) => return Err(StoreError::Unavailable),
    }
    Ok(())
}

#[cfg(windows)]
fn secure_dir(_path: &Path) -> Result<(), StoreError> {
    Err(StoreError::Unavailable)
}

#[cfg(unix)]
fn protected_create_new(path: &Path) -> Result<File, StoreError> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| StoreError::Unavailable)
}

#[cfg(windows)]
fn protected_create_new(_path: &Path) -> Result<File, StoreError> {
    Err(StoreError::Unavailable)
}

#[cfg(unix)]
fn protected_open_read(path: &Path) -> Result<File, StoreError> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| StoreError::Unavailable)
}

#[cfg(windows)]
fn protected_open_read(_path: &Path) -> Result<File, StoreError> {
    Err(StoreError::Unavailable)
}

#[cfg(unix)]
fn protected_open_lock(path: &Path) -> Result<File, StoreError> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| StoreError::Unavailable)?;
    let metadata = file.metadata().map_err(|_| StoreError::Unavailable)?;
    if !metadata.is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.permissions().mode() & 0o077 != 0
        || metadata.nlink() != 1
    {
        return Err(StoreError::UnsafeOrUnreadable);
    }
    Ok(file)
}

#[cfg(windows)]
fn protected_open_lock(_path: &Path) -> Result<File, StoreError> {
    Err(StoreError::Unavailable)
}

#[cfg(unix)]
fn validate_file_metadata(metadata: &fs::Metadata) -> Result<(), StoreError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    if metadata.uid() != unsafe { libc::geteuid() }
        || metadata.permissions().mode() & 0o077 != 0
        || metadata.nlink() != 1
    {
        Err(StoreError::UnsafeOrUnreadable)
    } else {
        Ok(())
    }
}

#[cfg(windows)]
fn validate_file_metadata(_metadata: &fs::Metadata) -> Result<(), StoreError> {
    Err(StoreError::Unavailable)
}

#[cfg(unix)]
fn lock_exclusive(file: &File) -> Result<(), StoreError> {
    lock_exclusive_for(file, LOCK_WAIT, LOCK_POLL)
}

#[cfg(unix)]
fn lock_exclusive_for(file: &File, wait: Duration, poll: Duration) -> Result<(), StoreError> {
    use std::os::fd::AsRawFd;
    let deadline = Instant::now() + wait;
    loop {
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        if error.kind() != std::io::ErrorKind::WouldBlock {
            return Err(StoreError::Unavailable);
        }
        if Instant::now() >= deadline {
            return Err(StoreError::Conflict);
        }
        std::thread::sleep(poll);
    }
}

#[cfg(windows)]
fn lock_exclusive(_file: &File) -> Result<(), StoreError> {
    Err(StoreError::Unavailable)
}

#[cfg(unix)]
fn unlock(file: &File) -> Result<(), StoreError> {
    use std::os::fd::AsRawFd;
    loop {
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) } == 0 {
            return Ok(());
        }
        if std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            return Err(StoreError::Unavailable);
        }
    }
}

#[cfg(windows)]
fn unlock(_file: &File) -> Result<(), StoreError> {
    Err(StoreError::Unavailable)
}

fn sync_dir(path: &Path) -> Result<(), StoreError> {
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|_| StoreError::Unavailable)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use ds_client_core::{
        CLIENT_PROFILE_SCHEMA, Client, ClientProfileInput, DeploymentLane, ProjectFormEditorCall,
        ProjectFormsCall, ProjectListCall, RefreshCall, SignInCall, SolarSnapshotCall,
        SurveyQueryCall, TransformerContextCall, Transport, TransportError, TransportResponse,
    };
    use std::os::unix::fs::{PermissionsExt, symlink};

    const NOW: u64 = 1_900_000_000;
    const SIGN_IN: &[u8] = include_bytes!(
        "../../../../ds-web/crates/ds-client-core/tests/fixtures/firebase-sign-in.json"
    );
    const REFRESH: &[u8] = include_bytes!(
        "../../../../ds-web/crates/ds-client-core/tests/fixtures/firebase-refresh.json"
    );

    #[derive(Default)]
    struct FixtureTransport {
        sign_in: Option<Vec<u8>>,
        refresh: Option<Vec<u8>>,
    }

    impl Transport for FixtureTransport {
        fn sign_in(&mut self, _call: SignInCall<'_>) -> Result<TransportResponse, TransportError> {
            self.sign_in
                .take()
                .map(|body| TransportResponse::new(200, body))
                .ok_or(TransportError::Unreachable)
        }

        fn refresh(&mut self, _call: RefreshCall<'_>) -> Result<TransportResponse, TransportError> {
            self.refresh
                .take()
                .map(|body| TransportResponse::new(200, body))
                .ok_or(TransportError::Unreachable)
        }

        fn list_projects(
            &mut self,
            _call: ProjectListCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            Err(TransportError::Unreachable)
        }

        fn transformer_context(
            &mut self,
            _call: TransformerContextCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            Err(TransportError::Unreachable)
        }

        fn project_forms(
            &mut self,
            _call: ProjectFormsCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            Err(TransportError::Unreachable)
        }

        fn project_form_editor(
            &mut self,
            _call: ProjectFormEditorCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            Err(TransportError::Unreachable)
        }

        fn solar_snapshot(
            &mut self,
            _call: SolarSnapshotCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            Err(TransportError::Unreachable)
        }

        fn survey_query(
            &mut self,
            _call: SurveyQueryCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            Err(TransportError::Unreachable)
        }
    }

    fn profile() -> ClientProfile {
        ClientProfile::validate(ClientProfileInput {
            schema_version: CLIENT_PROFILE_SCHEMA.to_owned(),
            lane: DeploymentLane::Stable,
            source_revision: "0123456789abcdef0123456789abcdef01234567".to_owned(),
            descriptor_sha256: "a".repeat(64),
            firebase_project_id: "firebase-project".to_owned(),
            firebase_api_key: "public-firebase-key".to_owned(),
            gateway_api_key: "public-gateway-key".to_owned(),
            gateway_origin: "https://eds-gateway-3c0q477h.ue.gateway.dev".to_owned(),
            project_list_method: "GET".to_owned(),
            project_list_path: "/api/v1/user/projects".to_owned(),
            transformer_context_method: "POST".to_owned(),
            transformer_context_path: "/api/v1/data".to_owned(),
            transformer_context_action: "get_transformers_data".to_owned(),
            transformer_context_fields: "context".to_owned(),
            project_forms_method: "POST".to_owned(),
            project_forms_path: "/api/v1/project-forms".to_owned(),
            project_forms_action: "activate".to_owned(),
            project_form_editor_action: "settings_editor".to_owned(),
            solar_snapshot_method: "POST".to_owned(),
            solar_snapshot_path: "/api/v1/solar".to_owned(),
            solar_snapshot_action: "desktop_snapshot".to_owned(),
            survey_query_method: "POST".to_owned(),
            survey_query_path: "/api/v1/survey/query".to_owned(),
        })
        .unwrap()
    }

    fn store(root: &Path) -> NativeRefreshStore {
        NativeRefreshStore {
            root: root.to_owned(),
            lease: None,
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "ds-auth-state-{name}-{}-{}",
            std::process::id(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        path
    }

    #[test]
    fn second_lease_times_out_as_conflict_and_first_survives() {
        let root = temp_dir("lease");
        let lock_path = root.join("state.lock");
        let first = protected_open_lock(&lock_path).unwrap();
        let second = protected_open_lock(&lock_path).unwrap();
        lock_exclusive(&first).unwrap();
        let started = Instant::now();
        assert_eq!(
            lock_exclusive_for(&second, Duration::from_millis(75), Duration::from_millis(5)),
            Err(StoreError::Conflict)
        );
        assert!(started.elapsed() >= Duration::from_millis(70));
        unlock(&first).unwrap();
        lock_exclusive_for(&second, Duration::from_millis(20), Duration::from_millis(2)).unwrap();
        unlock(&second).unwrap();
        assert!(lock_path.exists());
        fs::remove_file(lock_path).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn delayed_cleanup_never_clears_a_concurrent_context_replacement() {
        let root = temp_dir("conditional-context-clear");
        let path = root.join("context.json");
        let lock_path = root.join("context.lock");
        let lock = protected_open_lock(&lock_path).unwrap();
        lock_exclusive(&lock).unwrap();
        let lease = ProjectContextLease {
            path: path.clone(),
            lock,
        };
        let expected = ProjectContext {
            schema: CONTEXT_SCHEMA.to_owned(),
            lane: "stable".to_owned(),
            credential_audience_sha256: "a".repeat(64),
            uid: "uid-a".to_owned(),
            email: "operator@example.com".to_owned(),
            ds_project: "project-a".to_owned(),
            project_name: "Project A".to_owned(),
            display_name: None,
            role: Some("owner".to_owned()),
            status: "active".to_owned(),
        };
        let mut replacement = expected.clone();
        replacement.ds_project = "project-b".to_owned();
        replacement.project_name = "Project B".to_owned();

        atomic_write(&path, &serde_json::to_vec(&replacement).unwrap()).unwrap();
        lease.clear_if_unchanged(&expected).unwrap();
        let retained: ProjectContext =
            serde_json::from_slice(&protected_read(&path).unwrap().unwrap()).unwrap();
        assert_eq!(retained, replacement);

        lease.clear_if_unchanged(&replacement).unwrap();
        assert!(!path.exists());
        drop(lease);
        fs::remove_file(lock_path).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn selected_context_snapshot_releases_lease_before_network_phase() {
        let root = temp_dir("released-read-lease");
        let path = root.join("context.json");
        let lock_path = root.join("context.lock");
        let profile = profile();
        let expected = ProjectContext {
            schema: CONTEXT_SCHEMA.to_owned(),
            lane: profile.lane().token().to_owned(),
            credential_audience_sha256: profile.credential_audience_sha256().to_owned(),
            uid: "uid-a".to_owned(),
            email: "operator@example.com".to_owned(),
            ds_project: "project-a".to_owned(),
            project_name: "Project A".to_owned(),
            display_name: None,
            role: Some("owner".to_owned()),
            status: "active".to_owned(),
        };
        atomic_write(&path, &serde_json::to_vec(&expected).unwrap()).unwrap();
        let lock = protected_open_lock(&lock_path).unwrap();
        lock_exclusive(&lock).unwrap();
        let lease = ProjectContextLease {
            path: path.clone(),
            lock,
        };

        let selected = lease
            .load_snapshot(&profile, "uid-a", "operator@example.com")
            .unwrap()
            .unwrap();
        assert_eq!(selected.project_id(), "project-a");

        // This acquisition represents an unrelated context operation starting
        // while the selected-project network read is in flight.
        let during_network = protected_open_lock(&lock_path).unwrap();
        lock_exclusive_for(
            &during_network,
            Duration::from_millis(20),
            Duration::from_millis(2),
        )
        .unwrap();
        unlock(&during_network).unwrap();

        fs::remove_file(path).unwrap();
        fs::remove_file(lock_path).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn stale_context_snapshot_is_refused_and_retained() {
        let root = temp_dir("stale-read-context");
        let path = root.join("context.json");
        let lock_path = root.join("context.lock");
        let profile = profile();
        let stale = ProjectContext {
            schema: CONTEXT_SCHEMA.to_owned(),
            lane: profile.lane().token().to_owned(),
            credential_audience_sha256: "f".repeat(64),
            uid: "uid-a".to_owned(),
            email: "operator@example.com".to_owned(),
            ds_project: "project-a".to_owned(),
            project_name: "Project A".to_owned(),
            display_name: None,
            role: Some("owner".to_owned()),
            status: "active".to_owned(),
        };
        let stale_bytes = serde_json::to_vec(&stale).unwrap();
        atomic_write(&path, &stale_bytes).unwrap();
        let lock = protected_open_lock(&lock_path).unwrap();
        lock_exclusive(&lock).unwrap();
        let lease = ProjectContextLease {
            path: path.clone(),
            lock,
        };

        assert_eq!(
            lease
                .load_snapshot(&profile, "uid-a", "operator@example.com")
                .unwrap_err()
                .code(),
            "project_context_stale"
        );
        assert_eq!(protected_read(&path).unwrap().unwrap(), stale_bytes);

        let after_refusal = protected_open_lock(&lock_path).unwrap();
        lock_exclusive_for(
            &after_refusal,
            Duration::from_millis(20),
            Duration::from_millis(2),
        )
        .unwrap();
        unlock(&after_refusal).unwrap();

        fs::remove_file(path).unwrap();
        fs::remove_file(lock_path).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn final_symlink_and_hardlink_targets_are_refused_untouched() {
        let root = temp_dir("links");
        let target = root.join("target");
        fs::write(&target, b"credential-marker").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
        let link = root.join("link");
        symlink(&target, &link).unwrap();
        assert_eq!(protected_read(&link), Err(StoreError::UnsafeOrUnreadable));
        let hard = root.join("hard");
        fs::hard_link(&target, &hard).unwrap();
        assert_eq!(protected_read(&target), Err(StoreError::UnsafeOrUnreadable));
        assert_eq!(fs::read(&target).unwrap(), b"credential-marker");
        fs::remove_file(link).unwrap();
        fs::remove_file(hard).unwrap();
        fs::remove_file(target).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn atomic_state_is_owner_only_and_oversize_is_refused() {
        let root = temp_dir("atomic");
        let state = root.join("state.json");
        atomic_write(&state, b"bounded").unwrap();
        let metadata = fs::metadata(&state).unwrap();
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        assert_eq!(protected_read(&state).unwrap().unwrap(), b"bounded");
        assert_eq!(
            atomic_write(&state, &vec![0; MAX_STATE_BYTES as usize + 1]),
            Err(StoreError::UnsafeOrUnreadable)
        );
        assert_eq!(fs::read(&state).unwrap(), b"bounded");
        fs::remove_file(state).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn exact_crash_stage_is_cleaned_and_hostile_stage_is_refused() {
        let root = temp_dir("stage");
        let state = root.join("credential.json");
        atomic_write(&state, b"durable").unwrap();
        let stage = stage_path(&state).unwrap();

        fs::write(&stage, b"crash-left-refresh").unwrap();
        fs::set_permissions(&stage, fs::Permissions::from_mode(0o600)).unwrap();
        cleanup_stage(&state).unwrap();
        assert!(!stage.exists());
        assert_eq!(fs::read(&state).unwrap(), b"durable");

        let victim = root.join("victim");
        fs::write(&victim, b"untouched").unwrap();
        fs::set_permissions(&victim, fs::Permissions::from_mode(0o600)).unwrap();
        symlink(&victim, &stage).unwrap();
        assert_eq!(cleanup_stage(&state), Err(StoreError::UnsafeOrUnreadable));
        assert_eq!(fs::read(&victim).unwrap(), b"untouched");

        fs::remove_file(stage).unwrap();
        fs::remove_file(victim).unwrap();
        fs::remove_file(state).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn real_file_adapter_round_trips_core_rotation_and_logout() {
        let root = temp_dir("core-roundtrip");
        let mut signed_in = Client::new(
            profile(),
            FixtureTransport {
                sign_in: Some(SIGN_IN.to_vec()),
                refresh: None,
            },
            store(&root),
        );
        let user = signed_in
            .sign_in("user@example.com", "protected-password", NOW)
            .unwrap();
        assert_eq!(user.uid(), "uid-1");
        drop(signed_in);

        let mut restored = Client::new(
            profile(),
            FixtureTransport {
                sign_in: None,
                refresh: Some(REFRESH.to_vec()),
            },
            store(&root),
        );
        assert_eq!(restored.restore(NOW + 1).unwrap().unwrap().uid(), "uid-1");
        restored.sign_out().unwrap();
        drop(restored);

        let mut signed_out = Client::new(profile(), FixtureTransport::default(), store(&root));
        assert!(signed_out.restore(NOW + 2).unwrap().is_none());
        drop(signed_out);

        for entry in fs::read_dir(&root).unwrap() {
            let path = entry.unwrap().path();
            assert!(path.extension().is_some_and(|value| value == "lock"));
            fs::remove_file(path).unwrap();
        }
        fs::remove_dir(root).unwrap();
    }
}
