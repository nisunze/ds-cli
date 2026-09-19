//! `ds design activities` — who has been designing, across every project the
//! signed-in account can reach.
//!
//! The question this answers is an uncomfortable one: *who did what in my
//! system, and when.* It is worth saying precisely what the answer is made
//! of, because a page about people that quietly overstates its evidence is
//! worse than no page at all.
//!
//! There is **no design event log**. ds-brain keeps an audit record for
//! membership, roles, lifecycle and assets; it keeps none for design work, and
//! `/api/v1/user-activity` is a stub. What a project's status rows carry is
//! the LATEST stamp per phase per transformer — who last sketched it, drafted
//! it, processed it, reported it. That is a photograph, not a film.
//!
//! So this domain takes photographs. [`sweep`] visits each project once,
//! folds its rows into a ledger through the shared kernel, and retains the
//! result on this machine. [`read`] folds every retained ledger — and the one
//! retained before it — into the cross-project answer, offline. Successive
//! captures are the entire history: nothing between two of them is visible,
//! and nothing is interpolated between them either.
//!
//! What this can never tell you, stated once here and repeated in the
//! command output and in `docs/reference/design.md`:
//!
//! - Nothing that happened between two captures.
//! - No device or installation. Status rows carry none. An intruder using the
//!   owner's credentials reads as the owner. Device evidence lives in Desktop
//!   installations and `ds auth device list`, not here.
//! - Nothing that never stamps a row — reads, downloads, exports.
//! - Only projects this account is a member of; ds-brain fences `/report` by
//!   membership, so a refusal is recorded as a refusal, never as an absence.
//!
//! **No schedule, ever.** Nothing here installs a timer, a unit file, a cron
//! entry or a background refresh, and nothing should. A sweep happens because
//! a person or an agent asked for one. Every full status scan holds the single
//! scan slot of a ds-brain instance, so captures are taken one after another,
//! with a pause between them, under a bound.

pub mod read;
pub mod sweep;

use std::io::Write as _;
use std::path::{Path, PathBuf};

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Refusal};
use serde_json::{Map, Value, json};

/// The lane whose captures a command reads or writes. Two lanes are two
/// stores: a canary capture never answers for stable.
pub const LANE_ARG: Arg = Arg::value(
    "lane",
    "<stable|canary>",
    "Deployment lane; stable is the default.",
)
.default("stable")
.choices(&["stable", "canary"]);

pub const STATE_DIR_ARG: Arg = Arg::value(
    "state-dir",
    "<absolute-path>",
    "Where captures are retained; defaults to the operator's own XDG state root.",
);

pub const TZ_OFFSET_ARG: Arg = Arg::value(
    "tz-offset-minutes",
    "<minutes>",
    "Minutes to add to UTC for the reading day, as the Dashboard buckets it (-840..840).",
)
.default("0");

pub const BUCKET_ARG: Arg = Arg::value(
    "bucket",
    "<active|archived|testing|all>",
    "Which lifecycle bucket of the project directory to consider.",
)
.default("active")
.choices(&["active", "archived", "testing", "all"]);

pub const PROJECT_ARG: Arg = Arg::repeated(
    "project",
    "<project-id>",
    "Restrict to this exact project; repeat for several. Omitted means the whole bucket.",
);

macro_rules! refusal {
    ($name:ident, $code:literal, $when:literal, $remedy:literal) => {
        pub const $name: Refusal = Refusal {
            code: $code,
            when: $when,
            remedy: $remedy,
        };
    };
}

refusal!(
    STORE_UNAVAILABLE,
    "store_unavailable",
    "the capture store cannot be created, listed or written on this machine",
    "check the state directory exists and is writable, or pass --state-dir"
);
refusal!(
    STORE_UNSAFE,
    "store_unsafe",
    "the state directory is not an absolute path, or the store holds an entry that is not a capture",
    "pass an absolute --state-dir, and remove anything that is not a <13-digit>.json capture"
);
refusal!(
    STORE_EMPTY,
    "store_empty",
    "no capture is retained for the lane, account and projects this request names",
    "run ds design activities sweep --yes first"
);
refusal!(
    SNAPSHOT_INVALID,
    "snapshot_invalid",
    "a retained capture is unreadable, or the kernel refuses it against its own envelope",
    "re-sweep the project; the refusal names the member at fault"
);
refusal!(
    SINCE_NO_BASELINE,
    "since_no_baseline",
    "--since is earlier than every retained capture for these projects; nothing to diff",
    "sweep now and re-read, or pass a --since at or after the earliest capture named here"
);
refusal!(
    UNKNOWN_ACTOR,
    "unknown_actor",
    "--user names an actor no retained capture holds",
    "name an actor this refusal lists, by account or short name, or read without --user"
);
refusal!(
    PROJECT_NOT_VISIBLE,
    "project_not_visible",
    "--project names an id the retained directory does not carry",
    "choose an exact id from ds auth project list"
);
refusal!(
    ACTIVITIES_REFUSED,
    "activities_refused",
    "the shared kernel refuses the ledger, plan, key, retention or fold request",
    "read the nested cause; it names the field the kernel refused"
);

/// The refusals every command in this family shares.
pub const STORE_REFUSALS: &[Refusal] = &[
    STORE_UNAVAILABLE,
    STORE_UNSAFE,
    SNAPSHOT_INVALID,
    ACTIVITIES_REFUSED,
];

/// Exactly what a capture cannot tell anyone, carried in every reply.
///
/// It travels with the data rather than beside it in a document, because the
/// reader who most needs it is the one who did not go looking for it.
pub const NOT_CLAIMED: &[&str] = &[
    "Nothing between two captures: only the latest stamp per phase per transformer exists on the server.",
    "No device or installation attribution: status rows carry none. Credentials used by someone else read as their owner. Device evidence lives in Desktop installations and `ds auth device list`.",
    "Nothing that never stamps a status row: reads, downloads and exports are invisible here.",
    MEMBERSHIP_BOUND,
];

/// The membership sentence alone. It is true, and it is not the bound of a
/// READ: a read is bounded by what a sweep captured on this machine, and
/// `read` prefixes this line with that number so "captured" can never pass
/// for "the estate".
pub const MEMBERSHIP_BOUND: &str = "Only projects this account is a member of; a project that refused the read is recorded as a refusal, not as an absence.";

fn kernel_refused(action: &str, detail: String) -> Failure {
    Failure::invalid(
        ACTIVITIES_REFUSED.code,
        format!("the kernel refused `{action}`: {detail}"),
    )
    .remedy(ACTIVITIES_REFUSED.remedy)
}

/// One `ds.design-activities/v1` call.
pub fn activities_call(action: &str, mut request: Map<String, Value>) -> Result<Value, Failure> {
    request.insert(
        "schema".into(),
        json!(ds_command_kernel::design_activities::SCHEMA),
    );
    request.insert("action".into(), json!(action));
    let bytes = serde_json::to_vec(&Value::Object(request))
        .map_err(|error| kernel_refused(action, error.to_string()))?;
    let reply = ds_command_kernel::design_activities::evaluate(&bytes)
        .map_err(|error| kernel_refused(action, error))?;
    serde_json::from_str(&reply).map_err(|error| kernel_refused(action, error.to_string()))
}

/// One `ds.design-activities-store/v1` call.
pub fn store_call(action: &str, mut request: Map<String, Value>) -> Result<Value, Failure> {
    request.insert(
        "schema".into(),
        json!(ds_command_kernel::design_activities_store::SCHEMA),
    );
    request.insert("action".into(), json!(action));
    let bytes = serde_json::to_vec(&Value::Object(request))
        .map_err(|error| kernel_refused(action, error.to_string()))?;
    let reply = ds_command_kernel::design_activities_store::evaluate(&bytes)
        .map_err(|error| kernel_refused(action, error))?;
    serde_json::from_str(&reply).map_err(|error| kernel_refused(action, error.to_string()))
}

// ── where a capture lives ───────────────────────────────────────────────

/// The project id the account's own directory file is keyed under.
///
/// The kernel owns the path of a capture, including the segment that hides
/// the account behind a digest. Rather than reimplement that derivation to
/// find the account's folder, this asks for the key of one reserved project
/// name and keeps the segments above the project. `directory` is a valid
/// project segment and is never a real ds_project, so nothing collides.
const DIRECTORY_SEGMENT: &str = "directory";

/// The retained directory of visible projects, so a later offline read can
/// name a project the account can no longer reach.
pub const DIRECTORY_SCHEMA: &str = "ds.design-activities-directory/v1";

/// The root under which the kernel's segments are joined.
///
/// `--state-dir` when the caller passed one, otherwise `$XDG_STATE_HOME/ds`,
/// otherwise `$HOME/.local/state/ds`. The credentials namespace
/// (`DS_CONFIG_HOME`) is deliberately not used: this is a machine cache of
/// governed reads, not a credential.
pub fn state_root(inputs: &ds_cli_contract::Inputs) -> Result<PathBuf, Failure> {
    if let Some(given) = inputs.value("state-dir").filter(|value| !value.is_empty()) {
        let path = PathBuf::from(given);
        if !path.is_absolute() {
            return Err(Failure::invalid(
                STORE_UNSAFE.code,
                "--state-dir must be one absolute path",
            )
            .remedy(STORE_UNSAFE.remedy));
        }
        return Ok(path);
    }
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .map(|home| home.join(".local").join("state"))
        })
        .ok_or_else(|| {
            Failure::failed(
                STORE_UNAVAILABLE.code,
                "no absolute XDG state root or home directory is available",
            )
            .remedy(STORE_UNAVAILABLE.remedy)
        })?;
    Ok(base.join("ds"))
}

/// The kernel's `key` for one capture, as a path under `root`.
pub struct CaptureKey {
    pub directory: PathBuf,
    pub file_name: String,
}

pub fn capture_key(
    root: &Path,
    principal_uid: &str,
    audience_sha256: &str,
    lane: &str,
    ds_project: &str,
    captured_at_ms: i64,
) -> Result<CaptureKey, Failure> {
    let mut request = Map::new();
    request.insert("principal_uid".into(), json!(principal_uid));
    request.insert("audience_sha256".into(), json!(audience_sha256));
    request.insert("lane".into(), json!(lane));
    request.insert("ds_project".into(), json!(ds_project));
    request.insert("captured_at_ms".into(), json!(captured_at_ms));
    let reply = store_call("key", request)?;
    let mut directory = root.to_path_buf();
    for segment in reply["segments"].as_array().into_iter().flatten() {
        let segment = segment.as_str().ok_or_else(|| {
            Failure::failed(
                ACTIVITIES_REFUSED.code,
                "the kernel's key reply is not a list of path segments",
            )
            .remedy(ACTIVITIES_REFUSED.remedy)
        })?;
        directory.push(segment);
    }
    let file_name = reply["file_name"]
        .as_str()
        .ok_or_else(|| {
            Failure::failed(
                ACTIVITIES_REFUSED.code,
                "the kernel's key reply carries no file name",
            )
            .remedy(ACTIVITIES_REFUSED.remedy)
        })?
        .to_owned();
    Ok(CaptureKey {
        directory,
        file_name,
    })
}

/// The account's own folder inside the store: every project it has captured,
/// plus the retained project directory.
pub fn account_root(
    root: &Path,
    principal_uid: &str,
    audience_sha256: &str,
    lane: &str,
) -> Result<PathBuf, Failure> {
    let key = capture_key(
        root,
        principal_uid,
        audience_sha256,
        lane,
        DIRECTORY_SEGMENT,
        1,
    )?;
    key.directory
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| {
            Failure::failed(
                ACTIVITIES_REFUSED.code,
                "the kernel's key reply has no account segment",
            )
            .remedy(ACTIVITIES_REFUSED.remedy)
        })
}

/// Every capture the store holds for this account, newest last per project.
///
/// A file whose name is not exactly thirteen digits and `.json` is not a
/// capture; the store refuses rather than guessing, because a file name is
/// the only thing that says when a capture was taken.
pub fn inventory(account: &Path) -> Result<Vec<(String, Vec<i64>)>, Failure> {
    if !account.exists() {
        return Ok(Vec::new());
    }
    let mut out: Vec<(String, Vec<i64>)> = Vec::new();
    let entries = std::fs::read_dir(account).map_err(|error| {
        Failure::failed(
            STORE_UNAVAILABLE.code,
            format!("the capture store cannot be listed: {error}"),
        )
        .remedy(STORE_UNAVAILABLE.remedy)
    })?;
    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let project = entry.file_name().to_string_lossy().into_owned();
        if project == DIRECTORY_SEGMENT {
            continue;
        }
        let mut captures = Vec::new();
        let files = std::fs::read_dir(entry.path()).map_err(|error| {
            Failure::failed(
                STORE_UNAVAILABLE.code,
                format!("the captures of `{project}` cannot be listed: {error}"),
            )
            .remedy(STORE_UNAVAILABLE.remedy)
        })?;
        for file in files.flatten() {
            let name = file.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || name.ends_with(".tmp") {
                continue;
            }
            let Some(stem) = name.strip_suffix(".json") else {
                continue;
            };
            let captured: i64 = stem.parse().map_err(|_| {
                Failure::invalid(
                    STORE_UNSAFE.code,
                    format!("`{project}/{name}` is not a capture: its name is not a capture time"),
                )
                .remedy(STORE_UNSAFE.remedy)
            })?;
            captures.push(captured);
        }
        captures.sort_unstable();
        if !captures.is_empty() {
            out.push((project, captures));
        }
    }
    out.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(out)
}

/// The project directory the last sweep retained beside its captures: every
/// project this account could see at that instant, whether or not the sweep
/// reached it.
pub struct RetainedDirectory {
    pub captured_at_ms: i64,
    /// `ds_project`, `status` and `display_name` per project, as the
    /// directory listed them.
    pub projects: Vec<Value>,
}

impl RetainedDirectory {
    pub fn carries(&self, ds_project: &str) -> bool {
        self.projects
            .iter()
            .any(|project| project["ds_project"].as_str() == Some(ds_project))
    }
}

/// Read the directory a sweep retained, or say why there is none to read.
///
/// The directory is what lets an offline read state its own coverage — how
/// many visible projects it holds a capture for — instead of letting the
/// count of captures pass for the size of the estate. A store written before
/// directories were retained has none, and that is reported as "unknown",
/// never as a number: `Err` carries the reason and the caller says it.
pub fn read_directory(account: &Path) -> Result<RetainedDirectory, String> {
    let path = account.join("directory.json");
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(
                "no project directory is retained on this machine; a sweep retains one".into(),
            );
        }
        Err(error) => {
            return Err(format!(
                "the retained project directory cannot be read: {error}"
            ));
        }
    };
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("the retained project directory is not JSON: {error}"))?;
    if value["schema"].as_str() != Some(DIRECTORY_SCHEMA) {
        return Err(format!(
            "the retained project directory is not {DIRECTORY_SCHEMA}"
        ));
    }
    let projects = value["projects"]
        .as_array()
        .cloned()
        .ok_or_else(|| "the retained project directory lists no projects".to_owned())?;
    Ok(RetainedDirectory {
        captured_at_ms: value["captured_at_ms"].as_i64().unwrap_or(0),
        projects,
    })
}

/// Read one retained capture and hand it to the kernel for admission.
///
/// The caller decides what a refusal means. For a single named capture it is
/// a refusal; for one capture out of thirty in a cross-project read it is a
/// row, because losing twenty-nine honest answers to one damaged file is
/// exactly the silence this domain exists to avoid.
pub fn read_capture(path: &Path) -> Result<Value, Failure> {
    let bytes = std::fs::read(path).map_err(|error| {
        Failure::failed(
            STORE_UNAVAILABLE.code,
            format!("a retained capture cannot be read: {error}"),
        )
        .remedy(STORE_UNAVAILABLE.remedy)
    })?;
    let snapshot: Value = serde_json::from_slice(&bytes).map_err(|error| {
        Failure::invalid(
            SNAPSHOT_INVALID.code,
            format!("a retained capture is not JSON: {error}"),
        )
        .remedy(SNAPSHOT_INVALID.remedy)
    })?;
    let mut request = Map::new();
    request.insert("snapshot".into(), snapshot.clone());
    store_call("validate", request).map_err(|failure| {
        Failure::invalid(
            SNAPSHOT_INVALID.code,
            format!(
                "a retained capture is not admissible: {}",
                failure.message()
            ),
        )
        .remedy(SNAPSHOT_INVALID.remedy)
    })?;
    Ok(snapshot)
}

/// Write `bytes` at `path` without ever leaving a half-written capture, and
/// without following anything already there.
///
/// `create_new` on the staging name refuses an existing file — a symlink
/// included — so the write lands where it was addressed or not at all.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), Failure> {
    let parent = path.parent().ok_or_else(|| {
        Failure::invalid(STORE_UNSAFE.code, "a capture path has no directory")
            .remedy(STORE_UNSAFE.remedy)
    })?;
    secure_dir(parent)?;
    let temp = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        std::process::id()
    ));
    let _ = std::fs::remove_file(&temp);
    let unavailable = |error: std::io::Error| {
        Failure::failed(
            STORE_UNAVAILABLE.code,
            format!("a capture could not be written: {error}"),
        )
        .remedy(STORE_UNAVAILABLE.remedy)
    };
    let mut file = create_new_private(&temp).map_err(unavailable)?;
    let written = file
        .write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(unavailable);
    drop(file);
    if let Err(failure) = written {
        let _ = std::fs::remove_file(&temp);
        return Err(failure);
    }
    if let Err(error) = std::fs::rename(&temp, path) {
        let _ = std::fs::remove_file(&temp);
        return Err(unavailable(error));
    }
    Ok(())
}

#[cfg(unix)]
fn create_new_private(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn create_new_private(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}

/// The store is the operator's own: owner-only, created as such.
pub fn secure_dir(path: &Path) -> Result<(), Failure> {
    std::fs::create_dir_all(path).map_err(|error| {
        Failure::failed(
            STORE_UNAVAILABLE.code,
            format!("the capture store directory could not be created: {error}"),
        )
        .remedy(STORE_UNAVAILABLE.remedy)
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

/// The not-claimed list as the replies carry it.
pub fn not_claimed() -> Value {
    json!(NOT_CLAIMED)
}

/// How a human line spells a capture time. Epoch millis stay in the JSON;
/// a terminal reader needs a date.
pub fn spell_ms(value: i64) -> String {
    if value <= 0 {
        return "—".into();
    }
    let seconds = value / 1000;
    let days = seconds.div_euclid(86_400);
    let time = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}Z",
        time / 3600,
        (time % 3600) / 60
    )
}

/// Howard Hinnant's civil-from-days, the same arithmetic the kernel's own
/// date helpers use. No dependency, no locale, no ambiguity.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_capture_time_is_spelled_as_a_date_a_human_can_read() {
        assert_eq!(spell_ms(1_758_000_000_000), "2025-09-16 05:20Z");
        assert_eq!(spell_ms(0), "—");
    }

    #[test]
    fn the_not_claimed_list_names_the_device_gap_out_loud() {
        let spelled = NOT_CLAIMED.join(" ");
        assert!(spelled.contains("No device or installation attribution"));
        assert!(spelled.contains("Nothing between two captures"));
    }

    #[test]
    fn a_relative_state_dir_is_refused_before_anything_is_touched() {
        let inputs = ds_cli_contract::args::parse(
            &read::COMMAND,
            &["--state-dir".into(), "relative/path".into()],
        )
        .expect("the flag parses");
        let failure = state_root(&inputs).expect_err("a relative state dir is refused");
        assert_eq!(failure.code(), STORE_UNSAFE.code);
    }

    #[test]
    fn an_explicit_absolute_state_dir_is_the_store_root_exactly() {
        let inputs = ds_cli_contract::args::parse(
            &read::COMMAND,
            &["--state-dir".into(), "/var/tmp/ds-activities-test".into()],
        )
        .expect("the flag parses");
        assert_eq!(
            state_root(&inputs).expect("an absolute root answers"),
            std::path::Path::new("/var/tmp/ds-activities-test")
        );
    }

    #[test]
    fn a_capture_never_names_the_account_in_its_path() {
        let key = capture_key(
            std::path::Path::new("/tmp/store"),
            "uid-of-a-real-person",
            &"a".repeat(64),
            "canary",
            "arjgpydw_nyam",
            1_758_000_000_000,
        )
        .expect("the kernel answers a well-formed key");
        let spelled = key.directory.to_string_lossy().into_owned();
        assert!(!spelled.contains("uid-of-a-real-person"), "{spelled}");
        assert!(spelled.contains("design-activities/canary/"), "{spelled}");
        assert_eq!(key.file_name, "1758000000000.json");
    }
}
