//! Explicit portable private backup output. Scope/digest verification stays in
//! ds-client-core; this host only admits a new path and writes checked bytes.
use ds_cli_contract::Failure;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
/// Existing protected native probes only; no project selection or cloud read.
pub struct Captured {
    lane: String,
    identity: crate::ProviderIdentity,
    credential: String,
}
impl Captured {
    pub fn capture(lane: &str) -> Result<Self, Failure> {
        Ok(Self {
            lane: lane.into(),
            identity: crate::headless_identity_for_named_project(lane)?,
            credential: crate::runtime_credential_binding(lane)?,
        })
    }
    pub fn verify(&self) -> Result<(), Failure> {
        self.verify_observation(
            &crate::headless_identity_for_named_project(&self.lane)?,
            &crate::runtime_credential_binding(&self.lane)?,
        )
    }
    fn verify_observation(
        &self,
        identity: &crate::ProviderIdentity,
        credential: &str,
    ) -> Result<(), Failure> {
        if identity != &self.identity || credential != self.credential {
            return Err(Failure::conflict(
                "auth_context_mismatch",
                "Private backup account or credential changed before output.",
            )
            .remedy("Repeat the read under the intended native account and lane."));
        }
        Ok(())
    }
}
fn invalid(message: impl Into<String>) -> Failure {
    Failure::invalid("notes_request_invalid", message)
        .remedy("Choose one new absolute .json path in an existing real directory.")
}
pub fn admit(raw: &str) -> Result<PathBuf, Failure> {
    let path = PathBuf::from(raw);
    if raw.is_empty()
        || raw.len() > 4096
        || raw.chars().any(char::is_control)
        || !path.is_absolute()
        || path.extension().and_then(|e| e.to_str()) != Some("json")
    {
        return Err(invalid("Backup export requires an absolute .json path."));
    }
    match fs::symlink_metadata(&path) {
        Ok(_) => {
            return Err(Failure::conflict(
                "notes_output_exists",
                "The selected output exists; it was preserved.",
            )
            .remedy("Choose a new backup output path."));
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(e) => return Err(invalid(e.to_string())),
    }
    let parent = path
        .parent()
        .ok_or_else(|| invalid("Output has no parent."))?;
    let metadata = fs::symlink_metadata(parent).map_err(|e| invalid(e.to_string()))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(invalid("Output parent must be an existing real directory."));
    }
    Ok(path)
}
pub fn write(path: &Path, bytes: &[u8]) -> Result<(), Failure> {
    if bytes.is_empty() || bytes.len() > 512 << 10 {
        return Err(invalid("Checked backup bytes must be within 512 KiB."));
    }
    // Existing private IO owner supplies Unix 0600. create_new is the final
    // no-overwrite fence after acquisition, even if another writer raced admit.
    let mut file = ds_layer_store::private::open_options()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                Failure::conflict(
                    "notes_output_exists",
                    "Output appeared before export; it was preserved.",
                )
                .remedy("Choose a new backup output path.")
            } else {
                invalid(e.to_string())
            }
        })?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| invalid(e.to_string()))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn export_fence_refuses_account_lane_audience_or_credential_switch() {
        let digest = "a".repeat(64);
        let identity = crate::ProviderIdentity::new("stable", &digest, "owner").unwrap();
        let fence = Captured {
            lane: "stable".into(),
            identity: identity.clone(),
            credential: "instance-1".into(),
        };
        assert!(fence.verify_observation(&identity, "instance-1").is_ok());
        assert!(fence.verify_observation(&identity, "instance-2").is_err());
        for other in [
            crate::ProviderIdentity::new("canary", &digest, "owner").unwrap(),
            crate::ProviderIdentity::new("stable", &"b".repeat(64), "owner").unwrap(),
            crate::ProviderIdentity::new("stable", &digest, "other").unwrap(),
        ] {
            assert!(fence.verify_observation(&other, "instance-1").is_err());
        }
    }
    #[test]
    fn admitted_export_is_exact_private_and_never_overwrites() {
        let path = std::env::temp_dir().join(format!(
            "ds-note-backup-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let admitted = admit(path.to_str().unwrap()).unwrap();
        let exact = b"{\"schema\":\"x\"}";
        write(&admitted, exact).unwrap();
        assert_eq!(fs::read(&path).unwrap(), exact);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        assert_eq!(
            admit(path.to_str().unwrap()).unwrap_err().code(),
            "notes_output_exists"
        );
        assert_eq!(
            write(&path, b"different").unwrap_err().code(),
            "notes_output_exists"
        );
        assert_eq!(fs::read(&path).unwrap(), exact);
        fs::remove_file(path).unwrap();
        assert!(admit("relative.json").is_err());
        assert!(admit("/tmp/not-a-backup.pdf").is_err());
    }
}
