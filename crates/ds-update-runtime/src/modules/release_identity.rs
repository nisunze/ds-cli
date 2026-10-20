//! Main application and DS Core release identity: one intentional read of the
//! lane's catalog member through the captured release authority, and the
//! retained proof of the installed build beside the module receipts. The
//! Kernel admits the member and decides both rows; nothing here installs,
//! downloads or activates.
use super::{
    exists_plain, ordinary_directory, read_plain, sync_directory, CapturedReleaseAuthority,
    MAX_RELEASE_BYTES,
};
use crate::transfer;
use ds_command_kernel::desktop_update::installation::{
    Application, InstallationRecord, INSTALLATION_SCHEMA, MAX_RECORD_BYTES,
};
use serde::Deserialize;
use serde_json::Value;
use std::{
    io::Write,
    path::{Path, PathBuf},
};

impl CapturedReleaseAuthority {
    /// One intentional read of this lane's main application or Core member:
    /// the raw member, or null when the lane publishes none. No redirect,
    /// retry or polling; the Kernel admits whatever arrives.
    pub fn read_application_release(&self, application: Application) -> Result<Value, String> {
        self.read_application_release_with(application, |url| self.read_release(url))
    }
    fn read_application_release_with(
        &self,
        application: Application,
        read: impl FnOnce(&str) -> Result<Vec<u8>, String>,
    ) -> Result<Value, String> {
        let query = match application {
            Application::Shell => format!("channel={}", self.channel()),
            Application::Core => format!("component=core&channel={}", self.channel()),
        };
        let bytes = read(&self.catalog_url(&query)?)?;
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
        if !envelope.success {
            return Err(
                "module_release_unavailable: the release authority refused the read".into(),
            );
        }
        Ok(envelope.release)
    }
}

/// The retained proof of one installed main application or Core build:
/// `<module base>/installations/<lane>/<shell|core>.json`, beside the module
/// receipts under the same machine-owned base.
pub struct InstallationStore {
    base: PathBuf,
    channel: &'static str,
    application: Application,
}

impl InstallationStore {
    pub fn managed(application: Application, channel: &str) -> Result<Self, String> {
        let base = ds_cli_exec::addons::managed_module_base().map_err(|e| e.to_string())?;
        Self::new(&base, application, channel)
    }
    pub fn new(base: &Path, application: Application, channel: &str) -> Result<Self, String> {
        let channel =
            match channel {
                "stable" => "stable",
                "canary" => "canary",
                _ => return Err(
                    "module_store_invalid: installation records belong to an installed public lane"
                        .into(),
                ),
            };
        if !base.is_absolute() {
            return Err("module_store_invalid: use an absolute owned base".into());
        }
        Ok(Self {
            base: base.to_owned(),
            channel,
            application,
        })
    }
    fn root(&self) -> PathBuf {
        self.base.join("installations").join(self.channel)
    }
    fn path(&self) -> PathBuf {
        self.root()
            .join(format!("{}.json", self.application.as_str()))
    }
    /// The retained record, unvalidated: the Kernel admits or ignores it.
    /// Reading creates nothing; unreadable JSON is no record, so the next
    /// proof replaces it. A linked or reparsed path refuses.
    pub fn read(&self) -> Result<Option<Value>, String> {
        let root = self.root();
        for directory in [self.base.as_path(), &self.base.join("installations"), &root] {
            if !exists_plain(directory)? {
                return Ok(None);
            }
            ordinary_directory(directory)?;
        }
        let path = self.path();
        if !exists_plain(&path)? {
            return Ok(None);
        }
        let bytes = read_plain(&path, MAX_RECORD_BYTES as u64)?;
        Ok(serde_json::from_slice(&bytes).ok())
    }
    /// Replace the record with a Kernel-minted proof in one atomic step.
    pub fn write(&self, record: &InstallationRecord) -> Result<(), String> {
        if record.schema != INSTALLATION_SCHEMA
            || record.component != self.application
            || record.channel != self.channel
        {
            return Err(
                "module_receipt_invalid: the installation record names another component or lane"
                    .into(),
            );
        }
        let bytes = serde_json::to_vec(record).map_err(|e| e.to_string())?;
        if bytes.len() > MAX_RECORD_BYTES {
            return Err("module_receipt_invalid: excessive installation record".into());
        }
        let root = transfer::ensure_owned_directory(
            &self.base,
            &Path::new("installations").join(self.channel),
        )
        .map_err(|e| format!("module_store_invalid: {e}"))?;
        let mut staged = tempfile::NamedTempFile::new_in(&root).map_err(|e| e.to_string())?;
        staged
            .write_all(&bytes)
            .and_then(|_| staged.as_file().sync_all())
            .map_err(|e| e.to_string())?;
        // One atomic replace on Unix and Windows; never a torn record.
        staged
            .persist(self.path())
            .map_err(|e| format!("module_store_invalid: {}", e.error))?;
        sync_directory(&root)
    }
}

#[cfg(test)]
#[path = "release_identity_tests.rs"]
mod tests;
