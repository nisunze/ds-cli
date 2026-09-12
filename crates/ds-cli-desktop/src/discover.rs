//! Which live DS GridDesign instances are on this machine, and which one is
//! this operation for?
//!
//! A machine can run several instances at once — Stable beside Canary, two
//! windows of one build, a developer's local build beside an installed one —
//! so the unit of pairing is an *instance*, never an install profile. Each
//! running instance publishes its own descriptor under
//! `<app data>/cli-bridge.d/<instance_id>.json`, and the first one also
//! refreshes the legacy `<app data>/cli-bridge.json` so an older `ds` keeps
//! working. This module reads both, de-duplicates them, proves each one is
//! alive with an authenticated handshake, and then asks the kernel which
//! instance the operation belongs to.
//!
//! **Nothing here decides.** `ds_command_kernel::desktop_instance` owns four
//! answers — may this descriptor be used, who is it, which instance is this
//! operation for, and what may be shown about the ones that were found — and
//! this module performs the I/O around them: read the file, send the probe,
//! post to the endpoint the answer named. That split is why an explicit target
//! cannot fall through here: the fall-through would have to be written, and
//! there is nowhere to write it.
//!
//! The caller can always settle the choice itself, two ways that are not the
//! same thing:
//!
//! * `--target desktop:<instance_id>` names one live instance. It is honoured
//!   or refused by name; it never routes anywhere else.
//! * `--desktop-descriptor <path>` (or `DS_DESKTOP_DESCRIPTOR` for a whole
//!   session, which is what the desktop's own `cl` terminal sets) names one
//!   descriptor *file*. It is used verbatim, exactly as before.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ds_cli_contract::outcome::{ExitClass, Failure};
use ds_command_kernel::desktop_instance as kernel;
use serde_json::{Value, json};

/// The install profiles, in the order they are reported. Each is one Tauri
/// bundle identifier, which is what determines the descriptor's directory.
pub const PROFILES: &[(&str, &str)] = &[
    ("stable", "rw.datasolutions.desktop"),
    ("canary", "rw.datasolutions.desktop.canary"),
    ("dev", "rw.datasolutions.desktop.local-dev"),
    ("dev-canary", "rw.datasolutions.desktop.dev"),
];

/// The legacy per-profile descriptor. One live instance keeps refreshing it so
/// a `ds` that predates the registry directory still pairs with something real.
pub const DESCRIPTOR_FILE: &str = "cli-bridge.json";

/// The registry directory: one file per live instance, named by its id.
pub const DESCRIPTOR_DIR: &str = "cli-bridge.d";

/// The environment variable naming a descriptor for every command in a
/// session. `--desktop-descriptor` still wins when both are present, and
/// automatic discovery runs only when neither is: the variable is a default
/// for the flag, never an override of it.
pub const DESCRIPTOR_ENV: &str = "DS_DESKTOP_DESCRIPTOR";

/// Bound the descriptor read. The real file is a few hundred bytes, and the
/// number is the kernel's so both sides of the contract read the same one.
pub const MAX_DESCRIPTOR_BYTES: u64 = kernel::MAX_DESCRIPTOR_BYTES;

/// The most instances one enumeration carries, which is the kernel's own bound
/// on a candidate set. A profile that somehow holds more files than this
/// reports the overflow rather than silently choosing which ones to read.
pub const MAX_INSTANCES: usize = kernel::MAX_CANDIDATES;

/// Automatic discovery probes every live endpoint it found. A dead descriptor
/// must not make a live session ambiguous, and each dead probe must stay cheap.
const LIVE_PROBE_TIMEOUT: Duration = Duration::from_millis(150);

/// The handshake is a bounded projection of the session; the invoke path reads
/// the whole thing later, under its own bound.
const MAX_PROBE_BODY: u64 = 64 * 1024;

/// The instance an operation was routed to: where to reach it, what to
/// authenticate with, and which instance it is.
///
/// `url` and `token` are the two fields the transport needs and the only two
/// that are secret-adjacent; the type has no `Debug`, deliberately, so the
/// token cannot be formatted into a result by accident.
pub struct Descriptor {
    pub url: String,
    pub token: String,
    pub pid: u32,
    /// Minted by the instance, or derived by the kernel from what an older
    /// descriptor does say. `identity` says which.
    pub instance_id: String,
    pub identity: kernel::Identity,
    /// The install profile the descriptor was read under, when it was read
    /// under one. An explicitly named path belongs to no profile.
    pub profile: Option<String>,
    pub path: PathBuf,
    /// The window a caller pinned, when it pinned one. Automatic routing never
    /// names a window: which view serves an unpinned operation is the shell's
    /// own rule.
    pub window: Option<String>,
}

/// One resolved pairing: the instance, and where its descriptor was read.
///
/// `profile` stays a `&'static str` because callers use it to compare against
/// their own `--lane`, and because an explicitly named path has no profile of
/// its own — it is `explicit` or `environment`, which are facts about how it
/// was named rather than about which install it belongs to.
pub struct Found {
    pub profile: &'static str,
    pub path: PathBuf,
    pub descriptor: Descriptor,
}

/// What one live instance answered, once its descriptor had been admitted.
pub enum Handshake {
    /// A signed-in session that speaks the contract: a candidate for routing.
    Session(Box<kernel::Candidate>),
    /// Live and answering, but not routable: signed out, or publishing a
    /// session that is not the contract.
    Unusable(Unusable),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unusable {
    /// Running, but nobody is signed in — so it can serve nobody's work.
    SignedOut,
    /// Running and signed in, but what it published is not the contract.
    Contract,
}

/// One live instance: its admitted descriptor and what its handshake said.
pub struct Live {
    pub found: Found,
    pub handshake: Handshake,
    /// The raw session body, so a diagnostic can project fields the routing
    /// decision has no use for.
    pub session: Value,
}

impl Live {
    pub fn candidate(&self) -> Option<&kernel::Candidate> {
        match &self.handshake {
            Handshake::Session(candidate) => Some(candidate),
            Handshake::Unusable(_) => None,
        }
    }
    pub fn instance_id(&self) -> &str {
        &self.found.descriptor.instance_id
    }
}

/// Everything one enumeration found: the live instances, and the descriptors
/// that could not be used at all.
#[derive(Default)]
pub struct Enumeration {
    pub live: Vec<Live>,
    /// A descriptor file that exists and cannot be used, with the kernel's own
    /// reason. Reported rather than dropped: an operator whose app is running
    /// and whose `ds` says "not paired" needs to be told which file is wrong.
    pub unusable: Vec<(PathBuf, String)>,
    /// Descriptor files past [`MAX_INSTANCES`] that were not read.
    pub omitted: usize,
}

impl Enumeration {
    pub fn candidates(&self) -> Vec<kernel::Candidate> {
        self.live
            .iter()
            .filter_map(|live| live.candidate().cloned())
            .collect()
    }
    pub fn is_empty(&self) -> bool {
        self.live.is_empty()
    }
}

/// Where a Tauri app with `identifier` keeps its app data, matching
/// `AppHandle::path().app_data_dir()` on each platform.
pub fn app_data_dir(identifier: &str) -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA").map(|base| PathBuf::from(base).join(identifier))
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME").map(|home| {
            PathBuf::from(home)
                .join("Library/Application Support")
                .join(identifier)
        })
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share"))
            })
            .map(|base| base.join(identifier))
    }
}

/// Relay one kernel decision to a caller: its code, its sentence, its remedy
/// and its bounded detail, unchanged. The class is the only thing added here,
/// because the kernel does not model CLI exit classes.
pub fn relay(fault: kernel::Fault) -> Failure {
    let refusal = match fault {
        kernel::Fault::Refused(refusal) => refusal,
        // The kernel refuses a request the host built wrong — a candidate set
        // with one instance twice, a requirement naming no lane. A caller
        // cannot cause it and cannot act on it.
        kernel::Fault::Hard(message) => {
            return Failure::internal("desktop_enumeration_defect", message)
                .remedy("this is a defect in ds; report it with the command you ran");
        }
    };
    let class = match refusal.code {
        kernel::DESKTOP_AMBIGUOUS
        | kernel::DESKTOP_TARGET_NOT_LIVE
        | kernel::DESKTOP_TARGET_MISMATCH => ExitClass::InvalidInput,
        kernel::DESKTOP_PROJECT_NOT_OPEN | kernel::CONTEXT_GENERATION_STALE => ExitClass::Conflict,
        _ => ExitClass::Unavailable,
    };
    let mut failure = Failure::new(class, refusal.code, refusal.message).remedy(refusal.remedy);
    if let Some(detail) = refusal.detail {
        failure = failure.detail(*detail);
    }
    failure
}

/// Read one descriptor path and ask the kernel to admit it.
///
/// `profile` is the install profile the file was read *under* — the directory
/// it came from. The kernel refuses a descriptor that names a different one:
/// it has been copied out of its own install, and its token is not sent.
pub fn read(path: &Path, profile: Option<&str>) -> Result<Descriptor, Failure> {
    let metadata =
        std::fs::metadata(path).map_err(|error| unreadable(&error.kind().to_string()))?;
    if metadata.len() > MAX_DESCRIPTOR_BYTES {
        return Err(unreadable("descriptor is larger than its bound"));
    }
    let bytes = std::fs::read(path).map_err(|error| unreadable(&error.kind().to_string()))?;
    let published: kernel::Descriptor =
        serde_json::from_slice(&bytes).map_err(|_| unreadable("descriptor is not valid JSON"))?;
    let admission = kernel::admit_descriptor(&published, profile).map_err(relay)?;
    Ok(Descriptor {
        url: admission.origin.clone(),
        token: published.token.clone(),
        pid: admission.pid,
        instance_id: admission.instance_id,
        identity: admission.identity,
        profile: admission.profile,
        path: path.to_path_buf(),
        window: None,
    })
}

/// A file this process could not read at all. It is not a kernel decision —
/// the kernel never sees the bytes — so it keeps the shape the kernel's own
/// `descriptor_unusable` has, and callers classify it the way they always have.
fn unreadable(reason: &str) -> Failure {
    Failure::unavailable(
        kernel::DESCRIPTOR_UNUSABLE,
        format!("the published descriptor {reason}"),
    )
    .remedy("restart DS GridDesign, or name a descriptor with --desktop-descriptor <path>")
    .detail(json!({ "reason": reason }))
}

/// Every descriptor file on this machine, newest registry first and the legacy
/// per-profile file last, so an instance that publishes both is read under its
/// minted identity rather than a derived one.
fn descriptor_files() -> Vec<(&'static str, PathBuf)> {
    let mut files = Vec::new();
    for (profile, identifier) in PROFILES {
        let Some(dir) = app_data_dir(identifier) else {
            continue;
        };
        let mut registry: Vec<PathBuf> = std::fs::read_dir(dir.join(DESCRIPTOR_DIR))
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            })
            .filter(|path| path.is_file())
            .collect();
        // Sorted, so what a bounded read leaves out is the same set on every
        // run. Which instance a bounded read reaches is never the choice —
        // the kernel refuses ambiguity — but a set that changed between two
        // commands would make one refusal and one route out of one machine.
        registry.sort();
        files.extend(registry.into_iter().map(|path| (*profile, path)));
        let legacy = dir.join(DESCRIPTOR_FILE);
        if legacy.is_file() {
            files.push((*profile, legacy));
        }
    }
    files
}

/// Find every live instance: read the descriptors, admit each, drop the
/// duplicates, and prove liveness with an authenticated handshake.
pub fn enumerate() -> Enumeration {
    enumerate_with(descriptor_files(), probe)
}

/// The enumeration, with its two effects injected: which files exist, and what
/// each endpoint answers. Both are the host's I/O, and neither decides
/// anything — which is exactly why a test can supply them.
pub fn enumerate_with<P>(files: Vec<(&'static str, PathBuf)>, probe: P) -> Enumeration
where
    P: Fn(&Descriptor) -> Option<Value>,
{
    let mut enumeration = Enumeration::default();
    let mut admitted: Vec<Found> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for (profile, path) in files {
        if admitted.len() >= MAX_INSTANCES {
            enumeration.omitted += 1;
            continue;
        }
        match read(&path, Some(profile)) {
            Ok(descriptor) => {
                // One instance publishes two files — its own, and the legacy
                // copy for an older `ds`. Presenting it twice is a hard fault
                // in the kernel, deliberately, so the de-duplication is here.
                // It is by identity *and* by endpoint, because a legacy copy
                // written without an instance id admits under a derived one.
                let endpoint = format!("{}#{}", descriptor.url, descriptor.pid);
                if !seen.insert(descriptor.instance_id.clone()) || !seen.insert(endpoint) {
                    continue;
                }
                admitted.push(Found {
                    profile,
                    path,
                    descriptor,
                });
            }
            Err(failure) => enumeration.unusable.push((
                path,
                failure
                    .detail_value()
                    .and_then(|detail| detail["reason"].as_str().map(str::to_owned))
                    .unwrap_or_else(|| failure.message().to_owned()),
            )),
        }
    }

    let mut live: BTreeSet<String> = BTreeSet::new();
    for mut found in admitted {
        let Some(session) = probe(&found.descriptor) else {
            continue;
        };
        // A running instance names itself. Its own answer outranks an identity
        // derived from a file it may have outgrown, and a second file naming
        // the same running instance is the same instance.
        if let Some(published) = session
            .get("instance_id")
            .and_then(Value::as_str)
            .filter(|id| kernel::instance_id_valid(id))
        {
            found.descriptor.instance_id = published.to_owned();
            found.descriptor.identity = kernel::Identity::Minted;
        }
        if !live.insert(found.descriptor.instance_id.clone()) {
            continue;
        }
        let handshake = handshake_of(&found.descriptor, &session);
        enumeration.live.push(Live {
            found,
            handshake,
            session,
        });
    }
    enumeration
}

/// Read one live instance's handshake as the kernel's candidate, or say why it
/// is not one.
fn handshake_of(descriptor: &Descriptor, session: &Value) -> Handshake {
    let text = |key: &str| {
        session
            .get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    };
    let (Some(uid), Some(lane), Some(audience)) = (
        text("uid"),
        text("lane"),
        text("credential_audience_sha256"),
    ) else {
        return Handshake::Unusable(Unusable::SignedOut);
    };
    let candidate = kernel::Candidate {
        instance_id: descriptor.instance_id.clone(),
        profile: descriptor.profile.clone(),
        lane,
        uid,
        audience_sha256: audience,
        project: text("project"),
        build: text("build"),
        started_at_ms: session.get("started_at_ms").and_then(Value::as_u64),
        session_revision: session
            .get("session_revision")
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        windows: windows_of(session),
    };
    // Validated by the owner of the rule rather than by a second copy of it
    // here: `list` runs exactly the checks `select` would, on one candidate.
    match kernel::list(std::slice::from_ref(&candidate), None) {
        Ok(_) => Handshake::Session(Box::new(candidate)),
        Err(_) => Handshake::Unusable(Unusable::Contract),
    }
}

fn windows_of(session: &Value) -> Vec<kernel::Window> {
    session
        .get("windows")
        .and_then(Value::as_array)
        .map(|windows| {
            windows
                .iter()
                .take(kernel::MAX_WINDOWS)
                .filter_map(|window| {
                    Some(kernel::Window {
                        label: window.get("label")?.as_str()?.to_owned(),
                        project: window
                            .get("project")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                        generation: window.get("generation")?.as_u64()?,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Prove one descriptor is live, and read what it published.
///
/// A successful TCP handshake proves only that *something* reused this port. A
/// stale descriptor could therefore select an unrelated listener ahead of a
/// live Desktop, so a candidate participates only after the bridge answers its
/// own authenticated session request. Redirects are refused: the pairing token
/// is for the one loopback origin the descriptor named.
fn probe(descriptor: &Descriptor) -> Option<Value> {
    let response = ureq::get(&format!("{}/v1/session", descriptor.url))
        .header("authorization", &format!("Bearer {}", descriptor.token))
        .config()
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_global(Some(LIVE_PROBE_TIMEOUT))
        .build()
        .call()
        .ok()?;
    if response.status().as_u16() != 200 {
        return None;
    }
    let body = response
        .into_body()
        .with_config()
        .limit(MAX_PROBE_BODY)
        .read_to_string()
        .ok()?;
    let session: Value = serde_json::from_str(&body).ok()?;
    // The contract's own liveness proof: a session revision. Something that
    // answers 200 with anything else is not this bridge.
    session.get("session_revision")?.as_u64()?;
    Some(session)
}

/// Ask one endpoint who it is. Used where a caller named both a descriptor
/// file and an instance: the file's admitted identity may be derived, and only
/// the running process can say what it actually calls itself.
pub fn identify(descriptor: &Descriptor) -> Option<String> {
    let session = probe(descriptor)?;
    Some(
        session
            .get("instance_id")
            .and_then(Value::as_str)
            .filter(|id| kernel::instance_id_valid(id))
            .unwrap_or(&descriptor.instance_id)
            .to_owned(),
    )
}

/// The caller's own identity and what the operation needs of the instance that
/// serves it. Built by [`crate::ops`] from the scoped headless observation.
pub type Requirement = kernel::Requirement;
pub type Target = kernel::Target;

/// Ask the kernel which live instance this operation is for, and hand back the
/// endpoint of the one it named.
///
/// `requirement` is the caller's own identity when `ds` knows it. When it does
/// not — no native profile is configured on this machine, so nothing has told
/// `ds` who is running it — the identity is taken from the live instances
/// themselves, and a machine whose sessions disagree about who they are is an
/// ambiguity the caller has to settle.
pub fn choose(
    enumeration: Enumeration,
    target: Option<&Target>,
    requirement: Option<&Requirement>,
) -> Result<Found, Failure> {
    let candidates = enumeration.candidates();
    let owned;
    let requirement = match requirement {
        Some(requirement) => requirement,
        None => {
            owned = adopted_requirement(&candidates)?;
            &owned
        }
    };
    let route = kernel::select(&candidates, target, requirement).map_err(|fault| {
        let failure = relay(fault);
        // Nothing compatible is live, and this caller's own session is a
        // development build. That is a real, common answer with its own
        // remedy, and it is the caller's own instance being named — not
        // another account's.
        if failure.code() == kernel::DESKTOP_NOT_PAIRED
            && enumeration.live.iter().any(|live| {
                live.candidate().is_some_and(|candidate| {
                    candidate.uid == requirement.uid && candidate.lane == "local"
                })
            })
        {
            return unprovisioned_lane();
        }
        failure
    })?;

    let mut found = enumeration
        .live
        .into_iter()
        .find(|live| live.instance_id() == route.instance_id)
        .map(|live| live.found)
        .ok_or_else(|| {
            Failure::internal(
                "desktop_enumeration_defect",
                "the kernel routed to an instance this enumeration does not hold",
            )
            .remedy("this is a defect in ds; report it with the command you ran")
        })?;
    found.descriptor.window = route.window;
    Ok(found)
}

/// The identity to route by when the caller has none of its own.
///
/// With nothing live there is nothing to adopt, and nothing to route to
/// either: the only answers left are "you named an instance that is not live"
/// and "nothing is paired", both of which are the kernel's to give — so a
/// placeholder that matches nothing is what asks it for them.
fn adopted_requirement(candidates: &[kernel::Candidate]) -> Result<Requirement, Failure> {
    let identities: BTreeSet<(&str, &str, &str)> = candidates
        .iter()
        .map(|candidate| {
            (
                candidate.lane.as_str(),
                candidate.uid.as_str(),
                candidate.audience_sha256.as_str(),
            )
        })
        .collect();
    let mut identities = identities.into_iter();
    let Some((lane, uid, audience)) = identities.next() else {
        return Ok(Requirement {
            lane: "stable".to_owned(),
            uid: "-".to_owned(),
            audience_sha256: "0".repeat(kernel::AUDIENCE_CHARS),
            project: None,
            project_independent: true,
        });
    };
    if identities.next().is_some() {
        // Two signed-in sessions under different accounts, and no way to know
        // which one is this caller's. The kernel refuses ambiguity it can see;
        // this is the ambiguity only the host can see, and it refuses the same
        // way rather than adopting whichever identity sorted first.
        return Err(Failure::invalid(
            kernel::DESKTOP_AMBIGUOUS,
            "more than one DS GridDesign account is signed in on this machine",
        )
        .remedy("name one with --target desktop:<instance_id>")
        .detail(json!({
            "candidates": kernel::list(candidates, None).map(|(listed, _)| listed).unwrap_or_default()
        })));
    }
    if !matches!(lane, "stable" | "canary") {
        return Err(unprovisioned_lane());
    }
    Ok(Requirement {
        lane: lane.to_owned(),
        uid: uid.to_owned(),
        audience_sha256: audience.to_owned(),
        project: None,
        project_independent: true,
    })
}

fn unprovisioned_lane() -> Failure {
    Failure::unavailable(
        "desktop_operation_unsupported",
        "the paired local Desktop build has no provisioned native lane",
    )
    .remedy("use a provisioned Canary or Stable DS GridDesign build for paired CLI operations")
}

/// The descriptor a caller named, if it named one. `--desktop-descriptor` wins
/// over `DS_DESKTOP_DESCRIPTOR`, and automatic enumeration runs only when
/// neither is set: the variable is a default for the flag, never an override.
pub fn named(explicit: Option<&str>) -> Option<(&'static str, PathBuf)> {
    if let Some(path) = explicit {
        return Some(("explicit", PathBuf::from(path)));
    }
    std::env::var(DESCRIPTOR_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(|value| ("environment", PathBuf::from(value)))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    const AUDIENCE: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    const ONE: &str = "11111111111111111111111111111111";
    const TWO: &str = "22222222222222222222222222222222";

    static NEXT: AtomicU64 = AtomicU64::new(1);

    fn scratch() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "ds-descriptor-discovery-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir_all(&path).expect("scratch");
        path
    }

    fn write_descriptor(path: &Path, body: Value) -> PathBuf {
        fs::create_dir_all(path.parent().expect("a parent")).expect("directory");
        fs::write(path, serde_json::to_vec(&body).expect("encodes")).expect("descriptor");
        path.to_path_buf()
    }

    fn descriptor_body(port: u16, pid: u32, instance: Option<&str>) -> Value {
        let mut body = json!({
            "version": 1,
            "url": format!("http://127.0.0.1:{port}"),
            "token": "0123456789abcdef0123456789abcdef",
            "pid": pid,
        });
        if let Some(instance) = instance {
            body["instance_id"] = json!(instance);
        }
        body
    }

    fn session(instance: &str, project: Option<&str>) -> Value {
        json!({
            "session_revision": 3,
            "instance_id": instance,
            "uid": "uid-a",
            "lane": "canary",
            "credential_audience_sha256": AUDIENCE,
            "project": project,
            "build": "2026.9.12+1",
            "windows": [{"label": "main", "project": project, "generation": 2}],
        })
    }

    fn requirement(project: Option<&str>) -> Requirement {
        Requirement {
            lane: "canary".to_owned(),
            uid: "uid-a".to_owned(),
            audience_sha256: AUDIENCE.to_owned(),
            project: project.map(str::to_owned),
            project_independent: false,
        }
    }

    fn target(instance: &str) -> Target {
        Target {
            instance_id: instance.to_owned(),
            window: None,
        }
    }

    /// `expect_err` needs a `Debug` value on the success side, and neither a
    /// descriptor nor a resolved pairing has one — deliberately, because both
    /// hold a pairing token. So the refusal is taken out by hand.
    fn refusal<T>(result: Result<T, Failure>, what: &str) -> Failure {
        match result {
            Ok(_) => panic!("expected a refusal: {what}"),
            Err(failure) => failure,
        }
    }

    /// An enumeration over descriptor files that exist, with the handshake
    /// answered from a table instead of a socket.
    fn enumerated(root: &Path, sessions: &[(&str, Value)]) -> Enumeration {
        let mut files: Vec<(&'static str, PathBuf)> = fs::read_dir(root)
            .expect("scratch")
            .flatten()
            .map(|entry| ("canary", entry.path()))
            .filter(|(_, path)| path.is_file())
            .collect();
        files.sort_by(|left, right| left.1.cmp(&right.1));
        let answers: Vec<(String, Value)> = sessions
            .iter()
            .map(|(url, body)| ((*url).to_owned(), body.clone()))
            .collect();
        enumerate_with(files, |descriptor| {
            answers
                .iter()
                .find(|(url, _)| *url == descriptor.url)
                .map(|(_, body)| body.clone())
        })
    }

    fn session_listener(status: u16, token: &'static str) -> (u16, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("address").port();
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("connection");
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            let mut request = String::new();
            reader.read_line(&mut request).expect("request line");
            assert_eq!(request, "GET /v1/session HTTP/1.1\r\n");
            let mut authorization = String::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).expect("header");
                if line == "\r\n" {
                    break;
                }
                if line.to_ascii_lowercase().starts_with("authorization:") {
                    authorization = line;
                }
            }
            let expected = format!("Bearer {token}");
            assert_eq!(
                authorization.split_once(':').map(|(_, value)| value.trim()),
                Some(expected.as_str())
            );
            let body = if status == 200 {
                r#"{"session_revision":1}"#
            } else {
                r#"{"error":"pairing_required"}"#
            };
            let reason = match status {
                200 => "OK",
                302 => "Found",
                _ => "Unauthorized",
            };
            write!(
                stream,
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .expect("response");
        });
        (port, worker)
    }

    #[test]
    fn descriptor_refuses_spoofed_or_noncanonical_authorities_before_a_token_is_sent() {
        let root = scratch();
        for (index, url) in [
            "http://127.0.0.1:80@evil.example/",
            "http://token@127.0.0.1:80/",
            "https://127.0.0.1:443/",
            "http://127.0.0.1:80/v1/session",
            "http://127.0.0.1:80/?redirect=evil",
        ]
        .iter()
        .enumerate()
        {
            let path = root.join(format!("spoof-{index}.json"));
            fs::write(
                &path,
                format!(
                    r#"{{"version":1,"url":"{url}","token":"{}","pid":1}}"#,
                    "n".repeat(32)
                ),
            )
            .expect("descriptor");
            let refused = refusal(
                read(&path, None),
                &format!("{url} must not receive a token"),
            );
            assert_eq!(refused.code(), kernel::DESCRIPTOR_UNUSABLE, "{url}");
        }
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn authenticated_probe_does_not_follow_a_redirect() {
        let root = scratch();
        let (port, redirect) = session_listener(302, "paired-paired-paired-paired-pair");
        let path = write_descriptor(
            &root.join("redirect.json"),
            json!({"version": 1, "url": format!("http://127.0.0.1:{port}"),
                   "token": "paired-paired-paired-paired-pair", "pid": 1}),
        );
        let descriptor = read(&path, None).expect("loopback descriptor");
        assert!(probe(&descriptor).is_none());
        redirect.join().expect("redirect listener");
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn a_stale_descriptor_is_skipped_only_after_a_verified_handshake() {
        let root = scratch();
        let (stale_port, stale) = session_listener(401, "stale-stale-stale-stale-stale-st");
        let (live_port, live) = session_listener(200, "live-live-live-live-live-live-li");
        write_descriptor(
            &root.join("cli-bridge.d/stale.json"),
            json!({"version": 1, "url": format!("http://127.0.0.1:{stale_port}"),
                   "token": "stale-stale-stale-stale-stale-st", "pid": 1}),
        );
        write_descriptor(
            &root.join("cli-bridge.d/live.json"),
            json!({"version": 1, "url": format!("http://127.0.0.1:{live_port}"),
                   "token": "live-live-live-live-live-live-li", "pid": 2}),
        );
        let files: Vec<(&'static str, PathBuf)> = {
            let mut paths: Vec<PathBuf> = fs::read_dir(root.join("cli-bridge.d"))
                .expect("registry")
                .flatten()
                .map(|entry| entry.path())
                .collect();
            paths.sort();
            paths.into_iter().map(|path| ("canary", path)).collect()
        };
        // The real probe, against two real listeners: the dead one answers 401
        // and is skipped, and the live one is the only instance found.
        let enumeration = enumerate_with(files, probe);
        assert_eq!(enumeration.live.len(), 1);
        assert_eq!(enumeration.live[0].found.descriptor.pid, 2);
        stale.join().expect("stale listener");
        live.join().expect("live listener");
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn one_instance_that_publishes_two_files_is_enumerated_once() {
        let root = scratch();
        // The registry file and the legacy copy of the same running instance,
        // the legacy one written the old way with no instance id at all.
        write_descriptor(
            &root.join(format!("cli-bridge.d-{ONE}.json")),
            descriptor_body(41234, 4711, Some(ONE)),
        );
        write_descriptor(
            &root.join("cli-bridge.json"),
            descriptor_body(41234, 4711, None),
        );
        let enumeration = enumerated(&root, &[("http://127.0.0.1:41234", session(ONE, None))]);
        assert_eq!(enumeration.live.len(), 1, "one process, one instance");
        assert_eq!(enumeration.live[0].instance_id(), ONE);
        assert_eq!(
            enumeration.live[0].found.descriptor.identity,
            kernel::Identity::Minted
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn an_older_instance_is_enumerated_under_the_identity_the_kernel_derives() {
        let root = scratch();
        write_descriptor(
            &root.join("cli-bridge.json"),
            descriptor_body(41240, 91, None),
        );
        let mut older = session(ONE, Some("project-a"));
        older.as_object_mut().expect("object").remove("instance_id");
        let enumeration = enumerated(&root, &[("http://127.0.0.1:41240", older)]);
        assert_eq!(enumeration.live.len(), 1);
        let derived = enumeration.live[0].instance_id().to_owned();
        assert!(kernel::instance_id_valid(&derived));
        assert_eq!(
            enumeration.live[0].found.descriptor.identity,
            kernel::Identity::Derived
        );
        // And it is targetable within this enumeration, which is the whole
        // reason a derived identity exists.
        let found = choose(
            enumerated(
                &root,
                &[("http://127.0.0.1:41240", {
                    let mut older = session(ONE, Some("project-a"));
                    older.as_object_mut().expect("object").remove("instance_id");
                    older
                })],
            ),
            Some(&target(&derived)),
            Some(&requirement(Some("project-a"))),
        )
        .expect("the derived identity names this instance");
        assert_eq!(found.descriptor.instance_id, derived);
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn two_live_instances_refuse_without_an_explicit_target() {
        let root = scratch();
        write_descriptor(
            &root.join(format!("a-{ONE}.json")),
            descriptor_body(41234, 11, Some(ONE)),
        );
        write_descriptor(
            &root.join(format!("b-{TWO}.json")),
            descriptor_body(41235, 12, Some(TWO)),
        );
        let sessions = [
            ("http://127.0.0.1:41234", session(ONE, Some("project-a"))),
            ("http://127.0.0.1:41235", session(TWO, Some("project-a"))),
        ];
        let refused = refusal(
            choose(
                enumerated(&root, &sessions),
                None,
                Some(&requirement(Some("project-a"))),
            ),
            "two instances hold the project",
        );
        assert_eq!(refused.code(), kernel::DESKTOP_AMBIGUOUS);
        assert_eq!(
            refused.detail_value().expect("candidates")["candidates"]
                .as_array()
                .expect("an array")
                .len(),
            2
        );
        // Naming one settles it, and the endpoint is that instance's own.
        let found = choose(
            enumerated(&root, &sessions),
            Some(&target(TWO)),
            Some(&requirement(Some("project-a"))),
        )
        .expect("a named instance routes");
        assert_eq!(found.descriptor.instance_id, TWO);
        assert_eq!(found.descriptor.url, "http://127.0.0.1:41235");
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn an_explicit_dead_instance_never_falls_through_to_the_live_one() {
        let root = scratch();
        write_descriptor(
            &root.join(format!("a-{ONE}.json")),
            descriptor_body(41234, 11, Some(ONE)),
        );
        let refused = refusal(
            choose(
                enumerated(
                    &root,
                    &[("http://127.0.0.1:41234", session(ONE, Some("project-a")))],
                ),
                Some(&target(TWO)),
                Some(&requirement(Some("project-a"))),
            ),
            "the named instance is not live",
        );
        assert_eq!(refused.code(), kernel::DESKTOP_TARGET_NOT_LIVE);
        assert_eq!(
            refused.detail_value().expect("a target")["target"],
            json!(TWO)
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn a_saved_project_narrows_where_work_may_go_and_never_moves_a_map() {
        let root = scratch();
        write_descriptor(
            &root.join(format!("a-{ONE}.json")),
            descriptor_body(41234, 11, Some(ONE)),
        );
        let sessions = [("http://127.0.0.1:41234", session(ONE, Some("project-a")))];
        let refused = refusal(
            choose(
                enumerated(&root, &sessions),
                None,
                Some(&requirement(Some("project-z"))),
            ),
            "no live instance holds the saved project",
        );
        assert_eq!(refused.code(), kernel::DESKTOP_PROJECT_NOT_OPEN);
        assert!(
            refused
                .remedy_text()
                .is_some_and(|remedy| remedy.contains("ds desktop project switch --target")),
            "the remedy is the explicit switch: {:?}",
            refused.remedy_text()
        );
        // The same call, project-independent, runs on the same instance: a
        // saved selection is a condition on project work, not on every call.
        let found = choose(
            enumerated(&root, &sessions),
            None,
            Some(&Requirement {
                project_independent: true,
                ..requirement(Some("project-z"))
            }),
        )
        .expect("a project-independent operation runs on the compatible instance");
        assert_eq!(found.descriptor.instance_id, ONE);
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn a_signed_out_instance_is_live_but_never_a_candidate() {
        let root = scratch();
        write_descriptor(
            &root.join(format!("a-{ONE}.json")),
            descriptor_body(41234, 11, Some(ONE)),
        );
        let mut signed_out = session(ONE, None);
        for key in ["uid", "credential_audience_sha256"] {
            signed_out.as_object_mut().expect("object").remove(key);
        }
        let enumeration = enumerated(&root, &[("http://127.0.0.1:41234", signed_out)]);
        assert_eq!(enumeration.live.len(), 1);
        assert!(matches!(
            enumeration.live[0].handshake,
            Handshake::Unusable(Unusable::SignedOut)
        ));
        assert!(enumeration.candidates().is_empty());
        assert_eq!(
            refusal(
                choose(enumeration, None, Some(&requirement(None))),
                "nothing is signed in",
            )
            .code(),
            kernel::DESKTOP_NOT_PAIRED
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn a_development_lane_session_keeps_its_own_named_refusal() {
        let root = scratch();
        write_descriptor(
            &root.join(format!("a-{ONE}.json")),
            descriptor_body(41234, 11, Some(ONE)),
        );
        let mut local = session(ONE, Some("project-a"));
        local["lane"] = json!("local");
        let enumeration = enumerated(&root, &[("http://127.0.0.1:41234", local)]);
        assert_eq!(
            refusal(
                choose(enumeration, None, Some(&requirement(Some("project-a")))),
                "a development build has no provisioned lane",
            )
            .code(),
            "desktop_operation_unsupported"
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn two_accounts_and_no_caller_identity_is_an_ambiguity_the_caller_settles() {
        let root = scratch();
        write_descriptor(
            &root.join(format!("a-{ONE}.json")),
            descriptor_body(41234, 11, Some(ONE)),
        );
        write_descriptor(
            &root.join(format!("b-{TWO}.json")),
            descriptor_body(41235, 12, Some(TWO)),
        );
        let mut theirs = session(TWO, Some("project-b"));
        theirs["uid"] = json!("uid-b");
        let sessions = [
            ("http://127.0.0.1:41234", session(ONE, Some("project-a"))),
            ("http://127.0.0.1:41235", theirs),
        ];
        let refused = refusal(
            choose(enumerated(&root, &sessions), None, None),
            "ds does not know which account is running it",
        );
        assert_eq!(refused.code(), kernel::DESKTOP_AMBIGUOUS);
        // One account, no caller identity: the instance's own identity is
        // adopted and the single compatible instance routes.
        let one = [("http://127.0.0.1:41234", session(ONE, Some("project-a")))];
        assert_eq!(
            choose(enumerated(&root, &one), None, None)
                .expect("one signed-in session needs no argument")
                .descriptor
                .instance_id,
            ONE
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn nothing_live_is_not_paired_and_a_named_target_is_still_not_live() {
        let root = scratch();
        assert_eq!(
            refusal(
                choose(enumerated(&root, &[]), None, None),
                "nothing is running"
            )
            .code(),
            kernel::DESKTOP_NOT_PAIRED
        );
        assert_eq!(
            refusal(
                choose(enumerated(&root, &[]), Some(&target(ONE)), None),
                "the named instance is not running either",
            )
            .code(),
            kernel::DESKTOP_TARGET_NOT_LIVE
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn an_unusable_descriptor_is_reported_and_never_dropped() {
        let root = scratch();
        fs::write(root.join("broken.json"), "{").expect("descriptor");
        let enumeration = enumerated(&root, &[]);
        assert!(enumeration.live.is_empty());
        assert_eq!(enumeration.unusable.len(), 1);
        assert!(enumeration.unusable[0].1.contains("valid JSON"));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn development_profiles_cover_local_linux_and_source_backed_canary() {
        assert!(PROFILES.contains(&("dev", "rw.datasolutions.desktop.local-dev")));
        assert!(PROFILES.contains(&("dev-canary", "rw.datasolutions.desktop.dev")));
    }
}
