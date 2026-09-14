//! Offline verification reads retained authority only. It cannot fetch keys.
use super::*;
use ds_command_kernel::installation::{self, DesktopLeaseState};
const RETAINED_INSTALL: &str = "desktop-license.v1.json";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RetainedInstall {
    schema: u8,
    authority: String,
    request: VerifyInstallRequest,
    certificates: BTreeMap<String, String>,
    accepted_local_unix: i64,
    last_observed_local_unix: i64,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetainedDesktopLicense {
    pub decision: DesktopLeaseState,
    pub lease: VerifiedInstallLease,
}
struct RetainedKeys<'a>(&'a BTreeMap<String, String>);
impl PublicKeySource for RetainedKeys<'_> {
    fn resolve(&self, id: &str) -> Result<RsaPublicKey, String> {
        parse_rsa_certificate(self.0.get(id).ok_or(
            "The retained signing key is missing. Reconnect to renew the desktop license.",
        )?)
    }
}
impl AuthorityVerifier {
    pub(super) fn retain_install(&self, request: VerifyInstallRequest) -> Result<(), String> {
        let now = unix_now()?;
        let raw = read_bounded_file(
            &self.authority_dir.join(CERT_CACHE_FILE),
            MAX_AUTHORITY_RECORD_BYTES,
        )?
        .ok_or("Verified installation certificates were not retained")?;
        let cache: CertificateCache = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
        validate_certificate_cache(&cache, &self.pins, now)?;
        if !cache
            .certificates
            .contains_key(&request.capability.signature.key_id)
        {
            return Err("Verified installation signing key was not retained".into());
        }
        let retained = RetainedInstall {
            schema: 1,
            authority: self.pins.identity(),
            request,
            certificates: cache.certificates,
            accepted_local_unix: now,
            last_observed_local_unix: now,
        };
        let _lock = acquire_authority_lock(&self.authority_dir)?;
        atomic_replace(
            &self.authority_dir.join(RETAINED_INSTALL),
            &encode_record(&retained, MAX_AUTHORITY_RECORD_BYTES)?,
        )
    }
    pub fn retained_install(
        &self,
        install_id: &str,
        device: &str,
        arch: &str,
        app_version: &str,
        channel: &str,
    ) -> Result<RetainedDesktopLicense, String> {
        self.retained_install_at(install_id, device, arch, app_version, channel, unix_now()?)
    }
    fn retained_install_at(
        &self,
        install_id: &str,
        device: &str,
        arch: &str,
        app_version: &str,
        channel: &str,
        now: i64,
    ) -> Result<RetainedDesktopLicense, String> {
        let _lock = acquire_authority_lock(&self.authority_dir)?;
        let path = self.authority_dir.join(RETAINED_INSTALL);
        let raw = read_bounded_file(&path, MAX_AUTHORITY_RECORD_BYTES)?
            .ok_or("This desktop needs its first online license handshake")?;
        let mut retained: RetainedInstall = serde_json::from_slice(&raw)
            .map_err(|_| "The retained desktop license is malformed")?;
        if retained.schema != 1
            || retained.authority != self.pins.identity()
            || retained.accepted_local_unix < 0
            || retained.last_observed_local_unix < retained.accepted_local_unix
        {
            return Err("The retained desktop license belongs to another authority or has invalid clock evidence".into());
        }
        let expected = &retained.request.expected;
        if expected.install_id != install_id
            || expected.device != device
            || expected.arch != arch
            || expected.app_version != app_version
            || expected.channel != channel
        {
            return Err(
                "The retained desktop license does not match this installation and build".into(),
            );
        }
        validate_certificate_set(&retained.certificates)?;
        let lease = self.validate_install_with_keys(
            retained.request.clone(),
            &RetainedKeys(&retained.certificates),
            false,
        )?;
        let issued = parse_timestamp(&lease.issued_at, "issued at")?.timestamp();
        let expires = parse_timestamp(&lease.lease_expires_at, "expires at")?.timestamp();
        let server = parse_timestamp(&lease.server_time, "server time")?.timestamp();
        let effective_now = server.saturating_add(now.saturating_sub(retained.accepted_local_unix));
        let state = match lease.status {
            VerifiedInstallStatus::Active => "active",
            VerifiedInstallStatus::Blocked => "blocked",
            VerifiedInstallStatus::UpgradeRequired => "upgrade_required",
        };
        let mut decision = installation::evaluate(state, issued, expires, effective_now, issued);
        if now < retained.last_observed_local_unix {
            decision.access = installation::DesktopAccess::ClockRollback;
            decision.handshake_due = true;
        } else if now > retained.last_observed_local_unix {
            retained.last_observed_local_unix = now;
            atomic_replace(
                &path,
                &encode_record(&retained, MAX_AUTHORITY_RECORD_BYTES)?,
            )?;
        }
        Ok(RetainedDesktopLicense { decision, lease })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{install_payload, install_request, pins, private_key, CERTIFICATE};
    use rsa::pkcs8::EncodePublicKey;
    fn fixture() -> (PathBuf, AuthorityVerifier, RetainedInstall) {
        let root = std::env::temp_dir().join(format!(
            "ds-offline-license-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let authority = AuthorityVerifier::new(&root, pins()).unwrap();
        let payload = install_payload();
        let request = install_request(payload.clone(), &payload.issued_at);
        let body = CERTIFICATE
            .lines()
            .filter(|line| !line.starts_with("-----"))
            .collect::<String>();
        let mut cert =
            Certificate::from_der(&general_purpose::STANDARD.decode(body).unwrap()).unwrap();
        let public = private_key().to_public_key().to_public_key_der().unwrap();
        cert.tbs_certificate.subject_public_key_info = Decode::from_der(public.as_bytes()).unwrap();
        let encoded = general_purpose::STANDARD.encode(cert.to_der().unwrap());
        let body = encoded
            .as_bytes()
            .chunks(64)
            .map(|chunk| std::str::from_utf8(chunk).unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        let cert = format!("-----BEGIN CERTIFICATE-----\n{body}\n-----END CERTIFICATE-----\n");
        let retained = RetainedInstall {
            schema: 1,
            authority: pins().identity(),
            request,
            certificates: BTreeMap::from([("test-key".into(), cert)]),
            accepted_local_unix: 1_000_000,
            last_observed_local_unix: 1_000_000,
        };
        (root, authority, retained)
    }
    fn save(root: &Path, record: &RetainedInstall) {
        atomic_replace(
            &root.join(RETAINED_INSTALL),
            &encode_record(record, MAX_AUTHORITY_RECORD_BYTES).unwrap(),
        )
        .unwrap();
    }
    fn read(
        authority: &AuthorityVerifier,
        record: &RetainedInstall,
        now: i64,
    ) -> Result<RetainedDesktopLicense, String> {
        let expected = &record.request.expected;
        authority.retained_install_at(
            &expected.install_id,
            &expected.device,
            &expected.arch,
            &expected.app_version,
            &expected.channel,
            now,
        )
    }
    #[test]
    fn reboot_uses_retained_signature_without_network_and_expires_at_seven_days() {
        let (root, authority, record) = fixture();
        save(&root, &record);
        let six_days = record.accepted_local_unix + 6 * 24 * 60 * 60;
        let live = read(&authority, &record, six_days).unwrap();
        assert_eq!(live.decision.access, installation::DesktopAccess::Allowed);
        assert!(live.decision.handshake_due);
        assert_eq!(live.decision.remaining_seconds, 24 * 60 * 60);
        assert_eq!(
            read(&authority, &record, six_days - 1)
                .unwrap()
                .decision
                .access,
            installation::DesktopAccess::ClockRollback
        );
        assert_eq!(
            read(
                &authority,
                &record,
                record.accepted_local_unix + installation::OFFLINE_LEASE_SECONDS
            )
            .unwrap()
            .decision
            .access,
            installation::DesktopAccess::HandshakeRequired
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn signed_denial_survives_restart_and_tampering_is_refused() {
        let (root, authority, mut record) = fixture();
        let mut payload = install_payload();
        payload.status = "blocked".into();
        record.request = install_request(payload.clone(), &payload.issued_at);
        save(&root, &record);
        assert_eq!(
            read(&authority, &record, record.accepted_local_unix)
                .unwrap()
                .decision
                .access,
            installation::DesktopAccess::Blocked
        );
        record.request.capability.status = "active".into();
        save(&root, &record);
        assert!(read(&authority, &record, record.accepted_local_unix).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
