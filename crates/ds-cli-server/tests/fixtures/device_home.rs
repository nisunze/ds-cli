//! The protected native state a real `ds auth link` leaves on a machine —
//! built here, on disk, so the proof can run the REAL authorizer.
//!
//! The second adversarial pass refuted offline-first for the shipped wiring
//! and observed that only the stub `Authorizer` had ever been exercised
//! offline. The answer is not a better stub: it is the production
//! `NativeAuthorizer`, reading the production protected state, in the real
//! `ds server serve` process, with the network cut. That needs one thing this
//! box does not have — a signed-in device — so this module writes one.
//!
//! What it writes is exactly what the product reads, in the product's own
//! places: `$DS_CONFIG_HOME/ds/devices/<sha256(state key)>.json`, owner-only,
//! holding the closed core's own `ds-client.device-credential/v1` record. Two
//! values in it are not free: the LANE and the CREDENTIAL AUDIENCE must be the
//! ones the client profile names, or `DeviceCredential::decode_protected`
//! refuses the file as another profile's. The audience is therefore DERIVED
//! here the way `ClientProfile` derives it — the same length-prefixed digest
//! over the same four values, two of them the closed core's own published
//! constants — rather than copied, so a fixture cannot drift into agreeing
//! with a rule the product no longer has.
//!
//! Everything else in the record is a fixture: a name, an id, a fingerprint,
//! and a private key of thirty-two zero bytes. Nothing here ever signs
//! anything, because nothing here ever reaches a gateway — that is the point.

use std::fs;
use std::path::{Path, PathBuf};

use ds_client_core::{DEVICE_CREDENTIAL_AUDIENCE_SCHEMA, NATIVE_CLIENT_ID};
use ds_compute_runtime::digest;
use serde_json::{Value, json};

/// The development client catalogue this workspace ships for exactly this
/// purpose, and the one `run_ds` already hands every `ds` it starts.
pub fn catalogue() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../ds-cli-auth/tests/fixtures/development-catalog.json")
        .canonicalize()
        .expect("the workspace's development client catalogue")
}

/// A machine that holds one device credential for one lane.
///
/// It is a directory, not an object: everything a test does to it — writing a
/// credential, replacing it with another device's, removing it — is a change
/// to files on disk, which is the only kind of change the production
/// authorizer can observe.
pub struct DeviceHome {
    home: tempfile::TempDir,
    lane: String,
    audience: String,
}

/// The private key every fixture credential carries: thirty-two zero bytes,
/// base64url without padding. A valid Ed25519 seed and a deliberately
/// worthless one.
const ZERO_KEY: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

impl DeviceHome {
    /// A machine signed in on `lane`, holding `device_id`.
    pub fn linked(lane: &str, device_id: &str) -> Self {
        let home = tempfile::tempdir().expect("private config home");
        let audience = audience(lane);
        let machine = Self {
            home,
            lane: lane.to_owned(),
            audience,
        };
        owner_only_dir(&machine.root());
        owner_only_dir(&machine.devices());
        machine.link(device_id);
        machine
    }

    /// `$DS_CONFIG_HOME` — what a `ds` process is pointed at.
    pub fn config_home(&self) -> &Path {
        self.home.path()
    }

    fn root(&self) -> PathBuf {
        self.home.path().join("ds")
    }

    fn devices(&self) -> PathBuf {
        self.root().join("devices")
    }

    /// The file the device provider reads: named by the digest of the store's
    /// own logical key, exactly as `NativeDeviceStore` names it.
    pub fn credential_path(&self) -> PathBuf {
        let key = format!("device:{}:{}", self.lane, self.audience);
        self.devices()
            .join(format!("{}.json", digest(key.as_bytes())))
    }

    /// Put a device on this machine — or a DIFFERENT device, over the one
    /// already there. The binding the host authorizes against is
    /// `ds_device:<device id>:<fingerprint>`, so a second call with another
    /// id is precisely "the credential on disk changed".
    pub fn link(&self, device_id: &str) {
        let record = self.record(device_id);
        write_owner_only(
            &self.credential_path(),
            &serde_json::to_vec(&record).expect("a device credential record"),
        );
    }

    /// Sign this machine out, leaving the config home otherwise intact.
    pub fn unlink(&self) {
        fs::remove_file(self.credential_path()).expect("remove the held credential");
    }

    /// The binding string `runtime_credential_binding` derives from what is on
    /// disk — what the host compares on every request.
    pub fn binding(&self, device_id: &str) -> String {
        format!("ds_device:{device_id}:{}", fingerprint(device_id))
    }

    fn record(&self, device_id: &str) -> Value {
        json!({
            "schema": "ds-client.device-credential/v1",
            "device_id": device_id,
            "device_name": "isolation proof",
            "platform": "linux",
            "fingerprint": fingerprint(device_id),
            "uid": UID,
            "email": EMAIL,
            "lane": self.lane,
            "credential_audience": self.audience,
            // Approval provenance. The core validates their SHAPE and binds
            // the record to the profile by lane and audience alone, so these
            // are honest fixtures rather than a copy of this build's digests.
            "approved_profile_digest": format!("sha256:{}", "a".repeat(64)),
            "approved_catalog_digest": format!("sha256:{}", "b".repeat(64)),
            "credential_expires_at": "2099-01-01T00:00:00Z",
            "private_key": ZERO_KEY,
        })
    }
}

/// The account this machine is signed in as.
pub const UID: &str = "uid-proof-owner";
pub const EMAIL: &str = "owner@isolation.invalid";

/// One device's fingerprint: a digest of its id, in the prefixed form the
/// closed core insists on.
fn fingerprint(device_id: &str) -> String {
    format!("sha256:{}", digest(device_id.as_bytes()))
}

/// The credential audience for one lane, derived as `ClientProfile` derives
/// it: a SHA-256 over four length-prefixed values, of which two are the closed
/// core's published constants and one comes out of the catalogue this proof
/// hands `ds`. Deriving rather than copying is what keeps the fixture honest:
/// if the product changes how it fences a credential, this stops matching and
/// the proof fails instead of quietly testing a rule nobody has.
pub fn audience(lane: &str) -> String {
    let catalogue: Value = serde_json::from_slice(&fs::read(catalogue()).expect("the catalogue"))
        .expect("the catalogue parses");
    let firebase_project_id = catalogue["profiles"][lane]["firebase"]["project_id"]
        .as_str()
        .expect("the catalogue names this lane's Firebase project")
        .to_owned();
    let mut buffer = Vec::new();
    for value in [
        DEVICE_CREDENTIAL_AUDIENCE_SCHEMA,
        lane,
        NATIVE_CLIENT_ID,
        &firebase_project_id,
    ] {
        buffer.extend_from_slice(&(value.len() as u64).to_be_bytes());
        buffer.extend_from_slice(value.as_bytes());
    }
    digest(&buffer)
}

/// 0700, because the product refuses to read protected state out of a
/// directory anyone else can enter — as it should.
fn owner_only_dir(path: &Path) {
    fs::create_dir_all(path).expect("protected directory");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).expect("0700");
    }
}

/// 0600, single-linked, no symlink: the same three properties
/// `validate_file_metadata` checks before it will read a credential.
fn write_owner_only(path: &Path, bytes: &[u8]) {
    if path.exists() {
        fs::remove_file(path).expect("replace the held credential");
    }
    fs::write(path, bytes).expect("write the held credential");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).expect("0600");
    }
}
